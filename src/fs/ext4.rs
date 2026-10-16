#![allow(dead_code, static_mut_refs)]
//! ext4 dosya sistemi biçimlendiricisi ve temel yapıları.
//!
//! Özellikler:
//! - ext4 uyumlu superblock (EXT4_MAGIC = 0xEF53)
//! - INCOMPAT_EXTENTS (0x0040), INCOMPAT_64BIT (0x0080), FEATURE_RO_COMPAT_DIR_NLINK vb.
//! - Inode boyutu: 256 bayt (Modern Linux ext4 varsayılanı)
//! - Kök dizin (Inode 2) için Extent Tree (ext4_extent_header: 0xF30A)
//! - Block Group Descriptors (64-byte descriptor desteği)

use crate::drivers::ata;
extern crate alloc;

pub const EXT4_SUPER_MAGIC: u16 = 0xEF53;
pub const EXT4_INODE_SIZE: u16 = 256;
pub const EXT4_EXT_MAGIC: u16 = 0xF30A;

// ext4 feature flags
pub const EXT4_FEATURE_COMPAT_RESIZE_INODE: u32 = 0x0010;
pub const EXT4_FEATURE_COMPAT_DIR_INDEX: u32 = 0x0020;

pub const EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
pub const EXT4_FEATURE_RO_COMPAT_LARGE_FILE: u32 = 0x0002;
pub const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
pub const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: u32 = 0x0040;

pub const EXT4_FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;
pub const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;

fn wle16(b: &mut [u8], o: usize, v: u16) { b[o..o+2].copy_from_slice(&v.to_le_bytes()); }
fn wle32(b: &mut [u8], o: usize, v: u32) { b[o..o+4].copy_from_slice(&v.to_le_bytes()); }
fn wle64(b: &mut [u8], o: usize, v: u64) { b[o..o+8].copy_from_slice(&v.to_le_bytes()); }

/// Sürücüdeki bir bölümü veya diski ext4 olarak biçimlendirir.
/// `part_lba`: Başlangıç LBA (512-bayt sektör)
/// `part_sectors`: Toplam sektör sayısı
pub fn format(drive: u8, part_lba: u32, part_sectors: u64) -> bool {
    if drive >= 4 || ata::device(drive).map(|d| d.kind != ata::DeviceKind::Ata).unwrap_or(true) {
        return false;
    }

    // 1024-bayt bloklar
    let total_blocks = (part_sectors / 2) as u32;
    if total_blocks < 256 {
        return false;
    }

    let block_size: u32 = 1024;
    let inodes_count = (total_blocks / 4).max(64);
    let inode_size: u32 = 256;

    // Tek blok grubu için hesaplama
    let itable_blocks = (inodes_count * inode_size + block_size - 1) / block_size;
    // Blok yerleşimi (1K blok):
    // Blok 0: Boot sektörü
    // Blok 1: Superblock
    // Blok 2: Block Group Descriptor Table (BGDT, 64 baytlık descriptor)
    // Blok 3: Block Bitmap
    // Blok 4: Inode Bitmap
    // Blok 5 .. 5 + itable_blocks - 1: Inode Tablosu
    // Blok 5 + itable_blocks: Kök dizin bloğu (root_blk)
    let root_blk = 5 + itable_blocks;
    let first_free_block = root_blk + 1;

    let bl = |b: u32| part_lba + b * 2;

    // 1. ext4 Superblock hazırlığı (Blok 1, 1024 bayt)
    let mut sb = [0u8; 1024];
    wle32(&mut sb, 0, inodes_count);                  // s_inodes_count
    wle32(&mut sb, 4, total_blocks);                  // s_blocks_count_lo
    wle32(&mut sb, 8, 0);                             // s_r_blocks_count_lo
    wle32(&mut sb, 12, total_blocks - first_free_block); // s_free_blocks_count_lo
    wle32(&mut sb, 16, inodes_count - 12);            // s_free_inodes_count
    wle32(&mut sb, 20, 1);                            // s_first_data_block (1 for 1KB blocks)
    wle32(&mut sb, 24, 0);                            // s_log_block_size (1024 << 0 = 1024)
    wle32(&mut sb, 28, 0);                            // s_log_cluster_size
    wle32(&mut sb, 32, 8192);                         // s_blocks_per_group
    wle32(&mut sb, 36, 8192);                         // s_clusters_per_group
    wle32(&mut sb, 40, inodes_count);                 // s_inodes_per_group
    wle32(&mut sb, 48, 1);                            // s_mnt_count
    wle32(&mut sb, 52, 0xFFFF);                       // s_max_mnt_count
    wle16(&mut sb, 56, EXT4_SUPER_MAGIC);             // s_magic: 0xEF53
    wle16(&mut sb, 58, 1);                            // s_state: Cleanly unmounted
    wle16(&mut sb, 60, 1);                            // s_errors: Continue
    wle16(&mut sb, 62, 0);                            // s_minor_rev_level
    wle32(&mut sb, 64, 0x64000000);                   // s_lastcheck
    wle32(&mut sb, 68, 0);                            // s_checkinterval
    wle32(&mut sb, 72, 0);                            // s_creator_os: Linux
    wle32(&mut sb, 76, 1);                            // s_rev_level: Dynamic (rev 1)
    wle16(&mut sb, 84, 11);                           // s_first_ino (Reserved inodes = 10, first = 11)
    wle16(&mut sb, 88, EXT4_INODE_SIZE);              // s_inode_size: 256
    wle16(&mut sb, 90, 0);                            // s_block_group_nr: 0

    // ext4 Feature Flag'leri:
    let feature_compat = EXT4_FEATURE_COMPAT_DIR_INDEX;
    let feature_incompat = EXT4_FEATURE_INCOMPAT_FILETYPE
        | EXT4_FEATURE_INCOMPAT_EXTENTS
        | EXT4_FEATURE_INCOMPAT_64BIT;
    let feature_ro_compat = EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
        | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
        | EXT4_FEATURE_RO_COMPAT_DIR_NLINK
        | EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE;

    wle32(&mut sb, 92, feature_compat);               // s_feature_compat
    wle32(&mut sb, 96, feature_incompat);             // s_feature_incompat
    wle32(&mut sb, 100, feature_ro_compat);           // s_feature_ro_compat

    // UUID ve Volume Label
    sb[104..120].copy_from_slice(&[0x4d, 0x49, 0x4e, 0x4f, 0x53, 0x2d, 0x45, 0x58, 0x54, 0x34, 0x2d, 0x46, 0x53, 0x30, 0x31, 0x21]); // "MINOS-EXT4-FS01!"
    sb[120..136].copy_from_slice(b"minos-ext4      "); // Volume name
    wle16(&mut sb, 254, 64);                          // s_desc_size: 64 bayt (ext4 64-bit mode)

    // Superblock diske yaz (Blok 1 = sektör part_lba + 2 ve + 3)
    let mut s0 = [0u8; 512]; s0.copy_from_slice(&sb[..512]);
    let mut s1 = [0u8; 512]; s1.copy_from_slice(&sb[512..]);
    if !ata::write_sector(drive, bl(1), &s0) || !ata::write_sector(drive, bl(1) + 1, &s1) {
        return false;
    }

    // 2. Block Group Descriptor (Blok 2, 64 bayt descriptor)
    let mut bgd = [0u8; 1024];
    wle32(&mut bgd, 0, 3);                            // bg_block_bitmap_lo (Blok 3)
    wle32(&mut bgd, 4, 4);                            // bg_inode_bitmap_lo (Blok 4)
    wle32(&mut bgd, 8, 5);                            // bg_inode_table_lo (Blok 5)
    wle16(&mut bgd, 12, (8192 - (first_free_block - 1)) as u16); // bg_free_blocks_count_lo
    wle16(&mut bgd, 14, (inodes_count - 12) as u16);  // bg_free_inodes_count_lo
    wle16(&mut bgd, 16, 2);                           // bg_used_dirs_count_lo (kök dizin)
    wle16(&mut bgd, 18, 0);                           // bg_flags
    wle32(&mut bgd, 20, 0);                           // bg_exclude_bitmap_lo
    wle16(&mut bgd, 24, 0);                           // bg_block_bitmap_csum_lo
    wle16(&mut bgd, 26, 0);                           // bg_inode_bitmap_csum_lo
    wle16(&mut bgd, 28, 0);                           // bg_itable_unused_lo

    let mut g0 = [0u8; 512]; g0.copy_from_slice(&bgd[..512]);
    let mut g1 = [0u8; 512]; g1.copy_from_slice(&bgd[512..]);
    if !ata::write_sector(drive, bl(2), &g0) || !ata::write_sector(drive, bl(2) + 1, &g1) {
        return false;
    }

    // 3. Block Bitmap (Blok 3)
    let mut b_bmp = [0u8; 512];
    for b in 0..first_free_block {
        b_bmp[(b / 8) as usize] |= 1 << (b % 8);
    }
    if !ata::write_sector(drive, bl(3), &b_bmp) || !ata::write_sector(drive, bl(3) + 1, &b_bmp) {
        return false;
    }

    // 4. Inode Bitmap (Blok 4)
    let mut i_bmp = [0u8; 512];
    for i in 0..12 {
        i_bmp[(i / 8) as usize] |= 1 << (i % 8);
    }
    if !ata::write_sector(drive, bl(4), &i_bmp) || !ata::write_sector(drive, bl(4) + 1, &i_bmp) {
        return false;
    }

    // 5. Inode Tablosunu sıfırla (Blok 5 .. 5 + itable_blocks)
    let zero_sec = [0u8; 512];
    for b in 5..5 + itable_blocks {
        if !ata::write_sector(drive, bl(b), &zero_sec) || !ata::write_sector(drive, bl(b) + 1, &zero_sec) {
            return false;
        }
    }

    // 6. Kök Inode (#2, 256 bayt): Inode 1 rezerve, Inode 2 kök dizin
    // Inode 2 konumu: Blok 5, ofset 1 * 256 = 256 bayt (Sektör 0'ın 256..512 aralığı)
    let mut root_ino = [0u8; 256];
    wle16(&mut root_ino, 0, 0x41ED);                   // i_mode: Dizin (0o40000) + rwxr-xr-x (0o755)
    wle16(&mut root_ino, 2, 0);                        // i_uid: root (0)
    wle32(&mut root_ino, 4, 1024);                     // i_size_lo: 1024 bayt
    wle32(&mut root_ino, 8, 0x64000000);               // i_atime
    wle32(&mut root_ino, 12, 0x64000000);              // i_ctime
    wle32(&mut root_ino, 16, 0x64000000);              // i_mtime
    wle32(&mut root_ino, 20, 0);                       // i_dtime
    wle16(&mut root_ino, 24, 0);                       // i_gid: root (0)
    wle16(&mut root_ino, 26, 3);                       // i_links_count: 3 (. , .. ve üst dizin)
    wle32(&mut root_ino, 28, 2);                       // i_blocks_lo: 2 sektör (1024B)
    wle32(&mut root_ino, 32, 0x00080000);              // i_flags: EXT4_EXTENTS_FL (0x80000)

    // ext4 Extent Tree Yapısı (i_block alanı, ofset 40..100)
    // ext4_extent_header (12 bayt):
    wle16(&mut root_ino, 40, EXT4_EXT_MAGIC);          // eh_magic: 0xF30A
    wle16(&mut root_ino, 42, 1);                       // eh_entries: 1
    wle16(&mut root_ino, 44, 4);                       // eh_max: 4 extents
    wle16(&mut root_ino, 46, 0);                       // eh_depth: 0 (leaf node)
    wle32(&mut root_ino, 48, 0);                       // eh_generation: 0

    // ext4_extent (12 bayt, ofset 52):
    wle32(&mut root_ino, 52, 0);                       // ee_block: dosya içi blok 0
    wle16(&mut root_ino, 56, 1);                       // ee_len: 1 blok uzunluğunda
    wle16(&mut root_ino, 58, 0);                       // ee_start_hi: 0
    wle32(&mut root_ino, 60, root_blk);                // ee_start_lo: diskteki root_blk

    wle16(&mut root_ino, 128, 32);                     // i_extra_isize: 32 bayt

    let mut ib0 = [0u8; 512];
    ib0[256..512].copy_from_slice(&root_ino);
    if !ata::write_sector(drive, bl(5), &ib0) {
        return false;
    }

    // 7. Kök Dizin Bloğu (root_blk, 1024 bayt): '.' ve '..' girdileri
    let mut dir_block = [0u8; 1024];
    // Girdi 1: '.'
    wle32(&mut dir_block, 0, 2);                       // inode = 2
    wle16(&mut dir_block, 4, 12);                      // rec_len = 12
    dir_block[6] = 1;                                  // name_len = 1
    dir_block[7] = 2;                                  // file_type = 2 (EXT4_FT_DIR)
    dir_block[8] = b'.';

    // Girdi 2: '..' (bloğun kalanını kaplar: 1024 - 12 = 1012 bayt)
    wle32(&mut dir_block, 12, 2);                      // inode = 2
    wle16(&mut dir_block, 16, (1024 - 12) as u16);     // rec_len = 1012
    dir_block[18] = 2;                                 // name_len = 2
    dir_block[19] = 2;                                 // file_type = 2 (EXT4_FT_DIR)
    dir_block[20] = b'.';
    dir_block[21] = b'.';

    let mut d0 = [0u8; 512]; d0.copy_from_slice(&dir_block[..512]);
    let mut d1 = [0u8; 512]; d1.copy_from_slice(&dir_block[512..]);
    if !ata::write_sector(drive, bl(root_blk), &d0) || !ata::write_sector(drive, bl(root_blk) + 1, &d1) {
        return false;
    }

    ata::flush_cache(drive);
    true
}

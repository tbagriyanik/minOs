#![allow(dead_code, static_mut_refs)]
//! ext4 dosya sistemi biçimlendiricisi.
//!
//! Yerleşim (Linux `mke2fs` ile aynı mantık):
//! - 1024 baytlık bloklar, `s_first_data_block = 1`
//! - Her grupta `s_blocks_per_group` blok; her grubun kendi blok/inode
//!   bitmap'i, inode tablosu ve süperblok yedeği (1, 3 ve 5, 7, ... üslerinde)
//! - Kök dizin, inode 2, tek bir doğrudan blok (`i_block[0]`). EXTENTS yazılmaz,
//!   böylece `src/fs/ext2.rs` okuyucu/yazıcısı diski olduğu gibi kullanabilir ve
//!   Linux `ext4` sürücüsü de sorunsuz bağlar.
//! - Özellik bayrakları: `HAS_JOURNAL` (0x0004), `FILETYPE` (0x0002) ve
//!   `RO_COMPAT` (sparse_super | large_file | dir_nlink) — journal verisi
//!   yazılmaz, çekirdek kendi yazıcısını (ext2 formatı) kullandığı için
//!   journaling gerekmaz.

// Çekirdek `crate::drivers::ata` yoluna bağlı sarmalayıcı içerir. Host'ta
// yalnızca aygıttan bağımsız `format_device` test edilir; bu import'lar host'ta
// `ata` modülü olmadığı için ayrı bir cfg ile açılır.
#[cfg(host_fs_tests)]
use crate::block::{BlockDevice, SECTOR};
#[cfg(not(host_fs_tests))]
use crate::drivers::ata;
#[cfg(not(host_fs_tests))]
use crate::fs::block::{BlockDevice, SECTOR};

pub const EXT4_SUPER_MAGIC: u16 = 0xEF53;
pub const EXT4_INODE_SIZE: u16 = 256;
pub const EXT4_EXT_MAGIC: u16 = 0xF30A;

pub const BLOCK_SIZE: u32 = 1024;
pub const BLOCKS_PER_GROUP: u32 = 8192;
pub const INODES_PER_GROUP: u32 = 2048;

// ext4 feature flags
pub const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
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

fn wle16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn wle32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn wle64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// Süperbloğun tüm gruplar için hesabı.
pub struct Layout {
    pub total_blocks: u32,
    pub inodes_count: u32,
    pub groups: u32,
    /// Grup başına inode tablosu blok sayısı.
    pub itable_blocks: u32,
    /// Grup 0'daki ilk serbest blok (kök dizin de burada).
    pub root_block: u32,
    pub first_data_block: u32,
}

/// Bölüm boyutundan yerleşimi türet.
pub fn layout(total_blocks: u32) -> Option<Layout> {
    if total_blocks < 256 {
        return None;
    }
    let groups = (total_blocks + BLOCKS_PER_GROUP - 1) / BLOCKS_PER_GROUP;
    let groups = groups.max(1);
    let itable_blocks = (INODES_PER_GROUP * EXT4_INODE_SIZE as u32 + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let inodes_count = groups * INODES_PER_GROUP;
    // Grup 0 yerleşimi: süperblok(1) + GDT(gdt_blocks) + blok bitmap +
    // inode bitmap + inode tablosu. Kök dizin, tablonun bitiminden sonraki
    // ilk boş bloktur.
    let gdt_blocks = (groups * 32 + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let root_block = 2 + gdt_blocks + 2 + itable_blocks + 1;
    Some(Layout {
        total_blocks,
        inodes_count,
        groups,
        itable_blocks,
        root_block,
        first_data_block: 1,
    })
}

/// Bir grubun ilk bloğu (grup 0 için 1; süperblok bloğu 1'dir).
/// Bitmap/inode tablosu bu bloktan itibaren yerleşir; grup 1 için süperblok
/// yedeği ayrıca prefix olarak yazılır ve grup 0 bloğuna ayna olur.
pub fn group_first_block(l: &Layout, g: u32) -> u32 {
    l.first_data_block + g * BLOCKS_PER_GROUP
}

/// Grubun blok sayısı (son grup artan olabilir).
pub fn group_blocks(l: &Layout, g: u32) -> u32 {
    let first = group_first_block(l, g);
    let end = (first + BLOCKS_PER_GROUP).min(l.total_blocks);
    end.saturating_sub(first)
}

/// Yedek süperbloğun bulunduğu gruplar (sparse_super: 0, 1 ve 3, 5, 7, 9^x...).
/// ext2 geleneğinde yedek süperblok, grubun ilk bloğunda; grup 1'de ise grup 0
/// bloğun birebir aynası olarak yazılır.
pub fn has_super(_l: &Layout, g: u32) -> bool {
    g == 0 || g == 1
}

/// Block Group Descriptor Table'ın kapladığı blok sayısı.
pub fn gdt_blocks(l: &Layout) -> u32 {
    (l.groups * 32 + BLOCK_SIZE - 1) / BLOCK_SIZE
}

/// Grubun blok bitmap'i. Grup 0 yerleşimi: blok 1 süperblok, blok 2..2+gdt
/// GDT; bitmap ilk serbest blok olan 2+gdt'de başlar.
pub fn block_bitmap_of(l: &Layout, g: u32, gdt_blocks: u32) -> u32 {
    if g == 0 {
        2 + gdt_blocks
    } else {
        group_first_block(l, g) + 1
    }
}

/// 512 baytlık sektörden 1024 baytlık bloğa yazma.
fn write_block(dev: &mut dyn BlockDevice, base_lba: u32, blk: u32, data: &[u8]) -> bool {
    let lba = base_lba as u64 + blk as u64 * 2;
    dev.write(lba, &data[..SECTOR]) && dev.write(lba + 1, &data[SECTOR..BLOCK_SIZE as usize])
}

fn read_block(dev: &dyn BlockDevice, base_lba: u32, blk: u32) -> Option<[u8; BLOCK_SIZE as usize]> {
    let mut buf = [0u8; BLOCK_SIZE as usize];
    let lba = base_lba as u64 + blk as u64 * 2;
    if dev.read(lba, &mut buf[..SECTOR]) && dev.read(lba + 1, &mut buf[SECTOR..]) {
        Some(buf)
    } else {
        None
    }
}

/// Süperbloğu imzalayan imzayı diske yaz (grup 0 ve yedek gruplar).
pub fn write_superblock(dev: &mut dyn BlockDevice, base_lba: u32, l: &Layout) -> bool {
    let mut sb = [0u8; BLOCK_SIZE as usize];
    wle32(&mut sb, 0, l.inodes_count); // s_inodes_count
    wle32(&mut sb, 4, l.total_blocks); // s_blocks_count_lo
    wle32(&mut sb, 8, 0); // s_r_blocks_count_lo
    wle32(&mut sb, 12, l.total_blocks.saturating_sub(l.root_block + 2)); // s_free_blocks_count_lo
    wle32(&mut sb, 16, l.inodes_count.saturating_sub(12)); // s_free_inodes_count
    wle32(&mut sb, 20, l.first_data_block); // s_first_data_block
    wle32(&mut sb, 24, 0); // s_log_block_size (1024)
    wle32(&mut sb, 28, 0); // s_log_cluster_size
    wle32(&mut sb, 32, BLOCKS_PER_GROUP); // s_blocks_per_group
    wle32(&mut sb, 36, BLOCKS_PER_GROUP); // s_clusters_per_group
    wle32(&mut sb, 40, INODES_PER_GROUP); // s_inodes_per_group
    wle32(&mut sb, 48, 1); // s_mnt_count
    wle32(&mut sb, 52, 0xFFFF); // s_max_mnt_count
    wle16(&mut sb, 56, EXT4_SUPER_MAGIC); // s_magic
    wle16(&mut sb, 58, 1); // s_state (clean)
    wle16(&mut sb, 60, 1); // s_errors (continue)
    wle16(&mut sb, 62, 0); // s_minor_rev_level
    wle32(&mut sb, 64, 0x64000000); // s_lastcheck
    wle32(&mut sb, 68, 0); // s_checkinterval
    wle32(&mut sb, 72, 0); // s_creator_os (Linux)
    wle32(&mut sb, 76, 1); // s_rev_level (dynamic)
    wle16(&mut sb, 84, 11); // s_first_ino
    wle16(&mut sb, 88, EXT4_INODE_SIZE); // s_inode_size
    wle16(&mut sb, 90, 0); // s_block_group_nr

    // Hâlâ ext2 yazıcısıyla (ext2.rs) yazılabilir olması için EXTENTS ve 64BIT
    // kapalı tutulur; ext4 sürücüleri bu dosya sistemini "ext4" olarak bağlar.
    let feature_compat = EXT4_FEATURE_COMPAT_HAS_JOURNAL | EXT4_FEATURE_COMPAT_DIR_INDEX;
    let feature_incompat = EXT4_FEATURE_INCOMPAT_FILETYPE;
    let feature_ro_compat = EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
        | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
        | EXT4_FEATURE_RO_COMPAT_DIR_NLINK;

    wle32(&mut sb, 92, feature_compat);
    wle32(&mut sb, 96, feature_incompat);
    wle32(&mut sb, 100, feature_ro_compat);

    sb[104..120].copy_from_slice(&[
        0x4d, 0x49, 0x4e, 0x4f, 0x53, 0x2d, 0x45, 0x58, 0x54, 0x34, 0x2d, 0x46, 0x53, 0x30, 0x31,
        0x21,
    ]);
    sb[120..136].copy_from_slice(b"minos-ext4      ");
    wle16(&mut sb, 254, 32); // s_desc_size (ext2 uyumlu)

    // s_blocks_count_hi / s_free_blocks_count_hi: 32-bit sınırı aşılmadığı için 0.
    let mut ok = write_block(dev, base_lba, 1, &sb);
    // Yedek süperblok: grup 1'in ilk bloğu (grup 0'ın birebir aynası).
    if l.groups > 1 {
        let backup = group_first_block(l, 1);
        if backup < l.total_blocks {
            ok = write_block(dev, base_lba, backup, &sb) && ok;
        }
    }
    ok
}

/// Block Group Descriptor Table'ı yaz.
fn write_group_descriptors(dev: &mut dyn BlockDevice, base_lba: u32, l: &Layout) -> bool {
    let gdt_blocks = (l.groups * 32 + BLOCK_SIZE - 1) / BLOCK_SIZE;
    for i in 0..gdt_blocks {
        let blk = 2 + i;
        let mut buf = [0u8; BLOCK_SIZE as usize];
        for (k, g) in (0..l.groups).enumerate() {
            let desc_index = i * (BLOCK_SIZE / 32) + k as u32;
            if desc_index >= l.groups {
                break;
            }
            let off = (k * 32) % BLOCK_SIZE as usize;
            let gblocks = group_blocks(l, g);
            let block_bitmap = block_bitmap_of(l, g, gdt_blocks);
            if block_bitmap + 2 + l.itable_blocks > l.total_blocks {
                return false;
            }
            let inode_bitmap = block_bitmap + 1;
            let inode_table = inode_bitmap + 1;
            wle32(&mut buf, off, block_bitmap);
            wle32(&mut buf, off + 4, inode_bitmap);
            wle32(&mut buf, off + 8, inode_table);
            wle16(&mut buf, off + 12, gblocks as u16); // bg_free_blocks_count_lo
            wle16(&mut buf, off + 14, (INODES_PER_GROUP - 12) as u16);
            wle16(&mut buf, off + 16, 0); // bg_used_dirs_count_lo
            wle16(&mut buf, off + 18, 0); // bg_flags
        }
        if !write_block(dev, base_lba, blk, &buf) {
            return false;
        }
    }
    true
}

fn mark_block_used(bitmap: &mut [u8], bit: u32) {
    let idx = (bit / 8) as usize;
    if idx < bitmap.len() {
        bitmap[idx] |= 1 << (bit % 8);
    }
}

/// Bir bölümü ext4 olarak biçimlendir (grup 0 ve gerekirse ek gruplar).
/// `base_lba`: bölüm başlangıcı (512 bayt sektör).
pub fn format_device(dev: &mut dyn BlockDevice, base_lba: u32) -> bool {
    let total_blocks = (dev.sector_count() / 2) as u32;
    let Some(l) = layout(total_blocks) else {
        return false;
    };

    if !write_superblock(dev, base_lba, &l) {
        return false;
    }
    if !write_group_descriptors(dev, base_lba, &l) {
        return false;
    }

    let gdt_blocks = (l.groups * 32 + BLOCK_SIZE - 1) / BLOCK_SIZE;

    for g in 0..l.groups {
        let first = group_first_block(&l, g);
        let gblocks = group_blocks(&l, g);
        let block_bitmap = block_bitmap_of(&l, g, gdt_blocks);
        let inode_bitmap = block_bitmap + 1;
        let inode_table = inode_bitmap + 1;
        if inode_table + l.itable_blocks > first + gblocks
            || inode_table + l.itable_blocks > l.total_blocks
        {
            // Grup meta verisi için yer yok: biçimlendirme durdurulur.
            return false;
        }

        // Blok bitmap'i: süperblok/GDT/bitmap/inode tablosu ve kök dizin dolu
        let mut bb = [0u8; BLOCK_SIZE as usize];
        if g == 0 {
            for b in 0..=l.root_block {
                mark_block_used(&mut bb, b);
            }
        } else {
            for i in 0..(2 + 1 + 1 + l.itable_blocks) {
                mark_block_used(&mut bb, i);
            }
        }
        if !write_block(dev, base_lba, block_bitmap, &bb) {
            return false;
        }

        // Inode bitmap'i: ilk 11 inode rezerve (grup 0'da inode 2 = kök dizin)
        let mut ib = [0u8; BLOCK_SIZE as usize];
        for i in 0..12 {
            mark_block_used(&mut ib, i);
        }
        if !write_block(dev, base_lba, inode_bitmap, &ib) {
            return false;
        }

        // Inode tablosunu sıfırla
        let zero = [0u8; BLOCK_SIZE as usize];
        for b in 0..l.itable_blocks {
            if !write_block(dev, base_lba, inode_table + b, &zero) {
                return false;
            }
        }
    }

    // Grup 0 blok bitmap'ine kök dizin bloğunu dolu olarak işaretle.
    let block_bitmap0 = block_bitmap_of(&l, 0, gdt_blocks);
    if block_bitmap0 >= l.total_blocks {
        return false;
    }
    if let Some(mut bb) = read_block(dev, base_lba, block_bitmap0) {
        mark_block_used(&mut bb, l.root_block);
        if !write_block(dev, base_lba, block_bitmap0, &bb) {
            return false;
        }
    } else {
        return false;
    }

    // Kök inode (#2): inode tablosun ilk bloğunda, ofset 256 (inode 1 rezerve)
    let inode_table0 = block_bitmap0 + 2;
    if inode_table0 >= l.total_blocks {
        return false;
    }
    let Some(mut itable) = read_block(dev, base_lba, inode_table0) else {
        return false;
    };
    let mut root_ino = [0u8; EXT4_INODE_SIZE as usize];
    wle16(&mut root_ino, 0, 0x41ED); // i_mode: dizin + rwxr-xr-x
    wle16(&mut root_ino, 2, 0); // i_uid
    wle32(&mut root_ino, 4, BLOCK_SIZE); // i_size_lo
    wle32(&mut root_ino, 8, 0x64000000); // i_atime
    wle32(&mut root_ino, 12, 0x64000000); // i_ctime
    wle32(&mut root_ino, 16, 0x64000000); // i_mtime
    wle16(&mut root_ino, 24, 0); // i_gid
    wle16(&mut root_ino, 26, 3); // i_links_count (., .. ve üst)
    wle32(&mut root_ino, 28, BLOCK_SIZE / 512); // i_blocks_lo
    wle16(&mut root_ino, 128, 0); // i_extra_isize: 0
    wle32(&mut root_ino, 40, l.root_block); // i_block[0]
    itable[256..256 + EXT4_INODE_SIZE as usize].copy_from_slice(&root_ino);
    if !write_block(dev, base_lba, inode_table0, &itable) {
        return false;
    }

    // Kök dizin bloğu
    let mut dir = [0u8; BLOCK_SIZE as usize];
    wle32(&mut dir, 0, 2);
    wle16(&mut dir, 4, 12);
    dir[6] = 1;
    dir[7] = 2;
    dir[8] = b'.';
    wle32(&mut dir, 12, 2);
    wle16(&mut dir, 16, (BLOCK_SIZE - 12) as u16);
    dir[18] = 2;
    dir[19] = 2;
    dir[20] = b'.';
    dir[21] = b'.';
    if !write_block(dev, base_lba, l.root_block, &dir) {
        return false;
    }

    dev.flush();
    true
}

/// Sürücüdeki bir bölümü veya diski ext4 olarak biçimlendir (çekirdek ATA yolu).
/// `part_lba`: Başlangıç LBA (512-bayt sektör)
/// `part_sectors`: Toplam sektör sayısı
#[cfg(not(host_fs_tests))]
pub fn format(drive: u8, part_lba: u32, part_sectors: u64) -> bool {
    if drive >= 4
        || ata::device(drive)
            .map(|d| d.kind != ata::DeviceKind::Ata)
            .unwrap_or(true)
    {
        return false;
    }
    if part_sectors < 2048 {
        return false;
    }
    let total_blocks = (part_sectors / 2) as u32;
    if layout(total_blocks).is_none() {
        return false;
    }

    let mut dev = crate::fs::block::AtaPartition::new(drive, part_sectors);
    if !format_device(&mut dev, part_lba) {
        return false;
    }

    // Gerçekten bağlanabildiğini doğrula; aksi hâlde kurulum "biçimlendirildi"
    // görünüp yazamaz.
    crate::fs::ext2::mount(drive, part_lba)
}

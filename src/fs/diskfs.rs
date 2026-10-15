#![allow(dead_code, static_mut_refs)]

//! Disk tabanlı dosya sistemi okuyucuları: **FAT32** (USB bellek) ve
//! **ISO9660** (CD-ROM). Her ikisi de ATA/ATAPI PIO katmanı üzerinden çalışır.
//!
//! Amaç: `run.ps1 -UsbDisks` ile QEMU'ya bağlanan USB aygıtın içeriğini Dosya
//! Yöneticisi listesinde göstermek ve tarayıcıda `file://` adresleriyle yerel
//! sayfa açabilmek.

use crate::drivers::ata;

/// Listelenen azami girdi sayısı (giriş başına sabit alanlar).
pub const MAX_ENTRIES: usize = 64;
/// Dosya adı için azami uzunluk.
pub const NAME_MAX: usize = 32;
/// Tarayıcının yerel dosyadan göstermek için kesip alabileceği azami içerik.
pub const CONTENT_MAX: usize = 2048;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    None,
    Fat32,
    Iso9660,
}

impl FsKind {
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (FsKind::Fat32, true) => "FAT32",
            (FsKind::Fat32, false) => "FAT32",
            (FsKind::Iso9660, true) => "ISO9660",
            (FsKind::Iso9660, false) => "ISO9660",
            (FsKind::None, true) => "No medium",
            (FsKind::None, false) => "Ortam yok",
        }
    }
}

#[derive(Clone, Copy)]
pub struct DiskEntry {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub size: usize,
    pub is_dir: bool,
}

impl DiskEntry {
    const fn empty() -> Self {
        Self { name: [0; NAME_MAX], name_len: 0, size: 0, is_dir: false }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// USB (master ATA) aygıtı için önbellek penceresi: her sektör okuması PIO ile
/// yapıldığı için yalnızca çok küçük bir pencere tutulur.

static mut KIND0: FsKind = FsKind::None;
static mut KIND1: FsKind = FsKind::None;
static mut READY: bool = false;

static mut ENTRIES: [DiskEntry; MAX_ENTRIES] = [DiskEntry::empty(); MAX_ENTRIES];
static mut ENTRY_COUNT: usize = 0;
static mut LISTED: bool = false;

static mut CONTENT: [u8; CONTENT_MAX] = [0; CONTENT_MAX];
static mut CONTENT_LEN: usize = 0;
static mut CONTENT_NAME: [u8; NAME_MAX] = [0; NAME_MAX];
static mut CONTENT_NAME_LEN: usize = 0;

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

fn serial(s: &str) {
    crate::serial::print(s);
}

fn read_sector_cached(drive: u8, lba: u32, out: &mut [u8; 512]) -> bool {
    ata::read_sector(drive, lba, out)
}

// ---------------------------------------------------------------------------
// FAT32
// ---------------------------------------------------------------------------

const FAT_ATTR_VOLUME_ID: u8 = 0x08;
const FAT_ATTR_DIRECTORY: u8 = 0x10;
const FAT_ATTR_LFN: u8 = 0x0F;

struct FatState {
    drive: u8,
    partition_lba: u32,
    fat_lba: u32,
    data_lba: u32,
    root_cluster: u32,
    sectors_per_cluster: u32,
    bytes_per_sector: u32,
    fat_size: u32,
    num_fats: u32,
}

static mut FAT: FatState = FatState {
    drive: 0,
    partition_lba: 0,
    fat_lba: 0,
    data_lba: 0,
    root_cluster: 2,
    sectors_per_cluster: 1,
    bytes_per_sector: 512,
    fat_size: 0,
    num_fats: 0,
};

fn le16(b: &[u8], off: usize) -> u16 {
    (b[off] as u16) | ((b[off + 1] as u16) << 8)
}

fn le32(b: &[u8], off: usize) -> u32 {
    (b[off] as u32) | ((b[off + 1] as u32) << 8) | ((b[off + 2] as u32) << 16) | ((b[off + 3] as u32) << 24)
}

/// MBR/GPT bölüm tablosundan ilk FAT32 bölümünün başlangıç LBA'sını bulur.
fn find_partition_lba(drive: u8) -> u32 {
    let mut mbr = [0u8; 512];
    if !read_sector_cached(drive, 0, &mut mbr) {
        return 0;
    }
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return 0;
    }
    // 4 birincil bölüm girdisi (0x1BE'den itibaren 16 bayt)
    let mut best = 0u32;
    for i in 0..4 {
        let off = 0x1BE + i * 16;
        let part_type = mbr[off + 4];
        let start = le32(&mbr, off + 8);
        if start == 0 {
            continue;
        }
        // 0x01/0x04/0x06/0x0B/0x0C = FAT; 0xEE = GPT koruyucu
        let is_fat = matches!(part_type, 0x01 | 0x04 | 0x06 | 0x0B | 0x0C | 0x0E);
        if is_fat {
            return start;
        }
        if best == 0 {
            best = start;
        }
    }
    best
}

/// FAT32 BPB'sini okuyup durumu doldurur.
fn fat_mount(drive: u8, partition_lba: u32) -> bool {
    let mut bpb = [0u8; 512];
    if !read_sector_cached(drive, partition_lba, &mut bpb) {
        return false;
    }
    if bpb[510] != 0x55 || bpb[511] != 0xAA {
        return false;
    }
    let bps = le16(&bpb, 11) as u32;
    if bps != 512 {
        return false;
    }
    let spc = bpb[13] as u32;
    if spc == 0 {
        return false;
    }
    let reserved = le16(&bpb, 14) as u32;
    let num_fats = bpb[16] as u32;
    let fat_size_16 = le16(&bpb, 22) as u32;
    let fat_size_32 = le32(&bpb, 36);
    let fat_size = if fat_size_16 != 0 { fat_size_16 } else { fat_size_32 };
    if fat_size == 0 || num_fats == 0 {
        return false;
    }
    let root_cluster = le32(&bpb, 44);
    let fat_lba = partition_lba + reserved;
    let data_lba = fat_lba + num_fats * fat_size;

    unsafe {
        FAT.drive = drive;
        FAT.partition_lba = partition_lba;
        FAT.fat_lba = fat_lba;
        FAT.data_lba = data_lba;
        FAT.root_cluster = if root_cluster == 0 { 2 } else { root_cluster };
        FAT.sectors_per_cluster = spc;
        FAT.bytes_per_sector = bps;
        FAT.fat_size = fat_size;
        FAT.num_fats = num_fats;
    }

    serial("[FAT] BPB okundu. kok kume: ");
    print_num(root_cluster as u64);
    serial("  kume sektoru: ");
    print_num(spc as u64);
    serial("\n");
    true
}

fn print_num(mut v: u64) {
    if v == 0 {
        serial("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        serial(s);
    }
}

/// FAT tablosundan `cluster` için sonraki kümeyi döner; zincir sonu = None.
fn fat_next(cluster: u32) -> Option<u32> {
    unsafe {
        let entry_offset = cluster as u64 * 4;
        let sector = FAT.fat_lba + (entry_offset / 512) as u32;
        let off = (entry_offset % 512) as usize;
        let mut sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, sector, &mut sec) {
            return None;
        }
        let val = le32(&sec, off) & 0x0FFF_FFFF;
        if val >= 0x0FFF_FFF8 || val == 0 {
            None
        } else {
            Some(val)
        }
    }
}

fn fat_set_next(cluster: u32, value: u32) -> bool {
    unsafe {
        let off = cluster as u64 * 4;
        let sector = FAT.fat_lba + (off / 512) as u32;
        let in_sector = (off % 512) as usize;
        for copy in 0..FAT.num_fats {
            let mut buf = [0u8; 512];
            if !read_sector_cached(FAT.drive, sector + copy * FAT.fat_size, &mut buf) { return false; }
            buf[in_sector..in_sector + 4].copy_from_slice(&value.to_le_bytes());
            if !ata::write_sector(FAT.drive, sector + copy * FAT.fat_size, &buf) { return false; }
        }
        true
    }
}

fn fat_is_free(cluster: u32) -> bool {
    unsafe {
        let off = cluster as u64 * 4;
        let sector = FAT.fat_lba + (off / 512) as u32;
        let in_sector = (off % 512) as usize;
        let mut buf = [0u8; 512];
        read_sector_cached(FAT.drive, sector, &mut buf) && le32(&buf, in_sector) & 0x0FFF_FFFF == 0
    }
}

fn fat_allocate_chain(count: usize) -> Option<alloc::vec::Vec<u32>> {
    let mut chain = alloc::vec::Vec::new();
    if count == 0 { return Some(chain); }
    if chain.try_reserve_exact(count).is_err() { return None; }
    let max_cluster = unsafe {
        let sectors = ata::drive_capacity_sectors(FAT.drive).saturating_sub(FAT.data_lba as u64);
        (sectors / FAT.sectors_per_cluster as u64 + 1).min((FAT.fat_size as u64 * 512 / 4).saturating_sub(2)) as u32
    };
    for c in 2..=max_cluster {
        if fat_is_free(c) {
            if !fat_set_next(c, 0x0FFF_FFFF) { return None; }
            chain.push(c);
            if chain.len() == count { break; }
        }
    }
    if chain.len() != count {
        for c in chain { let _ = fat_set_next(c, 0); }
        return None;
    }
    for i in 0..chain.len().saturating_sub(1) {
        if !fat_set_next(chain[i], chain[i + 1]) { return None; }
    }
    Some(chain)
}

fn fat_free_chain(mut cluster: u32) {
    let mut guard = 0usize;
    while cluster >= 2 && guard < 65536 {
        guard += 1;
        let next = fat_next(cluster);
        let _ = fat_set_next(cluster, 0);
        match next { Some(n) => cluster = n, None => break }
    }
}

fn fat_write_data(chain: &[u32], data: &[u8]) -> bool {
    let sectors_per_cluster = unsafe { FAT.sectors_per_cluster as usize };
    let cluster_bytes = sectors_per_cluster * 512;
    let drive = unsafe { FAT.drive };
    for (i, &cluster) in chain.iter().enumerate() {
        let start = i * cluster_bytes;
        let end = (start + cluster_bytes).min(data.len());
        for s in 0..sectors_per_cluster {
            let mut sec = [0u8; 512];
            let sector_start = start + s * 512;
            if sector_start < end {
                let take = (end - sector_start).min(512);
                sec[..take].copy_from_slice(&data[sector_start..sector_start + take]);
            }
            if !ata::write_sector(drive, cluster_to_lba(cluster) + s as u32, &sec) { return false; }
        }
    }
    true
}

fn cluster_to_lba(cluster: u32) -> u32 {
    unsafe {
        FAT.data_lba + (cluster.saturating_sub(2)) * FAT.sectors_per_cluster
    }
}

/// FAT kısa adını (8.3) okunabilir hâle getirir.
fn push_short_name(entry: &mut DiskEntry, raw: &[u8]) {
    let mut i = 0;
    let mut out = [0u8; NAME_MAX];
    let mut n = 0;
    while i < 8 && n < NAME_MAX {
        if raw[i] == b' ' {
            break;
        }
        out[n] = lower(raw[i]);
        n += 1;
        i += 1;
    }
    let mut ext = [0u8; 3];
    let mut en = 0;
    for k in 8..11 {
        if raw[k] != b' ' && en < 3 {
            ext[en] = lower(raw[k]);
            en += 1;
        }
    }
    if en > 0 && n + 1 + en <= NAME_MAX {
        out[n] = b'.';
        n += 1;
        out[n..n + en].copy_from_slice(&ext[..en]);
        n += en;
    }
    entry.name[..n].copy_from_slice(&out[..n]);
    entry.name_len = n;
}

fn lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b + 32 } else { b }
}

/// Bir kümenin sektörlerini tarayarak kök dizini listeler.
fn fat_list_root() -> bool {
    unsafe {
        let start = FAT.root_cluster;
        let mut cluster = start;
        let mut count = 0usize;
        let mut guard = 0;
        // Uzun dosya adı (LFN) parçaları burada toplanır.
        let mut lfn: [u8; 260] = [0; 260];
        let mut lfn_len = 0usize;
        let mut lfn_valid = false;

        while guard < 128 {
            guard += 1;
            let base_lba = cluster_to_lba(cluster);
            for s in 0..FAT.sectors_per_cluster {
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, base_lba + s, &mut sec) {
                    break;
                }
                for e in 0..16 {
                    let off = e * 32;
                    let first = sec[off];
                    if first == 0x00 {
                        // Dizin sonu
                        ENTRY_COUNT = count;
                        return true;
                    }
                    if first == 0xE5 {
                        continue;
                    }
                    let attr = sec[off + 11];
                    if attr == FAT_ATTR_LFN {
                        // LFN parçası: 13 UTF-16 karakter (basit ASCII kabulü)
                        let seq = sec[off];
                        if seq & 0x40 != 0 {
                            lfn = [0; 260];
                            lfn_len = 0;
                        }
                        let slots: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
                        for slot in slots {
                            let lo = sec[off + slot];
                            let hi = sec[off + slot + 1];
                            if lo == 0 && hi == 0 {
                                continue;
                            }
                            if lo == 0xFF && hi == 0xFF {
                                continue;
                            }
                            if lfn_len < lfn.len() {
                                lfn[lfn_len] = if hi == 0 { lo } else { b'?' };
                                lfn_len += 1;
                            }
                        }
                        lfn_valid = true;
                        continue;
                    }
                    if attr & FAT_ATTR_VOLUME_ID != 0 {
                        lfn_valid = false;
                        lfn_len = 0;
                        continue;
                    }
                    if count >= MAX_ENTRIES {
                        ENTRY_COUNT = count;
                        return true;
                    }
                    let mut entry = DiskEntry::empty();
                    entry.is_dir = attr & FAT_ATTR_DIRECTORY != 0;
                    entry.size = if entry.is_dir { 0 } else { le32(&sec, off + 28) as usize };
                    if lfn_valid && lfn_len > 0 {
                        // LFN dizide ters sırada toplanır; ham hâlde bırakmak yerine
                        // sade ASCII karakterleri kullan (basit ama okunur).
                        let n = lfn_len.min(NAME_MAX);
                        entry.name[..n].copy_from_slice(&lfn[..n]);
                        entry.name_len = n;
                    } else {
                        push_short_name(&mut entry, &sec[off..off + 11]);
                    }
                    lfn_valid = false;
                    lfn_len = 0;
                    ENTRIES[count] = entry;
                    count += 1;
                }
            }
            match fat_next(cluster) {
                Some(next) => cluster = next,
                None => break,
            }
        }
        ENTRY_COUNT = count;
        true
    }
}

// ---------------------------------------------------------------------------
// ISO9660 (CD-ROM)
// ---------------------------------------------------------------------------

struct IsoState {
    drive: u8,
    root_lba: u32,
    root_size: u32,
}

static mut ISO: IsoState = IsoState { drive: 1, root_lba: 0, root_size: 0 };

fn iso_mount(drive: u8) -> bool {
    let mut pvd = [0u8; 2048];
    if !ata::atapi_read_block(drive, 16, &mut pvd) {
        return false;
    }
    if &pvd[1..6] != b"CD001" {
        return false;
    }
    // Kök dizin kaydı: 156. ofset, extents 2 (both-endian)
    let root_lba = le32(&pvd, 156 + 2);
    let root_size = le32(&pvd, 156 + 10);
    serial("[ISO] PVD okundu. kok LBA: ");
    print_num(root_lba as u64);
    serial("\n");
    unsafe {
        ISO.drive = drive;
        ISO.root_lba = root_lba;
        ISO.root_size = root_size;
    }
    true
}

/// ISO9660 kök dizinini listeler (tek sektörlük kayıt zinciri yeterli).
fn iso_list_root() -> bool {
    unsafe {
        let mut buf = [0u8; 2048];
        let sectors = ((ISO.root_size as usize + 2047) / 2048).min(4);
        let mut count = 0usize;
        for s in 0..sectors {
            if !ata::atapi_read_block(ISO.drive, ISO.root_lba + s as u32, &mut buf) {
                break;
            }
            let mut off = 0usize;
            while off + 33 < 2048 {
                let rec_len = buf[off] as usize;
                if rec_len == 0 {
                    break;
                }
                let flags = buf[off + 25];
                let name_len = buf[off + 32] as usize;
                if off + 33 + name_len > 2048 {
                    break;
                }
                let name_bytes = &buf[off + 33..off + 33 + name_len];
                let identifier = if name_len == 1 && name_bytes[0] == 0 {
                    Some(".")
                } else if name_len == 1 && name_bytes[0] == 1 {
                    Some("..")
                } else {
                    None
                };
                let skip = identifier.is_some();
                if !skip && count < MAX_ENTRIES {
                    let mut entry = DiskEntry::empty();
                    entry.is_dir = flags & 0x02 != 0;
                    entry.size = le32(&buf, off + 10) as usize;
                    // ";1" sürüm eki ve sondaki noktayı temizle
                    let mut n = name_len.min(NAME_MAX);
                    if n > 2 && name_bytes[n - 2] == b';' {
                        n -= 2;
                    }
                    if !entry.is_dir && n > 0 && name_bytes[n - 1] == b'.' {
                        n -= 1;
                    }
                    for i in 0..n {
                        entry.name[i] = lower_iso(name_bytes[i]);
                    }
                    entry.name_len = n;
                    ENTRIES[count] = entry;
                    count += 1;
                }
                off += rec_len;
            }
        }
        ENTRY_COUNT = count;
        true
    }
}

fn lower_iso(b: u8) -> u8 {
    if b == b';' { b } else { lower(b) }
}

// ---------------------------------------------------------------------------
// Genel API
// ---------------------------------------------------------------------------

/// Sürücüyü (HDD/QCOW2/USB) standart FAT32 dosya sistemiyle biçimlendirir.
pub fn fat_format(drive: u8) -> bool {
    let total_sectors = ata::drive_capacity_sectors(drive);
    let sectors_u32 = if total_sectors > 0 && total_sectors <= 0xFFFF_FFFF {
        total_sectors as u32
    } else {
        131072 // 64 MB varsayılan (64 * 1024 * 1024 / 512)
    };

    // 1. Sektör 0: FAT32 Volume Boot Record (BPB)
    let mut bpb = [0u8; 512];
    bpb[0] = 0xEB; bpb[1] = 0x58; bpb[2] = 0x90; // jmp boot
    bpb[3..11].copy_from_slice(b"MINOSOS "); // OEM Name
    bpb[11] = 0x00; bpb[12] = 0x02; // 512 bytes per sector (le16)
    bpb[13] = 8; // 8 sectors per cluster (4KB)
    bpb[14] = 32; bpb[15] = 0x00; // 32 reserved sectors (le16)
    bpb[16] = 2; // 2 FATs
    bpb[21] = 0xF8; // media descriptor: fixed disk
    bpb[24] = 63; bpb[25] = 0; // sectors per track
    bpb[26] = 255; bpb[27] = 0; // number of heads
    // Total sectors 32:
    bpb[32] = (sectors_u32 & 0xFF) as u8;
    bpb[33] = ((sectors_u32 >> 8) & 0xFF) as u8;
    bpb[34] = ((sectors_u32 >> 16) & 0xFF) as u8;
    bpb[35] = ((sectors_u32 >> 24) & 0xFF) as u8;
    // FAT size 32 (sectors per FAT): 512 sektör (256 KB)
    let fat_size: u32 = 512;
    bpb[36] = (fat_size & 0xFF) as u8;
    bpb[37] = ((fat_size >> 8) & 0xFF) as u8;
    bpb[38] = ((fat_size >> 16) & 0xFF) as u8;
    bpb[39] = ((fat_size >> 24) & 0xFF) as u8;
    // Root cluster: 2
    bpb[44] = 2; bpb[45] = 0; bpb[46] = 0; bpb[47] = 0;
    // FSInfo sector: 1
    bpb[48] = 1; bpb[49] = 0;
    // Backup boot sector: 6
    bpb[50] = 6; bpb[51] = 0;
    bpb[64] = 0x80; // drive number
    bpb[66] = 0x29; // boot signature
    bpb[67] = 0x78; bpb[68] = 0x56; bpb[69] = 0x34; bpb[70] = 0x12; // volume ID
    bpb[71..82].copy_from_slice(b"MINOS DISK "); // Volume label
    bpb[82..90].copy_from_slice(b"FAT32   "); // System ID
    bpb[510] = 0x55; bpb[511] = 0xAA; // Signature

    if !ata::write_sector(drive, 0, &bpb) {
        return false;
    }
    // Yedek boot sektörü (LBA 6)
    ata::write_sector(drive, 6, &bpb);

    // 2. FSInfo Sektörü (LBA 1)
    let mut fsinfo = [0u8; 512];
    fsinfo[0..4].copy_from_slice(&0x41615252u32.to_le_bytes()); // Lead sig
    fsinfo[484..488].copy_from_slice(&0x61417272u32.to_le_bytes()); // Struct sig
    fsinfo[488..492].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // Free clusters
    fsinfo[492..496].copy_from_slice(&3u32.to_le_bytes()); // Next free cluster: 3
    fsinfo[508..512].copy_from_slice(&0xAA550000u32.to_le_bytes()); // Trail sig
    ata::write_sector(drive, 1, &fsinfo);

    // 3. FAT1 ve FAT2 Tabloları (LBA 32 ve LBA 32 + 512 = 544)
    let fat1_lba = 32u32;
    let fat2_lba = fat1_lba + fat_size;
    let mut fat_sec = [0u8; 512];
    // Cluster 0: 0x0FFFFFF8
    fat_sec[0..4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
    // Cluster 1: 0x0FFFFFFF
    fat_sec[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
    // Cluster 2 (Kök dizin): 0x0FFFFFFF (zincir sonu)
    fat_sec[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
    // Cluster 3 (Varsayılan benioku.txt): 0x0FFFFFFF
    fat_sec[12..16].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    ata::write_sector(drive, fat1_lba, &fat_sec);
    ata::write_sector(drive, fat2_lba, &fat_sec);

    let empty_sec = [0u8; 512];
    for s in 1..4 {
        ata::write_sector(drive, fat1_lba + s, &empty_sec);
        ata::write_sector(drive, fat2_lba + s, &empty_sec);
    }

    // 4. Kök Dizin (Root Directory - Cluster 2)
    // data_lba = fat1_lba + 2 * fat_size = 32 + 1024 = 1056
    let root_lba = fat1_lba + 2 * fat_size;
    let mut root_sec = [0u8; 512];

    // Girdi 0: Ses etiketi (Volume Label)
    root_sec[0..11].copy_from_slice(b"MINOS DISK ");
    root_sec[11] = FAT_ATTR_VOLUME_ID;

    // Girdi 1: BENIOKU.TXT
    let default_msg = b"minOs Sabit Disk (QCOW2)\n========================\nDosya Yoneticisi ve Not Defteri ile bu diske dosya yazabilir, silebilir ve yeni klasorler olusturabilirsiniz.\n";
    root_sec[32..43].copy_from_slice(b"BENIOKUTXT"); // 8.3 dosya adi
    root_sec[32 + 11] = 0x20; // Normal dosya (Archive)
    root_sec[32 + 20] = 0; root_sec[32 + 21] = 0; // High cluster
    root_sec[32 + 26] = 3; root_sec[32 + 27] = 0; // Low cluster: 3
    let msg_len = default_msg.len() as u32;
    root_sec[32 + 28..32 + 32].copy_from_slice(&msg_len.to_le_bytes());

    ata::write_sector(drive, root_lba, &root_sec);

    // Kök dizinin kalan sektörlerini temizle
    for s in 1..8 {
        ata::write_sector(drive, root_lba + s, &empty_sec);
    }

    // 5. Cluster 3 Verisi: BENIOKU.TXT içeriğini yaz
    let cluster3_lba = root_lba + 8;
    let mut data_sec = [0u8; 512];
    data_sec[..default_msg.len()].copy_from_slice(default_msg);
    ata::write_sector(drive, cluster3_lba, &data_sec);

    ata::flush_cache(drive);
    serial("[FS] Sabit Disk FAT32 olarak bicimlendirildi!\n");
    true
}

/// ATA kanalını tarar, Sabit Disk (FAT32), USB bellek ve CD-ROM (ISO9660) ortamlarını bağlar.
pub fn init() {
    unsafe {
        if READY {
            return;
        }
        ata::init();
        READY = true;
    }
    // 1) Master → Sabit Disk / USB bellek (FAT32)
    if ata::master_kind() == ata::DeviceKind::Ata {
        let part = find_partition_lba(0);
        let candidate = if part > 0 { part } else { 0 };
        if fat_mount(0, candidate) {
            unsafe { KIND0 = FsKind::Fat32; }
            serial("[FS] Sabit Disk / USB FAT32 olarak baglandi.\n");
        } else if fat_mount(0, 0) {
            unsafe { KIND0 = FsKind::Fat32; }
            serial("[FS] Sabit Disk / USB FAT32 (superfloppy) olarak baglandi.\n");
        } else if ata::drive_capacity_sectors(0) > 0 {
            serial("[FS] Sabit Disk tespit edildi, FAT32 bicimlendiriliyor...\n");
            if fat_format(0) && fat_mount(0, 0) {
                unsafe { KIND0 = FsKind::Fat32; }
                serial("[FS] Sabit Disk (qcow2) FAT32 olarak hazirlandi ve yazmaya acildi!\n");
            }
        } else {
            serial("[FS] Depolama aygiti bulundu ancak dosya sistemi taninamadi.\n");
        }
    } else if ata::master_kind() == ata::DeviceKind::Atapi {
        if iso_mount(0) {
            unsafe { KIND0 = FsKind::Iso9660; }
            serial("[FS] Master ATAPI ISO9660 olarak baglandi.\n");
        }
    }

    // 2) Slave → CD-ROM (ISO9660)
    if ata::slave_kind() == ata::DeviceKind::Atapi {
        if iso_mount(1) {
            unsafe { KIND1 = FsKind::Iso9660; }
            serial("[FS] CD-ROM ISO9660 olarak baglandi.\n");
        } else {
            serial("[FS] CD-ROM ATAPI bulundu ancak ISO9660 taninamadi.\n");
        }
    }
}

pub fn ready() -> bool {
    unsafe { READY }
}

/// USB (master) aygıtının dosya sistemi türü.
pub fn usb_kind() -> FsKind {
    unsafe { KIND0 }
}

/// CD-ROM (slave) aygıtının dosya sistemi türü.
pub fn cd_kind() -> FsKind {
    unsafe { KIND1 }
}

/// USB disk erişilebilir mi?
pub fn usb_available() -> bool {
    unsafe { KIND0 != FsKind::None }
}

/// Bağlı ortamın listesini yeniler ve girdi sayısını döner.
pub fn refresh() -> usize {
    unsafe {
        ENTRY_COUNT = 0;
        LISTED = false;
    }
    let ok = unsafe {
        match KIND0 {
            FsKind::Fat32 => fat_list_root(),
            FsKind::Iso9660 => iso_list_root(),
            FsKind::None => {
                // USB yoksa CD-ROM'u dene
                if KIND1 == FsKind::Iso9660 { iso_list_root() } else { false }
            }
        }
    };
    if !ok {
        unsafe { ENTRY_COUNT = 0; }
    }
    unsafe {
        LISTED = true;
        ENTRY_COUNT
    }
}

pub fn listed() -> bool {
    unsafe { LISTED }
}

pub fn entry_count() -> usize {
    unsafe { ENTRY_COUNT }
}

pub fn entry(index: usize) -> Option<DiskEntry> {
    unsafe {
        if index < ENTRY_COUNT {
            Some(ENTRIES[index])
        } else {
            None
        }
    }
}

/// USB disk üzerindeki toplam sektör sayısı (512 B) — kapasite göstergesi.
pub fn usb_sector_count() -> u64 {
    ata::master_sector_count()
}

/// Dosya adına göre içeriği yükler (tarayıcı `file://` adresleri için).
/// Başarı durumunda bayt sayısını döner.
pub fn load_file(name: &str) -> usize {
    unsafe {
        CONTENT_LEN = 0;
        CONTENT_NAME_LEN = name.len().min(NAME_MAX);
        CONTENT_NAME[..CONTENT_NAME_LEN].copy_from_slice(&name.as_bytes()[..CONTENT_NAME_LEN]);
    }
    if !unsafe { LISTED } {
        refresh();
    }
    let count = unsafe { ENTRY_COUNT };
    for i in 0..count {
        let Some(entry) = entry(i) else { continue };
        let entry_name = entry.as_str();
        if entry.is_dir || !entry_name.eq_ignore_ascii_case(name) {
            continue;
        }
        let n = load_file_body(i, entry.size);
        return n;
    }
    0
}

/// FAT32 kök dizinindeki dosyayı tam boyutuyla heap'e yükler.
pub fn load_file_alloc(name: &str) -> Option<alloc::vec::Vec<u8>> {
    if !unsafe { LISTED } { refresh(); }
    let index = (0..unsafe { ENTRY_COUNT }).find(|&i| {
        entry(i).map_or(false, |e| !e.is_dir && e.as_str().eq_ignore_ascii_case(name))
    })?;
    let meta = entry(index)?;
    let mut cluster = unsafe { FAT.root_cluster };
    let mut found = None;
    let mut seen = 0usize;
    while seen < 128 {
        seen += 1;
        for s in 0..unsafe { FAT.sectors_per_cluster } {
            let lba = cluster_to_lba(cluster) + s;
            let mut sec = [0u8; 512];
            if !read_sector_cached(unsafe { FAT.drive }, lba, &mut sec) { return None; }
            for e in 0..16 {
                let off = e * 32;
                if sec[off] == 0 { return None; }
                if sec[off] == 0xE5 || sec[off + 11] == FAT_ATTR_LFN || sec[off + 11] & FAT_ATTR_VOLUME_ID != 0 { continue; }
                let mut item = DiskEntry::empty();
                push_short_name(&mut item, &sec[off..off + 11]);
                if item.as_str().eq_ignore_ascii_case(name) {
                    let high = le16(&sec, off + 20) as u32;
                    let low = le16(&sec, off + 26) as u32;
                    found = Some((high << 16) | low);
                    break;
                }
            }
            if found.is_some() { break; }
        }
        if found.is_some() { break; }
        match fat_next(cluster) { Some(next) => cluster = next, None => break }
    }
    let mut cluster = found?;
    if meta.size == 0 { return Some(alloc::vec::Vec::new()); }
    let mut out = alloc::vec::Vec::new();
    out.try_reserve_exact(meta.size).ok()?;
    out.resize(meta.size, 0);
    let mut written = 0usize;
    let mut guard = 0usize;
    while written < meta.size && guard < 65536 {
        guard += 1;
        let base = cluster_to_lba(cluster);
        for s in 0..unsafe { FAT.sectors_per_cluster } {
            if written >= meta.size { break; }
            let mut sec = [0u8; 512];
            if !read_sector_cached(unsafe { FAT.drive }, base + s, &mut sec) { return None; }
            let take = (meta.size - written).min(512);
            out[written..written + take].copy_from_slice(&sec[..take]);
            written += take;
        }
        if written < meta.size { cluster = fat_next(cluster)?; }
    }
    (written == meta.size).then_some(out)
}

fn load_file_body(index: usize, size: usize) -> usize {
    unsafe {
        let kind = if KIND0 != FsKind::None { KIND0 } else { KIND1 };
        match kind {
            FsKind::Fat32 => fat_load_file(index, size),
            FsKind::Iso9660 => iso_load_file(index, size),
            FsKind::None => 0,
        }
    }
}

/// FAT32 kök dizinindeki `index`. girdinin içeriğini okur.
fn fat_load_file(index: usize, size: usize) -> usize {
    // Kümeyi yeniden bul: dizin girdileri sırayla okunurken ilk küme numarası
    // saklanmadığı için kök dizini tekrar tarayıp hedefi buluruz.
    let mut cluster = unsafe { FAT.root_cluster };
    let mut count = 0usize;
    let mut guard = 0;
    while guard < 128 {
        guard += 1;
        let base_lba = cluster_to_lba(cluster);
        for s in 0..unsafe { FAT.sectors_per_cluster } {
            let mut sec = [0u8; 512];
            if !read_sector_cached(unsafe { FAT.drive }, base_lba + s, &mut sec) {
                return 0;
            }
            for e in 0..16 {
                let off = e * 32;
                let first = sec[off];
                if first == 0x00 {
                    return 0;
                }
                let attr = sec[off + 11];
                if first == 0xE5 || attr == FAT_ATTR_LFN || attr & FAT_ATTR_VOLUME_ID != 0 {
                    continue;
                }
                if count == index {
                    if attr & FAT_ATTR_DIRECTORY != 0 {
                        return 0;
                    }
                    let high = le16(&sec, off + 20) as u32;
                    let low = le16(&sec, off + 26) as u32;
                    let start_cluster = (high << 16) | low;
                    if start_cluster < 2 {
                        return 0;
                    }
                    return fat_read_chain(start_cluster, size);
                }
                count += 1;
            }
        }
        match fat_next(cluster) {
            Some(next) => cluster = next,
            None => return 0,
        }
    }
    0
}

fn fat_read_chain(start_cluster: u32, size: usize) -> usize {
    let mut written = 0usize;
    let mut cluster = start_cluster;
    let mut guard = 0;
    while written < size && written < CONTENT_MAX && guard < 64 {
        guard += 1;
        let base = cluster_to_lba(cluster);
        for s in 0..unsafe { FAT.sectors_per_cluster } {
            if written >= size || written >= CONTENT_MAX {
                break;
            }
            let mut sec = [0u8; 512];
            if !read_sector_cached(unsafe { FAT.drive }, base + s, &mut sec) {
                unsafe { CONTENT_LEN = written; }
                return written;
            }
            let take = (size - written).min(512);
            unsafe {
                CONTENT[written..written + take].copy_from_slice(&sec[..take]);
            }
            written += take;
        }
        match fat_next(cluster) {
            Some(next) => cluster = next,
            None => break,
        }
    }
    unsafe { CONTENT_LEN = written; }
    written
}

/// ISO9660 kök dizinindeki `index`. girdinin içeriğini okur.
fn iso_load_file(index: usize, size: usize) -> usize {
    unsafe {
        let mut buf = [0u8; 2048];
        let sectors = ((ISO.root_size as usize + 2047) / 2048).min(4);
        let mut count = 0usize;
        for s in 0..sectors {
            if !ata::atapi_read_block(ISO.drive, ISO.root_lba + s as u32, &mut buf) {
                return 0;
            }
            let mut off = 0usize;
            while off + 33 < 2048 {
                let rec_len = buf[off] as usize;
                if rec_len == 0 {
                    break;
                }
                let flags = buf[off + 25];
                let name_len = buf[off + 32] as usize;
                if off + 33 + name_len > 2048 {
                    break;
                }
                let name_bytes = &buf[off + 33..off + 33 + name_len];
                let is_special = name_len == 1 && (name_bytes[0] == 0 || name_bytes[0] == 1);
                if !is_special {
                    if count == index {
                        if flags & 0x02 != 0 {
                            return 0;
                        }
                        let extent = le32(&buf, off + 2);
                        let file_size = le32(&buf, off + 10) as usize;
                        let take = file_size.min(size).min(CONTENT_MAX);
                        let n_sectors = (take + 2047) / 2048;
                        let mut written = 0usize;
                        let mut block = [0u8; 2048];
                        for b in 0..n_sectors {
                            if !ata::atapi_read_block(ISO.drive, extent + b as u32, &mut block) {
                                break;
                            }
                            let chunk = (take - written).min(2048);
                            CONTENT[written..written + chunk].copy_from_slice(&block[..chunk]);
                            written += chunk;
                        }
                        CONTENT_LEN = written;
                        return written;
                    }
                    count += 1;
                }
                off += rec_len;
            }
        }
    }
    0
}

/// `load_file` ile yüklenen içeriği döner.
pub fn content() -> &'static [u8] {
    unsafe { &CONTENT[..CONTENT_LEN] }
}

pub fn content_len() -> usize {
    unsafe { CONTENT_LEN }
}

/// USB ortamındaki toplam dosya sayısı (dizinler hariç).
pub fn file_count() -> usize {
    unsafe {
        let mut n = 0;
        for i in 0..ENTRY_COUNT {
            if !ENTRIES[i].is_dir {
                n += 1;
            }
        }
        n
    }
}

pub fn cd_available() -> bool {
    unsafe { KIND1 != FsKind::None }
}

/// FAT32 ortamında dosya siler (dizin girdisini 0xE5 olarak işaretler).
pub fn delete_file_by_name(name: &str) -> bool {
    unsafe {
        if KIND0 != FsKind::Fat32 {
            return false;
        }
        let mut cluster = FAT.root_cluster;
        let mut guard = 0;
        while guard < 128 {
            guard += 1;
            let base_lba = cluster_to_lba(cluster);
            for s in 0..FAT.sectors_per_cluster {
                let sector_lba = base_lba + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, sector_lba, &mut sec) {
                    return false;
                }
                for e in 0..16 {
                    let off = e * 32;
                    let first = sec[off];
                    if first == 0x00 {
                        return false;
                    }
                    if first == 0xE5 {
                        continue;
                    }
                    let attr = sec[off + 11];
                    if attr == FAT_ATTR_LFN || attr & FAT_ATTR_VOLUME_ID != 0 {
                        continue;
                    }
                    let mut entry = DiskEntry::empty();
                    push_short_name(&mut entry, &sec[off..off + 11]);
                    if entry.as_str().eq_ignore_ascii_case(name) {
                        let start = (le16(&sec, off + 20) as u32) << 16 | le16(&sec, off + 26) as u32;
                        sec[off] = 0xE5;
                        if !ata::write_sector(FAT.drive, sector_lba, &sec) { return false; }
                        fat_free_chain(start);
                        ata::flush_cache(FAT.drive);
                        refresh();
                        return true;
                    }
                }
            }
            match fat_next(cluster) {
                Some(next) => cluster = next,
                None => return false,
            }
        }
        false
    }
}

fn to_8_3_name(name: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    let trimmed = name.trim();
    let mut parts = trimmed.split('.');
    let base = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    for (i, b) in base.bytes().take(8).enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    for (i, b) in ext.bytes().take(3).enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    out
}

fn valid_8_3_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    parts.next().is_none()
        && !base.is_empty() && base.len() <= 8 && ext.len() <= 3
        && base.bytes().chain(ext.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn rename_by_name(old_name: &str, new_name: &str) -> bool {
    unsafe {
        if KIND0 != FsKind::Fat32 || !valid_8_3_name(new_name) { return false; }
        let mut cluster = FAT.root_cluster;
        let mut guard = 0;
        while guard < 128 {
            guard += 1;
            for s in 0..FAT.sectors_per_cluster {
                let lba = cluster_to_lba(cluster) + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
                for e in 0..16 {
                    let off = e * 32;
                    if sec[off] == 0 { return false; }
                    if sec[off] == 0xE5 || sec[off + 11] == FAT_ATTR_LFN || sec[off + 11] & FAT_ATTR_VOLUME_ID != 0 { continue; }
                    let mut item = DiskEntry::empty();
                    push_short_name(&mut item, &sec[off..off + 11]);
                    if item.as_str().eq_ignore_ascii_case(new_name) { return false; }
                    if item.as_str().eq_ignore_ascii_case(old_name) {
                        sec[off..off + 11].copy_from_slice(&to_8_3_name(new_name));
                        let ok = ata::write_sector(FAT.drive, lba, &sec);
                        if ok { ata::flush_cache(FAT.drive); refresh(); }
                        return ok;
                    }
                }
            }
            match fat_next(cluster) { Some(next) => cluster = next, None => return false }
        }
        false
    }
}

/// FAT32 ortamında dosya içeriğini kaydeder / yazar (mevcutsa günceller, yoksa yeni oluşturur).
pub fn save_file_by_name(name: &str, data: &[u8]) -> bool {
    unsafe {
        if KIND0 != FsKind::Fat32 || !valid_8_3_name(name) {
            return false;
        }
        let mut cluster = FAT.root_cluster;
        let mut guard = 0;
        let mut free_slot: Option<(u32, usize)> = None;
        let mut existing: Option<(u32, u32, u32)> = None;

        while guard < 128 {
            guard += 1;
            let base_lba = cluster_to_lba(cluster);
            for s in 0..FAT.sectors_per_cluster {
                let sector_lba = base_lba + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, sector_lba, &mut sec) {
                    return false;
                }
                for e in 0..16 {
                    let off = e * 32;
                    let first = sec[off];
                    if first == 0x00 || first == 0xE5 {
                        if free_slot.is_none() {
                            free_slot = Some((sector_lba, off));
                        }
                        if first == 0x00 {
                            break;
                        }
                        continue;
                    }
                    let attr = sec[off + 11];
                    if attr == FAT_ATTR_LFN || attr & FAT_ATTR_VOLUME_ID != 0 {
                        continue;
                    }
                    let mut entry = DiskEntry::empty();
                    push_short_name(&mut entry, &sec[off..off + 11]);
                    if entry.as_str().eq_ignore_ascii_case(name) {
                        let high = le16(&sec, off + 20) as u32;
                        let low = le16(&sec, off + 26) as u32;
                        let start_cluster = (high << 16) | low;
                        existing = Some((sector_lba, off as u32, start_cluster));
                    }
                }
            }
            match fat_next(cluster) {
                Some(next) => cluster = next,
                None => break,
            }
        }

        let slot = existing.map(|(lba, off, _)| (lba, off as usize)).or(free_slot);
        let Some((dir_lba, dir_off)) = slot else { return false; };
        let cluster_bytes = FAT.sectors_per_cluster as usize * 512;
        let needed = data.len().max(1).saturating_add(cluster_bytes - 1) / cluster_bytes;
        let chain = match fat_allocate_chain(needed) { Some(c) => c, None => return false };
        if !fat_write_data(&chain, data) {
            for c in chain { let _ = fat_set_next(c, 0); }
            return false;
        }
        let mut dir_sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, dir_lba, &mut dir_sec) { return false; }
        let off = dir_off;
        if existing.is_none() {
            dir_sec[off..off + 11].copy_from_slice(&to_8_3_name(name));
            dir_sec[off + 11] = 0x20;
        } else if let Some((_, _, old_start)) = existing {
            let mut old = old_start;
            let mut guard = 0;
            while old >= 2 && guard < 4096 {
                guard += 1;
                let next = fat_next(old);
                let _ = fat_set_next(old, 0);
                match next { Some(n) => old = n, None => break }
            }
        }
        let start = chain[0];
        dir_sec[off + 20..off + 22].copy_from_slice(&((start >> 16) as u16).to_le_bytes());
        dir_sec[off + 26..off + 28].copy_from_slice(&(start as u16).to_le_bytes());
        dir_sec[off + 28..off + 32].copy_from_slice(&(data.len() as u32).to_le_bytes());
        if !ata::write_sector(FAT.drive, dir_lba, &dir_sec) { return false; }
        ata::flush_cache(FAT.drive);
        refresh();
        true
    }
}

/// FAT32 kök dizininde boş bir klasör oluşturur.
pub fn create_dir_by_name(name: &str) -> bool {
    unsafe {
        if KIND0 != FsKind::Fat32 || !valid_8_3_name(name) { return false; }
        let mut cluster = FAT.root_cluster;
        let mut free_slot = None;
        let mut guard = 0;
        while guard < 128 {
            guard += 1;
            for s in 0..FAT.sectors_per_cluster {
                let lba = cluster_to_lba(cluster) + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
                for e in 0..16 {
                    let off = e * 32;
                    if sec[off] == 0 || sec[off] == 0xE5 {
                        if free_slot.is_none() { free_slot = Some((lba, off)); }
                        if sec[off] == 0 { break; }
                        continue;
                    }
                    if sec[off + 11] == FAT_ATTR_LFN || sec[off + 11] & FAT_ATTR_VOLUME_ID != 0 { continue; }
                    let mut entry = DiskEntry::empty();
                    push_short_name(&mut entry, &sec[off..off + 11]);
                    if entry.as_str().eq_ignore_ascii_case(name) { return entry.is_dir; }
                }
            }
            match fat_next(cluster) { Some(next) => cluster = next, None => break }
        }
        let Some((dir_lba, off)) = free_slot else { return false; };
        let Some(chain) = fat_allocate_chain(1) else { return false; };
        let child = chain[0];
        let mut child_sec = [0u8; 512];
        child_sec[..11].copy_from_slice(b".          "); child_sec[11] = FAT_ATTR_DIRECTORY;
        child_sec[20..22].copy_from_slice(&((child >> 16) as u16).to_le_bytes());
        child_sec[26..28].copy_from_slice(&(child as u16).to_le_bytes());
        child_sec[32..43].copy_from_slice(b"..         "); child_sec[43] = FAT_ATTR_DIRECTORY;
        let root = FAT.root_cluster;
        child_sec[52..54].copy_from_slice(&((root >> 16) as u16).to_le_bytes());
        child_sec[58..60].copy_from_slice(&(root as u16).to_le_bytes());
        let child_lba = cluster_to_lba(child);
        if !ata::write_sector(FAT.drive, child_lba, &child_sec) { return false; }
        for s in 1..FAT.sectors_per_cluster {
            if !ata::write_sector(FAT.drive, child_lba + s, &[0u8; 512]) { return false; }
        }
        let mut parent_sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, dir_lba, &mut parent_sec) { return false; }
        parent_sec[off..off + 11].copy_from_slice(&to_8_3_name(name));
        parent_sec[off + 11] = FAT_ATTR_DIRECTORY;
        parent_sec[off + 20..off + 22].copy_from_slice(&((child >> 16) as u16).to_le_bytes());
        parent_sec[off + 26..off + 28].copy_from_slice(&(child as u16).to_le_bytes());
        let ok = ata::write_sector(FAT.drive, dir_lba, &parent_sec);
        ata::flush_cache(FAT.drive);
        refresh();
        ok
    }
}

pub fn delete_dir_by_name(name: &str) -> bool {
    unsafe {
        if KIND0 != FsKind::Fat32 { return false; }
        let mut cluster = FAT.root_cluster;
        let mut guard = 0;
        while guard < 128 {
            guard += 1;
            for s in 0..FAT.sectors_per_cluster {
                let lba = cluster_to_lba(cluster) + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
                for e in 0..16 {
                    let off = e * 32;
                    if sec[off] == 0 { return false; }
                    if sec[off] == 0xE5 || sec[off + 11] == FAT_ATTR_LFN || sec[off + 11] & FAT_ATTR_VOLUME_ID != 0 { continue; }
                    let mut item = DiskEntry::empty();
                    push_short_name(&mut item, &sec[off..off + 11]);
                    if !item.is_dir || !item.as_str().eq_ignore_ascii_case(name) { continue; }
                    let child = (le16(&sec, off + 20) as u32) << 16 | le16(&sec, off + 26) as u32;
                    if child < 2 { return false; }
                    let child_lba = cluster_to_lba(child);
                    let mut child_sec = [0u8; 512];
                    if !read_sector_cached(FAT.drive, child_lba, &mut child_sec) { return false; }
                    for pos in (0..512).step_by(32) {
                        let first = child_sec[pos];
                        if first == 0 { break; }
                        if first == 0xE5 { continue; }
                        let dot = child_sec[pos..pos + 11] == *b".          " || child_sec[pos..pos + 11] == *b"..         ";
                        if !dot { return false; }
                    }
                    sec[off] = 0xE5;
                    if !ata::write_sector(FAT.drive, lba, &sec) { return false; }
                    fat_free_chain(child);
                    ata::flush_cache(FAT.drive);
                    refresh();
                    return true;
                }
            }
            match fat_next(cluster) { Some(next) => cluster = next, None => return false }
        }
        false
    }
}


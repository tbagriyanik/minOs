#![allow(dead_code, static_mut_refs)]

//! Disk tabanlı dosya sistemi okuyucuları: **FAT32** (USB bellek) ve
//! **ISO9660** (CD-ROM). Her ikisi de ATA/ATAPI PIO katmanı üzerinden çalışır.
//!
//! Amaç: `run.ps1 -UsbDisks` ile QEMU'ya bağlanan USB aygıtın içeriğini Dosya
//! Yöneticisi listesinde göstermek ve tarayıcıda `file://` adresleriyle yerel
//! sayfa açabilmek.

use crate::drivers::ata;
extern crate alloc;
use alloc::string::ToString;

/// Listelenen azami girdi sayısı (giriş başına sabit alanlar).
pub const MAX_ENTRIES: usize = 64;
/// Dosya adı için azami uzunluk (LFN uzun adları için).
pub const NAME_MAX: usize = 64;
/// Tarayıcının yerel dosyadan göstermek için kesip alabileceği azami içerik.
pub const CONTENT_MAX: usize = 2048;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    None,
    Fat32,
    Ext2,
    Iso9660,
}

impl FsKind {
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (FsKind::Fat32, true) => "FAT32",
            (FsKind::Fat32, false) => "FAT32",
            (FsKind::Ext2, _) => "Ext2",
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
static mut INSTALLED: bool = false;

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
const FAT_ATTR_ARCHIVE: u8 = 0x20;

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
    total_sectors: u32,
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
    total_sectors: 0,
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
    let total_sectors_16 = le16(&bpb, 19) as u32;
    let total_sectors = if total_sectors_16 != 0 { total_sectors_16 } else { le32(&bpb, 32) };
    let data_lba = partition_lba.saturating_add(reserved).saturating_add(num_fats.saturating_mul(fat_size));
    let fat_entries = fat_size as u64 * 512 / 4;
    if total_sectors == 0 || data_lba <= partition_lba || data_lba >= partition_lba.saturating_add(total_sectors)
        || root_cluster < 2 || root_cluster as u64 >= fat_entries
    {
        return false;
    }
    let capacity = ata::drive_capacity_sectors(drive);
    if capacity > 0 && partition_lba as u64 + total_sectors as u64 > capacity { return false; }
    let fat_lba = partition_lba + reserved;

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
        FAT.total_sectors = total_sectors;
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
            if !ata::write_sector_unflushed(FAT.drive, sector + copy * FAT.fat_size, &buf) { return false; }
        }
        true
    }
}

fn fat_allocate_chain(count: usize) -> Option<alloc::vec::Vec<u32>> {
    let mut chain = alloc::vec::Vec::new();
    if count == 0 { return Some(chain); }
    if chain.try_reserve_exact(count).is_err() { return None; }
    let mut touched = alloc::vec::Vec::<(u32, [u8; 512])>::new();
    let max_cluster = unsafe {
        let data_rel = FAT.data_lba.saturating_sub(FAT.partition_lba);
        let data_sectors = FAT.total_sectors.saturating_sub(data_rel);
        let by_partition = (data_sectors / FAT.sectors_per_cluster.max(1)).saturating_add(2);
        let by_fat = (FAT.fat_size as u64 * 512 / 4).saturating_sub(2) as u32;
        by_partition.min(by_fat)
    };
    let mut sector_index = u32::MAX;
    let mut fat_sector = [0u8; 512];
    for c in 2..=max_cluster {
        let entry_offset = c as u64 * 4;
        let current_sector = (entry_offset / 512) as u32;
        if current_sector != sector_index {
            sector_index = current_sector;
            let lba = unsafe { FAT.fat_lba + current_sector };
            if !read_sector_cached(unsafe { FAT.drive }, lba, &mut fat_sector) { return None; }
        }
        let offset = (entry_offset % 512) as usize;
        if le32(&fat_sector, offset) & 0x0FFF_FFFF == 0 {
            chain.push(c);
            if chain.len() == count { break; }
        }
    }
    if chain.len() != count { return None; }
    // Re-read only distinct FAT sectors and update them in memory, then write
    // each sector once to both FAT copies.
    for (i, &cluster) in chain.iter().enumerate() {
        let value = chain.get(i + 1).copied().unwrap_or(0x0FFF_FFFF);
        let entry_offset = cluster as u64 * 4;
        let idx = (entry_offset / 512) as u32;
        let pos = touched.iter().position(|(s, _)| *s == idx);
        let slot = if let Some(p) = pos { p } else {
            touched.try_reserve(1).ok()?;
            let mut sec = [0u8; 512];
            let lba = unsafe { FAT.fat_lba + idx };
            if !read_sector_cached(unsafe { FAT.drive }, lba, &mut sec) { return None; }
            touched.push((idx, sec));
            touched.len() - 1
        };
        let offset = (entry_offset % 512) as usize;
        let old = le32(&touched[slot].1, offset);
        touched[slot].1[offset..offset + 4].copy_from_slice(&((old & 0xF000_0000) | value).to_le_bytes());
    }
    for (idx, sec) in touched.iter() {
        for copy in 0..unsafe { FAT.num_fats } {
            let lba = unsafe { FAT.fat_lba + *idx + copy * FAT.fat_size };
            if !ata::write_sector_unflushed(unsafe { FAT.drive }, lba, sec) { return None; }
        }
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
    let mut first = 0usize;
    while first < chain.len() {
        let mut end_cluster = first + 1;
        while end_cluster < chain.len() && chain[end_cluster] == chain[end_cluster - 1] + 1 { end_cluster += 1; }
        let start_byte = first * cluster_bytes;
        let run_sectors = (end_cluster - first) * sectors_per_cluster;
        let available = data.len().saturating_sub(start_byte).min(run_sectors * 512);
        let full_sectors = available / 512;
        let lba = cluster_to_lba(chain[first]);
        if full_sectors > 0 && ata::write_sectors_unflushed(drive, lba, full_sectors, &data[start_byte..start_byte + full_sectors * 512]) != full_sectors {
            return false;
        }
        let partial = available % 512;
        let mut next_sector = full_sectors;
        if partial > 0 {
            let mut sector = [0u8; 512];
            sector[..partial].copy_from_slice(&data[start_byte + full_sectors * 512..start_byte + available]);
            if !ata::write_sector_unflushed(drive, lba + next_sector as u32, &sector) { return false; }
            next_sector += 1;
        }
        let remaining_sectors = run_sectors.saturating_sub(next_sector);
        if remaining_sectors > 0 {
            let zero_bytes = remaining_sectors * 512;
            let mut zeros = alloc::vec::Vec::new();
            if zeros.try_reserve_exact(zero_bytes).is_err() { return false; }
            zeros.resize(zero_bytes, 0);
            if ata::write_sectors_unflushed(drive, lba + next_sector as u32, remaining_sectors, &zeros) != remaining_sectors { return false; }
        }
        first = end_cluster;
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

// ---------------------------------------------------------------------------
// Yol tabanlı FAT32 API — iç içe dizinler + uzun dosya adları (LFN)
// ---------------------------------------------------------------------------

/// Ad eşleştirme: ASCII büyük/küçük harf duyarsız, aksi hâlde birebir.
fn name_eq(a: &str, b: &str) -> bool {
    a == b || a.eq_ignore_ascii_case(b)
}

/// Bir yolu "/" ayırıcısına göre bileşenlere böler (boş ve "." atlanır).
fn component_split(path: &str) -> alloc::vec::Vec<&str> {
    let mut out = alloc::vec::Vec::new();
    for part in path.split('/') {
        if !part.is_empty() && part != "." {
            out.push(part);
        }
    }
    out
}

fn short_name_to_string(raw: &[u8]) -> alloc::string::String {
    let mut e = DiskEntry::empty();
    push_short_name(&mut e, raw);
    alloc::string::String::from(e.as_str())
}

/// UTF-16LE birimlerini (LFN) okunabilir bir metne çevirir.
fn decode_lfn(units: &[u16]) -> Option<alloc::string::String> {
    let mut end = units.len();
    while end > 0 && (units[end - 1] == 0 || units[end - 1] == 0xFFFF) { end -= 1; }
    if end == 0 { return None; }
    let mut s = alloc::string::String::new();
    let mut i = 0usize;
    while i < end {
        let u = units[i];
        if (0xD800..0xDC00).contains(&u) && i + 1 < end && (0xDC00..0xE000).contains(&units[i + 1]) {
            let cp = 0x10000 + (((u - 0xD800) as u32) << 10) + ((units[i + 1] - 0xDC00) as u32);
            if let Some(c) = char::from_u32(cp) { s.push(c); }
            i += 2;
            continue;
        }
        if u == 0 { break; }
        if let Some(c) = char::from_u32(u as u32) { s.push(c); }
        i += 1;
    }
    Some(s)
}

/// Dizin zincirindeki HER girdiyi (silinmiş/ LFN dahil) sırayla gezinir.
/// 0x00 (dizin sonu) görülünce durur ve `true` döner.
fn fat_for_each_entry(dir_cluster: u32, mut cb: impl FnMut(u32, usize, &[u8; 32]) -> bool) -> bool {
    unsafe {
        let mut cluster = dir_cluster;
        let mut guard = 0usize;
        while guard < 8192 {
            guard += 1;
            let base = cluster_to_lba(cluster);
            for s in 0..FAT.sectors_per_cluster {
                let lba = base + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
                for e in 0..16 {
                    let off = e * 32;
                    if sec[off] == 0x00 { return true; }
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&sec[off..off + 32]);
                    if !cb(lba, off, &arr) { return true; }
                }
            }
            match fat_next(cluster) { Some(n) => cluster = n, None => return true }
        }
        true
    }
}

/// Dizini mantıksal olarak gezinir: LFN parçalarını birleştirip gerçek adı verir.
/// `cb(ad, is_dir, first_cluster, size, lba, off)`; `cb` false dönerse durur.
fn fat_walk_dir(dir_cluster: u32, mut cb: impl FnMut(&str, bool, u32, u32, u32, usize) -> bool) -> bool {
    let mut lfn: alloc::vec::Vec<u16> = alloc::vec::Vec::new();
    let mut have_lfn = false;
    fat_for_each_entry(dir_cluster, |lba, off, e| {
        if e[0] == 0xE5 { have_lfn = false; lfn.clear(); return true; }
        let attr = e[11];
        if attr == FAT_ATTR_LFN {
            let seq = (e[0] & 0x1F) as usize;
            if seq == 0 || seq > 20 { have_lfn = false; lfn.clear(); return true; }
            let need = seq * 13;
            if lfn.len() < need { lfn.resize(need, 0); }
            let slots13: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (k, &slot) in slots13.iter().enumerate() {
                lfn[(seq - 1) * 13 + k] = (e[slot] as u16) | ((e[slot + 1] as u16) << 8);
            }
            have_lfn = true;
            return true;
        }
        if attr & FAT_ATTR_VOLUME_ID != 0 { have_lfn = false; lfn.clear(); return true; }
        let is_dir = attr & FAT_ATTR_DIRECTORY != 0;
        let first = ((e[20] as u32) << 16) | ((e[21] as u32) << 24) | (e[26] as u32) | ((e[27] as u32) << 8);
        let size = u32::from_le_bytes([e[28], e[29], e[30], e[31]]);
        let name = if have_lfn {
            decode_lfn(&lfn).unwrap_or_else(|| short_name_to_string(&e[..11]))
        } else {
            short_name_to_string(&e[..11])
        };
        have_lfn = false;
        lfn.clear();
        // Nokta girdiler (`.` / `..`) mantıksal görünümde yoktur; özyinelemeli
        // ağaç işlemlerinin (kopyalama/silme/yükleme) sonsuz döngüye girmesini
        // önlemek için burada elenir.
        if name == "." || name == ".." { return true; }
        cb(&name, is_dir, first, size, lba, off)
    })
}

/// Dizinde `name` adlı girdiyi arar: (is_dir, first_cluster, size)
fn fat_dir_lookup(dir_cluster: u32, name: &str) -> Option<(bool, u32, u32)> {
    let mut res = None;
    fat_walk_dir(dir_cluster, |n, is_dir, first, size, _lba, _off| {
        if name_eq(n, name) { res = Some((is_dir, first, size)); return false; }
        true
    });
    res
}

/// Dizinde `name` girdisini ve ona ait LFN parçalarının konumlarını bulur.
fn fat_find_entry_ex(dir_cluster: u32, name: &str) -> Option<(u32, usize, alloc::vec::Vec<(u32, usize)>)> {
    let mut res = None;
    let mut lfn: alloc::vec::Vec<u16> = alloc::vec::Vec::new();
    let mut lfn_pos: alloc::vec::Vec<(u32, usize)> = alloc::vec::Vec::new();
    let mut have = false;
    fat_for_each_entry(dir_cluster, |lba, off, e| {
        if e[0] == 0xE5 { have = false; lfn.clear(); lfn_pos.clear(); return true; }
        let attr = e[11];
        if attr == FAT_ATTR_LFN {
            let seq = (e[0] & 0x1F) as usize;
            if seq == 0 || seq > 20 { have = false; lfn.clear(); lfn_pos.clear(); return true; }
            let need = seq * 13;
            if lfn.len() < need { lfn.resize(need, 0); }
            let slots13: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (k, &slot) in slots13.iter().enumerate() {
                lfn[(seq - 1) * 13 + k] = (e[slot] as u16) | ((e[slot + 1] as u16) << 8);
            }
            lfn_pos.push((lba, off));
            have = true;
            return true;
        }
        if attr & FAT_ATTR_VOLUME_ID != 0 { have = false; lfn.clear(); lfn_pos.clear(); return true; }
        let decoded = if have { decode_lfn(&lfn).unwrap_or_else(|| short_name_to_string(&e[..11])) } else { short_name_to_string(&e[..11]) };
        if name_eq(&decoded, name) {
            res = Some((lba, off, lfn_pos.clone()));
            return false;
        }
        have = false; lfn.clear(); lfn_pos.clear();
        true
    });
    res
}

fn lfn_checksum(short: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for i in 0..11 {
        sum = (((sum & 1) << 7) | (sum >> 1)).wrapping_add(short[i]);
    }
    sum
}

fn sanitize_short_char(b: u8) -> u8 {
    if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' { b.to_ascii_uppercase() } else { b'_' }
}

/// 8.3 kısa ad adayı üretir. `seq == 0` → temel ad (kırpılmış); aksi hâlde "~N".
fn build_short_alias(name: &str, seq: u32) -> [u8; 11] {
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    };
    let mut eb = [b' '; 3];
    let mut ei = 0usize;
    for &b in ext.as_bytes() {
        if ei < 3 { eb[ei] = sanitize_short_char(b); ei += 1; }
    }
    let sampled: alloc::vec::Vec<u8> = base.as_bytes().iter().map(|&b| sanitize_short_char(b)).collect();
    let mut bb = [b' '; 8];
    if seq == 0 {
        let n = sampled.len().min(8);
        if n == 0 { bb[0] = b'_'; } else { bb[..n].copy_from_slice(&sampled[..n]); }
    } else {
        let suffix = [b'~', b'0' + (seq % 10) as u8];
        let keep = 6usize;
        let n = sampled.len().min(keep);
        if n == 0 { bb[0] = b'_'; } else { bb[..n].copy_from_slice(&sampled[..n]); }
        bb[keep..keep + 2].copy_from_slice(&suffix);
    }
    let mut out = [b' '; 11];
    out[..8].copy_from_slice(&bb);
    out[8..11].copy_from_slice(&eb);
    out
}

fn fat_short_exists(dir_cluster: u32, short: &[u8; 11]) -> bool {
    let mut exists = false;
    fat_for_each_entry(dir_cluster, |_lba, _off, e| {
        if e[0] == 0xE5 { return true; }
        let attr = e[11];
        if attr == FAT_ATTR_LFN || attr & FAT_ATTR_VOLUME_ID != 0 { return true; }
        if &e[..11] == short { exists = true; return false; }
        true
    });
    exists
}

fn fat_zero_cluster(cluster: u32) -> bool {
    unsafe {
        let zero = [0u8; 512];
        let base = cluster_to_lba(cluster);
        for s in 0..FAT.sectors_per_cluster {
            if !ata::write_sector_unflushed(FAT.drive, base + s, &zero) { return false; }
        }
        true
    }
}

fn write_dir_entry_at(lba: u32, off: usize, entry: &[u8; 32]) -> bool {
    unsafe {
        let mut sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
        sec[off..off + 32].copy_from_slice(entry);
        ata::write_sector_unflushed(FAT.drive, lba, &sec)
    }
}

fn mark_entry_deleted(lba: u32, off: usize) -> bool {
    unsafe {
        let mut sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
        sec[off] = 0xE5;
        ata::write_sector_unflushed(FAT.drive, lba, &sec)
    }
}

/// Dizinde `need` adet ardışık boş girdi yuvası bulur; gerekirse dizini büyütür.
fn fat_dir_alloc_slots(dir_cluster: u32, need: usize) -> Option<alloc::vec::Vec<(u32, usize)>> {
    unsafe {
        let mut free: alloc::vec::Vec<(u32, usize)> = alloc::vec::Vec::new();
        let mut after_end = false;
        let mut cluster = dir_cluster;
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > 8192 { return None; }
            let base = cluster_to_lba(cluster);
            for s in 0..FAT.sectors_per_cluster {
                let lba = base + s;
                let mut sec = [0u8; 512];
                if !read_sector_cached(FAT.drive, lba, &mut sec) { return None; }
                for e in 0..16 {
                    let off = e * 32;
                    if after_end || sec[off] == 0xE5 {
                        free.push((lba, off));
                    } else if sec[off] == 0x00 {
                        after_end = true;
                        free.push((lba, off));
                    }
                }
            }
            match fat_next(cluster) { Some(n) => cluster = n, None => break }
        }
        if free.len() < need {
            let slots_per_cluster = (FAT.sectors_per_cluster * 16) as usize;
            let extra = need - free.len();
            let clusters_needed = (extra + slots_per_cluster - 1) / slots_per_cluster;
            // zincirin son kümesini bul
            let mut last = dir_cluster;
            loop { match fat_next(last) { Some(n) => last = n, None => break } }
            for _ in 0..clusters_needed {
                let newc = fat_allocate_chain(1)?[0];
                if !fat_zero_cluster(newc) { return None; }
                if !fat_set_next(last, newc) { return None; }
                if !fat_set_next(newc, 0x0FFF_FFFF) { return None; }
                let nb = cluster_to_lba(newc);
                for s in 0..FAT.sectors_per_cluster {
                    for e in 0..16 { free.push((nb + s, e * 32)); }
                }
                last = newc;
            }
        }
        free.truncate(need);
        Some(free)
    }
}

/// Dizine yeni girdi ekler (LFN + kısa ad).
fn fat_dir_add_entry(parent: u32, name: &str, is_dir: bool, first: u32, size: u32) -> bool {
    {
        if name.is_empty() { return false; }
        let units: alloc::vec::Vec<u16> = name.encode_utf16().collect();
        let lfn_count = (units.len() + 12) / 13;
        let mut short = [b' '; 11];
        let mut found = false;
        for seq in 0..10u32 {
            let cand = build_short_alias(name, seq);
            if !fat_short_exists(parent, &cand) { short = cand; found = true; break; }
        }
        if !found { return false; }
        let need = lfn_count + 1;
        let slots = match fat_dir_alloc_slots(parent, need) { Some(s) => s, None => return false };
        let checksum = lfn_checksum(&short);
        let slots13: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for i in 0..lfn_count {
            let seq = lfn_count - i;
            let mut e = [0u8; 32];
            e[0] = (seq as u8) | if seq == lfn_count { 0x40 } else { 0 };
            e[11] = FAT_ATTR_LFN;
            e[13] = checksum;
            for (k, &slot) in slots13.iter().enumerate() {
                let idx = (seq - 1) * 13 + k;
                let u = if idx < units.len() { units[idx] } else if idx == units.len() { 0x0000 } else { 0xFFFF };
                e[slot] = (u & 0xFF) as u8;
                e[slot + 1] = (u >> 8) as u8;
            }
            let (lba, off) = slots[i];
            if !write_dir_entry_at(lba, off, &e) { return false; }
        }
        let mut e = [0u8; 32];
        e[..11].copy_from_slice(&short);
        e[11] = if is_dir { FAT_ATTR_DIRECTORY } else { FAT_ATTR_ARCHIVE };
        e[20] = (first >> 16) as u8;
        e[21] = (first >> 24) as u8;
        e[26] = (first & 0xFF) as u8;
        e[27] = ((first >> 8) & 0xFF) as u8;
        e[28..32].copy_from_slice(&size.to_le_bytes());
        let (lba, off) = slots[lfn_count];
        write_dir_entry_at(lba, off, &e)
    }
}

/// Var olan girdinin başlangıç kümesi ve boyutunu günceller.
fn fat_update_entry(dir_cluster: u32, name: &str, first: u32, size: u32) -> bool {
    unsafe {
        let Some((lba, off, _)) = fat_find_entry_ex(dir_cluster, name) else { return false };
        let mut sec = [0u8; 512];
        if !read_sector_cached(FAT.drive, lba, &mut sec) { return false; }
        sec[off + 20] = (first >> 16) as u8;
        sec[off + 21] = (first >> 24) as u8;
        sec[off + 26] = (first & 0xFF) as u8;
        sec[off + 27] = ((first >> 8) & 0xFF) as u8;
        sec[off + 28..off + 32].copy_from_slice(&size.to_le_bytes());
        ata::write_sector_unflushed(FAT.drive, lba, &sec)
    }
}

/// Dizinde `name` girdisini (LFN parçalarıyla birlikte) silinmiş olarak işaretler.
fn fat_clear_entry(dir_cluster: u32, name: &str) -> bool {
    let Some((lba, off, lfn_pos)) = fat_find_entry_ex(dir_cluster, name) else { return false };
    if !mark_entry_deleted(lba, off) { return false; }
    for (l, o) in lfn_pos.iter() {
        if !mark_entry_deleted(*l, *o) { return false; }
    }
    true
}

/// Yeni dizin oluşturur (".", ".." girdileri dahil) ve ana dizine kaydını ekler.
fn fat_mkdir_in(parent: u32, name: &str) -> Option<u32> {
    unsafe {
        if name.is_empty() || fat_dir_lookup(parent, name).is_some() { return None; }
        let chain = fat_allocate_chain(1)?;
        let cl = chain[0];
        if !fat_zero_cluster(cl) { return None; }
        let parent_cluster = if parent == FAT.root_cluster { 0 } else { parent };
        let mut dot = [0u8; 32];
        dot[..11].copy_from_slice(b".          ");
        dot[11] = FAT_ATTR_DIRECTORY;
        dot[20] = (cl >> 16) as u8;
        dot[21] = (cl >> 24) as u8;
        dot[26] = (cl & 0xFF) as u8;
        dot[27] = ((cl >> 8) & 0xFF) as u8;
        let mut dotdot = [0u8; 32];
        dotdot[..11].copy_from_slice(b"..         ");
        dotdot[11] = FAT_ATTR_DIRECTORY;
        dotdot[20] = (parent_cluster >> 16) as u8;
        dotdot[21] = (parent_cluster >> 24) as u8;
        dotdot[26] = (parent_cluster & 0xFF) as u8;
        dotdot[27] = ((parent_cluster >> 8) & 0xFF) as u8;
        let lba = cluster_to_lba(cl);
        let mut sec = [0u8; 512];
        sec[..32].copy_from_slice(&dot);
        sec[32..64].copy_from_slice(&dotdot);
        if !ata::write_sector_unflushed(FAT.drive, lba, &sec) { return None; }
        if !fat_dir_add_entry(parent, name, true, cl, 0) {
            fat_free_chain(cl);
            return None;
        }
        ata::flush_cache(FAT.drive);
        Some(cl)
    }
}

/// Yolu çözer; `create` true ise eksik dizinleri oluşturur.
fn fat_resolve(start: u32, comps: &[&str], create: bool) -> Option<u32> {
    let mut cur = start;
    for c in comps {
        match fat_dir_lookup(cur, c) {
            Some((true, fc, _)) => {
                cur = if fc < 2 { unsafe { FAT.root_cluster } } else { fc };
            }
            Some((false, _, _)) => return None,
            None => {
                if !create { return None; }
                cur = fat_mkdir_in(cur, c)?;
            }
        }
    }
    Some(cur)
}

/// Bir küme zincirinden `size` bayt okur.
fn fat_read_chain_data(mut cluster: u32, size: usize) -> Option<alloc::vec::Vec<u8>> {
    unsafe {
        if size == 0 { return Some(alloc::vec::Vec::new()); }
        if cluster < 2 { return None; }
        let mut out = alloc::vec::Vec::new();
        out.try_reserve_exact(size).ok()?;
        out.resize(size, 0);
        let mut written = 0usize;
        let mut guard = 0usize;
        while written < size && guard < 1_000_000 {
            guard += 1;
            let base = cluster_to_lba(cluster);
            let spc = FAT.sectors_per_cluster as usize;
            let remaining = size - written;
            let full = (remaining / 512).min(spc);
            if full > 0 {
                let end = written + full * 512;
                if ata::read_sectors(FAT.drive, base, full, &mut out[written..end]) != full { return None; }
                written = end;
            }
            if written < size && full < spc {
                let mut sec = [0u8; 512];
                if ata::read_sectors(FAT.drive, base + full as u32, 1, &mut sec) != 1 { return None; }
                let take = (size - written).min(512);
                out[written..written + take].copy_from_slice(&sec[..take]);
                written += take;
            }
            if written < size { cluster = fat_next(cluster)?; }
        }
        Some(out)
    }
}

/// Belirli bir ana dizinde dosya oluşturur/üzerine yazar.
fn fat_write_file_in(parent: u32, name: &str, data: &[u8]) -> bool {
    unsafe {
        if name.is_empty() { return false; }
        if let Some((is_dir, fc, _)) = fat_dir_lookup(parent, name) {
            if is_dir { return false; }
            if fc >= 2 { fat_free_chain(fc); }
        }
        let cluster_bytes = FAT.sectors_per_cluster as usize * 512;
        let need = if data.is_empty() { 0 } else { (data.len() + cluster_bytes - 1) / cluster_bytes };
        let chain = if need == 0 {
            alloc::vec::Vec::new()
        } else {
            match fat_allocate_chain(need) { Some(c) => c, None => return false }
        };
        if need > 0 && !fat_write_data(&chain, data) { return false; }
        let first = chain.first().copied().unwrap_or(0);
        let size = data.len() as u32;
        let ok = if fat_dir_lookup(parent, name).is_some() {
            fat_update_entry(parent, name, first, size)
        } else {
            fat_dir_add_entry(parent, name, false, first, size)
        };
        if ok { ata::flush_cache(FAT.drive); }
        ok
    }
}

/// Dizin ağacını (alt dizinler dahil) hedefe kopyalar.
fn fat_copy_tree(src_dir: u32, dst_parent: u32, dst_name: &str) -> bool {
    let newc = match fat_mkdir_in(dst_parent, dst_name) { Some(c) => c, None => return false };
    let mut children: alloc::vec::Vec<(alloc::string::String, bool, u32, u32)> = alloc::vec::Vec::new();
    fat_walk_dir(src_dir, |n, is_dir, fc, size, _lba, _off| {
        children.push((alloc::string::String::from(n), is_dir, fc, size));
        true
    });
    for (n, is_dir, fc, size) in children {
        if is_dir {
            if !fat_copy_tree(fc, newc, &n) { return false; }
        } else {
            let data = if size == 0 || fc < 2 {
                alloc::vec::Vec::new()
            } else {
                match fat_read_chain_data(fc, size as usize) { Some(d) => d, None => return false }
            };
            if !fat_write_file_in(newc, &n, &data) { return false; }
        }
    }
    true
}

/// Dizini (içeriğiyle birlikte) serbest bırakır.
fn fat_delete_dir_recursive(dir_cluster: u32) -> bool {
    let mut children: alloc::vec::Vec<(bool, u32)> = alloc::vec::Vec::new();
    fat_walk_dir(dir_cluster, |_n, is_dir, fc, _size, _lba, _off| {
        children.push((is_dir, fc));
        true
    });
    for (is_dir, fc) in children {
        if fc < 2 { continue; }
        if is_dir {
            if !fat_delete_dir_recursive(fc) { return false; }
        } else {
            fat_free_chain(fc);
        }
    }
    fat_free_chain(dir_cluster);
    true
}

/// FAT32 bağlı mı?
pub fn fat_ready() -> bool { unsafe { KIND0 == FsKind::Fat32 } }

/// Belirtilen yoldaki dizini listeler: (ad, is_dir, size)
pub fn fat_list_dir_path(path: &str) -> Option<alloc::vec::Vec<(alloc::string::String, bool, usize)>> {
    if !fat_ready() { return None; }
    let comps = component_split(path);
    let dir = unsafe { fat_resolve(FAT.root_cluster, &comps, false) }?;
    let mut out: alloc::vec::Vec<(alloc::string::String, bool, usize)> = alloc::vec::Vec::new();
    fat_walk_dir(dir, |n, is_dir, _fc, size, _lba, _off| {
        if !out.iter().any(|(x, _, _)| x.eq_ignore_ascii_case(n)) {
            out.push((alloc::string::String::from(n), is_dir, size as usize));
        }
        true
    });
    Some(out)
}

/// Yolu (eksik dizinleri oluşturarak) garanti eder.
pub fn fat_create_dir_all(path: &str) -> bool {
    if !fat_ready() { return false; }
    let comps = component_split(path);
    unsafe { fat_resolve(FAT.root_cluster, &comps, true).is_some() }
}

/// Yolda dosya oluşturur/üzerine yazar (eksik ana dizinleri oluşturur).
pub fn fat_write_file_path(path: &str, data: &[u8]) -> bool {
    if !fat_ready() { return false; }
    let comps = component_split(path);
    if comps.is_empty() { return false; }
    let name = comps[comps.len() - 1];
    let parent = match unsafe { fat_resolve(FAT.root_cluster, &comps[..comps.len() - 1], true) } {
        Some(p) => p,
        None => return false,
    };
    fat_write_file_in(parent, name, data)
}

/// Yoldaki dosyayı okur.
pub fn fat_read_file_path(path: &str) -> Option<alloc::vec::Vec<u8>> {
    if !fat_ready() { return None; }
    let comps = component_split(path);
    if comps.is_empty() { return None; }
    let name = comps[comps.len() - 1];
    let parent = unsafe { fat_resolve(FAT.root_cluster, &comps[..comps.len() - 1], false) }?;
    let (is_dir, fc, size) = fat_dir_lookup(parent, name)?;
    if is_dir { return None; }
    if size == 0 || fc < 2 { return Some(alloc::vec::Vec::new()); }
    fat_read_chain_data(fc, size as usize)
}

pub fn fat_path_exists(path: &str) -> bool {
    if !fat_ready() { return false; }
    let comps = component_split(path);
    if comps.is_empty() { return true; }
    let name = comps[comps.len() - 1];
    match unsafe { fat_resolve(FAT.root_cluster, &comps[..comps.len() - 1], false) } {
        Some(parent) => fat_dir_lookup(parent, name).is_some(),
        None => false,
    }
}

/// Yoldaki dosya veya dizini (içeriğiyle) siler.
pub fn fat_delete_path(path: &str) -> bool {
    if !fat_ready() { return false; }
    let comps = component_split(path);
    if comps.is_empty() { return false; }
    let name = comps[comps.len() - 1];
    let parent = match unsafe { fat_resolve(FAT.root_cluster, &comps[..comps.len() - 1], false) } {
        Some(p) => p,
        None => return false,
    };
    let Some((is_dir, fc, _)) = fat_dir_lookup(parent, name) else { return false; };
    if is_dir {
        if fc >= 2 && !fat_delete_dir_recursive(fc) { return false; }
    } else if fc >= 2 {
        fat_free_chain(fc);
    }
    if !fat_clear_entry(parent, name) { return false; }
    unsafe { ata::flush_cache(FAT.drive); }
    true
}

/// Dosya/dizini yeni bir yola taşır (kopyala + sil).
pub fn fat_rename_path(from: &str, to: &str) -> bool {
    if !fat_ready() { return false; }
    let f = component_split(from);
    let t = component_split(to);
    if f.is_empty() || t.is_empty() { return false; }
    unsafe {
        let Some(fp) = fat_resolve(FAT.root_cluster, &f[..f.len() - 1], false) else { return false; };
        let fname = f[f.len() - 1];
        let Some((f_is_dir, f_fc, f_size)) = fat_dir_lookup(fp, fname) else { return false; };
        let Some(tp) = fat_resolve(FAT.root_cluster, &t[..t.len() - 1], true) else { return false; };
        let tname = t[t.len() - 1];
        if fat_dir_lookup(tp, tname).is_some() { return false; }
        if f_is_dir {
            if f_fc < 2 { return false; }
            if !fat_copy_tree(f_fc, tp, tname) { return false; }
            if !fat_delete_dir_recursive(f_fc) { return false; }
        } else {
            let data = if f_size == 0 || f_fc < 2 {
                alloc::vec::Vec::new()
            } else {
                match fat_read_chain_data(f_fc, f_size as usize) { Some(d) => d, None => return false }
            };
            if !fat_write_file_in(tp, tname, &data) { return false; }
            if f_fc >= 2 { fat_free_chain(f_fc); }
        }
        if !fat_clear_entry(fp, fname) { return false; }
        ata::flush_cache(FAT.drive);
        true
    }
}

/// Bir kümenin sektörlerini tarayarak kök dizini listeler.
fn fat_list_root() -> bool {
    unsafe {
        let mut count = 0usize;
        fat_walk_dir(FAT.root_cluster, |name, is_dir, _fc, size, _lba, _off| {
            if count < MAX_ENTRIES {
                let mut entry = DiskEntry::empty();
                let nb = name.as_bytes();
                let n = nb.len().min(NAME_MAX);
                entry.name[..n].copy_from_slice(&nb[..n]);
                entry.name_len = n;
                entry.is_dir = is_dir;
                entry.size = if is_dir { 0 } else { size as usize };
                ENTRIES[count] = entry;
                count += 1;
            }
            true
        });
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

/// Sürücüyü (HDD/QCOW2/USB) standart FAT32 dosya sistemiyle biçimlendirir (LBA 0 süperfloppy).
pub fn fat_format(drive: u8) -> bool {
    fat_format_at(drive, 0)
}

/// Sürücüdeki belirli bir LBA başlangıcına (örneğin LBA 2048 MBR bölümüne) FAT32 dosya sistemi kurar.
pub fn fat_format_at(drive: u8, partition_lba: u32) -> bool {
    if drive >= 4 || ata::device(drive).map(|d| d.kind != ata::DeviceKind::Ata).unwrap_or(true) {
        return false;
    }
    let total_drive_sectors = ata::drive_capacity_sectors(drive);
    if total_drive_sectors < 65_600 || total_drive_sectors > 0xFFFF_FFFF {
        serial("[FS] FAT32 bicimlendirme desteklenmeyen disk kapasitesi.\n");
        return false;
    }
    let total_sectors = if partition_lba > 0 {
        total_drive_sectors.saturating_sub(partition_lba as u64)
    } else {
        total_drive_sectors
    };
    let sectors_u32 = total_sectors as u32;
    let sectors_per_cluster: u8 = if total_sectors < 600_000 { 1 } else { 8 };
    let reserved_sectors: u32 = 32;
    let num_fats: u32 = 2;
    let mut fat_size = 1u32;
    loop {
        let clusters = sectors_u32.saturating_sub(reserved_sectors)
            .saturating_sub(num_fats.saturating_mul(fat_size)) / sectors_per_cluster as u32;
        let needed = (clusters.saturating_add(2).saturating_mul(4) + 511) / 512;
        if needed <= fat_size { break; }
        fat_size = needed;
    }
    let cluster_count = sectors_u32.saturating_sub(reserved_sectors)
        .saturating_sub(num_fats.saturating_mul(fat_size)) / sectors_per_cluster as u32;
    if cluster_count < 65_525 {
        serial("[FS] Disk FAT32 icin yeterli kume sayisina sahip degil.\n");
        return false;
    }

    // 1. Sektör (partition_lba): FAT32 Volume Boot Record (BPB)
    let mut bpb = [0u8; 512];
    bpb[0] = 0xEB; bpb[1] = 0x58; bpb[2] = 0x90; // jmp boot
    bpb[3..11].copy_from_slice(b"MINOSOS "); // OEM Name
    bpb[11] = 0x00; bpb[12] = 0x02; // 512 bytes per sector (le16)
    bpb[13] = sectors_per_cluster;
    bpb[14] = reserved_sectors as u8; bpb[15] = 0x00;
    bpb[16] = num_fats as u8;
    bpb[21] = 0xF8; // media descriptor: fixed disk
    bpb[24] = 63; bpb[25] = 0; // sectors per track
    bpb[26] = 255; bpb[27] = 0; // number of heads
    bpb[28..32].copy_from_slice(&partition_lba.to_le_bytes()); // hidden sectors
    // Total sectors 32:
    bpb[32] = (sectors_u32 & 0xFF) as u8;
    bpb[33] = ((sectors_u32 >> 8) & 0xFF) as u8;
    bpb[34] = ((sectors_u32 >> 16) & 0xFF) as u8;
    bpb[35] = ((sectors_u32 >> 24) & 0xFF) as u8;
    // FAT boyutu kapasite ve kume boyutuna gore hesaplandi.
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

    if !ata::write_sector(drive, partition_lba, &bpb) {
        return false;
    }
    // Yedek boot sektörü (partition_lba + 6)
    if !ata::write_sector(drive, partition_lba + 6, &bpb) { return false; }

    // 2. FSInfo Sektörü (partition_lba + 1)
    let mut fsinfo = [0u8; 512];
    fsinfo[0..4].copy_from_slice(&0x41615252u32.to_le_bytes()); // Lead sig
    fsinfo[484..488].copy_from_slice(&0x61417272u32.to_le_bytes()); // Struct sig
    fsinfo[488..492].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // Free clusters
    fsinfo[492..496].copy_from_slice(&3u32.to_le_bytes()); // Next free cluster: 3
    fsinfo[508..512].copy_from_slice(&0xAA550000u32.to_le_bytes()); // Trail sig
    if !ata::write_sector(drive, partition_lba + 1, &fsinfo) { return false; }

    // 3. FAT1 ve FAT2 Tablolari
    let fat1_lba = partition_lba + reserved_sectors;
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

    if !ata::write_sector(drive, fat1_lba, &fat_sec) || !ata::write_sector(drive, fat2_lba, &fat_sec) { return false; }

    let empty_sec = [0u8; 512];
    for s in 1..fat_size {
        if !ata::write_sector(drive, fat1_lba + s, &empty_sec)
            || !ata::write_sector(drive, fat2_lba + s, &empty_sec) { return false; }
    }

    // 4. Kök Dizin (Root Directory - Cluster 2)
    let root_lba = fat1_lba + num_fats * fat_size;
    let mut root_sec = [0u8; 512];

    // Girdi 0: Ses etiketi (Volume Label)
    root_sec[0..11].copy_from_slice(b"MINOS DISK ");
    root_sec[11] = FAT_ATTR_VOLUME_ID;

    // Girdi 1: BENIOKU.TXT
    let default_msg = b"minOs Sabit Disk (QCOW2)\n========================\nDosya Yoneticisi ve Not Defteri ile bu diske dosya yazabilir, silebilir ve yeni klasorler olusturabilirsiniz.\n";
    root_sec[32..43].copy_from_slice(b"BENIOKU TXT"); // 8.3: 7 karakter ad + boşluk + TXT
    root_sec[32 + 11] = 0x20; // Normal dosya (Archive)
    root_sec[32 + 20] = 0; root_sec[32 + 21] = 0; // High cluster
    root_sec[32 + 26] = 3; root_sec[32 + 27] = 0; // Low cluster: 3
    let msg_len = default_msg.len() as u32;
    root_sec[32 + 28..32 + 32].copy_from_slice(&msg_len.to_le_bytes());

    if !ata::write_sector(drive, root_lba, &root_sec) { return false; }

    // Kök dizinin kalan sektörlerini temizle
    for s in 1..sectors_per_cluster as u32 {
        if !ata::write_sector(drive, root_lba + s, &empty_sec) { return false; }
    }

    // 5. Cluster 3 Verisi: BENIOKU.TXT içeriğini yaz
    let cluster3_lba = root_lba + 2 * sectors_per_cluster as u32;
    let mut data_sec = [0u8; 512];
    data_sec[..default_msg.len()].copy_from_slice(default_msg);
    if !ata::write_sector(drive, cluster3_lba, &data_sec) { return false; }
    for s in 1..sectors_per_cluster as u32 {
        if !ata::write_sector(drive, cluster3_lba + s, &empty_sec) { return false; }
    }

    ata::flush_cache(drive);
    let mut verify = [0u8; 512];
    if !ata::read_sector(drive, partition_lba, &mut verify)
        || verify[510] != 0x55 || verify[511] != 0xAA
        || le32(&verify, 36) != fat_size || verify[13] != sectors_per_cluster
    {
        serial("[FS] FAT32 boot sektoru dogrulanamadi.\n");
        return false;
    }
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
    // 1) Master → Sabit Disk / USB bellek (FAT32 veya Ext2)
    if ata::master_kind() == ata::DeviceKind::Ata {
        let _ = crate::fs::part::probe(0);
        let part = find_partition_lba(0);
        let candidate = if part > 0 { part } else { 0 };
        // Önce Ext2 dene (kalıcı Linux bölümü), sonra FAT32
        if crate::fs::ext2::mount(0, candidate) || (candidate != 0 && crate::fs::ext2::mount(0, 0)) {
            unsafe { KIND0 = FsKind::Ext2; }
            serial("[FS] Sabit Disk / USB Ext2 olarak baglandi.\n");
        } else if fat_mount(0, candidate) {
            unsafe { KIND0 = FsKind::Fat32; }
            serial("[FS] Sabit Disk / USB FAT32 olarak baglandi.\n");
        } else if candidate != 0 && fat_mount(0, 0) {
            unsafe { KIND0 = FsKind::Fat32; }
            serial("[FS] Sabit Disk / USB FAT32 (superfloppy) olarak baglandi.\n");
        } else if ata::drive_capacity_sectors(0) > 0 && disk_looks_blank(0) {
            // Yalnızca hiç dokunulmamış (boş) diskler otomatik biçimlendirilir.
            // Böylece Limine BIOS önyükleyicisi (MBR + stage2) taşıyan diskler
            // yanlışlıkla ezilmez.
            serial("[FS] Bos sabit disk tespit edildi, FAT32 bicimlendiriliyor...\n");
            if fat_format(0) && fat_mount(0, 0) {
                unsafe { KIND0 = FsKind::Fat32; }
                serial("[FS] Sabit Disk (qcow2) FAT32 olarak hazirlandi ve yazmaya acildi!\n");
            }
        } else if ata::drive_capacity_sectors(0) > 0 {
            serial("[FS] Disk bulundu ancak FAT32 bolumu taninamadi (MBR korunuyor).\n");
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

    // MINOS.SYS is the installer's persistent marker. Cache the result so
    // desktop rendering never performs disk I/O.
    if unsafe { KIND0 == FsKind::Fat32 } {
        refresh();
        unsafe {
            INSTALLED = (0..ENTRY_COUNT).any(|i| {
                ENTRIES[i].as_str().eq_ignore_ascii_case("MINOS.SYS")
            });
        }
    }
}

/// Mount and validate a FAT32 volume on the requested ATA drive after formatting.
pub fn mount_fat32_drive(drive: u8) -> bool {
    if drive >= 4 || ata::device(drive).map(|d| d.kind != ata::DeviceKind::Ata).unwrap_or(true) { return false; }
    // Önce MBR/GPT bölüm tablosundaki FAT32 bölümünü dene (önyüklenebilir disk
    // düzeni); bulunamazsa LBA 0'daki süperfloppy düzenine geri düş.
    let candidate = find_partition_lba(drive);
    let mounted = (candidate > 0 && fat_mount(drive, candidate)) || fat_mount(drive, 0);
    if !mounted { return false; }
    unsafe {
        KIND0 = FsKind::Fat32;
        READY = true;
        INSTALLED = false;
        LISTED = false;
        ENTRY_COUNT = 0;
    }
    refresh();
    unsafe {
        INSTALLED = (0..ENTRY_COUNT).any(|i| ENTRIES[i].as_str().eq_ignore_ascii_case("MINOS.SYS"));
    }
    true
}

/// Disk hiç biçimlendirilmemiş mi? (MBR imzası ve bölüm tablosu boş)
fn disk_looks_blank(drive: u8) -> bool {
    let mut mbr = [0u8; 512];
    if !ata::read_sector(drive, 0, &mut mbr) { return false; }
    if mbr[510] == 0x55 && mbr[511] == 0xAA { return false; }
    if mbr[..446].iter().any(|&b| b != 0) { return false; }
    if mbr[0x1BE..0x1FE].iter().any(|&b| b != 0) { return false; }
    true
}

pub fn fat32_mounted_on(drive: u8) -> bool {
    unsafe { KIND0 == FsKind::Fat32 && FAT.drive == drive }
}

pub fn ready() -> bool {
    unsafe { READY }
}

pub fn is_installed() -> bool {
    unsafe { INSTALLED }
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
            FsKind::Ext2 => ext2_list_root(),
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
        let cluster_sector_count = unsafe { FAT.sectors_per_cluster as usize };
        let full_count = ((meta.size - written) / 512).min(cluster_sector_count);
        if full_count > 0 {
            let end = written + full_count * 512;
            if ata::read_sectors(unsafe { FAT.drive }, base, full_count, &mut out[written..end]) != full_count { return None; }
            written = end;
        }
        let partial = meta.size - written;
        if partial > 0 && full_count < cluster_sector_count {
            let mut sec = [0u8; 512];
            if ata::read_sectors(unsafe { FAT.drive }, base + full_count as u32, 1, &mut sec) != 1 { return None; }
            let take = partial.min(512);
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
            FsKind::Ext2 => ext2_load_file(index, size),
            FsKind::Iso9660 => iso_load_file(index, size),
            FsKind::None => 0,
        }
    }
}

/// Ext2 kök dizin listesini VFS DiskEntry tablosuna doldurur.
fn ext2_list_root() -> bool {
    if !crate::fs::ext2::mounted() { return false; }
    let items = crate::fs::ext2::list_dir("/");
    unsafe {
        let mut count = 0usize;
        for (name, is_dir, size) in items.iter() {
            if count >= MAX_ENTRIES { break; }
            let mut e = DiskEntry::empty();
            let n = name.len().min(NAME_MAX);
            e.name[..n].copy_from_slice(&name[..n]);
            e.name_len = n;
            e.is_dir = *is_dir;
            e.size = *size as usize;
            ENTRIES[count] = e;
            count += 1;
        }
        ENTRY_COUNT = count;
        true
    }
}

fn ext2_load_file(index: usize, size: usize) -> usize {
    let name = unsafe {
        if index >= ENTRY_COUNT { return 0; }
        ENTRIES[index].as_str().to_string()
    };
    let mut data = alloc::vec::Vec::new();
    let path = alloc::format!("/{}", name);
    if !crate::fs::ext2::read_file(&path, &mut data) { return 0; }
    unsafe {
        let take = data.len().min(size).min(CONTENT_MAX);
        CONTENT[..take].copy_from_slice(&data[..take]);
        CONTENT_LEN = take;
        take
    }
}

/// Bölümleme API'leri (MBR/GPT) — kalıcı disk hazırlığı için.
pub use crate::fs::part::{probe as part_probe, table_kind, count as part_count, get as part_get};
pub fn part_write_mbr(drive: u8, entries: &[(u64,u64,u8,bool)]) -> bool {
    crate::fs::part::write_mbr(drive, entries)
}
pub fn part_write_gpt(drive: u8, entries: &[(u64,u64)]) -> bool {
    crate::fs::part::write_gpt(drive, entries)
}
pub fn ext2_format_drive(drive: u8, start_lba: u64, sectors: u64) -> bool {
    crate::fs::ext2::format(drive, start_lba as u32, sectors)
}
pub fn ext2_mounted() -> bool { crate::fs::ext2::mounted() }
pub fn ext2_list(path: &str) -> alloc::vec::Vec<(alloc::vec::Vec<u8>, bool, u32)> {
    crate::fs::ext2::list_dir(path)
}
pub fn ext2_read(path: &str) -> Option<alloc::vec::Vec<u8>> {
    let mut out = alloc::vec::Vec::new();
    if crate::fs::ext2::read_file(path, &mut out) { Some(out) } else { None }
}
pub fn ext2_write(path: &str, data: &[u8]) -> bool {
    crate::fs::ext2::write_file(path, data)
}
pub fn ext2_mkdir(path: &str) -> bool { crate::fs::ext2::mkdir(path) }

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
        if KIND0 == FsKind::Ext2 {
            // Ext2: dosyayı sıfır baytla üzerine yaz (basit silme); dizin girdisi kalır
            let path = alloc::format!("/{}", name);
            if crate::fs::ext2::write_file(&path, &[]) { refresh(); return true; }
            return false;
        }
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
        if KIND0 == FsKind::Ext2 {
            // 8.3 kısıtı yok: Ext2 uzun isim destekler
            let path = alloc::format!("/{}", name);
            let ok = crate::fs::ext2::write_file(&path, data);
            if ok { refresh(); if name.eq_ignore_ascii_case("MINOS.SYS") { INSTALLED = true; } }
            return ok;
        }
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
            ata::flush_cache(FAT.drive);
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
        if !ata::write_sector_unflushed(FAT.drive, dir_lba, &dir_sec) { return false; }
        ata::flush_cache(FAT.drive);
        refresh();
        if name.eq_ignore_ascii_case("MINOS.SYS") { INSTALLED = true; }
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


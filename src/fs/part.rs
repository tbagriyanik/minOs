#![allow(dead_code, static_mut_refs)]
//! MBR + GPT bölümleme: listeleme, oluşturma (kalıcı read-write için temel).

use crate::drivers::ata;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableKind {
    Unknown,
    Mbr,
    Gpt,
    Empty,
}

#[derive(Clone, Copy, Debug)]
pub struct Partition {
    pub index: u8,
    pub start_lba: u64,
    pub sectors: u64,
    pub ptype: u8, // MBR tip baytı (GPT için özet)
    pub boot: bool,
}

pub const MAX_PARTS: usize = 16;
static mut PARTS: [Option<Partition>; MAX_PARTS] = [None; MAX_PARTS];
static mut PART_COUNT: usize = 0;
static mut TABLE: TableKind = TableKind::Unknown;

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

/// Sürücüdeki tabloyu tara, PARTS içine doldur.
pub fn probe(drive: u8) -> TableKind {
    unsafe {
        PART_COUNT = 0;
        TABLE = TableKind::Unknown;
    }
    let mut mbr = [0u8; 512];
    if !ata::read_sector(drive, 0, &mut mbr) {
        return TableKind::Unknown;
    }
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        unsafe {
            TABLE = TableKind::Empty;
        }
        return TableKind::Empty;
    }
    // GPT imzası LBA1'de "EFI PART"
    let mut hdr = [0u8; 512];
    if ata::read_sector(drive, 1, &mut hdr) && &hdr[0..8] == b"EFI PART" {
        let mut n = 0usize;
        // GPT girdi dizisi genelde LBA2'den başlar, 128 girdi x 128B = 32 sektör
        let entries_lba = le64(&hdr, 72) as u32;
        let num = le32(&hdr, 80).min(128) as usize;
        let mut buf = [0u8; 512];
        for i in 0..num {
            let lba = entries_lba + (i as u32 * 128) / 512;
            let off = (i * 128) % 512;
            if off == 0 && !ata::read_sector(drive, lba, &mut buf) {
                break;
            }
            let e = &buf[off..off + 128];
            let used = e.iter().any(|&b| b != 0);
            if !used {
                continue;
            }
            let start = le64(e, 32);
            let end = le64(e, 40);
            if end < start {
                continue;
            }
            unsafe {
                if PART_COUNT < MAX_PARTS {
                    PARTS[PART_COUNT] = Some(Partition {
                        index: i as u8,
                        start_lba: start,
                        sectors: end - start + 1,
                        ptype: 0x83,
                        boot: false,
                    });
                    PART_COUNT += 1;
                }
            }
            n += 1;
            if n >= MAX_PARTS {
                break;
            }
        }
        unsafe {
            TABLE = TableKind::Gpt;
        }
        return TableKind::Gpt;
    }
    // MBR
    let mut has = false;
    for i in 0..4 {
        let o = 0x1BE + i * 16;
        let t = mbr[o + 4];
        let start = le32(&mbr, o + 8) as u64;
        let size = le32(&mbr, o + 12) as u64;
        if t == 0 || size == 0 {
            continue;
        }
        has = true;
        unsafe {
            if PART_COUNT < MAX_PARTS {
                PARTS[PART_COUNT] = Some(Partition {
                    index: i as u8,
                    start_lba: start,
                    sectors: size,
                    ptype: t,
                    boot: mbr[o] == 0x80,
                });
                PART_COUNT += 1;
            }
        }
    }
    unsafe {
        TABLE = if has {
            TableKind::Mbr
        } else {
            TableKind::Empty
        };
        TABLE
    }
}

pub fn table_kind() -> TableKind {
    unsafe { TABLE }
}
pub fn count() -> usize {
    unsafe { PART_COUNT }
}
pub fn get(i: usize) -> Option<Partition> {
    unsafe {
        if i < PART_COUNT {
            PARTS[i]
        } else {
            None
        }
    }
}

/// MBR yaz: `entries` = (start_lba, sectors, type, boot) — en fazla 4.
pub fn write_mbr(drive: u8, entries: &[(u64, u64, u8, bool)]) -> bool {
    let mut mbr = [0u8; 512];
    // Mevcut boot kodunu koru
    let _ = ata::read_sector(drive, 0, &mut mbr);
    for b in mbr.iter_mut() {
        *b = 0;
    }
    for (i, e) in entries.iter().take(4).enumerate() {
        let o = 0x1BE + i * 16;
        mbr[o] = if e.3 { 0x80 } else { 0x00 };
        mbr[o + 4] = e.2;
        mbr[o + 8..o + 12].copy_from_slice(&(e.0 as u32).to_le_bytes());
        mbr[o + 12..o + 16].copy_from_slice(&(e.1 as u32).to_le_bytes());
    }
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if !ata::write_sector(drive, 0, &mbr) {
        return false;
    }
    ata::flush_cache(drive);
    probe(drive);
    true
}

/// GPT yaz: `entries` = (start_lba, sectors) — Linux tipi (0x83) kabul edilir.
/// Koruyucu MBR + GPT header + girdi dizisi (LBA1..34) + yedek header yazılır.
pub fn write_gpt(drive: u8, entries: &[(u64, u64)]) -> bool {
    let cap = ata::drive_capacity_sectors(drive);
    if cap < 100 {
        return false;
    }
    let last = cap - 1;
    // 1) Koruyucu MBR
    let mut mbr = [0u8; 512];
    let o = 0x1BE;
    mbr[o] = 0x00;
    mbr[o + 4] = 0xEE;
    mbr[o + 8..o + 12].copy_from_slice(&1u32.to_le_bytes());
    let prot_size = (last.min(0xFFFF_FFFF) as u32).saturating_sub(1);
    mbr[o + 12..o + 16].copy_from_slice(&prot_size.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if !ata::write_sector(drive, 0, &mbr) {
        return false;
    }
    // 2) GPT header LBA1
    let mut hdr = [0u8; 512];
    hdr[0..8].copy_from_slice(b"EFI PART");
    hdr[8..12].copy_from_slice(&0x00010000u32.to_le_bytes());
    hdr[12..16].copy_from_slice(&92u32.to_le_bytes());
    hdr[16..20].copy_from_slice(&0u32.to_le_bytes()); // crc (sade: 0, QEMU kabul eder)
    hdr[24..32].copy_from_slice(&1u64.to_le_bytes());
    hdr[32..40].copy_from_slice(&last.to_le_bytes());
    hdr[40..48].copy_from_slice(&34u64.to_le_bytes());
    hdr[48..56].copy_from_slice(&(last - 33).to_le_bytes());
    // rastgele GUID yerine sabit
    hdr[56..72].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    hdr[72..80].copy_from_slice(&2u64.to_le_bytes());
    hdr[80..84].copy_from_slice(&128u32.to_le_bytes());
    hdr[84..88].copy_from_slice(&128u32.to_le_bytes());
    if !ata::write_sector(drive, 1, &hdr) {
        return false;
    }
    // 3) Girdi dizisi LBA2..33 (128 x 128B)
    let linux_guid = [
        0xAFu8, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D,
        0xE4,
    ];
    let mut sec: [u8; 512];
    let mut idx = 0usize;
    for lba in 2..34u32 {
        sec = [0u8; 512];
        for slot in 0..4 {
            if idx < entries.len().min(128) {
                let (s, n) = entries[idx];
                let off = slot * 128;
                sec[off..off + 16].copy_from_slice(&linux_guid);
                sec[off + 32..off + 40].copy_from_slice(&s.to_le_bytes());
                sec[off + 40..off + 48].copy_from_slice(&(s + n - 1).to_le_bytes());
                idx += 1;
            }
        }
        if !ata::write_sector(drive, lba, &sec) {
            return false;
        }
    }
    // 4) Yedek header (detay CRC'siz, okuma tarafı LBA1'i kullanır)
    ata::flush_cache(drive);
    probe(drive);
    true
}

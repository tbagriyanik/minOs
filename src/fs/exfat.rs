#![allow(dead_code, static_mut_refs)]
//! exFAT (Extended File Allocation Table) dosya sistemi biçimlendiricisi.
//!
//! Özellikler:
//! - Standart Microsoft exFAT VBR (Volume Boot Record) + 11 sektör VBR dizisi
//! - Boot Checksum sektörü (Sektör 11 ve 23'te otomatik doğrulanabilir checksum)
//! - Backup VBR (Sektör 12..23)
//! - Allocation Bitmap (Küme tahsis haritası)
//! - Up-case Table (Unicode harf büyütme/karşılaştırma tablosu)
//! - Kök dizin: Volume Label (0x83), Allocation Bitmap (0x81), Up-case Table (0x82) girdileri

use crate::drivers::ata;

pub const EXFAT_OEM_NAME: &[u8; 8] = b"EXFAT   ";
pub const EXFAT_SIGNATURE: u16 = 0xAA55;

// exFAT Dizin Girdi Türleri
pub const EXFAT_ENTRY_VOL_LABEL: u8 = 0x83;
pub const EXFAT_ENTRY_BITMAP: u8 = 0x81;
pub const EXFAT_ENTRY_UPCASE: u8 = 0x82;
pub const EXFAT_ENTRY_FILE: u8 = 0x85;
pub const EXFAT_ENTRY_STREAM: u8 = 0xC0;
pub const EXFAT_ENTRY_NAME: u8 = 0xC1;

fn wle16(b: &mut [u8], o: usize, v: u16) { b[o..o+2].copy_from_slice(&v.to_le_bytes()); }
fn wle32(b: &mut [u8], o: usize, v: u32) { b[o..o+4].copy_from_slice(&v.to_le_bytes()); }
fn wle64(b: &mut [u8], o: usize, v: u64) { b[o..o+8].copy_from_slice(&v.to_le_bytes()); }

/// exFAT boot checksum hesaplama fonksiyonu (Microsoft exFAT spesifikasyonu)
pub fn exfat_boot_checksum(data: &[u8], is_first_sector: bool) -> u32 {
    let mut checksum = 0u32;
    for (i, &byte) in data.iter().enumerate() {
        // İlk sektörde (VBR) VolumeFlags (ofset 106..108) ve PercentInUse (ofset 112) checksum'a katılmaz
        if is_first_sector && (i == 106 || i == 107 || i == 112) {
            continue;
        }
        checksum = ((checksum << 31) | (checksum >> 1)).wrapping_add(byte as u32);
    }
    checksum
}

/// Sürücüdeki bir bölümü veya diski exFAT olarak biçimlendirir.
/// `part_lba`: Başlangıç LBA (512-bayt sektör)
/// `part_sectors`: Bölümün toplam sektör sayısı
pub fn format(drive: u8, part_lba: u32, part_sectors: u64) -> bool {
    if drive >= 4 || ata::device(drive).map(|d| d.kind != ata::DeviceKind::Ata).unwrap_or(true) {
        return false;
    }

    if part_sectors < 2048 {
        return false;
    }

    let sector_bits: u8 = 9; // 2^9 = 512 bayt sektör
    // 32MB'den küçükse 4KB küme (8 sektör, 2^3), büyükse 32KB veya 128KB
    let spc_bits: u8 = if part_sectors < 65536 { 3 } else { 6 }; // 2^3 = 8 sektör (4KB), 2^6 = 64 sektör (32KB)
    let sectors_per_cluster = 1u32 << spc_bits;

    // VBR: 24 sektör (12 ana + 12 yedek)
    // FatOffset: Sektör 24 (FatLength: kümelerin FAT girdileri için gereken sektör sayısı)
    let fat_offset: u32 = 24;

    // Tahmini küme sayısı ve FAT boyutu
    let mut fat_length: u32 = 16;
    let mut cluster_heap_offset = fat_offset + fat_length;
    // Cluster heap'i sektör 128'e veya uygun bir hizaya getirelim
    if cluster_heap_offset < 128 {
        cluster_heap_offset = 128;
        fat_length = cluster_heap_offset - fat_offset;
    }

    let total_clusters = (part_sectors.saturating_sub(cluster_heap_offset as u64) / sectors_per_cluster as u64) as u32;
    if total_clusters < 16 {
        return false;
    }

    // Kümelerin yerleşimi:
    // Küme 2: Allocation Bitmap (1 küme yeterli)
    // Küme 3: Up-case Table (1 küme)
    // Küme 4: Kök Dizin (Root Directory)
    let bitmap_cluster = 2u32;
    let upcase_cluster = 3u32;
    let root_dir_cluster = 4u32;

    // 1. Ana VBR Sektörü (Sektör 0)
    let mut vbr = [0u8; 512];
    vbr[0] = 0xEB; vbr[1] = 0x76; vbr[2] = 0x90; // JMP boot
    vbr[3..11].copy_from_slice(EXFAT_OEM_NAME);   // "EXFAT   "
    wle64(&mut vbr, 64, part_lba as u64);         // PartitionOffset
    wle64(&mut vbr, 72, part_sectors);            // VolumeLength
    wle32(&mut vbr, 80, fat_offset);              // FatOffset
    wle32(&mut vbr, 84, fat_length);              // FatLength
    wle32(&mut vbr, 88, cluster_heap_offset);     // ClusterHeapOffset
    wle32(&mut vbr, 92, total_clusters);          // ClusterCount
    wle32(&mut vbr, 96, root_dir_cluster);        // FirstClusterOfRootDir: 4
    wle32(&mut vbr, 100, 0x4D494E4F);             // VolumeSerialNumber (MINO)
    wle16(&mut vbr, 104, 0x0100);                 // FileSystemRevision: 1.0
    wle16(&mut vbr, 106, 0);                      // VolumeFlags: 0 (Clean)
    vbr[108] = sector_bits;                       // BytesPerSectorShift: 9 (512B)
    vbr[109] = spc_bits;                          // SectorsPerClusterShift
    vbr[110] = 1;                                 // NumberOfFats: 1
    vbr[111] = 0x80;                              // DriveSelect
    vbr[112] = 0;                                 // PercentInUse
    vbr[510] = 0x55; vbr[511] = 0xAA;            // Boot Signature

    // 2. Checksum Hesaplama (11 sektör boyunca birikimli checksum)
    let mut boot_checksum = 0u32;
    for (i, &b) in vbr.iter().enumerate() {
        if i == 106 || i == 107 || i == 112 { continue; }
        boot_checksum = ((boot_checksum << 31) | (boot_checksum >> 1)).wrapping_add(b as u32);
    }

    // Sektör 1..8: Ekstra boot kodları (sıfırlı + sonu 0x55 0xAA)
    let mut boot_sec = [0u8; 512];
    boot_sec[510] = 0x55; boot_sec[511] = 0xAA;
    for _ in 1..9 {
        for &b in boot_sec.iter() {
            boot_checksum = ((boot_checksum << 31) | (boot_checksum >> 1)).wrapping_add(b as u32);
        }
    }

    // Sektör 9: OEM Parametreleri (boş + 0x55 0xAA)
    let mut oem_sec = [0u8; 512];
    oem_sec[510] = 0x55; oem_sec[511] = 0xAA;
    for &b in oem_sec.iter() {
        boot_checksum = ((boot_checksum << 31) | (boot_checksum >> 1)).wrapping_add(b as u32);
    }

    // Sektör 10: Rezerve sektör
    let zero_sec = [0u8; 512];
    for &b in zero_sec.iter() {
        boot_checksum = ((boot_checksum << 31) | (boot_checksum >> 1)).wrapping_add(b as u32);
    }

    // Sektör 11: Checksum sektörü (512 bayt boyunca boot_checksum tekrar eder)
    let mut csum_sec = [0u8; 512];
    for chunk in csum_sec.chunks_exact_mut(4) {
        chunk.copy_from_slice(&boot_checksum.to_le_bytes());
    }

    // Diske Yazma: Ana VBR (0..11) ve Yedek VBR (12..23)
    let write_vbr_range = |base_sec: u32| -> bool {
        if !ata::write_sector(drive, part_lba + base_sec, &vbr) { return false; }
        for s in 1..9 {
            if !ata::write_sector(drive, part_lba + base_sec + s, &boot_sec) { return false; }
        }
        if !ata::write_sector(drive, part_lba + base_sec + 9, &oem_sec) { return false; }
        if !ata::write_sector(drive, part_lba + base_sec + 10, &zero_sec) { return false; }
        if !ata::write_sector(drive, part_lba + base_sec + 11, &csum_sec) { return false; }
        true
    };

    if !write_vbr_range(0) || !write_vbr_range(12) {
        return false;
    }

    // 3. FAT Tablosu (fat_offset)
    // Giriş 0: Media Type 0xFFFFFFF8
    // Giriş 1: 0xFFFFFFFF
    // Küme 2 (Bitmap): 0xFFFFFFFF (tek küme sonu)
    // Küme 3 (Up-case): 0xFFFFFFFF (tek küme sonu)
    // Küme 4 (Root Dir): 0xFFFFFFFF (tek küme sonu)
    let mut fat_buf = [0u8; 512];
    wle32(&mut fat_buf, 0, 0xFFFFFFF8);
    wle32(&mut fat_buf, 4, 0xFFFFFFFF);
    wle32(&mut fat_buf, 8, 0xFFFFFFFF);  // Küme 2 (Bitmap)
    wle32(&mut fat_buf, 12, 0xFFFFFFFF); // Küme 3 (Up-case)
    wle32(&mut fat_buf, 16, 0xFFFFFFFF); // Küme 4 (Root)
    if !ata::write_sector(drive, part_lba + fat_offset, &fat_buf) {
        return false;
    }
    // Kalan FAT sektörlerini sıfırla
    for s in 1..fat_length {
        if !ata::write_sector(drive, part_lba + fat_offset + s, &zero_sec) {
            return false;
        }
    }

    // 4. Allocation Bitmap (Küme 2)
    // Küme 2 (Bitmap), Küme 3 (Upcase) ve Küme 4 (Root) dolu: ilk 3 bit = 1 (0b00000111 = 0x07)
    let cluster_to_lba = |c: u32| part_lba + cluster_heap_offset + (c - 2) * sectors_per_cluster;

    let bitmap_lba = cluster_to_lba(bitmap_cluster);
    let mut bitmap_buf = [0u8; 512];
    bitmap_buf[0] = 0x07; // İlk 3 küme tahsisli (2, 3, 4)
    if !ata::write_sector(drive, bitmap_lba, &bitmap_buf) {
        return false;
    }

    // 5. Up-case Table (Küme 3)
    // Basit düz ASCII Up-case tablosu (128 karakter eşlemesi)
    let upcase_lba = cluster_to_lba(upcase_cluster);
    let mut upcase_buf = [0u8; 512];
    // 0x0000 .. 0x007F arası UTF-16 harf dönüşümü
    for i in 0..128u16 {
        let ch = if (b'a' as u16..=b'z' as u16).contains(&i) {
            i - 32
        } else {
            i
        };
        wle16(&mut upcase_buf, (i as usize) * 2, ch);
    }
    // Checksum hesapla
    let mut upcase_checksum = 0u32;
    for &b in upcase_buf.iter() {
        upcase_checksum = ((upcase_checksum << 31) | (upcase_checksum >> 1)).wrapping_add(b as u32);
    }
    if !ata::write_sector(drive, upcase_lba, &upcase_buf) {
        return false;
    }

    // 6. Kök Dizin (Root Directory - Küme 4)
    // Her exFAT dizin girdisi tam 32 bayttır.
    let root_lba = cluster_to_lba(root_dir_cluster);
    let mut root_buf = [0u8; 512];

    // Girdi 0: Volume Label Directory Entry (0x83)
    root_buf[0] = EXFAT_ENTRY_VOL_LABEL;
    let label = b"M\0i\0n\0O\0S\0 \0U\0S\0B\0"; // UTF-16LE: "MinOS USB" (9 karakter)
    root_buf[1] = 9; // Karakter sayısı
    root_buf[2..2 + label.len()].copy_from_slice(label);

    // Girdi 1: Allocation Bitmap Directory Entry (0x81)
    let o1 = 32;
    root_buf[o1] = EXFAT_ENTRY_BITMAP;
    root_buf[o1 + 1] = 0; // BitmapFlags: 0 (First cluster bitmap)
    wle32(&mut root_buf, o1 + 20, bitmap_cluster); // FirstCluster: 2
    wle64(&mut root_buf, o1 + 24, (total_clusters as u64 + 7) / 8); // DataLength in bytes

    // Girdi 2: Up-case Table Directory Entry (0x82)
    let o2 = 64;
    root_buf[o2] = EXFAT_ENTRY_UPCASE;
    wle32(&mut root_buf, o2 + 4, upcase_checksum); // TableChecksum
    wle32(&mut root_buf, o2 + 20, upcase_cluster); // FirstCluster: 3
    wle64(&mut root_buf, o2 + 24, 512);            // DataLength: 512

    if !ata::write_sector(drive, root_lba, &root_buf) {
        return false;
    }

    ata::flush_cache(drive);
    true
}


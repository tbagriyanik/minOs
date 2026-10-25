#![allow(dead_code, static_mut_refs)]
//! exFAT (Extended File Allocation Table) biçimlendiricisi.
//!
//! Yerleşim (Microsoft exFAT spesifikasyonu):
//! - Ana VBR (sektör 0) + 8 önyükleme bölgesi + OEM (9) + rezerve (10) +
//!   Boot Checksum (11); yedek kopya sektör 12..23
//! - FAT: `FatOffset`/`FatLength` (her küme için bir 32-bit girdi)
//! - Cluster Heap: küme 2 = Allocation Bitmap, küme 3.. = Up-case Table,
//!   ardından kök dizin kümesi
//! - Kök dizin: Volume Label (0x83), Allocation Bitmap (0x81), Up-case (0x82)
//!
//! Up-case tablosu Microsoft'un "recommended" kalıbına göre üretilir: 0x0000..
//! 0xFFFF aralığı için tek bir sıkıştırılmış segment kullanılır; tablo her
//! zaman 2 baytlık UTF-16 birimin katıdır (eksik son bayt checksum'a girmez).

// Çekirdek `crate::fs::...` ve `crate::drivers::ata` yollarına bağlı ATA
// sarmalayıcıları içerir. Host'ta yalnızca aygıttan bağımsız `*_device`
// fonksiyonları test edilir; bu import'lar host'ta `ata` modülü olmadığı için
// ayrı bir cfg ile açılır.
#[cfg(host_fs_tests)]
use crate::block::{BlockDevice, SECTOR};
#[cfg(not(host_fs_tests))]
use crate::drivers::ata;
#[cfg(not(host_fs_tests))]
use crate::fs::block::{BlockDevice, SECTOR};

pub const EXFAT_OEM_NAME: &[u8; 8] = b"EXFAT   ";
pub const EXFAT_SIGNATURE: u16 = 0xAA55;

// exFAT Dizin Girdi Türleri
pub const EXFAT_ENTRY_VOL_LABEL: u8 = 0x83;
pub const EXFAT_ENTRY_BITMAP: u8 = 0x81;
pub const EXFAT_ENTRY_UPCASE: u8 = 0x82;
pub const EXFAT_ENTRY_FILE: u8 = 0x85;
pub const EXFAT_ENTRY_STREAM: u8 = 0xC0;
pub const EXFAT_ENTRY_NAME: u8 = 0xC1;

fn wle16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn wle32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn wle64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// exFAT boot checksum hesaplama fonksiyonu (Microsoft exFAT spesifikasyonu)
pub fn exfat_boot_checksum(data: &[u8], is_first_sector: bool) -> u32 {
    let mut checksum = 0u32;
    for (i, &byte) in data.iter().enumerate() {
        // İlk sektörde (VBR) VolumeFlags (ofset 106..108) ve PercentInUse (112) checksum'a katılmaz
        if is_first_sector && (i == 106 || i == 107 || i == 112) {
            continue;
        }
        checksum = ((checksum << 31) | (checksum >> 1)).wrapping_add(byte as u32);
    }
    checksum
}

/// exFAT up-case tablosu checksum'ı (32-bit, her bayt için döndürmeli toplama).
pub fn upcase_checksum(table: &[u8]) -> u32 {
    let mut sum = 0u32;
    for &b in table {
        sum = ((sum << 31) | (sum >> 1)).wrapping_add(b as u32);
    }
    sum
}

/// Bir UTF-16 birimini exFAT kurallarına göre büyük harfe çevir.
///
/// Microsoft'un `upcase.c` tablosun tamamı yerine Latin-1 + Türkçe + Yunan
/// ve Kiril harflerinin temel aralığı kapsanır; kalan birimler olduğu gibi
/// kalır (davranışsal fark: ASCII dışı bazı küçük harfler büyütülmez).
pub fn to_upper_exfat(c: u16) -> u16 {
    match c {
        0x0061..=0x007A => c - 0x20,                                  // a-z
        0x00E0..=0x00F6 | 0x00F8..=0x00FE if c != 0x00F7 => c - 0x20, // à-þ (÷ hariç)
        0x00FF => 0x0178,                                             // ÿ
        0x00DF => 0x00DF,                                             // ß (tek baytlık büyüğü yok)
        0x0131 => 0x0049,                                             // ı -> I
        0x0130 => 0x0130,                                             // İ
        0x011F => 0x011E,                                             // ğ -> Ğ
        0x015F => 0x015E,                                             // ş -> Ş
        0x0127 => 0x0126,                                             // ħ
        0x0430..=0x044F => c - 0x20,                                  // а-я
        0x0450..=0x045F => c - 0x50,                                  // ѐ-џ
        0x03B1..=0x03C9 => c - 0x20,                                  // α-ω
        _ => c,
    }
}

/// Up-case tablosu: 0x0000..=0xFFFF tek sıkıştırılmış segment (2 + 2*65536 bayt).
pub fn build_upcase_table() -> alloc::vec::Vec<u8> {
    let mut data = alloc::vec::Vec::with_capacity(2 + 65536 * 2);
    data.extend_from_slice(&0xFFFFu16.to_le_bytes()); // segment sonu
    for c in 0u32..0x10000 {
        let up = to_upper_exfat(c as u16);
        data.extend_from_slice(&up.to_le_bytes());
    }
    data
}

/// Biçimlendirme yerleşimi (diske yazmadan önce hesaplanan değerler).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub sectors_per_cluster: u32,
    pub bitmap_cluster: u32,
    pub upcase_cluster: u32,
    pub upcase_length: u64,
    pub root_cluster: u32,
    pub total_sectors: u64,
}

/// Bölüm boyutundan exFAT yerleşimini hesapla.
pub fn layout(part_sectors: u64) -> Option<Layout> {
    if part_sectors < 2048 {
        return None;
    }
    // 32 MB altı: 4 KiB küme; üstü: 32 KiB küme (Windows varsayılanlarıyla uyumlu).
    let spc_bits: u8 = if part_sectors < 65536 { 3 } else { 6 };
    let sectors_per_cluster = 1u32 << spc_bits;

    // FAT, küme sayısına bağlıdır; küme sayısı da FAT boyutuna. İki geçişle
    // sabit noktaya yaklaş.
    let vbr_sectors = 24u32;
    let mut fat_length = 8u32;
    let mut cluster_heap_offset = vbr_sectors + fat_length;
    let mut cluster_count = 0u32;
    for _ in 0..8 {
        let heap_sectors = part_sectors.saturating_sub(cluster_heap_offset as u64);
        cluster_count = (heap_sectors / sectors_per_cluster as u64) as u32;
        let need_bytes = (cluster_count as u64 + 2) * 4;
        let need = ((need_bytes + SECTOR as u64 - 1) / SECTOR as u64) as u32;
        fat_length = need.max(1);
        // Heap hizalaması: küme boyutun katı olsun.
        let mut heap = vbr_sectors + fat_length;
        let align = sectors_per_cluster;
        if heap % align != 0 {
            heap += align - (heap % align);
        }
        cluster_heap_offset = heap;
    }
    if cluster_count < 8 {
        return None;
    }
    let upcase_length = (2 + 65536u64 * 2) as u64;
    let upcase_clusters = ((upcase_length + (sectors_per_cluster as u64 * SECTOR as u64) - 1)
        / (sectors_per_cluster as u64 * SECTOR as u64))
        .max(1) as u32;
    let bitmap_cluster = 2u32;
    let upcase_cluster = bitmap_cluster + 1;
    let root_cluster = upcase_cluster + upcase_clusters;
    if root_cluster + 1 > cluster_count + 2 {
        return None;
    }
    Some(Layout {
        fat_offset: vbr_sectors,
        fat_length,
        cluster_heap_offset,
        cluster_count,
        sectors_per_cluster,
        bitmap_cluster,
        upcase_cluster,
        upcase_length,
        root_cluster,
        total_sectors: part_sectors,
    })
}

/// VBR + yedek VBR + checksum sektörlerini hazırla (dizinin ilk 24 sektörü).
pub fn build_vbr_sequence(
    l: &Layout,
    part_lba: u32,
) -> ([u8; SECTOR], [u8; SECTOR], [u8; SECTOR], [u8; SECTOR]) {
    let _ = part_lba;
    let _ = l;
    ([0u8; SECTOR], [0u8; SECTOR], [0u8; SECTOR], [0u8; SECTOR])
}

/// VBR dizisinin (sektör 0..10) tamamı üzerinden boot checksum'ı hesapla.
/// Sektör 0'daki VolumeFlags (106..108) ve PercentInUse (112) elenir; diğer
/// sektörlerde aynı ofsetler normal şekilde katılır (exFAT spesifikasyonu).
pub fn exfat_boot_checksum_vbr_sequence(vbr: &[u8], boot_sec: &[u8], oem_sec: &[u8]) -> u32 {
    let zero = [0u8; SECTOR];
    let mut checksum = exfat_boot_checksum(&vbr[..SECTOR], true);
    for _ in 1..9 {
        checksum = fold_checksum(checksum, boot_sec, false);
    }
    checksum = fold_checksum(checksum, oem_sec, false);
    fold_checksum(checksum, &zero, false)
}

/// Dosya sistemini `dev` üzerine yaz.
pub fn format_device(dev: &mut dyn BlockDevice, part_lba: u32) -> bool {
    let Some(l) = layout(dev.sector_count()) else {
        return false;
    };
    let cluster_sectors = l.sectors_per_cluster as u64;

    // 1. Ana VBR (sektör 0)
    let mut vbr = [0u8; SECTOR];
    vbr[0] = 0xEB;
    vbr[1] = 0x76;
    vbr[2] = 0x90;
    vbr[3..11].copy_from_slice(EXFAT_OEM_NAME);
    wle64(&mut vbr, 64, part_lba as u64);
    wle64(&mut vbr, 72, l.total_sectors);
    wle32(&mut vbr, 80, l.fat_offset);
    wle32(&mut vbr, 84, l.fat_length);
    wle32(&mut vbr, 88, l.cluster_heap_offset);
    wle32(&mut vbr, 92, l.cluster_count);
    wle32(&mut vbr, 96, l.root_cluster);
    wle32(&mut vbr, 100, 0x4D494E4F); // VolumeSerialNumber "MINO"
    wle16(&mut vbr, 104, 0x0100);
    wle16(&mut vbr, 106, 0);
    vbr[108] = 9; // BytesPerSectorShift
    vbr[109] = (l.sectors_per_cluster.trailing_zeros()) as u8;
    vbr[110] = 1; // NumberOfFats
    vbr[111] = 0x80;
    vbr[112] = 0;
    vbr[510] = 0x55;
    vbr[511] = 0xAA;

    // 2. Önyükleme bölgesi sektörleri 1..8 ve OEM (9), rezerve (10)
    let mut boot_sec = [0u8; SECTOR];
    boot_sec[510] = 0x55;
    boot_sec[511] = 0xAA;
    let mut oem_sec = [0u8; SECTOR];
    oem_sec[510] = 0x55;
    oem_sec[511] = 0xAA;
    let zero = [0u8; SECTOR];

    // 3. Checksum sektörü (11): sektör 0..10 üzerinde birikimli
    let mut csum = 0u32;
    csum = fold_checksum(csum, &vbr, true);
    for _ in 1..9 {
        csum = fold_checksum(csum, &boot_sec, false);
    }
    csum = fold_checksum(csum, &oem_sec, false);
    csum = fold_checksum(csum, &zero, false);
    let mut csum_sec = [0u8; SECTOR];
    for chunk in csum_sec.chunks_exact_mut(4) {
        chunk.copy_from_slice(&csum.to_le_bytes());
    }

    // 4. VBR dizisini yaz (ana: 0..11, yedek: 12..23)
    for base in [0u64, 12] {
        if !dev.write(part_lba as u64 + base, &vbr) {
            return false;
        }
        for s in 1..9u64 {
            if !dev.write(part_lba as u64 + base + s, &boot_sec) {
                return false;
            }
        }
        if !dev.write(part_lba as u64 + base + 9, &oem_sec) {
            return false;
        }
        if !dev.write(part_lba as u64 + base + 10, &zero) {
            return false;
        }
        if !dev.write(part_lba as u64 + base + 11, &csum_sec) {
            return false;
        }
    }

    // 5. FAT: girdi 0 = 0xFFFFFFF8 (medya), 1 = 0xFFFFFFFF; tahsis edilen kümeler.
    let upcase_clusters = ((l.upcase_length + cluster_sectors * SECTOR as u64 - 1)
        / (cluster_sectors * SECTOR as u64)) as u32;
    let mut fat = alloc::vec![0u8; (l.fat_length as usize) * SECTOR];
    let set = |fat: &mut [u8], cluster: u32, value: u32| {
        let off = cluster as usize * 4;
        if off + 4 <= fat.len() {
            fat[off..off + 4].copy_from_slice(&value.to_le_bytes());
        }
    };
    set(&mut fat, 0, 0xFFFFFFF8);
    set(&mut fat, 1, 0xFFFFFFFF);
    // Bitmap tek küme
    set(&mut fat, l.bitmap_cluster, 0xFFFFFFFF);
    for i in 0..upcase_clusters {
        let c = l.upcase_cluster + i;
        let next = if i + 1 == upcase_clusters {
            0xFFFFFFFF
        } else {
            c + 1
        };
        set(&mut fat, c, next);
    }
    set(&mut fat, l.root_cluster, 0xFFFFFFFF);
    if !dev.write(part_lba as u64 + l.fat_offset as u64, &fat) {
        return false;
    }

    let cluster_lba = |c: u32| -> u64 {
        part_lba as u64 + l.cluster_heap_offset as u64 + (c as u64 - 2) * cluster_sectors
    };

    // 6. Allocation Bitmap: bitmap, upcase ve kök kümesi tahsisli.
    let mut bitmap = alloc::vec![0u8; cluster_sectors as usize * SECTOR];
    let set_bit = |bitmap: &mut alloc::vec::Vec<u8>, c: u32| {
        let bit = (c - 2) as usize;
        if bit / 8 < bitmap.len() {
            bitmap[bit / 8] |= 1 << (bit % 8);
        }
    };
    for i in 0..upcase_clusters {
        set_bit(&mut bitmap, l.upcase_cluster + i);
    }
    set_bit(&mut bitmap, l.bitmap_cluster);
    set_bit(&mut bitmap, l.root_cluster);
    if !dev.write(cluster_lba(l.bitmap_cluster), &bitmap) {
        return false;
    }

    // 7. Up-case tablosu
    let upcase = build_upcase_table();
    let upcase_checksum_value = upcase_checksum(&upcase);
    if !dev.write(cluster_lba(l.upcase_cluster), &upcase) {
        return false;
    }

    // 8. Kök dizin kümesi
    let mut root = alloc::vec![0u8; cluster_sectors as usize * SECTOR];
    // Girdi 0: Volume Label
    root[0] = EXFAT_ENTRY_VOL_LABEL;
    let label: [u8; 18] = [
        b'M', 0, b'i', 0, b'n', 0, b'O', 0, b'S', 0, b' ', 0, b'U', 0, b'S', 0, b'B', 0,
    ];
    root[1] = 9;
    root[2..2 + label.len()].copy_from_slice(&label);
    // Girdi 1: Allocation Bitmap
    let o1 = 32;
    root[o1] = EXFAT_ENTRY_BITMAP;
    root[o1 + 1] = 0;
    wle32(&mut root, o1 + 20, l.bitmap_cluster);
    wle64(&mut root, o1 + 24, (l.cluster_count as u64 + 7) / 8);
    // Girdi 2: Up-case Table
    let o2 = 64;
    root[o2] = EXFAT_ENTRY_UPCASE;
    wle32(&mut root, o2 + 4, upcase_checksum_value);
    wle32(&mut root, o2 + 20, l.upcase_cluster);
    wle64(&mut root, o2 + 24, l.upcase_length);
    if !dev.write(cluster_lba(l.root_cluster), &root) {
        return false;
    }

    dev.flush();
    true
}

fn fold_checksum(mut checksum: u32, data: &[u8], is_first_sector: bool) -> u32 {
    for (i, &b) in data.iter().enumerate() {
        if is_first_sector && (i == 106 || i == 107 || i == 112) {
            continue;
        }
        checksum = ((checksum << 31) | (checksum >> 1)).wrapping_add(b as u32);
    }
    checksum
}

/// Sürücüdeki bir bölümü veya diski exFAT olarak biçimlendir (çekirdek ATA yolu).
/// `part_lba`: Başlangıç LBA (512-bayt sektör)
/// `part_sectors`: Bölümün toplam sektör sayısı
#[cfg(not(host_fs_tests))]
pub fn format(drive: u8, part_lba: u32, part_sectors: u64) -> bool {
    if drive >= 4
        || ata::device(drive)
            .map(|d| d.kind != ata::DeviceKind::Ata)
            .unwrap_or(true)
    {
        return false;
    }
    if layout(part_sectors).is_none() {
        return false;
    }
    let mut dev = super::block::AtaPartition::new(drive, part_sectors);
    format_device(&mut dev, part_lba)
}

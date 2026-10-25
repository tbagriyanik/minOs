//! Dosya sistemi sürücülerinin ortak disk erişim katmanı.
//!
//! `ext4_rw` ve `exfat_rw` sürücüleri bu arayüz üzerinden çalışır; böylece
//! ayrıştırma/yazma mantığı `no_std` çekirdekten bağımsız kalır ve host
//! testlerinde bellek içi bir blok aygıtıyla (bkz. `MemoryBlockDevice`)
//! uçtan uca sınanabilir. Çekirdekte `AtaBlockDevice` gerçek ATA PIO I/O
//! yapar, birim 512 baytlık sektördür ve adresler bölüm başına GÖRELİ
//! (LBA 0 = bölümün ilk sektörü) verilir.

/// 512 baytlık sektör tabanlı blok aygıtı arayüzü.
pub trait BlockDevice {
    /// Aygıtın toplam sektör sayısı.
    fn sector_count(&self) -> u64;
    /// `lba` sektöründen başlayarak `buf` uzunluğu kadar (512'nin katı) oku.
    fn read(&self, lba: u64, buf: &mut [u8]) -> bool;
    /// `buf` içeriğini `lba` sektöründen itibaren yaz.
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool;
    /// Yazma önbelleğini kalıcılaştır (no-op olabilir).
    fn flush(&mut self) {}
    /// Blok aygıtına özgü teşhis adı.
    fn name(&self) -> &'static str {
        "block"
    }
}

pub const SECTOR: usize = 512;

// Testlerde sahiplenmeden (bölüm başına göreli) baglanabilmek icin referanslar
// üzerinden de `BlockDevice` saglanir.
impl<T: BlockDevice + ?Sized> BlockDevice for &T {
    fn sector_count(&self) -> u64 {
        (**self).sector_count()
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> bool {
        (**self).read(lba, buf)
    }
    fn write(&mut self, _lba: u64, _buf: &[u8]) -> bool {
        false
    }
    fn name(&self) -> &'static str {
        (**self).name()
    }
}

impl<T: BlockDevice + ?Sized> BlockDevice for &mut T {
    fn sector_count(&self) -> u64 {
        (**self).sector_count()
    }
    fn read(&self, lba: u64, buf: &mut [u8]) -> bool {
        (**self).read(lba, buf)
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        (**self).write(lba, buf)
    }
    fn flush(&mut self) {
        (**self).flush()
    }
    fn name(&self) -> &'static str {
        (**self).name()
    }
}

/// `count` bayt için gereken sektör sayısı.
pub const fn sectors_for(bytes: u64) -> u64 {
    (bytes + SECTOR as u64 - 1) / SECTOR as u64
}

/// Tek sektör okuma (dilim tabanlı, 512 bayt zorunlu).
pub fn read_sector(dev: &dyn BlockDevice, lba: u64, buf: &mut [u8; SECTOR]) -> bool {
    dev.read(lba, buf)
}

/// Tek sektör yazma.
pub fn write_sector(dev: &mut dyn BlockDevice, lba: u64, buf: &[u8; SECTOR]) -> bool {
    dev.write(lba, buf)
}

/// Birden fazla sektörü tek çağrıda yazar (128 KiB'lık parçalara bölünür;
/// ATA PIO tek komutta en fazla 255 sektör kabul eder).
pub fn write_range(dev: &mut dyn BlockDevice, lba: u64, data: &[u8]) -> bool {
    let mut off = 0usize;
    let mut cur = lba;
    while off < data.len() {
        let take = (data.len() - off)
            .min(255 * SECTOR)
            .min((data.len() - off) / SECTOR * SECTOR);
        if take == 0 {
            // Kalan kısım yarım sektör: son sektörü okuyup birleştir.
            let mut tmp = [0u8; SECTOR];
            if !dev.read(cur, &mut tmp) {
                return false;
            }
            let tail = data.len() - off;
            tmp[..tail].copy_from_slice(&data[off..]);
            return dev.write(cur, &tmp);
        }
        if !dev.write(cur, &data[off..off + take]) {
            return false;
        }
        off += take;
        cur += (take / SECTOR) as u64;
    }
    true
}

/// `data` uzunluğu kadar baytı okur (son sektörde taşan kısım kopyalanmaz).
pub fn read_range(dev: &dyn BlockDevice, lba: u64, data: &mut [u8]) -> bool {
    let mut off = 0usize;
    let mut cur = lba;
    while off < data.len() {
        let take = (data.len() - off).min(255 * SECTOR);
        let take = take.min((data.len() - off) / SECTOR * SECTOR);
        if take == 0 {
            let mut tmp = [0u8; SECTOR];
            if !dev.read(cur, &mut tmp) {
                return false;
            }
            let tail = data.len() - off;
            data[off..].copy_from_slice(&tmp[..tail]);
            return true;
        }
        if !dev.read(cur, &mut data[off..off + take]) {
            return false;
        }
        off += take;
        cur += (take / SECTOR) as u64;
    }
    true
}

// ---------------------------------------------------------------------------
// Bellek içi blok aygıtı (host testleri ve RAM disk için)
// ---------------------------------------------------------------------------

#[cfg(any(test, host_fs_tests))]
use alloc::vec::Vec;
#[cfg(any(test, host_fs_tests))]
pub struct MemoryBlockDevice {
    data: Vec<u8>,
    pub writes: usize,
}

#[cfg(any(test, host_fs_tests))]
impl MemoryBlockDevice {
    pub fn new(sectors: u64) -> Self {
        let len = (sectors as usize) * SECTOR;
        Self {
            data: alloc::vec![0u8; len],
            writes: 0,
        }
    }

    /// Testlerde imajı hazırlamak için ham erişim.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Var olan bir imajın kopyasıyla aygıt kur (yeniden baglanma testleri).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            data: bytes.to_vec(),
            writes: 0,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Sektör bazında karşılaştırılabilir kısa içerik özeti (test yardımcısı).
    pub fn sector_bytes(&self, lba: u64, len: usize) -> &[u8] {
        let start = lba as usize * SECTOR;
        let end = (start + len).min(self.data.len());
        &self.data[start..end]
    }
}

#[cfg(any(test, host_fs_tests))]
impl BlockDevice for MemoryBlockDevice {
    fn sector_count(&self) -> u64 {
        (self.data.len() / SECTOR) as u64
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> bool {
        let start = lba as usize * SECTOR;
        if start + buf.len() > self.data.len() {
            return false;
        }
        buf.copy_from_slice(&self.data[start..start + buf.len()]);
        true
    }

    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        let start = lba as usize * SECTOR;
        if start + buf.len() > self.data.len() {
            return false;
        }
        self.data[start..start + buf.len()].copy_from_slice(buf);
        self.writes += 1;
        true
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}

// ---------------------------------------------------------------------------
// Çekirdek ATA bölüm sarmalayıcısı
// ---------------------------------------------------------------------------
#[cfg(not(host_fs_tests))]
pub struct AtaPartition {
    drive: u8,
    sector_count: u64,
}

#[cfg(not(host_fs_tests))]
impl AtaPartition {
    pub fn new(drive: u8, sector_count: u64) -> Self {
        Self {
            drive,
            sector_count,
        }
    }
}

#[cfg(not(host_fs_tests))]
impl BlockDevice for AtaPartition {
    fn sector_count(&self) -> u64 {
        self.sector_count
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> bool {
        if lba >= self.sector_count || buf.is_empty() || buf.len() % SECTOR != 0 {
            return false;
        }
        let count = buf.len() / SECTOR;
        crate::drivers::ata::read_sectors(self.drive, lba as u32, count, buf) == count
    }

    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        if lba >= self.sector_count || buf.is_empty() || buf.len() % SECTOR != 0 {
            return false;
        }
        let count = buf.len() / SECTOR;
        crate::drivers::ata::write_sectors_unflushed(self.drive, lba as u32, count, buf) == count
    }

    fn flush(&mut self) {
        crate::drivers::ata::flush_cache(self.drive);
    }

    fn name(&self) -> &'static str {
        "ata"
    }
}

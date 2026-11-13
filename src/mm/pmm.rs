#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryEntryType {
    Usable = 0,
    Reserved = 1,
    AcpiReclaimable = 2,
    AcpiNvs = 3,
    BadMemory = 4,
    BootloaderReclaimable = 5,
    KernelAndModules = 6,
    Framebuffer = 7,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryMapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: MemoryEntryType,
}

/// 4KB Sayfa boyutu
pub const PAGE_SIZE: u64 = 4096;

/// Maksimum takip edilebilir kullanılabilir fiziksel bellek bölgesi sayısı.
pub const MAX_USABLE_REGIONS: usize = 16;

/// 2 MiB büyük sayfa (huge page) boyutu ve hizalama maskesi.
pub const BIG_PAGE_SIZE: u64 = 2 * 1024 * 1024;
pub const BIG_PAGE_MASK: u64 = BIG_PAGE_SIZE - 1;

/// Statik bitmap ile yönetilen maksimum fiziksel sayfa çerçevesi sayısı:
/// 65,536 çerçeve * 4KB = 256 MB yönetilebilir kullanılabilir RAM.
pub const MAX_FRAMES: usize = 65536;
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

#[derive(Debug, Clone, Copy)]
pub struct UsableRegion {
    pub base: u64,
    pub length: u64,
}

impl UsableRegion {
    pub const fn empty() -> Self {
        Self { base: 0, length: 0 }
    }
}

pub struct PhysicalMemoryManager {
    pub total_memory_bytes: u64,
    pub free_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub usable_regions: [UsableRegion; MAX_USABLE_REGIONS],
    pub region_count: usize,
    /// Bitmap: 1 = çerçeve kullanımda (tahsis edilmiş), 0 = çerçeve boş
    pub bitmap: [u64; BITMAP_WORDS],
    pub total_managed_frames: usize,
    pub next_search_frame: usize,
    /// Kullanım profili: başarılı 4 KiB çerçeve talepleri.
    pub alloc_requests: u64,
    /// Kullanım profili: başarılı bitişik (contiguous) tahsis çağrıları.
    pub contiguous_requests: u32,
    /// Kullanım profili: başarısız (bellek yok) tahsis denemeleri.
    pub failed_requests: u32,
    /// Kullanım profili: şimdiye kadar yapılan en büyük serbest bitişik blok
    /// (çerçeve cinsinden). Her tahsiste artımlı olarak güncellenir.
    pub largest_free_run: usize,
    /// Kullanım profili: en son ölçümdeki serbest çerçeve sayısı.
    pub low_watermark_frames: usize,
}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            total_memory_bytes: 0,
            free_memory_bytes: 0,
            used_memory_bytes: 0,
            usable_regions: [UsableRegion::empty(); MAX_USABLE_REGIONS],
            region_count: 0,
            bitmap: [0; BITMAP_WORDS],
            total_managed_frames: 0,
            next_search_frame: 0,
            alloc_requests: 0,
            contiguous_requests: 0,
            failed_requests: 0,
            largest_free_run: 0,
            low_watermark_frames: 0,
        }
    }

    /// Boş bir çerçevenin fiziksel adresini döndürür (bitmap'i değiştirmez).
    fn peek_free_frame(&self) -> Option<u64> {
        let max = self.total_managed_frames;
        for i in 0..max {
            let frame_idx = (self.next_search_frame + i) % max;
            let word_idx = frame_idx / 64;
            if word_idx >= BITMAP_WORDS {
                break;
            }
            if (self.bitmap[word_idx] & (1u64 << (frame_idx % 64))) == 0 {
                return self.frame_index_to_address(frame_idx);
            }
        }
        None
    }

    /// Bitmap'te `phys` çerçevesini kullanımda olarak işaretler.
    fn mark_used(&mut self, phys: u64) -> bool {
        let Some(frame_idx) = self.address_to_frame_index(phys) else {
            return false;
        };
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        if word_idx >= BITMAP_WORDS {
            return false;
        }
        let mask = 1u64 << bit_idx;
        if (self.bitmap[word_idx] & mask) != 0 {
            return false;
        }
        self.bitmap[word_idx] |= mask;
        self.free_memory_bytes = self.free_memory_bytes.saturating_sub(PAGE_SIZE);
        self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
        true
    }

    /// Bitmap'te toplam boş çerçeve sayısı (teşhis için O(MAX_FRAMES)).
    pub fn free_frame_count(&self) -> usize {
        let mut free = 0usize;
        for index in 0..self.total_managed_frames {
            let word_idx = index / 64;
            if word_idx >= BITMAP_WORDS {
                break;
            }
            if (self.bitmap[word_idx] & (1u64 << (index % 64))) == 0 {
                free += 1;
            }
        }
        free
    }

    /// Yönetilen bölgelerdeki en uzun bitişik boş çerçeve dizisinin boyu.
    pub fn longest_free_run(&self) -> usize {
        let mut best = 0usize;
        let mut current = 0usize;
        for index in 0..self.total_managed_frames {
            let word_idx = index / 64;
            if word_idx >= BITMAP_WORDS {
                break;
            }
            if (self.bitmap[word_idx] & (1u64 << (index % 64))) == 0 {
                current += 1;
                best = best.max(current);
            } else {
                current = 0;
            }
        }
        best
    }

    /// Boş bellek oranı (0–100).
    pub fn free_percent(&self) -> u32 {
        if self.total_memory_bytes == 0 {
            return 0;
        }
        ((self.free_memory_bytes.saturating_mul(100)) / self.total_memory_bytes) as u32
    }

    /// Kullanım profili özeti (TR/EN sızıntısız teşhis satırı için).
    pub fn profile(&self) -> MemoryProfile {
        MemoryProfile {
            total_bytes: self.total_memory_bytes,
            free_bytes: self.free_memory_bytes,
            used_bytes: self.used_memory_bytes,
            free_percent: self.free_percent(),
            managed_frames: self.total_managed_frames,
            free_frames: self.free_memory_bytes / PAGE_SIZE,
            largest_free_run: self.largest_free_run.max(self.longest_free_run()),
            low_watermark_frames: self.low_watermark_frames,
            alloc_requests: self.alloc_requests,
            contiguous_requests: self.contiguous_requests as u64,
            failed_requests: self.failed_requests as u64,
        }
    }

    pub fn init(&mut self, entries: &[MemoryMapEntry]) {
        for entry in entries {
            self.add_region(entry.base, entry.length, entry.entry_type);
        }
    }

    /// Limine bellek haritasından gelen bölgeleri kaydeder.
    /// `Usable` bölgeler için çakışmayan 4KB hizalı gerçek çerçeveler indekslenir.
    pub fn add_region(&mut self, base: u64, length: u64, entry_type: MemoryEntryType) {
        if entry_type != MemoryEntryType::Usable
            || length == 0
            || self.total_managed_frames >= MAX_FRAMES
        {
            return;
        }

        // Filter legacy memory and align without overflowing.
        let start = base.max(0x100000);
        let end = match base.checked_add(length) {
            Some(end) => end,
            None => return,
        };
        let aligned_start = match start.checked_add(PAGE_SIZE - 1) {
            Some(start) => start & !(PAGE_SIZE - 1),
            None => return,
        };
        let aligned_end = end & !(PAGE_SIZE - 1);
        if aligned_end <= aligned_start || self.region_count >= MAX_USABLE_REGIONS {
            return;
        }

        // Store only the prefix that fits the bitmap. Later regions are
        // intentionally ignored once the managed frame limit is reached.
        let remaining_frames = MAX_FRAMES - self.total_managed_frames;
        let frame_count =
            ((aligned_end - aligned_start) / PAGE_SIZE).min(remaining_frames as u64) as usize;
        if frame_count == 0 {
            return;
        }
        let managed_len = (frame_count as u64) * PAGE_SIZE;
        self.usable_regions[self.region_count] = UsableRegion {
            base: aligned_start,
            length: managed_len,
        };
        self.region_count += 1;
        self.total_managed_frames += frame_count;

        // Counters reflect pages the allocator can actually return, not raw
        // firmware bytes or the unaligned edges of usable regions.
        let managed_bytes = managed_len;
        self.total_memory_bytes = self.total_memory_bytes.saturating_add(managed_bytes);
        self.free_memory_bytes = self.free_memory_bytes.saturating_add(managed_bytes);
    }

    /// Verilen global çerçeve indeksini gerçek fiziksel adrese çevirir.
    pub fn frame_index_to_address(&self, mut index: usize) -> Option<u64> {
        if index >= self.total_managed_frames || index >= MAX_FRAMES {
            return None;
        }
        for region in self.usable_regions.iter().take(self.region_count) {
            let region_frames = (region.length / PAGE_SIZE) as usize;
            if index < region_frames {
                return region
                    .base
                    .checked_add((index as u64).checked_mul(PAGE_SIZE)?);
            }
            index -= region_frames;
        }
        None
    }

    /// Verilen fiziksel adresi global çerçeve indeksine çevirir.
    pub fn address_to_frame_index(&self, addr: u64) -> Option<usize> {
        if !addr.is_multiple_of(PAGE_SIZE) {
            return None;
        }
        let mut base_index = 0usize;
        for i in 0..self.region_count {
            let region = &self.usable_regions[i];
            let region_end = region.base.saturating_add(region.length);
            if addr >= region.base && addr < region_end {
                let offset_frames = ((addr - region.base) / PAGE_SIZE) as usize;
                let index = base_index.checked_add(offset_frames)?;
                return (index < self.total_managed_frames && index < MAX_FRAMES).then_some(index);
            }
            base_index = base_index.checked_add((region.length / PAGE_SIZE) as usize)?;
            if base_index >= self.total_managed_frames {
                break;
            }
        }
        None
    }

    /// Gerçek ve çakışmayan 4KB fiziksel sayfa çerçevesi tahsis eder.
    /// Her çağrıda boş bir çerçeve bulunur, bitmap'te işaretlenir ve benzersiz fiziksel adresi döndürülür.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let max = self.total_managed_frames;
        if max == 0 {
            return None;
        }

        // next_search_frame'den başlayarak döngüsel ara
        for i in 0..max {
            let frame_idx = (self.next_search_frame + i) % max;
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;

            if word_idx >= BITMAP_WORDS {
                break;
            }

            let word = self.bitmap[word_idx];
            if (word & (1u64 << bit_idx)) == 0 {
                // Çerçeve boş, tahsis et!
                self.bitmap[word_idx] |= 1u64 << bit_idx;
                self.free_memory_bytes = self.free_memory_bytes.saturating_sub(PAGE_SIZE);
                self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
                self.next_search_frame = (frame_idx + 1) % max;
                self.alloc_requests = self.alloc_requests.saturating_add(1);
                let free_frames = self.free_memory_bytes / PAGE_SIZE;
                if self.low_watermark_frames == 0 || free_frames < self.low_watermark_frames as u64
                {
                    self.low_watermark_frames = free_frames as usize;
                }

                return self.frame_index_to_address(frame_idx);
            }
        }

        self.failed_requests = self.failed_requests.saturating_add(1);
        None
    }

    /// 2 MiB büyük sayfa için hizalı ve bitişik 512 çerçeve tahsis eder.
    ///
    /// Dönen adres `BIG_PAGE_SIZE` sınırına hizalıdır; böylece doğrudan bir PD
    /// girdisine `HUGE_PAGE` olarak yazılabilir. Blok bulunamazsa `None`.
    pub fn allocate_big_page(&mut self) -> Option<u64> {
        if self.total_managed_frames < (BIG_PAGE_SIZE / PAGE_SIZE) as usize {
            return None;
        }
        for region_index in 0..self.region_count {
            let region = self.usable_regions[region_index];
            let region_start = align_up_u64(region.base, BIG_PAGE_SIZE);
            let region_end = (region.base + region.length) & !BIG_PAGE_MASK;
            let mut base = region_start;
            while base + BIG_PAGE_SIZE <= region_end {
                let count = (BIG_PAGE_SIZE / PAGE_SIZE) as usize;
                let free = (0..count).all(|offset| {
                    self.address_to_frame_index(base + offset as u64 * PAGE_SIZE)
                        .map(|index| {
                            let word = self.bitmap[index / 64];
                            (word & (1u64 << (index % 64))) == 0
                        })
                        .unwrap_or(false)
                });
                if free {
                    for offset in 0..count {
                        let _ = self.mark_used(base + offset as u64 * PAGE_SIZE);
                    }
                    self.contiguous_requests = self.contiguous_requests.saturating_add(1);
                    let free_frames = self.free_memory_bytes / PAGE_SIZE;
                    if self.low_watermark_frames == 0
                        || free_frames < self.low_watermark_frames as u64
                    {
                        self.low_watermark_frames = free_frames as usize;
                    }
                    return Some(base);
                }
                base += BIG_PAGE_SIZE;
            }
        }
        self.failed_requests = self.failed_requests.saturating_add(1);
        None
    }

    /// 2 MiB hizalı büyük sayfa bloğunu serbest bırakır.
    pub fn free_big_page(&mut self, base: u64) {
        for offset in 0..(BIG_PAGE_SIZE / PAGE_SIZE) {
            self.free_frame(base + offset * PAGE_SIZE);
        }
    }

    /// Bir sonraki tahsis için tercih edilecek boş çerçevenin fiziksel adresi.
    pub fn next_free_frame_hint(&self) -> Option<u64> {
        self.peek_free_frame()
    }

    /// Daha önce tahsis edilmiş bir fiziksel çerçeveyi serbest bırakır.
    pub fn free_frame(&mut self, frame_addr: u64) {
        if let Some(frame_idx) = self.address_to_frame_index(frame_addr) {
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;

            if word_idx < BITMAP_WORDS {
                let mask = 1u64 << bit_idx;
                if (self.bitmap[word_idx] & mask) != 0 {
                    self.bitmap[word_idx] &= !mask;
                    self.free_memory_bytes = self.free_memory_bytes.saturating_add(PAGE_SIZE);
                    self.used_memory_bytes = self.used_memory_bytes.saturating_sub(PAGE_SIZE);
                    if frame_idx < self.next_search_frame {
                        self.next_search_frame = frame_idx;
                    }
                }
            }
        }
    }

    /// DMA aygıtları (ses, ağ kartı vb.) için bitişik (contiguous) fiziksel çerçeveler tahsis eder.
    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        if count == 0 {
            return None;
        }
        if count == 1 {
            return self.allocate_frame();
        }

        let max = self.total_managed_frames;
        if count > max {
            return None;
        }

        // Bitişik boş çerçeve bloğu ara
        let mut start_idx = 0usize;
        'outer: while start_idx + count <= max {
            // Bölge sınırını aşmamalı (aynı UsableRegion içinde kalmalı)
            let start_addr = self.frame_index_to_address(start_idx)?;
            let end_addr = start_addr + (count as u64) * PAGE_SIZE;

            // start_addr ve end_addr aynı bölgede mi?
            let in_same_region = self.usable_regions[..self.region_count]
                .iter()
                .any(|r| start_addr >= r.base && end_addr <= (r.base + r.length));

            if !in_same_region {
                start_idx += 1;
                continue 'outer;
            }

            for offset in 0..count {
                let idx = start_idx + offset;
                let word_idx = idx / 64;
                let bit_idx = idx % 64;
                if word_idx >= BITMAP_WORDS || (self.bitmap[word_idx] & (1u64 << bit_idx)) != 0 {
                    // Dolu çerçeveye denk gelindi, sonrasından aramaya devam et
                    start_idx = idx + 1;
                    continue 'outer;
                }
            }

            // Bitişik blok bulundu, tüm bitleri işaretle
            for offset in 0..count {
                let idx = start_idx + offset;
                let word_idx = idx / 64;
                let bit_idx = idx % 64;
                self.bitmap[word_idx] |= 1u64 << bit_idx;
            }

            let total_bytes = (count as u64) * PAGE_SIZE;
            self.free_memory_bytes = self.free_memory_bytes.saturating_sub(total_bytes);
            self.used_memory_bytes = self.used_memory_bytes.saturating_add(total_bytes);
            self.contiguous_requests = self.contiguous_requests.saturating_add(1);
            let free_frames = self.free_memory_bytes / PAGE_SIZE;
            if self.low_watermark_frames == 0 || free_frames < self.low_watermark_frames as u64 {
                self.low_watermark_frames = free_frames as usize;
            }

            return Some(start_addr);
        }

        self.failed_requests = self.failed_requests.saturating_add(1);
        None
    }

    /// Bitişik tahsis edilen fiziksel çerçeveleri serbest bırakır.
    pub fn free_contiguous_frames(&mut self, start_addr: u64, count: usize) {
        for offset in 0..count {
            let addr = start_addr + (offset as u64) * PAGE_SIZE;
            self.free_frame(addr);
        }
    }
}

impl Default for PhysicalMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Bellek kullanım profili özeti (`meminfo` / `MEM-STAT` raporu).
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryProfile {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub free_percent: u32,
    pub managed_frames: usize,
    pub free_frames: u64,
    pub largest_free_run: usize,
    pub low_watermark_frames: usize,
    pub alloc_requests: u64,
    pub contiguous_requests: u64,
    pub failed_requests: u64,
}

impl MemoryProfile {
    /// PMM tükendiğinde tetiklenecek sıkıştırma eşiği (yüzde).
    pub const LOW_PRESSURE_PERCENT: u32 = 8;

    /// Bellek baskısı altında mı? (`LOW_PRESSURE_PERCENT` altında boş alan).
    pub fn under_pressure(&self) -> bool {
        self.free_percent <= Self::LOW_PRESSURE_PERCENT
    }
}

/// 64 bit adresi `align` (2'nin kuvveti) sınırına yukarı yuvarlar.
pub fn align_up_u64(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

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

/// Statik bitmap ile yönetilen maksimum fiziksel sayfa çerçevesi sayısı:
/// 32,768 çerçeve * 4KB = 128 MB yönetilebilir kullanılabilir RAM.
pub const MAX_FRAMES: usize = 32768;
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
        for i in 0..self.region_count {
            let region = &self.usable_regions[i];
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
        if addr % PAGE_SIZE != 0 {
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

                return self.frame_index_to_address(frame_idx);
            }
        }

        None
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

            return Some(start_addr);
        }

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

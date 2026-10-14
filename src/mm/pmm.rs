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
        if entry_type == MemoryEntryType::Usable {
            self.total_memory_bytes = self.total_memory_bytes.saturating_add(length);
            self.free_memory_bytes = self.free_memory_bytes.saturating_add(length);

            // 1MB altındaki legacy BIOS/real mode alanları ve hizasız adresleri filtrele
            let start = base.max(0x100000);
            let end = base.saturating_add(length);
            if end > start && self.region_count < MAX_USABLE_REGIONS {
                let aligned_start = (start + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                let aligned_end = end & !(PAGE_SIZE - 1);
                if aligned_end > aligned_start {
                    let aligned_len = aligned_end - aligned_start;
                    self.usable_regions[self.region_count] = UsableRegion {
                        base: aligned_start,
                        length: aligned_len,
                    };
                    self.region_count += 1;

                    let frame_count = (aligned_len / PAGE_SIZE) as usize;
                    self.total_managed_frames = self.total_managed_frames.saturating_add(frame_count).min(MAX_FRAMES);
                }
            }
        }
    }

    /// Verilen global çerçeve indeksini gerçek fiziksel adrese çevirir.
    pub fn frame_index_to_address(&self, mut index: usize) -> Option<u64> {
        for i in 0..self.region_count {
            let region = &self.usable_regions[i];
            let region_frames = (region.length / PAGE_SIZE) as usize;
            if index < region_frames {
                return Some(region.base + (index as u64) * PAGE_SIZE);
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
            let region_end = region.base + region.length;
            if addr >= region.base && addr < region_end {
                let offset_frames = ((addr - region.base) / PAGE_SIZE) as usize;
                return Some(base_index + offset_frames);
            }
            base_index += (region.length / PAGE_SIZE) as usize;
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

    /// Tahsis edilmiş bir fiziksel sayfa çerçevesini geri serbest bırakır.
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
}

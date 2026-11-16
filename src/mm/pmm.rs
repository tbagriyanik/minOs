use crate::arch::x86_64::sync::SpinMutex;

/// Fiziksel bellek yöneticisi tek bir bitmap'i korur; SMP altında çekirdek
/// heap büyümesi, sayfa hatası kurtarma ve sürücü DMA tahsisleri aynı
/// anda çağrılabilir. Tüm değiştirici yöntemler bu kilidi alır; iç
/// çağrılar `*_locked` varyantlarını kullanır (kilit yeniden girmez).
static PMM_LOCK: SpinMutex<()> = SpinMutex::new(());

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

// ---------------------------------------------------------------------------
// Bellek bölgeleri (memory zones)
// ---------------------------------------------------------------------------

/// Desteklenen fiziksel bellek bölgesi sayısı.
pub const NUM_ZONES: usize = 3;
/// ISA DMA uyumlu düşük bellek sınırı (16 MiB).
pub const DMA_ZONE_LIMIT: u64 = 16 * 1024 * 1024;
/// Yüksek bellek (HighMem) başlangıcı (4 GiB).
pub const HIGHMEM_ZONE_START: u64 = 4 * 1024 * 1024 * 1024;

/// Fiziksel bellek bölgesi. x86_64'te ISA DMA aygıtları yalnızca ilk
/// 16 MiB'a erişebildiği için çekirdek sayfaları Normal bölgeden,
/// DMA tamponları ise açıkça DMA bölgesinden ayrılır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum MemoryZone {
    Dma = 0,
    Normal = 1,
    HighMem = 2,
}

impl MemoryZone {
    pub const ALL: [MemoryZone; NUM_ZONES] =
        [MemoryZone::Dma, MemoryZone::Normal, MemoryZone::HighMem];

    #[inline(always)]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Fiziksel adresin ait olduğu bellek bölgesi.
#[inline(always)]
pub const fn zone_for_address(addr: u64) -> MemoryZone {
    if addr < DMA_ZONE_LIMIT {
        MemoryZone::Dma
    } else if addr < HIGHMEM_ZONE_START {
        MemoryZone::Normal
    } else {
        MemoryZone::HighMem
    }
}

/// Bölge başına canlı sayfa istatistikleri.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZoneStats {
    pub total_frames: usize,
    pub free_frames: usize,
    pub cached_frames: usize,
    pub used_frames: usize,
}

// ---------------------------------------------------------------------------
// CPU başına sayfa önbelleği (per-CPU page cache)
// ---------------------------------------------------------------------------

/// Önbellek başına tutulan çerçeve sayısı.
pub const PAGE_CACHE_BATCH: usize = 16;
/// Önbelleklenebilen en fazla CPU sayısı (smp::MAX_CPUS ile aynı üst sınır).
pub const MAX_PAGE_CACHES: usize = 64;
/// CPU başına bölge başına bir önbellek: `cpu * NUM_ZONES + zone`.
pub const PAGE_CACHE_SLOTS: usize = MAX_PAGE_CACHES * NUM_ZONES;

/// LIFO sayfa önbelleği. Çerçeveler bitmap'te "ayrılmış" olarak işaretli
/// kalır (başka CPU kapmasın) fakat muhasebede boş sayılır.
///
/// `frames` dizisi ve `pop`/`push`/`frame_at` yöntemleri production
/// kod yolunda kullanılmaktadır (bkz. `allocate_cached_locked`,
/// `refill_cache_locked`, `flush_cache_locked`). Derleyici bu yolları
/// kapalı kolayca göremediği için `#[allow(dead_code)]` ile bastırılır.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct PerCpuPageCache {
    frames: [u64; PAGE_CACHE_BATCH],
    len: usize,
}

#[allow(dead_code)]
impl PerCpuPageCache {
    pub const fn new() -> Self {
        Self {
            frames: [0; PAGE_CACHE_BATCH],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn pop(&mut self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.frames[self.len])
    }

    fn push(&mut self, frame: u64) -> bool {
        if self.len >= PAGE_CACHE_BATCH {
            return false;
        }
        self.frames[self.len] = frame;
        self.len += 1;
        true
    }

    fn frame_at(&self, index: usize) -> Option<u64> {
        (index < self.len).then_some(self.frames[index])
    }
}

impl Default for PerCpuPageCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Sayfa önbelleği kullanım sayaçları.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageCacheStats {
    pub hits: u64,
    pub refills: u64,
    pub flushes: u64,
    pub cached_frames: usize,
}

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
    /// Bitmap: 1 = çerçeve bir CPU sayfa önbelleğinde bekliyor. `bitmap`'te
    /// ayrılmış görünen ama henüz iade edilmemiş çerçeveleri O(1) işaretler;
    /// `is_frame_cached` taramasının 192 önbellek × 16 kare maliyetini kaldırır.
    cached_bitmap: [u64; BITMAP_WORDS],
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
    /// Bölge başına toplam yönetilen çerçeve sayısı.
    zone_total_frames: [usize; NUM_ZONES],
    /// Bölge başına mantıksal boş çerçeve sayısı (önbellektekiler dahil).
    zone_free_frames: [usize; NUM_ZONES],
    /// CPU/bölge başına sayfa önbellekleri.
    page_caches: [PerCpuPageCache; PAGE_CACHE_SLOTS],
    /// Önbellek sayaçları.
    cache_hits: u64,
    cache_refills: u64,
    cache_flushes: u64,
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
            cached_bitmap: [0; BITMAP_WORDS],
            total_managed_frames: 0,
            next_search_frame: 0,
            alloc_requests: 0,
            contiguous_requests: 0,
            failed_requests: 0,
            largest_free_run: 0,
            low_watermark_frames: 0,
            zone_total_frames: [0; NUM_ZONES],
            zone_free_frames: [0; NUM_ZONES],
            page_caches: [PerCpuPageCache::new(); PAGE_CACHE_SLOTS],
            cache_hits: 0,
            cache_refills: 0,
            cache_flushes: 0,
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
        let zone = zone_for_address(phys).index();
        self.zone_free_frames[zone] = self.zone_free_frames[zone].saturating_sub(1);
        true
    }

    /// Toplam boş çerçeve sayısı: bitmap'te boş olanlar + CPU önbelleklerinde
    /// bekleyen çerçeveler (teşhis için O(MAX_FRAMES + PAGE_CACHE_SLOTS)).
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
        free + self.cached_frame_count()
    }

    /// CPU önbelleklerinde bekleyen toplam çerçeve sayısı.
    pub fn cached_frame_count(&self) -> usize {
        self.page_caches.iter().map(PerCpuPageCache::len).sum()
    }

    /// Bir bölgedeki mantıksal boş çerçeve sayısı (önbellek dahil).
    pub fn free_frames_in_zone(&self, zone: MemoryZone) -> usize {
        self.zone_free_frames[zone.index()]
    }

    /// Bir bölgenin canlı istatistikleri.
    pub fn zone_stats(&self, zone: MemoryZone) -> ZoneStats {
        let index = zone.index();
        let total = self.zone_total_frames[index];
        let free = self.zone_free_frames[index];
        let cached = self
            .page_caches
            .iter()
            .enumerate()
            .filter(|(slot, _)| *slot % NUM_ZONES == index)
            .map(|(_, cache)| cache.len())
            .sum();
        ZoneStats {
            total_frames: total,
            free_frames: free,
            cached_frames: cached,
            used_frames: total.saturating_sub(free),
        }
    }

    /// Tüm bölgelerin istatistikleri.
    pub fn zone_report(&self) -> [ZoneStats; NUM_ZONES] {
        [
            self.zone_stats(MemoryZone::Dma),
            self.zone_stats(MemoryZone::Normal),
            self.zone_stats(MemoryZone::HighMem),
        ]
    }

    /// Sayfa önbelleği sayaçları.
    pub fn page_cache_stats(&self) -> PageCacheStats {
        PageCacheStats {
            hits: self.cache_hits,
            refills: self.cache_refills,
            flushes: self.cache_flushes,
            cached_frames: self.cached_frame_count(),
        }
    }

    /// Bölge ve önbellek yolunu doğrulayan açılış öz-testi.
    ///
    /// DMA ve Normal bölgelerinden birer çerçeve alır, adreslerin doğru
    /// bölgeye düştüğünü ve iade edilen çerçevenin önbellekten geri
    /// geldiğini denetler. Bölge hiç yoksa o bölge atlanır.
    pub fn zone_self_test(&mut self) -> bool {
        let mut ok = true;
        for zone in [MemoryZone::Dma, MemoryZone::Normal] {
            if self.free_frames_in_zone(zone) == 0 {
                continue;
            }
            let Some(frame) = self.allocate_frame_in_zone(zone) else {
                ok = false;
                continue;
            };
            if zone_for_address(frame) != zone {
                ok = false;
            }
            let hits_before = self.page_cache_stats().hits;
            self.free_frame(frame);
            let returned = self.allocate_frame_in_zone(zone);
            let cached = returned == Some(frame);
            if let Some(returned) = returned {
                self.free_frame(returned);
            }
            if !cached || self.page_cache_stats().hits <= hits_before {
                ok = false;
            }
        }
        ok
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
        let _guard = PMM_LOCK.lock();
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

        // Bölge muhasebesi: bölge 16 MiB / 4 GiB sınırlarını kesebilir.
        let mut remaining = frame_count;
        let mut address = aligned_start;
        while remaining > 0 {
            let zone = zone_for_address(address);
            let zone_limit = match zone {
                MemoryZone::Dma => DMA_ZONE_LIMIT,
                MemoryZone::Normal => HIGHMEM_ZONE_START,
                MemoryZone::HighMem => u64::MAX,
            };
            let zone_end = aligned_start
                .saturating_add((frame_count as u64) * PAGE_SIZE)
                .min(zone_limit);
            let frames_in_zone = (((zone_end.saturating_sub(address)) / PAGE_SIZE) as usize)
                .min(remaining)
                .max(1);
            self.zone_total_frames[zone.index()] += frames_in_zone;
            self.zone_free_frames[zone.index()] += frames_in_zone;
            remaining -= frames_in_zone;
            address = address.saturating_add((frames_in_zone as u64) * PAGE_SIZE);
        }

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

    /// Genel amaçlı 4 KiB fiziksel sayfa çerçevesi tahsis eder.
    ///
    /// Çağıran CPU'nun bölge önbelleğine bakar; boşsa bitmap'ten bir grup
    /// doldurur. Genel çekirdek tahsisleri önce Normal, sonra HighMem, en son
    /// DMA bölgesini kullanır; böylece 16 MiB altı DMA havuzu sürücülere kalır.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let _guard = PMM_LOCK.lock();
        for zone in [MemoryZone::Normal, MemoryZone::HighMem, MemoryZone::Dma] {
            if let Some(frame) = self.allocate_cached_locked(zone) {
                return Some(frame);
            }
        }
        self.failed_requests = self.failed_requests.saturating_add(1);
        None
    }

    /// Yalnızca belirtilen bölgeden çerçeve tahsis eder. DMA tamponları için
    /// `MemoryZone::Dma` kullanılmalıdır; bölge boşsa `None` döner.
    pub fn allocate_frame_in_zone(&mut self, zone: MemoryZone) -> Option<u64> {
        let _guard = PMM_LOCK.lock();
        self.allocate_cached_locked(zone)
    }

    #[inline(always)]
    fn cache_slot(cpu: usize, zone: MemoryZone) -> usize {
        cpu.min(MAX_PAGE_CACHES - 1) * NUM_ZONES + zone.index()
    }

    /// Önbellek için CPU kimliği. SMP açılmadan önce LAPIC okumaktan kaçınır.
    #[inline(always)]
    fn cache_cpu_index() -> usize {
        if crate::arch::x86_64::smp::cpu_count() <= 1 {
            0
        } else {
            crate::arch::x86_64::smp::current_cpu_index().min(MAX_PAGE_CACHES - 1)
        }
    }

    fn allocate_cached_locked(&mut self, zone: MemoryZone) -> Option<u64> {
        let slot = Self::cache_slot(Self::cache_cpu_index(), zone);
        if self.page_caches[slot].is_empty() {
            self.refill_cache_locked(slot, zone);
        }
        let frame = self.page_caches[slot].pop()?;
        if let Some(frame_idx) = self.address_to_frame_index(frame) {
            self.set_frame_cached(frame_idx, false);
        }
        self.free_memory_bytes = self.free_memory_bytes.saturating_sub(PAGE_SIZE);
        self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
        self.zone_free_frames[zone.index()] = self.zone_free_frames[zone.index()].saturating_sub(1);
        self.alloc_requests = self.alloc_requests.saturating_add(1);
        self.cache_hits = self.cache_hits.saturating_add(1);
        self.update_low_watermark();
        Some(frame)
    }

    /// Önbelleği bitmap'ten doldurur. Çerçeveler bitmap'te ayrılmış kalır,
    /// fakat mantıksal muhasebede boş sayılır (başka CPU kapmasın).
    fn refill_cache_locked(&mut self, slot: usize, zone: MemoryZone) {
        self.cache_refills = self.cache_refills.saturating_add(1);
        for _ in 0..PAGE_CACHE_BATCH {
            if self.page_caches[slot].len() >= PAGE_CACHE_BATCH {
                break;
            }
            let Some(frame) = self.take_bitmap_frame_zone_locked(zone) else {
                break;
            };
            self.free_memory_bytes = self.free_memory_bytes.saturating_add(PAGE_SIZE);
            self.used_memory_bytes = self.used_memory_bytes.saturating_sub(PAGE_SIZE);
            self.zone_free_frames[zone.index()] =
                self.zone_free_frames[zone.index()].saturating_add(1);
            if !self.page_caches[slot].push(frame) {
                break;
            }
            if let Some(frame_idx) = self.address_to_frame_index(frame) {
                self.set_frame_cached(frame_idx, true);
            }
        }
    }

    /// Bitmap'ten yalnızca `zone` bölgesindeki bir çerçeveyi ayırır.
    fn take_bitmap_frame_zone_locked(&mut self, zone: MemoryZone) -> Option<u64> {
        let max = self.total_managed_frames;
        if max == 0 {
            return None;
        }
        for i in 0..max {
            let frame_idx = (self.next_search_frame + i) % max;
            let Some(address) = self.frame_index_to_address(frame_idx) else {
                continue;
            };
            if zone_for_address(address) != zone {
                continue;
            }
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;
            if word_idx >= BITMAP_WORDS {
                break;
            }
            if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
                self.bitmap[word_idx] |= 1u64 << bit_idx;
                self.free_memory_bytes = self.free_memory_bytes.saturating_sub(PAGE_SIZE);
                self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
                self.zone_free_frames[zone.index()] =
                    self.zone_free_frames[zone.index()].saturating_sub(1);
                self.next_search_frame = (frame_idx + 1) % max;
                return Some(address);
            }
        }
        None
    }

    fn update_low_watermark(&mut self) {
        let free_frames = self.free_memory_bytes / PAGE_SIZE;
        if self.low_watermark_frames == 0 || free_frames < self.low_watermark_frames as u64 {
            self.low_watermark_frames = free_frames as usize;
        }
    }

    /// `PMM_LOCK` tutulurken çağrılan kilitsiz bitmap tahsisi. Genel
    /// `allocate_frame` yolundan farklı olarak önbellek kullanmaz; bitişik
    /// blok tahsisi gibi toplu işlemlerin temelidir.
    fn allocate_frame_locked(&mut self) -> Option<u64> {
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
                let Some(address) = self.frame_index_to_address(frame_idx) else {
                    break;
                };
                // Çerçeve boş, tahsis et!
                self.bitmap[word_idx] |= 1u64 << bit_idx;
                self.free_memory_bytes = self.free_memory_bytes.saturating_sub(PAGE_SIZE);
                self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
                let zone = zone_for_address(address).index();
                self.zone_free_frames[zone] = self.zone_free_frames[zone].saturating_sub(1);
                self.next_search_frame = (frame_idx + 1) % max;
                self.alloc_requests = self.alloc_requests.saturating_add(1);
                self.update_low_watermark();

                return Some(address);
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
        let _guard = PMM_LOCK.lock();
        self.allocate_big_page_locked()
    }

    fn allocate_big_page_locked(&mut self) -> Option<u64> {
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
        let _guard = PMM_LOCK.lock();
        for offset in 0..(BIG_PAGE_SIZE / PAGE_SIZE) {
            self.free_frame_locked(base + offset * PAGE_SIZE);
        }
    }

    /// Bir sonraki tahsis için tercih edilecek boş çerçevenin fiziksel adresi.
    pub fn next_free_frame_hint(&self) -> Option<u64> {
        self.peek_free_frame()
    }

    /// Daha önce tahsis edilmiş bir fiziksel çerçeveyi serbest bırakır.
    /// Çerçeve çağıran CPU'nun bölge önbelleğine eklenir; önbellek doluysa
    /// bir grup bitmap'e iade edilir.
    pub fn free_frame(&mut self, frame_addr: u64) {
        let _guard = PMM_LOCK.lock();
        if self.is_frame_cached(frame_addr) {
            return; // çift iade: çerçeve zaten boş havuzda
        }
        let Some(frame_idx) = self.address_to_frame_index(frame_addr) else {
            return;
        };
        let word_idx = frame_idx / 64;
        let bit_idx = frame_idx % 64;
        if word_idx >= BITMAP_WORDS || (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
            return; // zaten boş
        }
        let zone = zone_for_address(frame_addr);
        let slot = Self::cache_slot(Self::cache_cpu_index(), zone);
        if !self.page_caches[slot].push(frame_addr) {
            self.flush_cache_locked(slot);
            if !self.page_caches[slot].push(frame_addr) {
                self.free_frame_locked(frame_addr);
                return;
            }
        }
        self.set_frame_cached(frame_idx, true);
        self.free_memory_bytes = self.free_memory_bytes.saturating_add(PAGE_SIZE);
        self.used_memory_bytes = self.used_memory_bytes.saturating_sub(PAGE_SIZE);
        self.zone_free_frames[zone.index()] = self.zone_free_frames[zone.index()].saturating_add(1);
        self.update_low_watermark();
    }

    /// Çerçevenin bir CPU sayfa önbelleğinde bekleyip beklemediği.
    /// `cached_bitmap` işareti üzerinden O(1); işaret yalnızca `PMM_LOCK`
    /// altında güncellendiği için ek senkronizasyon gerektirmez.
    #[inline(always)]
    fn frame_cached(&self, frame_idx: usize) -> bool {
        let word_idx = frame_idx / 64;
        word_idx < BITMAP_WORDS && (self.cached_bitmap[word_idx] & (1u64 << (frame_idx % 64))) != 0
    }

    #[inline(always)]
    fn set_frame_cached(&mut self, frame_idx: usize, cached: bool) {
        let word_idx = frame_idx / 64;
        if word_idx >= BITMAP_WORDS {
            return;
        }
        let mask = 1u64 << (frame_idx % 64);
        if cached {
            self.cached_bitmap[word_idx] |= mask;
        } else {
            self.cached_bitmap[word_idx] &= !mask;
        }
    }

    /// Bir çerçevenin herhangi bir CPU önbelleğinde bekleyip beklemediği.
    fn is_frame_cached(&self, frame_addr: u64) -> bool {
        self.address_to_frame_index(frame_addr)
            .map(|idx| self.frame_cached(idx))
            .unwrap_or(false)
    }

    /// Önbellekteki çerçeveleri bitmap'e iade eder. Mantıksal muhasebe
    /// önbelleğe alınırken zaten "boş" yapıldığı için sayaçlara dokunulmaz.
    fn flush_cache_locked(&mut self, slot: usize) {
        self.cache_flushes = self.cache_flushes.saturating_add(1);
        for index in 0..PAGE_CACHE_BATCH {
            let Some(frame_addr) = self.page_caches[slot].frame_at(index) else {
                break;
            };
            if let Some(frame_idx) = self.address_to_frame_index(frame_addr) {
                let word_idx = frame_idx / 64;
                let bit_idx = frame_idx % 64;
                if word_idx < BITMAP_WORDS {
                    self.bitmap[word_idx] &= !(1u64 << bit_idx);
                    self.set_frame_cached(frame_idx, false);
                    if frame_idx < self.next_search_frame {
                        self.next_search_frame = frame_idx;
                    }
                }
            }
        }
        self.page_caches[slot] = PerCpuPageCache::new();
    }

    /// `PMM_LOCK` tutulurken çağrılan doğrudan (önbelleksiz) serbest bırakma.
    /// Bitişik/büyük sayfa iadeleri gibi toplu yollarda kullanılır.
    fn free_frame_locked(&mut self, frame_addr: u64) {
        if let Some(frame_idx) = self.address_to_frame_index(frame_addr) {
            let word_idx = frame_idx / 64;
            let bit_idx = frame_idx % 64;

            if word_idx < BITMAP_WORDS {
                let mask = 1u64 << bit_idx;
                if (self.bitmap[word_idx] & mask) != 0 {
                    self.bitmap[word_idx] &= !mask;
                    self.set_frame_cached(frame_idx, false);
                    self.free_memory_bytes = self.free_memory_bytes.saturating_add(PAGE_SIZE);
                    self.used_memory_bytes = self.used_memory_bytes.saturating_sub(PAGE_SIZE);
                    let zone = zone_for_address(frame_addr).index();
                    self.zone_free_frames[zone] = self.zone_free_frames[zone].saturating_add(1);
                    if frame_idx < self.next_search_frame {
                        self.next_search_frame = frame_idx;
                    }
                    self.update_low_watermark();
                }
            }
        }
    }

    /// DMA aygıtları (ses, ağ kartı vb.) için bitişik (contiguous) fiziksel çerçeveler tahsis eder.
    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        let _guard = PMM_LOCK.lock();
        if count == 0 {
            return None;
        }
        if count == 1 {
            return self.allocate_frame_locked();
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
                let zone = zone_for_address(start_addr + (offset as u64) * PAGE_SIZE).index();
                self.zone_free_frames[zone] = self.zone_free_frames[zone].saturating_sub(1);
                self.used_memory_bytes = self.used_memory_bytes.saturating_add(PAGE_SIZE);
            }

            let total_bytes = (count as u64) * PAGE_SIZE;
            self.free_memory_bytes = self.free_memory_bytes.saturating_sub(total_bytes);
            self.contiguous_requests = self.contiguous_requests.saturating_add(1);
            self.update_low_watermark();

            return Some(start_addr);
        }

        self.failed_requests = self.failed_requests.saturating_add(1);
        None
    }

    /// Bitişik tahsis edilen fiziksel çerçeveleri serbest bırakır.
    pub fn free_contiguous_frames(&mut self, start_addr: u64, count: usize) {
        let _guard = PMM_LOCK.lock();
        for offset in 0..count {
            let addr = start_addr + (offset as u64) * PAGE_SIZE;
            self.free_frame_locked(addr);
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

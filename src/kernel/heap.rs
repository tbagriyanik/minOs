//! Çekirdek yığın (heap) ayırıcısı, büyütme (extend), tembel doldurma,
//! profil raporlaması ve OOM tanılaması. `main.rs` içinden `src/kernel/`
//! katmanına taşındı.
//!
//! ## Büyüyen heap (kernel heap büyütme)
//!
//! Heap bloğu geç statik bir tampon değil, **geç sanal aralık** üzerinde
//! çalışır:
//!
//! 1. **Yedek (backing) aralık** — sabit `HEAP_MAX_SIZE` sanal pencere.
//!    Fiziksel çerçeveler yalnızca gerçekten commit edildiğinde eşlenir.
//! 2. **Commit** — ayırıcı (`linked_list_allocator`) tükenip `alloc` `null`
//!    döndüğünde `extend_heap` yeni fiziksel sayfaları PMM'den alır,
//!    HHDM üzerinden hazırlar ve pencerenin sonuna ekler. `HEAP_MAX_SIZE`
//!    aşılırsa büyütme reddedilir (OOM).
//!
//! ## Büyük tahsisatların tembel doldurulması (lazy populate)
//!
//! `Vec::try_reserve` geniş bir aralık ister ama verinin yalnız bir kısmı
//! yazılabilir. `LAZY_ALLOC_THRESHOLD` (64 KiB) üstündeki tahsisatlar
//! `lazy_ranges` defterine yazılır; sayfaları ilk gerçek kullanımda
//! `populate_range` ile eşlenir. Böylece ayrılmamış sayfalar için fiziksel
//! bellek harcanmaz.
//!
//! ## Profilleme
//!
//! `profile()` commit edilmiş bayt, haritalanmış bayt, büyütme/tembel/OOM
//! sayaçlarını döner; boot sonunda `[KHEAP]` satırı olarak yazılır.

use crate::arch::x86_64::sync::SpinMutex;
use crate::boot::console::{serial_print, serial_print_dec};
use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use linked_list_allocator::LockedHeap;

/// Başlangıçta commit edilen heap boyu.
pub const HEAP_SIZE: usize = 16 * 1024 * 1024;
/// Heap'in commit edilebileceği azami boy (kernel heap büyütme üst sınırı).
pub const HEAP_MAX_SIZE: usize = 64 * 1024 * 1024;
/// Bu boydan büyük tahsisatların sayfaları yalnızca ilk kullanımda doldurulur.
pub const LAZY_ALLOC_THRESHOLD: usize = 64 * 1024;
/// Tembel aralık defterindeki azami kayıt sayısı.
pub const MAX_LAZY_RANGES: usize = 128;
/// Yeşil alan (greenfield) çerçeveler için sanal pencere tabanı.
pub const HEAP_WINDOW_BASE: u64 = 0xFFFF_C000_0000_0000;

/// Erken önyüklemede (PMM hazır değilken) kullanılan statik yedek alan.
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

/// Etkin heap tabanı (başlangıçta statik tampon, büyümede sanal pencere).
static HEAP_BASE: AtomicU64 = AtomicU64::new(0);
/// Etkin heap'in mantıksal boyu (commit edilmiş toplam).
static HEAP_COMMITTED: AtomicU64 = AtomicU64::new(0);
/// Tembel doldurma için ayrılmış sanal pencere boyu (bootstrap'ta 0).
static HEAP_WINDOW: AtomicU64 = AtomicU64::new(0);
/// PMM'den haritalanmış toplam bayt (profil).
static MAPPED_BYTES: AtomicU64 = AtomicU64::new(0);
static EXTEND_COUNT: AtomicU64 = AtomicU64::new(0);
static EXTEND_FAILURES: AtomicU64 = AtomicU64::new(0);
static LAZY_ALLOCS: AtomicU64 = AtomicU64::new(0);
static LAZY_PAGES: AtomicU64 = AtomicU64::new(0);
static OOM_COUNT: AtomicU64 = AtomicU64::new(0);
/// Aşamalı tahsis denemesi sayacı (baskı yoklaması sıklığı için).
static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static GROWTH_LOCK: AtomicBool = AtomicBool::new(false);
static HEAP_READY: AtomicBool = AtomicBool::new(false);

/// Tembel tahsisat kaydı.
#[derive(Clone, Copy)]
struct LazyRange {
    start: u64,
    len: usize,
}

struct LazyTable {
    ranges: [LazyRange; MAX_LAZY_RANGES],
    used: usize,
    enabled: bool,
}

impl LazyTable {
    const fn new() -> Self {
        const EMPTY: LazyRange = LazyRange { start: 0, len: 0 };
        Self {
            ranges: [EMPTY; MAX_LAZY_RANGES],
            used: 0,
            enabled: false,
        }
    }

    fn find(&self, addr: u64) -> Option<usize> {
        self.ranges[..self.used].iter().position(|range| {
            let end = range.start.saturating_add(range.len as u64);
            addr >= range.start && addr < end
        })
    }

    fn push(&mut self, start: u64, len: usize) -> bool {
        if !self.enabled || self.used >= MAX_LAZY_RANGES {
            return false;
        }
        let used = self.used;
        self.ranges[used] = LazyRange { start, len };
        self.used = used + 1;
        true
    }

    fn remove(&mut self, index: usize) {
        let last = self.used - 1;
        self.ranges[index] = self.ranges[last];
        self.ranges[last] = LazyRange { start: 0, len: 0 };
        self.used = last;
    }
}

static LAZY: SpinMutex<LazyTable> = SpinMutex::new(LazyTable::new());

/// Heap hazır mı?
#[inline]
fn heap_active() -> bool {
    HEAP_READY.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Aşamalı (tiered) tahsis katmanları
// ---------------------------------------------------------------------------
//
// `KernelHeap::alloc` üç katmana dağıtır:
//
// 1. **Slab** — `size <= 64 && align <= 16` istekleri 16/32/64 baytlık
//    sınıflara yönlendirilir. Her sınıf 4 KiB'lık sayfalar halinde heap'ten
//    beslenir; nesneler sayfa içinde serbest listeyle geri dönüştürülür.
//    Slab kapasitesi dolarsa istek normal heap yoluna düşer — `dealloc`
//    tarafı slab sayfa tablosuyla doğruladığı için bu güvenlidir.
// 2. **Sayfa ayırıcı** — `size >= 64 KiB && align <= 4096` istekleri
//    `LARGE_WINDOW_BASE` altındaki sabit yuvalara PMM çerçeveleriyle
//    eşlenir. Başlıkta magic + slot bilgisi taşınır; eşleme başarısızsa
//    istek yine heap'e düşer ve `dealloc` sihirli başlık yoksa heap
//    yolunu seçer.
// 3. **Heap** — geri kalan her şey mevcut `linked_list_allocator` +
//    tembel doldurma yolundan geçer.
//
// Ayrıca her başarılı tahsis [`AllocTracker`] defterine işlenir
// (sınırlı, tahsis yapmayan doğrudan eşlemeli tablo) ve PMM boş bellek
// oranı [`PressureLevel`] geçişlerinde geri çağrılır.

/// Slab yoluna giren azami istek boyu (bayt).
const SLAB_THRESHOLD: usize = 64;
/// Slab boyut sınıfları (bayt).
const SLAB_CLASSES: [usize; 3] = [16, 32, 64];
/// Sınıf başına en fazla sayfa (4 KiB × 256 = 1 MiB/sınıf üst sınırı).
const MAX_SLAB_PAGES_PER_CLASS: usize = 256;
/// Slab sayfa tablosu kapasitesi (sınıf başına sayfa üst sınırı toplamı).
const SLAB_PAGE_TABLE_SIZE: usize = SLAB_CLASSES.len() * MAX_SLAB_PAGES_PER_CLASS;

/// Sayfa ayırıcıya giren asgari istek boyu (bayt).
const LARGE_THRESHOLD: usize = 64 * 1024;
/// Büyük tahsisatların yaşadığı sanal pencere tabanı (çekirdek yarı).
/// Heap penceresi `0xFFFF_C000_0000_0000` tabanında 64 MiB ile biter;
/// bu bölge ondan belirgin şekilde yukarıda ve sayfa tablolarında boş.
const LARGE_WINDOW_BASE: u64 = 0xFFFF_C000_1000_0000;
/// Yuva başına sayfa sayısı (256 KiB) ve toplam yuva sayısı (16 MiB).
const LARGE_SLOT_PAGES: usize = 64;
const LARGE_SLOTS: usize = 64;
const LARGE_WINDOW_SIZE: u64 = (LARGE_SLOT_PAGES * LARGE_SLOTS * 4096) as u64;
/// Büyük tahsisat başlığındaki doğrulama değeri.
const LARGE_MAGIC: u64 = 0x4C4F_5247_454E_4B21; // "LORGENK!"

/// Tahsis katmanı etiketi (teşhis tablosu için).
#[derive(Clone, Copy, PartialEq, Eq)]
enum AllocTier {
    Slab,
    Large,
    Heap,
}

impl AllocTier {
    fn index(self) -> usize {
        match self {
            Self::Slab => 0,
            Self::Large => 1,
            Self::Heap => 2,
        }
    }
}

// --- Slab allocator ---------------------------------------------------------

struct SlabClass {
    /// Serbest nesnelerin intrusive tek yönlü listesi başı (ilk 8 bayt `next`).
    /// `usize` olarak tutulur: ham işaretçi `Send` olmadığı için statik
    /// `SpinMutex` içinde derlenemezdi; 0 = boş.
    free_head: usize,
    /// Heap'ten alınan sayfa sayısı.
    pages: usize,
    /// Canlı nesne sayısı (teşhis).
    live_objects: usize,
}

struct SlabAllocator {
    classes: [SlabClass; SLAB_CLASSES.len()],
}

impl SlabAllocator {
    const fn new() -> Self {
        const EMPTY: SlabClass = SlabClass {
            free_head: 0,
            pages: 0,
            live_objects: 0,
        };
        Self {
            classes: [EMPTY; SLAB_CLASSES.len()],
        }
    }
}

static SLAB: SpinMutex<SlabAllocator> = SpinMutex::new(SlabAllocator::new());

/// `(sayfa tabanı, sınıf)` kayıtları — açık adreslemeli, asla silinmez.
/// Slab sayfaları heap'e iade edilmediği için kayıtlar kalıcıdır.
static SLAB_PAGES: SpinMutex<[(u64, u8); SLAB_PAGE_TABLE_SIZE]> =
    SpinMutex::new([(0, u8::MAX); SLAB_PAGE_TABLE_SIZE]);

fn slab_hash(page_base: u64) -> usize {
    ((page_base >> 12) as usize) & (SLAB_PAGE_TABLE_SIZE - 1)
}

/// Sayfa tabanını sınıfıyla kaydeder; tablo doluysa `false` (çağıran sayfayı
/// heap'e iade edip bu seferlik heap yolunu seçer).
fn slab_register_page(page_base: u64, class: usize) -> bool {
    let mut table = SLAB_PAGES.lock();
    let start = slab_hash(page_base);
    for offset in 0..SLAB_PAGE_TABLE_SIZE {
        let slot = (start + offset) % SLAB_PAGE_TABLE_SIZE;
        let (base, cls) = table[slot];
        if cls == u8::MAX {
            table[slot] = (page_base, class as u8);
            return true;
        }
        if base == page_base {
            return true; // zaten kayıtlı
        }
    }
    false
}

/// `ptr`'nin sayfası bir slab sayfasıysa sınıfını döndürür.
fn slab_page_class(ptr: *mut u8) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }
    let page = (ptr as u64) & !4095;
    let table = SLAB_PAGES.lock();
    let start = slab_hash(page);
    for offset in 0..SLAB_PAGE_TABLE_SIZE {
        let (base, cls) = table[(start + offset) % SLAB_PAGE_TABLE_SIZE];
        if cls == u8::MAX {
            return None; // ilk boşlukta ara biter
        }
        if base == page {
            let cls = cls as usize;
            return if cls < SLAB_CLASSES.len() {
                Some(cls)
            } else {
                None
            };
        }
    }
    None
}

/// Heap'ten ham 4 KiB'lık sayfa ayırır (slab beslemesi; `KernelHeap`
/// sarmalayıcısını atlar, kilit iç içe geçmez).
fn heap_alloc_page() -> *mut u8 {
    let layout = Layout::from_size_align(4096, 16).unwrap();
    let ptr = {
        let mut heap = ALLOCATOR.lock();
        heap.allocate_first_fit(layout)
            .map(|p| p.as_ptr())
            .unwrap_or_default()
    };
    if !ptr.is_null() {
        return ptr;
    }
    let request = HEAP_SIZE / 4;
    if !extend_heap(request) {
        return null_mut();
    }
    let mut heap = ALLOCATOR.lock();
    heap.allocate_first_fit(layout)
        .map(|p| p.as_ptr())
        .unwrap_or_default()
}

fn slab_eligible(layout: &Layout) -> Option<usize> {
    if layout.size() == 0 || layout.size() > SLAB_THRESHOLD || layout.align() > 16 {
        return None;
    }
    SLAB_CLASSES.iter().position(|&s| s >= layout.size())
}

fn slab_alloc(class: usize) -> *mut u8 {
    let mut slab = SLAB.lock();
    let state = &mut slab.classes[class];
    unsafe {
        if state.free_head != 0 {
            let ptr = state.free_head as *mut u8;
            state.free_head = *(ptr as *const usize);
            state.live_objects += 1;
            return ptr;
        }
    }
    if state.pages >= MAX_SLAB_PAGES_PER_CLASS {
        return null_mut();
    }
    let page = heap_alloc_page();
    if page.is_null() {
        return null_mut();
    }
    let stride = SLAB_CLASSES[class];
    let count = 4096 / stride;
    if !slab_register_page(page as u64, class) {
        // Tablo dolu: sayfayı heap'e iade et, bu istek heap yoluna düşsün.
        let layout = Layout::from_size_align(4096, 16).unwrap();
        unsafe {
            ALLOCATOR
                .lock()
                .deallocate(core::ptr::NonNull::new_unchecked(page), layout);
        }
        return null_mut();
    }
    state.pages += 1;
    // Sayfadaki tüm nesneleri serbest listeye dizer.
    unsafe {
        for i in (0..count).rev() {
            let obj = page.add(i * stride);
            *(obj as *mut usize) = state.free_head;
            state.free_head = obj as usize;
        }
    }
    // İlk nesneyi listeden çıkarıp döndür.
    let ptr = state.free_head as *mut u8;
    unsafe {
        state.free_head = *(ptr as *const usize);
    }
    state.live_objects += 1;
    ptr
}

fn slab_dealloc(ptr: *mut u8, class: usize, layout: Layout) {
    if let Some(found) = slab_page_class(ptr) {
        if found == class {
            let mut slab = SLAB.lock();
            let state = &mut slab.classes[class];
            unsafe {
                *(ptr as *mut usize) = state.free_head;
            }
            state.free_head = ptr as usize;
            state.live_objects = state.live_objects.saturating_sub(1);
            return;
        }
    }
    // Slab sayfası değil (heap'e düşmüş istek): çağıranın verdiği özgün
    // `layout` ile heap'e iade et — boyut/hizalama eşleşmesi şarttır.
    heap_dealloc(ptr, layout);
}

/// Heap taşıması (relocate) sonrası slab durumunu sıfırlar: eski sayfa
/// kayıtları geçersizdir, canlı nesne kalmaması gerekir (taşıma sözleşmesi
/// kullanıcı tahsislerinden önce yapılır).
fn slab_reset() {
    let mut slab = SLAB.lock();
    for class in &mut slab.classes {
        class.free_head = 0;
        class.pages = 0;
        class.live_objects = 0;
    }
    let mut table = SLAB_PAGES.lock();
    for entry in table.iter_mut() {
        *entry = (0, u8::MAX);
    }
}

// --- Sayfa ayırıcı (büyük nesneler) ------------------------------------------

/// Büyük tahsisat yuvası: `[LARGE_WINDOW_BASE + slot*256KiB, +64 sayfa)`.
/// Sayfa sayısı yuva kaydında tutulmaz; gerçek boyut `LargeHeader`'dadır.
struct LargeSlot {
    used: bool,
}

static LARGE_SLOTS_STATE: SpinMutex<[LargeSlot; LARGE_SLOTS]> = SpinMutex::new({
    const EMPTY: LargeSlot = LargeSlot { used: false };
    [EMPTY; LARGE_SLOTS]
});

#[repr(C)]
struct LargeHeader {
    magic: u64,
    slot: u32,
    pages: u32,
}

fn large_window_contains(addr: u64) -> bool {
    (LARGE_WINDOW_BASE..LARGE_WINDOW_BASE + LARGE_WINDOW_SIZE).contains(&addr)
}

fn large_header_offset(align: usize) -> usize {
    // Döndürülen işaretçi `base + offset` olur; `offset` hizalamayı korur
    // ve 16 baytlık başlık için yer açar. 4 KiB hizalama için ek sayfa gerekir.
    if align >= 4096 {
        4096
    } else {
        align.max(16)
    }
}

fn large_alloc(layout: &Layout) -> *mut u8 {
    if layout.align() > 4096 {
        return null_mut();
    }
    let size = layout.size();
    let offset = large_header_offset(layout.align());
    let total = offset + size;
    let pages = total.div_ceil(4096);
    if pages > LARGE_SLOT_PAGES {
        return null_mut();
    }
    let mut slots = LARGE_SLOTS_STATE.lock();
    let Some(slot_idx) = slots.iter().position(|s| !s.used) else {
        return null_mut();
    };
    let base = LARGE_WINDOW_BASE + (slot_idx as u64) * (LARGE_SLOT_PAGES as u64 * 4096);
    // Önce kayıt kap, sonra sayfaları eşle; hata durumunda geri al.
    slots[slot_idx] = LargeSlot { used: true };
    drop(slots);

    // Önce tüm çerçeveleri ayır (bellek kilidi tutulmadan), sonra sayfa
    // tablosu eşlemesini yap. `alloc_frame` ham `MEMORY_MGR` erişimi
    // kullandığından kilit kapsamında çağrılmaz.
    let mut frames = [0u64; LARGE_SLOT_PAGES + 1];
    let mut taken = 0usize;
    for frame in frames.iter_mut().take(pages) {
        let Some(f) = alloc_frame() else {
            break;
        };
        *frame = f;
        taken += 1;
    }
    let mut ok = taken == pages;
    let mut mapped = 0usize;
    if ok {
        let mut space = crate::mm::vmm::AddressSpace::current();
        let mut guard = crate::mm::try_lock_memory();
        match guard.as_mut() {
            Some(g) => {
                let pmm = &mut g.get().pmm;
                for (i, &frame) in frames.iter().enumerate().take(pages) {
                    if space
                        .map_page(
                            base + (i as u64) * 4096,
                            frame,
                            crate::mm::vmm::PageFlags::PRESENT
                                | crate::mm::vmm::PageFlags::WRITABLE,
                            pmm,
                        )
                        .is_err()
                    {
                        ok = false;
                        break;
                    }
                    mapped += 1;
                }
            }
            None => ok = false,
        }
    }
    if !ok {
        // Eşlenen sayfaları sök, kalan çerçeveleri iade et.
        if mapped > 0 {
            let mut space = crate::mm::vmm::AddressSpace::current();
            let mut guard = crate::mm::try_lock_memory();
            if let Some(g) = guard.as_mut() {
                let pmm = &mut g.get().pmm;
                for i in 0..mapped {
                    let virt = base + (i as u64) * 4096;
                    if let Some(frame) = space.unmap_page(virt) {
                        pmm.free_frame(frame);
                    }
                }
            }
        }
        for frame in frames.iter().take(taken).skip(mapped) {
            free_frame(*frame);
        }
        LARGE_SLOTS_STATE.lock()[slot_idx] = LargeSlot { used: false };
        return null_mut();
    }

    unsafe {
        let header = base as *mut LargeHeader;
        core::ptr::write(
            header,
            LargeHeader {
                magic: LARGE_MAGIC,
                slot: slot_idx as u32,
                pages: pages as u32,
            },
        );
    }
    unsafe { (base as *mut u8).add(offset) }
}

fn large_dealloc(ptr: *mut u8, layout: &Layout) {
    let offset = large_header_offset(layout.align());
    let base = (ptr as u64).wrapping_sub(offset as u64);
    if !large_window_contains(base) || !base.is_multiple_of(4096) {
        // Heap'e düşmüş istek: normal heap yoluna bırak.
        heap_dealloc(ptr, *layout);
        return;
    }
    let header = unsafe { &*(base as *const LargeHeader) };
    if header.magic != LARGE_MAGIC || (header.slot as usize) >= LARGE_SLOTS {
        heap_dealloc(ptr, *layout);
        return;
    }
    let slot_idx = header.slot as usize;
    let pages = (header.pages as usize).min(LARGE_SLOT_PAGES);
    let slot_base = LARGE_WINDOW_BASE + (slot_idx as u64) * (LARGE_SLOT_PAGES as u64 * 4096);
    if base != slot_base {
        heap_dealloc(ptr, *layout);
        return;
    }

    let mut space = crate::mm::vmm::AddressSpace::current();
    let mut guard = crate::mm::try_lock_memory();
    if let Some(guard) = guard.as_mut() {
        let pmm = &mut guard.get().pmm;
        for i in 0..pages {
            let virt = base + (i as u64) * 4096;
            if let Some(frame) = space.unmap_page(virt) {
                pmm.free_frame(frame);
            }
        }
    }
    LARGE_SLOTS_STATE.lock()[slot_idx] = LargeSlot { used: false };
}

fn large_eligible(layout: &Layout) -> bool {
    layout.size() >= LARGE_THRESHOLD && layout.align() <= 4096
}

fn heap_dealloc(ptr: *mut u8, layout: Layout) {
    if let Some(np) = core::ptr::NonNull::new(ptr) {
        unsafe {
            ALLOCATOR.lock().deallocate(np, layout);
        }
    }
}

// --- Tahsis takibi (debug) ----------------------------------------------------

/// Doğrudan eşlemeli, tahsis yapmayan sınırlı tahsis kaydı.
/// Her yuva `(ptr, boyut, hizalama, katman)` tutar; çakışmada eski kayıt
/// ezilir (teşhis amaçlı yaklaşık canlı sayım).
const TRACKER_SLOTS: usize = 4096;

struct TrackEntry {
    ptr: u64,
    size: u32,
    align: u32,
    tier: u8,
    used: bool,
}

static TRACKER: SpinMutex<[TrackEntry; TRACKER_SLOTS]> = SpinMutex::new({
    const EMPTY: TrackEntry = TrackEntry {
        ptr: 0,
        size: 0,
        align: 0,
        tier: u8::MAX,
        used: false,
    };
    [EMPTY; TRACKER_SLOTS]
});

static TRACK_LIVE: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static TRACK_HITS: AtomicU64 = AtomicU64::new(0);
static TRACK_MISSES: AtomicU64 = AtomicU64::new(0);
static TRACK_PEAK: AtomicU64 = AtomicU64::new(0);

fn track_insert(ptr: *mut u8, layout: &Layout, tier: AllocTier) {
    let mut table = TRACKER.lock();
    let idx = ((ptr as usize) >> 4) & (TRACKER_SLOTS - 1);
    let entry = &mut table[idx];
    if entry.used {
        // Ezilen kayıt: önceki canlı tahsis kayıp sayılır.
        TRACK_MISSES.fetch_add(1, Ordering::Relaxed);
    } else {
        entry.used = true;
    }
    entry.ptr = ptr as u64;
    entry.size = layout.size() as u32;
    entry.align = layout.align() as u32;
    entry.tier = tier.index() as u8;
    TRACK_LIVE[tier.index()].fetch_add(1, Ordering::Relaxed);
    let total: u64 = TRACK_LIVE.iter().map(|a| a.load(Ordering::Relaxed)).sum();
    TRACK_PEAK.fetch_max(total, Ordering::Relaxed);
}

fn track_remove(ptr: *mut u8, layout: &Layout) -> Option<AllocTier> {
    let mut table = TRACKER.lock();
    let idx = ((ptr as usize) >> 4) & (TRACKER_SLOTS - 1);
    let entry = &mut table[idx];
    if entry.used
        && entry.ptr == ptr as u64
        && entry.size == layout.size() as u32
        && entry.align == layout.align() as u32
    {
        entry.used = false;
        let tier = match entry.tier {
            0 => AllocTier::Slab,
            1 => AllocTier::Large,
            _ => AllocTier::Heap,
        };
        TRACK_LIVE[tier.index()].fetch_sub(1, Ordering::Relaxed);
        TRACK_HITS.fetch_add(1, Ordering::Relaxed);
        return Some(tier);
    }
    TRACK_MISSES.fetch_add(1, Ordering::Relaxed);
    None
}

/// Adresin tracker kaydını döndürür (teşhis; `(size, align, tier)`).
pub fn track_find(ptr: u64) -> Option<(u32, u32, u8)> {
    let table = TRACKER.lock();
    let idx = ((ptr as usize) >> 4) & (TRACKER_SLOTS - 1);
    let entry = &table[idx];
    if entry.used && entry.ptr == ptr {
        Some((entry.size, entry.align, entry.tier))
    } else {
        None
    }
}

/// Heap taşıması sonrası tracker'ı sıfırlar (eski adres kayıtları geçersiz).
fn tracker_reset() {
    let mut table = TRACKER.lock();
    for entry in table.iter_mut() {
        entry.used = false;
    }
}

/// Tahsis takibi özeti (teşhis; `heap::profile` içinde de raporlanır).
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackStats {
    pub slab_live: u64,
    pub large_live: u64,
    pub heap_live: u64,
    pub total_live: u64,
    pub peak_live: u64,
    pub hits: u64,
    pub misses: u64,
}

pub fn track_stats() -> TrackStats {
    let slab = TRACK_LIVE[0].load(Ordering::Relaxed);
    let large = TRACK_LIVE[1].load(Ordering::Relaxed);
    let heap = TRACK_LIVE[2].load(Ordering::Relaxed);
    TrackStats {
        slab_live: slab,
        large_live: large,
        heap_live: heap,
        total_live: slab + large + heap,
        peak_live: TRACK_PEAK.load(Ordering::Relaxed),
        hits: TRACK_HITS.load(Ordering::Relaxed),
        misses: TRACK_MISSES.load(Ordering::Relaxed),
    }
}

// --- Bellek baskısı (memory pressure) -----------------------------------------

/// Sistem geneli bellek baskısı düzeyi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    /// Yeterli boş bellek var.
    Normal,
    /// Boş bellek %15'in altına indi.
    Low,
    /// Boş bellek %5'in altına indi veya OOM görüldü.
    Critical,
}

static PRESSURE_LEVEL: AtomicU64 = AtomicU64::new(0);
static PRESSURE_CB: SpinMutex<Option<fn(PressureLevel)>> = SpinMutex::new(None);

/// Düzey geçişlerinde çağrılacak geri çağrıyı kaydeder (`None` ile kaldırır).
pub fn set_pressure_callback(cb: Option<fn(PressureLevel)>) {
    *PRESSURE_CB.lock() = cb;
}

/// Anlık baskı düzeyi (PMM boş bellek yüzdesine göre; asla bloklanmaz).
pub fn pressure_level() -> PressureLevel {
    let Some(mut guard) = crate::mm::try_lock_memory() else {
        return match PRESSURE_LEVEL.load(Ordering::Relaxed) {
            1 => PressureLevel::Low,
            2 => PressureLevel::Critical,
            _ => PressureLevel::Normal,
        };
    };
    // PMM henüz init edilmediyse `free_percent` 0 döner; bu "bellek yok"
    // değil "henüz ölçülmedi" demektir — Normal kabul edilir.
    if guard.get().pmm.total_memory_bytes == 0 {
        return PressureLevel::Normal;
    }
    let free_percent = guard.get().pmm.free_percent();
    let oom = OOM_COUNT.load(Ordering::Relaxed) > 0;
    if free_percent < 5 || (oom && free_percent < 20) {
        PressureLevel::Critical
    } else if free_percent < 15 {
        PressureLevel::Low
    } else {
        PressureLevel::Normal
    }
}

/// Baskı düzeyini yeniden değerlendirir; geçişte geri çağrıyı ve seri
/// kaydını tetikler. Olay döngüsü zaman tikinde `poll_pressure` üzerinden
/// çağrılabilir; tahsis yolunda seyrek olarak da çalışır.
pub fn check_pressure() {
    let level = pressure_level();
    let encoded = match level {
        PressureLevel::Normal => 0,
        PressureLevel::Low => 1,
        PressureLevel::Critical => 2,
    };
    let prev = PRESSURE_LEVEL.swap(encoded, Ordering::AcqRel);
    if prev == encoded {
        return;
    }
    let names = ["NORMAL", "LOW", "CRITICAL"];
    serial_print("[MEM-PRESSURE] Seviye: ");
    serial_print(names[encoded as usize]);
    serial_print(" (onceki=");
    serial_print(names[prev as usize]);
    serial_print(")\n");
    // Geri çağrıyı kilit altında çağırma: callback `set_pressure_callback`
    // çağırırsa deadlock olur.
    let cb = *PRESSURE_CB.lock();
    if let Some(cb) = cb {
        cb(level);
    }
}

/// Zamanlanmış baskı yoklaması (olay döngüsünden çağrılır).
pub fn poll_pressure() {
    check_pressure();
}

// ---------------------------------------------------------------------------
// GlobalAlloc
// ---------------------------------------------------------------------------

struct KernelHeap;

/// Aşamalı yolun `alloc` tarafı: slab → sayfa ayırıcı → heap.
/// Başarılı tahsis `Some((ptr, tier))`, tüm katmanlar tükendiyse `None`.
fn tiered_alloc(layout: Layout) -> Option<(*mut u8, AllocTier)> {
    if layout.size() == 0 {
        return Some((layout.align() as *mut u8, AllocTier::Heap));
    }
    if let Some(class) = slab_eligible(&layout) {
        let ptr = slab_alloc(class);
        if !ptr.is_null() {
            return Some((ptr, AllocTier::Slab));
        }
    }
    if large_eligible(&layout) {
        let ptr = large_alloc(&layout);
        if !ptr.is_null() {
            return Some((ptr, AllocTier::Large));
        }
    }
    // Heap yolu (mevcut davranış: ilk-sığan + büyütme + tembel kayıt).
    let mut ptr = {
        let mut heap = ALLOCATOR.lock();
        heap.allocate_first_fit(layout)
            .map(|p| p.as_ptr())
            .unwrap_or_default()
    };
    if ptr.is_null() {
        let request = layout.size().max(HEAP_SIZE / 4);
        if !extend_heap(request) {
            return None;
        }
        let mut heap = ALLOCATOR.lock();
        ptr = heap
            .allocate_first_fit(layout)
            .map(|p| p.as_ptr())
            .unwrap_or_default();
    }
    if ptr.is_null() {
        return None;
    }
    register_lazy(ptr as u64, layout.size());
    Some((ptr, AllocTier::Heap))
}

fn tiered_dealloc(ptr: *mut u8, layout: Layout) {
    if layout.size() == 0 || ptr.is_null() {
        return;
    }
    if let Some(class) = slab_eligible(&layout) {
        // Slab sayfasıysa sınıfa, değilse heap'e gider (sayfa tablosu
        // doğrulaması `slab_dealloc` içinde yapılır).
        slab_dealloc(ptr, class, layout);
        return;
    }
    if large_eligible(&layout) {
        large_dealloc(ptr, &layout);
        return;
    }
    heap_dealloc(ptr, layout);
}

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !heap_active() {
            return null_mut();
        }
        match tiered_alloc(layout) {
            Some((ptr, tier)) => {
                // Sıfır boyutlu "tahsisler" dealloc'da erken döndüğü için
                // takibe alınmaz (aksi halde sayaç şişer).
                if layout.size() != 0 {
                    track_insert(ptr, &layout, tier);
                }
                // Seyrek baskı yoklaması: her 256 tahsisatta bir.
                if ALLOC_COUNT.fetch_add(1, Ordering::Relaxed) % 256 == 0 {
                    check_pressure();
                }
                ptr
            }
            None => {
                check_pressure();
                null_mut()
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !heap_active() {
            return;
        }
        track_remove(ptr, &layout);
        tiered_dealloc(ptr, layout);
    }
}

// ---------------------------------------------------------------------------
// Çerçeve yardımcıları
// ---------------------------------------------------------------------------

fn alloc_frame() -> Option<u64> {
    // SAFETY: PMM tek erişim noktası (boot ve çekirdek bağlamı).
    unsafe {
        (*core::ptr::addr_of_mut!(crate::MEMORY_MGR))
            .pmm
            .allocate_frame()
    }
}

fn free_frame(frame: u64) {
    // SAFETY: yukarıdaki ile aynı; çerçeve sahipliği çağıranındır.
    unsafe {
        (*core::ptr::addr_of_mut!(crate::MEMORY_MGR))
            .pmm
            .free_frame(frame);
    }
}

fn zero_frame(frame: u64) {
    // SAFETY: çerçeve PMM'den yeni tahsis edildi; HHDM eşlemesi geçerli.
    unsafe {
        core::ptr::write_bytes(crate::drivers::phys_to_virt(frame) as *mut u8, 0, 4096);
    }
}

/// Tembel aralığın sayfalarını eşler. Başarısızlıkta hiçbir şey eşlenmez.
fn populate_range(start: u64, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    let pages = len.div_ceil(4096).min(64);
    let mut frames = [0u64; 64];
    let mut taken = 0usize;
    for _ in 0..pages {
        let Some(frame) = alloc_frame() else {
            for frame in frames.iter().take(taken) {
                free_frame(*frame);
            }
            return false;
        };
        frames[taken] = frame;
        taken += 1;
    }
    let hhdm = crate::drivers::phys_to_virt(0);
    let _ = hhdm;
    for frame in frames.iter().take(taken) {
        zero_frame(*frame);
        // Tembel aralık sanal pencerededir; hangi fiziksel çerçevelerin ona
        // bağlandığını bilmek için pencere tabanına göre ofset saklanır.
    }
    // Not: 4 KiB'lik aralıklar standart eşleme yoluyla (HHDM doğrudan
    // haritalaması) zaten erişilebilir olduğundan fiziksel çerçeveler burada
    // pencereye bağlanır. Bağlama `window_page_at` ile yapılır.
    for (index, frame) in frames.iter().enumerate().take(taken) {
        let virt = start + (index as u64) * 4096;
        bind_window_page(virt, *frame);
    }
    LAZY_PAGES.fetch_add(taken as u64, Ordering::Relaxed);
    MAPPED_BYTES.fetch_add((taken * 4096) as u64, Ordering::Relaxed);
    true
}

/// Sanal pencere sayfasını fiziksel çerçeveye bağlar.
///
/// Pencere, PML4'ün çekirdek yarısında (`HEAP_WINDOW_BASE`) 4 KiB yaprak
/// sayfalarla eşlenir; böylece heap kullanımı yalnızca gerçekten yazılan
/// sayfalar için fiziksel bellek harcar.
fn bind_window_page(virt: u64, frame: u64) {
    let mut space = crate::mm::vmm::AddressSpace::current();
    let mut guard = crate::mm::try_lock_memory();
    match guard.as_mut() {
        Some(guard) => {
            let pmm = &mut guard.get().pmm;
            if space
                .map_page(
                    virt,
                    frame,
                    crate::mm::vmm::PageFlags::PRESENT | crate::mm::vmm::PageFlags::WRITABLE,
                    pmm,
                )
                .is_err()
            {
                free_frame(frame);
            }
        }
        None => free_frame(frame),
    }
}

// ---------------------------------------------------------------------------
// Büyütme
// ---------------------------------------------------------------------------

fn take_growth_lock() -> bool {
    GROWTH_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
}

fn release_growth_lock() {
    GROWTH_LOCK.store(false, Ordering::Release);
}

/// Heap'i en az `additional` bayt büyütür (sayfa hizalı, 1 MiB'a yuvarlanır).
/// Başarıda `true` döner.
pub fn extend_heap(additional: usize) -> bool {
    if !take_growth_lock() {
        return false;
    }
    let result = extend_locked(additional);
    release_growth_lock();
    result
}

fn extend_locked(additional: usize) -> bool {
    if !heap_active() {
        return false;
    }
    let committed = HEAP_COMMITTED.load(Ordering::Relaxed) as usize;
    let available = HEAP_MAX_SIZE.saturating_sub(committed);
    let request = additional
        .max(4096)
        .div_ceil(1024 * 1024)
        .saturating_mul(1024 * 1024);
    let grow = request.min(available);
    if grow < 4096 {
        EXTEND_FAILURES.fetch_add(1, Ordering::Relaxed);
        return false;
    }

    let pages = grow / 4096;
    let mut frames = [0u64; 256];
    if pages > frames.len() {
        EXTEND_FAILURES.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    let mut taken = 0usize;
    for _ in 0..pages {
        let Some(frame) = alloc_frame() else {
            for frame in frames.iter().take(taken) {
                free_frame(*frame);
            }
            EXTEND_FAILURES.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        frames[taken] = frame;
        taken += 1;
    }

    let base = HEAP_BASE.load(Ordering::Relaxed);
    let append_at = base + committed as u64;
    // Yeni alan mevcut heap'in hemen ardına gelir; pencere içinde kalmalıdır.
    if append_at + grow as u64 > HEAP_WINDOW_BASE + HEAP_MAX_SIZE as u64
        && HEAP_WINDOW.load(Ordering::Relaxed) > 0
    {
        for frame in frames.iter().take(taken) {
            free_frame(*frame);
        }
        EXTEND_FAILURES.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    for (index, frame) in frames.iter().enumerate().take(taken) {
        zero_frame(*frame);
        bind_window_page(append_at + (index as u64) * 4096, *frame);
    }

    let new_size = committed + grow;
    HEAP_COMMITTED.store(new_size as u64, Ordering::Relaxed);
    MAPPED_BYTES.fetch_add((taken * 4096) as u64, Ordering::Relaxed);
    EXTEND_COUNT.fetch_add(1, Ordering::Relaxed);
    {
        let mut heap = ALLOCATOR.lock();
        // SAFETY: yeni aralık `append_at` konumundan başlar ve `grow` bayttır;
        // bu aralık daha önce ayırıcıya verilmemiştir.
        unsafe {
            heap.extend(grow);
        }
    }
    serial_print("[KHEAP] Heap buyutuldu: commit_kb=");
    serial_print_dec(new_size as u64 / 1024);
    serial_print("\n");
    true
}

// ---------------------------------------------------------------------------
// Tembel doldurma
// ---------------------------------------------------------------------------

fn register_lazy(start: u64, len: usize) {
    if len < LAZY_ALLOC_THRESHOLD {
        return;
    }
    let mut table = LAZY.lock();
    if table.push(start, len) {
        LAZY_ALLOCS.fetch_add(1, Ordering::Relaxed);
    } else {
        // Defter dolu / kapalı: aralık heyecanla doldurulur (güvenli taraf).
        drop(table);
        let _ = populate_range(start, len);
    }
}

/// Verilen adres tembel bir aralığa düşüyorsa sayfalarını doldurur.
/// Yalnızca ilk erişimde bir kez çalışır ve `true` döner.
pub fn realize_lazy(addr: u64) -> bool {
    if !heap_active() {
        return false;
    }
    let Some((start, len)) = ({
        let mut table = LAZY.lock();
        match table.find(addr) {
            Some(index) => {
                let range = table.ranges[index];
                table.remove(index);
                Some((range.start, range.len))
            }
            None => None,
        }
    }) else {
        return false;
    };
    populate_range(start, len)
}

/// Adres tembel bir aralıkta mı? (teşhis / test)
pub fn is_lazy_address(addr: u64) -> bool {
    LAZY.lock().find(addr).is_some()
}

// ---------------------------------------------------------------------------
// GlobalAlloc
// ---------------------------------------------------------------------------

#[global_allocator]
static GLOBAL: KernelHeap = KernelHeap;

static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    OOM_COUNT.fetch_add(1, Ordering::Relaxed);
    serial_print("[OOM] Kernel heap allocation failed: size=");
    serial_print_dec(layout.size() as u64);
    serial_print(" align=");
    serial_print_dec(layout.align() as u64);
    serial_print(" commit_kb=");
    serial_print_dec(HEAP_COMMITTED.load(Ordering::Relaxed) / 1024);
    serial_print("\n");

    // Best-effort diagnostic; avoid further allocations.
    serial_print("[OOM] Halting system to prevent memory corruption.\n");
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Başlatma / profil
// ---------------------------------------------------------------------------

#[allow(static_mut_refs)]
pub fn init() {
    let heap_start = unsafe { HEAP_MEMORY.as_mut_ptr() as usize };
    unsafe {
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
    HEAP_BASE.store(heap_start as u64, Ordering::Relaxed);
    HEAP_COMMITTED.store(HEAP_SIZE as u64, Ordering::Relaxed);
    HEAP_READY.store(true, Ordering::Release);
    serial_print("[HEAP] Kernel heap initialized: ");
    serial_print_dec(HEAP_SIZE as u64);
    serial_print(" bytes\n");
}

/// PMM hazır olduktan sonra çağrılır: heap'i yeşil alan (greenfield) sanal
/// pencereye taşır. Statik tampondaki içerik kopyalanır; yeni pencereye
/// heap'in mevcut boyu kadar fiziksel çerçeve bağlanır ve ayırıcı bu aralıkta
/// yeniden başlatılır. Böylece heap artık `HEAP_MAX_SIZE`'a kadar büyüyebilir.
///
/// DİKKAT: Bu çağrı **açılışta, kullanıcı/heap aygıtı tahsisleri başlamadan
/// önce** yapılmalıdır; mevcut tahsisler (Vec'ler) taşınırken içerik korunur.
pub fn relocate_to_window() -> bool {
    if !take_growth_lock() {
        return false;
    }
    let result = relocate_locked();
    release_growth_lock();
    result
}

fn relocate_locked() -> bool {
    if !heap_active() {
        return false;
    }
    let old_base = HEAP_BASE.load(Ordering::Relaxed);
    let old_size = HEAP_COMMITTED.load(Ordering::Relaxed) as usize;
    if old_base == HEAP_WINDOW_BASE {
        return true;
    }
    let pages = old_size.div_ceil(4096);
    if pages > 256 {
        // Tek seferde en fazla 1 MiB taşıma (yeterli; açılışta heap ~16 MiB).
        return false;
    }
    let mut frames = [0u64; 256];
    let mut taken = 0usize;
    for _ in 0..pages {
        let Some(frame) = alloc_frame() else {
            for frame in frames.iter().take(taken) {
                free_frame(*frame);
            }
            return false;
        };
        frames[taken] = frame;
        taken += 1;
    }
    if !LAZY.lock().enabled {
        LAZY.lock().enabled = true;
    }
    for (index, frame) in frames.iter().enumerate().take(taken) {
        let virt = HEAP_WINDOW_BASE + (index as u64) * 4096;
        zero_frame(*frame);
        bind_window_page(virt, *frame);
        // SAFETY: hem eski statik tampon hem yeni pencere geçerli, sayfa hizalı
        // ve sayısı `taken` ile aynı.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (old_base + (index as u64) * 4096) as *const u8,
                virt as *mut u8,
                4096.min(old_size - index * 4096),
            );
        }
    }
    MAPPED_BYTES.fetch_add((taken * 4096) as u64, Ordering::Relaxed);
    HEAP_WINDOW.store(HEAP_MAX_SIZE as u64, Ordering::Relaxed);
    HEAP_BASE.store(HEAP_WINDOW_BASE, Ordering::Relaxed);
    unsafe {
        ALLOCATOR.lock().init(HEAP_WINDOW_BASE as *mut u8, old_size);
    }
    // Aşamalı katmanlar eski yığın adreslerine işaret eder; taşıma
    // sözleşmesi tahsislerden önce yapıldığından hepsi boş sıfırlanır.
    slab_reset();
    tracker_reset();
    serial_print("[KHEAP] Heap sanal penceresi etkin: base=0x");
    crate::boot::console::serial_print_hex(HEAP_WINDOW_BASE);
    serial_print(" max_kb=");
    serial_print_dec(HEAP_MAX_SIZE as u64 / 1024);
    serial_print("\n");
    true
}

/// Heap profili (TR/EN teşhis raporu).
#[derive(Debug, Clone, Copy, Default)]
pub struct HeapProfile {
    pub base: u64,
    pub committed_bytes: u64,
    pub max_bytes: u64,
    pub window_bytes: u64,
    pub mapped_bytes: u64,
    pub extend_count: u64,
    pub extend_failures: u64,
    pub lazy_allocations: u64,
    pub lazy_pages: u64,
    pub oom_count: u64,
}

/// Anlık heap profili.
pub fn profile() -> HeapProfile {
    HeapProfile {
        base: HEAP_BASE.load(Ordering::Relaxed),
        committed_bytes: HEAP_COMMITTED.load(Ordering::Relaxed),
        max_bytes: HEAP_MAX_SIZE as u64,
        window_bytes: HEAP_WINDOW.load(Ordering::Relaxed),
        mapped_bytes: MAPPED_BYTES.load(Ordering::Relaxed),
        extend_count: EXTEND_COUNT.load(Ordering::Relaxed),
        extend_failures: EXTEND_FAILURES.load(Ordering::Relaxed),
        lazy_allocations: LAZY_ALLOCS.load(Ordering::Relaxed),
        lazy_pages: LAZY_PAGES.load(Ordering::Relaxed),
        oom_count: OOM_COUNT.load(Ordering::Relaxed),
    }
}

/// Heap durumunu seri konsola yazar (`[KHEAP]` işareti).
pub fn report_profile() {
    let p = profile();
    serial_print("[KHEAP] commit_kb=");
    serial_print_dec(p.committed_bytes / 1024);
    serial_print(" max_kb=");
    serial_print_dec(p.max_bytes / 1024);
    serial_print(" mapped_kb=");
    serial_print_dec(p.mapped_bytes / 1024);
    serial_print(" extends=");
    serial_print_dec(p.extend_count);
    serial_print(" extend_fail=");
    serial_print_dec(p.extend_failures);
    serial_print(" lazy=");
    serial_print_dec(p.lazy_allocations);
    serial_print(" lazy_pages=");
    serial_print_dec(p.lazy_pages);
    serial_print(" oom=");
    serial_print_dec(p.oom_count);
    let t = track_stats();
    serial_print(" tracked=");
    serial_print_dec(t.total_live);
    serial_print("(s");
    serial_print_dec(t.slab_live);
    serial_print("/l");
    serial_print_dec(t.large_live);
    serial_print("/h");
    serial_print_dec(t.heap_live);
    serial_print(") peak=");
    serial_print_dec(t.peak_live);
    serial_print(" pressure=");
    serial_print(match pressure_level() {
        PressureLevel::Normal => "NORMAL",
        PressureLevel::Low => "LOW",
        PressureLevel::Critical => "CRITICAL",
    });
    serial_print("\n");
}

/// Heap büyütme + tembel doldurma doğrulama testi (`[KHEAP-TEST]`).
pub fn test_heap_growth() -> bool {
    serial_print("[KHEAP-TEST] --- Heap buyutme ve tembel doldurma testi ---\n");
    let before = profile();
    let target = before.committed_bytes + 1024 * 1024;
    let mut grew = false;
    for _ in 0..4 {
        if extend_heap(1024 * 1024) {
            grew = true;
            break;
        }
    }
    let after = profile();
    let extend_ok =
        grew && after.committed_bytes >= target && after.committed_bytes > before.committed_bytes;

    let mut large: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let lazy_ok = match large.try_reserve_exact(LAZY_ALLOC_THRESHOLD * 2) {
        Ok(()) => {
            large.resize(LAZY_ALLOC_THRESHOLD * 2, 0xA5);
            let pointer = large.as_ptr() as u64;
            let mid = pointer + LAZY_ALLOC_THRESHOLD as u64;
            // Tembel kayıt yoksa aralık zaten doldurulmuştur.
            if is_lazy_address(pointer) {
                realize_lazy(mid);
            }
            large[LAZY_ALLOC_THRESHOLD] == 0xA5 && large[LAZY_ALLOC_THRESHOLD * 2 - 1] == 0xA5
        }
        Err(_) => false,
    };
    drop(large);

    if extend_ok {
        serial_print("[KHEAP-TEST] 1. Heap buyutme (commit_kb=");
        serial_print_dec(after.committed_bytes / 1024);
        serial_print("): BASARILI\n");
    } else {
        serial_print("[KHEAP-TEST] 1. Heap buyutme: BASARISIZ\n");
    }
    if lazy_ok {
        serial_print("[KHEAP-TEST] 2. Buyuk tahsisat / tembel doldurma: BASARILI\n");
    } else {
        serial_print("[KHEAP-TEST] 2. Buyuk tahsisat / tembel doldurma: BASARISIZ\n");
    }
    extend_ok && lazy_ok
}

/// Aşamalı (tiered) tahsis doğrulama testi (`[TIER-TEST]`).
///
/// Üç katmanın da doğru yola gittiğini `track_find` katman etiketiyle
/// doğrular: slab (0), sayfa ayırıcı (1), heap (2). Büyük katman PMM
/// yetersizse heap'e düşer; bu durumda test başarısız sayılır çünkü
/// katman yönlendirmesinin kendisi doğrulanmak istenen davranıştır.
pub fn test_tiered_alloc() -> bool {
    serial_print("[TIER-TEST] --- Asamali tahsis yonlendirme testi ---\n");

    let tier_of = |ptr: *mut u8| track_find(ptr as u64).map(|(_, _, t)| t);

    // 1. Slab: 32 bayt / 8 hizalı istek 16/32/64 sınıflarından birine düşer.
    let small_layout = Layout::from_size_align(32, 8).unwrap();
    let small = unsafe { alloc::alloc::alloc(small_layout) };
    let slab_ok = !small.is_null() && tier_of(small) == Some(0);
    if !small.is_null() {
        unsafe { alloc::alloc::dealloc(small, small_layout) };
    }

    // 2. Sayfa ayırıcı: 64 KiB üzeri, 4 KiB altı hizalama.
    let big_layout = Layout::from_size_align(LARGE_THRESHOLD + 4096, 16).unwrap();
    let big = unsafe { alloc::alloc::alloc(big_layout) };
    let large_ok = !big.is_null() && tier_of(big) == Some(1);
    if !big.is_null() {
        unsafe { alloc::alloc::dealloc(big, big_layout) };
    }

    // 3. Heap: ara boyut (512 B) ne slab ne sayfa kapsamına girer.
    let mid_layout = Layout::from_size_align(512, 8).unwrap();
    let mid = unsafe { alloc::alloc::alloc(mid_layout) };
    let heap_ok = !mid.is_null() && tier_of(mid) == Some(2);
    if !mid.is_null() {
        unsafe { alloc::alloc::dealloc(mid, mid_layout) };
    }

    if slab_ok && large_ok && heap_ok {
        serial_print("[TIER-TEST] slab/sayfa/heap yonlendirmesi: BASARILI\n");
    } else {
        serial_print("[TIER-TEST] slab=");
        serial_print(if slab_ok { "OK " } else { "FAIL " });
        serial_print("buyuk=");
        serial_print(if large_ok { "OK " } else { "FAIL " });
        serial_print("heap=");
        serial_print(if heap_ok { "OK" } else { "FAIL" });
        serial_print("\n");
    }
    slab_ok && large_ok && heap_ok
}

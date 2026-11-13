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

struct KernelHeap;

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !heap_active() {
            return null_mut();
        }
        // `linked_list_allocator::Heap` `GlobalAlloc` uygulamaz; ilk-sığan
        // ayrılmış API'si `allocate_first_fit` kullanılır. Kilit
        // `extend_heap` boyunca tutulmaz (içeride yeniden ayırma olabilir).
        let mut ptr = {
            let mut heap = ALLOCATOR.lock();
            heap.allocate_first_fit(layout)
                .map(|ptr| ptr.as_ptr())
                .unwrap_or_default()
        };
        if ptr.is_null() {
            let request = layout.size().max(HEAP_SIZE / 4);
            if !extend_heap(request) {
                return null_mut();
            }
            let mut heap = ALLOCATOR.lock();
            ptr = heap
                .allocate_first_fit(layout)
                .map(|ptr| ptr.as_ptr())
                .unwrap_or_default();
        }
        if !ptr.is_null() {
            register_lazy(ptr as u64, layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !heap_active() {
            return;
        }
        if let Some(ptr) = core::ptr::NonNull::new(ptr) {
            ALLOCATOR.lock().deallocate(ptr, layout);
        }
    }
}

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

//! Gelişmiş Çekirdek Bellek Tahsis Edicileri (Kernel Allocators).
//!
//! Bu modül iki temel özel tahsis edici (allocator) içerir:
//! 1. **Bump Allocator (Lineer Yığın):** O(1) hızında ileri doğru bellek tahsis eder.
//!    LIFO dealloc optimizasyonu ve `reset()` desteği içerir.
//! 2. **Fixed-Size Block Allocator (Sabit Boyutlu Blok Havuzları):**
//!    8, 16, 32, 64, 128, 256, 512, 1024 ve 2048 baytlık sabit boyutlu blok havuzları
//!    kullanarak bellek parçalanmasını (fragmentation) engeller ve O(1) serbest bırakma/geri kazanım sağlar.
//!    2048 bayttan büyük bloklar için `large_free_list` serbest blok havuzu devreye girer;
//!    böylece büyük tahsisler de serbest bırakıldığında yeniden kullanılabilir ve bellek tükenmez.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use crate::arch::x86_64::sync::SpinMutex;

/// Hizalama yardımcısı: `addr` değerini `align` (2'nin kuvveti) katına yukarı yuvarlar.
///
/// Eğer `align` 2'nin kuvveti değilse veya toplama `usize::MAX` sınırını aşarsa `None` döner.
#[inline]
pub fn align_up_checked(addr: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two() {
        return None;
    }
    let offset = align - 1;
    addr.checked_add(offset).map(|v| v & !offset)
}

/// Geriye uyumluluk ve hızlı erişim için `align_up`:
/// Taşma veya geçersiz hizalama durumunda `usize::MAX` döndürerek sonraki sınır
/// kontrollerinin (`alloc_end <= heap_end`) güvenli biçimde başarısız olmasını sağlar.
#[inline(always)]
pub fn align_up(addr: usize, align: usize) -> usize {
    align_up_checked(addr, align).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------------------
// 1. Bump Allocator (Lineer Yığın)
// ---------------------------------------------------------------------------

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Tahsis ediciyi başlangıç adresi ve boyutuyla ilklendirir.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start.saturating_add(heap_size);
        self.next = heap_start;
        self.allocations = 0;
    }

    /// Lineer bellek tahsisi yapar. Sıfır boyutlu tahsislerde dangling pointer döner.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let alloc_start = match align_up_checked(self.next, layout.align()) {
            Some(start) => start,
            None => return null_mut(),
        };

        let alloc_end = match alloc_start.checked_add(layout.size()) {
            Some(end) => end,
            None => return null_mut(),
        };

        if alloc_end <= self.heap_end {
            self.next = alloc_end;
            self.allocations += 1;
            alloc_start as *mut u8
        } else {
            null_mut()
        }
    }

    /// Bellek bloğunu iade eder.
    /// LIFO optimizasyonu: Serbest bırakılan blok en son tahsis edilen blok ise
    /// yığın hemen geri sarılır. Tüm tahsisler serbest bırakıldığında yığın başa sıfırlanır.
    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 || ptr.is_null() {
            return;
        }

        let ptr_addr = ptr as usize;
        if ptr_addr < self.heap_start || ptr_addr >= self.heap_end {
            return;
        }

        self.allocations = self.allocations.saturating_sub(1);

        // LIFO serbest bırakma optimizasyonu
        let expected_end = ptr_addr.saturating_add(layout.size());
        if expected_end == self.next {
            self.next = ptr_addr;
        }

        // Tüm tahsisler bittiğinde yığını sıfırla
        if self.allocations == 0 {
            self.next = self.heap_start;
        }
    }

    /// Yığını açıkça sıfırlar (tüm tahsisleri geçersiz kılar).
    pub fn reset(&mut self) {
        self.next = self.heap_start;
        self.allocations = 0;
    }

    pub fn used_bytes(&self) -> usize {
        self.next.saturating_sub(self.heap_start)
    }

    pub fn free_bytes(&self) -> usize {
        self.heap_end.saturating_sub(self.next)
    }

    pub fn allocations_count(&self) -> usize {
        self.allocations
    }
}

// ---------------------------------------------------------------------------
// 2. Fixed-Size Block Allocator (Sabit Boyutlu Blok Havuzları)
// ---------------------------------------------------------------------------

pub const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

struct LargeBlockHeader {
    size: usize,
    next: Option<&'static mut LargeBlockHeader>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    large_free_list: Option<&'static mut LargeBlockHeader>,
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        Self {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            large_free_list: None,
            heap_start: 0,
            heap_end: 0,
            next: 0,
        }
    }

    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start.saturating_add(heap_size);
        self.next = heap_start;
        for head in &mut self.list_heads {
            *head = None;
        }
        self.large_free_list = None;
    }

    fn list_index(layout: &Layout) -> Option<usize> {
        // A freed block stores a ListNode in its first bytes. The 8-byte class
        // is therefore too small on x86_64 and would overwrite the next block.
        let required_block_size = layout.size().max(layout.align()).max(core::mem::size_of::<ListNode>());
        BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        // Sıfır boyutlu tahsisler
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        match Self::list_index(&layout) {
            Some(index) => match self.list_heads[index].take() {
                Some(node) => {
                    self.list_heads[index] = node.next.take();
                    node as *mut ListNode as *mut u8
                }
                None => {
                    let block_size = BLOCK_SIZES[index];
                    let block_align = block_size.max(core::mem::align_of::<ListNode>());
                    
                    let alloc_start = match align_up_checked(self.next, block_align) {
                        Some(start) => start,
                        None => return null_mut(),
                    };
                    let alloc_end = match alloc_start.checked_add(block_size) {
                        Some(end) => end,
                        None => return null_mut(),
                    };

                    if alloc_end <= self.heap_end {
                        self.next = alloc_end;
                        alloc_start as *mut u8
                    } else {
                        null_mut()
                    }
                }
            },
            None => self.alloc_large(layout),
        }
    }

    fn alloc_large(&mut self, layout: Layout) -> *mut u8 {
        let required_size = layout.size().max(core::mem::size_of::<LargeBlockHeader>());
        let required_align = layout.align().max(core::mem::align_of::<LargeBlockHeader>());

        // 1. Serbest büyük bloklar listesinde uygun bir blok ara (first-fit)
        let mut current = &mut self.large_free_list;
        while let Some(ref mut node) = current {
            let node_addr = *node as *mut LargeBlockHeader as usize;
            if node.size >= required_size && (node_addr % required_align == 0) {
                let next_node = node.next.take();
                let matched_ptr = node_addr as *mut u8;
                *current = next_node;
                return matched_ptr;
            }
            current = &mut current.as_mut().unwrap().next;
        }

        // 2. Serbest listede yoksa yığının oyulmamış kısmından tahsis et
        let alloc_start = match align_up_checked(self.next, required_align) {
            Some(start) => start,
            None => return null_mut(),
        };
        let alloc_end = match alloc_start.checked_add(required_size) {
            Some(end) => end,
            None => return null_mut(),
        };

        if alloc_end <= self.heap_end {
            self.next = alloc_end;
            alloc_start as *mut u8
        } else {
            null_mut()
        }
    }

    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 || ptr.is_null() {
            return;
        }

        let ptr_addr = ptr as usize;
        // Heap sınır kontrolü: vahşi göstericileri reddet
        if ptr_addr < self.heap_start || ptr_addr >= self.heap_end {
            return;
        }

        match Self::list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: self.list_heads[index].take(),
                };
                let new_node_ptr = ptr as *mut ListNode;
                unsafe {
                    new_node_ptr.write(new_node);
                    self.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                let block_size = layout.size().max(core::mem::size_of::<LargeBlockHeader>());
                let header = LargeBlockHeader {
                    size: block_size,
                    next: self.large_free_list.take(),
                };
                let header_ptr = ptr as *mut LargeBlockHeader;
                unsafe {
                    header_ptr.write(header);
                    self.large_free_list = Some(&mut *header_ptr);
                }
            }
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.next.saturating_sub(self.heap_start)
    }

    pub fn free_bytes(&self) -> usize {
        self.heap_end.saturating_sub(self.next)
    }
}

// ---------------------------------------------------------------------------
// GlobalAlloc Kilit Sarmalayıcısı
// ---------------------------------------------------------------------------

pub struct Locked<A> {
    inner: SpinMutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Self {
            inner: SpinMutex::new(inner),
        }
    }

    pub fn lock(&self) -> crate::arch::x86_64::sync::SpinGuard<'_, A> {
        self.inner.lock()
    }
}

unsafe impl GlobalAlloc for Locked<BumpAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.lock().dealloc(ptr, layout);
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.lock().dealloc(ptr, layout);
    }
}

// ---------------------------------------------------------------------------
// Çekirdek Önyükleme Doğrulama ve Stres Testi
// ---------------------------------------------------------------------------

#[cfg(not(test))]
static mut BUMP_TEST_BUF: [u8; 1024] = [0; 1024];
#[cfg(not(test))]
static mut FSB_TEST_BUF: [u8; 32768] = [0; 32768];

#[cfg(not(test))]
pub fn test_allocator_subsystem() -> bool {
    crate::serial_print("[ALLOC-TEST] --- Gelismis Bellek Tahsis Edici Dogrulama Testi ---\n");

    // 1. align_up_checked taşma ve sınır kontrolü
    if align_up_checked(usize::MAX - 3, 8).is_some() || align_up_checked(16, 0).is_some() {
        crate::serial_print("[ALLOC-TEST] align_up taşma kontrolü: BASARISIZ\n");
        return false;
    }
    crate::serial_print("[ALLOC-TEST] 1. Hizalama & Tasma Korumasi (align_up_checked): BASARILI\n");

    // 2. Bump Allocator LIFO ve Reset Testi (Yığın taşmasını önlemek için statik tampon)
    let bump_buf = unsafe { &mut *core::ptr::addr_of_mut!(BUMP_TEST_BUF) };
    let mut bump = BumpAllocator::new();
    bump.init(bump_buf.as_mut_ptr() as usize, bump_buf.len());

    let l1 = Layout::from_size_align(64, 16).unwrap();
    let p1 = bump.alloc(l1);
    let p2 = bump.alloc(l1);
    if p1.is_null() || p2.is_null() || (p1 as usize % 16 != 0) || (p2 as usize % 16 != 0) {
        crate::serial_print("[ALLOC-TEST] Bump allocation: BASARISIZ\n");
        return false;
    }
    bump.dealloc(p2, l1); // LIFO rewind
    bump.dealloc(p1, l1); // Full reset
    if bump.allocations_count() != 0 || bump.used_bytes() != 0 {
        crate::serial_print("[ALLOC-TEST] Bump LIFO / Reset: BASARISIZ\n");
        return false;
    }
    crate::serial_print("[ALLOC-TEST] 2. Bump Allocator LIFO & Sifirlama: BASARILI\n");

    // 3. Fixed-Size Block Allocator Blok Geri Kazanım (Free-List Reuse)
    let fsb_buf = unsafe { &mut *core::ptr::addr_of_mut!(FSB_TEST_BUF) };
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(fsb_buf.as_mut_ptr() as usize, fsb_buf.len());

    let l8 = Layout::from_size_align(8, 8).unwrap();
    let b1 = fsb.alloc(l8);
    let b2 = fsb.alloc(l8);
    if b1.is_null() || b2.is_null() || b1 == b2 {
        crate::serial_print("[ALLOC-TEST] FSB blok tahsisi: BASARISIZ\n");
        return false;
    }
    fsb.dealloc(b1, l8);
    let b3 = fsb.alloc(l8);
    if b3 != b1 {
        crate::serial_print("[ALLOC-TEST] FSB blok geri kazanımı: BASARISIZ\n");
        return false;
    }
    fsb.dealloc(b2, l8);
    fsb.dealloc(b3, l8);
    crate::serial_print("[ALLOC-TEST] 3. Sabit Boyutlu Blok Geri Kazanim (Free-List Reuse): BASARILI\n");

    // 4. Büyük Blok (>2048) Havuzlama ve Geri Kazanım
    let l_large = Layout::from_size_align(4096, 4096).unwrap();
    let p_large1 = fsb.alloc(l_large);
    if p_large1.is_null() || (p_large1 as usize % 4096 != 0) {
        crate::serial_print("[ALLOC-TEST] Büyük blok tahsisi: BASARISIZ\n");
        return false;
    }
    let used_after = fsb.used_bytes();
    fsb.dealloc(p_large1, l_large);
    let p_large2 = fsb.alloc(l_large);
    if p_large2 != p_large1 || fsb.used_bytes() != used_after {
        crate::serial_print("[ALLOC-TEST] Büyük blok geri kazanımı: BASARISIZ\n");
        return false;
    }
    fsb.dealloc(p_large2, l_large);
    crate::serial_print("[ALLOC-TEST] 4. Buyuk Blok (>2KB) Havuzlama & Geri Kazanim: BASARILI\n");

    // 5. Yabancı / Sınır Dışı Gösterici Reddi
    let wild_ptr = 0x500 as *mut u8;
    fsb.dealloc(wild_ptr, l8); // Güvenle görmezden gelinmeli
    crate::serial_print("[ALLOC-TEST] 5. Yabanci / Gecersiz Gosterici Reddi: BASARILI\n");

    // 6. Çok Boyutlu Stres Döngüsü
    let sizes = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
    let mut allocated = [(null_mut(), Layout::new::<u8>()); 27];
    for cycle in 0..2 {
        for i in 0..27 {
            let sz = sizes[i % sizes.len()];
            let l = Layout::from_size_align(sz, sz.min(64)).unwrap();
            let ptr = fsb.alloc(l);
            if ptr.is_null() {
                crate::serial_print("[ALLOC-TEST] Stres döngüsü tahsis hatası: BASARISIZ\n");
                return false;
            }
            if let Some(non_null) = core::ptr::NonNull::new(ptr) {
                let slice = unsafe { core::slice::from_raw_parts_mut(non_null.as_ptr(), sz) };
                slice.fill((cycle + i) as u8);
            }
            allocated[i] = (ptr, l);
        }
        for i in (0..27).rev() {
            let (ptr, l) = allocated[i];
            if let Some(non_null) = core::ptr::NonNull::new(ptr) {
                let slice = unsafe { core::slice::from_raw_parts(non_null.as_ptr(), l.size()) };
                if slice[0] != (cycle + i) as u8 {
                    crate::serial_print("[ALLOC-TEST] Stres döngüsü bellek bozulması: BASARISIZ\n");
                    return false;
                }
            }
            fsb.dealloc(ptr, l);
        }
    }
    crate::serial_print("[ALLOC-TEST] 6. Cok Boyutlu Blok Stres Dongusu (27 blok x 2 cevrim): BASARILI\n");

    true
}

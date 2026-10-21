//! Gelişmiş Çekirdek Bellek Tahsis Edicileri (Kernel Allocators).
//!
//! Bu modül iki temel özel tahsis edici (allocator) içerir:
//! 1. **Bump Allocator (Lineer Yığın):** O(1) hızında ileri doğru bellek tahsis eder.
//!    Tahsis edilen nesneler tek tek serbest bırakılamaz; tüm tahsisler tamamlandığında
//!    yığın başa sıfırlanabilir (`reset`).
//! 2. **Fixed-Size Block Allocator (Sabit Boyutlu Blok Havuzları):**
//!    8, 16, 32, 64, 128, 256, 512, 1024 ve 2048 baytlık sabit boyutlu blok havuzları
//!    kullanarak bellek parçalanmasını (fragmentation) engeller ve O(1) serbest bırakma sağlar.
//!    2048 bayttan büyük bloklar için yedek Bump Allocator devreye girer.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use crate::arch::x86_64::sync::SpinMutex;

/// Hizalama yardımcısı: `addr` değerini `align` (2'nin kuvveti) katına yukarı yuvarlar.
#[inline(always)]
pub fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
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

    /// Lineer bellek tahsisi yapar.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let alloc_start = align_up(self.next, layout.align());
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

    /// Bellek bloğunu iade eder. Tüm tahsisler bittiğinde başa döner.
    pub fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {
        self.allocations = self.allocations.saturating_sub(1);
        if self.allocations == 0 {
            self.next = self.heap_start;
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
// 2. Fixed-Size Block Allocator (Sabit Boyutlu Blok Havuzları)
// ---------------------------------------------------------------------------

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: BumpAllocator,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        Self {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: BumpAllocator::new(),
        }
    }

    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
        for head in &mut self.list_heads {
            *head = None;
        }
    }

    fn list_index(layout: &Layout) -> Option<usize> {
        let required_block_size = layout.size().max(layout.align());
        BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        match Self::list_index(&layout) {
            Some(index) => match self.list_heads[index].take() {
                Some(node) => {
                    self.list_heads[index] = node.next.take();
                    node as *mut ListNode as *mut u8
                }
                None => {
                    let block_size = BLOCK_SIZES[index];
                    let block_align = block_size;
                    let block_layout = Layout::from_size_align(block_size, block_align).unwrap();
                    self.fallback_allocator.alloc(block_layout)
                }
            },
            None => self.fallback_allocator.alloc(layout),
        }
    }

    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
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
                self.fallback_allocator.dealloc(ptr, layout);
            }
        }
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


/// Advanced Kernel Memory Allocators.
///
/// This module provides two specialized allocators for kernel memory management:
///
/// 1. **Bump Allocator (Linear Heap)**: O(1) forward-only allocation with
///    LIFO deallocation optimization and `reset()` support. Used for
///    temporary/short-lived allocations.
///
/// 2. **Fixed-Size Block Allocator**: Nine block pools (8, 16, 32, 64, 128,
///    256, 512, 1024, 2048 bytes) for common small allocations to prevent
///    fragmentation. Larger allocations (>2048 bytes) use a free-list
///    for efficient reuse.
///
/// # Design
///
/// - All operations are `no_std` compatible
/// - Thread-safe via [`SpinMutex`]
/// - Allocation tracking for debugging
/// - Configurable heap size at init
use crate::arch::x86_64::sync::SpinMutex;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

/// Rounds `addr` up to the next multiple of `align` (power of two).
///
/// Returns `None` if `align` is not a power of two or if the addition overflows.
#[inline]
pub fn align_up_checked(addr: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two() {
        return None;
    }
    let offset = align - 1;
    addr.checked_add(offset).map(|v| v & !offset)
}

/// Rounds `addr` up to `align`, returning `usize::MAX` on invalid input or overflow.
///
/// The saturating sentinel allows callers to fail bounds checks safely.
#[inline(always)]
pub fn align_up(addr: usize, align: usize) -> usize {
    align_up_checked(addr, align).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------------------
// 1. Bump Allocator (Linear Heap)
// ---------------------------------------------------------------------------

/// Simple forward-only allocator with O(1) allocation and optional LIFO reclamation.
///
/// Best suited for phase-scoped or short-lived allocations where bulk reset is common.
pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
}

impl BumpAllocator {
    /// Creates an uninitialized bump allocator.
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Initializes the allocator over `[heap_start, heap_start + heap_size)`.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start.saturating_add(heap_size);
        self.next = heap_start;
        self.allocations = 0;
    }

    /// Allocates memory with `layout`, or returns null on overflow/OOM.
    ///
    /// Zero-sized allocations return a dangling aligned pointer as per Rust allocator conventions.
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

    /// Frees `ptr`.
    ///
    /// Applies LIFO optimization when the block is at the heap tip and resets fully
    /// when the allocation count reaches zero.
    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 || ptr.is_null() {
            return;
        }

        let ptr_addr = ptr as usize;
        if ptr_addr < self.heap_start || ptr_addr >= self.heap_end {
            return;
        }

        self.allocations = self.allocations.saturating_sub(1);

        // LIFO free optimization
        let expected_end = ptr_addr.saturating_add(layout.size());
        if expected_end == self.next {
            self.next = ptr_addr;
        }

        // Reset when all allocations are released
        if self.allocations == 0 {
            self.next = self.heap_start;
        }
    }

    /// Resets the allocator, invalidating all outstanding allocations.
    pub fn reset(&mut self) {
        self.next = self.heap_start;
        self.allocations = 0;
    }

    /// Returns bytes currently consumed from the heap front.
    pub fn used_bytes(&self) -> usize {
        self.next.saturating_sub(self.heap_start)
    }

    /// Returns bytes still available before heap end.
    pub fn free_bytes(&self) -> usize {
        self.heap_end.saturating_sub(self.next)
    }

    /// Returns the number of live allocations tracked by this allocator.
    pub fn allocations_count(&self) -> usize {
        self.allocations
    }
}

// ---------------------------------------------------------------------------
// 2. Fixed-Size Block Allocator
// ---------------------------------------------------------------------------

/// Supported fixed block classes (bytes). Requests larger than 2048 use the large free list.
pub const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

struct LargeBlockHeader {
    size: usize,
    next: Option<&'static mut LargeBlockHeader>,
}

/// Block allocator with per-size free lists for small objects and a large-block free list.
///
/// Provides O(1) alloc/free for common sizes and reuses freed large blocks.
pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    large_free_list: Option<&'static mut LargeBlockHeader>,
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl FixedSizeBlockAllocator {
    /// Creates an uninitialized block allocator.
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

    /// Initializes the allocator over `[heap_start, heap_start + heap_size)`.
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
        let required_block_size = layout
            .size()
            .max(layout.align())
            .max(core::mem::size_of::<ListNode>());
        BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
    }

    /// Allocates memory from an appropriate block class or the large allocator path.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
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
        let required_align = layout
            .align()
            .max(core::mem::align_of::<LargeBlockHeader>());

        // 1. First-fit search in large free list
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

        // 2. Bump allocation from fresh heap space
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

    /// Frees memory back to the appropriate free list.
    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 || ptr.is_null() {
            return;
        }

        let ptr_addr = ptr as usize;
        // Reject pointers outside managed heap
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

    /// Returns bytes allocated from the bump frontier (excludes recycled list memory).
    pub fn used_bytes(&self) -> usize {
        self.next.saturating_sub(self.heap_start)
    }

    /// Returns bytes remaining before heap end in the bump frontier.
    pub fn free_bytes(&self) -> usize {
        self.heap_end.saturating_sub(self.next)
    }
}

// ---------------------------------------------------------------------------
// GlobalAlloc wrapper
// ---------------------------------------------------------------------------

/// Spinlock wrapper providing `GlobalAlloc`/`lock()` access for allocators.
pub struct Locked<A> {
    inner: SpinMutex<A>,
}

impl<A> Locked<A> {
    /// Wraps an allocator in a spinlock.
    pub const fn new(inner: A) -> Self {
        Self {
            inner: SpinMutex::new(inner),
        }
    }

    /// Locks and returns mutable access to the inner allocator.
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
// Boot-time self tests
// ---------------------------------------------------------------------------

#[cfg(not(test))]
static mut BUMP_TEST_BUF: [u8; 1024] = [0; 1024];
#[cfg(not(test))]
static mut FSB_TEST_BUF: [u8; 32768] = [0; 32768];

/// Runs allocator smoke tests during kernel boot (non-test builds).
#[cfg(not(test))]
pub fn test_allocator_subsystem() -> bool {
    crate::serial_print("[ALLOC-TEST] --- Gelişmiş bellek tahsis edici doğrulama testi ---\n");

    // 1. align_up_checked overflow and boundary checks
    if align_up_checked(usize::MAX - 3, 8).is_some() || align_up_checked(16, 0).is_some() {
        crate::serial_print("[ALLOC-TEST] align_up taşma kontrolü: BAŞARISIZ\n");
        return false;
    }
    crate::serial_print(
        "[ALLOC-TEST] 1. Hizalama ve taşma koruması (align_up_checked): BAŞARILI\n",
    );

    // 2. Bump allocator LIFO and reset
    let bump_buf = unsafe { &mut *core::ptr::addr_of_mut!(BUMP_TEST_BUF) };
    let mut bump = BumpAllocator::new();
    bump.init(bump_buf.as_mut_ptr() as usize, bump_buf.len());

    let l1 = Layout::from_size_align(64, 16).unwrap();
    let p1 = bump.alloc(l1);
    let p2 = bump.alloc(l1);
    if p1.is_null() || p2.is_null() || (p1 as usize % 16 != 0) || (p2 as usize % 16 != 0) {
        crate::serial_print("[ALLOC-TEST] Bump allocation: BAŞARISIZ\n");
        return false;
    }
    bump.dealloc(p2, l1); // LIFO rewind
    bump.dealloc(p1, l1); // Full reset
    if bump.allocations_count() != 0 || bump.used_bytes() != 0 {
        crate::serial_print("[ALLOC-TEST] Bump LIFO / Reset: BAŞARISIZ\n");
        return false;
    }
    crate::serial_print("[ALLOC-TEST] 2. Bump LIFO ve Reset optimizasyonu: BAŞARILI\n");

    // 3. Fixed-size allocator free list reuse
    let fsb_buf = unsafe { &mut *core::ptr::addr_of_mut!(FSB_TEST_BUF) };
    let mut fsb = FixedSizeBlockAllocator::new();
    fsb.init(fsb_buf.as_mut_ptr() as usize, fsb_buf.len());
    let l_small = Layout::from_size_align(64, 8).unwrap();
    let s1 = fsb.alloc(l_small);
    if s1.is_null() {
        crate::serial_print("[ALLOC-TEST] FSB alloc: BAŞARISIZ\n");
        return false;
    }
    fsb.dealloc(s1, l_small);
    let s2 = fsb.alloc(l_small);
    if s2 != s1 {
        crate::serial_print("[ALLOC-TEST] FSB free-list reuse: BAŞARISIZ\n");
        return false;
    }
    fsb.dealloc(s2, l_small);
    crate::serial_print("[ALLOC-TEST] 3. FSB free-list reuse: BAŞARILI\n");

    true
}

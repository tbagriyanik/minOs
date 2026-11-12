//! Çekirdek yığın (heap) ayırıcısı ve OOM tanılaması.
//! `main.rs` içinden `src/kernel/` katmanına taşındı.

use crate::boot::console::{serial_print, serial_print_dec};
use linked_list_allocator::LockedHeap;

pub const HEAP_SIZE: usize = 16 * 1024 * 1024;

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    serial_print("[OOM] Kernel heap allocation failed: size=");
    serial_print_dec(layout.size() as u64);
    serial_print(" align=");
    serial_print_dec(layout.align() as u64);
    serial_print("\n");

    // Best-effort diagnostic; avoid further allocations.
    serial_print("[OOM] Halting system to prevent memory corruption.\n");
    loop {
        core::hint::spin_loop();
    }
}

#[allow(static_mut_refs)]
pub fn init() {
    unsafe {
        ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE);
    }
    serial_print("[HEAP] Kernel heap initialized: ");
    serial_print_dec(HEAP_SIZE as u64);
    serial_print(" bytes\n");
}

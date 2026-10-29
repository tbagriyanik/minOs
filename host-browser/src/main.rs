#![no_std]
#![no_main]

extern crate alloc;

use linked_list_allocator::LockedHeap;

use core::panic::PanicInfo;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_SIZE: usize = 16 * 1024 * 1024;

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

mod app;
mod color;
mod css;
mod html;
mod imgdecode;
mod types;

use host_browser::app::HostBrowser;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE);
    }

    // minos kernel cagrisi: tarayici baslatilir ve cizim dongusu calistirilir.
    // Bu fonksiyon çekirdek tarafinda `host_browser_main` olarak cagrilir.
    let mut browser = HostBrowser::new();

    // Varsayilan olarak yardim sayfasini yukle.
    browser.navigate("minos://help");

    loop {
        core::hint::spin_loop();
    }
}

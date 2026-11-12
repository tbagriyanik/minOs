#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod arch;
pub mod kernel;
pub mod net_smoltcp;
pub mod random;

// Geriye dönük uyumluluk: eski crate kökü modül yolları.
pub use kernel::diag as kernel_diag;
pub use kernel::formats as kernel_formats;
pub use kernel::primitives as kernel_primitives;

pub mod mm;

pub mod fs;

pub mod drivers;

pub mod gui;

pub mod apps;

pub mod serial;

// --- Asama 1: ayristirilan moduller -------------------------------------------------

// Tasima yok: bu modullerdeki ogeler crate kokunde yeniden disa aktarilir, boylece

// `crate::serial_print`, `crate::monotonic_ms`, `crate::valid_limine_response` gibi

// cagrilar degismeden calismaya devam eder.

pub mod boot;

pub mod desktop;

pub mod task;

pub mod panic_handler;
pub mod time_budget;
pub mod watchdog;

// These modules were split out of this file, but the rest of the kernel still

// uses their names from the crate root.  A private `use` is sufficient for

// intra-crate lookup; `pub use` silently drops private items and caused the

// compiler to report hundreds of cascading "not found" errors.

// Çekirdeğin geri kalanı bu adları doğrudan `crate::...` üzerinden kullanır;
// bu nedenle kök kapsamda yeniden içe aktarılırlar (bkz. Asama 1 refactor).
#[allow(unused_imports)]
use boot::console::*;
#[allow(unused_imports)]
use boot::devices::*;
#[allow(unused_imports)]
use boot::limine::*;
#[allow(unused_imports)]
use boot::progress::*;
#[allow(unused_imports)]
use boot::time::*;
#[allow(unused_imports)]
use desktop::helpers::*;

pub(crate) static mut MEMORY_MGR: mm::MemoryManager = mm::MemoryManager::new();

fn alloc_pmm_frame_global() -> Option<u64> {
    unsafe { (*core::ptr::addr_of_mut!(MEMORY_MGR)).pmm.allocate_frame() }
}

/// Çekirdek giriş noktası: masaüstü çalışma zamanını başlatır.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    gui::event_loop::run_desktop(boot::start::init())
}


/// Terminalde `edit <dosya>` / `start <dosya>` calistirildiysa istenen
/// dosyayi uygun uygulamada acar (metin -> Not Defteri, digerleri -> Gezgin).
pub(crate) fn handle_terminal_open_request(
    terminal_app: &mut apps::terminal::TerminalApp,
    vfs: &mut fs::vfs::VirtualFileSystem,
    notepad_app: &mut apps::notepad::NotepadApp,
    browser_app: &mut apps::browser::BrowserApp,
    compositor: &mut gui::MinGuiCompositor,
) {
    let Some(path) = terminal_app.take_open_request() else {
        return;
    };

    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();

    let is_web = matches!(ext.as_str(), "html" | "htm" | "xhtml");

    if is_web {
        match vfs.read_file(&path) {
            Some(data) => {
                let data: alloc::vec::Vec<u8> = alloc::vec::Vec::from(data);
                let wrap = compositor
                    .windows
                    .iter()
                    .flatten()
                    .find(|w| w.app_id == 3u8)
                    .map_or(700, |w| w.width.saturating_sub(40));
                browser_app.set_url(path.as_bytes());
                browser_app.load_html(&data, wrap, 256, Some(&mut *vfs));
                open_app(compositor, 3);
                return;
            }
            None => {
                browser_app.set_url(path.as_bytes());
                browser_app.navigate();
                open_app(compositor, 3);
                return;
            }
        }
    }

    match vfs.read_file(&path) {
        Some(data) => {
            let data: alloc::vec::Vec<u8> = alloc::vec::Vec::from(data);
            notepad_app.open_file(&path, &data);
            open_app(compositor, 2);
        }
        None => {
            // Dosya yoksa Not Defteri'ni bos olarak acip yolu hazirlar.
            notepad_app.open_file(&path, b"");
            open_app(compositor, 2);
        }
    }
}

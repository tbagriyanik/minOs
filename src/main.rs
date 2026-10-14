#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;

#[allow(dead_code)]
const HEAP_SIZE: usize = 1024 * 1024;
#[allow(dead_code)]
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    serial_print("[PANIC] Kernel heap allocation failed: size=");
    let mut v = layout.size() as u64;
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if v == 0 {
        serial_print("0");
    } else {
        while v > 0 && i > 0 {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        if let Ok(s) = core::str::from_utf8(&buf[i..]) {
            serial_print(s);
        }
    }
    serial_print(" align=");
    let mut a = layout.align() as u64;
    let mut i2 = buf.len();
    while a > 0 && i2 > 0 {
        i2 -= 1;
        buf[i2] = b'0' + (a % 10) as u8;
        a /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i2..]) {
        serial_print(s);
    }
    serial_print("\n");
    panic!("Allocation error");
}

#[allow(static_mut_refs)]
fn init_heap() {
    unsafe { ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE); }
}
pub mod arch;
pub mod mm;
pub mod fs;
pub mod drivers;
pub mod gui;
pub mod apps;
pub mod serial;

// Limine Protokolü İstek Yapıları (Sıfır Dış Bağımlılık)
#[repr(C, align(8))]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

#[repr(C, align(8))]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

unsafe impl Sync for LimineFramebufferRequest {}

#[used]
#[link_section = ".requests_start_marker"]
static LIMINE_REQUESTS_START_MARKER: [u64; 4] = [
    0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf,
    0x785c6ed015d3e316, 0x181e920a7852b9d9,
];

#[used]
#[link_section = ".requests_base_revision"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3,
];

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

#[used]
#[link_section = ".requests_end_marker"]
static LIMINE_REQUESTS_END_MARKER: [u64; 2] = [
    0xadc0e0531bb10d03, 0x9572709f31764c62,
];

// --- Limine HHDM (Higher-Half Direct Map) istegi: fiziksel<->sanal donusum ---
#[repr(C, align(8))]
pub struct LimineHhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C, align(8))]
pub struct LimineHhdmRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineHhdmResponse,
}

#[repr(C, align(8))]
pub struct LimineMemoryMapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: u64,
    pub unused: [u64; 4],
}

#[repr(C, align(8))]
pub struct LimineMemoryMapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *const *const LimineMemoryMapEntry,
}

#[repr(C, align(8))]
pub struct LimineMemoryMapRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineMemoryMapResponse,
}

unsafe impl Sync for LimineMemoryMapRequest {}

unsafe impl Sync for LimineHhdmRequest {}

#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b, 0x48dcf1cb8ad2b852, 0x63984e959a98244b],
    revision: 0,
    response: core::ptr::null(),
};

#[repr(C, align(8))]
pub struct LimineStackSizeResponse {
    pub revision: u64,
}

#[repr(C, align(8))]
pub struct LimineStackSizeRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineStackSizeResponse,
    pub stack_size: u64,
}

unsafe impl Sync for LimineStackSizeRequest {}

#[used]
#[link_section = ".requests"]
static STACK_SIZE_REQUEST: LimineStackSizeRequest = LimineStackSizeRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x224ef0460a8e8926,
        0xe1cb0fc25f46ea3d,
    ],
    revision: 0,
    response: core::ptr::null(),
    stack_size: 1024 * 1024, // 1 MiB kernel stack
};

#[used]
#[link_section = ".requests"]
static MEMORY_MAP_REQUEST: LimineMemoryMapRequest = LimineMemoryMapRequest {
    id: [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b, 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    revision: 0,
    response: core::ptr::null(),
};

#[inline(never)]
fn serial_print(s: &str) {
    serial::print(s);
}

#[inline(never)]
fn serial_print_dec(v: u64) {
    serial::print_dec(v);
}

#[inline(never)]
fn serial_print_hex(v: u64) {
    serial::print_hex(v);
}


fn restart_system() -> ! {
    serial_print("[minOs] Restart requested from Start Menu.\n");
    unsafe {
        // QEMU/PC reset control register, then legacy 8042 fallback.
        core::arch::asm!("out dx, al", in("dx") 0xCF9u16, in("al") 0x06u8);
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8);
        core::arch::asm!("cli");
    }
    loop { core::hint::spin_loop(); }
}

fn read_cpu_cycles() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((high as u64) << 32) | low as u64
}

static mut TSC_CYCLES_PER_US: u64 = 1;

/// TSC frekansını PIT kanal 2 ile kalibre eder.
fn init_timebase() {
    unsafe {
        let mut port61: u8;
        core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
        let saved = port61;
        port61 = (port61 & !0x03) | 0x01;
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
        // PIT kanal 2 mode 0, 65536 sayım ≈ 54.9 ms.
        core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") 0u8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") 0u8, options(nomem, nostack, preserves_flags));
        let start = read_cpu_cycles();
        loop {
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
            if port61 & 0x20 != 0 { break; }
            core::hint::spin_loop();
        }
        let elapsed = read_cpu_cycles().wrapping_sub(start).max(1);
        TSC_CYCLES_PER_US = (elapsed / 54_925).max(1);
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") saved, options(nomem, nostack, preserves_flags));
    }
}

fn monotonic_ms() -> u64 {
    let cycles_per_us = unsafe { TSC_CYCLES_PER_US }.max(1);
    read_cpu_cycles() / cycles_per_us / 1_000
}

pub(crate) fn main_monotonic_ms() -> usize { monotonic_ms() as usize }

fn cpu_vendor() -> &'static str {
    let result = core::arch::x86_64::__cpuid(0);
    let ebx = result.ebx;
    let edx = result.edx;
    let ecx = result.ecx;
    if ebx == u32::from_le_bytes(*b"Genu") && edx == u32::from_le_bytes(*b"ineI") && ecx == u32::from_le_bytes(*b"ntel") {
        "Intel"
    } else if ebx == u32::from_le_bytes(*b"Auth") && edx == u32::from_le_bytes(*b"enti") && ecx == u32::from_le_bytes(*b"cAMD") {
        "AMD"
    } else {
        "x86_64 uyumlu"
    }
}

fn cpu_logical_count() -> u32 {
    let result = core::arch::x86_64::__cpuid(1);
    ((result.ebx >> 16) & 0xFF).max(1)
}

#[inline]
fn is_canonical_ptr<T>(ptr: *const T) -> bool {
    if ptr.is_null() {
        return false;
    }
    let addr = ptr as usize as u64;
    let upper = addr >> 47;
    (upper == 0 || upper == 0x1_FFFF) && addr % core::mem::align_of::<T>() as u64 == 0
}

/// Limine yanıt yuvasını (`&REQUEST.response`) güvenli biçimde okur.
///
/// Okuma yapıldıktan sonra dönen işaretçi null olmamalı, doğru hizalanmalı ve
/// canonical bir adres olmalıdır. Tüm bu koşullar sağlandığında `Some` döner;
/// aksi halde `None` döner ve çağıran taraf yanıtı hiç dereference etmez.
/// Böylece tek bir noktada doğrulanan işaretçi CodeQL için de temiz kalır.
///
/// # Safety
/// `slot`, `&REQUEST.response` gibi geçerli ve okunabilir bir yuvaya işaret
/// etmelidir.
#[inline]
unsafe fn valid_limine_response<T>(slot: *const *const T) -> Option<*const T> {
    if slot.is_null() || !is_canonical_ptr(slot) {
        return None;
    }
    let ptr = core::ptr::read_volatile(slot);
    if ptr.is_null() || !is_canonical_ptr(ptr) { None } else { Some(ptr) }
}

/// Uygulama başlatma isteğinde yeni örnek açar; tekil uygulamalar var olan pencereye gider.
fn open_app(compositor: &mut gui::MinGuiCompositor, app_id: usize) {
    if let Some(idx) = compositor.launch_app_window(app_id) {
        if let Some(win) = compositor.windows[idx].as_mut() {
            win.is_open = true;
            win.is_minimized = false;
            win.is_maximized = false;
        }
        compositor.focus_window(idx);
    }
}

/// Belirli bir pencere örneğini açar ve odaklar.
fn focus_app_window(compositor: &mut gui::MinGuiCompositor, idx: usize) {
    if let Some(win) = compositor.windows.get_mut(idx).and_then(Option::as_mut) {
        win.is_open = true;
        win.is_minimized = false;
        win.is_maximized = false;
    }
    compositor.focus_window(idx);
}

fn keep_windows_on_screen(compositor: &mut gui::MinGuiCompositor, screen_w: usize, screen_h: usize) {
    let screen_w = screen_w.max(1);
    let usable_h = screen_h.saturating_sub(gui::taskbar_h()).max(1);
    for window in compositor.windows.iter_mut().flatten() {
        if !window.is_open { continue; }
        window.width = window.width.min(screen_w).max(1);
        window.height = window.height.min(usable_h).max(1);
        window.x = window.x.min(screen_w.saturating_sub(window.width));
        window.y = window.y.min(usable_h.saturating_sub(window.height));
    }
}

fn terminate_app(
    idx: usize,
    compositor: &mut gui::MinGuiCompositor,
    fm: &mut apps::filemanager::FileManagerApp,
    settings: &mut apps::settings::SystemSettings,
    notepad: &mut apps::notepad::NotepadApp,
    browser: &mut apps::browser::BrowserApp,
    imageviewer: &mut apps::imageviewer::ImageViewerApp,
    musicplayer: &mut apps::musicplayer::MusicPlayerApp,
    videoplayer: &mut apps::videoplayer::VideoPlayerApp,
    taskmgr: &mut apps::taskmgr::TaskManagerApp,
    calculator: &mut apps::calculator::CalculatorApp,
    paint: &mut Option<apps::paint::PaintApp>,
    terminal: &mut apps::terminal::TerminalApp,
) {
    let app_id = compositor.app_id_for_window(idx);
    if let Some(window) = compositor.windows.get_mut(idx).and_then(Option::as_mut) {
        window.is_open = false;
        window.is_minimized = false;
        window.is_focused = false;
        window.is_maximized = false;
        window.scroll.x = 0;
        window.scroll.y = 0;
    } else {
        return;
    }

    let another_instance_open = compositor.windows.iter().any(|slot| {
        slot.map_or(false, |window| window.app_id as usize == app_id && window.is_open)
    });
    if !another_instance_open { match app_id {
        0 => *fm = apps::filemanager::FileManagerApp::new(),
        1 => settings.active_tab = 0,
        2 => notepad.clear(),
        3 => *browser = apps::browser::BrowserApp::new(),
        4 => *imageviewer = apps::imageviewer::ImageViewerApp::new(),
        5 => {
            *musicplayer = apps::musicplayer::MusicPlayerApp::new();
            drivers::hda::silence();
            // PC speaker çıkışını da kapat.
            let audio = drivers::audio::AudioDevice::new();
            audio.set_pc_speaker_tone(0);
        }
        6 => *videoplayer = apps::videoplayer::VideoPlayerApp::new(),
        7 => *taskmgr = apps::taskmgr::TaskManagerApp::new(),
        8 => *calculator = apps::calculator::CalculatorApp::new(),
        9 => *paint = None,
        10 => {
            *terminal = apps::terminal::TerminalApp::new();
            terminal.init_banner();
        }
        _ => return,
    } }
    compositor.focus_top_window();
}

fn app_is_focused(compositor: &gui::MinGuiCompositor, app_id: usize) -> bool {
    compositor.windows.iter().any(|slot| {
        slot.map_or(false, |window| window.app_id as usize == app_id && window.is_open && !window.is_minimized && window.is_focused)
    })
}

fn focused_window_for_app(compositor: &gui::MinGuiCompositor, app_id: usize) -> Option<(usize, gui::Window)> {
    compositor.windows.iter().enumerate().find_map(|(idx, slot)| {
        slot.filter(|window| window.app_id as usize == app_id && window.is_open && !window.is_minimized && window.is_focused)
            .map(|window| (idx, window))
    })
}

/// Bir pencerenin (uygulama indeksine göre) dikey içerik yüksekliğini hesaplar.
/// Kaydırma çubuğunun gerekip gerekmediğini ve tutamaç boyutunu belirlemek için
/// hem çizim hem de tıklama kod yollarında ortak kullanılır.
fn window_content_h(
    app_id: usize,
    _win_h: usize,
    win_w: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
) -> usize {
    match app_id {
        // Dosya Yöneticisi: liste içeriği (üst araç çubuğu + satırlar + durum çubuğu).
        0 => apps::filemanager::list_top() + fm.list_content_h() + 30,
        // Not Defteri: satır kaydırma (wrap) sonrası metin yüksekliği.
        // Satır yüksekliği ve karakter genişliği font ölçeğine duyarlı olmalı.
        2 => {
            let char_w = gui::sp(12);
            let line_h = gui::sp(16);
            let text_w = win_w.saturating_sub(gui::sp(12) + gui::sp(32) + gui::sp(16));
            let wrap_cols = (text_w / char_w.max(1)).max(1);
            let (lines, _) = np.text_extent(wrap_cols);
            lines * line_h + gui::sp(58) + gui::sp(22)
        }
        // Görev Yöneticisi: başlık + süreç satırları + bellek grafiği + durum.
        7 => 275,
        // Diğer uygulamalar sabit düzenlidir: kaydırma gerekmez, içerik pencereye
        // kırpılır (clip). `0` döndürmek "kaydırma yok" anlamına gelir.
        _ => 0,
    }
}

/// Takvimde ay ileri/geri gezinme.
fn step_month(year: u16, month: u8, forward: bool) -> (u16, u8) {
    if forward {
        if month >= 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        }
    } else if month <= 1 {
        (year.saturating_sub(1), 12)
    } else {
        (year, month - 1)
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    arch::x86_64::enable_sse();
    arch::x86_64::disable_pic();
    init_timebase();
    // Initialize heap for dynamic allocations
    init_heap();
    serial_print("[minOs] Kernel _start reached!\n");
    serial_print("[minOs] SSE enabled & PIC IRQs masked!\n");

    // HHDM ofsetini al (fiziksel MMIO ve sayfa tablosu sanal erisimi icin).
    let hhdm_offset = unsafe {
        // SAFETY: the response pointer is validated before being dereferenced.
        if let Some(resp) = valid_limine_response(core::ptr::addr_of!(HHDM_REQUEST.response))
            .filter(|resp| !resp.is_null())
        {
            let off = core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset));
            serial_print("[minOs] HHDM offset okundu (0 yoksa gecerli).\n");
            off
        } else {
            serial_print("[minOs] UYARI: HHDM response yok; MMIO erisimi calismaz!\n");
            0xFFFF_8000_0000_0000
        }
    };
    drivers::set_hhdm_offset(hhdm_offset);

    // 1. Donanım Kesmeleri (IDT) ve PMM Başlatımı
    arch::x86_64::init_interrupts();
    let mut memory_mgr = mm::MemoryManager::new();
    unsafe {
        // SAFETY: `response` is validated (non-null, aligned, canonical) by
        // `valid_limine_response` before the response object is dereferenced.
        if let Some(response) = valid_limine_response(core::ptr::addr_of!(MEMORY_MAP_REQUEST.response))
            .filter(|response| !response.is_null())
        {
            let count = core::ptr::read_volatile(core::ptr::addr_of!((*response).entry_count));
            let entries = core::ptr::read_volatile(core::ptr::addr_of!((*response).entries));
            let count = count.min(256) as usize;
            let array_bytes = count.checked_mul(core::mem::size_of::<*const LimineMemoryMapEntry>());
            if count > 0 && !entries.is_null() && is_canonical_ptr(entries) && array_bytes.is_some() {
                let array_end = (entries as usize).checked_add(array_bytes.unwrap());
                if array_end.map_or(false, |end| is_canonical_ptr(end as *const LimineMemoryMapEntry)) {
                for i in 0..count {
                    let slot_addr = match (entries as usize).checked_add(i * core::mem::size_of::<*const LimineMemoryMapEntry>()) {
                        Some(addr) => addr,
                        None => break,
                    };
                    let slot = slot_addr as *const *const LimineMemoryMapEntry;
                    if slot.is_null() || !is_canonical_ptr(slot) { break; }
                    let entry_ptr = core::ptr::read_volatile(slot);
                    if entry_ptr.is_null() || !is_canonical_ptr(entry_ptr) { continue; }
                    // SAFETY: entry pointer is supplied by Limine and checked
                    // for null, alignment and canonical address before reads.
                    let base = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).base));
                    let length = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).length));
                    let entry_type = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).entry_type));
                    if length == 0 || base.checked_add(length).is_none() { continue; }
                    let kind = match entry_type {
                        0 => mm::pmm::MemoryEntryType::Usable,
                        1 => mm::pmm::MemoryEntryType::Reserved,
                        2 => mm::pmm::MemoryEntryType::AcpiReclaimable,
                        3 => mm::pmm::MemoryEntryType::AcpiNvs,
                        4 => mm::pmm::MemoryEntryType::BadMemory,
                        5 => mm::pmm::MemoryEntryType::BootloaderReclaimable,
                        6 => mm::pmm::MemoryEntryType::KernelAndModules,
                        7 => mm::pmm::MemoryEntryType::Framebuffer,
                        _ => continue,
                    };
                    memory_mgr.pmm.add_region(base, length, kind);
                }
                }
            }
        }
    }
    serial_print("[PMM] Fiziksel bellek yoneticisi baslatildi.\n");

    // PMM Kendi Kendini Test Etme (Tahsisat ve Geri Birakma Dogrulamasi)
    if let (Some(f1), Some(f2)) = (memory_mgr.pmm.allocate_frame(), memory_mgr.pmm.allocate_frame()) {
        if f1 != f2 && f1 % mm::pmm::PAGE_SIZE == 0 && f2 % mm::pmm::PAGE_SIZE == 0 {
            serial_print("[PMM] Cerceve tahsis testi basarili (cakismayan cerceveler dogrulandi).\n");
        }
        memory_mgr.pmm.free_frame(f2);
        memory_mgr.pmm.free_frame(f1);
    }

    // VMM ve Guvenli Uygulama Yukleyicisi Dogrulamasi (Sanal Bellek Eşleme ve Yükleme Testi)
    let mut address_space = memory_mgr.kernel_address_space();
    let test_vaddr = mm::loader::USER_CODE_BASE;
    if let Some(frame) = memory_mgr.pmm.allocate_frame() {
        if address_space.map_page(test_vaddr, frame, mm::vmm::PageFlags::USER_CODE, &mut memory_mgr.pmm).is_ok() {
            if let Some(translated_phys) = address_space.translate(test_vaddr) {
                if translated_phys == frame {
                    serial_print("[VMM] 4-seviyeli sanal bellek esleme ve adres cozümleme aktif.\n");
                }
            }
            let _ = address_space.unmap_page(test_vaddr);
        }
        memory_mgr.pmm.free_frame(frame);
    }

    // Test amacli ornek kucuk uygulama bayt dizisi (ret / nop talimatlari)
    let sample_app_code: [u8; 8] = [0x90, 0x90, 0x90, 0xC3, 0x00, 0x00, 0x00, 0x00];
    if let Ok(loaded) = mm::loader::AppLoader::load_binary(&sample_app_code, &mut address_space, &mut memory_mgr.pmm) {
        if loaded.entry_point == mm::loader::USER_CODE_BASE && loaded.stack_pointer > 0 {
            serial_print("[LOADER] Uygulama kodu guvenli bellek izolasyonu ile yerlestirildi.\n");
        }
    }

    // Surec Yoneticisi ve Sistem Cagrilarinin (Syscalls) Baslatimi
    memory_mgr.proc_mgr.init_kernel_process();
    // Sistem cagrilarini test et (sys_getpid & sys_alloc_frame)
    let test_pid = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_GETPID, 0, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    let test_frame = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    if test_frame != 0 {
        let _ = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_FREE_FRAME, test_frame, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    }
    if test_pid == 0 {
        serial_print("[PROC] Surec tablosu ve sistem cagrilar (syscalls) aktif.\n");
    }

    // 2. Sanal Dosya Sistemi (VFS / TarFS) Başlatımı
    let mut fs_mgr = fs::FileSystemManager::new();

    // 3. Ağ ve Ses Sürücülerinin Başlatımı (HDA + e1000)
    // Sabit adresler (0x300000, 0x400000) yerine PMM'den güvenli, çakışmayan bitişik DMA çerçeveleri tahsis et.
    // HDA DMA Havuzu: ~20 KB (~5 sayfa)
    let hda_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(6);
    if hda_dma_phys.is_some() {
        serial_print("[DMA] HDA ses surucusu icin dinamik DMA havuzu tahsis edildi.\n");
    }
    drivers::hda::init(hda_dma_phys);
    if drivers::hda::is_ready() {
        drivers::hda::play_test_tone();
    }

    // e1000 RX/TX Ring ve Tampon Havuzu: ~132 KB (~33 sayfa)
    let e1000_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(34);
    if e1000_dma_phys.is_some() {
        serial_print("[DMA] e1000 ag karti icin dinamik DMA ring havuzu tahsis edildi.\n");
    }
    drivers::e1000::init(e1000_dma_phys);

    drivers::netstack::init();
    let _ = drivers::netstack::dhcp_request();
    drivers::usb::init();
    drivers::usb::test_usb_subsystems();
    let _net_dev = drivers::net::NetworkInterface::new();
    let mut audio_dev = drivers::audio::AudioDevice::new();

    // 4. Limine Framebuffer Alımı
    let (fb_ptr, width, height, pitch, bpp) = unsafe {
        // SAFETY: the response pointer is validated before being dereferenced.
        if let Some(resp) = valid_limine_response(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))
            .filter(|resp| !resp.is_null())
        {
            let framebuffer_count = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffer_count));
            let fb_ptr_ptr = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffers));
            if framebuffer_count == 0 || fb_ptr_ptr.is_null() || !is_canonical_ptr(fb_ptr_ptr) {
                serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\n");
                loop { core::hint::spin_loop(); }
            }
            // SAFETY: fb_ptr_ptr is non-null/aligned/canonical and points to the
            // first framebuffer entry of the response above.
            let fb_info_ptr = core::ptr::read_volatile(fb_ptr_ptr);
            if fb_info_ptr.is_null() || !is_canonical_ptr(fb_info_ptr) {
                serial_print("[minOs] ERROR: Limine framebuffer entry is NULL!\n");
                loop { core::hint::spin_loop(); }
            }
            let fb_info = core::ptr::read_volatile(fb_info_ptr);
            if fb_info.address.is_null() || !is_canonical_ptr(fb_info.address as *const u8) {
                serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical!\n");
                loop { core::hint::spin_loop(); }
            }
            (
                fb_info.address as *mut u32,
                fb_info.width as usize,
                fb_info.height as usize,
                fb_info.pitch as usize,
                fb_info.bpp as usize,
            )
        } else {
            serial_print("[minOs] ERROR: Limine Framebuffer response is NULL!\n");
            loop {
                core::hint::spin_loop();
            }
        }
    };

    serial_print("[VIDEO] Limine Framebuffer alindi: ");
    serial_print_dec(width as u64);
    serial_print("x");
    serial_print_dec(height as u64);
    serial_print(" @ ");
    serial_print_dec(bpp as u64);
    serial_print(" bpp | Pitch: ");
    serial_print_dec(pitch as u64);
    serial_print(" byte (");
    serial_print_dec((pitch / 4) as u64);
    serial_print(" kelime) | Adres: ");
    serial_print_hex(fb_ptr as u64);
    serial_print("\n");

    let mut fb = unsafe {
        drivers::video::Framebuffer::new(fb_ptr, width, height, pitch, bpp)
    };

    if fb.verify_bounds() {
        serial_print("[VIDEO] Cizim sinirlari, kirpma ve pitch guvenligi dogrulandi.\n");
    } else {
        serial_print("[VIDEO] UYARI: Cizim sinirlari veya pitch dogrulamasi basarisiz!\n");
    }

    // 5. PS/2 Donanım Sürücüleri
    let mut mouse = drivers::mouse::MouseState::new();
    let mut keyboard = drivers::keyboard::KeyboardState::new();
    drivers::ps2::init();

    // 5b. Tema motorunu başlat
    gui::theme::init();
    serial_print("[minOs] Theme engine initialized.\n");

    // İlk açılış ekranı: kedi logosu ve ilerleme animasyonu.
    fb.draw_rect(0, 0, width, height, gui::Color::DARK_BG);
    let cx = width.saturating_sub(32) / 2;
    let cy = height.saturating_sub(32) / 2;
    gui::MinGuiCompositor::draw_cat_icon(&mut fb, cx, cy.saturating_sub(18));
    gui::MinGuiCompositor::draw_string(&mut fb, cx.saturating_sub(8), cy + 8, "minOs", gui::Color::TEXT_MAIN);
    for phase in 0..3 {
        let dots = match phase { 0 => ".  ", 1 => ".. ", _ => "..." };
        gui::MinGuiCompositor::draw_string(&mut fb, cx + 34, cy + 8, dots, gui::Color::ACCENT_BLUE);
        for _ in 0..180_000 { core::hint::spin_loop(); }
    }
    serial_print("[minOs] Splash rendered; initializing GUI state.\n");

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar
    let mut compositor = gui::MinGuiCompositor::new();
    serial_print("[minOs] Compositor initialized.\n");
    let mut filemanager_app = apps::filemanager::FileManagerApp::new();
    serial_print("[minOs] File manager state initialized.\n");
    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
    serial_print("[minOs] File manager populated.\n");
    let mut settings_app = apps::settings::SystemSettings::default();
    serial_print("[minOs] Settings state initialized.\n");
    settings_app.display_res = (width, height);
    settings_app.load_from_vfs(&fs_mgr.vfs);
    keyboard.layout = settings_app.keyboard_layout;
    gui::theme::set_theme(settings_app.theme);
    let mut notepad_app = apps::notepad::NotepadApp::new();
    serial_print("[minOs] Notepad state initialized.\n");
    let mut browser_app = apps::browser::BrowserApp::new();
    serial_print("[minOs] Browser state initialized.\n");
    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    serial_print("[minOs] ImageViewer state initialized.\n");
    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    serial_print("[minOs] MusicPlayer state initialized.\n");
    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    serial_print("[minOs] VideoPlayer state initialized.\n");
    let mut taskmgr_app = apps::taskmgr::TaskManagerApp::new();
    serial_print("[minOs] TaskManager state initialized.\n");
    let mut calculator_app = apps::calculator::CalculatorApp::new();
    serial_print("[minOs] Calculator state initialized.\n");
    // Başlangıçta yalnızca sabit uygulama pencereleri için state oluştur.
    // MAX_WINDOWS kadar 4096 noktalı tuval ayırmak 1 MiB kernel heap'ini aşar.
    let mut paint_apps: alloc::vec::Vec<Option<apps::paint::PaintApp>> = (0..gui::MAX_WINDOWS).map(|_| None).collect();
    serial_print("[minOs] Paint slots initialized.\n");
    let mut terminal_app = apps::terminal::TerminalApp::new();
    serial_print("[minOs] Terminal state initialized.\n");
    terminal_app.init_banner();
    serial_print("[minOs] App state initialized.\n");

    // 11 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimliği).
    // Açılışta TÜM pencereler kapalıdır; kullanıcı temiz bir masaüstüyle başlar
    // ve uygulamaları Başlat Menüsü, masaüstü simgeleri veya sağ tık ile açar.
    // 0: Dosya Yöneticisi
    let files_width = 720.min(width.saturating_sub(20)).max(200);
    let files_height = 500.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "Dosya Yöneticisi", "Dosyalar",
        width.saturating_sub(files_width) / 2,
        height.saturating_sub(files_height + gui::taskbar_h()) / 2,
        files_width, files_height, false,
    ));
    // 1: Sistem Ayarları
    let settings_width = 700.min(width.saturating_sub(20)).max(200);
    let settings_height = 540.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "Sistem Ayarları", "Ayarlar",
        width.saturating_sub(settings_width) / 2,
        height.saturating_sub(settings_height + gui::taskbar_h()) / 2,
        settings_width, settings_height, false,
    ));
    // 2: Not Defteri
    compositor.add_window(gui::Window::new("Not Defteri", "Notlar", 80, 70, 520, 330, false));
    // 3: Web Tarayıcısı
    compositor.add_window(gui::Window::new("Web Gezgini", "Web", 550, 60, 600, 380, false));
    // 4: Resim Gösterici
    compositor.add_window(gui::Window::new("Resim Gösterici", "Resim", 120, 90, 500, 330, false));
    // 5: Müzik Çalar
    compositor.add_window(gui::Window::new("Müzik Çalar", "Müzik", 560, 80, 460, 245, false));
    // 6: Video Oynatıcı
    compositor.add_window(gui::Window::new("Video Oynatıcı", "Video", 150, 110, 540, 330, false));
    // 7: Görev Yöneticisi
    compositor.add_window(gui::Window::new("Görev Yöneticisi", "Görevler", 220, 60, 480, 460, false));
    compositor.add_window(gui::Window::new("Hesap Makinesi", "Hesap", 260, 120, 360, 340, false));
    compositor.add_window(gui::Window::new("Çizim Tahtası", "Çizim", 180, 80, 520, 360, false));
    compositor.add_window(gui::Window::new("Komut İstemi", "Komut", 220, 100, 560, 300, false));

    // Açılışta hiçbir pencere odaklı değildir (temiz masaüstü).
    compositor.active_window_idx = None;
    serial_print("[minOs] Window list initialized.\n");

    // Her açılışta dil ve masaüstü teması seçilebilen hoş geldin ekranı.
    let mut welcome_was_left_down = false;
    let mut welcome_dirty = true;
    let mut welcome_mouse_pos = (mouse.x, mouse.y);
    let mut welcome_accepted = false;
    serial_print("[minOs] Entering welcome screen.\n");
    loop {
        if welcome_dirty {
            gui::render_welcome_screen(&mut fb, width, height, settings_app.language.is_english(), settings_app.theme, &mouse);
            welcome_dirty = false;
        }
        drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);

        // Enter veya Escape ile hoş geldin ekranını geç.
        while let Some(action) = keyboard.take_last_action() {
            if matches!(action, drivers::keyboard::KeyAction::EnterKey) {
                welcome_accepted = true;
            }
        }
        while let Some(ch) = keyboard.take_last_char() {
            if ch == '\x1b' {
                welcome_accepted = true;
            }
        }
        if welcome_accepted {
            settings_app.save_all(&compositor, &mut fs_mgr.vfs);
            break;
        }

        if welcome_mouse_pos != (mouse.x, mouse.y) {
            welcome_mouse_pos = (mouse.x, mouse.y);
            welcome_dirty = true;
        }
        let clicked = mouse.left_button && !welcome_was_left_down;
        welcome_was_left_down = mouse.left_button;
        if !clicked { continue; }

        let mx = mouse.x.max(0) as usize;
        let my = mouse.y.max(0) as usize;
        let point_in = |rect: (usize, usize, usize, usize)| {
            mx >= rect.0 && my >= rect.1 && mx < rect.0 + rect.2 && my < rect.1 + rect.3
        };
        let mut selection_changed = false;
        for i in 0..2 {
            if point_in(gui::welcome_language_rect(width, height, i)) {
                settings_app.language = if i == 0 { apps::settings::UiLanguage::Turkish } else { apps::settings::UiLanguage::English };
                gui::set_ui_language_english(settings_app.language.is_english());
                selection_changed = true;
                break;
            }
        }
        if !selection_changed {
            for (i, id) in gui::theme::ThemeId::ALL.iter().enumerate() {
                if point_in(gui::welcome_theme_rect(width, height, i)) {
                    settings_app.theme = *id;
                    gui::theme::set_theme(*id);
                    selection_changed = true;
                    break;
                }
            }
        }
        if selection_changed {
            welcome_dirty = true;
        } else if point_in(gui::welcome_continue_rect(width, height)) {
            settings_app.save_all(&compositor, &mut fs_mgr.vfs);
            break;
        }
    }

    // RTC - gerçek zamanlı saat
    let mut now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
    compositor.cal_year = now.year;
    compositor.cal_month = now.month;
    serial_print("[minOs] RTC clock online.\n");

    // Etkileşim durumu
    let mut last_mouse_x = mouse.x;
    let mut last_mouse_y = mouse.y;
    let mut last_buttons = (mouse.left_button, mouse.right_button);
    let mut dragging_win: Option<usize> = None;
    let mut resizing_win: Option<usize> = None;
    let mut resize_edge: Option<gui::ResizeEdge> = None;
    // Kenar yeniden boyutlandırma başlangıcında pencere geometrisi (sol/üst kenarlar için)
    let mut resize_orig: (usize, usize, usize, usize) = (0, 0, 0, 0);
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    // Kaydırma çubuğu sürükleme: (idx, thumb_üst_ofset, thumb_h, track_h, content_h, view_h)
    let mut vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)> = None;
    let mut anim_tick: usize = 0;
    let mut last_frame_ms = monotonic_ms();
    let mut loop_counter: u64 = 0;
    let mut last_clock_update_ms = monotonic_ms();
    let mut last_music_beat: usize = 0;
    let mut cpu_sample_start = read_cpu_cycles();
    let mut cpu_sample_total_cycles = 0u64;
    let mut cpu_sample_active_cycles = 0u64;
    let mut music_note: usize = 0;
    let mut speaker_on = false;
    let mut last_title_idx: Option<usize> = None;
    let mut last_title_click_ms: u64 = 0;
        // Not Defteri metin seçimi sürükleme durumu.
    let mut notepad_dragging = false;


    let mut app_render_cycles = [0u64; gui::MAX_WINDOWS];
    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);
    let render_full_frame = |fb: &mut drivers::video::Framebuffer,
                             compositor: &gui::MinGuiCompositor,
                             fm: &apps::filemanager::FileManagerApp,
                             st: &apps::settings::SystemSettings,
                             np: &apps::notepad::NotepadApp,
                             br: &apps::browser::BrowserApp,
                             iv: &apps::imageviewer::ImageViewerApp,
                             mp: &apps::musicplayer::MusicPlayerApp,
                             vp: &apps::videoplayer::VideoPlayerApp,
                             tm: &apps::taskmgr::TaskManagerApp,
                             calc: &apps::calculator::CalculatorApp,
                             paint: &[Option<apps::paint::PaintApp>],
                             term: &apps::terminal::TerminalApp,
                             now: &drivers::rtc::DateTime,
                             mouse: &drivers::mouse::MouseState,
                             kbd: &drivers::keyboard::KeyboardState,
                             total_ram_bytes: u64,
                             tick: usize,
                             app_render_cycles: &mut [u64; gui::MAX_WINDOWS],
                             dirty_rect: Option<(usize, usize, usize, usize)>| {
        // 1. Masaüstü Arka Planı
        compositor.render_desktop_bg(fb);

        // 2. Açık pencereleri z-sırasına göre (arkadan öne) çiz
        let (order, count) = compositor.draw_order();
        for k in 0..count {
            let idx = order[k];
            if let Some(win) = compositor.windows[idx] {
                if !win.is_open || win.is_minimized {
                    continue;
                }
                compositor.render_window_frame(fb, &win, mouse, idx);

                // İçerik alanını pencere çerçevesine kırp (taşma önleme).
                let (cx, cy, cw, ch) = win.content_rect();
                fb.set_clip(cx, cy, cw, ch);

                // Uygulamaya göre ölçülen içerik boyutu (kaydırma çubuğu gerekli mi?).
                let app_id = compositor.app_id_for_window(idx);
                let content_h = window_content_h(app_id, win.height, win.width, fm, np);
                let content_w = cw;
                let want_v = content_h > ch;
                let want_h = content_w > cw;
                let v_gutter = if want_v { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                let h_gutter = if want_h { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                // İçerik alanı (kaydırma çubukları kadar daraltılmış).
                let content_area = (cx, cy, cw.saturating_sub(v_gutter), ch.saturating_sub(h_gutter));
                fb.set_clip(content_area.0, content_area.1, content_area.2.max(1), content_area.3.max(1));

                let render_start = read_cpu_cycles();
                match app_id {
                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),
                    1 => st.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout, now, total_ram_bytes,
                        cpu_vendor(), cpu_logical_count(),
                        if drivers::hda::is_ready() { gui::ui_text("Intel HDA (PCM aktif)", "Intel HDA (PCM active)") } else { gui::ui_text("Ses aygıtı bulunamadı", "Audio device unavailable") },
                        if drivers::e1000::is_ready() { "Intel e1000" } else { gui::ui_text("Bulunamadı", "Not found") },
                        if drivers::e1000::is_ready() { gui::ui_text("Hazır", "Ready") } else { gui::ui_text("Başlatılamadı / aygıt yok", "Initialization failed / device missing") }),
                    2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick, &win.scroll),
                    3 => br.render_content(fb, win.x, win.y, win.width, win.height),
                    4 => iv.render_content(fb, win.x, win.y, win.width, win.height),
                    5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    7 => tm.render_content(fb, win.x, win.y, win.width),
                    8 => calc.render_content(fb, win.x, win.y, win.width, win.height),
                    9 => if let Some(app) = paint[idx].as_ref() { app.render_content(fb, win.x, win.y, win.width, win.height) },
                    10 => term.render_content(fb, win.x, win.y, win.width, win.height, win.is_focused),
                    _ => {}
                }
                app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);

                // Kırpmayı kaldır ve kaydırma çubuklarını pencerenin içinde çiz.
                fb.reset_clip();
                // Sağ-alt köşedeki resize tutamacıyla çakışmayı önle: kaydırma
                // çubuklarının pencerenin son `RESIZE_GRIP` pikseline girmesini engelle.
                let grip = crate::gui::RESIZE_GRIP;
                let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;
                if want_v {
                    let v_h = content_area.3.saturating_sub(grip);
                    if v_h > 0 {
                        let track = (cx + cw - sb, cy, sb, v_h);
                        let g = crate::gui::scrollbar::v_geom(track, content_h, v_h, win.scroll.y);
                        crate::gui::scrollbar::draw_vertical(fb, &g);
                    }
                }
                if want_h {
                    let h_w = content_area.2.saturating_sub(grip);
                    if h_w > 0 {
                        let track = (cx, cy + ch - sb, h_w, sb);
                        let g = crate::gui::scrollbar::h_geom(track, content_w, h_w, win.scroll.x);
                        crate::gui::scrollbar::draw_horizontal(fb, &g);
                    }
                }
            }
        }
        fb.reset_clip();

        // 3. Görev Çubuğu, sistem tepsisi, popup'lar ve fare imleci
        compositor.render_taskbar_and_overlays(
            fb, mouse, kbd, now, st.volume, st.is_muted, st.language.is_english(),
            drivers::e1000::is_ready(),
        );

        // 4. Backbuffer'ı VRAM'e kopyala (flicker-free; sürüklemede sadece değişen bölge)
        fb.apply_night_light(if st.night_light { st.night_light_level } else { 0 });
        if let Some(r) = dirty_rect {
            fb.flush_rect(r);
        } else {
            fb.flush();
        }
    };

    // İlk kareyi çiz (tüm ekran)
    render_full_frame(
        &mut fb, &compositor, &filemanager_app, &settings_app,
        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app, &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick,
        &mut app_render_cycles,
        None,
    );

    let mut drag_prev_box: Option<(usize, usize, usize, usize)> = None;
    let mut drag_dirty_rect: Option<(usize, usize, usize, usize)> = None;

    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü
    loop {
        let loop_start_cycles = read_cpu_cycles();
        let input_update = drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);
        loop_counter = loop_counter.wrapping_add(1);
        let mx = mouse.x as usize;
        let my = mouse.y as usize;
        let mouse_clicked = mouse.left_button && !last_buttons.0;
        let mouse_released = !mouse.left_button && last_buttons.0;
        let right_clicked = mouse.right_button && !last_buttons.1;
        let l = gui::MinGuiCompositor::tray_layout(width, height);
        let tb_y = l.tb_y;

        // RTC saatini işlem döngüsü sayısına göre değil, geçen gerçek zamana göre yenile.
        let mut force_render = false;
        let clock_now_ms = monotonic_ms();
        if clock_now_ms.wrapping_sub(last_clock_update_ms) >= 1_000 {
            last_clock_update_ms = clock_now_ms;
            now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
            force_render = true;
        }

        // Ses cihazını ayarlarla senkronla
        audio_dev.set_volume(settings_app.volume);
        audio_dev.is_muted = settings_app.is_muted;
        musicplayer_app.volume = settings_app.volume as usize;

        // 1. Fare Sağ Tık — tıklanan yere göre bağlam menüsü
        if right_clicked {
            compositor.is_calendar_open = false;
            compositor.is_volume_popup_open = false;
            compositor.close_start_menu();
            compositor.is_context_menu_open = true;
            compositor.context_menu_x = mx;
            compositor.context_menu_y = my;
            compositor.context_file_idx = None;
            compositor.context_window = None;

            // Tıklanan pencereyi en üstten aşağıya ara
            let (order, count) = compositor.draw_order();
            let mut kind = gui::ContextKind::Desktop;
            for k in (0..count).rev() {
                let idx = order[k];
                if let Some(win) = compositor.windows[idx] {
                    let inside = mx >= win.x && mx < win.x + win.width
                        && my >= win.y && my < win.y + win.height;
                    if !inside {
                        continue;
                    }
                    compositor.context_window = Some(idx);
                    if my < win.y + gui::titlebar_h() {
                        // Başlık çubuğu → pencere menüsü
                        kind = gui::ContextKind::Window;
                    } else if idx == 2 {
                        // Not Defteri metin alanı → düzenleme menüsü
                        kind = gui::ContextKind::Notepad;
                        compositor.focus_window(idx);
                    } else if idx == 0 {
                        // Dosya Yöneticisi → dosya üzerinde mi?
                        let list_x = win.x + 124;
                        if mx >= list_x && mx <= win.x + win.width - 10 {
                            let row_y_base = win.y + 86;
                            for i in 0..filemanager_app.file_count {
                                let item_y = row_y_base + i * 24;
                                if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                    compositor.context_file_idx = Some(i);
                                    filemanager_app.selected_idx = i;
                                    break;
                                }
                            }
                        }
                        kind = gui::ContextKind::FileManager;
                        compositor.focus_window(idx);
                    } else {
                        // Diğer uygulamalar → pencere menüsü
                        kind = gui::ContextKind::Window;
                        compositor.focus_window(idx);
                    }
                    break;
                }
            }
            compositor.context_kind = kind;
        }

        // 2. Fare Sol Tıklama Olayları (Mouse Down)
        if mouse_clicked {
            compositor.finish_alt_tab();
            let mut consumed = false;

            // Açık menü/popup dışına tıklanınca kapat; tıklamayı tüketmeden
            // bırak ki aynı tıklama alttaki pencere veya görev çubuğuna ulaşsın.
            if compositor.is_start_menu_open {
                let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);
                let submenu = gui::MinGuiCompositor::start_menu_submenu_rect(
                    menu_y, compositor.active_start_group, tb_y,
                );
                if !gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h))
                    && !gui::in_rect(mx, my, submenu)
                {
                    compositor.close_start_menu();
                }
            }
            if compositor.is_context_menu_open
                && compositor.context_hit(width, height, mx, my).is_none()
            {
                compositor.is_context_menu_open = false;
            }
            if compositor.is_volume_popup_open
                && !gui::in_rect(mx, my, compositor.volume_slider_rect(&l))
            {
                compositor.is_volume_popup_open = false;
            }
            if compositor.is_calendar_open
                && !gui::in_rect(mx, my, gui::MinGuiCompositor::calendar_rect(width, &l))
            {
                compositor.is_calendar_open = false;
            }
            if taskmgr_app.launch_menu_open {
                let in_menu = compositor.windows[7].map_or(false, |w| {
                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_menu_rect(w.x, w.y, w.width))
                });
                let in_button = compositor.windows[7].map_or(false, |w| {
                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_button_rect(w.x, w.y, w.width))
                });
                if !in_menu && !in_button { taskmgr_app.launch_menu_open = false; }
            }
            // Not Defteri metin alanı: imleci konumlandır, çift tıkta kelime seç,
            // tek tıkta seçimi temizle ve sürükleyerek seçimi başlat.
            if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                if w.is_focused
                    && gui::in_rect(mx, my, (w.x + 6, w.y + 58, w.width.saturating_sub(12), w.height.saturating_sub(86)))
                {
                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);
                    // Çift tık algılama: aynı konuma kısa sürede ikinci tık.
                    let now_ms = monotonic_ms() as usize;
                    let dbl = now_ms.abs_diff(notepad_app.last_click_tick) <= 300
                        && notepad_app.last_click_pos.0.abs_diff(mx) < 4
                        && notepad_app.last_click_pos.1.abs_diff(my) < 4;
                    if dbl {
                        notepad_app.select_word_at(at);
                    } else {
                        notepad_app.cursor = at;
                        notepad_app.selection_start = Some(at);
                        notepad_app.selection_end = Some(at);
                        notepad_dragging = true;
                    }
                    notepad_app.last_click_tick = now_ms;
                    notepad_app.last_click_pos = (mx, my);
                    consumed = true;
                }
            }

            // A) Takvim açıkken
            if compositor.is_calendar_open {
                let crect = gui::MinGuiCompositor::calendar_rect(width, &l);
                if gui::in_rect(mx, my, crect) {
                    let (cx, cy, cw, _ch) = crect;
                    if my < cy + 34 {
                        if mx < cx + 34 {
                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, false);
                            compositor.cal_year = y;
                            compositor.cal_month = m;
                        } else if mx > cx + cw - 34 {
                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, true);
                            compositor.cal_year = y;
                            compositor.cal_month = m;
                        }
                    }
                    consumed = true;
                } else {
                    compositor.is_calendar_open = false;
                }
            }

            // B) Ses paneli açıkken
            if !consumed && compositor.is_volume_popup_open {
                let panel = compositor.volume_slider_rect(&l);
                if gui::in_rect(mx, my, panel) {
                    if gui::in_rect(mx, my, compositor.volume_mute_rect(&l)) {
                        settings_app.is_muted = !settings_app.is_muted;
                    } else {
                        let track = compositor.volume_track_rect(&l);
                        if gui::in_rect(mx, my, track) { settings_app.set_volume_from_range(mx, track.0, track.2); }
                    }
                    consumed = true;
                } else {
                    compositor.is_volume_popup_open = false;
                }
            }

            // C) Bağlam menüsü açıkken
            if !consumed && compositor.is_context_menu_open {
                if let Some(clicked_item) = compositor.context_hit(width, height, mx, my) {
                    match compositor.context_kind {
                        gui::ContextKind::Desktop => match clicked_item {
                            0 => open_app(&mut compositor, 0),
                            1 => open_app(&mut compositor, 2),
                            2 => open_app(&mut compositor, 1),
                            3 => open_app(&mut compositor, 7),
                            4 => settings_app.cycle_theme(),
                            _ => {}
                        },
                        gui::ContextKind::FileManager => {
                            match clicked_item {
                                0 => { // Aç / Klasöre Gir
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                            if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                                    notepad_app.open_file(&path, data);
                                                }
                                                open_app(&mut compositor, 2);
                                            } else if tag == "IMG" {
                                                open_app(&mut compositor, 4);
                                            } else if tag == "SND" {
                                                open_app(&mut compositor, 5);
                                            } else if tag == "VID" {
                                                open_app(&mut compositor, 6);
                                            } else if tag == "WEB" {
                                                open_app(&mut compositor, 3);
                                            }
                                        }
                                    }
                                }
                                1 => { // Not Defterinde Düzenle
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        let path = filemanager_app.selected_full_path();
                                        if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                            notepad_app.open_file(&path, data);
                                        }
                                        open_app(&mut compositor, 2);
                                    }
                                }
                                2 => { // Yeni Dosya Oluştur
                                    filemanager_app.new_file(&mut fs_mgr.vfs);
                                }
                                3 => { // Yeni Klasör Oluştur
                                    filemanager_app.new_folder(&mut fs_mgr.vfs);
                                }
                                5 => { // Dosyayı / Klasörü Sil
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        filemanager_app.delete_selected(&mut fs_mgr.vfs);
                                    }
                                }
                                4 => { // Adını değiştir
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        filemanager_app.begin_rename();
                                    }
                                }
                                _ => {}
                            }
                        }
                        gui::ContextKind::Notepad => match clicked_item {
                            0 => notepad_app.cut_selection(),
                            1 => notepad_app.copy_selection(),
                            2 => notepad_app.paste_clipboard(),
                            3 => notepad_app.delete_selection(),
                            4 => notepad_app.select_all(),
                            _ => notepad_app.clear(),
                        },
                        gui::ContextKind::Window => {
                            if let Some(wi) = compositor.context_window {
                                match clicked_item {
                                    0 => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.is_minimized = true;
                                        }
                                        compositor.focus_top_window();
                                    }
                                    1 => {
                                        compositor.focus_window(wi);
                                        compositor.toggle_maximize(wi, width, height);
                                    }
                                    2 => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.x = width.saturating_sub(w.width) / 2;
                                            w.y = height
                                                .saturating_sub(gui::taskbar_h())
                                                .saturating_sub(w.height)
                                                / 2;
                                        }
                                    }
                                    _ => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.is_open = false;
                                        }
                                        compositor.focus_top_window();
                                    }
                                }
                            }
                        }
                    }
                }
                compositor.is_context_menu_open = false;
                consumed = true;
            }

        if compositor.is_start_menu_open {
            let (_, menu_y, _, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);
            for g in 0..4 {
                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);
                if gui::in_rect(mx, my, cat_r) {
                    compositor.active_start_group = g;
                    break;
                }
            }
        }

        // D) Başlat menüsü açıkken
        if !consumed && compositor.is_start_menu_open {
            let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);
            let search_rect = gui::MinGuiCompositor::start_search_rect(menu_x, menu_y, menu_w);
            let active_g = compositor.active_start_group;
            let mut sub_rect = gui::MinGuiCompositor::start_menu_submenu_rect(menu_y, active_g, tb_y);
            if compositor.start_search_len > 0 {
                let mut matches = [0; 11];
                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);
                sub_rect = gui::MinGuiCompositor::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));
            }
            if compositor.start_search_len > 0 && gui::in_rect(mx, my, sub_rect) {
                let mut matches = [0; 11];
                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);
                for (row, app_id) in matches[..count].iter().enumerate() {
                    let row_rect = (sub_rect.0 + 6, sub_rect.1 + gui::sp(30) + row * gui::sp(24), sub_rect.2 - 12, gui::sp(22));
                    if gui::in_rect(mx, my, row_rect) {
                        open_app(&mut compositor, *app_id);
                        compositor.close_start_menu();
                        break;
                    }
                }
                consumed = true;
            } else if compositor.start_search_len > 0 {
                let menu_inside = gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h));
                if menu_inside && !gui::in_rect(mx, my, search_rect) {
                    consumed = true;
                } else if !menu_inside {
                    compositor.close_start_menu();
                    consumed = true;
                }
            }
            if gui::in_rect(mx, my, search_rect) {
                compositor.clear_start_search();
                consumed = true;
            }

            let mut cat_clicked = consumed;
            if gui::in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22)) {
                open_app(&mut compositor, 1);
                settings_app.active_tab = 0;
                compositor.close_start_menu();
                consumed = true;
                cat_clicked = true;
            }
            for g in 0..4 {
                if cat_clicked { break; }
                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);
                if gui::in_rect(mx, my, cat_r) {
                    compositor.active_start_group = g;
                    cat_clicked = true;
                    consumed = true;
                    break;
                }
            }

            if !cat_clicked {
                let (_, first, count) = gui::MENU_GROUPS[active_g];
                let mut item_clicked = false;
                for k in 0..count {
                    let i = first + k;
                    let rect = gui::MinGuiCompositor::start_menu_subitem_rect(menu_y, active_g, k, tb_y);
                    if gui::in_rect(mx, my, rect) {
                        item_clicked = true;
                        if i == 11 {
                            if compositor.confirm_shutdown {
                                serial_print("[minOs] System Shutdown confirmed via Start Menu!\n");
                                unsafe {
                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                                    core::arch::asm!("out dx, ax", in("dx") 0x404u16, in("ax") 0x3400u16);
                                    core::arch::asm!("cli; hlt");
                                }
                            } else {
                                compositor.confirm_shutdown = true;
                                compositor.confirm_restart = false;
                            }
                            break;
                        }
                        if i == 12 {
                            if compositor.confirm_restart {
                                restart_system();
                            }
                            compositor.confirm_restart = true;
                            compositor.confirm_shutdown = false;
                            break;
                        }
                        compositor.confirm_shutdown = false;
                        compositor.confirm_restart = false;
                        if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {
                            open_app(&mut compositor, app_idx);
                        }
                        compositor.close_start_menu();
                        break;
                    }
                }

                if !item_clicked {
                    if gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) || gui::in_rect(mx, my, sub_rect) {
                        consumed = true;
                    } else {
                        compositor.close_start_menu();
                        consumed = true;
                    }
                } else {
                    consumed = true;
                }
            }
        }

        // E) Sistem tepsisi düğmeleri
        if !consumed {
            if gui::in_rect(mx, my, l.start) {
                compositor.is_start_menu_open = !compositor.is_start_menu_open;
                if compositor.is_start_menu_open {
                    compositor.active_start_group = 0;
                    compositor.clear_start_search();
                } else {
                    compositor.close_start_menu();
                }
                consumed = true;
                } else if gui::in_rect(mx, my, l.clock) {
                    compositor.is_calendar_open = !compositor.is_calendar_open;
                    if compositor.is_calendar_open {
                        compositor.cal_year = now.year;
                        compositor.cal_month = now.month;
                    }
                    consumed = true;
                } else if gui::in_rect(mx, my, l.volume) {
                    compositor.is_volume_popup_open = !compositor.is_volume_popup_open;
                    consumed = true;
                } else if gui::in_rect(mx, my, l.network) {
                    // Ethernet simgesi ağ ayarları sekmesini açar.
                    open_app(&mut compositor, 1);
                    settings_app.active_tab = 2;
                    settings_app.status_msg = gui::ui_text("Ağ ayarları açıldı.", "Network settings opened.");
                    consumed = true;
                } else if gui::in_rect(mx, my, l.theme) {
                    settings_app.cycle_theme();
                    consumed = true;
                } else if gui::in_rect(mx, my, l.kbd) {
                    keyboard.toggle_layout();
                    settings_app.keyboard_layout = keyboard.layout;
                    consumed = true;
                }
            }

            // F) Görev çubuğu uygulama sekmeleri (font ölçeğine duyarlı)
            let tab_h = gui::MinGuiCompositor::taskbar_tab_rect_at(0, tb_y).3;
            if !consumed && my >= tb_y + 5 && my < tb_y + 5 + tab_h {
                let open_count = compositor.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open)).count();
                let (tab_start, tab_width, tab_step, shown_tabs, tab_slots) =
                    gui::MinGuiCompositor::taskbar_tabs_layout(width, height, open_count);
                let mut tab_slot = 0usize;
                for idx in 0..gui::MAX_WINDOWS {
                    if let Some(win) = compositor.windows[idx] {
                        if win.is_open {
                            if tab_slot >= shown_tabs { break; }
                            let tab_x = tab_start + tab_slot * tab_step;
                            if mx >= tab_x && mx < tab_x + tab_width {
                                if win.is_minimized {
                                    focus_app_window(&mut compositor, idx);
                                } else if win.is_focused {
                                    if let Some(w) = compositor.windows[idx].as_mut() {
                                        w.is_minimized = true;
                                    }
                                    compositor.focus_top_window();
                                } else {
                                    compositor.focus_window(idx);
                                }
                                consumed = true;
                                break;
                            }
                            tab_slot += 1;
                        }
                    }
                }
                if !consumed && tab_slots > shown_tabs {
                    let badge_x = tab_start + shown_tabs * tab_step;
                    let badge_w = tab_width.min(gui::MinGuiCompositor::tray_layout(width, height).kbd.0
                        .saturating_sub(gui::sp(4)).saturating_sub(badge_x));
                    if mx >= badge_x && mx < badge_x + badge_w {
                        // +N alanı gizli sekmeler arasında sırayla dolaşır.
                        let mut ordinal = 0usize;
                        let mut first_hidden = None;
                        let mut focused_hidden_ordinal = None;
                        for hidden_idx in 0..gui::MAX_WINDOWS {
                            if let Some(hidden) = compositor.windows[hidden_idx] {
                                if hidden.is_open {
                                    if ordinal >= shown_tabs {
                                        if first_hidden.is_none() { first_hidden = Some(hidden_idx); }
                                        if hidden.is_focused { focused_hidden_ordinal = Some(ordinal); }
                                    }
                                    ordinal += 1;
                                }
                            }
                        }
                        let mut next_hidden = None;
                        if let Some(active_ordinal) = focused_hidden_ordinal {
                            let mut ordinal = 0usize;
                            for hidden_idx in 0..gui::MAX_WINDOWS {
                                if let Some(hidden) = compositor.windows[hidden_idx] {
                                    if hidden.is_open {
                                        if ordinal > active_ordinal { next_hidden = Some(hidden_idx); break; }
                                        ordinal += 1;
                                    }
                                }
                            }
                        }
                        if let Some(target) = next_hidden.or(first_hidden) {
                            focus_app_window(&mut compositor, target);
                        }
                        consumed = true;
                    }
                }
            }

            // G) Masaüstü simgeleri
            if !consumed && !compositor.is_point_covered_by_window(mx, my) && mx >= 20 && mx <= 220 {
                let desk_icons = [0, 2, 3, 5, 1];
                for (i, &app_idx) in desk_icons.iter().enumerate() {
                    let dy = 30 + i * 54;
                    if my >= dy && my <= dy + 30 {
                        open_app(&mut compositor, app_idx);
                        compositor.close_start_menu();
                        consumed = true;
                        break;
                    }
                }
            }

            // H) Pencereler (başlık, butonlar, içerik)
            if !consumed {
                let (order, count) = compositor.draw_order();
                for k in (0..count).rev() {
                    let idx = order[k];
                    let win = match compositor.windows[idx] {
                        Some(w) => w,
                        None => continue,
                    };
                    let (wx, wy, ww, wh) = (win.x, win.y, win.width, win.height);
                    // Standart pencere davranışı: üstteki pencere gövdesine tıklama odağı alır.
                    if gui::in_rect(mx, my, (wx, wy, ww, wh)) { compositor.focus_window(idx); }

                    // Kapat [X]
                    if gui::in_rect(mx, my, win.close_rect()) {
                        terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,
                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],
                            &mut terminal_app);
                        break;
                    }
                    // Maksimize / Geri Yükle
                    if gui::in_rect(mx, my, win.max_rect()) {
                        compositor.focus_window(idx);
                        compositor.toggle_maximize(idx, width, height);
                        break;
                    }
                    // Simge durumuna küçült [-]
                    if gui::in_rect(mx, my, win.min_rect()) {
                        if let Some(w) = compositor.windows[idx].as_mut() {
                            w.is_minimized = true;
                        }
                        compositor.focus_top_window();
                        break;
                    }
                    // Pencerenin kenar ve köşelerinden boyutlandırma.
                    if !win.is_maximized {
                        if let Some(edge) = win.resize_edge_at(mx, my) {
                            // Sağ kenardaki scrollbar için kendi tutma alanını bırak.
                            if edge != gui::ResizeEdge::Right || mx >= wx + ww - 6 {
                                compositor.focus_window(idx);
                                resizing_win = Some(idx);
                                resize_edge = Some(edge);
                                resize_orig = (wx, wy, ww, wh);
                                break;
                            }
                        }
                    }
                    // Dikey kaydırma çubuğu (pencere sağ kenarı) — tıklama/sürükleme.
                    {
                        let (ccx, ccy, ccw, cch) = win.content_rect();
                        let c_h = window_content_h(win.app_id as usize, wh, ww, &filemanager_app, &notepad_app);
                        let grip = gui::RESIZE_GRIP;
                        let view_h = cch.saturating_sub(grip);
                        if c_h > view_h {
                            let t = gui::scrollbar::SCROLLBAR_THICKNESS;
                            let track = (ccx + ccw - t, ccy, t, view_h);
                            if gui::in_rect(mx, my, track) {
                                let g = gui::scrollbar::v_geom(track, c_h, view_h, win.scroll.y);
                                // Tutamaç yakınındaysa tutamacın üstünü koru, değilse anında zıpla.
                                if gui::in_rect(mx, my, g.thumb) {
                                    vscroll_drag = Some((idx, g.thumb.1.saturating_sub(my), g.thumb.3, track.3, c_h, view_h));
                                } else {
                                    // Track üzerinde tıklanan konumdan kaydırmayı hesapla.
                                    let ratio = (my.saturating_sub(track.1)) as f32 / track.3.max(1) as f32;
                                    let max_off = c_h.saturating_sub(view_h);
                                    if let Some(wm) = compositor.windows[idx].as_mut() {
                                        wm.scroll.y = ((ratio * max_off as f32) as usize).min(max_off);
                                    }
                                    vscroll_drag = Some((idx, g.thumb.3 / 2, g.thumb.3, track.3, c_h, view_h));
                                }
                                compositor.focus_window(idx);
                                break;
                            }
                        }
                    }
                    // Başlık çubuğu — sürükle / çift tık ile maksimize
                    if gui::in_rect(mx, my, win.title_drag_rect()) {
                        let now_ms = monotonic_ms();
                        let dbl = last_title_idx == Some(idx)
                            && now_ms.wrapping_sub(last_title_click_ms) <= 300;
                        compositor.focus_window(idx);
                        if dbl {
                            compositor.toggle_maximize(idx, width, height);
                            dragging_win = None;
                            // Çift tık tüketildikten sonra eşleşmeyi temizle;
                            // üçüncü kısa tık yeni bir çift tık başlatabilsin.
                            last_title_idx = None;
                        } else {
                            last_title_idx = Some(idx);
                            last_title_click_ms = now_ms;
                            if !win.is_maximized {
                                dragging_win = Some(idx);
                                drag_offset_x = mx.saturating_sub(wx);
                                drag_offset_y = my.saturating_sub(wy);
                            }
                        }
                        break;
                    }

                    // Pencere gövdesi içindeki uygulama düğmeleri
                    if mx >= wx && mx < wx + ww && my >= wy && my < wy + wh {
                        let app_id = compositor.app_id_for_window(idx);
                        match app_id {
                            0 => {
                                // Dosya Yöneticisi: [Yenile], [+Dosya], [+Klasör], [Sil], [Aç], gezinti, liste
                                if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::refresh_button_rect(wx, wy, ww)) {
                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::new_file_button_rect(wx, wy, ww)) {
                                    filemanager_app.new_file(&mut fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::new_folder_button_rect(wx, wy, ww)) {
                                    filemanager_app.new_folder(&mut fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::delete_button_rect(wx, wy, ww)) {
                                    filemanager_app.delete_selected(&mut fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::open_button_rect(wx, wy, ww)) {
                                    // Geri Dönüşüm Kutusu'ndayken "Aç" düğmesi öğeyi geri yükler.
                                    if filemanager_app.path() == crate::fs::vfs::TRASH_DIR {
                                        filemanager_app.restore_selected(&mut fs_mgr.vfs);
                                    } else if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                        if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                            if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                                notepad_app.open_file(&path, data);
                                            }
                                            open_app(&mut compositor, 2);
                                        } else if tag == "IMG" {
                                            open_app(&mut compositor, 4);
                                        } else if tag == "SND" {
                                            open_app(&mut compositor, 5);
                                        } else if tag == "VID" {
                                            open_app(&mut compositor, 6);
                                        } else if tag == "WEB" {
                                            open_app(&mut compositor, 3);
                                        }
                                    }
                                } else if mx >= wx + 6 && mx < wx + 116 && my >= wy + 66 && my < wy + 66 + 9 * 22 {
                                    let nav = (my.saturating_sub(wy + 66)) / 22;
                                    let target_path = match nav {
                                        0 => "/",
                                        1 => "/root/Documents",
                                        2 => "/root/Media",
                                        3 => "/root/Downloads",
                                        4 => "/boot",
                                        5 => "/media/usb0",
                                        6 => "/media/hdd0",
                                        7 => "/media/cdrom",
                                        _ => "/root/Trash",
                                    };
                                    filemanager_app.set_path(target_path);
                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 {
                                    for i in 0..filemanager_app.file_count {
                                        if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::row_rect(wx, wy, ww, i)) {
                                            filemanager_app.selected_idx = i;
                                            if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                                if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                    if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                                        notepad_app.open_file(&path, data);
                                                    }
                                                    open_app(&mut compositor, 2);
                                                } else if tag == "IMG" {
                                                    open_app(&mut compositor, 4);
                                                } else if tag == "SND" {
                                                    open_app(&mut compositor, 5);
                                                } else if tag == "VID" {
                                                    open_app(&mut compositor, 6);
                                                } else if tag == "WEB" {
                                                    open_app(&mut compositor, 3);
                                                }
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                            1 => {
                                // Sistem Ayarları
                                for t in 0..4 {
                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::tab_rect(wx, wy, t)) {
                                        settings_app.active_tab = t;
                                    }
                                }
                                if gui::in_rect(mx, my, apps::settings::SystemSettings::save_btn_rect(wx, wy, wh)) {
                                    settings_app.save_all(&compositor, &mut fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh)) {
                                    settings_app.reset_defaults();
                                    settings_app.apply_runtime(&mut keyboard, &mut compositor, &mut now);
                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, (wx + 115, wy + 178, 130, 22)) {
                                    settings_app.dhcp_enabled = !settings_app.dhcp_enabled;
                                    settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };
                                } else if settings_app.active_tab == 0 {
                                    let mut swatch_hit = false;
                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count() {
                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_swatch_rect(wx, wy, i)) {
                                            settings_app.theme = gui::theme::ThemeId::ALL[i];
                                            gui::theme::set_theme(settings_app.theme);
                                            swatch_hit = true;
                                            break;
                                        }
                                    }
                                    if swatch_hit {
                                        // Tema paletinden dogrudan secim yapildi.
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_btn_rect(wx, wy, ww)) {
                                        settings_app.cycle_theme();
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::kbd_btn_rect(wx, wy, ww)) {
                                        keyboard.toggle_layout();
                                        settings_app.keyboard_layout = keyboard.layout;
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::language_btn_rect(wx, wy, ww)) {
                                        settings_app.language.toggle();
                                        settings_app.status_msg = gui::ui_text("Arayüz dili Türkçe olarak ayarlandı.", "Interface language set to English.");
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::mute_btn_rect(wx, wy)) {
                                        settings_app.is_muted = !settings_app.is_muted;
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::volume_track_rect(wx, wy)) {
                                        let track=apps::settings::SystemSettings::volume_track_rect(wx,wy); settings_app.set_volume_from_range(mx,track.0,track.2);
                                    }
                                }
                                if settings_app.active_tab == 1 {
                                    for i in 0..3 {
                                        if gui::in_rect(mx,my,apps::settings::SystemSettings::zoom_btn_rect(wx,wy,i)) {
                                            settings_app.zoom = [100,125,150][i];
                                            gui::set_font_zoom(settings_app.zoom);
                                            settings_app.status_msg = gui::ui_text("Yazı yakınlaştırması değiştirildi.", "Font zoom changed.");
                                        }
                                    }
                                    for i in 0..4 {
                                        if gui::in_rect(mx,my,apps::settings::SystemSettings::resolution_btn_rect(wx,wy,i)) {
                                            let requested = match i { 0 => (800,600), 1 => (1024,768), 2 => (1280,720), _ => (1920,1080) };
                                            if requested == (fb.width, fb.height) {
                                                settings_app.screen_res = requested;
                                                settings_app.display_res = requested;
                                                settings_app.status_msg = gui::ui_text("Bu çözünürlük zaten etkin.", "This resolution is already active.");
                                            } else {
                                                settings_app.screen_res = (fb.width, fb.height);
                                                settings_app.status_msg = gui::ui_text("Çözünürlük açılışta Limine tarafından belirlenir.", "Resolution is set by Limine at boot.");
                                            }
                                        }
                                    }
                                    if gui::in_rect(mx,my,apps::settings::SystemSettings::night_light_rect(wx,wy)) {
                                        settings_app.night_light = !settings_app.night_light;
                                        settings_app.status_msg = if settings_app.night_light {
                                            gui::ui_text("Mavi ışık filtresi açıldı.", "Blue light filter enabled.")
                                        } else { gui::ui_text("Mavi ışık filtresi kapatıldı.", "Blue light filter disabled.") };
                                    }
                                    let nl_track = apps::settings::SystemSettings::night_light_track_rect(wx,wy);
                                    if gui::in_rect(mx,my,nl_track) {
                                        let span=nl_track.2.saturating_sub(1).max(1);
                                        settings_app.night_light_level=((mx-nl_track.0).min(span)*100/span).min(100) as u8;
                                        settings_app.night_light=true;
                                    }
                                    if gui::in_rect(mx,my,apps::settings::SystemSettings::region_btn_rect(wx,wy)) {
                                        settings_app.region=(settings_app.region+1)%3;
                                        now=drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
                                        compositor.cal_year=now.year;
                                        compositor.cal_month=now.month;
                                        settings_app.status_msg=gui::ui_text("Bölge ve saat dilimi değiştirildi.", "Region and time zone updated.");
                                    }
                                }
                            }
                            2 => {
                                // Not Defteri: Dosya/Düzen/Yardım araç çubuğu ve menü aksiyonları
                                if my >= wy + 30 && my <= wy + 56 && mx >= wx + 8 && mx < wx + 60 { notepad_app.menu = 1; }
                                else if my >= wy + 30 && my <= wy + 56 && mx >= wx + 68 && mx < wx + 128 { notepad_app.menu = 2; }
                                else if my >= wy + 30 && my <= wy + 56 && mx >= wx + 128 && mx < wx + 190 { notepad_app.menu = 3; }
                                else if notepad_app.menu != 0 && mx >= wx + 8 && mx < wx + 340 && my >= wy + 54 && my < wy + 170 {
                                    let item = (my.saturating_sub(wy + 54)) / 20;
                                    match (notepad_app.menu, item) {
                                        // Dosya Menüsü: Yeni, Aç, Kaydet, Temizle
                                        (1, 0) => notepad_app.clear(),
                                        (1, 1) => {
                                            if let Some(data) = fs_mgr.vfs.read_file("/root/Documents/notlar.txt") {
                                                notepad_app.open_file("/root/Documents/notlar.txt", data);
                                            }
                                        },
                                        (1, 2) => { notepad_app.save_file(&mut fs_mgr.vfs); },
                                        (1, 3) => notepad_app.clear(),
                                        // Düzen Menüsü: 0: Kes, 1: Kopyala, 2: Yapıştır, 3: Tümünü Seç
                                        (2, 0) => notepad_app.cut_selection(),
                                        (2, 1) => notepad_app.copy_selection(),
                                        (2, 2) => notepad_app.paste_clipboard(),
                                        (2, 3) => notepad_app.select_all(),
                                        // Yardım Menüsü: Hakkında sekmesini aç
                                        (3, 0) => {
                                            open_app(&mut compositor, 1);
                                            settings_app.active_tab = 3;
                                        },
                                        _ => {}
                                    }
                                    notepad_app.menu = 0;
                                }
                                if mx >= wx + ww - 130 && mx <= wx + ww - 74 && my >= wy + 32 && my <= wy + 52 {
                                    notepad_app.clear();
                                } else if mx >= wx + ww - 68 && mx <= wx + ww - 12 && my >= wy + 32 && my <= wy + 52 {
                                    notepad_app.save_file(&mut fs_mgr.vfs);
                                }
                                if gui::in_rect(mx, my, (wx + 6, wy + 58, ww.saturating_sub(12), wh.saturating_sub(90))) {
                                    let at = notepad_app.index_at_scrolled(mx - wx, my - wy, ww, compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0));
                                    notepad_app.cursor = at;
                                    notepad_app.selection_start = Some(at);
                                    notepad_app.selection_end = Some(at);
                                    notepad_app.menu = 0;
                                }
                            }
                            3 => {
                                if gui::in_rect(mx, my, (wx + 8, wy + 30, 140, 20)) {
                                    browser_app.active_tab = 0;
                                    browser_app.set_url(b"minos://help");
                                    browser_app.navigate();
                                } else if gui::in_rect(mx, my, (wx + 152, wy + 30, 85, 20)) {
                                    browser_app.active_tab = 1;
                                    browser_app.set_url(b"minos://cats");
                                    browser_app.navigate();
                                } else if gui::in_rect(mx, my, (wx + 241, wy + 30, 100, 20)) {
                                    browser_app.active_tab = 3;
                                    browser_app.page_loaded = true;
                                } else if gui::in_rect(mx, my, (wx + 12, wy + 56, 22, 20)) {
                                    browser_app.back();
                                } else if gui::in_rect(mx, my, (wx + 38, wy + 56, 22, 20)) {
                                    browser_app.forward();
                                } else if gui::in_rect(mx, my, (wx + 64, wy + 56, 22, 20)) {
                                    browser_app.navigate();
                                } else if gui::in_rect(mx, my, (wx + 94, wy + 56, ww.saturating_sub(160), 20)) {
                                    browser_app.focus_address_at(mx, wx + 100);
                                } else if gui::in_rect(mx, my, (wx + ww - 60, wy + 56, 50, 20)) {
                                    browser_app.navigate();
                                } else {
                                    browser_app.address_focused = false;
                                }
                            }
                            4 => {
                                // Resim: yakınlaştır / uzaklaştır
                                if my >= wy + 32 && my <= wy + 52 {
                                    if mx >= wx + ww - 180 && mx <= wx + ww - 130 {
                                        imageviewer_app.zoom_in();
                                    } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {
                                        imageviewer_app.zoom_out();
                                    }
                                }
                            }
                            5 => {
                                // Müzik çalar: önceki / oynat / sonraki
                                let ctrl_y = wy + 180;
                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 10 && mx <= wx + 46 {
                                        musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);
                                        musicplayer_app.progress = 0;
                                    } else if mx >= wx + 52 && mx <= wx + 98 {
                                        musicplayer_app.is_playing = !musicplayer_app.is_playing;
                                    } else if mx >= wx + 104 && mx <= wx + 140 {
                                        musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;
                                        musicplayer_app.progress = 0;
                                    } else if mx >= wx + 146 && mx < wx + 230 {
                                        let player_volume = (((mx - (wx + 146)) * 100) / 84).min(100) as u8;
                                        settings_app.volume = player_volume;
                                        musicplayer_app.volume = player_volume as usize;
                                    }
                                } else if mx >= wx + 250 && mx < wx + ww - 8 && my >= wy + 96 && my < wy + 194 {
                                    musicplayer_app.current_track = ((my - (wy + 96)) / 18).min(4);
                                    musicplayer_app.progress = 0;
                                    musicplayer_app.is_playing = true;
                                } else if my >= wy + 156 && my <= wy + 170 && mx >= wx + 54 && mx <= wx + 190 {
                                    musicplayer_app.progress = (((mx - (wx + 54)) * 100) / 136).min(100);
                                }
                            }
                            6 => {
                                // Video: oynat / durdur
                                let ctrl_y = wy + 178;
                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 12 && mx <= wx + 54 {
                                        videoplayer_app.is_playing = !videoplayer_app.is_playing;
                                    } else if mx >= wx + 58 && mx <= wx + 98 {
                                        videoplayer_app.is_playing = false;
                                        videoplayer_app.frame = 0;
                                    }
                                }
                            }
                            7 => {
                                if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::applications_tab_rect(wx, wy)) {
                                    taskmgr_app.active_tab = 0;
                                    taskmgr_app.focused_control = 0;
                                    taskmgr_app.launch_menu_open = false;
                                } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::performance_tab_rect(wx, wy)) {
                                    taskmgr_app.active_tab = 1;
                                    taskmgr_app.focused_control = 1;
                                    taskmgr_app.launch_menu_open = false;
                                } else if taskmgr_app.launch_menu_open {
                                    if let Some(item) = apps::taskmgr::TaskManagerApp::launch_item_at(mx, my, wx, wy, ww) {
                                        taskmgr_app.launch_selected = item;
                                        taskmgr_app.focused_control = 2;
                                        let target = taskmgr_app.launch_app_id();
                                        taskmgr_app.launch_menu_open = false;
                                        open_app(&mut compositor, target);
                                    } else {
                                        taskmgr_app.launch_menu_open = false;
                                    }
                                } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_button_rect(wx, wy, ww)) {
                                    taskmgr_app.focused_control = 2;
                                    taskmgr_app.launch_menu_open = true;
                                    taskmgr_app.launch_selected = 0;
                                } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::kill_btn_rect(wx, wy, ww)) {
                                    taskmgr_app.focused_control = 3;
                                    if let Some(target) = taskmgr_app.selected_app_idx() {
                                        taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                                        terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],
                                            &mut terminal_app);
                                    }
                                } else if taskmgr_app.active_tab == 0 {
                                    if let Some(i) = taskmgr_app.row_at(mx, my, wx, wy, ww) {
                                        taskmgr_app.selected = i;
                                        taskmgr_app.focused_control = 4 + i;
                                    }
                                }
                            }
                            8 => { if let Some(key) = apps::calculator::CalculatorApp::key_at(mx, my, wx, wy, ww) { calculator_app.press(key); } }
                            9 => {
                                let palette_start = if ww.saturating_sub(16) >= 258 { wx + 58 } else { wx + 40 };
                                if my >= wy + 34 && my < wy + 56 {
                                    // Araç çubuğu: renk paleti, silgi ve temizle düğmeleri.
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let paint = paint_apps[idx].as_mut().unwrap();
                                    if mx >= palette_start && mx < palette_start + 16 { paint.color=gui::Color::ACCENT_BLUE; paint.eraser=false; }
                                    else if mx >= palette_start + 22 && mx < palette_start + 38 { paint.color=gui::Color::ACCENT_RED; paint.eraser=false; }
                                    else if mx >= palette_start + 44 && mx < palette_start + 60 { paint.color=gui::Color::ACCENT_GREEN; paint.eraser=false; }
                                    else if mx >= palette_start + 66 && mx < palette_start + 82 { paint.color=gui::Color::BLACK; paint.eraser=false; }
                                    else if mx >= palette_start + 92 && mx < palette_start + 150 { paint.eraser=true; }
                                    else if mx >= wx + ww.saturating_sub(198) && mx < wx + ww.saturating_sub(140) { paint.open_bmp(&fs_mgr.vfs); }
                                    else if mx >= wx + ww.saturating_sub(132) && mx < wx + ww.saturating_sub(74) {
                                        let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);
                                        paint.save_bmp(&mut fs_mgr.vfs, cw, ch);
                                    }
                                    else if mx >= wx + ww.saturating_sub(66) && mx < wx + ww.saturating_sub(8) { paint.clear(); }
                                } else {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let paint = paint_apps[idx].as_mut().unwrap();
                                    if let Some((cxp, cyp)) = paint.to_canvas(ww, wh, mx - wx, my - wy) {
                                    // İlk tıklama noktası (last=None iken tek nokta başlangıcı).
                                    paint.paint(cxp, cyp);
                                    } else {
                                    // Tuval dışına tıklandı: kalem yolunu bitir.
                                    paint.end_stroke();
                                    }
                                }
                            }
                            _ => {}
                        }
                        compositor.focus_window(idx);
                        break;
                    }
                }
            }
        }

        // 3. Klavye kısayolları
        let mut enter_char_consumed = false;
        while let Some(action) = keyboard.take_last_action() {
            match action {
                drivers::keyboard::KeyAction::ToggleStartMenu => {
                    compositor.is_start_menu_open = !compositor.is_start_menu_open;
                    if compositor.is_start_menu_open {
                        compositor.active_start_group = 0;
                        compositor.clear_start_search();
                    } else {
                        compositor.close_start_menu();
                    }
                }
                drivers::keyboard::KeyAction::CloseActiveWindow => {
                    let active_window = compositor.active_window_idx.filter(|&idx| {
                        compositor.windows[idx].map_or(false, |w| w.is_open && !w.is_minimized)
                    });
                    if let Some(idx) = active_window {
                        terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,
                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],
                            &mut terminal_app);
                    } else {
                        // Masaüstündeyken Alt+F4 güç menüsünü açar.
                        compositor.is_start_menu_open = true;
                        compositor.active_start_group = 3;
                        compositor.selected_start_item = 0;
                        compositor.confirm_shutdown = false;
                        compositor.confirm_restart = false;
                        compositor.is_context_menu_open = false;
                        compositor.is_calendar_open = false;
                        compositor.is_volume_popup_open = false;
                    }
                }
                drivers::keyboard::KeyAction::CycleWindowFocus => {
                    compositor.is_alt_tab_open = true;
                    compositor.cycle_focus(keyboard.shift);
                }
                drivers::keyboard::KeyAction::WinShowDesktop => {
                    let show_desktop = compositor.windows.iter().flatten().any(|w| w.is_open && !w.is_minimized);
                    for slot in compositor.windows.iter_mut() {
                        if let Some(w) = slot { if w.is_open { w.is_minimized = show_desktop; w.is_focused = false; } }
                    }
                    if show_desktop { compositor.active_window_idx = None; }
                    else { compositor.focus_top_window(); }
                }
                drivers::keyboard::KeyAction::WinMinimizeAll => {
                    for slot in compositor.windows.iter_mut() {
                        if let Some(w) = slot { if w.is_open { w.is_minimized = true; w.is_focused = false; } }
                    }
                    compositor.active_window_idx = None;
                }
                drivers::keyboard::KeyAction::WinOpenFiles => {
                    open_app(&mut compositor, 0);
                    compositor.close_start_menu();
                }
                drivers::keyboard::KeyAction::RenameSelected => {
                    if app_is_focused(&compositor, 0) { filemanager_app.begin_rename(); }
                }
                drivers::keyboard::KeyAction::VolumeUp => {
                    settings_app.volume = settings_app.volume.saturating_add(5).min(100);
                    settings_app.is_muted = false;
                    settings_app.status_msg = gui::ui_text("Ses artırıldı.", "Volume increased.");
                }
                drivers::keyboard::KeyAction::VolumeDown => {
                    settings_app.volume = settings_app.volume.saturating_sub(5);
                    settings_app.status_msg = gui::ui_text("Ses azaltıldı.", "Volume decreased.");
                }
                drivers::keyboard::KeyAction::VolumeMute => {
                    settings_app.is_muted = !settings_app.is_muted;
                    settings_app.status_msg = if settings_app.is_muted { gui::ui_text("Ses kapatıldı.", "Muted.") } else { gui::ui_text("Ses açıldı.", "Unmuted.") };
                }
                drivers::keyboard::KeyAction::MediaPlayPause => {
                    musicplayer_app.is_playing = !musicplayer_app.is_playing;
                }
                drivers::keyboard::KeyAction::MediaPrevious => {
                    musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);
                    musicplayer_app.progress = 0;
                }
                drivers::keyboard::KeyAction::MediaNext => {
                    musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;
                    musicplayer_app.progress = 0;
                }
                drivers::keyboard::KeyAction::OpenTaskManager => {
                    open_app(&mut compositor, 7);
                    compositor.close_start_menu();
                }
                drivers::keyboard::KeyAction::ArrowUp => {
                    if compositor.is_start_menu_open {
                        if compositor.start_search_len > 0 {
                            compositor.move_start_search_selection(settings_app.language.is_english(), false);
                        } else if compositor.selected_start_item > 0 {
                            compositor.selected_start_item -= 1;
                        }
                    } else if compositor.is_context_menu_open {
                        // Sağ tık / Bağlam menüsü gezintisi
                        if compositor.context_file_idx.unwrap_or(0) > 0 {
                            compositor.context_file_idx = Some(compositor.context_file_idx.unwrap() - 1);
                        }
                    } else {
                        let np_focused = app_is_focused(&compositor, 2);
                        let terminal_focused = app_is_focused(&compositor, 10);
                        let tm_focused = app_is_focused(&compositor, 7);
                        let fm_focused = app_is_focused(&compositor, 0);
                        let set_focused = app_is_focused(&compositor, 1);
                        let br_focused = app_is_focused(&compositor, 3);

                        if terminal_focused {
                            terminal_app.history_up();
                        } else if tm_focused && taskmgr_app.launch_menu_open {
                            taskmgr_app.select_launch_prev();
                        } else if np_focused {
                            notepad_app.move_vertical_ext(true, keyboard.shift);
                        } else if tm_focused && taskmgr_app.active_tab == 0 {
                            taskmgr_app.select_up();
                        } else if fm_focused {
                            filemanager_app.select_prev();
                        } else if set_focused && settings_app.focused_control < 4 {
                            settings_app.select_tab_prev();
                        } else if br_focused && browser_app.focused_control < 3 {
                            browser_app.select_tab_prev();
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowDown => {
                    if compositor.is_start_menu_open {
                        if compositor.start_search_len > 0 {
                            compositor.move_start_search_selection(settings_app.language.is_english(), true);
                        } else {
                            let active_g = compositor.active_start_group;
                            let count = gui::MENU_GROUPS[active_g].2;
                            if compositor.selected_start_item + 1 < count {
                                compositor.selected_start_item += 1;
                            }
                        }
                    } else if compositor.is_context_menu_open {
                        let items_len = compositor.context_items().len();
                        let cur = compositor.context_file_idx.unwrap_or(0);
                        if cur + 1 < items_len {
                            compositor.context_file_idx = Some(cur + 1);
                        }
                    } else {
                        let np_focused = app_is_focused(&compositor, 2);
                        let terminal_focused = app_is_focused(&compositor, 10);
                        let tm_focused = app_is_focused(&compositor, 7);
                        let fm_focused = app_is_focused(&compositor, 0);
                        let set_focused = app_is_focused(&compositor, 1);
                        let br_focused = app_is_focused(&compositor, 3);

                        if terminal_focused {
                            terminal_app.history_down();
                        } else if tm_focused && taskmgr_app.launch_menu_open {
                            taskmgr_app.select_launch_next();
                        } else if np_focused {
                            notepad_app.move_vertical_ext(false, keyboard.shift);
                        } else if tm_focused && taskmgr_app.active_tab == 0 {
                            taskmgr_app.select_down();
                        } else if fm_focused {
                            filemanager_app.select_next();
                        } else if set_focused && settings_app.focused_control < 4 {
                            settings_app.select_tab_next();
                        } else if br_focused && browser_app.focused_control < 3 {
                            browser_app.select_tab_next();
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowLeft => {
                    if compositor.is_start_menu_open {
                        compositor.active_start_group = compositor.active_start_group.saturating_sub(1);
                        compositor.selected_start_item = 0;
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_move_left();
                    } else if app_is_focused(&compositor, 2) {
                        notepad_app.move_left_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 0;
                        taskmgr_app.focused_control = 0;
                    } else if app_is_focused(&compositor, 1) {
                        settings_app.select_tab_prev();
                    } else if app_is_focused(&compositor, 3) {
                        if browser_app.address_focused { browser_app.address_move_left(); }
                        else { browser_app.select_tab_prev(); }
                    }
                }
                drivers::keyboard::KeyAction::ArrowRight => {
                    if compositor.is_start_menu_open {
                        if compositor.active_start_group + 1 < 4 {
                            compositor.active_start_group += 1;
                            compositor.selected_start_item = 0;
                        }
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_move_right();
                    } else if app_is_focused(&compositor, 2) {
                        notepad_app.move_right_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 1;
                        taskmgr_app.focused_control = 1;
                    } else if app_is_focused(&compositor, 1) {
                        settings_app.select_tab_next();
                    } else if app_is_focused(&compositor, 3) {
                        if browser_app.address_focused { browser_app.address_move_right(); }
                        else { browser_app.select_tab_next(); }
                    }
                }
                drivers::keyboard::KeyAction::EnterKey => {
                    if compositor.is_start_menu_open {
                        enter_char_consumed = true;
                        if compositor.start_search_len > 0 {
                            if let Some(app_idx) = compositor.selected_start_search_result(settings_app.language.is_english()) {
                                open_app(&mut compositor, app_idx);
                                compositor.close_start_menu();
                            }
                            continue;
                        }
                        let active_g = compositor.active_start_group;
                        let (_, first, count) = gui::MENU_GROUPS[active_g];
                        let item_k = compositor.selected_start_item.min(count.saturating_sub(1));
                        let i = first + item_k;
                        if i == 11 {
                            if compositor.confirm_shutdown {
                                serial_print("[minOs] System Shutdown confirmed via Keyboard!\n");
                                unsafe {
                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                                    core::arch::asm!("cli; hlt");
                                }
                            } else {
                                compositor.confirm_shutdown = true;
                            }
                        } else if i == 12 {
                            if compositor.confirm_restart {
                                restart_system();
                            }
                            compositor.confirm_restart = true;
                        } else {
                            if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {
                                open_app(&mut compositor, app_idx);
                            }
                            compositor.close_start_menu();
                        }
                    } else if compositor.is_context_menu_open {
                        enter_char_consumed = true;
                        compositor.is_context_menu_open = false;
                    } else if app_is_focused(&compositor, 7) && taskmgr_app.launch_menu_open {
                        enter_char_consumed = true;
                        let target = taskmgr_app.launch_app_id();
                        taskmgr_app.launch_menu_open = false;
                        open_app(&mut compositor, target);
                    } else if app_is_focused(&compositor, 7) {
                        enter_char_consumed = true;
                        match taskmgr_app.focused_control {
                            0 => taskmgr_app.active_tab = 0,
                            1 => taskmgr_app.active_tab = 1,
                            2 => {
                                taskmgr_app.launch_menu_open = true;
                                taskmgr_app.launch_selected = 0;
                            }
                            3 => {
                                if let Some(target) = taskmgr_app.selected_app_idx() {
                                    taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                                    terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                        &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                        &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[target],
                                        &mut terminal_app);
                                }
                            }
                            _ => {}
                        }
                    } else if app_is_focused(&compositor, 0) {
                        enter_char_consumed = true;
                        match filemanager_app.focused_control {
                            0 => filemanager_app.refresh_from_vfs(&fs_mgr.vfs),
                            1 => filemanager_app.new_file(&mut fs_mgr.vfs),
                            2 => filemanager_app.new_folder(&mut fs_mgr.vfs),
                            3 => { filemanager_app.delete_selected(&mut fs_mgr.vfs); },
                            _ => {
                                if filemanager_app.path() == crate::fs::vfs::TRASH_DIR {
                                    filemanager_app.restore_selected(&mut fs_mgr.vfs);
                                } else if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                    if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                        if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                            notepad_app.open_file(&path, data);
                                        }
                                        open_app(&mut compositor, 2);
                                    } else if tag == "IMG" {
                                        open_app(&mut compositor, 4);
                                    } else if tag == "SND" {
                                        open_app(&mut compositor, 5);
                                    } else if tag == "VID" {
                                        open_app(&mut compositor, 6);
                                    } else if tag == "WEB" {
                                        open_app(&mut compositor, 3);
                                    }
                                }
                            }
                        }
                    } else if app_is_focused(&compositor, 1) {
                        enter_char_consumed = true;
                        if settings_app.focused_control < 4 {
                            settings_app.active_tab = settings_app.focused_control;
                        } else {
                            match settings_app.active_tab {
                                0 => match settings_app.focused_control {
                                    4 => settings_app.cycle_theme(),
                                    5 => {
                                        keyboard.toggle_layout();
                                        settings_app.keyboard_layout = keyboard.layout;
                                    }
                                    6 => {
                                        settings_app.language.toggle();
                                        settings_app.status_msg = gui::ui_text("Arayüz dili değiştirildi.", "Interface language changed.");
                                    }
                                    7 => settings_app.is_muted = !settings_app.is_muted,
                                    8 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    9 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut keyboard, &mut compositor, &mut now); }
                                    _ => {}
                                },
                                1 => match settings_app.focused_control {
                                    4..=7 => {
                                        let i = settings_app.focused_control - 4;
                                        settings_app.screen_res = match i { 0 => (800,600), 1 => (1024,768), 2 => (1280,720), _ => (1920,1080) };
                                    }
                                    8..=10 => {
                                        let i = settings_app.focused_control - 8;
                                        settings_app.zoom = [100, 125, 150][i];
                                        gui::set_font_zoom(settings_app.zoom);
                                    }
                                    11 => settings_app.night_light = !settings_app.night_light,
                                    12 => {
                                        settings_app.region = (settings_app.region + 1) % 3;
                                        now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
                                    }
                                    13 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    14 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut keyboard, &mut compositor, &mut now); },
                                    _ => {}
                                },
                                2 => match settings_app.focused_control {
                                    4 => {
                                        settings_app.dhcp_enabled = !settings_app.dhcp_enabled;
                                        settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };
                                    }
                                    5 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    6 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut keyboard, &mut compositor, &mut now); },
                                    _ => {}
                                },
                                _ => match settings_app.focused_control {
                                    4 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    5 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut keyboard, &mut compositor, &mut now); },
                                    _ => {}
                                },
                            }
                        }
                    } else if app_is_focused(&compositor, 3) {
                        enter_char_consumed = true;
                        match browser_app.focused_control {
                            0 => { browser_app.active_tab = 0; browser_app.set_url(b"minos://help"); browser_app.navigate(); }
                            1 => { browser_app.active_tab = 1; browser_app.set_url(b"minos://cats"); browser_app.navigate(); }
                            2 => { browser_app.active_tab = 3; browser_app.page_loaded = true; }
                            3 => browser_app.back(),
                            4 => browser_app.forward(),
                            5 | 7 => browser_app.navigate(),
                            6 => browser_app.navigate(),
                            _ => {}
                        }
                    } else if app_is_focused(&compositor, 8) {
                        enter_char_consumed = true;
                        calculator_app.press('=');
                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        enter_char_consumed = true;
                        notepad_app.insert_char('\n');
                    } else if app_is_focused(&compositor, 2) {
                        enter_char_consumed = true;
                        notepad_app.menu = 0;
                    }
                }
                drivers::keyboard::KeyAction::ShutdownOS => {
                    serial_print("[minOs] System Shutdown requested!\n");
                    unsafe {
                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                        core::arch::asm!("cli; hlt");
                    }
                }
                drivers::keyboard::KeyAction::CopyText => {
                    if app_is_focused(&compositor, 2) { notepad_app.copy_selection(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_copy(); }
                    else if app_is_focused(&compositor, 8) { calculator_app.copy_result(); }
                }
                drivers::keyboard::KeyAction::PasteText => {
                    if app_is_focused(&compositor, 2) { notepad_app.paste_clipboard(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_paste(); }
                    else if app_is_focused(&compositor, 8) { calculator_app.paste_expression(); }
                }
                drivers::keyboard::KeyAction::CutText => {
                    if app_is_focused(&compositor, 2) { notepad_app.cut_selection(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_cut(); }
                    else if app_is_focused(&compositor, 8) { calculator_app.cut_expression(); }
                }
                drivers::keyboard::KeyAction::SelectAllText => {
                    let focused = app_is_focused(&compositor, 2);
                    if focused { notepad_app.select_all(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_select_all(); }
                }
                drivers::keyboard::KeyAction::Undo => {
                    let focused = app_is_focused(&compositor, 2);
                    if focused { notepad_app.undo(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_undo_edit(); }
                }
                drivers::keyboard::KeyAction::Redo => {
                    let focused = app_is_focused(&compositor, 2);
                    if focused { notepad_app.redo(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_redo_edit(); }
                }
                drivers::keyboard::KeyAction::HomeKey => {
                    let focused = app_is_focused(&compositor, 2);
                    if focused { notepad_app.move_home(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_home(); }
                }
                drivers::keyboard::KeyAction::EndKey => {
                    let focused = app_is_focused(&compositor, 2);
                    if focused { notepad_app.move_end(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_end(); }
                }
                drivers::keyboard::KeyAction::Delete => {
                    // Görev Yöneticisi odaklıysa seçili süreci sonlandır.
                    let tm_focused = app_is_focused(&compositor, 7);
                    if tm_focused {
                        if let Some(target) = taskmgr_app.selected_app_idx() {
                            taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[target],
                                &mut terminal_app);
                        }
                    } else if app_is_focused(&compositor, 2) {
                        notepad_app.delete_forward();
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_delete();
                    }
                }
                drivers::keyboard::KeyAction::Refresh => { /* TODO */ }
                drivers::keyboard::KeyAction::BrowserAddress => {
                    let focused = app_is_focused(&compositor, 3);
                    if focused { browser_app.focus_address_end(); }
                }
                drivers::keyboard::KeyAction::BrowserRefresh => {
                    let focused = app_is_focused(&compositor, 3);
                    if focused { browser_app.navigate(); }
                }
            }
        }

        // 4. Fare Tekerlek (Scroll)
        let scroll_val = mouse.take_scroll_delta();
        if scroll_val != 0 {
            // Başlat menüsü açıkken scroll wheel desteği
            if compositor.is_start_menu_open {
                let active_g = compositor.active_start_group;
                let count = gui::MENU_GROUPS[active_g].2;
                if scroll_val > 0 {
                    compositor.selected_start_item = compositor.selected_start_item.saturating_sub(1);
                } else if compositor.selected_start_item + 1 < count {
                    compositor.selected_start_item += 1;
                }
            }
            // Dosya Yöneticisi (pencere 0): listeyi kaydır ve imleci de taşı.
            if let Some((_, w)) = focused_window_for_app(&compositor, 0) {
                if w.is_open && w.is_focused && !w.is_minimized {
                    if scroll_val > 0 {
                        if filemanager_app.selected_idx > 0 { filemanager_app.selected_idx -= 1; }
                    } else if filemanager_app.selected_idx + 1 < filemanager_app.file_count {
                        filemanager_app.selected_idx += 1;
                    }
                }
            }
            // CMD geçmişi: yalnızca çıktı panelini kaydır; giriş satırı sabit kalır.
            if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                if w.is_open && w.is_focused && !w.is_minimized
                    && gui::in_rect(mouse.x as usize, mouse.y as usize, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h() + 28))) {
                    let view_lines = w.height.saturating_sub(86) / 13;
                    terminal_app.scroll_output(scroll_val as isize, view_lines);
                }
            }
            // Not Defteri (pencere 2): satır bazlı dikey kaydırma.
            if let Some((idx, w)) = focused_window_for_app(&compositor, 2) {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(2, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 16; // 1 satır = 16px
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[idx].as_mut() {
                        wm.scroll.y = new_off;
                    }
                }
            }
            // Görüntüleyicide tekerlek zoom kısayoludur (yukarı yakınlaştırır).
            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {
                if w.is_open && w.is_focused && !w.is_minimized
                    && gui::in_rect(mx, my, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h()))) {
                    if scroll_val > 0 { imageviewer_app.zoom_in(); } else { imageviewer_app.zoom_out(); }
                }
            }
            // Dosya Yöneticisi listesi de kaydırılabilir (içerik sığmıyorsa).
            if let Some((idx, w)) = focused_window_for_app(&compositor, 0) {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(0, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 24;
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[idx].as_mut() {
                        wm.scroll.y = new_off;
                    }
                }
            }
        }

        // 5. Pencere Sürükleme ve Yeniden Boyutlandırma
        if mouse.left_button {
            if let Some(w) = compositor.windows[1] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    if settings_app.active_tab == 0 {
                        let track = apps::settings::SystemSettings::volume_track_rect(w.x, w.y);
                        if gui::in_rect(mx, my, (track.0, track.1.saturating_sub(5), track.2, track.3 + 10)) {
                            settings_app.set_volume_from_range(mx, track.0, track.2);
                        }
                    } else if settings_app.active_tab == 1 {
                        let track = apps::settings::SystemSettings::night_light_track_rect(w.x, w.y);
                        if gui::in_rect(mx, my, (track.0, track.1.saturating_sub(5), track.2, track.3 + 10)) {
                            let span=track.2.saturating_sub(1).max(1);
                            settings_app.night_light_level = ((mx-track.0).min(span)*100/span).min(100) as u8;
                            settings_app.night_light = true;
                        }
                    }
                }
            }
            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {
                if w.is_focused {
                    // Windows Paint davranışı: sol tuş basılı tutuldukça fare
                    // hareketi sürekli çizer. Tuval dışına çıkınca vuruşu bitir.
                    if paint_apps[paint_idx].is_none() { paint_apps[paint_idx] = Some(apps::paint::PaintApp::new()); }
                    let paint = paint_apps[paint_idx].as_mut().unwrap();
                    if let Some((cxp, cyp)) = paint.to_canvas(w.width, w.height, mx - w.x, my - w.y) {
                        paint.paint(cxp, cyp);
                    } else {
                        paint.end_stroke();
                    }
                }
            }
            if compositor.is_volume_popup_open {
                let track = compositor.volume_track_rect(&l);
                if gui::in_rect(mx, my, (track.0.saturating_sub(8), track.1.saturating_sub(8), track.2+16, track.3+16)) {
                    settings_app.set_volume_from_range(mx, track.0, track.2);
                }
            }
            if notepad_dragging {
                if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);
                    notepad_app.selection_end = Some(at);
                }
            }
            if let Some((idx, grab, thumb_h, track_h, content_h, view_h)) = vscroll_drag {
                // Tutamacı fare ile sürükle → kaydırma ofseti.
                let track_top = compositor.windows[idx]
                    .map(|w| w.content_rect().1)
                    .unwrap_or(0);
                let max_thumb_travel = track_h.saturating_sub(thumb_h);
                let desired_top = my.saturating_sub(grab).max(track_top).min(track_top + max_thumb_travel);
                let ratio = if max_thumb_travel == 0 { 0.0 }
                    else { (desired_top - track_top) as f32 / max_thumb_travel as f32 };
                let max_off = content_h.saturating_sub(view_h);
                let new_off = ((ratio * max_off as f32) as usize).min(max_off);
                if let Some(wm) = compositor.windows[idx].as_mut() {
                    wm.scroll.y = new_off;
                }
            } else if let Some(idx) = dragging_win {
                if let Some(w) = compositor.windows[idx].as_mut() {
                    let max_x = width.saturating_sub(w.width);
                    let max_y = height.saturating_sub(w.height + gui::taskbar_h());
                    let prev_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);
                    w.x = mx.saturating_sub(drag_offset_x).min(max_x);
                    w.y = my.saturating_sub(drag_offset_y).min(max_y);
                    let new_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);
                    let dirty = match drag_prev_box {
                        Some(p) => drivers::video::rect_union(p, new_box),
                        None => drivers::video::rect_union(prev_box, new_box),
                    };
                    drag_dirty_rect = Some(drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24)));
                    drag_prev_box = Some(new_box);
                }
            } else if let Some(idx) = resizing_win {
                let edge = resize_edge.unwrap_or(gui::ResizeEdge::BottomRight);
                if let Some(w) = compositor.windows[idx].as_mut() {
                    use gui::ResizeEdge::*;
                    let (ox, oy, ow, oh) = resize_orig;
                    let left = ox + ow;   // sol kenarın sabit sağ referansı
                    let right = ox;       // sağ kenarın sabit sol referansı
                    let top = oy + oh;    // üst kenarın sabit alt referansı
                    let bottom = oy;      // alt kenarın sabit üst referansı

                    let mut nx = w.x;
                    let mut ny = w.y;
                    let mut nw = w.width;
                    let mut nh = w.height;

                    // Sağ kenar sürükleniyor
                    if matches!(edge, Right | TopRight | BottomRight) {
                        nw = mx.saturating_sub(right).max(w.min_width)
                            .min(w.max_width.min(width.saturating_sub(w.x)));
                    }
                    // Sol kenar sürükleniyor
                    if matches!(edge, Left | TopLeft | BottomLeft) {
                        let desired = mx.min(left);
                        nw = left.saturating_sub(desired).max(w.min_width).min(w.max_width).min(left);
                        nx = left.saturating_sub(nw).min(width.saturating_sub(nw));
                    }
                    // Alt kenar sürükleniyor
                    if matches!(edge, Bottom | BottomLeft | BottomRight) {
                        let max_h = height.saturating_sub(gui::taskbar_h()).saturating_sub(w.y);
                        nh = my.saturating_sub(bottom).max(w.min_height).min(w.max_height).min(max_h);
                    }
                    // Üst kenar sürükleniyor
                    if matches!(edge, Top | TopLeft | TopRight) {
                        let desired = my.min(top);
                        nh = top.saturating_sub(desired).max(w.min_height).min(w.max_height).min(top);
                        ny = top.saturating_sub(nh);
                    }

                    let prev_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);
                    w.x = nx;
                    w.y = ny;
                    w.width = nw;
                    w.height = nh;
                    w.is_maximized = false;
                    let new_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);
                    let dirty = match drag_prev_box {
                        Some(p) => drivers::video::rect_union(p, new_box),
                        None => drivers::video::rect_union(prev_box, new_box),
                    };
                    drag_dirty_rect = Some(drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24)));
                    drag_prev_box = Some(new_box);
                }
            }
        }

        if mouse_released {
            drag_prev_box = None;
            drag_dirty_rect = None;
            if let Some(paint_idx) = compositor.active_window_idx {
                if let Some(paint) = paint_apps[paint_idx].as_mut() { paint.end_stroke(); }
            }
            notepad_dragging = false;
            // Modern pencere "snap": başlık çubuğu ekranın en üstüne sürüklenip
            // bırakılırsa pencere maksimize edilir (Windows/KDE davranışı).
            if let Some(idx) = dragging_win {
                if my <= 4 {
                    compositor.focus_window(idx);
                    compositor.toggle_maximize(idx, width, height);
                }
            }
            dragging_win = None;
            resizing_win = None;
            resize_edge = None;
            vscroll_drag = None;
        }

        // Alt tuşu bırakıldığında Alt+Tab penceresini kapat
        if !keyboard.alt && compositor.is_alt_tab_open {
            compositor.finish_alt_tab();
        }

        // 6. Klavye karakter girişleri
        while let Some(ch) = keyboard.take_last_char() {
            if ch == '\n' && enter_char_consumed {
                enter_char_consumed = false;
                continue;
            }
            // Başlat menüsü açıkken klavye kısayolları menü SIRASINI kullanır
            // (pencere indeksini değil) — start_menu_app_idx ile eşlenir.
            if ch == '\x1b' && compositor.is_start_menu_open {
                compositor.close_start_menu();
            } else if ch == '\x1b' && compositor.is_context_menu_open {
                compositor.is_context_menu_open = false;
            } else if ch == '\x1b' && taskmgr_app.launch_menu_open {
                taskmgr_app.launch_menu_open = false;
            } else if ch == '\x1b' && notepad_app.menu != 0 {
                notepad_app.menu = 0;
            } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {
                if ch == '\x1b' {
                    filemanager_app.rename_active = false;
                } else if ch == '\x08' {
                    filemanager_app.rename_backspace();
                } else if ch == '\n' {
                    filemanager_app.commit_rename(&mut fs_mgr.vfs);
                } else if ch >= ' ' {
                    filemanager_app.rename_char(ch);
                }
            } else if compositor.is_start_menu_open && ch == '\x08' {
                if compositor.start_search_len > 0 {
                    compositor.start_search_len -= 1;
                    compositor.start_search[compositor.start_search_len] = 0;
                    compositor.selected_start_item = 0;
                }
            } else if compositor.is_start_menu_open && ch.is_ascii_alphanumeric() {
                if compositor.start_search_len == 0 { compositor.selected_start_item = 0; }
                compositor.push_start_search(ch);
            } else if compositor.is_start_menu_open && (ch >= '1' && ch <= '9') {
                let menu_index = (ch as usize) - ('1' as usize);
                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, app_idx);
                    compositor.close_start_menu();
                }
            } else if compositor.is_start_menu_open && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B') {
                let menu_index = if ch == 'a' || ch == 'A' { 9 } else { 10 };
                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, app_idx);
                    compositor.close_start_menu();
                }
            } else if compositor.is_start_menu_open && (ch == 'r' || ch == 'R') {
                if compositor.confirm_restart { restart_system(); }
                compositor.confirm_restart = true;
                compositor.confirm_shutdown = false;
            } else if compositor.is_start_menu_open && ch == '0' {
                if compositor.confirm_shutdown {
                    unsafe { core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16); core::arch::asm!("cli; hlt"); }
                }
                compositor.confirm_shutdown = true;
                compositor.confirm_restart = false;
            } else if ch == '\t' && app_is_focused(&compositor, 10) {
                terminal_app.complete();
            } else if ch == '\t' && app_is_focused(&compositor, 7) {
                taskmgr_app.focus_next_control();
            } else if ch == '\t' && app_is_focused(&compositor, 1) {
                settings_app.focus_next_control();
            } else if ch == '\t' && app_is_focused(&compositor, 3) {
                browser_app.focus_next_control();
            } else if ch == '\t' && app_is_focused(&compositor, 0) {
                filemanager_app.focus_next_control();
            } else {
                let browser_focused = app_is_focused(&compositor, 3);
                if browser_focused && browser_app.address_focused {
                    if ch == '\x08' { browser_app.address_backspace(); }
                    else if ch == '\n' { browser_app.navigate(); }
                    else if ch >= ' ' { browser_app.address_char(ch); }
                }
                let notepad_focused = app_is_focused(&compositor, 2);
                if notepad_focused {
                    if ch == '\x08' {
                        notepad_app.backspace();
                    } else if ch == '\n' || ch >= ' ' {
                        notepad_app.insert_char(ch);
                    }
                }
                let calc_focused = app_is_focused(&compositor, 8);
                let term_focused = app_is_focused(&compositor, 10);
                if calc_focused {
                    if ch == '\x08' { calculator_app.backspace(); }
                    else if ch == '\x1b' { calculator_app.clear(); }
                    else if ch == '=' { calculator_app.press('='); }
                    else if ch.is_ascii_digit() || ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '.' { calculator_app.insert(ch); }
                }
                if term_focused { if ch == '\x08' { terminal_app.backspace(); } else if ch == '\n' { terminal_app.submit(); } else if ch >= ' ' { terminal_app.insert(ch); } }
                // Terminalde Enter'a basıldıysa bekleyen komutu VFS/PMM bağlamında yürüt.
                if let Some(cmd) = terminal_app.take_pending() {
                    let n = terminal_app.pending_len();
                    let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");
                    if let Some(app_idx) = terminal_app.execute(line, &mut fs_mgr.vfs, &memory_mgr.pmm, &now) {
                        open_app(&mut compositor, app_idx);
                    }
                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                }
                if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                    if let Some(paint) = paint_apps[paint_idx].as_mut() {
                        if ch == 'c' || ch == 'C' { paint.clear(); }
                        else if ch == 'e' || ch == 'E' { paint.eraser = !paint.eraser; }
                    }
                }
                let image_focused = app_is_focused(&compositor, 4);
                if image_focused {
                    if ch == '+' || ch == '=' { imageviewer_app.zoom_in(); }
                    else if ch == '-' || ch == '_' { imageviewer_app.zoom_out(); }
                    else if ch == '0' { imageviewer_app.zoom_level = 4; }
                }
            }
        }

        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;
        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;

        let is_dragging = dragging_win.is_some() || resizing_win.is_some() || vscroll_drag.is_some() || notepad_dragging;
        let paint_active = mouse.left_button && app_is_focused(&compositor, 9);
        let is_interacting = is_dragging || paint_active;

        let media_animating = (musicplayer_app.is_playing && !audio_dev.is_muted) || videoplayer_app.is_playing;
        let frame_interval_ms = if is_interacting { 16 } else { 33 };
        let timer_tick_fired = monotonic_ms().wrapping_sub(last_frame_ms) >= frame_interval_ms;
        if timer_tick_fired {
            last_frame_ms = monotonic_ms();
            anim_tick = anim_tick.wrapping_add(1);
        }

        // Ağ kartından gelen paketleri işle (ARP cevapları, ICMP vs.).
        drivers::e1000::poll();
        browser_app.poll_network();

        // Beş parçalı, HDA PCM tabanlı melodi yürütücüsü. (HDA yoksa PIT/PC speaker fallback).
        audio_dev.set_volume(settings_app.volume);
        audio_dev.is_muted = settings_app.is_muted;
        musicplayer_app.volume = settings_app.volume as usize;
        if musicplayer_app.is_playing && !audio_dev.is_muted && audio_dev.volume_left > 0 {
            if anim_tick.wrapping_sub(last_music_beat) >= 24 {
                const NOTES: [u32; 8] = [440, 523, 587, 659, 587, 523, 494, 392];
                let track_offset = musicplayer_app.current_track as u32 * 22;
                let freq = NOTES[music_note % NOTES.len()] + track_offset;
                // HDA hazır ise gerçek PCM örneği üret ve gönder.
                if drivers::hda::is_ready() {
                    let sample_count = 1920; // 40 ms
                    let mut pcm: [i16; 512] = [0; 512]; // 256 L + 256 R çifti
                    let volume_scale = (audio_dev.volume_left as i32).min(100) as i16;
                    let mut idx = 0;
                    while idx < sample_count {
                        let n = (sample_count - idx).min(pcm.len() / 2);
                        for k in 0..n {
                            let t = (idx + k) as f32 / 48_000.0;
                            let angle = 6.28318f32 * freq as f32 * t;
                            let x = angle - 6.28318 * ((angle / 6.28318) as i32) as f32;
                            let x2 = x * x;
                            let v = x - x2 * x / 6.0 + x2 * x2 * x / 120.0 - x2 * x2 * x2 * x / 5040.0;
                            let sample = (v * 0.3 * volume_scale as f32 / 100.0 * 32767.0) as i16;
                            pcm[k * 2] = sample;
                            pcm[k * 2 + 1] = sample;
                        }
                        drivers::hda::push_pcm(&pcm[..n * 2]);
                        idx += n;
                    }
                } else {
                    audio_dev.set_pc_speaker_tone(freq);
                }
                speaker_on = true;
                music_note = (music_note + 1) % NOTES.len();
                musicplayer_app.progress = (musicplayer_app.progress + 1) % 101;
                last_music_beat = anim_tick;
            }
        } else if speaker_on || audio_dev.is_muted || audio_dev.volume_left == 0 {
            drivers::hda::silence();
            audio_dev.set_pc_speaker_tone(0);
            speaker_on = false;
            last_music_beat = anim_tick;
        }

        let night_light_active = settings_app.night_light;
        let full_redraw = input_update.keyboard || buttons_changed || scroll_val != 0
            || force_render || (mouse_moved && (is_interacting || night_light_active))
            || (timer_tick_fired && media_animating);

        if full_redraw {
            let tm_open = compositor.windows.iter().flatten().any(|w| w.app_id == 7 && w.is_open && !w.is_minimized);
            if tm_open {
                taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);
            }
            keep_windows_on_screen(&mut compositor, width, height);
            render_full_frame(
                &mut fb, &compositor, &filemanager_app, &settings_app,
                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app,
                &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick, &mut app_render_cycles,
                drag_dirty_rect,
            );
            drag_dirty_rect = None;
        } else if mouse_moved {
            // İmleç hareketinde uygulamaları ve tüm ekranı yeniden çizmek yerine
            // sadece önceki ve yeni imleç alanlarını birleşik tek bir dikdörtgen olarak güncelle.
            let old_rect = fb.restore_cursor_background();
            compositor.draw_mouse_cursor(&mut fb, mx, my);
            let new_rect = fb.cursor_rect();
            match (old_rect, new_rect) {
                (Some(o), Some(n)) => {
                    let u = drivers::video::rect_union(o, n);
                    fb.flush_rect(u);
                }
                (Some(o), None) => fb.flush_rect(o),
                (None, Some(n)) => fb.flush_rect(n),
                _ => {}
            }
        }

        last_mouse_x = mouse.x;
        last_mouse_y = mouse.y;
        last_buttons = (mouse.left_button, mouse.right_button);

        if !input_update.mouse && !input_update.keyboard && !mouse_moved && !buttons_changed && !force_render {
            // Boşta bekleme: port 0x80 VM çıkışları olmadan spin_loop ile bekle.
            // PS/2 giriş tamponunda veri algılandığı anda gecikmesiz döngüden çık.
            let wait_start = read_cpu_cycles();
            let wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(2_500);
            while read_cpu_cycles().wrapping_sub(wait_start) < wait_cycles {
                if drivers::ps2::has_input() {
                    break;
                }
                core::hint::spin_loop();
            }
            let loop_end_cycles = read_cpu_cycles();
            let elapsed = loop_end_cycles.wrapping_sub(loop_start_cycles);
            let idle_elapsed = loop_end_cycles.wrapping_sub(wait_start).min(elapsed);
            cpu_sample_total_cycles = cpu_sample_total_cycles.saturating_add(elapsed);
            cpu_sample_active_cycles = cpu_sample_active_cycles.saturating_add(elapsed.saturating_sub(idle_elapsed));
        } else {
            core::hint::spin_loop();
            let elapsed = read_cpu_cycles().wrapping_sub(loop_start_cycles);
            cpu_sample_total_cycles = cpu_sample_total_cycles.saturating_add(elapsed);
            cpu_sample_active_cycles = cpu_sample_active_cycles.saturating_add(elapsed);
        }

        let now_cycles = read_cpu_cycles();
        let sample_window = unsafe { TSC_CYCLES_PER_US }.saturating_mul(250_000);
        if now_cycles.wrapping_sub(cpu_sample_start) >= sample_window {
            taskmgr_app.cpu_usage_pct = if cpu_sample_total_cycles == 0 { 0 } else {
                (cpu_sample_active_cycles.saturating_mul(100) / cpu_sample_total_cycles).min(100) as usize
            };
            cpu_sample_start = now_cycles;
            cpu_sample_total_cycles = 0;
            cpu_sample_active_cycles = 0;
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_print("[PANIC] Kernel panic: panic details unavailable\n");
    let _ = info;
    loop {
        core::hint::spin_loop();
    }
}






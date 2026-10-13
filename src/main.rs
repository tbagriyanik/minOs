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
    panic!("Allocation error: {:?}", layout);
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

#[used]
#[link_section = ".requests"]
static MEMORY_MAP_REQUEST: LimineMemoryMapRequest = LimineMemoryMapRequest {
    id: [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b, 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    revision: 0,
    response: core::ptr::null(),
};

#[inline(never)]
fn serial_print(s: &str) {
    for b in s.bytes() {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b);
        }
    }
}

fn restart_system() -> ! {
    serial_print("[MinOS] Restart requested from Start Menu.\n");
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
    let addr = ptr as usize as u64;
    let upper = addr >> 47;
    (upper == 0 || upper == 0x1_FFFF) && addr % core::mem::align_of::<T>() as u64 == 0
}

/// Verilen uygulama penceresini açar, küçültme durumunu kaldırır ve odaklar.
fn open_app(compositor: &mut gui::MinGuiCompositor, idx: usize) {
    if let Some(win) = compositor.windows[idx].as_mut() {
        win.is_open = true;
        win.is_minimized = false;
        win.is_maximized = false;
    }
    compositor.focus_window(idx);
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
    paint: &mut apps::paint::PaintApp,
    terminal: &mut apps::terminal::TerminalApp,
) {
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

    match idx {
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
        9 => *paint = apps::paint::PaintApp::new(),
        10 => {
            *terminal = apps::terminal::TerminalApp::new();
            terminal.init_banner();
        }
        _ => return,
    }
    compositor.focus_top_window();
}

/// Bir pencerenin (uygulama indeksine göre) dikey içerik yüksekliğini hesaplar.
/// Kaydırma çubuğunun gerekip gerekmediğini ve tutamaç boyutunu belirlemek için
/// hem çizim hem de tıklama kod yollarında ortak kullanılır.
fn window_content_h(
    idx: usize,
    _win_h: usize,
    win_w: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
) -> usize {
    match idx {
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
    init_timebase();
    // Initialize heap for dynamic allocations
    init_heap();
    serial_print("[MinOS] Kernel _start reached!\n");
    arch::x86_64::enable_sse();
    arch::x86_64::disable_pic();
    serial_print("[MinOS] SSE enabled & PIC IRQs masked!\n");

    // 1. Donanım Kesmeleri (IDT) ve PMM Başlatımı
    arch::x86_64::init_interrupts();
    let mut memory_mgr = mm::MemoryManager::new();
    unsafe {
        let response = core::ptr::read_volatile(core::ptr::addr_of!(MEMORY_MAP_REQUEST.response));
        if is_canonical_ptr(response) {
            // SAFETY: pointer comes from Limine's response slot and passed null,
            // alignment and canonical-address checks above.
            let count = core::ptr::read_volatile(core::ptr::addr_of!((*response).entry_count));
            let entries = core::ptr::read_volatile(core::ptr::addr_of!((*response).entries));
            let count = count.min(256) as usize;
            let array_bytes = count.checked_mul(core::mem::size_of::<*const LimineMemoryMapEntry>());
            if count > 0 && is_canonical_ptr(entries) && array_bytes.is_some() {
                let array_end = (entries as usize).checked_add(array_bytes.unwrap());
                if array_end.map_or(false, |end| is_canonical_ptr(end as *const LimineMemoryMapEntry)) {
                for i in 0..count {
                    let slot_addr = match (entries as usize).checked_add(i * core::mem::size_of::<*const LimineMemoryMapEntry>()) {
                        Some(addr) => addr,
                        None => break,
                    };
                    let slot = slot_addr as *const *const LimineMemoryMapEntry;
                    if !is_canonical_ptr(slot) { break; }
                    let entry_ptr = core::ptr::read_volatile(slot);
                    if !is_canonical_ptr(entry_ptr) { continue; }
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

    // 2. Sanal Dosya Sistemi (VFS / TarFS) Başlatımı
    let mut fs_mgr = fs::FileSystemManager::new();

    // 3. Ağ ve Ses Sürücülerinin Başlatımı (HDA + e1000)
    // Önce HHDM ofsetini al (fiziksel MMIO'ya sanal erişim için).
    let hhdm_offset = unsafe {
        let resp = core::ptr::read_volatile(core::ptr::addr_of!(HHDM_REQUEST.response));
        if !resp.is_null() {
            let off = core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset));
            serial_print("[MinOS] HHDM offset okundu (0 yoksa gecerli).\n");
            off
        } else {
            serial_print("[MinOS] UYARI: HHDM response yok; MMIO erisimi calismaz!\n");
            0xFFFF_8000_0000_0000
        }
    };
    serial_print("[MinOS] HHDM offset alindi.\n");
    drivers::set_hhdm_offset(hhdm_offset);
    drivers::hda::init();
    drivers::e1000::init();
    drivers::netstack::init();
    let _ = drivers::netstack::dhcp_request();
    drivers::usb::init();
    let _net_dev = drivers::net::NetworkInterface::new();
    let mut audio_dev = drivers::audio::AudioDevice::new();

    // 4. Limine Framebuffer Alımı
    let (fb_ptr, width, height, pitch) = unsafe {
        let resp = core::ptr::read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response));
        if !resp.is_null() {
            let framebuffer_count = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffer_count));
            let fb_ptr_ptr = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffers));
            if framebuffer_count == 0 || fb_ptr_ptr.is_null() {
                serial_print("[MinOS] ERROR: Limine framebuffer list is NULL!\n");
                loop { core::hint::spin_loop(); }
            }
            let fb_info_ptr = core::ptr::read_volatile(fb_ptr_ptr);
            if fb_info_ptr.is_null() {
                serial_print("[MinOS] ERROR: Limine framebuffer entry is NULL!\n");
                loop { core::hint::spin_loop(); }
            }
            let fb_info = core::ptr::read_volatile(fb_info_ptr);
            (
                fb_info.address as *mut u32,
                fb_info.width as usize,
                fb_info.height as usize,
                fb_info.pitch as usize,
            )
        } else {
            serial_print("[MinOS] ERROR: Limine Framebuffer response is NULL!\n");
            loop {
                core::hint::spin_loop();
            }
        }
    };

    let mut fb = unsafe {
        drivers::video::Framebuffer::new(fb_ptr, width, height, pitch)
    };

    // 5. PS/2 Donanım Sürücüleri
    let mut mouse = drivers::mouse::MouseState::new();
    let mut keyboard = drivers::keyboard::KeyboardState::new();
    drivers::ps2::init();

    // 5b. Tema motorunu başlat
    gui::theme::init();
    serial_print("[MinOS] Theme engine initialized.\n");

    // İlk açılış ekranı: kedi logosu ve ilerleme animasyonu.
    fb.draw_rect(0, 0, width, height, gui::Color::DARK_BG);
    let cx = width.saturating_sub(32) / 2;
    let cy = height.saturating_sub(32) / 2;
    gui::MinGuiCompositor::draw_cat_icon(&mut fb, cx, cy.saturating_sub(18));
    gui::MinGuiCompositor::draw_string(&mut fb, cx.saturating_sub(8), cy + 8, "MinOS", gui::Color::TEXT_MAIN);
    for phase in 0..3 {
        let dots = match phase { 0 => ".  ", 1 => ".. ", _ => "..." };
        gui::MinGuiCompositor::draw_string(&mut fb, cx + 34, cy + 8, dots, gui::Color::ACCENT_BLUE);
        for _ in 0..180_000 { core::hint::spin_loop(); }
    }

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar
    let mut compositor = gui::MinGuiCompositor::new();
    let mut filemanager_app = apps::filemanager::FileManagerApp::new();
    let mut settings_app = apps::settings::SystemSettings::default();
    settings_app.display_res = (width, height);
    settings_app.load_from_vfs(&fs_mgr.vfs);
    keyboard.layout = settings_app.keyboard_layout;
    gui::theme::set_theme(settings_app.theme);
    let mut notepad_app = apps::notepad::NotepadApp::new();
    let mut browser_app = apps::browser::BrowserApp::new();
    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    let mut taskmgr_app = apps::taskmgr::TaskManagerApp::new();
    let mut calculator_app = apps::calculator::CalculatorApp::new();
    let mut paint_app = apps::paint::PaintApp::new();
    let mut terminal_app = apps::terminal::TerminalApp::new();
    terminal_app.init_banner();

    // 11 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimliği).
    // Açılışta TÜM pencereler kapalıdır; kullanıcı temiz bir masaüstüyle başlar
    // ve uygulamaları Başlat Menüsü, masaüstü simgeleri veya sağ tık ile açar.
    // 0: Dosya Yöneticisi
    let files_width = 620.min(width.saturating_sub(20)).max(200);
    let files_height = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "Dosya Yöneticisi - VFS / Ev", "Dosyalar",
        width.saturating_sub(files_width) / 2,
        height.saturating_sub(files_height + gui::taskbar_h()) / 2,
        files_width, files_height, false,
    ));
    // 1: Sistem Ayarları
    let settings_width = 620.min(width.saturating_sub(20)).max(200);
    let settings_height = 500.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "Sistem Ayarları", "Ayarlar",
        width.saturating_sub(settings_width) / 2,
        height.saturating_sub(settings_height + gui::taskbar_h()) / 2,
        settings_width, settings_height, false,
    ));
    // 2: Not Defteri
    compositor.add_window(gui::Window::new("Not Defteri - adsız.txt", "Notlar", 80, 70, 460, 270, false));
    // 3: Web Tarayıcısı
    compositor.add_window(gui::Window::new("Web Gezgini", "Web", 550, 60, 480, 280, false));
    // 4: Resim Gösterici
    compositor.add_window(gui::Window::new("Resim Gösterici - kedi_maskotu.bmp", "Resim", 120, 90, 450, 270, false));
    // 5: Müzik Çalar
    compositor.add_window(gui::Window::new("Müzik Çalar", "Müzik", 560, 80, 460, 245, false));
    // 6: Video Oynatıcı
    compositor.add_window(gui::Window::new("Video Oynatıcı - retro_drive.mp4", "Video", 150, 110, 460, 275, false));
    // 7: Görev Yöneticisi
    compositor.add_window(gui::Window::new("Görev Yöneticisi", "Görevler", 220, 60, 420, 420, false));
    compositor.add_window(gui::Window::new("Hesap Makinesi", "Hesap", 260, 120, 360, 340, false));
    compositor.add_window(gui::Window::new("Çizim Tahtası", "Çizim", 180, 80, 520, 360, false));
    compositor.add_window(gui::Window::new("Komut İstemi", "Komut", 220, 100, 560, 300, false));

    // Açılışta hiçbir pencere odaklı değildir (temiz masaüstü).
    compositor.active_window_idx = None;

    // RTC - gerçek zamanlı saat
    let mut now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
    compositor.cal_year = now.year;
    compositor.cal_month = now.month;
    serial_print("[MinOS] RTC clock online.\n");

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
    let mut clock_spin: usize = 0;
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


    let mut app_render_cycles = [0u64; 11];
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
                             paint: &apps::paint::PaintApp,
                             term: &apps::terminal::TerminalApp,
                             now: &drivers::rtc::DateTime,
                             mouse: &drivers::mouse::MouseState,
                             kbd: &drivers::keyboard::KeyboardState,
                             total_ram_bytes: u64,
                             tick: usize,
                             app_render_cycles: &mut [u64; 11]| {
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
                let content_h = window_content_h(idx, win.height, win.width, fm, np);
                let content_w = cw;
                let want_v = content_h > ch;
                let want_h = content_w > cw;
                let v_gutter = if want_v { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                let h_gutter = if want_h { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                // İçerik alanı (kaydırma çubukları kadar daraltılmış).
                let content_area = (cx, cy, cw.saturating_sub(v_gutter), ch.saturating_sub(h_gutter));
                fb.set_clip(content_area.0, content_area.1, content_area.2.max(1), content_area.3.max(1));

                let render_start = read_cpu_cycles();
                match idx {
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
                    9 => paint.render_content(fb, win.x, win.y, win.width, win.height),
                    10 => term.render_content(fb, win.x, win.y, win.width, win.height),
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

        // 4. Backbuffer'ı VRAM'e tek seferde kopyala (flicker-free)
        fb.apply_night_light(if st.night_light { st.night_light_level } else { 0 });
        fb.flush();
    };

    // İlk kareyi çiz
    render_full_frame(
        &mut fb, &compositor, &filemanager_app, &settings_app,
        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_app, &terminal_app, &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick,
        &mut app_render_cycles,
    );

    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü
    loop {
        let loop_start_cycles = read_cpu_cycles();
        let input_occurred = drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);
        loop_counter = loop_counter.wrapping_add(1);
        let mx = mouse.x as usize;
        let my = mouse.y as usize;
        let mouse_clicked = mouse.left_button && !last_buttons.0;
        let mouse_released = !mouse.left_button && last_buttons.0;
        let right_clicked = mouse.right_button && !last_buttons.1;
        let l = gui::MinGuiCompositor::tray_layout(width, height);
        let tb_y = l.tb_y;

        // Saati periyodik olarak yenile
        let mut force_render = false;
        clock_spin += 1;
        if clock_spin >= 3_000 {
            clock_spin = 0;
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
            compositor.is_start_menu_open = false;
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
                            for i in 0..filemanager_app.files.len() {
                                let item_y = row_y_base + i * 24;
                                if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                    compositor.context_file_idx = Some(i);
                                    filemanager_app.selected_idx = i;
                                    break;
                                }
                            }
                        }
                        kind = if compositor.context_file_idx.is_some() {
                            gui::ContextKind::FileManager
                        } else {
                            gui::ContextKind::Window
                        };
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
            compositor.is_alt_tab_open = false;
            if taskmgr_app.launch_menu_open {
                let in_menu = compositor.windows[7].map_or(false, |w| {
                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_menu_rect(w.x, w.y, w.width))
                });
                let in_button = compositor.windows[7].map_or(false, |w| {
                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_button_rect(w.x, w.y, w.width))
                });
                if !in_menu && !in_button { taskmgr_app.launch_menu_open = false; }
            }
            let mut consumed = false;
            // Not Defteri metin alanı: imleci konumlandır, çift tıkta kelime seç,
            // tek tıkta seçimi temizle ve sürükleyerek seçimi başlat.
            if let Some(w) = compositor.windows[2] {
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
                            if let Some(file_idx) = compositor.context_file_idx {
                                let tag = filemanager_app.files[file_idx].icon_tag;
                                let target_app_idx = match clicked_item {
                                    0 => match tag {
                                        "TXT" | "LOG" | "CFG" => 2,
                                        "IMG" => 4,
                                        "SND" => 5,
                                        "VID" => 6,
                                        "WEB" => 3,
                                        _ => 2,
                                    },
                                    1 => 2,
                                    2 => 4,
                                    3 => 5,
                                    4 => 6,
                                    _ => 0,
                                };
                                open_app(&mut compositor, target_app_idx);
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
            let active_g = compositor.active_start_group;
            let sub_rect = gui::MinGuiCompositor::start_menu_submenu_rect(menu_y, active_g, tb_y);

            let mut cat_clicked = false;
            if gui::in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22)) {
                open_app(&mut compositor, 1);
                settings_app.active_tab = 0;
                compositor.is_start_menu_open = false;
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
                                serial_print("[MinOS] System Shutdown confirmed via Start Menu!\n");
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
                        compositor.is_start_menu_open = false;
                        break;
                    }
                }

                if !item_clicked {
                    if gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) || gui::in_rect(mx, my, sub_rect) {
                        consumed = true;
                    } else {
                        compositor.is_start_menu_open = false;
                        compositor.confirm_shutdown = false;
                        compositor.confirm_restart = false;
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
                } else {
                    compositor.confirm_shutdown = false;
                    compositor.confirm_restart = false;
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
                let mut tab_x = gui::MinGuiCompositor::taskbar_tab_start();
                for idx in 0..11 {
                    if let Some(win) = compositor.windows[idx] {
                        if win.is_open {
                            if mx >= tab_x && mx < tab_x + gui::MinGuiCompositor::taskbar_tab_width() {
                                if win.is_minimized {
                                    open_app(&mut compositor, idx);
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
                            tab_x += gui::MinGuiCompositor::taskbar_tab_step();
                        }
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
                        compositor.is_start_menu_open = false;
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
                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_app,
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
                        let c_h = window_content_h(idx, wh, ww, &filemanager_app, &notepad_app);
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
                        match idx {
                            0 => {
                                // Dosya Yöneticisi: [Yenile], [Aç], liste
                                if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::refresh_button_rect(wx, wy, ww)) {
                                    filemanager_app.selected_idx = 0;
                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::open_button_rect(wx, wy, ww)) {
                                    let selected_file = &filemanager_app.files[filemanager_app.selected_idx];
                                    let target = match selected_file.icon_tag {
                                        "TXT" | "LOG" | "CFG" => 2,
                                        "IMG" => 4,
                                        "SND" => 5,
                                        "VID" => 6,
                                        "WEB" => 3,
                                        _ => 2,
                                    };
                                    open_app(&mut compositor, target);
                                } else if mx >= wx + 6 && mx < wx + 116 && my >= wy + 68 && my < wy + 68 + 6 * 22 {
                                    let nav = ((my - (wy + 68)) / 22).min(5);
                                    filemanager_app.current_path = match nav {
                                        0 => "/",
                                        1 => "/root/home/minos/Documents",
                                        2 => "/root/home/minos/Media",
                                        3 => "/root/home/minos/Downloads",
                                        4 => "/boot",
                                        _ => "/media/usb0",
                                    };
                                    filemanager_app.selected_idx = 0;
                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 && my <= wy + 250 {
                                    for i in 0..filemanager_app.files.len() {
                                        if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::row_rect(wx, wy, ww, i)) {
                                            filemanager_app.selected_idx = i;
                                            let selected_file = &filemanager_app.files[i];
                                            let target = match selected_file.icon_tag {
                                                "TXT" | "LOG" | "CFG" => 2,
                                                "IMG" => 4,
                                                "SND" => 5,
                                                "VID" => 6,
                                                "WEB" => 3,
                                                _ => 2,
                                            };
                                            open_app(&mut compositor, target);
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
                                else if notepad_app.menu != 0 && mx >= wx + 8 && mx < wx + 340 && my >= wy + 54 && my < wy + 150 {
                                    let item = (my.saturating_sub(wy + 54)) / 20;
                                    match (notepad_app.menu, item) {
                                        // Dosya Menüsü: 0: Yeni, 1: Kaydet, 2: Temizle
                                        (1, 0) => notepad_app.clear(),
                                        (1, 1) => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                        (1, 2) => notepad_app.clear(),
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
                                    settings_app.save_all(&compositor, &mut fs_mgr.vfs);
                                }
                                if gui::in_rect(mx, my, (wx + 6, wy + 58, ww.saturating_sub(12), wh.saturating_sub(90))) {
                                    let at = notepad_app.index_at_scrolled(mx - wx, my - wy, ww, compositor.windows[2].map(|w| w.scroll.y).unwrap_or(0));
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
                                    browser_app.address_focused = true;
                                    browser_app.set_url(b"");
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
                                        imageviewer_app.zoom_level = (imageviewer_app.zoom_level + 1).min(8);
                                    } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {
                                        imageviewer_app.zoom_level = (imageviewer_app.zoom_level.saturating_sub(1)).max(1);
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
                                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_app,
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
                                    if mx >= palette_start && mx < palette_start + 16 { paint_app.color=gui::Color::ACCENT_BLUE; paint_app.eraser=false; }
                                    else if mx >= palette_start + 22 && mx < palette_start + 38 { paint_app.color=gui::Color::ACCENT_RED; paint_app.eraser=false; }
                                    else if mx >= palette_start + 44 && mx < palette_start + 60 { paint_app.color=gui::Color::ACCENT_GREEN; paint_app.eraser=false; }
                                    else if mx >= palette_start + 66 && mx < palette_start + 82 { paint_app.color=gui::Color::BLACK; paint_app.eraser=false; }
                                    else if mx >= palette_start + 92 && mx < palette_start + 150 { paint_app.eraser=true; }
                                    else if mx >= wx + ww.saturating_sub(66) && mx < wx + ww.saturating_sub(8) { paint_app.clear(); }
                                } else if let Some((cxp, cyp)) = paint_app.to_canvas(ww, wh, mx - wx, my - wy) {
                                    // İlk tıklama noktası (last=None iken tek nokta başlangıcı).
                                    paint_app.paint(cxp, cyp);
                                } else {
                                    // Tuval dışına tıklandı: kalem yolunu bitir.
                                    paint_app.end_stroke();
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
                    } else {
                        compositor.confirm_shutdown = false;
                        compositor.confirm_restart = false;
                    }
                }
                drivers::keyboard::KeyAction::CloseActiveWindow => {
                    let active_window = compositor.active_window_idx.filter(|&idx| {
                        compositor.windows[idx].map_or(false, |w| w.is_open && !w.is_minimized)
                    });
                    if let Some(idx) = active_window {
                        terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,
                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_app,
                            &mut terminal_app);
                    }
                }
                drivers::keyboard::KeyAction::CycleWindowFocus => {
                    compositor.is_alt_tab_open = true;
                    compositor.cycle_focus();
                }
                drivers::keyboard::KeyAction::OpenTaskManager => {
                    open_app(&mut compositor, 7);
                    compositor.is_start_menu_open = false;
                }
                drivers::keyboard::KeyAction::ArrowUp => {
                    if compositor.is_start_menu_open {
                        if compositor.selected_start_item > 0 {
                            compositor.selected_start_item -= 1;
                        }
                    } else if compositor.is_context_menu_open {
                        // Sağ tık / Bağlam menüsü gezintisi
                        if compositor.context_file_idx.unwrap_or(0) > 0 {
                            compositor.context_file_idx = Some(compositor.context_file_idx.unwrap() - 1);
                        }
                    } else {
                        let np_focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let terminal_focused = compositor.windows[10].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let tm_focused = compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let fm_focused = compositor.windows[0].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let set_focused = compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let br_focused = compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);

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
                        let active_g = compositor.active_start_group;
                        let count = gui::MENU_GROUPS[active_g].2;
                        if compositor.selected_start_item + 1 < count {
                            compositor.selected_start_item += 1;
                        }
                    } else if compositor.is_context_menu_open {
                        let items_len = compositor.context_items().len();
                        let cur = compositor.context_file_idx.unwrap_or(0);
                        if cur + 1 < items_len {
                            compositor.context_file_idx = Some(cur + 1);
                        }
                    } else {
                        let np_focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let terminal_focused = compositor.windows[10].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let tm_focused = compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let fm_focused = compositor.windows[0].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let set_focused = compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                        let br_focused = compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);

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
                    } else if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        notepad_app.move_left_ext(keyboard.shift);
                    } else if compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 0;
                        taskmgr_app.focused_control = 0;
                    } else if compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        settings_app.select_tab_prev();
                    } else if compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        browser_app.select_tab_prev();
                    }
                }
                drivers::keyboard::KeyAction::ArrowRight => {
                    if compositor.is_start_menu_open {
                        if compositor.active_start_group + 1 < 4 {
                            compositor.active_start_group += 1;
                            compositor.selected_start_item = 0;
                        }
                    } else if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        notepad_app.move_right_ext(keyboard.shift);
                    } else if compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 1;
                        taskmgr_app.focused_control = 1;
                    } else if compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        settings_app.select_tab_next();
                    } else if compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        browser_app.select_tab_next();
                    }
                }
                drivers::keyboard::KeyAction::EnterKey => {
                    if compositor.is_start_menu_open {
                        enter_char_consumed = true;
                        let active_g = compositor.active_start_group;
                        let (_, first, count) = gui::MENU_GROUPS[active_g];
                        let item_k = compositor.selected_start_item.min(count.saturating_sub(1));
                        let i = first + item_k;
                        if i == 11 {
                            if compositor.confirm_shutdown {
                                serial_print("[MinOS] System Shutdown confirmed via Keyboard!\n");
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
                            compositor.is_start_menu_open = false;
                        }
                    } else if compositor.is_context_menu_open {
                        enter_char_consumed = true;
                        compositor.is_context_menu_open = false;
                    } else if compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) && taskmgr_app.launch_menu_open {
                        enter_char_consumed = true;
                        let target = taskmgr_app.launch_app_id();
                        taskmgr_app.launch_menu_open = false;
                        open_app(&mut compositor, target);
                    } else if compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
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
                                        &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_app,
                                        &mut terminal_app);
                                }
                            }
                            _ => {}
                        }
                    } else if compositor.windows[0].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        enter_char_consumed = true;
                        match filemanager_app.focused_control {
                            0 => {
                                filemanager_app.selected_idx = 0;
                            }
                            1 | 2 => {
                                if let Some(selected_file) = filemanager_app.row(filemanager_app.selected_idx) {
                                    let target = match selected_file.icon_tag {
                                        "TXT" | "LOG" | "CFG" => 2,
                                        "IMG" => 4,
                                        "SND" => 5,
                                        "VID" => 6,
                                        "WEB" => 3,
                                        _ => 2,
                                    };
                                    open_app(&mut compositor, target);
                                }
                            }
                            _ => {}
                        }
                    } else if compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
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
                    } else if compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
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
                    } else if compositor.windows[8].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        enter_char_consumed = true;
                        calculator_app.press('=');
                    } else if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) && notepad_app.menu == 0 {
                        enter_char_consumed = true;
                        notepad_app.insert_char('\n');
                    } else if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        enter_char_consumed = true;
                        notepad_app.menu = 0;
                    }
                }
                drivers::keyboard::KeyAction::ShutdownOS => {
                    serial_print("[MinOS] System Shutdown requested!\n");
                    unsafe {
                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                        core::arch::asm!("cli; hlt");
                    }
                }
                drivers::keyboard::KeyAction::CopyText => {
                    if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) { notepad_app.copy_selection(); }
                }
                drivers::keyboard::KeyAction::PasteText => {
                    if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) { notepad_app.paste_clipboard(); }
                }
                drivers::keyboard::KeyAction::CutText => {
                    if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) { notepad_app.cut_selection(); }
                }
                drivers::keyboard::KeyAction::SelectAllText => {
                    let focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { notepad_app.select_all(); }
                }
                drivers::keyboard::KeyAction::Undo => {
                    let focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { notepad_app.undo(); }
                }
                drivers::keyboard::KeyAction::Redo => {
                    let focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { notepad_app.redo(); }
                }
                drivers::keyboard::KeyAction::HomeKey => {
                    let focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { notepad_app.move_home(); }
                }
                drivers::keyboard::KeyAction::EndKey => {
                    let focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { notepad_app.move_end(); }
                }
                drivers::keyboard::KeyAction::Delete => {
                    // Görev Yöneticisi odaklıysa seçili süreci sonlandır.
                    let tm_focused = compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if tm_focused {
                        if let Some(target) = taskmgr_app.selected_app_idx() {
                            taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_app,
                                &mut terminal_app);
                        }
                    } else if compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                        notepad_app.delete_forward();
                    }
                }
                drivers::keyboard::KeyAction::Refresh => { /* TODO */ }
                drivers::keyboard::KeyAction::BrowserAddress => {
                    let focused = compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                    if focused { browser_app.address_focused = true; browser_app.set_url(b""); }
                }
                drivers::keyboard::KeyAction::BrowserRefresh => {
                    let focused = compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
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
            if let Some(w) = compositor.windows[0] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    if scroll_val > 0 {
                        if filemanager_app.selected_idx > 0 { filemanager_app.selected_idx -= 1; }
                    } else if filemanager_app.selected_idx + 1 < filemanager_app.files.len() {
                        filemanager_app.selected_idx += 1;
                    }
                }
            }
            // CMD geçmişi: yalnızca çıktı panelini kaydır; giriş satırı sabit kalır.
            if let Some(w) = compositor.windows[10] {
                if w.is_open && w.is_focused && !w.is_minimized
                    && gui::in_rect(mouse.x as usize, mouse.y as usize, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h() + 28))) {
                    let view_lines = w.height.saturating_sub(86) / 13;
                    terminal_app.scroll_output(scroll_val as isize, view_lines);
                }
            }
            // Not Defteri (pencere 2): satır bazlı dikey kaydırma.
            if let Some(w) = compositor.windows[2] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(2, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 16; // 1 satır = 16px
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[2].as_mut() {
                        wm.scroll.y = new_off;
                    }
                }
            }
            // Dosya Yöneticisi listesi de kaydırılabilir (içerik sığmıyorsa).
            if let Some(w) = compositor.windows[0] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(0, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 24;
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[0].as_mut() {
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
            if let Some(w) = compositor.windows[9] {
                if w.is_focused {
                    // Windows Paint davranışı: sol tuş basılı tutuldukça fare
                    // hareketi sürekli çizer. Tuval dışına çıkınca vuruşu bitir.
                    if let Some((cxp, cyp)) = paint_app.to_canvas(w.width, w.height, mx - w.x, my - w.y) {
                        paint_app.paint(cxp, cyp);
                    } else {
                        paint_app.end_stroke();
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
                if let Some(w) = compositor.windows[2] {
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
                    w.x = mx.saturating_sub(drag_offset_x).min(max_x);
                    w.y = my.saturating_sub(drag_offset_y).min(max_y);
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

                    w.x = nx;
                    w.y = ny;
                    w.width = nw;
                    w.height = nh;
                    w.is_maximized = false;
                }
            }
        }

        if mouse_released {
            paint_app.end_stroke();
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
            compositor.is_alt_tab_open = false;
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
                compositor.is_start_menu_open = false;
            } else if ch == '\x1b' && compositor.is_context_menu_open {
                compositor.is_context_menu_open = false;
            } else if ch == '\x1b' && taskmgr_app.launch_menu_open {
                taskmgr_app.launch_menu_open = false;
            } else if ch == '\x1b' && notepad_app.menu != 0 {
                notepad_app.menu = 0;
            } else if compositor.is_start_menu_open && (ch >= '1' && ch <= '9') {
                let menu_index = (ch as usize) - ('1' as usize);
                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, app_idx);
                    compositor.is_start_menu_open = false;
                }
            } else if compositor.is_start_menu_open && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B') {
                let menu_index = if ch == 'a' || ch == 'A' { 9 } else { 10 };
                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, app_idx);
                    compositor.is_start_menu_open = false;
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
            } else if ch == '\t' && compositor.windows[10].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                terminal_app.complete();
            } else if ch == '\t' && compositor.windows[7].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                taskmgr_app.focus_next_control();
            } else if ch == '\t' && compositor.windows[1].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                settings_app.focus_next_control();
            } else if ch == '\t' && compositor.windows[3].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                browser_app.focus_next_control();
            } else if ch == '\t' && compositor.windows[0].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused) {
                filemanager_app.focus_next_control();
            } else {
                let browser_focused = compositor.windows[3]
                    .as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                if browser_focused && browser_app.address_focused {
                    if ch == '\x08' { browser_app.address_backspace(); }
                    else if ch == '\n' { browser_app.navigate(); }
                    else if ch >= ' ' { browser_app.address_char(ch); }
                }
                let notepad_focused = compositor.windows[2]
                    .as_ref()
                    .map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                if notepad_focused {
                    if ch == '\x08' {
                        notepad_app.backspace();
                    } else if ch == '\n' || ch >= ' ' {
                        notepad_app.insert_char(ch);
                    }
                }
                let calc_focused = compositor.windows[8].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                let term_focused = compositor.windows[10].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
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
                    if let Some(app_idx) = terminal_app.execute(line, &fs_mgr.vfs, &memory_mgr.pmm, &now) {
                        open_app(&mut compositor, app_idx);
                    }
                }
                let paint_focused = compositor.windows[9].as_ref().map_or(false, |w| w.is_focused);
                if paint_focused && (ch == 'c' || ch == 'C') { paint_app.clear(); }
            }
        }

        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;
        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;

        // Görsel animasyonlar için sınırlı kare hızı; RDTSC ile zamanlama.
        let timer_tick_fired = monotonic_ms().wrapping_sub(last_frame_ms) >= 33;
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
                    // 48kHz, 16-bit stereo: 1 beat = ~40ms = 1920 sample = 3840 byte
                    // Bir nota periyodu için basit sine dalgası üret (Taylor serisi sin).
                    let sample_count = 1920; // 40 ms
                    let mut pcm: [i16; 512] = [0; 512]; // 256 L + 256 R çifti
                    let volume_scale = (audio_dev.volume_left as i32).min(100) as i16;
                    let mut idx = 0;
                    while idx < sample_count {
                        let n = (sample_count - idx).min(pcm.len() / 2);
                        for k in 0..n {
                            let t = (idx + k) as f32 / 48_000.0;
                            let angle = 6.28318f32 * freq as f32 * t;
                            // Taylor serisi sin (±PI aralığında yeterli doğruluk)
                            let x = angle - 6.28318 * ((angle / 6.28318) as i32) as f32;
                            let x2 = x * x;
                            let v = x - x2 * x / 6.0 + x2 * x2 * x / 120.0 - x2 * x2 * x2 * x / 5040.0;
                            let sample = (v * 0.3 * volume_scale as f32 / 100.0 * 32767.0) as i16;
                            pcm[k * 2] = sample;
                            pcm[k * 2 + 1] = sample; // stereo aynı örnek
                        }
                        drivers::hda::push_pcm(&pcm[..n * 2]);
                        idx += n;
                    }
                } else {
                    // HDA yok: PC speaker fallback
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

        if input_occurred || mouse_moved || buttons_changed || timer_tick_fired || force_render {
            last_mouse_x = mouse.x;
            last_mouse_y = mouse.y;
            last_buttons = (mouse.left_button, mouse.right_button);

            taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);
            render_full_frame(
                &mut fb, &compositor, &filemanager_app, &settings_app,
                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_app, &terminal_app,
                &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick, &mut app_render_cycles,
            );
        }

        if !input_occurred && !mouse_moved && !buttons_changed && !force_render {
            // PIC/IRQ zamanlayıcısı kurulmadığı için HLT kullanılamaz. Kısa
            // kalibre edilmiş I/O beklemesi CPU'ya/QEMU'ya zaman bırakır;
            // giriş en geç 1 ms içinde yeniden yoklanır.
            let wait_start = read_cpu_cycles();
            let wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(1_000);
            while read_cpu_cycles().wrapping_sub(wait_start) < wait_cycles {
                unsafe { core::arch::asm!("in al, 0x80", options(nomem, nostack, preserves_flags)); }
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
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

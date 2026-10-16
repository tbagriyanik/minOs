#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::fmt::Write as FmtWrite;
use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;

#[allow(dead_code)]
const HEAP_SIZE: usize = 2 * 1024 * 1024;
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

// --- Limine Kernel File istegi: önyükleme komut satirini (cmdline) okur ---
// Boot menüsündeki "live" / "installed" seçimi bu cmdline ile kernel'e bildirilir.
#[repr(C, align(8))]
pub struct LimineKernelFileRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineKernelFileResponse,
}

#[repr(C, align(8))]
pub struct LimineKernelFileResponse {
    pub revision: u64,
    pub kernel_file: *const LimineFile,
}

/// `struct limine_file` başlangıcı. İlk alanlar (cmdline'a kadar) yeterlidir.
#[repr(C, align(8))]
pub struct LimineFile {
    pub revision: u64,
    pub address: *const u8,
    pub size: u64,
    pub path: *const u8,
    pub cmdline: *const u8,
}

unsafe impl Sync for LimineKernelFileRequest {}

#[used]
#[link_section = ".requests"]
static KERNEL_FILE_REQUEST: LimineKernelFileRequest = LimineKernelFileRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0xad97e90e83f1ed67,
        0x31eb5d1c5ff23b69,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Limine'in sağladığı önyükleme komut satirini (cmdline) döndürür.
/// `None` ise cmdline alınamadı demektir (bu durumda canlı sistem kabul edilir).
fn limine_boot_cmdline() -> Option<&'static str> {
    unsafe {
        let resp = valid_limine_response(core::ptr::addr_of!(KERNEL_FILE_REQUEST.response))?;
        let file = core::ptr::read_volatile(core::ptr::addr_of!((*resp).kernel_file));
        if !is_mapped_limine_ptr(file) {
            return None;
        }
        let cmdline = core::ptr::read_volatile(core::ptr::addr_of!((*file).cmdline));
        if !is_mapped_limine_ptr(cmdline) {
            return None;
        }
        // NUL sonlandırıcıya kadar (en fazla 256 bayt) oku.
        let mut len = 0usize;
        while len < 256 {
            let byte = core::ptr::read_volatile(cmdline.add(len));
            if byte == 0 {
                break;
            }
            len += 1;
        }
        core::str::from_utf8(core::slice::from_raw_parts(cmdline, len)).ok()
    }
}

/// Limine tarafından belleğe yüklenmiş çekirdek dosyasının ikili dilimini döndürür.
pub fn limine_kernel_binary_slice() -> Option<&'static [u8]> {
    unsafe {
        let resp = valid_limine_response(core::ptr::addr_of!(KERNEL_FILE_REQUEST.response))?;
        let file = core::ptr::read_volatile(core::ptr::addr_of!((*resp).kernel_file));
        if !is_mapped_limine_ptr(file) {
            return None;
        }
        let addr = core::ptr::read_volatile(core::ptr::addr_of!((*file).address));
        let size = core::ptr::read_volatile(core::ptr::addr_of!((*file).size)) as usize;
        if addr.is_null() || size == 0 {
            return None;
        }
        Some(core::slice::from_raw_parts(addr, size))
    }
}

/// Önyükleme komut satirinda verilen token'i (kelime) arar.
fn cmdline_has_token(cmdline: &str, token: &str) -> bool {
    cmdline
        .split(|c: char| c.is_whitespace())
        .any(|part| part == token)
}

/// Kernel'in kurulu sistem modunda baslatilmak istenip istenmedigini bildirir.
fn boot_wants_installed() -> bool {
    limine_boot_cmdline().map_or(false, |cmdline| cmdline_has_token(cmdline, "installed"))
}

#[inline(never)]
fn serial_print(s: &str) {
    serial::print(s);
}

fn save_boot_log(vfs: &mut fs::vfs::VirtualFileSystem) {
    let log = serial::log_snapshot();
    let _ = vfs.write_file("/root/Documents/boot.log", &log);
}

#[inline(never)]
fn serial_print_dec(v: u64) {
    if v == 0 { serial_print("0"); return; }
    let mut value = v;
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if let Ok(text) = core::str::from_utf8(&buf[i..]) { serial_print(text); }
}

#[inline(never)]
fn serial_print_hex(v: u64) {
    serial_print("0x");
    if v == 0 { serial_print("0"); return; }
    let mut value = v;
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    let hex = b"0123456789ABCDEF";
    while value > 0 {
        i -= 1;
        buf[i] = hex[(value & 0xF) as usize];
        value >>= 4;
    }
    if let Ok(text) = core::str::from_utf8(&buf[i..]) { serial_print(text); }
}


fn restart_system() -> ! {
    #[cfg(minos_boot_log)]
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

pub fn monotonic_ms() -> u64 {
    let cycles_per_us = unsafe { TSC_CYCLES_PER_US }.max(1);
    read_cpu_cycles() / cycles_per_us / 1_000
}

struct BusyProgressContext {
    fb: *mut drivers::video::Framebuffer,
    mx: usize,
    my: usize,
    message: &'static str,
    last_percent: usize,
    cursor_shown: bool,
}

static mut GLOBAL_MOUSE: drivers::mouse::MouseState = drivers::mouse::MouseState::new();
static mut GLOBAL_KEYBOARD: drivers::keyboard::KeyboardState = drivers::keyboard::KeyboardState::new();
static mut BUSY_DIRTY: bool = false;

/// İlerleme yüzdesini hesaplar; değer asla geri gitmez (monotonik).
fn busy_percent(state: &mut BusyProgressContext, done: usize, total: usize) -> usize {
    let mut p = if total == 0 { 30 } else { 5 + done.saturating_mul(90) / total };
    if p < state.last_percent { p = state.last_percent; }
    if p > 99 { p = 99; }
    state.last_percent = p;
    p
}

fn draw_io_progress(context: *mut (), done: usize, total: usize) {
    if context.is_null() { return; }
    let state = unsafe { &mut *(context as *mut BusyProgressContext) };

    // 1. Girişi pompa'la: uzun işlem sürerken fare imleci donmasın.
    unsafe {
        let w = (*state.fb).width as i32;
        let h = (*state.fb).height as i32;
        let mouse_ptr = core::ptr::addr_of_mut!(GLOBAL_MOUSE);
        let kbd_ptr = core::ptr::addr_of_mut!(GLOBAL_KEYBOARD);
        drivers::ps2::poll_input(&mut *mouse_ptr, &mut *kbd_ptr, w, h);
        state.mx = (*mouse_ptr).x.max(0) as usize;
        state.my = (*mouse_ptr).y.max(0) as usize;
    }
    let fb = unsafe { &mut *state.fb };

    // 2. Önceki kum saati imlecini temizle.
    if state.cursor_shown {
        if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
        state.cursor_shown = false;
    }

    // 3. Panel + ilerleme çubuğunu güncelle (tek, monotonik çubuk).
    let percent = busy_percent(state, done, total);
    gui::draw_busy_overlay(fb, state.mx, state.my, state.message, percent);

    // 4. Kum saati imlecini yeni konuma çiz ve ekranı güncelle.
    gui::draw_busy_hourglass(fb, state.mx, state.my);
    if let Some(rect) = fb.cursor_rect() {
        fb.flush_rect(rect);
    }
    state.cursor_shown = true;
}

fn with_busy_progress<T>(
    fb: &mut drivers::video::Framebuffer,
    mx: usize,
    my: usize,
    message: &'static str,
    operation: impl FnOnce() -> T,
) -> T {
    // Önceki karede çizilen ok imlecini kaldır (kum saati ile çakışmasın).
    if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
    gui::draw_busy_overlay(fb, mx, my, message, 5);
    gui::draw_busy_hourglass(fb, mx, my);

    // `fb` işaretçisi bağlama devredilir; bundan sonraki çizimler üzerinden yapılır.
    let mut state = BusyProgressContext { fb, mx, my, message, last_percent: 5, cursor_shown: true };
    drivers::ata::set_io_progress_hook(Some(draw_io_progress), &mut state as *mut _ as *mut ());
    let result = operation();
    drivers::ata::set_io_progress_hook(None, core::ptr::null_mut());

    // İşlem sonunda kum saatini temizle; ok imlecini ana döngü yeniden çizer.
    let fb = unsafe { &mut *state.fb };
    if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
    gui::draw_busy_overlay(fb, mx, my, message, 100);
    // Panel bir sonraki tam yeniden çizimde kalksın.
    unsafe { BUSY_DIRTY = true; }
    result
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
    let from_cpuid = ((result.ebx >> 16) & 0xFF).max(1);
    // SMP açılışı tamamlandıysa gerçek çevrimiçi CPU sayısını yansıt.
    let from_smp = arch::x86_64::smp::cpu_count() as u32;
    from_cpuid.max(from_smp).max(1)
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

/// Limine response pointers are expected in the mapped higher half. Reject
/// lower-half values so malformed boot data cannot direct these reads into an
/// unmapped region.
#[inline]
fn is_mapped_limine_ptr<T>(ptr: *const T) -> bool {
    is_canonical_ptr(ptr) && ((ptr as usize as u64) >> 47) == 0x1_FFFF
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
pub(crate) unsafe fn valid_limine_response<T>(slot: *const *const T) -> Option<*const T> {
    if slot.is_null() || !is_canonical_ptr(slot) {
        return None;
    }
    let ptr = core::ptr::read_volatile(slot);
    if !is_mapped_limine_ptr(ptr) { None } else { Some(ptr) }
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

fn desktop_item_rect(idx: usize) -> (usize, usize, usize, usize) {
    // Dosya ızgarası sol şerit hızlı simgelerin sağından başlar; iki grup
    // asla üst üste binmez (çizim + tıklama aynı kanonik konumu kullanır).
    let origin_x = gui::MinGuiCompositor::desktop_strip_right();
    let col = idx / 12;
    let row = idx % 12;
    (origin_x + col * 150, 30 + row * 54, 138, 46)
}

fn rects_overlap(a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Masaüstü simgesinin gerçekten çizildiği (görünür) alan mı?
/// Çizim döngüsü ile tıklama testi bu kuralı paylaşır: ekran dışına taşan,
/// görev çubuğuna gömülen veya sağ üst filigrana binen hücreler hem
/// çizilmez hem tıklanamaz.
fn desktop_item_visible(rect: (usize, usize, usize, usize), width: usize, height: usize) -> bool {
    if rect.0 + rect.2 > width || rect.1 + rect.3 >= height.saturating_sub(gui::taskbar_h()) {
        return false;
    }
    !rects_overlap(rect, gui::MinGuiCompositor::desktop_watermark_rect(width))
}

fn desktop_item_at(x: usize, y: usize, fm: &apps::filemanager::FileManagerApp, width: usize, height: usize) -> Option<(usize, (usize, usize, usize, usize))> {
    for i in 0..fm.file_count {
        let rect = desktop_item_rect(i);
        if !desktop_item_visible(rect, width, height) { continue; }
        if gui::in_rect(x, y, rect) { return Some((i, rect)); }
    }
    None
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

fn open_image_file(
    path: &str,
    vfs: &fs::vfs::VirtualFileSystem,
    _paints: &[Option<apps::paint::PaintApp>],
    viewer: &mut apps::imageviewer::ImageViewerApp,
) {
    // Large VFS entries intentionally have an empty inline `data` vector;
    // check the shared payload store before `read_file`, which can return that
    // empty slice for the metadata row.
    if let Some(data) = vfs.read_large_file(path) {
        if !viewer.open_bmp(data) { viewer.clear_image("cizim.bmp"); }
    } else if let Some(data) = vfs.read_file(path).filter(|data| !data.is_empty()) {
        let data: alloc::sync::Arc<[u8]> = alloc::sync::Arc::from(data);
        if !viewer.open_bmp(data) { viewer.clear_image("cizim.bmp"); }
    } else {
        viewer.clear_image(if path.ends_with("cizim.bmp") { "cizim.bmp" } else { "Image unavailable" });
    }
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
    installer: &mut apps::installer::InstallerApp,
    viewer3d: &mut apps::viewer3d::Viewer3DApp,
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
            let mut audio = drivers::audio::AudioDevice::new();
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
        11 => *installer = apps::installer::InstallerApp::new(),
        12 => *viewer3d = apps::viewer3d::Viewer3DApp::new(),
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

/// Defter imlecini her zaman görünür kılacak dikey kaydırmayı uygular.
fn notepad_ensure_cursor_visible(compositor: &mut gui::MinGuiCompositor, np: &apps::notepad::NotepadApp) {
    let Some((idx, w)) = focused_window_for_app(compositor, 2) else { return; };
    let desired = np.ensure_cursor_visible(w.width, w.height, w.scroll.y);
    if let Some(wm) = compositor.windows[idx].as_mut() {
        wm.scroll.y = desired;
    }
}

/// Dosya Yöneticisi listesinde seçili satırı görünür tutacak kaydırmayı uygular.
fn fm_ensure_selection_visible(compositor: &mut gui::MinGuiCompositor, fm: &apps::filemanager::FileManagerApp) {
    let Some((idx, w)) = focused_window_for_app(compositor, 0) else { return; };
    let view_h = w.height.saturating_sub(apps::filemanager::list_top() + 30);
    if view_h == 0 { return; }
    let row_h = apps::filemanager::FileManagerApp::row_h();
    let max_off = fm.list_content_h().saturating_sub(view_h);
    let row_top = fm.selected_idx.saturating_mul(row_h);
    let row_bottom = row_top + row_h;
    let mut sy = w.scroll.y.min(max_off);
    if row_top < sy {
        sy = row_top;
    } else if row_bottom > sy + view_h {
        sy = row_bottom - view_h;
    }
    if let Some(wm) = compositor.windows[idx].as_mut() {
        wm.scroll.y = sy.min(max_off);
    }
}

/// Dosya Yöneticisi listesinde bir sayfaya sığan satır sayısı (PageUp/PageDown).
fn fm_page_rows(w: &gui::Window) -> usize {
    let view_h = w.height.saturating_sub(apps::filemanager::list_top() + 30);
    (view_h / apps::filemanager::FileManagerApp::row_h()).max(1)
}

/// Terminal çıktı panelinde görünen satır sayısı (tekerlek ve sayfa gezinmesine ortak).
fn terminal_view_lines(w: &gui::Window) -> usize {
    (w.height.saturating_sub(86) / 13).max(1)
}

fn image_viewer_canvas_rect(w: &gui::Window) -> (usize, usize, usize, usize) {
    let inner_x = w.x + 10;
    let inner_w = w.width.saturating_sub(20).max(1);
    let content_y = w.y + 60;
    let status_y = w.y + w.height.saturating_sub(24);
    let content_h = status_y.saturating_sub(content_y + 6).max(1);
    if inner_w < 360 {
        (inner_x, content_y, inner_w, (content_h * 2 / 3).max(1))
    } else {
        let panel_w = (inner_w / 3).clamp(130, 220).min(inner_w.saturating_sub(13));
        (inner_x, content_y, inner_w.saturating_sub(panel_w + 12).max(1), content_h)
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
    // Seri konsolu ilk iş olarak hazırla: sonraki tüm loglar (PMM/VMM/SMP…)
    // 115200 8N1 ile çerçeveye oturur, VirtualBox yakalaması bozulmaz.
    serial::init();
    init_timebase();
    // Initialize heap for dynamic allocations
    init_heap();
    #[cfg(minos_boot_log)]
    serial_print("[minOs] Kernel _start reached!\n");
    #[cfg(minos_boot_log)]
    serial_print("[minOs] SSE enabled & PIC IRQs masked!\n");

    // HHDM ofsetini al (fiziksel MMIO ve sayfa tablosu sanal erisimi icin).
    let hhdm_offset = unsafe {
        // SAFETY: the response pointer is validated before being dereferenced.
        if let Some(resp) = valid_limine_response(core::ptr::addr_of!(HHDM_REQUEST.response))
            .filter(|resp| !resp.is_null())
        {
            let off = core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset));
            #[cfg(minos_boot_log)]
            serial_print("[minOs] HHDM offset okundu (0 yoksa gecerli).\n");
            off
        } else {
            #[cfg(minos_boot_log)]
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
            if count > 0 && is_mapped_limine_ptr(entries) && array_bytes.is_some() {
                let array_end = (entries as usize).checked_add(array_bytes.unwrap());
                if array_end.map_or(false, |end| is_mapped_limine_ptr(end as *const LimineMemoryMapEntry)) {
                for i in 0..count {
                    let slot_addr = match (entries as usize).checked_add(i * core::mem::size_of::<*const LimineMemoryMapEntry>()) {
                        Some(addr) => addr,
                        None => break,
                    };
                    let slot = slot_addr as *const *const LimineMemoryMapEntry;
                    if !is_mapped_limine_ptr(slot) { break; }
                    let entry_ptr = core::ptr::read_volatile(slot);
                    if !is_mapped_limine_ptr(entry_ptr) { continue; }
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

    // Test amaçlı standart ELF64 ikili dosyasını yükle ve doğrula
    let sample_elf_bin = mm::loader::AppLoader::create_sample_elf();
    if let Ok(loaded) = mm::loader::AppLoader::load_binary(&sample_elf_bin, &mut address_space, &mut memory_mgr.pmm) {
        if loaded.entry_point >= mm::loader::USER_CODE_BASE && loaded.stack_pointer > 0 {
            serial_print("[LOADER] Standart ELF64 ikilisi guvenli bellek izolasyonu ile yuklendi (W^X / NX aktif).\n");
        }
        loaded.unmap_and_free(&mut address_space, &mut memory_mgr.pmm);
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

    // Bellek tahsisi, sayfalama ve kullanıcı kipi izolasyon doğrulama testi
    mm::vmm::test_memory_and_isolation(&mut memory_mgr.pmm);

    // Süreç yaşam döngüsü, kaza, geçersiz syscall ve kaynak temizleme testi
    mm::process::test_process_lifecycle_and_fault_recovery(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);

    // 1b. SMP + Ring3: kendi GDT'miz (kullanıcı segmentleri + TSS), AP açılışı,
    // Ring3 izolasyon demosu ve paralel sağlama toplamı (checksum) doğrulaması.
    unsafe { arch::x86_64::gdt::load_for_cpu(0) };
    let cpu_total = arch::x86_64::smp::init();
    let cpu_online = arch::x86_64::smp::wait_for_aps(50_000_000);
    arch::x86_64::smp::print_locked("[SMP] CPU durumu: ");
    arch::x86_64::smp::print_locked_dec(cpu_online as u64);
    arch::x86_64::smp::print_locked("/");
    arch::x86_64::smp::print_locked_dec(cpu_total as u64);
    arch::x86_64::smp::print_locked(" cekirdek cevrimici.\n");
    for cpu in 1..cpu_total {
        let hb0 = arch::x86_64::smp::heartbeat(cpu);
        arch::x86_64::smp::print_locked("[SMP] AP");
        arch::x86_64::smp::print_locked_dec(cpu as u64);
        arch::x86_64::smp::print_locked(" kalp atisi: ");
        arch::x86_64::smp::print_locked_dec(hb0);
        arch::x86_64::smp::print_locked("\n");
    }
    if arch::x86_64::ring3::demo(&mut address_space, &mut memory_mgr.pmm) {
        arch::x86_64::smp::print_locked("[RING3] Kullanic kipi izolasyon demosu: TAMAM.\n");
    } else {
        arch::x86_64::smp::print_locked("[RING3] Kullanic kipi izolasyon demosu: BASARISIZ.\n");
    }

    // 1c. Donanım Kesmeleri ve APIC Zamanlayıcı (APIC Timer & Hardware Interrupts)
    arch::x86_64::apic::init_timer();
    arch::x86_64::smp::print_locked("[APIC] Local APIC Timer ve donanim kesme vektorleri (0x20) aktif.\n");
    // Paralel sağlama toplamı: AP'ler + BSP aynı tamponu parçalı işler.
    {
        const SUM_FRAMES: usize = 64;
        const U64_PER_FRAME: usize = 512;
        if let Some(buf_phys) = memory_mgr.pmm.allocate_contiguous_frames(SUM_FRAMES) {
            unsafe {
                let base = crate::drivers::phys_to_virt(buf_phys) as *mut u64;
                let mut expected: u64 = 0;
                for i in 0..SUM_FRAMES * U64_PER_FRAME {
                    let v = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678);
                    base.add(i).write_volatile(v);
                    expected = expected.wrapping_add(v);
                }
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                match arch::x86_64::smp::parallel_checksum(buf_phys, SUM_FRAMES * U64_PER_FRAME, 4 * U64_PER_FRAME) {
                    Some(got) if got == expected => {
                        serial_print("[SMP] Paralel saglama toplami (AP+BSP) eslesti: BASARILI\n");
                    }
                    Some(_) => {
                        serial_print("[SMP] Paralel saglama toplami UYUSMADI: BASARISIZ\n");
                    }
                    None => {
                        serial_print("[SMP] Paralel saglama toplami zaman asimi (tek cekirdek yedegi yok).\n");
                    }
                }
            }
            for i in 0..SUM_FRAMES {
                memory_mgr.pmm.free_frame(buf_phys + (i as u64) * 4096);
            }
        }
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

    // e1000 RX/TX Ring ve Tampon Havuzu: ~132 KB (~33 sayfa)
    let e1000_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(34);
    if e1000_dma_phys.is_some() {
        serial_print("[DMA] e1000 ag karti icin dinamik DMA ring havuzu tahsis edildi.\n");
    }
    drivers::e1000::init(e1000_dma_phys);

    drivers::netstack::init();
    let _ = drivers::netstack::dhcp_request();
    drivers::netstack::run_network_self_test();
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
            if framebuffer_count == 0 || !is_mapped_limine_ptr(fb_ptr_ptr) {
    serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\n");
                loop { core::hint::spin_loop(); }
            }
            // SAFETY: fb_ptr_ptr is non-null/aligned/canonical and points to the
            // first framebuffer entry of the response above.
            let fb_info_ptr = core::ptr::read_volatile(fb_ptr_ptr);
            if !is_mapped_limine_ptr(fb_info_ptr) {
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

    // 4b. Gelişmiş grafik sürücüleri: BGA/VMware/VGA yetenek taraması.
    let gpu_caps = drivers::gpu::init();
    if gpu_caps.bga_present {
        serial_print("[GPU] BGA algilandi, cozunurluk degisimi acik.\n");
        // BGA grafik modda HW sprite yoktur; yazılım imleci çizilmeye devam
        // edilir (görünürlük için şart). CRTC'ye konum bildirimi sürer.
        fb.set_hw_cursor(false);
        serial_print("[GPU] Hizlandirilmis yazilim imleci etkin.\n");
    } else {
        serial_print("[GPU] BGA yok; cozunurluk Limine ile sabit, yazilim imleci.\n");
    }

    // 5. PS/2 Donanım Sürücüleri
    let mouse = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_MOUSE) };
    let keyboard = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_KEYBOARD) };
    drivers::ps2::init();
    serial_print("[BOOT] PS/2 hazir.\n");

    // 5b. Tema motorunu başlat
    gui::theme::init();
    serial_print("[BOOT] Tema hazir.\n");

    // 5c. Önyükleme modu denetimi: Boot menüsünden "installed" seçildiyse ve
    // diskte kurulu bir minOs (MINOS.SYS) yoksa canlı sisteme düşmeden hata
    // verilir ve sistem bu ekranda bekler.
    // (Disk taraması tek yapılır; sonucu loga yazılır.)
    let wants_installed = boot_wants_installed();
    let installed = fs::diskfs::is_installed();
    serial_print("[BOOT] Kurulum denetimi: wants_installed=");
    serial_print(if wants_installed { "1" } else { "0" });
    serial_print(" installed=");
    serial_print(if installed { "1" } else { "0" });
    serial_print("\n");
    if wants_installed && !installed {
        serial_print("[BOOT] 'installed' modu istendi ancak diskte kurulum bulunamadi.\n");
        gui::render_boot_error(&mut fb, width, height);
        loop {
            core::hint::spin_loop();
        }
    }

    // 5d. Eğer diskte varsa (MINOS.SYS + fat32) — kurulu sistem —
    // welcome (hoş geldin) ekranını atlayıp doğrudan masaüstüye geç.
    // Live ortam ise welcome ekranı gösterilir.
    if installed {
        serial_print("[BOOT] Kurulu sistem tespit edildi; masaustu hazirlaniyor...\n");
        for phase in 0..4 {
            gui::render_loading_transition(&mut fb, width, height, phase,
                gui::ui_text("Masaustu hazirlaniyor", "Preparing desktop"));
            let frame_start = monotonic_ms();
            while monotonic_ms().saturating_sub(frame_start) < 25 { core::hint::spin_loop(); }
        }
    } else {
        // İlk açılış geçişi: merkezde minOs + hareketli kedi ve yükleme çubuğu.
        for phase in 0..8 {
            gui::render_loading_transition(&mut fb, width, height, phase,
                gui::ui_text("Sistem yukleniyor", "Loading system"));
            let frame_start = monotonic_ms();
            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }
        }
    }

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar
    let mut compositor = gui::MinGuiCompositor::new();
    let mut filemanager_app = apps::filemanager::FileManagerApp::new();
    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
    let mut desktop_files = apps::filemanager::FileManagerApp::new();
    desktop_files.set_path("/desktop");
    desktop_files.refresh_from_vfs(&fs_mgr.vfs);
    let mut desktop_drag_source = false;
    let mut file_clipboard = apps::filemanager::FileClipboard::new();
    let mut settings_app = apps::settings::SystemSettings::default();
    settings_app.display_res = (width, height);
    // Disk üzerinde önceden kaydedilmiş kalıcı ayarlar var mı kontrol et
    if fs::diskfs::load_file("settings.cfg") > 0 {
        let cfg_bytes = fs::diskfs::content();
        let _ = fs_mgr.vfs.write_file("settings.cfg", cfg_bytes);
    }
    settings_app.load_from_vfs(&fs_mgr.vfs);
    mouse.speed = settings_app.mouse_speed;
    mouse.reverse_scroll = settings_app.reverse_scroll;
    keyboard.layout = settings_app.keyboard_layout;
    gui::theme::set_theme(settings_app.theme);
    let mut notepad_app = apps::notepad::NotepadApp::new();
    let mut browser_app = apps::browser::BrowserApp::new();
    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    let mut taskmgr_app = apps::taskmgr::TaskManagerApp::new();
    let mut calculator_app = apps::calculator::CalculatorApp::new();
    // Başlangıçta yalnızca sabit uygulama pencereleri için state oluştur.
    // MAX_WINDOWS kadar 4096 noktalı tuval ayırmak 1 MiB kernel heap'ini aşar.
    let mut paint_apps: alloc::vec::Vec<Option<apps::paint::PaintApp>> = (0..gui::MAX_WINDOWS).map(|_| None).collect();
    let mut terminal_app = apps::terminal::TerminalApp::new();
    terminal_app.init_banner();
    let mut installer_app = apps::installer::InstallerApp::new();
    installer_app.refresh_drives();
    let mut viewer3d_app = apps::viewer3d::Viewer3DApp::new();

    // 12 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimliği).
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
    // 11: minOs Tek Tıkla Kur Sihirbazı
    let inst_w = 540.min(width.saturating_sub(20)).max(200);
    let inst_h = 460.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "minOs Tek Tıkla Kur", "Kurulum",
        width.saturating_sub(inst_w) / 2,
        height.saturating_sub(inst_h + gui::taskbar_h()) / 2,
        inst_w, inst_h, false,
    ));
    // 12: 3D Gösterici
    let v3d_w = 600.min(width.saturating_sub(20)).max(200);
    let v3d_h = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);
    compositor.add_window(gui::Window::new(
        "3D Gösterici", "3D",
        width.saturating_sub(v3d_w) / 2,
        height.saturating_sub(v3d_h + gui::taskbar_h()) / 2,
        v3d_w, v3d_h, false,
    ));

    // Açılışta hiçbir pencere odaklı değildir (temiz masaüstü).
    compositor.active_window_idx = None;

    // Kurulu sistemse welcome ekranını atla, live ortam ise göster.
    serial_print("[BOOT] Gecis animasyonu bitti; arayuz kuruluyor.\n");
    if !installed {
        // Her açılışta dil ve masaüstü teması seçilebilen hoş geldin ekranı.
        let mut welcome_was_left_down = false;
        let mut welcome_dirty = true;
        let mut welcome_mouse_pos = (mouse.x, mouse.y);
        let mut welcome_accepted = false;
        let mut welcome_announced = false;
        let mut welcome_diag_last = monotonic_ms();
        let mut welcome_diag_iters: u64 = 0;
        let mut welcome_diag_last_trace: u64 = 0;
        loop {
            if !welcome_announced {
                serial_print("[BOOT] Hosgeldin dongusu: Enter/ESC/tik bekleniyor.\n");
                welcome_announced = true;
            }
            if welcome_dirty {
                gui::render_welcome_screen(&mut fb, width, height, settings_app.language.is_english(), settings_app.theme, &mouse);
                welcome_dirty = false;
            }
            let welcome_update = drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);
            if welcome_update.keyboard || welcome_update.mouse {
                serial_print("[BOOT-DIAG] PS/2 bayti isleniyor.\n");
            }
            let (tr_st, tr_d, tr_n) = drivers::ps2::trace_last();
            if tr_n != welcome_diag_last_trace {
                welcome_diag_last_trace = tr_n;
                serial_print("[BOOT-DIAG] ham bayt #");
                serial_print_dec(tr_n);
                serial_print(" st=0x");
                serial_print_hex(tr_st as u64);
                serial_print(" d=0x");
                serial_print_hex(tr_d as u64);
                serial_print("\n");
            }
            welcome_diag_iters += 1;
            let welcome_now = monotonic_ms();
            if welcome_now.wrapping_sub(welcome_diag_last) >= 5000 {
                welcome_diag_last = welcome_now;
                serial_print("[BOOT-DIAG] dongu sag (iter=");
                serial_print_dec(welcome_diag_iters);
                serial_print(" fare=");
                serial_print_dec(mouse.x.max(0) as u64);
                serial_print(",");
                serial_print_dec(mouse.y.max(0) as u64);
                serial_print(" ps2st=0x");
                serial_print_hex(drivers::ps2::status_byte() as u64);
                serial_print(")\n");
            }

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

            if !welcome_update.mouse && !welcome_update.keyboard {
                for _ in 0..1_000 {
                    core::hint::spin_loop();
                }
            }
        }
        serial_print("[BOOT] Hosgeldin kabul edildi; masaustune geciliyor.\n");
    }

    // Welcome ekranından masaüstüne geçişi de aynı animasyonla yumuşat.
    // Sadece live ortamda göster; kurulu sistemde atla.
    if !installed {
        for phase in 0..6 {
            gui::render_loading_transition(&mut fb, width, height, phase + 2, gui::ui_text("Masaustu hazirlaniyor", "Preparing desktop"));
            let frame_start = monotonic_ms();
            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }
        }
    }

    // RTC - gerçek zamanlı saat
    let mut now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());
    compositor.cal_year = now.year;
    compositor.cal_month = now.month;
    #[cfg(minos_boot_log)]
    serial_print("[minOs] RTC clock online.\n");
    // Save once the boot transcript is complete, including numeric fields.
    save_boot_log(&mut fs_mgr.vfs);

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
    let mut image_drag_last: Option<(usize, usize)> = None;


    let mut app_render_cycles = [0u64; gui::MAX_WINDOWS];
    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);
    let render_full_frame = |fb: &mut drivers::video::Framebuffer,
                             compositor: &gui::MinGuiCompositor,
                             fm: &apps::filemanager::FileManagerApp,
                             desktop_fm: &apps::filemanager::FileManagerApp,
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
                             inst: &apps::installer::InstallerApp,
                             v3d: &apps::viewer3d::Viewer3DApp,
                             now: &drivers::rtc::DateTime,
                             mouse: &drivers::mouse::MouseState,
                             kbd: &drivers::keyboard::KeyboardState,
                             total_ram_bytes: u64,
                             tick: usize,
                             app_render_cycles: &mut [u64; gui::MAX_WINDOWS],
                             dirty_rect: Option<(usize, usize, usize, usize)>| {
        // 1. Masaüstü Arka Planı
        compositor.render_desktop_bg(fb);
        // /desktop içeriğini masaüstü simgeleri olarak çiz.
        for i in 0..desktop_fm.file_count {
            let item = &desktop_fm.files[i];
            let (x, y, w, h) = desktop_item_rect(i);
            if !desktop_item_visible((x, y, w, h), fb.width, fb.height) { continue; }
            if desktop_fm.is_row_selected(i) { fb.draw_rounded_rect(x, y, w, h, 5, gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28)); }
            if item.is_dir { apps::filemanager::FileManagerApp::draw_folder_icon(fb, x + 5, y + 5, false); }
            else { apps::filemanager::FileManagerApp::draw_file_icon(fb, x + 5, y + 5, item.icon_tag); }
            gui::MinGuiCompositor::draw_string_clipped(fb, x + 30, y + 10, item.display_name(), gui::Color::TEXT_MAIN, (x + 30, y + 2, w - 34, h - 4));
        }

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
                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll, kbd.ctrl),
                    1 => st.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout, now, total_ram_bytes,
                        cpu_vendor(), cpu_logical_count(),
                        if drivers::hda::is_ready() { if drivers::hda::is_dma_progressing() { gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)") } else { gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)") } } else { gui::ui_text("PC Speaker (PWM fallback)", "PC Speaker (PWM fallback)") },
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
                    11 => inst.render_content(fb, win.x, win.y, win.width, win.height),
                    12 => v3d.render_content(fb, win.x, win.y, win.width, win.height),
                    _ => {}
                }
                app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);

                // Kırpmayı kaldır ve kaydırma çubuklarını pencerenin içinde çiz.
                fb.reset_clip();
                let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;
                if want_v {
                    let v_h = ch.saturating_sub(if want_h { sb } else { 0 });
                    if v_h > 0 {
                        // Sağa tam dayalı dikey kaydırma çubuğu
                        let track = (cx + cw - sb, cy, sb, v_h);
                        let g = crate::gui::scrollbar::v_geom(track, content_h, v_h, win.scroll.y);
                        crate::gui::scrollbar::draw_vertical(fb, &g);
                    }
                }
                if want_h {
                    let h_w = cw.saturating_sub(if want_v { sb } else { 0 });
                    if h_w > 0 {
                        // Alta tam dayalı yatay kaydırma çubuğu
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

        if fm.drag_active {
            fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);
        } else if desktop_fm.drag_active {
            desktop_fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);
        }

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
        &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,
        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app, &installer_app, &viewer3d_app, &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick,
        &mut app_render_cycles,
        None,
    );

    let mut drag_prev_box: Option<(usize, usize, usize, usize)> = None;
    let mut drag_dirty_rect: Option<(usize, usize, usize, usize)> = None;

    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü
    serial_print("[BOOT] Masaustu dongusu basladi.\n");
    loop {
        let loop_start_cycles = read_cpu_cycles();
        mouse.speed = settings_app.mouse_speed;
        mouse.reverse_scroll = settings_app.reverse_scroll;
        let input_update = drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);
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
                    } else if idx == 7 {
                        // Görev Yöneticisi → süreç satırı üzerinde mi?
                        if taskmgr_app.active_tab == 0 {
                            if let Some(row) = taskmgr_app.row_at(mx, my, win.x, win.y, win.width) {
                                taskmgr_app.selected = row;
                                taskmgr_app.focused_control = 4 + row;
                                kind = gui::ContextKind::TaskManager;
                            } else {
                                kind = gui::ContextKind::Window;
                            }
                        } else {
                            kind = gui::ContextKind::Window;
                        }
                        compositor.focus_window(idx);
                    } else if my < tb_y {
                        let selected = desktop_files.selected_items;
                        let click_idx = desktop_files.last_click_idx;
                        let click_ms = desktop_files.last_click_ms;
                        desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                        desktop_files.selected_items = selected;
                        desktop_files.last_click_idx = click_idx;
                        desktop_files.last_click_ms = click_ms;
                        if let Some((desk_idx, _)) = desktop_item_at(mx, my, &desktop_files, width, height) {
                            compositor.context_file_idx = Some(desk_idx);
                            desktop_files.select_row(desk_idx, keyboard.ctrl, keyboard.shift);
                            kind = gui::ContextKind::DesktopItem;
                        } else {
                            kind = gui::ContextKind::Desktop;
                        }
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
                if notepad_app.menu != 0 {
                    let in_menu_box = gui::in_rect(
                        mx, my,
                        (
                            w.x + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu),
                            w.y + crate::apps::notepad::NotepadApp::menu_box_y(),
                            crate::apps::notepad::NotepadApp::menu_box_w(),
                            crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu),
                        ),
                    );
                    let in_tabs = my >= w.y + 30 && my < w.y + 30 + crate::gui::sp(24);
                    if !in_menu_box && !in_tabs {
                        // Menü açıkken dışarı tıklandı -> menüyü kapat, alttaki metni tıklama
                        notepad_app.menu = 0;
                        notepad_dragging = false;
                        consumed = true;
                    }
                    // Menü kutusu veya sekmeler tıklanmışsa pencere olay işleyicisi işleyecek
                } else if w.is_focused
                    && gui::in_rect(mx, my, (w.x + 6, w.y + 58, w.width.saturating_sub(12), w.height.saturating_sub(86)))
                {
                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);
                    // Çift tık algılama: aynı konuma kısa sürede ikinci tık.
                    let now_ms = monotonic_ms() as usize;
                    let dbl = now_ms.abs_diff(notepad_app.last_click_tick) <= settings_app.double_click_ms as usize
                        && notepad_app.last_click_pos.0.abs_diff(mx) < 4
                        && notepad_app.last_click_pos.1.abs_diff(my) < 4;
                    if dbl {
                        notepad_app.select_word_at(at);
                    } else {
                        notepad_app.cursor = at;
                        if keyboard.shift {
                            let anchor = notepad_app.selection_anchor.unwrap_or(at);
                            if anchor == at {
                                notepad_app.selection_start = None;
                                notepad_app.selection_end = None;
                            } else {
                                notepad_app.selection_start = Some(anchor.min(at));
                                notepad_app.selection_end = Some(anchor.max(at));
                            }
                        } else {
                            notepad_app.selection_anchor = Some(at);
                            notepad_app.selection_start = None;
                            notepad_app.selection_end = None;
                        }
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
                            0 => { filemanager_app.set_path("/root/Documents"); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0); }
                            1 => open_app(&mut compositor, 2),
                            2 => { desktop_files.new_file(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }
                            3 => { desktop_files.new_folder(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }
                            4 => { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }
                            5 => open_app(&mut compositor, 1),
                            6 => open_app(&mut compositor, 7),
                            7 => settings_app.cycle_theme(),
                            8 => { desktop_files.refresh_from_vfs(&fs_mgr.vfs); }
                            _ => {}
                        },
                        gui::ContextKind::DesktopItem => {
                            match clicked_item {
                                0 => if let Some(i) = compositor.context_file_idx {
                                    let mut path = alloc::string::String::from("/desktop/"); path.push_str(desktop_files.files[i].display_name());
                                    if desktop_files.files[i].is_dir { filemanager_app.set_path(&path); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0); }
                            else {
                                let tag = desktop_files.files[i].icon_tag;
                                if tag == "TXT" || tag == "LOG" || tag == "CFG" { if let Some(data) = fs_mgr.vfs.read_file(&path) { notepad_app.open_file(&path, data); open_app(&mut compositor, 2); } }
                                else if tag == "IMG" { open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app); open_app(&mut compositor, 4); }
                                else if tag == "SND" { open_app(&mut compositor, 5); }
                                else if tag == "VID" { open_app(&mut compositor, 6); }
                                else if tag == "WEB" { open_app(&mut compositor, 3); }
                                else if tag == "3D" { if let Some(data) = fs_mgr.vfs.read_file(&path) { viewer3d_app.load_file(&path, data); open_app(&mut compositor, 12); } }
                            }
                                },
                                1 => if let Some(i) = compositor.context_file_idx { let mut path = alloc::string::String::from("/desktop/"); path.push_str(desktop_files.files[i].display_name()); if let Some(data) = fs_mgr.vfs.read_file(&path) { notepad_app.open_file(&path, data); open_app(&mut compositor, 2); } },
                                2 => desktop_files.copy_selection(&mut file_clipboard, true),
                                3 => desktop_files.copy_selection(&mut file_clipboard, false),
                                4 => { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },
                                5 => { desktop_files.new_file(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },
                                6 => { desktop_files.new_folder(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },
                                7 => desktop_files.begin_rename(),
                                8 => { desktop_files.delete_selected(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },
                                _ => {}
                            }
                        }
                        gui::ContextKind::FileManager => {
                            match clicked_item {
                                0 => { // Aç / Klasöre Gir
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                            if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                                if let Some(data) = data {
                                                    notepad_app.open_file(&path, data);
                                                }
                                                open_app(&mut compositor, 2);
                                            } else if tag == "IMG" {
                                                with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim aciliyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));
                                                open_app(&mut compositor, 4);
                                            } else if tag == "SND" {
                                                open_app(&mut compositor, 5);
                                            } else if tag == "VID" {
                                                open_app(&mut compositor, 6);
                                            } else if tag == "WEB" {
                                                open_app(&mut compositor, 3);
                                            } else if tag == "3D" {
                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model aciliyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));
                                                if let Some(data) = data {
                                                    viewer3d_app.load_file(&path, data);
                                                }
                                                open_app(&mut compositor, 12);
                                            }
                                        }
                                    }
                                }
                                1 => { // Not Defterinde Düzenle
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        let path = filemanager_app.selected_full_path();
                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                        if let Some(data) = data {
                                            notepad_app.open_file(&path, data);
                                        }
                                        open_app(&mut compositor, 2);
                                    }
                                }
                                2 => { filemanager_app.copy_selection(&mut file_clipboard, true); }
                                3 => { filemanager_app.copy_selection(&mut file_clipboard, false); }
                                9 => { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }
                                5 => { // Yeni Dosya Oluştur
                                    filemanager_app.new_file(&mut fs_mgr.vfs);
                                }
                                6 => { // Yeni Klasör Oluştur
                                    filemanager_app.new_folder(&mut fs_mgr.vfs);
                                }
                                8 => { // Dosyayı / Klasörü Sil
                                    if let Some(file_idx) = compositor.context_file_idx {
                                        filemanager_app.selected_idx = file_idx;
                                        filemanager_app.delete_selected(&mut fs_mgr.vfs);
                                    }
                                }
                                7 => { // Adını değiştir
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
                        gui::ContextKind::TaskManager => match clicked_item {
                            0 => {
                                // Sonlandır (Del): seçili süreci kapat
                                if let Some(target) = taskmgr_app.selected_app_idx() {
                                    taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                                    if let Some(wi) = compositor.context_window {
                                        terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[wi],
                                            &mut terminal_app, &mut installer_app, &mut viewer3d_app);
                                    }
                                }
                            }
                            _ => {}
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
                let mut matches = [0; 13];
                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);
                sub_rect = gui::MinGuiCompositor::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));
            }
            if compositor.start_search_len > 0 && gui::in_rect(mx, my, sub_rect) {
                let mut matches = [0; 13];
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
                        if i == 13 {
                            if compositor.confirm_shutdown {
                                #[cfg(minos_boot_log)]
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
                        if i == 14 {
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
                } else if l.volume.2 > 0 && gui::in_rect(mx, my, l.volume) {
                    compositor.is_volume_popup_open = !compositor.is_volume_popup_open;
                    consumed = true;
                } else if l.network.2 > 0 && gui::in_rect(mx, my, l.network) {
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
            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {
                // Yenileme tıklama durumunu sıfırlar; çift tık tespiti için koru
                // (yoksa her tıklama durumu siler ve çift tıklama hiç algılanamaz).
                let saved_click_idx = desktop_files.last_click_idx;
                let saved_click_ms = desktop_files.last_click_ms;
                desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                desktop_files.last_click_idx = saved_click_idx;
                desktop_files.last_click_ms = saved_click_ms;
                if let Some((i, _)) = desktop_item_at(mx, my, &desktop_files, width, height) {
                    desktop_files.select_row(i, keyboard.ctrl, keyboard.shift);
                    compositor.active_window_idx = None;
                    let now_ms = monotonic_ms();
                    // Masaüstünün kendi tıklama durumu kullanılır (dosya yöneticisi
                    // penceresinin durumu değil); aksi halde çift tık hiç tutmaz.
                    let is_double = desktop_files.last_click_idx == Some(i)
                        && now_ms.wrapping_sub(desktop_files.last_click_ms) <= settings_app.double_click_ms as u64;
                    if is_double {
                        desktop_files.last_click_idx = None;
                        desktop_files.drag_item = None;
                        let mut p = alloc::string::String::from("/desktop/");
                        p.push_str(desktop_files.files[i].display_name());
                        if desktop_files.files[i].is_dir {
                            filemanager_app.set_path(&p); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0);
                        } else {
                            let tag = desktop_files.files[i].icon_tag;
                            if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&p));
                                if let Some(data) = data { notepad_app.open_file(&p, data); }
                                open_app(&mut compositor, 2);
                            } else if tag == "IMG" {
                                with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim aciliyor...", "Opening image..."), || open_image_file(&p, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));
                                open_app(&mut compositor, 4);
                            }
                            else if tag == "SND" { open_app(&mut compositor, 5); }
                            else if tag == "VID" { open_app(&mut compositor, 6); }
                            else if tag == "WEB" { open_app(&mut compositor, 3); }
                            else if tag == "3D" {
                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model aciliyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&p));
                                if let Some(data) = data {
                                    viewer3d_app.load_file(&p, data);
                                }
                                open_app(&mut compositor, 12);
                            }
                        }
                    } else {
                        desktop_files.last_click_idx = Some(i);
                        desktop_files.last_click_ms = now_ms;
                        desktop_files.drag_item = Some(i);
                        desktop_files.drag_start = (mx, my);
                        desktop_drag_source = true;
                    }
                    consumed = true;
                }
            }
            // Masaüstü sol şerit hızlı simgeleri (çizimle aynı kanonik geometri).
            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {
                let desk_apps = [0, 2, 3, 5, 1, 11];
                // Kurulu sistemde kaldırılan kurulum kısayolunu (app 11) atla.
                let desk_icon_count = gui::MinGuiCompositor::desktop_quick_count();
                for (i, &app_idx) in desk_apps.iter().enumerate().take(desk_icon_count) {
                    if gui::in_rect(mx, my, gui::MinGuiCompositor::desktop_quick_rect(i)) {
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
                            &mut terminal_app, &mut installer_app, &mut viewer3d_app);
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
                    // Dikey kaydırma çubuğu (pencere sağ kenarı) — tıklama/sürükleme.
                    {
                        let (ccx, ccy, ccw, cch) = win.content_rect();
                        let c_h = window_content_h(win.app_id as usize, wh, ww, &filemanager_app, &notepad_app);
                        let view_h = cch;
                        if c_h > view_h {
                            let t = gui::scrollbar::SCROLLBAR_THICKNESS;
                            // Sağa tam dayalı dikey scrollbar track'i
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
                    // Pencere kenar ve köşelerinden boyutlandırma. Scrollbar
                    // alanı yukarıda tüketildi; geri kalan kenar alanı resize'a ait.
                    if !win.is_maximized {
                        if let Some(edge) = win.resize_edge_at(mx, my) {
                            compositor.focus_window(idx);
                            resizing_win = Some(idx);
                            resize_edge = Some(edge);
                            resize_orig = (wx, wy, ww, wh);
                            break;
                        }
                    }
                    // Başlık çubuğu — sürükle / çift tık ile maksimize
                    if gui::in_rect(mx, my, win.title_drag_rect()) {
                        let now_ms = monotonic_ms();
                        let dbl = last_title_idx == Some(idx)
                            && now_ms.wrapping_sub(last_title_click_ms) <= settings_app.double_click_ms as u64;
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
                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                            if let Some(data) = data {
                                                notepad_app.open_file(&path, data);
                                            }
                                            open_app(&mut compositor, 2);
                                        } else if tag == "IMG" {
                                            with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim aciliyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));
                                            open_app(&mut compositor, 4);
                                        } else if tag == "SND" {
                                            open_app(&mut compositor, 5);
                                        } else if tag == "VID" {
                                            open_app(&mut compositor, 6);
                                        } else if tag == "WEB" {
                                            open_app(&mut compositor, 3);
                                        } else if tag == "3D" {
                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model aciliyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));
                                            if let Some(data) = data {
                                                viewer3d_app.load_file(&path, data);
                                            }
                                            open_app(&mut compositor, 12);
                                        }
                                    }
                                } else if mx >= wx + 6 && mx < wx + 116 && my >= wy + 66 && my < wy + 66 + 10 * 22 {
                                    let nav = (my.saturating_sub(wy + 66)) / 22;
                                    let target_path = match nav {
                                        0 => "/",
                                        1 => "/desktop",
                                        2 => "/root/Documents",
                                        3 => "/root/Media",
                                        4 => "/root/Downloads",
                                        5 => "/boot",
                                        6 => "/media/usb0",
                                        7 => "/media/hdd0",
                                        8 => "/media/cdrom",
                                        _ => "/root/Trash",
                                    };
                                    filemanager_app.set_path(target_path);
                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 {
                                    let sy = compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0);
                                    let list_top = wy + apps::filemanager::LIST_TOP;
                                    let list_bottom = wy + wh.saturating_sub(30);
                                    for i in 0..filemanager_app.file_count {
                                        if my >= list_top && my < list_bottom && gui::in_rect(mx, my, apps::filemanager::FileManagerApp::row_rect_scrolled(wx, wy, ww, i, sy)) {
                                            filemanager_app.select_row(i, keyboard.ctrl, keyboard.shift);
                                            let now_ms = monotonic_ms();
                                            let is_double_click = filemanager_app.last_click_idx == Some(i)
                                                && now_ms.wrapping_sub(filemanager_app.last_click_ms) <= settings_app.double_click_ms as u64;
                                            if is_double_click {
                                                filemanager_app.last_click_idx = None;
                                                filemanager_app.drag_item = None;
                                                filemanager_app.drag_active = false;
                                                filemanager_app.drag_target = None;
                                                if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {
                                                    if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                                        if let Some(data) = data {
                                                            notepad_app.open_file(&path, data);
                                                        }
                                                        open_app(&mut compositor, 2);
                                                    } else if tag == "IMG" {
                                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim aciliyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));
                                                        open_app(&mut compositor, 4);
                                                    } else if tag == "SND" {
                                                        open_app(&mut compositor, 5);
                                                    } else if tag == "VID" {
                                                        open_app(&mut compositor, 6);
                                                    } else if tag == "WEB" {
                                                        open_app(&mut compositor, 3);
                                                    } else if tag == "3D" {
                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model aciliyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));
                                                        if let Some(data) = data {
                                                            viewer3d_app.load_file(&path, data);
                                                        }
                                                        open_app(&mut compositor, 12);
                                                    }
                                                }
                                            } else {
                                                filemanager_app.last_click_idx = Some(i);
                                                filemanager_app.last_click_ms = now_ms;
                                                if filemanager_app.files[i].display_name() != ".." {
                                                    filemanager_app.drag_item = Some(i);
                                                    filemanager_app.drag_start = (mx, my);
                                                    desktop_drag_source = false;
                                                    filemanager_app.drag_active = false;
                                                    filemanager_app.drag_target = None;
                                                }
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                            1 => {
                                // Sistem Ayarları
                                for t in 0..5 {
                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::tab_rect(wx, wy, t)) {
                                        settings_app.active_tab = t;
                                    }
                                }
                                if gui::in_rect(mx, my, apps::settings::SystemSettings::save_btn_rect(wx, wy, wh)) {
                                    settings_app.save_all(&compositor, &mut fs_mgr.vfs);
                                } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh)) {
                                    settings_app.reset_defaults();
                                    settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now);
                                    settings_app.write_config(&mut fs_mgr.vfs);
                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::dhcp_btn_rect(wx, wy)) {
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
                                            } else if drivers::gpu::mode_supported(requested.0, requested.1) {
                                                let (switched, pitch) = drivers::gpu::try_set_resolution(requested.0, requested.1);
                                                if switched {
                                                    let p = pitch.unwrap_or(requested.0 * 4);
                                                    if fb.resize(requested.0, requested.1, p) {
                                                        settings_app.screen_res = requested;
                                                        settings_app.display_res = requested;
                                                        settings_app.status_msg = gui::ui_text("Çözünürlük değiştirildi (BGA).", "Resolution changed (BGA).");
                                                    } else {
                                                        settings_app.status_msg = gui::ui_text("Çözünürlük uygulanamadı (bellek sınırı).", "Resolution failed (memory limit).");
                                                    }
                                                } else {
                                                    settings_app.screen_res = (fb.width, fb.height);
                                                    settings_app.status_msg = gui::ui_text("Çözünürlük açılışta Limine tarafından belirlenir.", "Resolution is set by Limine at boot.");
                                                }
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
                                if settings_app.active_tab == 0 {
                                    let speed_track = apps::settings::SystemSettings::mouse_speed_track_rect(wx, wy);
                                    let mut mouse_setting_changed = true;
                                    if gui::in_rect(mx, my, speed_track) {
                                        let span = speed_track.2.saturating_sub(1).max(1);
                                        settings_app.mouse_speed = (1 + (mx.saturating_sub(speed_track.0).min(span) * 9 / span)) as u8;
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::double_click_dec_rect(wx, wy)) {
                                        settings_app.double_click_ms = settings_app.double_click_ms.saturating_sub(50).max(150);
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::double_click_inc_rect(wx, wy)) {
                                        settings_app.double_click_ms = settings_app.double_click_ms.saturating_add(50).min(600);
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reverse_scroll_rect(wx, wy)) {
                                        settings_app.reverse_scroll = !settings_app.reverse_scroll;
                                    } else {
                                        mouse_setting_changed = false;
                                    }
                                    if mouse_setting_changed {
                                        settings_app.status_msg = gui::ui_text("Fare ayarları değiştirildi. Kalıcı olması için Kaydet'e tıklayın.", "Mouse settings changed. Click Save to keep them.");
                                    }
                                }
                                if settings_app.active_tab == 4 {
                                    for i in 0..apps::settings::SystemSettings::test_screen_btn_count() {
                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::test_screen_btn_rect(wx, wy, i)) {
                                            settings_app.test_screen_pattern = i as u8;
                                        }
                                    }
                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::test_ram_btn_rect(wx, wy)) {
                                        settings_app.run_ram_test();
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::test_disk_btn_rect(wx, wy)) {
                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Disk testi çalışıyor...", "Running disk test..."), || settings_app.run_disk_test());
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::test_audio_btn_rect(wx, wy)) {
                                        settings_app.run_audio_test(&mut audio_dev);
                                    }
                                }
                            }
                            2 => {
                                // Not Defteri: Dosya/Düzen/Yardım araç çubuğu ve menü aksiyonları
                                // Tıklama testleri, menü çizimiyle aynı geometri
                                // yardımcılarını (apps::notepad) kullanır; böylece font
                                // ölçeği 100'den farklıyken de koordinatlar örtüşür.
                                let menu_tb_y = wy + 30;
                                let menu_tb_h = crate::gui::sp(24);
                                let tb_hit = my >= menu_tb_y && my < menu_tb_y + menu_tb_h;
                                let tab1 = crate::apps::notepad::NotepadApp::menu_tab_rect(1);
                                let tab2 = crate::apps::notepad::NotepadApp::menu_tab_rect(2);
                                let tab3 = crate::apps::notepad::NotepadApp::menu_tab_rect(3);

                                if tb_hit && gui::in_rect(mx, my, (wx + tab1.0, menu_tb_y, tab1.1, menu_tb_h)) {
                                    notepad_app.menu = if notepad_app.menu == 1 { 0 } else { 1 };
                                } else if tb_hit && gui::in_rect(mx, my, (wx + tab2.0, menu_tb_y, tab2.1, menu_tb_h)) {
                                    notepad_app.menu = if notepad_app.menu == 2 { 0 } else { 2 };
                                } else if tb_hit && gui::in_rect(mx, my, (wx + tab3.0, menu_tb_y, tab3.1, menu_tb_h)) {
                                    notepad_app.menu = if notepad_app.menu == 3 { 0 } else { 3 };
                                } else if notepad_app.menu != 0
                                    && gui::in_rect(mx, my, (wx + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu), wy + crate::apps::notepad::NotepadApp::menu_box_y(), crate::apps::notepad::NotepadApp::menu_box_w(), crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu)))
                                {
                                    let rel_y = my.saturating_sub(wy + crate::apps::notepad::NotepadApp::menu_box_y() + crate::apps::notepad::NotepadApp::menu_pad());
                                    let item_idx = rel_y / crate::apps::notepad::NotepadApp::menu_item_h();
                                    let items = crate::apps::notepad::NotepadApp::menu_items(notepad_app.menu);
                                    if let Some(def) = items.get(item_idx) {
                                        match def.action {
                                            crate::apps::notepad::NotepadMenuAction::Open => {
                                                let path = alloc::string::String::from(notepad_app.file_path_str());
                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                                if let Some(data) = data {
                                                    notepad_app.open_file(&path, data);
                                                }
                                            }
                                            crate::apps::notepad::NotepadMenuAction::Save => {
                                                if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {
                                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                                }
                                            }
                                            crate::apps::notepad::NotepadMenuAction::About => {
                                                notepad_app.execute_action(def.action, &mut fs_mgr.vfs);
                                                open_app(&mut compositor, 1);
                                                settings_app.active_tab = 3;
                                            }
                                            _ => {
                                                notepad_app.execute_action(def.action, &mut fs_mgr.vfs);
                                            }
                                        }
                                    }
                                    notepad_app.menu = 0;
                                } else if notepad_app.menu != 0 {
                                    // Menü açıkken menü dışına tıklandığında menüyü kapat
                                    notepad_app.menu = 0;
                                } else if mx >= wx + ww.saturating_sub(130) && mx <= wx + ww.saturating_sub(74) && my >= wy + 32 && my <= wy + 52 {
                                    notepad_app.clear();
                                } else if mx >= wx + ww.saturating_sub(68) && mx <= wx + ww.saturating_sub(12) && my >= wy + 32 && my <= wy + 52 {
                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {
                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                } else if gui::in_rect(mx, my, (wx + 6, wy + 58, ww.saturating_sub(12), wh.saturating_sub(90))) {
                                    let at = notepad_app.index_at_scrolled(mx - wx, my - wy, ww, compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0));
                                    notepad_app.cursor = at;
                                    if keyboard.shift {
                                        let anchor = notepad_app.selection_anchor.unwrap_or(at);
                                        if anchor == at {
                                            notepad_app.selection_start = None;
                                            notepad_app.selection_end = None;
                                        } else {
                                            notepad_app.selection_start = Some(anchor.min(at));
                                            notepad_app.selection_end = Some(anchor.max(at));
                                        }
                                    } else {
                                        notepad_app.selection_anchor = Some(at);
                                        notepad_app.selection_start = None;
                                        notepad_app.selection_end = None;
                                    }
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
                                            &mut terminal_app, &mut installer_app, &mut viewer3d_app);
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
                                // Tüm dikdörtgenler `PaintApp::*_rect` yardımcılarından gelir;
                                // çizim (`render_content`) ile tıklama testi birebir örtüşür.
                                let r = apps::paint::PaintApp::open_btn_rect(ww);
                                let open_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::save_btn_rect(ww);
                                let save_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::clear_btn_rect(ww);
                                let clear_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::eraser_rect();
                                let eraser_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::brush_minus_rect();
                                let brush_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::brush_plus_rect();
                                let brush_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::zoom_minus_rect(ww);
                                let zoom_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::zoom_plus_rect(ww);
                                let zoom_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let file_btn = apps::paint::PaintApp::file_menu_rect(ww);
                                let file_btn_rect = (wx + file_btn.0, wy + file_btn.1, file_btn.2, file_btn.3);
                                // Açılır menü öğesi seçiliyse önce onu işle ve menüyü kapat.
                                let mut menu_handled = false;
                                if paint_apps[idx].as_ref().map_or(false, |p| p.file_menu_open) {
                                    for item in 0..apps::paint::PaintApp::FILE_MENU_COUNT {
                                        let r = apps::paint::PaintApp::file_menu_item_rect(ww, item);
                                        let item_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                        if gui::in_rect(mx, my, item_rect) {
                                            if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                            paint_apps[idx].as_mut().unwrap().file_menu_open = false;
                                            let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);
                                            match item {
                                                0 => paint_apps[idx].as_mut().unwrap().new_document(),
                                                1 => { with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP aciliyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs)); }
                                                2 => {
                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Cizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {
                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                                    }
                                                }
                                                _ => {
                                                    // Farklı Kaydet: dosya adını türetip yeni dosyaya yaz.
                                                    let name = {
                                                        let cur = paint_apps[idx].as_ref().unwrap().current_name();
                                                        apps::paint::PaintApp::derive_save_as_name(cur)
                                                    };
                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Farkli kaydediliyor...", "Saving as..."), || paint_apps[idx].as_mut().unwrap().save_as(&mut fs_mgr.vfs, cw, ch, name.as_str())) {
                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                                    }
                                                }
                                            }
                                            menu_handled = true;
                                            break;
                                        }
                                    }
                                    // Menü açıkken dışına tıklandıysa sadece menüyü kapat.
                                    if !menu_handled {
                                        paint_apps[idx].as_mut().unwrap().file_menu_open = false;
                                        menu_handled = true;
                                    }
                                }
                                if menu_handled {
                                    // Menü işlemi tamamlandı; diğer isabet testleri atlanır.
                                } else if gui::in_rect(mx, my, file_btn_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let p = paint_apps[idx].as_mut().unwrap();
                                    p.file_menu_open = !p.file_menu_open;
                                } else if gui::in_rect(mx, my, open_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP aciliyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs));
                                } else if gui::in_rect(mx, my, save_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);
                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Cizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {
                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                } else if gui::in_rect(mx, my, clear_rect) {
                                    if let Some(paint) = paint_apps[idx].as_mut() { paint.clear(); }
                                } else if gui::in_rect(mx, my, eraser_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    paint_apps[idx].as_mut().unwrap().eraser = true;
                                } else if gui::in_rect(mx, my, brush_minus_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    paint_apps[idx].as_mut().unwrap().adjust_brush(-1);
                                } else if gui::in_rect(mx, my, brush_plus_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    paint_apps[idx].as_mut().unwrap().adjust_brush(1);
                                } else if gui::in_rect(mx, my, zoom_minus_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    paint_apps[idx].as_mut().unwrap().adjust_zoom(-1);
                                } else if gui::in_rect(mx, my, zoom_plus_rect) {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    paint_apps[idx].as_mut().unwrap().adjust_zoom(1);
                                } else if {
                                    // Renk kutularından birine basıldı mı?
                                    let mut hit: Option<usize> = None;
                                    for i in 0..4 {
                                        let r = apps::paint::PaintApp::swatch_rect(i);
                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) { hit = Some(i); break; }
                                    }
                                    hit.is_some()
                                } {
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let paint = paint_apps[idx].as_mut().unwrap();
                                    for i in 0..4 {
                                        let r = apps::paint::PaintApp::swatch_rect(i);
                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) {
                                            paint.color = apps::paint::PALETTE[i];
                                            paint.eraser = false;
                                            break;
                                        }
                                    }
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
                            11 => {
                                // minOs Kurulum Sihirbazı
                                let sel_rect = apps::installer::InstallerApp::drive_select_rect(wx, wy, ww);
                                let type_rect = apps::installer::InstallerApp::format_type_btn_rect(wx, wy, ww, wh);
                                let fmt_rect = apps::installer::InstallerApp::format_btn_rect(wx, wy, ww, wh);
                                let btn_rect = apps::installer::InstallerApp::install_btn_rect(wx, wy, ww, wh);
                                if gui::in_rect(mx, my, sel_rect) {
                                    installer_app.select_next_drive();
                                } else if gui::in_rect(mx, my, type_rect) {
                                    installer_app.cycle_format_type();
                                } else if gui::in_rect(mx, my, fmt_rect) {
                                    // 2. Adım: Sadece biçimlendir.
                                    if installer_app.state == apps::installer::InstallState::Selecting
                                        || installer_app.state == apps::installer::InstallState::Failed
                                    {
                                        let formatted = with_busy_progress(&mut fb, mx, my, gui::ui_text("Disk bicimlendiriliyor...", "Formatting disk..."), || installer_app.format_selected_drive());
                                        if formatted {
                                            filemanager_app.set_path("/media/hdd0");
                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                        }
                                    }
                                } else if gui::in_rect(mx, my, btn_rect) {
                                    // 3. Adım: Onaylı tek tıkla kurulum.
                                    if installer_app.state == apps::installer::InstallState::Selecting
                                        || installer_app.state == apps::installer::InstallState::Failed
                                    {
                                        if installer_app.disk_formatted {
                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));
                                            if installed { filemanager_app.set_path("/media/hdd0"); }
                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                        } else if installer_app.confirm_install {
                                            // Onay verildi: biçimlendirip kur.
                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));
                                            if installed { filemanager_app.set_path("/media/hdd0"); }
                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                        } else {
                                            // İlk tıklama: onay iste (diski biçimlendirecek).
                                            installer_app.confirm_install = true;
                                            installer_app.status_msg = "UYARI: Disk silinip FAT32 olarak bicimlendirilecek. Onaylamak icin tekrar tiklayin.";
                                        }
                                    }
                                }
                            }
                            12 => {
                                viewer3d_app.handle_mouse_down(mx.saturating_sub(wx), my.saturating_sub(wy), ww, wh);
                            }
                            _ => {}
                        }
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
                            &mut terminal_app, &mut installer_app, &mut viewer3d_app);
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
                    else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] { desktop_files.begin_rename(); }
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
                            if notepad_app.menu == 0 {
                                notepad_app.move_vertical_ext(true, keyboard.shift);
                                notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                            }
                        } else if tm_focused && taskmgr_app.active_tab == 0 {
                            taskmgr_app.select_up();
                        } else if fm_focused {
                            filemanager_app.select_prev();
                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                        } else if set_focused && settings_app.focused_control < 5 {
                            settings_app.select_tab_prev();
                        } else if br_focused && browser_app.focused_control < 3 {
                            browser_app.select_tab_prev();
                        } else if app_is_focused(&compositor, 12) {
                            viewer3d_app.rot_x -= 0.1;
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
                            if notepad_app.menu == 0 {
                                notepad_app.move_vertical_ext(false, keyboard.shift);
                                notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                            }
                        } else if tm_focused && taskmgr_app.active_tab == 0 {
                            taskmgr_app.select_down();
                        } else if fm_focused {
                            filemanager_app.select_next();
                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                        } else if set_focused && settings_app.focused_control < 5 {
                            settings_app.select_tab_next();
                        } else if br_focused && browser_app.focused_control < 3 {
                            browser_app.select_tab_next();
                        } else if app_is_focused(&compositor, 12) {
                            viewer3d_app.rot_x += 0.1;
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowLeft => {
                    if desktop_files.rename_active {
                        desktop_files.rename_move_cursor(false);
                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {
                        filemanager_app.rename_move_cursor(false);
                    } else if compositor.is_start_menu_open {
                        compositor.active_start_group = compositor.active_start_group.saturating_sub(1);
                        compositor.selected_start_item = 0;
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        if keyboard.ctrl {
                            browser_app.address_move_word_left_ext(keyboard.shift);
                        } else {
                            browser_app.address_move_left_ext(keyboard.shift);
                        }
                    } else if app_is_focused(&compositor, 2) {
                        if notepad_app.menu != 0 {
                            notepad_app.menu = match notepad_app.menu {
                                1 => 3,
                                2 => 1,
                                _ => 2,
                            };
                        } else {
                            if keyboard.ctrl {
                                notepad_app.move_word_left_ext(keyboard.shift);
                            } else {
                                notepad_app.move_left_ext(keyboard.shift);
                            }
                            notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                        }
                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 0;
                        taskmgr_app.focused_control = 0;
                    } else if app_is_focused(&compositor, 1) {
                        settings_app.select_tab_prev();
                    } else if app_is_focused(&compositor, 3) {
                        if browser_app.address_focused {
                            if keyboard.ctrl {
                                browser_app.address_move_word_left_ext(keyboard.shift);
                            } else {
                                browser_app.address_move_left_ext(keyboard.shift);
                            }
                        } else {
                            browser_app.select_tab_prev();
                        }
                    } else if app_is_focused(&compositor, 12) {
                        viewer3d_app.rot_y -= 0.1;
                    }
                }
                drivers::keyboard::KeyAction::ArrowRight => {
                    if desktop_files.rename_active {
                        desktop_files.rename_move_cursor(true);
                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {
                        filemanager_app.rename_move_cursor(true);
                    } else if compositor.is_start_menu_open {
                        if compositor.active_start_group + 1 < 4 {
                            compositor.active_start_group += 1;
                            compositor.selected_start_item = 0;
                        }
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        if keyboard.ctrl {
                            browser_app.address_move_word_right_ext(keyboard.shift);
                        } else {
                            browser_app.address_move_right_ext(keyboard.shift);
                        }
                    } else if app_is_focused(&compositor, 2) {
                        if notepad_app.menu != 0 {
                            notepad_app.menu = match notepad_app.menu {
                                1 => 2,
                                2 => 3,
                                _ => 1,
                            };
                        } else {
                            if keyboard.ctrl {
                                notepad_app.move_word_right_ext(keyboard.shift);
                            } else {
                                notepad_app.move_right_ext(keyboard.shift);
                            }
                            notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                        }
                    } else if app_is_focused(&compositor, 12) {
                        viewer3d_app.rot_y += 0.1;
                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {
                        taskmgr_app.active_tab = 1;
                        taskmgr_app.focused_control = 1;
                    } else if app_is_focused(&compositor, 1) {
                        settings_app.select_tab_next();
                    } else if app_is_focused(&compositor, 3) {
                        if browser_app.address_focused {
                            if keyboard.ctrl {
                                browser_app.address_move_word_right_ext(keyboard.shift);
                            } else {
                                browser_app.address_move_right_ext(keyboard.shift);
                            }
                        } else {
                            browser_app.select_tab_next();
                        }
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
                        if i == 13 {
                            if compositor.confirm_shutdown {
                                #[cfg(minos_boot_log)]
    serial_print("[minOs] System Shutdown confirmed via Keyboard!\n");
                                unsafe {
                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                                    core::arch::asm!("cli; hlt");
                                }
                            } else {
                                compositor.confirm_shutdown = true;
                            }
                        } else if i == 14 {
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
                                        &mut terminal_app, &mut installer_app, &mut viewer3d_app);
                                }
                            }
                            _ => {}
                        }
                    } else if app_is_focused(&compositor, 0) {
                        enter_char_consumed = true;
                        if filemanager_app.rename_active {
                            filemanager_app.commit_rename(&mut fs_mgr.vfs);
                        } else {
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
                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                                        if let Some(data) = data {
                                            notepad_app.open_file(&path, data);
                                        }
                                        open_app(&mut compositor, 2);
                                    } else if tag == "IMG" {
                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim aciliyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));
                                        open_app(&mut compositor, 4);
                                    } else if tag == "SND" {
                                        open_app(&mut compositor, 5);
                                    } else if tag == "VID" {
                                        open_app(&mut compositor, 6);
                                    } else if tag == "WEB" {
                                        open_app(&mut compositor, 3);
                                    } else if tag == "3D" {
                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model aciliyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));
                                        if let Some(data) = data {
                                            viewer3d_app.load_file(&path, data);
                                        }
                                        open_app(&mut compositor, 12);
                                    }
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
                                    9 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); }
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
                                    14 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },
                                    _ => {}
                                },
                                2 => match settings_app.focused_control {
                                    4 => {
                                        settings_app.dhcp_enabled = !settings_app.dhcp_enabled;
                                        settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };
                                    }
                                    5 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    6 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },
                                    _ => {}
                                },
                                _ => match settings_app.focused_control {
                                    4 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),
                                    5 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },
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
                    #[cfg(minos_boot_log)]
    serial_print("[minOs] System Shutdown requested!\n");
                    unsafe {
                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                        core::arch::asm!("cli; hlt");
                    }
                }
                drivers::keyboard::KeyAction::CopyText => {
                    if app_is_focused(&compositor, 2) { notepad_app.copy_selection(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_copy(); }
                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, false); }
                    else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] { desktop_files.copy_selection(&mut file_clipboard, false); }
                    else if app_is_focused(&compositor, 8) { calculator_app.copy_result(); }
                }
                drivers::keyboard::KeyAction::PasteText => {
                    if app_is_focused(&compositor, 2) { notepad_app.paste_clipboard(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_paste(); }
                    else if app_is_focused(&compositor, 0) { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }
                    else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }
                    else if app_is_focused(&compositor, 8) { calculator_app.paste_expression(); }
                }
                drivers::keyboard::KeyAction::CutText => {
                    if app_is_focused(&compositor, 2) { notepad_app.cut_selection(); }
                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_cut(); }
                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, true); }
                    else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] { desktop_files.copy_selection(&mut file_clipboard, true); }
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
                drivers::keyboard::KeyAction::SaveDocument => {
                    if app_is_focused(&compositor, 2) {
                        if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {
                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                        }
                    }
                }
                drivers::keyboard::KeyAction::NewDocument => {
                    if app_is_focused(&compositor, 2) {
                        notepad_app.clear();
                        notepad_app.status_msg = gui::ui_text("Yeni belge oluşturuldu", "New document created");
                    }
                }
                drivers::keyboard::KeyAction::OpenDocument => {
                    if app_is_focused(&compositor, 2) {
                        let path = alloc::string::String::from(notepad_app.file_path_str());
                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya aciliyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                        if let Some(data) = data {
                            notepad_app.open_file(&path, data);
                        }
                    }
                }
                drivers::keyboard::KeyAction::HomeKey => {
                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        if keyboard.ctrl { notepad_app.move_document_home(keyboard.shift); }
                        else { notepad_app.move_home_ext(keyboard.shift); }
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_home_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 0) {
                        filemanager_app.select_first();
                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                    } else if app_is_focused(&compositor, 7) {
                        taskmgr_app.select_first();
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);
                            terminal_app.scroll_top(vl);
                        }
                    }
                }
                drivers::keyboard::KeyAction::EndKey => {
                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        if keyboard.ctrl { notepad_app.move_document_end(keyboard.shift); }
                        else { notepad_app.move_end_ext(keyboard.shift); }
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_end_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 0) {
                        filemanager_app.select_last();
                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                    } else if app_is_focused(&compositor, 7) {
                        taskmgr_app.select_last();
                    } else if app_is_focused(&compositor, 10) {
                        terminal_app.scroll_bottom();
                    }
                }
                drivers::keyboard::KeyAction::PageUpKey => {
                    if app_is_focused(&compositor, 0) {
                        let rows = focused_window_for_app(&compositor, 0).map(|(_, w)| fm_page_rows(&w)).unwrap_or(8);
                        filemanager_app.select_page(false, rows);
                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        let lines = focused_window_for_app(&compositor, 2).map(|(_, w)| notepad_app.page_lines(w.height)).unwrap_or(10);
                        notepad_app.move_page_ext(true, lines, keyboard.shift);
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 7) {
                        taskmgr_app.select_page(false, 8);
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);
                            terminal_app.scroll_output(vl as isize, vl);
                        }
                    }
                }
                drivers::keyboard::KeyAction::PageDownKey => {
                    if app_is_focused(&compositor, 0) {
                        let rows = focused_window_for_app(&compositor, 0).map(|(_, w)| fm_page_rows(&w)).unwrap_or(8);
                        filemanager_app.select_page(true, rows);
                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);
                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        let lines = focused_window_for_app(&compositor, 2).map(|(_, w)| notepad_app.page_lines(w.height)).unwrap_or(10);
                        notepad_app.move_page_ext(false, lines, keyboard.shift);
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 7) {
                        taskmgr_app.select_page(true, 8);
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);
                            terminal_app.scroll_output(-(vl as isize), vl);
                        }
                    }
                }
                drivers::keyboard::KeyAction::ToggleOverwrite => {
                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 { notepad_app.toggle_overwrite(); }
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
                                &mut terminal_app, &mut installer_app, &mut viewer3d_app);
                        }
                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        notepad_app.delete_action();
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_delete();
                    } else if app_is_focused(&compositor, 0) && !filemanager_app.rename_active {
                        filemanager_app.delete_selected(&mut fs_mgr.vfs);
                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {
                        filemanager_app.rename_delete();
                    } else if desktop_files.rename_active {
                        desktop_files.rename_delete();
                    } else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] {
                        desktop_files.delete_selected(&mut fs_mgr.vfs);
                    }
                }
                drivers::keyboard::KeyAction::DeleteWordBack => {
                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        notepad_app.delete_word_backward();
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_delete_word_back();
                    } else if app_is_focused(&compositor, 10) {
                        terminal_app.delete_word();
                    }
                }
                drivers::keyboard::KeyAction::DeleteWordForward => {
                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {
                        notepad_app.delete_word_forward();
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {
                        browser_app.address_delete_word_forward();
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
            // Ayarlar > Testler: tekerlek sınaması sayacı.
            if app_is_focused(&compositor, 1) && settings_app.active_tab == 4 {
                if let Some((_, w)) = focused_window_for_app(&compositor, 1) {
                    let pad = apps::settings::SystemSettings::test_mouse_pad_rect(w.x, w.y);
                    if gui::in_rect(mx, my, pad) {
                        settings_app.note_scroll_test(scroll_val.unsigned_abs() as u32);
                    }
                }
            }
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
            // CMD geçmişi: yalnızca çıktı panelini kaydır; giriş satırı sabit kalır.
            if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                if w.is_open && w.is_focused && !w.is_minimized
                    && gui::in_rect(mouse.x as usize, mouse.y as usize, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h() + 28))) {
                    let view_lines = w.height.saturating_sub(86) / 13;
                    terminal_app.scroll_output(scroll_val as isize, view_lines);
                }
            }
            // İmlecin altındaki en üst pencerede içerik kaydırmasını güncelle.
            // Scrollbar aynı ScrollState üzerinden çizildiği için tutamaç da
            // tekerlekle birlikte anında hareket eder.
            let (window_order, window_count) = compositor.draw_order();
            for k in (0..window_count).rev() {
                let idx = window_order[k];
                let Some(w) = compositor.windows[idx] else { continue; };
                if !w.is_open || w.is_minimized || !gui::in_rect(mx, my, (w.x, w.y, w.width, w.height)) {
                    continue;
                }
                let app_id = w.app_id as usize;
                let (cx, cy, cw, ch) = w.content_rect();
                let c_h = window_content_h(app_id, w.height, w.width, &filemanager_app, &notepad_app);
                let c_w = cw;
                let over_content = gui::in_rect(mx, my, (cx, cy, cw, ch));
                if over_content && (c_h > ch || c_w > cw) {
                    let max_y = c_h.saturating_sub(ch);
                    let step = if app_id == 2 { gui::sp(16) } else { gui::sp(24) };
                    let delta = (scroll_val as isize) * step.max(1) as isize;
                    if let Some(wm) = compositor.windows[idx].as_mut() {
                        wm.scroll.y = ((wm.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    }
                }
                break;
            }
            // Görüntüleyicide tekerlek zoom kısayoludur (yukarı yakınlaştırır).
            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {
                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&w);
                if gui::in_rect(mx, my, (cx, cy, cw, ch)) {
                    imageviewer_app.pan_y = imageviewer_app.pan_y.saturating_add((scroll_val as isize) * 24);
                }
            }
            // Çizim tuvalinde tekerlek yakınlaştırmayı ayarlar.
            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {
                if gui::in_rect(mx, my, (w.x, w.y, w.width, w.height)) {
                    viewer3d_app.handle_scroll(scroll_val);
                }
            }
            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {
                if let Some(paint) = paint_apps[paint_idx].as_mut() {
                    if paint.to_canvas(w.width, w.height, mx.saturating_sub(w.x), my.saturating_sub(w.y)).is_some() {
                        paint.adjust_zoom(scroll_val);
                    }
                }
            }
        }

        // 5. Pencere Sürükleme ve Yeniden Boyutlandırma
        if mouse.left_button {
            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {
                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&w);
                if w.is_focused && gui::in_rect(mx, my, (cx, cy, cw, ch)) {
                    if let Some((last_x, last_y)) = image_drag_last {
                        imageviewer_app.pan_x = imageviewer_app.pan_x.saturating_add(mx as isize - last_x as isize);
                        imageviewer_app.pan_y = imageviewer_app.pan_y.saturating_add(my as isize - last_y as isize);
                    }
                    image_drag_last = Some((mx, my));
                } else {
                    image_drag_last = None;
                }
            } else {
                image_drag_last = None;
            }
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
            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {
                if w.is_focused {
                    viewer3d_app.handle_mouse_move(mx.saturating_sub(w.x), my.saturating_sub(w.y));
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
            if notepad_dragging && notepad_app.menu == 0 {
                if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);
                    notepad_app.cursor = at;
                    let anchor = notepad_app.selection_anchor.unwrap_or(at);
                    if anchor == at {
                        notepad_app.selection_start = None;
                        notepad_app.selection_end = None;
                    } else {
                        notepad_app.selection_start = Some(anchor.min(at));
                        notepad_app.selection_end = Some(anchor.max(at));
                    }
                }
            }
            let (drag_item, drag_start, drag_active) = if desktop_drag_source {
                (desktop_files.drag_item, desktop_files.drag_start, desktop_files.drag_active)
            } else { (filemanager_app.drag_item, filemanager_app.drag_start, filemanager_app.drag_active) };
            if let Some(src_idx) = drag_item {
                if !drag_active && (mx.abs_diff(drag_start.0) > 4 || my.abs_diff(drag_start.1) > 4) {
                    if desktop_drag_source {
                        desktop_files.drag_active = true;
                        if !desktop_files.selected_items[src_idx] { desktop_files.selected_items = [false; apps::filemanager::MAX_ITEMS]; desktop_files.selected_items[src_idx] = true; }
                    } else {
                        filemanager_app.drag_active = true;
                        filemanager_app.last_click_idx = None;
                        if !filemanager_app.selected_items[src_idx] { filemanager_app.selected_items = [false; apps::filemanager::MAX_ITEMS]; filemanager_app.selected_items[src_idx] = true; }
                    }
                }
                if let Some((_, fm_win)) = focused_window_for_app(&compositor, 0) {
                    let target = filemanager_app.drop_target_at(fm_win.x, fm_win.y, fm_win.width, fm_win.height, fm_win.scroll.y, mx, my);
                    if desktop_drag_source { desktop_files.drag_target = target; }
                    else { filemanager_app.drag_target = target; }
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
        if mouse_released { image_drag_last = None; }

        if mouse_released {
            drag_prev_box = None;
            drag_dirty_rect = None;
            viewer3d_app.handle_mouse_up();
            if let Some(paint_idx) = compositor.active_window_idx {
                if let Some(paint) = paint_apps[paint_idx].as_mut() { paint.end_stroke(); }
            }
            notepad_dragging = false;
            let (is_drag_active, drag_item, drag_target) = if desktop_drag_source {
                (desktop_files.drag_active, desktop_files.drag_item, desktop_files.drag_target)
            } else { (filemanager_app.drag_active, filemanager_app.drag_item, filemanager_app.drag_target) };
            if is_drag_active {
                if let Some(src_idx) = drag_item {
                    if let Some(target) = drag_target {
                        let src_app = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };
                        match src_app.execute_drop(src_idx, target, keyboard.ctrl, &mut fs_mgr.vfs) {
                            Ok(_) => { src_app.refresh_from_vfs(&fs_mgr.vfs); }
                            Err(err) => { src_app.set_status_text(err); }
                        }
                    } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {
                        let source = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };
                        let indices = source.drag_items;
                        let source_count = source.file_count;
                        for i in 0..source_count {
                            if !(indices[i] || i == src_idx) { continue; }
                            let name = source.files[i].display_name();
                            if name == ".." { continue; }
                            let mut src_path = alloc::string::String::from(source.path());
                            if !src_path.ends_with('/') { src_path.push('/'); }
                            src_path.push_str(name);
                            if desktop_drag_source { if keyboard.ctrl { let _ = fs_mgr.vfs.copy_item(&src_path, "/desktop"); } else { let _ = fs_mgr.vfs.move_item(&src_path, "/desktop"); } }
                            else if keyboard.ctrl { let _ = fs_mgr.vfs.copy_item(&src_path, "/desktop"); }
                            else { let _ = fs_mgr.vfs.move_item(&src_path, "/desktop"); }
                        }
                        desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                        if !desktop_drag_source { filemanager_app.refresh_from_vfs(&fs_mgr.vfs); }
                    }
                }
            }
            desktop_drag_source = false;
            desktop_files.drag_item = None;
            desktop_files.drag_active = false;
            desktop_files.drag_target = None;
            filemanager_app.drag_item = None;
            filemanager_app.drag_active = false;
            filemanager_app.drag_target = None;
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
            } else if ch == '\x1b' && (filemanager_app.drag_active || desktop_files.drag_active) {
                filemanager_app.drag_item = None;
                filemanager_app.drag_active = false;
                filemanager_app.drag_target = None;
                desktop_files.drag_item = None;
                desktop_files.drag_active = false;
                desktop_files.drag_target = None;
            } else if filemanager_app.rename_active && app_is_focused(&compositor, 0) {
                if ch == '\x1b' { filemanager_app.rename_active = false; }
                else if ch == '\x08' { filemanager_app.rename_backspace(); }
                else if ch == '\n' { filemanager_app.commit_rename(&mut fs_mgr.vfs); }
                else if ch >= ' ' { filemanager_app.rename_char(ch); }
            } else if desktop_files.rename_active {
                if ch == '\x1b' { desktop_files.rename_active = false; }
                else if ch == '\x08' { desktop_files.rename_backspace(); }
                else if ch == '\n' { desktop_files.commit_rename(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }
                else if ch >= ' ' { desktop_files.rename_char(ch); }
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
                    if notepad_app.menu != 0 {
                        notepad_app.menu = 0;
                    } else if ch == '\x08' {
                        notepad_app.backspace();
                    } else if ch == '\t' {
                        // Tab = girinti ekle, Shift+Tab = girintiyi geri al.
                        if keyboard.shift { notepad_app.unindent(); } else { notepad_app.insert_indent(); }
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                    } else if ch == '\n' || ch >= ' ' {
                        notepad_app.insert_char(ch);
                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
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
                    if let Some(app_idx) = terminal_app.execute(line, &mut fs_mgr.vfs, &mut memory_mgr.pmm, &now) {
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
                let v3d_focused = app_is_focused(&compositor, 12);
                if v3d_focused {
                    if ch == ' ' || ch == 'r' || ch == 'R' {
                        viewer3d_app.toggle_auto_rotate();
                    } else if ch == 'w' || ch == 'W' {
                        viewer3d_app.toggle_wireframe();
                    } else if ch == '+' || ch == '=' {
                        viewer3d_app.zoom_in();
                    } else if ch == '-' || ch == '_' {
                        viewer3d_app.zoom_out();
                    } else if ch == '1' {
                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Cube);
                    } else if ch == '2' {
                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Cylinder);
                    } else if ch == '3' {
                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Sphere);
                    } else if ch == '4' {
                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Pyramid);
                    }
                }
            }
        }

        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;
        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;

        let v3d_dragging = mouse.left_button && viewer3d_app.drag_active;
        let is_dragging = dragging_win.is_some() || resizing_win.is_some() || vscroll_drag.is_some() || notepad_dragging || filemanager_app.drag_active || v3d_dragging;
        let paint_active = mouse.left_button && app_is_focused(&compositor, 9);
        let is_interacting = is_dragging || paint_active;

        let viewer3d_open = compositor.windows.iter().flatten().any(|w| w.app_id == 12 && w.is_open && !w.is_minimized);
        let viewer3d_animating = viewer3d_open && (viewer3d_app.auto_rotate || viewer3d_app.drag_active);
        let media_animating = (musicplayer_app.is_playing && !audio_dev.is_muted) || videoplayer_app.is_playing || viewer3d_animating;
        let frame_interval_ms = if is_interacting || viewer3d_animating { 16 } else { 33 };
        let timer_tick_fired = monotonic_ms().wrapping_sub(last_frame_ms) >= frame_interval_ms;
        if timer_tick_fired {
            last_frame_ms = monotonic_ms();
            anim_tick = anim_tick.wrapping_add(1);
            if viewer3d_open {
                viewer3d_app.tick();
            }
        }

        // Ağ kartından gelen paketleri işle (ARP cevapları, ICMP vs.).
        drivers::e1000::poll();
        drivers::socket::poll_timeouts();
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
                // HDA hazır ve DMA ilerliyorsa gerçek PCM örneği üret ve gönder.
                if audio_dev.is_hda_active() {
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
                    if speaker_on {
                        audio_dev.set_pc_speaker_tone(0);
                        speaker_on = false;
                    }
                } else {
                    // Otomatik Fallback: HDA yok veya sanallaştırma ortamında DMA duraksadıysa PC Speaker PWM tonlama yap!
                    audio_dev.set_pc_speaker_tone(freq);
                    speaker_on = true;
                }
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
        // Ayarlar > Testler: ekran deseni dışında fare test verisini canlı besle.
        let settings_test_active = app_is_focused(&compositor, 1) && settings_app.active_tab == 4;
        if settings_test_active {
            if let Some((_, w)) = focused_window_for_app(&compositor, 1) {
                settings_app.capture_mouse_test(mx, my, mouse.left_button, mouse.right_button, mouse.middle_button, w.x, w.y);
            }
        }
        let busy_dirty = unsafe {
            let dirty = BUSY_DIRTY;
            BUSY_DIRTY = false;
            dirty
        };
        let full_redraw = input_update.keyboard || buttons_changed || scroll_val != 0
            || force_render || busy_dirty || (mouse_moved && (is_interacting || night_light_active || settings_test_active))
            || (timer_tick_fired && media_animating);

        if full_redraw {
            let tm_open = compositor.windows.iter().flatten().any(|w| w.app_id == 7 && w.is_open && !w.is_minimized);
            if tm_open {
                taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);
            }
            keep_windows_on_screen(&mut compositor, width, height);
            render_full_frame(
                &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,
                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app,
                &installer_app, &viewer3d_app,
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
    struct PanicSerial;
    impl FmtWrite for PanicSerial {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            serial_print(text);
            Ok(())
        }
    }

    serial_print("[PANIC] Kernel panic: ");
    let mut output = PanicSerial;
    let _ = writeln!(&mut output, "{}", info.message());
    if let Some(location) = info.location() {
        serial_print("[PANIC] Location: ");
        let _ = writeln!(
            &mut output,
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    loop {
        core::hint::spin_loop();
    }
}






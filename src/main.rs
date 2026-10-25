#![no_std]

#![no_main]

#![feature(alloc_error_handler)]



extern crate alloc;




use linked_list_allocator::LockedHeap;



#[allow(dead_code)]

const HEAP_SIZE: usize = 16 * 1024 * 1024;

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



// --- Asama 1: ayristirilan moduller -------------------------------------------------

// Tasima yok: bu modullerdeki ogeler crate kokunde yeniden disa aktarilir, boylece

// `crate::serial_print`, `crate::monotonic_ms`, `crate::valid_limine_response` gibi

// cagrilar degismeden calismaya devam eder.

pub mod boot;

pub mod desktop;

pub mod task;

pub mod panic_handler;



// These modules were split out of this file, but the rest of the kernel still

// uses their names from the crate root.  A private `use` is sufficient for

// intra-crate lookup; `pub use` silently drops private items and caused the

// compiler to report hundreds of cascading "not found" errors.

use boot::console::*;

use boot::cpu::*;

use boot::devices::*;

use boot::limine::*;

use boot::progress::*;

use boot::time::*;

use desktop::helpers::*;



pub(crate) static mut MEMORY_MGR: mm::MemoryManager = mm::MemoryManager::new();

fn alloc_pmm_frame_global() -> Option<u64> {
    unsafe {
        (*core::ptr::addr_of_mut!(MEMORY_MGR)).pmm.allocate_frame()
    }
}



/// Basit metin cizim fonksiyonu (font sistemi olmadan)

#[no_mangle]

pub extern "C" fn _start() -> ! {

    arch::x86_64::enable_sse();

    arch::x86_64::disable_pic();

    // Seri konsolu ilk is olarak hazirla: sonraki tum loglar (PMM/VMM/SMP…)

    // 115200 8N1 ile cerceveye oturur, VirtualBox yakalamasi bozulmaz.

    serial::init();

    // Initialize heap for dynamic allocations

    init_heap();

    // Install early display output before PMM/VMM/SMP and device probes.

    let (fb_ptr, mut width, mut height, pitch, bpp) = unsafe {

        let resp = valid_limine_response(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))

            .filter(|resp| !resp.is_null())

            .unwrap_or_else(|| boot::text_console::fatal("Limine framebuffer yaniti yok"));

        let count = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffer_count));

        let entries = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffers));

        if count == 0 || !is_mapped_limine_ptr(entries) {

            boot::text_console::fatal("Limine framebuffer listesi gecersiz; GUI acilmadi");

        }

        let entry = core::ptr::read_volatile(entries);

        if !is_mapped_limine_ptr(entry) {

            boot::text_console::fatal("Limine framebuffer girdisi gecersiz; GUI acilmadi");

        }

        let info = core::ptr::read_volatile(entry);

        if info.address.is_null() || !is_canonical_ptr(info.address as *const u8) {

            boot::text_console::fatal("Limine framebuffer adresi gecersiz; GUI acilmadi");

        }

        (info.address as *mut u32, info.width as usize, info.height as usize,

         info.pitch as usize, info.bpp as usize)

    };

    if bpp != 32 || width < 320 || height < 200 || pitch < width.saturating_mul(4)

        || pitch % 4 != 0 || width.checked_mul(height).map_or(true, |n| n > drivers::video::BACKBUFFER_LEN)

    {

        boot::text_console::fatal("Framebuffer dogrulamasi basarisiz; GUI acilmadi");

    }

    let mut fb = unsafe { drivers::video::Framebuffer::new(fb_ptr, width, height, pitch, bpp) };

    if !fb.verify_bounds() {

        boot::text_console::fatal("Framebuffer cizim sinirlari dogrulanamadi; GUI acilmadi");

    }

    unsafe {

        PANIC_FB_ADDR = fb_ptr;

        PANIC_FB_W = width;

        PANIC_FB_H = height;

        PANIC_FB_PITCH = pitch;

        PANIC_FB_BPP = bpp;

        boot::text_console::init(drivers::video::Framebuffer::new(fb_ptr, width, height, pitch, bpp));

    }

    serial_print("[BOOT] UEFI metin konsolu etkin; donanım ve bellek hazırlanıyor.\n");

    serial_print("[BOOT] PIT/TSC zaman tabanı başlatılıyor...\n");

    if init_timebase() {

        serial_print("[BOOT] PIT/TSC zaman tabanı hazır.\n");

    } else {

        serial_print("[BOOT] UYARI: PIT yanıt vermedi; CPUID TSC tahmini kullanılıyor.\n");

    }

    serial_print("[VIDEO] "); serial_print_dec(width as u64); serial_print("x");
    serial_print_dec(height as u64); serial_print(" @ "); serial_print_dec(bpp as u64); serial_print(" bpp.\n");



    #[cfg(minos_boot_log)]

    serial_print("[minOs] Çekirdek _start noktasına ulaştı.\n");

    #[cfg(minos_boot_log)]

    serial_print("[minOs] SSE etkinleştirildi, PIC kesmeleri maskelendi.\n");



    // HHDM ofsetini al (fiziksel MMIO ve sayfa tablosu sanal erisimi icin).

    let hhdm_offset = unsafe {
        // SAFETY: the response pointer is validated before being dereferenced.
        let response_offset = valid_limine_response(core::ptr::addr_of!(HHDM_REQUEST.response))
            .map(|resp| core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset)));
        match response_offset {
            Some(offset) if is_mapped_limine_ptr(offset as *const u8) => offset,
            Some(_) => {
                serial_print("[BOOT] Invalid HHDM offset / gecersiz HHDM ofseti; using fallback.\n");
                0xFFFF_8000_0000_0000
            }
            None => {
                serial_print("[BOOT] HHDM response missing / HHDM yaniti yok; using fallback.\n");
                0xFFFF_8000_0000_0000
            }
        }
    };
    serial_print("[BOOT] HHDM offset / ofset: ");
    crate::boot::console::serial_print_hex(hhdm_offset);
    serial_print("\n");

    drivers::set_hhdm_offset(hhdm_offset);



    let memory_mgr = unsafe { &mut *core::ptr::addr_of_mut!(MEMORY_MGR) };

    let boot::subsystems::BootSubsystems {
        mut fs_mgr,
        boot_log: _boot_log,
        gpu_caps: _gpu_caps,
        mut audio_dev,
        mouse,
        keyboard,
        installed,
        wants_installed: _wants_installed,
    } = boot::subsystems::init_subsystems(&mut fb, fb_ptr, memory_mgr, alloc_pmm_frame_global);



    // 5d. Eger diskte varsa (MINOS.SYS + fat32) — kurulu sistem —

    // welcome (hos geldin) ekranini atlayip dogrudan masaustuye gec.

    // Live ortam ise welcome ekrani gosterilir.

    // 6. MinGUI Pencere Yoneticisi ve Tum Uygulamalar

    serial_print("[BOOT] Pencere yöneticisi ve uygulamalar hazırlanıyor...\n");

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
    settings_app.screen_res = (width, height);

    // Disk uzerinde onceden kaydedilmis kalici ayarlar var mi kontrol et

    if fs::diskfs::load_file("settings.cfg") > 0 {

        let cfg_bytes = fs::diskfs::content();

        let _ = fs_mgr.vfs.write_file("settings.cfg", cfg_bytes);

    }

    settings_app.load_from_vfs(&fs_mgr.vfs);

    // Eger Limine onyukleme komut satirinda res=WxH secilmisse oncelik ver:
    if let Some(cmd_res) = boot::limine::cmdline_resolution() {
        settings_app.screen_res = cmd_res;
    }

    // Secili cozunurluk su anki ekrandan farkliysa ve GPU tarafindan destekleniyorsa acilista uygula:
    let target_res = settings_app.screen_res;
    if target_res != (fb.width, fb.height) && drivers::gpu::mode_supported(target_res.0, target_res.1) {
        let (switched, pitch_opt) = drivers::gpu::try_set_resolution(target_res.0, target_res.1);
        if switched {
            let p = pitch_opt.unwrap_or(target_res.0 * 4);
            let new_bytes = p.saturating_mul(target_res.1);
            let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
            for off in (0..new_bytes.max(16 * 1024 * 1024)).step_by(2 * 1024 * 1024) {
                arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
            }
            if fb.resize(target_res.0, target_res.1, p) {
                width = fb.width;
                height = fb.height;
                unsafe {
                    boot::text_console::update_framebuffer(
                        drivers::video::Framebuffer::new(fb_ptr, width, height, p, bpp),
                    );
                }
                settings_app.display_res = target_res;
                serial_print("[BOOT] Seçili çözünürlük başarıyla uygulandı: ");
                serial_print_dec(width as u64);
                serial_print("x");
                serial_print_dec(height as u64);
                serial_print("\n");
            }
        }
    }

    crate::gui::wallpaper::apply(settings_app.wallpaper, settings_app.wall_color);

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

    // Baslangicta yalnizca sabit uygulama pencereleri icin state olustur.

    // MAX_WINDOWS kadar 4096 noktali tuval ayirmak 1 MiB kernel heap'ini asar.

    let mut paint_apps: alloc::vec::Vec<Option<apps::paint::PaintApp>> = (0..gui::MAX_WINDOWS).map(|_| None).collect();

    let mut terminal_app = apps::terminal::TerminalApp::new();

    terminal_app.init_banner();

    let mut installer_app = apps::installer::InstallerApp::new();

    installer_app.refresh_drives();

    let mut viewer3d_app = apps::viewer3d::Viewer3DApp::new();

    let mut pkgmgr_app = apps::pkgmgr::PackageManagerApp::new();

    pkgmgr_app.refresh_installed(&fs_mgr.vfs);
    compositor.sync_installed_apps(&fs_mgr.vfs);



    // 15 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimligi).

    // Acilista TuM pencereler kapalidir; kullanici temiz bir masaustuyle baslar

    // ve uygulamalari Baslat Menusu, masaustu simgeleri veya sag tik ile acar.

    // 0: Dosya Yoneticisi

    let files_width = 720.min(width.saturating_sub(20)).max(200);

    let files_height = 500.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Dosya Yoneticisi", "Dosyalar",

        width.saturating_sub(files_width) / 2,

        height.saturating_sub(files_height + gui::taskbar_h()) / 2,

        files_width, files_height, false,

    ));

    // 1: Sistem Ayarlari

    let settings_width = 700.min(width.saturating_sub(20)).max(200);

    let settings_height = 540.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Sistem Ayarlari", "Ayarlar",

        width.saturating_sub(settings_width) / 2,

        height.saturating_sub(settings_height + gui::taskbar_h()) / 2,

        settings_width, settings_height, false,

    ));

    // 2: Not Defteri

    compositor.add_window(gui::Window::new("Not Defteri", "Notlar", 80, 70, 520, 330, false));

    // 3: Web Tarayicisi

    let browser_w = 760.min(width.saturating_sub(20)).max(300);

    let browser_h = 490.min(height.saturating_sub(gui::taskbar_h() + 10)).max(200);

    compositor.add_window(gui::Window::new(

        "Web Gezgini", "Web",

        width.saturating_sub(browser_w) / 2,

        height.saturating_sub(browser_h + gui::taskbar_h()) / 2,

        browser_w, browser_h, false,

    ));

    // 4: Resim Gosterici

    compositor.add_window(gui::Window::new("Resim Gosterici", "Resim", 120, 90, 500, 330, false));

    // 5: Muzik calar

    compositor.add_window(gui::Window::new("Muzik calar", "Muzik", 560, 80, 460, 245, false));

    // 6: Video Oynatici

    compositor.add_window(gui::Window::new("Video Oynatici", "Video", 150, 110, 540, 330, false));

    // 7: Gorev Yoneticisi

    compositor.add_window(gui::Window::new("Gorev Yoneticisi", "Gorevler", 220, 60, 480, 460, false));

    compositor.add_window(gui::Window::new("Hesap Makinesi", "Hesap", 260, 120, 360, 340, false));

    compositor.add_window(gui::Window::new("cizim Tahtasi", "cizim", 180, 80, 520, 360, false));

    compositor.add_window(gui::Window::new("Komut Istemi", "Komut", 220, 100, 560, 300, false));

    // 11: minOs Tek Tikla Kur Sihirbazi

    let inst_w = 540.min(width.saturating_sub(20)).max(200);

    let inst_h = 460.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "minOs Tek Tikla Kur", "Kurulum",

        width.saturating_sub(inst_w) / 2,

        height.saturating_sub(inst_h + gui::taskbar_h()) / 2,

        inst_w, inst_h, false,

    ));

    // 12: 3D Gosterici

    let v3d_w = 600.min(width.saturating_sub(20)).max(200);

    let v3d_h = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "3D Gosterici", "3D",

        width.saturating_sub(v3d_w) / 2,

        height.saturating_sub(v3d_h + gui::taskbar_h()) / 2,

        v3d_w, v3d_h, false,

    ));

    // 13: Paket Yoneticisi

    let pkg_w = 620.min(width.saturating_sub(20)).max(200);

    let pkg_h = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Paket Yoneticisi", "Paketler",

        width.saturating_sub(pkg_w) / 2,

        height.saturating_sub(pkg_h + gui::taskbar_h()) / 2,

        pkg_w, pkg_h, false,

    ));

    // 14: Kontrol Paneli (canli sistem / ag bilgileri)

    let cp_w = 620.min(width.saturating_sub(20)).max(200);

    let cp_h = 470.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Kontrol Paneli", "Kontrol",

        width.saturating_sub(cp_w) / 2,

        height.saturating_sub(cp_h + gui::taskbar_h()) / 2,

        cp_w, cp_h, false,

    ));



    // Acilista hicbir pencere odakli degildir (temiz masaustu).

    compositor.active_window_idx = None;



    // Kurulu sistemse welcome ekranini atla, live ortam ise goster.

    serial_print("[BOOT] Hoş geldin ekranı hazırlanıyor; grafik öz denetimleri çalıştırılıyor.\n");

    // Widget/Component agaci mimarisini (yerlesim, Tab odagi, olaylar) dogrula.

    if !gui::widget::self_test(&mut fb) {

        boot::text_console::fatal("Grafik arayuz oz-denetimi basarisiz; GUI acilmadi");

    }

    serial_print("[BOOT] Grafik arayüz denetimleri tamamlandı.\n");

    serial_print("[BOOT] CRITICAL_CHECKS_COMPLETE - Kritik denetimler tamamlandı; grafik arayüz başlatılıyor.\n");

    boot::text_console::disable();

    if installed {

        serial_print("[BOOT] Kurulu sistem tespit edildi; masaüstü hazırlanıyor...\n");

        for phase in 0..4 {

            gui::render_loading_transition(&mut fb, width, height, phase,

                gui::ui_text("Masaüstü hazırlanıyor", "Preparing desktop"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 25 { core::hint::spin_loop(); }

        }

    } else {

        for phase in 0..8 {

            gui::render_loading_transition(&mut fb, width, height, phase,

                gui::ui_text("Sistem yükleniyor", "Loading system"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }

        }

    }

    if !installed {
        desktop::welcome::run_welcome_screen(
            &mut fb,
            width,
            height,
            mouse,
            keyboard,
            &mut settings_app,
            &compositor,
            &mut fs_mgr,
            memory_mgr,
        );
        serial_print("[BOOT] Hoş geldin kabul edildi; masaüstüne geçiliyor.\n");
    }



    // Welcome ekranindan masaustune gecisi de ayni animasyonla yumusat.

    // Sadece live ortamda goster; kurulu sistemde atla.

    if !installed {

        for phase in 0..6 {

            gui::render_loading_transition(&mut fb, width, height, phase + 2, gui::ui_text("Masaüstü hazırlanıyor", "Preparing desktop"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }

        }

    }



    // RTC - gercek zamanli saat

    let mut now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());

    compositor.cal_year = now.year;

    compositor.cal_month = now.month;

    #[cfg(minos_boot_log)]

    serial_print("[minOs] RTC saati etkin.\n");

    // Save once the boot transcript is complete, including numeric fields.

    save_boot_log(&mut fs_mgr.vfs);



    // Acilis melodisi (Chime)

    audio_dev.set_volume(settings_app.volume);

    audio_dev.is_muted = settings_app.is_muted;

    audio_dev.play_chime();



    // Etkilesim durumu

    let mut last_mouse_x = mouse.x;

    let mut last_mouse_y = mouse.y;

    let mut last_buttons = (mouse.left_button, mouse.right_button);

    let mut pending_window_control: Option<(usize, u8)> = None;

    let mut dragging_win: Option<usize> = None;

    let mut resizing_win: Option<usize> = None;

    let mut resize_edge: Option<gui::ResizeEdge> = None;

    // Kenar yeniden boyutlandirma baslangicinda pencere geometrisi (sol/ust kenarlar icin)

    let mut resize_orig: (usize, usize, usize, usize) = (0, 0, 0, 0);

    let mut drag_offset_x = 0;

    let mut drag_offset_y = 0;

    // Aero Snap onizleme dikdortgeni: (x, y, w, h)

    let mut active_snap_preview: Option<(usize, usize, usize, usize)> = None;

    // Kaydirma cubugu surukleme: (idx, thumb_ust_ofset, thumb_h, track_h, content_h, view_h)

    let mut vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)> = None;

    let mut anim_tick: usize = 0;

    let mut last_frame_ms = monotonic_ms();

    let mut loop_counter: u64 = 0;

    let mut last_clock_update_ms = monotonic_ms();

    let mut last_music_beat: usize = 0;

    // Muzik calar ilerleme cubugu icin gercek zaman tabani (ms).
    let mut last_music_progress_ms = monotonic_ms();

    // Video oynatıcı kare zaman tabanı (ms); ~30 FPS.
    let mut last_video_frame_ms = monotonic_ms();

    let mut cpu_sample_start = read_cpu_cycles();

    let mut cpu_sample_total_cycles = 0u64;

    let mut cpu_sample_active_cycles = 0u64;

    let mut last_ap_heartbeats = [0u64; arch::x86_64::smp::MAX_CPUS];

    for c in 0..arch::x86_64::smp::MAX_CPUS {

        last_ap_heartbeats[c] = arch::x86_64::smp::heartbeat(c);

    }

    let mut music_note: usize = 0;

    let mut speaker_on = false;

    let mut last_title_idx: Option<usize> = None;

    let mut last_title_click_ms: u64 = 0;

    // Not Defteri metin secimi surukleme durumu.

    let mut notepad_dragging = false;
    let mut browser_address_dragging = false;
    let mut start_search_dragging = false;

    let mut image_drag_last: Option<(usize, usize)> = None;

    let mut screenshot_counter: usize = 1;

    let mut run_dialog_open = false;

    let mut run_dialog_input = gui::TextBuf::new();



    // Dosya / Klasor özellikler (Properties) Modali

    let mut properties_open = false;

    let mut properties_name = gui::TextBuf::new();

    let mut properties_path = gui::TextBuf::new();

    let mut properties_type = gui::TextBuf::new();

    let mut properties_size = gui::TextBuf::new();

    let mut properties_location = gui::TextBuf::new();



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

                             pkg: &apps::pkgmgr::PackageManagerApp,

                             now: &drivers::rtc::DateTime,

                             mouse: &drivers::mouse::MouseState,

                             kbd: &drivers::keyboard::KeyboardState,

                             total_ram_bytes: u64,

                             used_ram_bytes: u64,

                             tick: usize,

                             app_render_cycles: &mut [u64; gui::MAX_WINDOWS],

                             run_dialog_state: (bool, &gui::TextBuf),

                             properties_state: (bool, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf),

                             snap_preview: Option<(usize, usize, usize, usize)>,

                             dirty_rect: Option<(usize, usize, usize, usize)>| {

        // Kirli dikdortgen verildiyse yalnizca bu bolge cizilip blit edilir;

        // aksi halde tum ekran yeniden cizilir.

        fb.set_frame_clip(dirty_rect);

        // 1. Masaustu Arka Plani

        compositor.render_desktop_bg(fb);

        // /desktop icerigini masaustu simgeleri olarak ciz.

        for i in 0..desktop_fm.file_count {

            let item = &desktop_fm.files[i];

            let (x, y, w, h) = desktop_item_rect(i);

            if !desktop_item_visible((x, y, w, h), fb.width, fb.height) { continue; }

            if desktop_fm.is_row_selected(i) {

                fb.draw_rounded_rect_alpha(x, y, w, h, 5, gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28), 160);

                fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::ACCENT_BLUE);

            } else {

                fb.draw_rounded_rect_alpha(x, y, w, h, 5, gui::MinGuiCompositor::shade(gui::Color::WINDOW_BG, 8), 128);

                fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::BORDER);

            }

            if item.is_dir { apps::filemanager::FileManagerApp::draw_folder_icon(fb, x + 5, y + 5, false); }

            else { apps::filemanager::FileManagerApp::draw_file_icon(fb, x + 5, y + 5, item.icon_tag); }

            let item_fg = if desktop_fm.is_row_selected(i) {

                gui::MinGuiCompositor::text_on(gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28))

            } else {

                gui::Color::TEXT_MAIN

            };

            gui::MinGuiCompositor::draw_string_clipped(fb, x + 30, y + 10, item.display_name(), item_fg, (x + 30, y + 2, w - 34, h - 4));

        }



        // 2. Acik pencereleri z-sirasina gore (arkadan one) ciz

        let (order, count) = compositor.draw_order();

        for k in 0..count {

            let idx = order[k];

            if let Some(win) = compositor.windows[idx] {

                if !win.is_open || win.is_minimized {

                    continue;

                }

                // Off-screen pencereleri tamamen atla (cizim maliyeti yok).

                if win.x >= fb.width || win.y >= fb.height

                    || win.x.saturating_add(win.width) == win.x

                    || win.y.saturating_add(win.height) == win.y {

                    continue;

                }

                // Kirli bolgeyle kesismeyen pencereyi hic cizme (golge payi dahil).

                if let Some((dx, dy, dw, dh)) = dirty_rect {

                    let wx0 = win.x.saturating_sub(8);

                    let wy0 = win.y.saturating_sub(8);

                    let wx1 = win.x.saturating_add(win.width).saturating_add(8);

                    let wy1 = win.y.saturating_add(win.height).saturating_add(8);

                    let overlaps = dx < wx1 && wx0 < dx.saturating_add(dw)

                        && dy < wy1 && wy0 < dy.saturating_add(dh);

                    if !overlaps {

                        continue;

                    }

                }

                compositor.render_window_frame(fb, &win, mouse, idx);



                // Icerik alanini pencere cercevesine kirp (tasma onleme).

                let (cx, cy, cw, ch) = win.content_rect();

                fb.set_clip(cx, cy, cw, ch);



                // Uygulamaya gore olculen icerik boyutu (kaydirma cubugu gerekli mi?).

                let app_id = compositor.app_id_for_window(idx);

                let content_h = window_content_h(app_id, win.height, win.width, fm, np, br);

                let content_w = cw;

                let want_v = content_h > ch;

                let want_h = content_w > cw;

                let v_gutter = if want_v { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };

                let h_gutter = if want_h { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };

                // Icerik alani (kaydirma cubuklari kadar daraltilmis).

                let content_area = (cx, cy, cw.saturating_sub(v_gutter), ch.saturating_sub(h_gutter));

                fb.set_clip(content_area.0, content_area.1, content_area.2.max(1), content_area.3.max(1));



                let render_start = read_cpu_cycles();

                match app_id {

                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll, kbd.ctrl),

                    1 => st.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout, now, total_ram_bytes,

                        cpu_vendor(), cpu_logical_count(),

                        if drivers::hda::is_ready() { if drivers::hda::is_dma_progressing() { gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)") } else { gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)") } } else { gui::ui_text("PC Speaker (PWM fallback)", "PC Speaker (PWM fallback)") },

                        if drivers::e1000::is_ready() { drivers::e1000::nic_name() } else { gui::ui_text("Bulunamadı", "Not found") },

                        if drivers::e1000::is_ready() { gui::ui_text("Hazir", "Ready") } else { gui::ui_text("Başlatılamadı / aygıt yok", "Initialization failed / device missing") }),

                    2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick, &win.scroll),

                    3 => br.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),

                    4 => iv.render_content(fb, win.x, win.y, win.width, win.height),

                    5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),

                    6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),

                    7 => tm.render_content(fb, win.x, win.y, win.width, win.height),

                    8 => calc.render_content(fb, win.x, win.y, win.width, win.height),

                    9 => if let Some(app) = paint[idx].as_ref() { app.render_content(fb, win.x, win.y, win.width, win.height) },

                    10 => term.render_content(fb, win.x, win.y, win.width, win.height, win.is_focused),

                    11 => inst.render_content(fb, win.x, win.y, win.width, win.height),

                    12 => v3d.render_content(fb, win.x, win.y, win.width, win.height),

                    13 => pkg.render_content(fb, win.x, win.y, win.width, win.height),

                    14 => apps::controlpanel::render_content(fb, win.x, win.y, win.width, win.height, now, total_ram_bytes, used_ram_bytes, kbd.layout),

                    _ => {}

                }

                app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);



                // Kirpmayi kaldir ve kaydirma cubuklarini pencerenin icinde ciz.

                fb.reset_clip();

                let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;

                if want_v {

                    let v_h = ch.saturating_sub(if want_h { sb } else { 0 });

                    if v_h > 0 {

                        // Saga tam dayali dikey kaydirma cubugu

                        let track = (cx + cw - sb, cy, sb, v_h);

                        let g = crate::gui::scrollbar::v_geom(track, content_h, v_h, win.scroll.y);

                        crate::gui::scrollbar::draw_vertical(fb, &g);

                    }

                }

                if want_h {

                    let h_w = cw.saturating_sub(if want_v { sb } else { 0 });

                    if h_w > 0 {

                        // Alta tam dayali yatay kaydirma cubugu

                        let track = (cx, cy + ch - sb, h_w, sb);

                        let g = crate::gui::scrollbar::h_geom(track, content_w, h_w, win.scroll.x);

                        crate::gui::scrollbar::draw_horizontal(fb, &g);

                    }

                }

            }

        }

        fb.reset_clip();



        // Aero Snap onizleme cercevesi (Pencere ekrana yaslanirken yari saydam/kesikli gosterim)

        if let Some((sx, sy, sw, sh)) = snap_preview {

            fb.draw_rounded_rect_alpha(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE, 55);

            fb.draw_rounded_rect_outline(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE);

            fb.draw_rounded_rect_outline(sx + 1, sy + 1, sw.saturating_sub(2), sh.saturating_sub(2), 4, gui::Color::WHITE);

        }



        // 3. Gorev cubugu, sistem tepsisi, popup'lar ve fare imleci

        compositor.render_taskbar_and_overlays(

            fb, mouse, kbd, now, st.volume, st.is_muted, st.language.is_english(),

            drivers::e1000::is_ready(),

        );



        if fm.drag_active {

            fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);

        } else if desktop_fm.drag_active {

            desktop_fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);

        }



        // Dosya / Klasor özellikler (Properties) Modali

        // NOT: Gorev cubugu/katmanlarindan oNCE cizilir; boylece fare imleci

        // modalin ustunde gorunur ve modal imleci gizlemez.

        if properties_state.0 {

            let pw = 340usize.min(fb.width.saturating_sub(20));

            let ph = 260usize.min(fb.height.saturating_sub(40));

            let px = fb.width.saturating_sub(pw) / 2;

            let py = fb.height.saturating_sub(ph) / 2;



            // Golge ve modern kart govdesi

            fb.draw_shadow(px, py, pw, ph, 0x000000);

            fb.draw_rounded_rect(px, py, pw, ph, 8, gui::Color::WINDOW_BG);

            fb.draw_rounded_rect_outline(px, py, pw, ph, 8, gui::Color::ACCENT_BLUE);



            // Baslik cubugu

            fb.draw_rect(px, py, pw, 28, gui::Color::TITLE_BAR);

            let prop_title = gui::ui_text("özellikler", "Properties");

            gui::MinGuiCompositor::draw_string(fb, px + 12, py + 7, prop_title, gui::Color::WHITE);



            // Kapatma butonu (Sag ust 'X')

            let close_r = (px + pw.saturating_sub(24), py + 6, 16, 16);

            let mx = mouse.x as usize;

            let my = mouse.y as usize;

            let close_hover = gui::in_rect(mx, my, close_r);

            fb.draw_rect(close_r.0, close_r.1, close_r.2, close_r.3, if close_hover { gui::Color::ACCENT_RED } else { gui::Color::TITLE_BAR });

            gui::MinGuiCompositor::draw_string(fb, close_r.0 + 4, close_r.1 + 1, "x", gui::Color::WHITE);



            // Dosya / Klasor Ikonu

            gui::MinGuiCompositor::draw_gear_icon(fb, px + 18, py + 38);



            // oge Adi

            let item_name = properties_state.1.as_str();

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 44, py + 42, item_name, gui::Color::TEXT_MAIN, (px + 44, py + 38, pw.saturating_sub(54), 20));



            // Ayirici cizgi

            fb.draw_rect(px + 14, py + 66, pw.saturating_sub(28), 1, gui::Color::BORDER);



            // Detay Satirlari

            let label_col = gui::Color::TEXT_MUTED;

            let val_col = gui::Color::TEXT_MAIN;

            let start_y = py + 76;

            let row_gap = 26;



            // Tur:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y, gui::ui_text("Tur:", "Type:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y, properties_state.2.as_str(), val_col, (px + 90, start_y, pw.saturating_sub(100), 20));



            // Boyut:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap, gui::ui_text("Boyut:", "Size:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap, properties_state.3.as_str(), val_col, (px + 90, start_y + row_gap, pw.saturating_sub(100), 20));



            // Konum:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap * 2, gui::ui_text("Konum:", "Location:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap * 2, properties_state.4.as_str(), val_col, (px + 90, start_y + row_gap * 2, pw.saturating_sub(100), 20));



            // Tam Yol:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap * 3, gui::ui_text("Tam Yol:", "Full Path:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap * 3, properties_state.5.as_str(), val_col, (px + 90, start_y + row_gap * 3, pw.saturating_sub(100), 20));



            // Tamam (Kapat) Butonu

            let ok_btn_w = 76;

            let ok_btn_h = 24;

            let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;

            let ok_btn_y = py + ph.saturating_sub(34);

            let ok_hover = gui::in_rect(mx, my, (ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h));

            fb.draw_rounded_rect(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, if ok_hover { gui::Color::ACCENT_BLUE } else { gui::Color::TITLE_BAR });

            fb.draw_rounded_rect_outline(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, gui::Color::BORDER);

            let ok_text = gui::ui_text("Tamam", "OK");

            let tw = gui::MinGuiCompositor::text_width(ok_text, 1);

            gui::MinGuiCompositor::draw_string(fb, ok_btn_x + (ok_btn_w.saturating_sub(tw)) / 2, ok_btn_y + 4, ok_text, gui::Color::WHITE);

        }



        // Hizli calistir (Run Dialog: Super+R) Diyalogu

        if run_dialog_state.0 {

            let dw = 320usize;

            let dh = 110usize;

            let dx = fb.width.saturating_sub(dw) / 2;

            let dy = fb.height.saturating_sub(dh) / 2;

            fb.draw_shadow(dx, dy, dw, dh, 0x000000);

            fb.draw_rounded_rect(dx, dy, dw, dh, 6, gui::Color::WINDOW_BG);

            fb.draw_rounded_rect_outline(dx, dy, dw, dh, 6, gui::Color::ACCENT_BLUE);

            fb.draw_rect(dx, dy, dw, 24, gui::Color::TITLE_BAR);

            gui::MinGuiCompositor::draw_string(fb, dx + 10, dy + 6,

                gui::ui_text("çalıştır (Run)", "Run Program"), gui::Color::WHITE);

            gui::MinGuiCompositor::draw_string(fb, dx + 12, dy + 34,

                gui::ui_text("Komut veya uygulama adı:", "Command or app name:"), gui::Color::TEXT_MUTED);

            // Giris kutusu

            let bx = dx + 12;

            let by = dy + 54;

            let bw = dw - 24;

            let bh = 24;

            fb.draw_rect(bx, by, bw, bh, gui::Color::DARK_BG);

            fb.draw_rect_outline(bx, by, bw, bh, gui::Color::BORDER);

            let input_text = run_dialog_state.1.as_str();

            gui::MinGuiCompositor::draw_string(fb, bx + 6, by + 6, input_text, gui::Color::TEXT_MAIN);

            // Imlec

            let cur_x = bx + 6 + gui::MinGuiCompositor::text_width(input_text, 1);

            if (tick / 20) % 2 == 0 {

                fb.draw_rect(cur_x, by + 4, 2, 16, gui::Color::ACCENT_YELLOW);

            }

            // Ipucu

            gui::MinGuiCompositor::draw_string(fb, dx + 12, dy + 88,

                gui::ui_text("cmd, not, ayar, task, calc, web  |  [Enter] Aç  [Esc] çık",

                              "cmd, not, ayar, task, calc, web  |  [Enter] Open  [Esc] Cancel"),

                gui::Color::TEXT_MUTED);

        }



        // 4. Backbuffer'i VRAM'e kopyala.

        //    Kirli dikdortgen verildiyse yalnizca o bolge blit edilir ve mavi

        //    isik filtresi de sadece o bolgeye uygulanir; boylece kalan bolge

        //    onceki karedeki (filtrelenmis) hâlini korur.

        let nl = if st.night_light { st.night_light_level } else { 0 };

        let region = fb.base_clip();

        if dirty_rect.is_some() {

            fb.apply_night_light_rect(nl, region);

            fb.flush_rect((region.x0, region.y0, region.x1 - region.x0, region.y1 - region.y0));

        } else {

            fb.apply_night_light(nl);

            fb.flush();

        }

        fb.clear_frame_clip();

    };



    // Karsilama bildirimi (Modern Toast)

    let welcome_title = gui::ui_text("minOs'a Hoş Geldiniz", "Welcome to minOs");

    let welcome_msg = gui::ui_text("Sistem hazır. Başlat menüsü ve kısayollar etkin.", "System is ready. Start menu and shortcuts active.");

    compositor.push_toast(gui::ToastKind::Info, welcome_title, welcome_msg, monotonic_ms(), 5500);



    // Ilk kareyi ciz (tum ekran)

    render_full_frame(

        &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,

        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,

        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app, &installer_app, &viewer3d_app, &pkgmgr_app, &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, memory_mgr.pmm.used_memory_bytes, anim_tick,

        &mut app_render_cycles,

        (run_dialog_open, &run_dialog_input),

        (properties_open, &properties_name, &properties_type, &properties_size, &properties_location, &properties_path),

        None,

        None,

    );



    let mut drag_prev_box: Option<(usize, usize, usize, usize)> = None;

    let mut drag_dirty_rect: Option<(usize, usize, usize, usize)> = None;

    let mut _app_dirty_rect: Option<(usize, usize, usize, usize)> = None;



    // 7. Ana Masaustu Etkilesim ve Olay Dongusu

    serial_print("[BOOT] Masaüstü döngüsü başladı.\n");

    loop {

        let loop_start_cycles = read_cpu_cycles();

        mouse.speed = settings_app.mouse_speed;

        mouse.reverse_scroll = settings_app.reverse_scroll;

        let mut input_update = drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);

        let usb_input_update = drivers::usb_hid::poll_input(

            &mut *mouse,

            &mut *keyboard,

            width as i32,

            height as i32,

            &mut memory_mgr.pmm,

        );

        input_update.keyboard |= usb_input_update.keyboard;

        input_update.mouse |= usb_input_update.mouse;

        loop_counter = loop_counter.wrapping_add(1);

        let mx = mouse.x as usize;

        let my = mouse.y as usize;

        gui::MinGuiCompositor::set_pointer_state(mx, my, mouse.left_button);

        let mouse_clicked = mouse.left_button && !last_buttons.0;

        let mouse_released = !mouse.left_button && last_buttons.0;

        let right_clicked = mouse.right_button && !last_buttons.1;

        let l = gui::MinGuiCompositor::tray_layout(width, height);

        let tb_y = l.tb_y;



        // RTC saatini islem dongusu sayisina gore degil, gecen gercek zamana gore yenile.

        let mut force_render = false;

        // USB Hotplug (tak/çıkar) bildirimleri ve donanım envanteri canlı güncellemesi
        while let Some(ev) = drivers::usb_hid::pop_hotplug_event() {
            force_render = true;
            let is_en = settings_app.language.is_english();
            let action_str = match (ev.action, is_en) {
                (drivers::usb_hid::HotplugAction::Connected, false) => "takıldı",
                (drivers::usb_hid::HotplugAction::Connected, true) => "connected",
                (drivers::usb_hid::HotplugAction::Disconnected, false) => "çıkarıldı",
                (drivers::usb_hid::HotplugAction::Disconnected, true) => "disconnected",
            };
            let dev_str = match (ev.kind, is_en) {
                (drivers::usb_hid::HidKind::Keyboard, false) => "USB Klavye",
                (drivers::usb_hid::HidKind::Keyboard, true) => "USB Keyboard",
                (drivers::usb_hid::HidKind::Mouse, false) => "USB Fare",
                (drivers::usb_hid::HidKind::Mouse, true) => "USB Mouse",
            };
            let toast_title = if is_en { "USB Device Event" } else { "USB Aygıt Olayı" };
            let mut toast_msg = [0u8; 96];
            let mut msg_len = 0;
            for b in dev_str.as_bytes() { if msg_len < toast_msg.len() { toast_msg[msg_len] = *b; msg_len += 1; } }
            if msg_len < toast_msg.len() { toast_msg[msg_len] = b' '; msg_len += 1; }
            for b in action_str.as_bytes() { if msg_len < toast_msg.len() { toast_msg[msg_len] = *b; msg_len += 1; } }
            let msg_text = core::str::from_utf8(&toast_msg[..msg_len]).unwrap_or("USB Event");
            compositor.push_toast(
                if ev.action == drivers::usb_hid::HotplugAction::Connected { gui::ToastKind::Success } else { gui::ToastKind::Info },
                toast_title,
                msg_text,
                monotonic_ms(),
                3500,
            );
        }
        if drivers::usb_hid::check_and_clear_hotplug_dirty() {
            force_render = true;
        }

        let clock_now_ms = monotonic_ms();

        if clock_now_ms.wrapping_sub(last_clock_update_ms) >= 1_000 {

            last_clock_update_ms = clock_now_ms;

            now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());

            force_render = true;

        }



        // Toast bildirim surelerini guncelle

        let had_toasts = compositor.toasts.iter().any(|t| t.is_some());

        compositor.update_toasts(clock_now_ms);

        let has_toasts = compositor.toasts.iter().any(|t| t.is_some());

        if had_toasts != has_toasts {

            force_render = true;

        }



        // Ses cihazini ayarlarla senkronla

        audio_dev.set_volume(settings_app.volume);

        audio_dev.is_muted = settings_app.is_muted;

        musicplayer_app.volume = settings_app.volume as usize;



        // 1. Fare Sag Tik — tiklanan yere gore baglam menusu

        if right_clicked {

            compositor.is_calendar_open = false;

            compositor.is_volume_popup_open = false;

            compositor.close_start_menu();

            compositor.is_context_menu_open = true;

            compositor.is_context_submenu_open = false;

            compositor.context_menu_x = mx;

            compositor.context_menu_y = my;

            compositor.context_file_idx = None;

            compositor.context_window = None;



            // Tiklanan pencereyi en ustten asagiya ara

            let (order, count) = compositor.draw_order();

            let mut hit_window: Option<(usize, gui::ContextKind)> = None;

            for k in (0..count).rev() {

                let idx = order[k];

                if let Some(win) = compositor.windows[idx] {

                    if !win.is_open || win.is_minimized {

                        continue;

                    }

                    let inside = mx >= win.x && mx < win.x + win.width

                        && my >= win.y && my < win.y + win.height;

                    if !inside {

                        continue;

                    }

                    compositor.context_window = Some(idx);

                    if my < win.y + gui::titlebar_h() {

                        hit_window = Some((idx, gui::ContextKind::Window));

                    } else if idx == 2 {

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::Notepad));

                    } else if idx == 0 {

                        if let Some(i) = filemanager_app.hit_test(win.x, win.y, win.width, win.height, mx, my, win.scroll.y) {

                            compositor.context_file_idx = Some(i);

                            filemanager_app.selected_idx = i;

                        }

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::FileManager));

                    } else if idx == 7 {

                        if taskmgr_app.active_tab == 0 {

                            if let Some(row) = taskmgr_app.row_at(mx, my, win.x, win.y, win.width) {

                                taskmgr_app.selected = row;

                                taskmgr_app.focused_control = 4 + row;

                                hit_window = Some((idx, gui::ContextKind::TaskManager));

                            } else {

                                hit_window = Some((idx, gui::ContextKind::Window));

                            }

                        } else {

                            hit_window = Some((idx, gui::ContextKind::Window));

                        }

                        compositor.focus_window(idx);

                    } else {

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::Window));

                    }

                    break;

                }

            }



            if let Some((_, kind)) = hit_window {

                compositor.context_kind = kind;

            } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

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

                    compositor.context_kind = gui::ContextKind::DesktopItem;

                } else {

                    compositor.context_kind = gui::ContextKind::Desktop;

                }

            } else {

                compositor.is_context_menu_open = false;

            }

        }



        // 2. Fare Sol Tiklama Olaylari (Mouse Down)

        if mouse_clicked {

            compositor.finish_alt_tab();

            let mut consumed = false;



            // Acik menu/popup disina tiklaninca kapat; tiklamayi tuketmeden

            // birak ki ayni tiklama alttaki pencere veya gorev cubuguna ulassin.

            if compositor.is_start_menu_open {

                let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);

                let submenu = gui::MinGuiCompositor::start_menu_submenu_rect(

                    menu_y, compositor.active_start_group, tb_y,

                );

                if !gui::in_rect(mx, my, (menu_x, menu_y, menu_w + 6, menu_h))

                    && !gui::in_rect(mx, my, submenu)

                {

                    compositor.close_start_menu();

                }

            }

            if compositor.is_context_menu_open {

                let hit_main = compositor.context_hit(width, height, mx, my).is_some();

                let hit_sub = compositor.context_submenu_hit(width, height, mx, my).is_some();

                if !hit_main && !hit_sub {

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                }

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

                            if taskmgr_app.actions_menu_open {
                                let in_menu = compositor.windows[7].map_or(false, |w| {
                                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::actions_menu_rect(w.x, w.y, w.width))
                                });
                                let in_button = compositor.windows[7].map_or(false, |w| {
                                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::actions_button_rect(w.x, w.y, w.width))
                                });
                                if !in_menu && !in_button { taskmgr_app.actions_menu_open = false; }
                            }

            if run_dialog_open {

                let rbox = (width.saturating_sub(320) / 2, height.saturating_sub(110) / 2, 320, 110);

                if !gui::in_rect(mx, my, rbox) {

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else {

                    consumed = true;

                }

            }

            // özellikler (Properties) modali: modal acikken tum tiklamalari modal ustlenir.

            // X / Tamam butonlari ve modal disina tiklama pencereyi kapatir.

            if properties_open {

                let pw = 340usize.min(width.saturating_sub(20));

                let ph = 260usize.min(height.saturating_sub(40));

                let px = width.saturating_sub(pw) / 2;

                let py = height.saturating_sub(ph) / 2;

                let close_r = (px + pw.saturating_sub(24), py + 6, 16, 16);

                let ok_btn_w = 76usize;

                let ok_btn_h = 24usize;

                let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;

                let ok_btn_y = py + ph.saturating_sub(34);

                let on_close = gui::in_rect(mx, my, close_r);

                let on_ok = gui::in_rect(mx, my, (ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h));

                if on_close || on_ok || !gui::in_rect(mx, my, (px, py, pw, ph)) {

                    properties_open = false;

                    properties_name.clear();

                    properties_type.clear();

                    properties_size.clear();

                    properties_location.clear();

                    properties_path.clear();

                }

                consumed = true;

                force_render = true;

            }

            if !consumed && compositor.toast_click(width, tb_y, mx, my) {

                consumed = true;

                force_render = true;

            }

            // Not Defteri metin alani: imleci konumlandir, cift tikta kelime sec,

            // tek tikta secimi temizle ve surukleyerek secimi baslat.

            if let Some((_, w)) = focused_window_for_app(&compositor, 2) {

                if notepad_app.menu != 0 {

                    let in_menu_box = gui::in_rect(

                        mx, my,

                        (

                            w.x + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu, w.width),

                            w.y + crate::apps::notepad::NotepadApp::menu_box_y(),

                            crate::apps::notepad::NotepadApp::menu_box_w(),

                            crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu),

                        ),

                    );

                    let overflow = crate::apps::notepad::NotepadApp::overflow_button_rect(w.width);
                    let in_tabs = gui::in_rect(mx, my, (w.x + overflow.0, w.y + 30 + overflow.1, overflow.2, overflow.3));

                    if !in_menu_box && !in_tabs {

                        // Menu acikken disari tiklandi -> menuyu kapat, alttaki metni tiklama

                        notepad_app.menu = 0;

                        notepad_dragging = false;

                        consumed = true;

                    }

                    // Menu kutusu veya sekmeler tiklanmissa pencere olay isleyicisi isleyecek

                } else if w.is_focused

                    && gui::in_rect(mx, my, (w.x + 6, w.y + 58, w.width.saturating_sub(12), w.height.saturating_sub(86)))

                {

                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);

                    let now_ms = monotonic_ms() as usize;
                    let click_count = notepad_app.record_click(
                        now_ms,
                        (mx, my),
                        settings_app.double_click_ms as usize,
                    );

                    if click_count == 3 {

                        notepad_app.select_paragraph_at(at);

                    } else if click_count == 2 {

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

                    consumed = true;

                }

            }



            // A) Takvim acikken

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



            // B) Ses paneli acikken

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



            // C) Baglam menusu acikken

            if !consumed && compositor.is_context_menu_open {

                // once varsa acik olan "Yeni ▶" alt menusundeki tiklamayi kontrol et

                if let Some(sub_item) = compositor.context_submenu_hit(width, height, mx, my) {

                    match compositor.context_kind {

                        gui::ContextKind::Desktop | gui::ContextKind::DesktopItem => {

                            match sub_item {

                                0 => { // Metin Belgesi (.txt)

                                    desktop_files.new_file(&mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                1 => { // Klasor

                                    desktop_files.new_folder(&mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                2 => { // cizim / Resim (.bmp)

                                    desktop_files.new_custom_file("cizim_", ".bmp", &[], &mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                _ => { // Bos Belge

                                    desktop_files.new_custom_file("belge_", ".dat", &[], &mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                            }

                        }

                        gui::ContextKind::FileManager => {

                            match sub_item {

                                0 => { // Metin Belgesi (.txt)

                                    filemanager_app.new_file(&mut fs_mgr.vfs);

                                }

                                1 => { // Klasor

                                    filemanager_app.new_folder(&mut fs_mgr.vfs);

                                }

                                2 => { // cizim / Resim (.bmp)

                                    filemanager_app.new_custom_file("cizim_", ".bmp", &[], &mut fs_mgr.vfs);

                                }

                                _ => { // Bos Belge

                                    filemanager_app.new_custom_file("belge_", ".dat", &[], &mut fs_mgr.vfs);

                                }

                            }

                        }

                        _ => {}

                    }

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                    consumed = true;

                } else if let Some(clicked_item) = compositor.context_hit(width, height, mx, my) {

                    // "Yeni ▶" ogesine tiklandiysa alt menuyu ac / kapa, ana menuyu kapatma

                    let is_sub_parent = match compositor.context_kind {

                        gui::ContextKind::Desktop => clicked_item == 1,

                        gui::ContextKind::DesktopItem => clicked_item == 4,

                        gui::ContextKind::FileManager => clicked_item == 5,

                        _ => false,

                    };



                    if is_sub_parent {

                        compositor.is_context_submenu_open = !compositor.is_context_submenu_open;

                        consumed = true;

                    } else {

                        match compositor.context_kind {

                            gui::ContextKind::Desktop => match clicked_item {

                                0 => { filemanager_app.set_path("/root/Documents"); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0); }

                                2 => { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                                3 => open_app(&mut compositor, 1),

                                4 => open_app(&mut compositor, 7),

                                5 => settings_app.cycle_theme(),

                                6 => { desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                                7 => open_app(&mut compositor, 2),

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

                                    4 => {} // Yeni ▶

                                    5 => desktop_files.begin_rename(),

                                    6 => { desktop_files.delete_selected(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },

                                    _ => {}

                                }

                            }

                            gui::ContextKind::FileManager => {

                                match clicked_item {

                                    0 => { // Ac / Klasore Gir

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                                if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                                    let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                    if let Some(data) = data {

                                                        notepad_app.open_file(&path, data);

                                                    }

                                                    open_app(&mut compositor, 2);

                                                } else if tag == "IMG" {

                                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                                    open_app(&mut compositor, 4);

                                                } else if tag == "SND" {

                                                    open_app(&mut compositor, 5);

                                                } else if tag == "VID" {

                                                    open_app(&mut compositor, 6);

                                                } else if tag == "WEB" {

                                                    open_app(&mut compositor, 3);

                                                } else if tag == "3D" {

                                                    let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                                    if let Some(data) = data {

                                                        viewer3d_app.load_file(&path, data);

                                                    }

                                                    open_app(&mut compositor, 12);

                                                }

                                            }

                                        }

                                    }

                                    1 => { // Not Defterinde Duzenle

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            let path = filemanager_app.selected_full_path();

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                notepad_app.open_file(&path, data);

                                            }

                                            open_app(&mut compositor, 2);

                                        }

                                    }

                                    2 => { filemanager_app.copy_selection(&mut file_clipboard, true); }

                                    3 => { filemanager_app.copy_selection(&mut file_clipboard, false); }

                                    4 => { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                                    5 => {} // Yeni ▶

                                    6 => { // Adini degistir

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            filemanager_app.begin_rename();

                                        }

                                    }

                                    7 => { // Dosyayi / Klasoru Sil

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            filemanager_app.delete_selected(&mut fs_mgr.vfs);

                                        }

                                    }

                                    8 => { // özellikler

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            if file_idx < filemanager_app.file_count {

                                                let file = &filemanager_app.files[file_idx];

                                                properties_name.clear();

                                                properties_name.push_str(file.display_name());

                                                properties_type.clear();

                                                properties_type.push_str(file.file_type);

                                                properties_size.clear();

                                                properties_size.push_str(file.size_str.as_str());

                                                properties_location.clear();

                                                properties_location.push_str(filemanager_app.path());

                                                properties_path.clear();

                                                let full_path = filemanager_app.selected_full_path();

                                                properties_path.push_str(&full_path);

                                                properties_open = true;

                                            }

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

                                    // Sonlandir (Del): secili sureci kapat

                                    if let Some(target) = taskmgr_app.selected_app_idx() {

                                        taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                                        if let Some(wi) = compositor.context_window {

                                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[wi],

                                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

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

                        compositor.is_context_menu_open = false;

                        compositor.is_context_submenu_open = false;

                        consumed = true;

                    }

                } else {

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                    consumed = true;

                }

            }



        if compositor.is_start_menu_open {

            let (_, menu_y, _, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);

            for g in 0..5 {

                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);

                if gui::in_rect(mx, my, cat_r) {

                    compositor.active_start_group = g;

                    break;

                }

            }

        }



        // D) Baslat menusu acikken

        if !consumed && compositor.is_start_menu_open {

            let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);

            let search_rect = gui::MinGuiCompositor::start_search_rect(menu_x, menu_y, menu_w);

            let active_g = compositor.active_start_group;

            let mut sub_rect = gui::MinGuiCompositor::start_menu_submenu_rect(menu_y, active_g, tb_y);

            if compositor.start_search_len > 0 {

                let mut matches = [0; 15];

                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);

                sub_rect = gui::MinGuiCompositor::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));

            }

            if compositor.start_search_len > 0 && gui::in_rect(mx, my, sub_rect) {

                let mut matches = [0; 15];

                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);

                for (row, app_id) in matches[..count].iter().enumerate() {

                    let row_rect = (sub_rect.0 + gui::sp(6), sub_rect.1 + gui::sp(30) + row * gui::sp(28), sub_rect.2.saturating_sub(gui::sp(12)), gui::sp(25));

                    if gui::in_rect(mx, my, row_rect) {

                        if *app_id >= 100 {
                            let app_name = alloc::string::String::from(compositor.installed_app_name(*app_id - 100));
                            if !app_name.is_empty() {
                                desktop::helpers::launch_installed_pkg(
                                    &app_name,
                                    &mut compositor,
                                    &mut terminal_app,
                                    &mut notepad_app,
                                    &mut calculator_app,
                                    &mut paint_apps,
                                    &mut fs_mgr.vfs,
                                    &mut memory_mgr.pmm,
                                    &now,
                                );
                            }
                        } else {
                            open_app(&mut compositor, *app_id);
                        }

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

                compositor.start_search_click_at(
                    mx, my, search_rect.0 + 6, monotonic_ms(),
                    settings_app.double_click_ms as u64,
                );
                start_search_dragging = true;

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

            for g in 0..5 {

                if cat_clicked { break; }

                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);

                if gui::in_rect(mx, my, cat_r) {

                    compositor.active_start_group = g;

                    compositor.selected_start_item = 0;

                    compositor.start_menu_focus_submenu = true;

                    cat_clicked = true;

                    consumed = true;

                    break;

                }

            }



            if !cat_clicked {

                let count = if active_g == 3 {
                    1 + compositor.installed_apps_count
                } else if active_g == 4 {
                    2
                } else {
                    gui::MENU_GROUPS[active_g].2
                };

                let mut item_clicked = false;

                for k in 0..count {

                    let i = if active_g < 3 { gui::MENU_GROUPS[active_g].1 + k } else { 0 };

                    let rect = gui::MinGuiCompositor::start_menu_subitem_rect(menu_y, active_g, k, tb_y);

                    if gui::in_rect(mx, my, rect) {

                        item_clicked = true;

                        if active_g == 4 {

                            if k == 0 {

                                if compositor.confirm_shutdown {

                                    #[cfg(minos_boot_log)]

    serial_print("[minOs] Başlat menüsünden sistem kapatma onaylandı.\n");

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

                            if k == 1 {

                                if compositor.confirm_restart {

                                    restart_system();

                                }

                                compositor.confirm_restart = true;

                                compositor.confirm_shutdown = false;

                                break;

                            }

                        }

                        compositor.confirm_shutdown = false;

                        compositor.confirm_restart = false;

                        if active_g == 3 {
                            if k == 0 {
                                open_app(&mut compositor, 13);
                            } else {
                                let app_name = alloc::string::String::from(compositor.installed_app_name(k - 1));
                                if !app_name.is_empty() {
                                    desktop::helpers::launch_installed_pkg(
                                        &app_name,
                                        &mut compositor,
                                        &mut terminal_app,
                                        &mut notepad_app,
                                        &mut calculator_app,
                                        &mut paint_apps,
                                        &mut fs_mgr.vfs,
                                        &mut memory_mgr.pmm,
                                        &now,
                                    );
                                }
                            }
                        } else if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {

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



        // E) Sistem tepsisi dugmeleri

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

                    // Ethernet simgesi ag ayarlari sekmesini acar.

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



            // F) Gorev cubugu uygulama sekmeleri (font olcegine duyarli)

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

                        // +N alani gizli sekmeler arasinda sirayla dolasir.

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



            // G) Masaustu simgeleri

            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                // Yenileme tiklama durumunu sifirlar; cift tik tespiti icin koru

                // (yoksa her tiklama durumu siler ve cift tiklama hic algilanamaz).

                let saved_click_idx = desktop_files.last_click_idx;

                let saved_click_ms = desktop_files.last_click_ms;

                desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                desktop_files.last_click_idx = saved_click_idx;

                desktop_files.last_click_ms = saved_click_ms;

                if let Some((i, _)) = desktop_item_at(mx, my, &desktop_files, width, height) {

                    desktop_files.select_row(i, keyboard.ctrl, keyboard.shift);

                    compositor.active_window_idx = None;

                    let now_ms = monotonic_ms();

                    // Masaustunun kendi tiklama durumu kullanilir (dosya yoneticisi

                    // penceresinin durumu degil); aksi halde cift tik hic tutmaz.

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

                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&p));

                                if let Some(data) = data { notepad_app.open_file(&p, data); }

                                open_app(&mut compositor, 2);

                            } else if tag == "IMG" {

                                with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&p, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                open_app(&mut compositor, 4);

                            }

                            else if tag == "SND" { open_app(&mut compositor, 5); }

                            else if tag == "VID" { open_app(&mut compositor, 6); }

                            else if tag == "WEB" { open_app(&mut compositor, 3); }

                            else if tag == "3D" {

                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&p));

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

            // Masaustu sol serit hizli simgeleri (cizimle ayni kanonik geometri).

            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                let desk_apps = [0, 2, 3, 5, 1, 11];

                // Kurulu sistemde kaldirilan kurulum kisayolunu (app 11) atla.

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



            // H) Pencereler (baslik, butonlar, icerik)

            if !consumed {

                let (order, count) = compositor.draw_order();

                for k in (0..count).rev() {

                    let idx = order[k];

                    let win = match compositor.windows[idx] {

                        Some(w) => w,

                        None => continue,

                    };

                    let (wx, wy, ww, wh) = (win.x, win.y, win.width, win.height);

                    // Standart pencere davranisi: ustteki pencere govdesine tiklama odagi alir.

                    if gui::in_rect(mx, my, (wx, wy, ww, wh)) { compositor.focus_window(idx); }



                    // Window controls activate on release so their pressed state is visible.
                    let control = if gui::in_rect(mx, my, win.close_rect()) {
                        Some(0)
                    } else if gui::in_rect(mx, my, win.max_rect()) {
                        Some(1)
                    } else if gui::in_rect(mx, my, win.min_rect()) {
                        Some(2)
                    } else {
                        None
                    };
                    if let Some(control) = control {
                        pending_window_control = Some((idx, control));
                        compositor.focus_window(idx);
                        break;
                    }

                    // Dikey kaydirma cubugu (pencere sag kenari) — tiklama/surukleme.

                    {

                        let (ccx, ccy, ccw, cch) = win.content_rect();

                        let c_h = window_content_h(win.app_id as usize, wh, ww, &filemanager_app, &notepad_app, &browser_app);

                        let view_h = cch;

                        if c_h > view_h {

                            let t = gui::scrollbar::SCROLLBAR_THICKNESS;

                            // Saga tam dayali dikey scrollbar track'i

                            let track = (ccx + ccw - t, ccy, t, view_h);

                            if gui::in_rect(mx, my, track) {

                                let g = gui::scrollbar::v_geom(track, c_h, view_h, win.scroll.y);

                                // Tutamac yakinindaysa tutamacin ustunu koru, degilse aninda zipla.

                                if gui::in_rect(mx, my, g.thumb) {

                                    vscroll_drag = Some((idx, g.thumb.1.saturating_sub(my), g.thumb.3, track.3, c_h, view_h));

                                } else {

                                    // Track uzerinde tiklanan konumdan kaydirmayi hesapla.

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

                    // Pencere kenar ve koselerinden boyutlandirma. Scrollbar

                    // alani yukarida tuketildi; geri kalan kenar alani resize'a ait.

                    if !win.is_maximized {

                        if let Some(edge) = win.resize_edge_at(mx, my) {

                            compositor.focus_window(idx);

                            resizing_win = Some(idx);

                            resize_edge = Some(edge);

                            resize_orig = (wx, wy, ww, wh);

                            break;

                        }

                    }

                    // Baslik cubugu — surukle / cift tik ile maksimize

                    if gui::in_rect(mx, my, win.title_drag_rect()) {

                        let now_ms = monotonic_ms();

                        let dbl = last_title_idx == Some(idx)

                            && now_ms.wrapping_sub(last_title_click_ms) <= settings_app.double_click_ms as u64;

                        compositor.focus_window(idx);

                        if dbl {

                            compositor.toggle_maximize(idx, width, height);

                            dragging_win = None;

                            // cift tik tuketildikten sonra eslesmeyi temizle;

                            // ucuncu kisa tik yeni bir cift tik baslatabilsin.

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



                    // Pencere govdesi icindeki uygulama dugmeleri

                    if mx >= wx && mx < wx + ww && my >= wy && my < wy + wh {

                        let app_id = compositor.app_id_for_window(idx);

                        match app_id {

                            0 => {

                                let menu_button = apps::filemanager::FileManagerApp::actions_button_rect(wx, wy, ww);
                                let mut menu_action = None;
                                if gui::in_rect(mx, my, menu_button) {
                                    filemanager_app.actions_menu_open = !filemanager_app.actions_menu_open;
                                } else if filemanager_app.actions_menu_open {
                                    menu_action = (0..7).find(|&i| gui::in_rect(mx, my,
                                        apps::filemanager::FileManagerApp::actions_menu_item_rect(wx, wy, ww, i)));
                                    filemanager_app.actions_menu_open = false;
                                }

                                if gui::in_rect(mx, my, menu_button) {

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::search_rect(wx, wy, ww)) {

                                    filemanager_app.focused_control = 5;

                                } else if menu_action == Some(0) {

                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                } else if menu_action == Some(1) {

                                    filemanager_app.new_file(&mut fs_mgr.vfs);

                                } else if menu_action == Some(2) {

                                    filemanager_app.new_folder(&mut fs_mgr.vfs);

                                } else if menu_action == Some(3) {

                                    filemanager_app.delete_selected(&mut fs_mgr.vfs);

                                } else if menu_action == Some(4) {

                                    // Geri Donusum Kutusu'ndayken "Ac" dugmesi ogeyi geri yukler.

                                    if filemanager_app.path() == crate::fs::vfs::TRASH_DIR {

                                        filemanager_app.restore_selected(&mut fs_mgr.vfs);

                                    } else if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                        if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                notepad_app.open_file(&path, data);

                                            }

                                            open_app(&mut compositor, 2);

                                        } else if tag == "IMG" {

                                            with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                            open_app(&mut compositor, 4);

                                        } else if tag == "SND" {

                                            open_app(&mut compositor, 5);

                                        } else if tag == "VID" {

                                            open_app(&mut compositor, 6);

                                        } else if tag == "WEB" {

                                            open_app(&mut compositor, 3);

                                        } else if tag == "3D" {

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                viewer3d_app.load_file(&path, data);

                                            }

                                            open_app(&mut compositor, 12);

                                        }

                                    }

                                } else if menu_action == Some(5) {

                                    filemanager_app.toggle_view_mode();

                                    filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if menu_action == Some(6) {

                                    filemanager_app.toggle_preview();

                                    filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    filemanager_app.update_preview(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_name_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_size_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_type_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

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

                                        if my >= list_top && my < list_bottom && gui::in_rect(mx, my, filemanager_app.item_rect(wx, wy, ww, i, sy)) {

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

                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                        if let Some(data) = data {

                                                            notepad_app.open_file(&path, data);

                                                        }

                                                        open_app(&mut compositor, 2);

                                                    } else if tag == "IMG" {

                                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                                        open_app(&mut compositor, 4);

                                                    } else if tag == "SND" {

                                                        open_app(&mut compositor, 5);

                                                    } else if tag == "VID" {

                                                        open_app(&mut compositor, 6);

                                                    } else if tag == "WEB" {

                                                        open_app(&mut compositor, 3);

                                                    } else if tag == "3D" {

                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

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

                                // Sistem Ayarlari

                                for t in 0..5 {

                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::tab_rect(wx, wy, t)) {

                                        settings_app.active_tab = t;

                                    }

                                }

                                if gui::in_rect(mx, my, apps::settings::SystemSettings::save_btn_rect(wx, wy, wh)) {

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Ayarlar kaydediliyor...", "Saving settings..."), || settings_app.save_all(&compositor, &mut fs_mgr.vfs));

                                } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh)) {

                                    settings_app.reset_defaults();

                                    settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now);

                                    settings_app.write_config(&mut fs_mgr.vfs);

                                } else if settings_app.active_tab == 2 && (0..4).any(|octet| gui::in_rect(mx, my, apps::settings::SystemSettings::static_ip_octet_rect(wx, wy, octet))) {

                                    for octet in 0..4 {
                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::static_ip_octet_rect(wx, wy, octet)) {
                                            settings_app.static_ip_octet = octet;
                                            settings_app.static_ip_value = 0;
                                            settings_app.static_ip_digits = 0;
                                            settings_app.static_ip_edit = true;
                                            settings_app.dhcp_enabled = false;
                                            settings_app.focused_control = 7 + octet;
                                            break;
                                        }
                                    }

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::dhcp_btn_rect(wx, wy)) {

                                    settings_app.static_ip_edit = false;

                                    settings_app.dhcp_enabled = !settings_app.dhcp_enabled;

                                    settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_net_btn_rect(wx, wy)) {

                                    settings_app.static_ip_edit = false;

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Ağ testi yapılıyor...", "Running network test..."), || settings_app.run_network_test());

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_ping_btn_rect(wx, wy)) {

                                    settings_app.static_ip_edit = false;

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Ping gönderiliyor...", "Sending ping..."), || settings_app.run_ping_test());

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_http_btn_rect(wx, wy)) {

                                    settings_app.static_ip_edit = false;

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("HTTP bağlantısı test ediliyor...", "Testing HTTP connection..."), || settings_app.run_http_test());

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

                                        let track = apps::settings::SystemSettings::volume_track_rect(wx, wy); settings_app.set_volume_from_range(mx, track.0, track.2);

                                    }

                                }

                                if settings_app.active_tab == 1 {

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

                                    } else {

                                    let mut handled = false;
                                    for i in 0..3 {
                                        if gui::in_rect(mx,my,apps::settings::SystemSettings::zoom_btn_rect(wx,wy,i)) {
                                            settings_app.zoom = [100,125,150][i];
                                            gui::set_font_zoom(settings_app.zoom);
                                            settings_app.status_msg = gui::ui_text("Yazı yakınlaştırması değiştirildi.", "Font zoom changed.");
                                            handled = true;
                                            break;
                                        }
                                    }

                                    if !handled {
                                        for i in 0..7 {
                                            if gui::in_rect(mx,my,apps::settings::SystemSettings::resolution_btn_rect(wx,wy,i)) {
                                                let requested = match i { 0 => (800,600), 1 => (1024,768), 2 => (1280,720), 3 => (1920,1080), 4 => (1920,1200), 5 => (2560,1440), 6 => (2560,1600), _ => (1920,1080) };
                                                if requested == (fb.width, fb.height) {
                                                    settings_app.screen_res = requested;
                                                    settings_app.display_res = requested;
                                                    settings_app.status_msg = gui::ui_text("Bu çözünürlük zaten etkin.", "This resolution is already active.");
                                                } else if drivers::gpu::mode_supported(requested.0, requested.1) {
                                                    let (switched, pitch_opt) = drivers::gpu::try_set_resolution(requested.0, requested.1);
                                                    if switched {
                                                        let p = pitch_opt.unwrap_or(requested.0 * 4);
                                                        let new_bytes = p.saturating_mul(requested.1);
                                                        let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
                                                        for off in (0..new_bytes.max(16 * 1024 * 1024)).step_by(2 * 1024 * 1024) {
                                                            arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
                                                        }
                                                        if fb.resize(requested.0, requested.1, p) {
                                                            width = fb.width;
                                                            height = fb.height;
                                                                                                                        settings_app.screen_res = requested;
                                                            settings_app.display_res = requested;
                                                            crate::gui::wallpaper::apply(settings_app.wallpaper, settings_app.wall_color);
                                                            keep_windows_on_screen(&mut compositor, width, height);
                                                            force_render = true;
                                                            settings_app.status_msg = gui::ui_text("Çözünürlük değiştirildi (BGA).", "Resolution changed (BGA).");
                                                        } else {
                                                            settings_app.status_msg = gui::ui_text("Çözünürlük uygulanamadı (bellek sınırı).", "Resolution failed (memory limit).");
                                                        }
                                                    } else {
                                                        settings_app.screen_res = (fb.width, fb.height);
                                                        settings_app.status_msg = gui::ui_text("Çözünürlük açılışta Limine tarafından belirlenir.", "Resolution is set by Limine at boot.");
                                                    }
                                                }
                                                handled = true;
                                                break;
                                            }
                                        }
                                    }

                                    if !handled && gui::in_rect(mx,my,apps::settings::SystemSettings::night_light_rect(wx,wy)) {

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

                                    for i in 0..apps::settings::SystemSettings::wallpaper_count() {

                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::wallpaper_btn_rect(wx, wy, i)) {

                                            settings_app.wallpaper = i as u8;

                                            crate::gui::wallpaper::apply(settings_app.wallpaper, settings_app.wall_color);

                                            settings_app.status_msg = gui::ui_text("Arka plan değiştirildi.", "Background changed.");

                                        }

                                    }

                                    } // tema else kapatma

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

                                // Not Defteri: Dosya/Duzen/Yardim arac cubugu ve menu aksiyonlari

                                // Tiklama testleri, menu cizimiyle ayni geometri

                                // yardimcilarini (apps::notepad) kullanir; boylece font

                                // olcegi 100'den farkliyken de koordinatlar ortusur.

                                let overflow = crate::apps::notepad::NotepadApp::overflow_button_rect(ww);
                                let overflow_rect = (wx + overflow.0, wy + 30 + overflow.1, overflow.2, overflow.3);

                                if gui::in_rect(mx, my, overflow_rect) {

                                    notepad_app.menu = if notepad_app.menu == 4 { 0 } else { 4 };

                                } else if notepad_app.menu != 0

                                    && gui::in_rect(mx, my, (wx + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu, ww), wy + crate::apps::notepad::NotepadApp::menu_box_y(), crate::apps::notepad::NotepadApp::menu_box_w(), crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu)))

                                {

                                    let rel_y = my.saturating_sub(wy + crate::apps::notepad::NotepadApp::menu_box_y() + crate::apps::notepad::NotepadApp::menu_pad());

                                    let item_idx = rel_y / crate::apps::notepad::NotepadApp::menu_item_h();

                                    let items = crate::apps::notepad::NotepadApp::menu_items(notepad_app.menu);
                                    let menu_was_overflow = notepad_app.menu == 4;

                                    if let Some(def) = items.get(item_idx) {

                                        match def.action {
                                            crate::apps::notepad::NotepadMenuAction::FileCategory => notepad_app.menu = 1,
                                            crate::apps::notepad::NotepadMenuAction::EditCategory => notepad_app.menu = 2,
                                            crate::apps::notepad::NotepadMenuAction::HelpCategory => notepad_app.menu = 3,

                                            crate::apps::notepad::NotepadMenuAction::Open => {

                                                let path = alloc::string::String::from(notepad_app.file_path_str());

                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

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

                                    if !(menu_was_overflow && (1..=3).contains(&notepad_app.menu)) { notepad_app.menu = 0; }

                                } else if notepad_app.menu != 0 {

                                    // Menu acikken menu disina tiklandiginda menuyu kapat

                                    notepad_app.menu = 0;

                                } else if mx >= wx + ww.saturating_sub(130) && mx <= wx + ww.saturating_sub(74) && my >= wy + 32 && my <= wy + 52 {

                                    notepad_app.clear();

                                } else if mx >= wx + ww.saturating_sub(68) && mx <= wx + ww.saturating_sub(12) && my >= wy + 32 && my <= wy + 52 {

                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {

                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                    }

                                } else if notepad_app.find_active {
                                    let bar_r = apps::notepad::NotepadApp::find_bar_area_rect(ww, notepad_app.replace_active);
                                    if gui::in_rect(mx, my, (wx + bar_r.0, wy + bar_r.1, bar_r.2, bar_r.3)) {
                                        // Bul/Değiştir çubuğu içinde — düğme ve giriş kutuları.
                                        let close_r = apps::notepad::NotepadApp::find_close_rect(ww);
                                        if gui::in_rect(mx, my, (wx + close_r.0, wy + close_r.1, close_r.2, close_r.3)) {
                                            notepad_app.close_find();
                                        } else {
                                            let fin_r = apps::notepad::NotepadApp::find_input_rect();
                                            if gui::in_rect(mx, my, (wx + fin_r.0, wy + fin_r.1, fin_r.2, fin_r.3)) {
                                                notepad_app.find_focused_replace = false;
                                            } else {
                                                let nxt_r = apps::notepad::NotepadApp::find_next_rect();
                                                if gui::in_rect(mx, my, (wx + nxt_r.0, wy + nxt_r.1, nxt_r.2, nxt_r.3)) {
                                                    notepad_app.find_next(true);
                                                    notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                                                } else {
                                                    let prv_r = apps::notepad::NotepadApp::find_prev_rect();
                                                    if gui::in_rect(mx, my, (wx + prv_r.0, wy + prv_r.1, prv_r.2, prv_r.3)) {
                                                        notepad_app.find_next(false);
                                                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                                                    } else if notepad_app.replace_active {
                                                        let rin_r = apps::notepad::NotepadApp::replace_input_rect();
                                                        if gui::in_rect(mx, my, (wx + rin_r.0, wy + rin_r.1, rin_r.2, rin_r.3)) {
                                                            notepad_app.find_focused_replace = true;
                                                        } else {
                                                            let rpl_r = apps::notepad::NotepadApp::replace_btn_rect();
                                                            if gui::in_rect(mx, my, (wx + rpl_r.0, wy + rpl_r.1, rpl_r.2, rpl_r.3)) {
                                                                notepad_app.replace_current();
                                                                notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                                                            } else {
                                                                let rall_r = apps::notepad::NotepadApp::replace_all_rect();
                                                                if gui::in_rect(mx, my, (wx + rall_r.0, wy + rall_r.1, rall_r.2, rall_r.3)) {
                                                                    notepad_app.replace_all();
                                                                    notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        // Çubuğun dışına tıklandı: bul penceresini kapat ve metin alanına geç.
                                        notepad_app.close_find();
                                        if gui::in_rect(mx, my, (wx + 6, wy + notepad_app.text_y() - 4, ww.saturating_sub(12), wh.saturating_sub(notepad_app.text_y() + 28))) {
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
                                } else if gui::in_rect(mx, my, (wx + 6, wy + notepad_app.text_y() - 4, ww.saturating_sub(12), wh.saturating_sub(notepad_app.text_y() + 28))) {

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

                                if browser_app.find_active && gui::in_rect(mx, my, apps::browser::BrowserApp::find_bar_rect(wx, wy, ww)) {

                                    browser_app.find_next();

                                } else if gui::in_rect(mx, my, apps::browser::BrowserApp::toolbar_menu_button_rect(wx, wy, ww)) {

                                    browser_app.toolbar_menu_open = !browser_app.toolbar_menu_open;

                                } else if browser_app.toolbar_menu_open {

                                    let item = (0..4).find(|&i| gui::in_rect(mx, my, apps::browser::BrowserApp::toolbar_menu_item_rect(wx, wy, ww, i)));
                                    browser_app.toolbar_menu_open = false;
                                    match item {
                                        Some(0) => browser_app.show_history(),
                                        Some(1) => {
                                            let current = core::str::from_utf8(&browser_app.url[..browser_app.url_len]).unwrap_or("");
                                            if apps::browser::BOOKMARKS.iter().chain(browser_app.bookmarks_extra.iter()).any(|b| b.url == current) {
                                                browser_app.remove_bookmark_current();
                                            } else {
                                                browser_app.add_bookmark_current();
                                            }
                                        }
                                        Some(2) => { browser_app.save_current_download(&mut fs_mgr.vfs); }
                                        Some(3) => browser_app.navigate(),
                                        _ => {}
                                    }

                                } else if gui::in_rect(mx, my, (wx + 12, wy + 32, 22, 20)) {

                                    browser_app.back();

                                } else if gui::in_rect(mx, my, (wx + 38, wy + 32, 22, 20)) {

                                    browser_app.forward();

                                } else if gui::in_rect(mx, my, (wx + 64, wy + 32, 22, 20)) {

                                    browser_app.navigate();

                                } else if gui::in_rect(mx, my, (wx + 94, wy + 32, ww.saturating_sub(142), 20)) {

                                    browser_app.address_click_at(
                                        mx, my, wx + 100, monotonic_ms(),
                                        settings_app.double_click_ms as u64,
                                    );
                                    browser_address_dragging = true;

                                } else if let Some(url) = browser_app.click_bookmark_at(mx, my, wx, wy, ww) {

                                    browser_app.set_url(url.as_bytes());

                                    browser_app.navigate();

                                } else if let Some(url) = browser_app.click_link_at(mx, my) {

                                    // Sayfa icindeki baglantiya tiklandi: gecmise kaydet ve ac.

                                    browser_app.set_url(url.as_bytes());

                                    browser_app.navigate();

                                } else {

                                    browser_app.address_focused = false;

                                }

                            }

                            4 => {

                                // Resim: yakinlastir / uzaklastir

                                if my >= wy + 32 && my <= wy + 52 {

                                    if mx >= wx + ww - 180 && mx <= wx + ww - 130 {

                                        imageviewer_app.zoom_in();

                                    } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {

                                        imageviewer_app.zoom_out();

                                    }

                                }

                            }

                            5 => {

                                // Muzik calar: onceki / oynat / sonraki

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

                                    } else if mx >= wx + 146 && mx < wx + 240 {

                                        let player_volume = (((mx - (wx + 146)) * 100) / 94).min(100) as u8;

                                        settings_app.volume = player_volume;

                                        settings_app.is_muted = false;

                                        musicplayer_app.volume = player_volume as usize;

                                    } else if my >= wy + 198 && my <= wy + 212 && mx >= wx + 150 && mx <= wx + 240 {

                                        let player_volume = (((mx - (wx + 150)) * 100) / 90).min(100) as u8;

                                        settings_app.volume = player_volume;

                                        settings_app.is_muted = false;

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

                                // Video: oynat / durdur / -10s / +10s / tam ekran

                                let ctrl_y = wy + apps::videoplayer::VideoPlayerApp::CONTROLS_Y;

                                if my >= ctrl_y && my <= ctrl_y + 24 {

                                    if mx >= wx + 12 && mx <= wx + 54 {

                                        videoplayer_app.is_playing = !videoplayer_app.is_playing;

                                    } else if mx >= wx + 58 && mx <= wx + 98 {

                                        videoplayer_app.is_playing = false;

                                        videoplayer_app.frame = 0;

                                    } else if mx >= wx + 102 && mx <= wx + 150 {

                                        let fps = apps::videoplayer::VideoPlayerApp::FPS;
                                        if videoplayer_app.frame >= fps * 10 {
                                            videoplayer_app.frame = videoplayer_app.frame - fps * 10;
                                        } else {
                                            videoplayer_app.frame = 0;
                                        }

                                    } else if mx >= wx + 154 && mx <= wx + 202 {

                                        let fps = apps::videoplayer::VideoPlayerApp::FPS;
                                        let dur = apps::videoplayer::VideoPlayerApp::DURATION_S;
                                        let total = fps * dur;
                                        videoplayer_app.frame = (videoplayer_app.frame + fps * 10).min(total.saturating_sub(1));

                                    } else if let Some(w) = compositor.windows[idx].as_ref() {
                                        if mx >= wx + w.width.saturating_sub(76) && mx <= wx + w.width.saturating_sub(12) {
                                            compositor.toggle_maximize(idx, width, height);
                                        }
                                    }

                                }

                            }

                            7 => {

                                if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::applications_tab_rect(wx, wy)) {

                                    taskmgr_app.active_tab = 0;

                                    taskmgr_app.focused_control = 0;

                                    taskmgr_app.launch_menu_open = false;
                                    taskmgr_app.actions_menu_open = false;

                                } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::performance_tab_rect(wx, wy)) {

                                    taskmgr_app.active_tab = 1;

                                    taskmgr_app.focused_control = 1;

                                    taskmgr_app.launch_menu_open = false;
                                    taskmgr_app.actions_menu_open = false;

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

                                } else if taskmgr_app.active_tab == 0 {

                                    if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::actions_button_rect(wx, wy, ww)) {

                                        taskmgr_app.focused_control = 2;

                                        taskmgr_app.actions_menu_open = !taskmgr_app.actions_menu_open;

                                    } else if taskmgr_app.actions_menu_open {

                                        let action = (0..2).find(|&i| gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::actions_menu_item_rect(wx, wy, ww, i)));
                                        taskmgr_app.actions_menu_open = false;
                                        match action {
                                            Some(0) => { taskmgr_app.launch_menu_open = true; taskmgr_app.launch_selected = 0; }
                                            Some(1) => if let Some(target) = taskmgr_app.selected_app_idx() {
                                                taskmgr_app.focused_control = 3;
                                                taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");
                                                terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,
                                                    &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                                    &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],
                                                    &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);
                                            },
                                            _ => {}
                                        }

                                    } else if let Some(i) = taskmgr_app.row_at(mx, my, wx, wy, ww) {

                                        taskmgr_app.selected = i;

                                        taskmgr_app.focused_control = 4 + i;

                                    }

                                }

                            }

                            8 => { if let Some(key) = apps::calculator::CalculatorApp::key_at(mx, my, wx, wy, ww) { calculator_app.press(key); } }

                            9 => {

                                // Tum dikdortgenler `PaintApp::*_rect` yardimcilarindan gelir;

                                // cizim (`render_content`) ile tiklama testi birebir ortusur.

                                let r = apps::paint::PaintApp::open_btn_rect(ww);

                                let open_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::save_btn_rect(ww);

                                let save_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::clear_btn_rect(ww);

                                let clear_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::undo_btn_rect();
                                let undo_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::redo_btn_rect();
                                let redo_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::tool_line_rect();
                                let tool_line_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::tool_rect_rect();
                                let tool_rect_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::tool_circle_rect();
                                let tool_circle_rect = (wx + r.0, wy + r.1, r.2, r.3);
                                let r = apps::paint::PaintApp::picker_btn_rect();
                                let picker_btn_rect = (wx + r.0, wy + r.1, r.2, r.3);

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

                                // Acilir menu ogesi seciliyse once onu isle ve menuyu kapat.

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

                                                1 => { with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP açılıyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs)); }

                                                2 => {

                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {

                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                    }

                                                }

                                                _ => {

                                                    // Farkli Kaydet: dosya adini turetip yeni dosyaya yaz.

                                                    let name = {

                                                        let cur = paint_apps[idx].as_ref().unwrap().current_name();

                                                        apps::paint::PaintApp::derive_save_as_name(cur)

                                                    };

                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Farklı kaydediliyor...", "Saving as..."), || paint_apps[idx].as_mut().unwrap().save_as(&mut fs_mgr.vfs, cw, ch, name.as_str())) {

                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                    }

                                                }

                                            }

                                            menu_handled = true;

                                            break;

                                        }

                                    }

                                    // Menu acikken disina tiklandiysa sadece menuyu kapat.

                                    if !menu_handled {

                                        paint_apps[idx].as_mut().unwrap().file_menu_open = false;

                                        menu_handled = true;

                                    }

                                }

                                if menu_handled {

                                    // Menu islemi tamamlandi; diger isabet testleri atlanir.

                                } else if gui::in_rect(mx, my, file_btn_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let p = paint_apps[idx].as_mut().unwrap();

                                    p.file_menu_open = !p.file_menu_open;

                                } else if gui::in_rect(mx, my, open_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP açılıyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs));

                                } else if gui::in_rect(mx, my, save_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);

                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {

                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                    }

                                } else if gui::in_rect(mx, my, clear_rect) {

                                    if let Some(paint) = paint_apps[idx].as_mut() { paint.clear(); }

                                } else if gui::in_rect(mx, my, undo_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let _ = paint_apps[idx].as_mut().unwrap().undo();

                                } else if gui::in_rect(mx, my, redo_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let _ = paint_apps[idx].as_mut().unwrap().redo();

                                } else if gui::in_rect(mx, my, tool_line_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().tool = apps::paint::PaintTool::Line;
                                    paint_apps[idx].as_mut().unwrap().picker_open = false;

                                } else if gui::in_rect(mx, my, tool_rect_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().tool = apps::paint::PaintTool::Rect;
                                    paint_apps[idx].as_mut().unwrap().picker_open = false;

                                } else if gui::in_rect(mx, my, tool_circle_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().tool = apps::paint::PaintTool::Circle;
                                    paint_apps[idx].as_mut().unwrap().picker_open = false;

                                } else if gui::in_rect(mx, my, picker_btn_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().picker_open = !paint_apps[idx].as_mut().unwrap().picker_open;

                                } else if paint_apps[idx].as_ref().map_or(false, |p| p.picker_open) {

                                    // renk secici
                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }
                                    let mut picked = None;
                                    for i in 0..apps::paint::PICKER_COLORS.len() {
                                        let r = apps::paint::PaintApp::picker_cell_rect(i);
                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) { picked = Some(i); break; }
                                    }
                                    if let Some(i) = picked {
                                        let p = paint_apps[idx].as_mut().unwrap();
                                        p.color = apps::paint::PICKER_COLORS[i];
                                        p.tool = apps::paint::PaintTool::Brush;
                                        p.picker_open = false;
                                    } else {
                                        // popup disina tiklandiysa kapat
                                        let pr = apps::paint::PaintApp::picker_rect();
                                        if !gui::in_rect(mx, my, (wx + pr.0, wy + pr.1, pr.2, pr.3)) {
                                            paint_apps[idx].as_mut().unwrap().picker_open = false;
                                        }
                                    }

                                } else if gui::in_rect(mx, my, eraser_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let p = paint_apps[idx].as_mut().unwrap();
                                    p.tool = apps::paint::PaintTool::Eraser;
                                    p.picker_open = false;

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

                                    // Renk kutularindan birine basildi mi?

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

                                            paint.tool = apps::paint::PaintTool::Brush;
                                            paint.picker_open = false;

                                            break;

                                        }

                                    }

                                } else {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let paint = paint_apps[idx].as_mut().unwrap();

                                    if let Some((cxp, cyp)) = paint.to_canvas(ww, wh, mx - wx, my - wy) {

                                    // Ilk tiklama noktasi (last=None iken tek nokta baslangici).

                                    paint.paint(cxp, cyp);

                                    } else {

                                    // Tuval disina tiklandi: kalem yolunu bitir.

                                    paint.end_stroke();

                                    }

                                }

                            }

                            11 => {

                                // minOs Kurulum Sihirbazi

                                let sel_rect = apps::installer::InstallerApp::drive_select_rect(wx, wy, ww);

                                let radio_fat32 = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 0);

                                let radio_ext4 = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 1);

                                let radio_exfat = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 2);

                                let fmt_rect = apps::installer::InstallerApp::format_btn_rect(wx, wy, ww, wh);

                                let btn_rect = apps::installer::InstallerApp::install_btn_rect(wx, wy, ww, wh);

                                if gui::in_rect(mx, my, sel_rect) {

                                    installer_app.select_next_drive();

                                } else if gui::in_rect(mx, my, radio_fat32) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Fat32);

                                } else if gui::in_rect(mx, my, radio_ext4) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Ext4);

                                } else if gui::in_rect(mx, my, radio_exfat) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Exfat);

                                } else if gui::in_rect(mx, my, fmt_rect) {

                                    // 2. Adim: Sadece bicimlendir.

                                    if installer_app.state != apps::installer::InstallState::Formatting

                                        && installer_app.state != apps::installer::InstallState::Copying

                                    {

                                        let formatted = with_busy_progress(&mut fb, mx, my, gui::ui_text("Disk biçimlendiriliyor...", "Formatting disk..."), || installer_app.format_selected_drive());

                                        if formatted {

                                            filemanager_app.set_path("/media/hdd0");

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        }

                                    }

                                } else if gui::in_rect(mx, my, btn_rect) {

                                    // 3. Adim: Onayli tek tikla kurulum.

                                    if installer_app.state != apps::installer::InstallState::Formatting

                                        && installer_app.state != apps::installer::InstallState::Copying

                                    {

                                        if installer_app.disk_formatted {

                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));

                                            if installed { filemanager_app.set_path("/media/hdd0"); }

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        } else if installer_app.confirm_install {

                                            // Onay verildi: bicimlendirip kur.

                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));

                                            if installed { filemanager_app.set_path("/media/hdd0"); }

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        } else {

                                            // Ilk tiklama: onay iste (diski bicimlendirecek).

                                            installer_app.confirm_install = true;

                                            installer_app.status_msg = (

                                                "UYARI: Disk silinip biçimlendirilecek. Onaylamak için tekrar tıklayın.",

                                                "WARNING: Disk will be erased and formatted. Click again to confirm.",

                                            );

                                        }

                                    }

                                }

                            }

                            12 => {

                                viewer3d_app.handle_mouse_down(mx.saturating_sub(wx), my.saturating_sub(wy), ww, wh);

                            }

                            13 => {

                                pkgmgr_app.handle_click(mx.saturating_sub(wx), my.saturating_sub(wy), ww, wh, &mut fs_mgr.vfs);
                                compositor.sync_installed_apps(&fs_mgr.vfs);
                                if let Some(app_name) = pkgmgr_app.take_pending_launch() {
                                    desktop::helpers::launch_installed_pkg(
                                        &app_name,
                                        &mut compositor,
                                        &mut terminal_app,
                                        &mut notepad_app,
                                        &mut calculator_app,
                                        &mut paint_apps,
                                        &mut fs_mgr.vfs,
                                        &mut memory_mgr.pmm,
                                        &now,
                                    );
                                }

                            }

                            14 => {

                                for tab in 0..apps::controlpanel::TAB_COUNT {

                                    if gui::in_rect(mx, my, apps::controlpanel::tab_rect(wx, wy, ww, tab)) {

                                        apps::controlpanel::select_tab(tab);

                                        break;

                                    }

                                }

                                if gui::in_rect(mx, my, apps::controlpanel::refresh_btn_rect(wx, wy, ww, wh)) {

                                    apps::controlpanel::refresh();

                                } else if gui::in_rect(mx, my, apps::controlpanel::net_test_btn_rect(wx, wy, ww, wh)) {

                                    apps::controlpanel::start_network_test();

                                } else if gui::in_rect(mx, my, apps::controlpanel::theme_btn_rect(wx, wy, ww, wh)) {

                                    settings_app.cycle_theme();

                                }

                            }

                            _ => {}

                        }

                        break;

                    }

                }

            }

        }



        // 3. Klavye kisayollari

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

                            &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                    } else {

                        // Masaustundeyken Alt+F4 guc menusunu acar.

                        compositor.is_start_menu_open = true;

                        compositor.active_start_group = 4;

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

                        } else if !compositor.start_menu_focus_submenu {

                            if compositor.active_start_group > 0 {
                                compositor.active_start_group -= 1;
                                compositor.selected_start_item = 0;
                                compositor.confirm_shutdown = false;
                                compositor.confirm_restart = false;
                            }

                        } else if compositor.selected_start_item > 0 {

                            compositor.selected_start_item -= 1;

                        }

                    } else if compositor.is_context_menu_open {

                        // Sag tik / Baglam menusu gezintisi

                        if compositor.context_file_idx.unwrap_or(0) > 0 {

                            compositor.context_file_idx = Some(compositor.context_file_idx.unwrap() - 1);

                        }

                    } else {

                        let np_focused = app_is_focused(&compositor, 2);

                        let terminal_focused = app_is_focused(&compositor, 10);

                        let tm_focused = app_is_focused(&compositor, 7);

                        let fm_focused = app_is_focused(&compositor, 0);

                        let set_focused = app_is_focused(&compositor, 1);



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

                            let win_w = focused_window_for_app(&compositor, 0)

                                .map(|(_, w)| w.width).unwrap_or(0);

                            filemanager_app.move_selection_up(win_w);

                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                        } else if set_focused && settings_app.focused_control < 5 {

                            settings_app.select_tab_prev();

                        } else if app_is_focused(&compositor, 12) {

                            viewer3d_app.rot_x -= 0.1;

                        }

                    }

                }

                drivers::keyboard::KeyAction::ArrowDown => {

                    if compositor.is_start_menu_open {

                        if compositor.start_search_len > 0 {

                            compositor.move_start_search_selection(settings_app.language.is_english(), true);

                        } else if !compositor.start_menu_focus_submenu {

                            if compositor.active_start_group + 1 < 5 {
                                compositor.active_start_group += 1;
                                compositor.selected_start_item = 0;
                                compositor.confirm_shutdown = false;
                                compositor.confirm_restart = false;
                            }

                        } else {

                            let active_g = compositor.active_start_group;

                            let count = if active_g == 3 {
                                1 + compositor.installed_apps_count
                            } else if active_g == 4 {
                                2
                            } else {
                                gui::MENU_GROUPS[active_g].2
                            };

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

                            let win_w = focused_window_for_app(&compositor, 0)

                                .map(|(_, w)| w.width).unwrap_or(0);

                            filemanager_app.move_selection_down(win_w);

                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                        } else if set_focused && settings_app.focused_control < 5 {

                            settings_app.select_tab_next();

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

                        if compositor.start_search_focused {
                            let target = if keyboard.ctrl {
                                let mut i = compositor.start_search_cursor;
                                while i > 0 && compositor.start_search[i - 1] == b' ' { i -= 1; }
                                while i > 0 && compositor.start_search[i - 1].is_ascii_alphanumeric() { i -= 1; }
                                i
                            } else { compositor.start_search_cursor.saturating_sub(1) };
                            compositor.start_search_move_to(target, keyboard.shift);
                        } else {
                            compositor.start_menu_focus_submenu = false;
                        }

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.move_selection_left();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

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

                            browser_app.focus_prev_control();

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

                        if compositor.start_search_focused {
                            let target = if keyboard.ctrl {
                                let mut i = compositor.start_search_cursor;
                                while i < compositor.start_search_len && compositor.start_search[i].is_ascii_alphanumeric() { i += 1; }
                                while i < compositor.start_search_len && compositor.start_search[i] == b' ' { i += 1; }
                                i
                            } else { (compositor.start_search_cursor + 1).min(compositor.start_search_len) };
                            compositor.start_search_move_to(target, keyboard.shift);
                        } else {

                            compositor.start_menu_focus_submenu = true;

                            compositor.selected_start_item = 0;

                        }

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.move_selection_right();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

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

                            browser_app.focus_next_control();

                        }

                    }

                }

                drivers::keyboard::KeyAction::Properties => {

                    // Alt+Enter: secili dosyanin özelliklerini durum cubugunda goster.

                    if app_is_focused(&compositor, 0) {

                        if let Some(item) = filemanager_app.row(filemanager_app.selected_idx) {

                            let mut info = gui::TextBuf::new();

                            info.push_str(item.display_name());

                            info.push_str("  |  ");

                            info.push_str(item.file_type);

                            info.push_str("  |  ");

                            info.push_str(item.size_str.as_str());

                            filemanager_app.set_status_text(info.as_str());

                        }

                    }

                }

                drivers::keyboard::KeyAction::EnterKey => {

                    if app_is_focused(&compositor, 3) && browser_app.find_active {
                        browser_app.find_next();
                        enter_char_consumed = true;
                    }

                    // Alt+Enter: özellikler penceresini ac

                    if keyboard.alt && app_is_focused(&compositor, 0) && filemanager_app.selected_idx < filemanager_app.file_count {

                        let file_idx = filemanager_app.selected_idx;

                        let file = &filemanager_app.files[file_idx];

                        properties_name.clear();

                        properties_name.push_str(file.display_name());

                        properties_type.clear();

                        properties_type.push_str(file.file_type);

                        properties_size.clear();

                        properties_size.push_str(file.size_str.as_str());

                        properties_location.clear();

                        properties_location.push_str(filemanager_app.path());

                        properties_path.clear();

                        let full_path = filemanager_app.selected_full_path();

                        properties_path.push_str(&full_path);

                        properties_open = true;

                        enter_char_consumed = true;

                    }

                    if compositor.is_start_menu_open {

                        enter_char_consumed = true;

                        if compositor.start_search_len > 0 {

                            if let Some(app_idx) = compositor.selected_start_search_result(settings_app.language.is_english()) {

                                if app_idx >= 100 {
                                    let app_name = alloc::string::String::from(compositor.installed_app_name(app_idx - 100));
                                    if !app_name.is_empty() {
                                        desktop::helpers::launch_installed_pkg(
                                            &app_name,
                                            &mut compositor,
                                            &mut terminal_app,
                                            &mut notepad_app,
                                            &mut calculator_app,
                                            &mut paint_apps,
                                            &mut fs_mgr.vfs,
                                            &mut memory_mgr.pmm,
                                            &now,
                                        );
                                    }
                                } else {
                                    open_app(&mut compositor, app_idx);
                                }

                                compositor.close_start_menu();

                            }

                            continue;

                        }

                        if !compositor.start_menu_focus_submenu {
                            compositor.start_menu_focus_submenu = true;
                            compositor.selected_start_item = 0;
                            continue;
                        }

                        let active_g = compositor.active_start_group;

                        let count = if active_g == 3 {
                            compositor.installed_apps_count
                        } else if active_g == 4 {
                            2
                        } else {
                            gui::MENU_GROUPS[active_g].2
                        };

                        let item_k = compositor.selected_start_item.min(count.saturating_sub(1));

                        let i = if active_g < 3 { gui::MENU_GROUPS[active_g].1 + item_k } else { 0 };

                        if active_g == 4 && item_k == 0 {

                            if compositor.confirm_shutdown {

                                #[cfg(minos_boot_log)]

    serial_print("[minOs] Klavyeden sistem kapatma onaylandı.\n");

                                unsafe {

                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);

                                    core::arch::asm!("cli; hlt");

                                }

                            } else {

                                compositor.confirm_shutdown = true;

                            }

                        } else if active_g == 4 && item_k == 1 {

                            if compositor.confirm_restart {

                                restart_system();

                            }

                            compositor.confirm_restart = true;

                        } else {

                            if active_g == 3 {
                                if item_k == 0 {
                                    open_app(&mut compositor, 13);
                                } else {
                                    let app_name = alloc::string::String::from(compositor.installed_app_name(item_k - 1));
                                    if !app_name.is_empty() {
                                        desktop::helpers::launch_installed_pkg(
                                            &app_name,
                                            &mut compositor,
                                            &mut terminal_app,
                                            &mut notepad_app,
                                            &mut calculator_app,
                                            &mut paint_apps,
                                            &mut fs_mgr.vfs,
                                            &mut memory_mgr.pmm,
                                            &now,
                                        );
                                    }
                                }
                            } else if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {

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

                                        &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

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

                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                        if let Some(data) = data {

                                            notepad_app.open_file(&path, data);

                                        }

                                        open_app(&mut compositor, 2);

                                    } else if tag == "IMG" {

                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                        open_app(&mut compositor, 4);

                                    } else if tag == "SND" {

                                        open_app(&mut compositor, 5);

                                    } else if tag == "VID" {

                                        open_app(&mut compositor, 6);

                                    } else if tag == "WEB" {

                                        open_app(&mut compositor, 3);

                                    } else if tag == "3D" {

                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

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

                        if settings_app.focused_control < 5 {

                            settings_app.active_tab = settings_app.focused_control;

                        } else {

                            match settings_app.active_tab {

                                0 => match settings_app.focused_control {

                                    5 => {

                                        keyboard.toggle_layout();

                                        settings_app.keyboard_layout = keyboard.layout;

                                    }

                                    6 => {

                                        settings_app.language.toggle();

                                        settings_app.status_msg = gui::ui_text("Arayüz dili değiştirildi.", "Interface language changed.");

                                    }

                                    7 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    8 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); }

                                    _ => {}

                                },

                                1 => match settings_app.focused_control {

                                    5 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    6 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },

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

                        if browser_app.find_active { browser_app.find_next(); } else { match browser_app.focused_control {

                            0 => browser_app.back(),

                            1 => browser_app.forward(),

                            2 => browser_app.navigate(),

                            3 => browser_app.show_history(),

                            4 | 5 => browser_app.navigate(),

                            _ => {}

                        } }

                    } else if app_is_focused(&compositor, 8) {

                        enter_char_consumed = true;

                        calculator_app.press('=');

                    } else if app_is_focused(&compositor, 10) {

                        enter_char_consumed = true;

                        terminal_app.submit();

                        if let Some(cmd) = terminal_app.take_pending() {

                            let n = terminal_app.pending_len();

                            let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");

                            if let Some(app_idx) = terminal_app.execute(line, &mut fs_mgr.vfs, &mut memory_mgr.pmm, &now) {

                                open_app(&mut compositor, app_idx);

                            }

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                        }

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

    serial_print("[minOs] Sistem kapatma isteği alındı.\n");

                    unsafe {

                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);

                        core::arch::asm!("cli; hlt");

                    }

                }

                drivers::keyboard::KeyAction::CopyText => {

                    if app_is_focused(&compositor, 2) { notepad_app.copy_selection(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_copy(); }

                    else if compositor.is_start_menu_open && compositor.start_search_focused { compositor.copy_start_search(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, false); }

                    else if desktop_files.selected_items.iter().any(|&s| s) || desktop_files.selected_idx < desktop_files.file_count { desktop_files.copy_selection(&mut file_clipboard, false); }

                    else if app_is_focused(&compositor, 8) { calculator_app.copy_result(); }

                    else if app_is_focused(&compositor, 10) { terminal_app.copy_input(); }

                }

                drivers::keyboard::KeyAction::PasteText => {

                    if app_is_focused(&compositor, 2) { notepad_app.paste_clipboard(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_paste(); }

                    else if compositor.is_start_menu_open && compositor.start_search_focused { compositor.paste_start_search(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                    else if app_is_focused(&compositor, 8) { calculator_app.paste_expression(); }

                    else if app_is_focused(&compositor, 10) { terminal_app.paste_clipboard(); }

                    else { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                }

                drivers::keyboard::KeyAction::CutText => {

                    if app_is_focused(&compositor, 2) { notepad_app.cut_selection(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_cut(); }

                    else if compositor.is_start_menu_open && compositor.start_search_focused { compositor.cut_start_search(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, true); }

                    else if desktop_files.selected_items.iter().any(|&s| s) || desktop_files.selected_idx < desktop_files.file_count { desktop_files.copy_selection(&mut file_clipboard, true); }

                    else if app_is_focused(&compositor, 8) { calculator_app.cut_expression(); }

                }

                drivers::keyboard::KeyAction::SelectAllText => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.select_all(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_select_all(); }

                    else if compositor.is_start_menu_open && compositor.start_search_focused { compositor.select_all_start_search(); }

                }

                drivers::keyboard::KeyAction::Undo => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.undo(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_undo_edit(); }

                    else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if let Some(paint) = paint_apps[paint_idx].as_mut() { let _ = paint.undo(); }
                        }
                    }

                }

                drivers::keyboard::KeyAction::Redo => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.redo(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_redo_edit(); }

                    else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if let Some(paint) = paint_apps[paint_idx].as_mut() { let _ = paint.redo(); }
                        }
                    }

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
                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));
                        if let Some(data) = data {
                            notepad_app.open_file(&path, data);
                        }
                    }
                }

                drivers::keyboard::KeyAction::FindText => {
                    if app_is_focused(&compositor, 2) {
                        notepad_app.open_find();
                    } else if app_is_focused(&compositor, 3) {
                        browser_app.open_find();
                    }
                }

                drivers::keyboard::KeyAction::ReplaceText => {
                    if app_is_focused(&compositor, 2) {
                        notepad_app.open_replace();
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

                        let rows = focused_window_for_app(&compositor, 0)

                            .map(|(_, w)| fm_page_rows(&w, &filemanager_app)).unwrap_or(8);

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

                        let rows = focused_window_for_app(&compositor, 0)

                            .map(|(_, w)| fm_page_rows(&w, &filemanager_app)).unwrap_or(8);

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

                    // Gorev Yoneticisi odakliysa secili sureci sonlandir.

                    let tm_focused = app_is_focused(&compositor, 7);

                    if tm_focused {

                        if let Some(target) = taskmgr_app.selected_app_idx() {

                            taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[target],

                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

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

                    } else if desktop_files.selected_items.iter().any(|&s| s) || (desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx]) {

                        desktop_files.delete_selected(&mut fs_mgr.vfs);

                        desktop_files.refresh_from_vfs(&fs_mgr.vfs);

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

                drivers::keyboard::KeyAction::Refresh => {

                    // F5: odakli uygulamanin icerigini yenile.

                    if app_is_focused(&compositor, 0) {

                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                    } else if app_is_focused(&compositor, 3) {

                        browser_app.navigate();

                    } else if app_is_focused(&compositor, 13) {

                        pkgmgr_app.refresh_installed(&fs_mgr.vfs);

                    } else if app_is_focused(&compositor, 7) {

                        taskmgr_app.status = gui::ui_text("Görev listesi canlı güncelleniyor.", "Task list updates live.");

                    } else if compositor.active_window_idx.is_none() {

                        desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                    }

                }

                drivers::keyboard::KeyAction::BrowserAddress => {

                    let focused = app_is_focused(&compositor, 3);

                    if focused { browser_app.focus_address_end(); }

                }

                drivers::keyboard::KeyAction::BrowserRefresh => {

                    let focused = app_is_focused(&compositor, 3);

                    if focused { browser_app.navigate(); }

                }

                drivers::keyboard::KeyAction::RunDialog => {

                    run_dialog_open = !run_dialog_open;

                    run_dialog_input.clear();

                    compositor.close_start_menu();

                }

                drivers::keyboard::KeyAction::Screenshot => {

                    // Ekran goruntusu (F12 / PrtScn) -> BMP olarak VFS'ye kaydet

                    audio_dev.play_beep(980, 40);

                    let sw = width;

                    let sh = height;

                    let row_bytes = (sw * 3 + 3) & !3;

                    let image_size = row_bytes * sh;

                    let file_size = 54 + image_size;



                    let mut bmp_data = alloc::vec::Vec::new();

                    if bmp_data.try_reserve_exact(file_size).is_err() {

                        serial_print("[SCREENSHOT] HATA: Yetersiz bellek!\n");

                        terminal_app.push_line(gui::ui_text("Ekran görüntüsü alınamadı: Yetersiz bellek", "Screenshot failed: Out of memory"), gui::Color::ACCENT_RED);

                    } else {

                        // BMP Header (14 bytes)

                        bmp_data.extend_from_slice(b"BM");

                        bmp_data.extend_from_slice(&(file_size as u32).to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&54u32.to_le_bytes());

                        // DIB Header (40 bytes)

                        bmp_data.extend_from_slice(&40u32.to_le_bytes());

                        bmp_data.extend_from_slice(&(sw as i32).to_le_bytes());

                        bmp_data.extend_from_slice(&(sh as i32).to_le_bytes());

                        bmp_data.extend_from_slice(&1u16.to_le_bytes());

                        bmp_data.extend_from_slice(&24u16.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&(image_size as u32).to_le_bytes());

                        bmp_data.extend_from_slice(&2835u32.to_le_bytes());

                        bmp_data.extend_from_slice(&2835u32.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        // Pixel Data (Alt satirdan ust satira BGR)

                        for y in (0..sh).rev() {

                            for x in 0..sw {

                                let p = fb.get_pixel(x, y);

                                let b = (p & 0xFF) as u8;

                                let g = ((p >> 8) & 0xFF) as u8;

                                let r = ((p >> 16) & 0xFF) as u8;

                                bmp_data.push(b);

                                bmp_data.push(g);

                                bmp_data.push(r);

                            }

                            for _ in 0..(row_bytes - sw * 3) {

                                bmp_data.push(0);

                            }

                        }

                        let mut path = alloc::string::String::from("/desktop/ekran_");

                        let mut num = gui::TextBuf::new();

                        num.push_usize(screenshot_counter);

                        path.push_str(num.as_str());

                        path.push_str(".bmp");

                        serial_print("[SCREENSHOT] Ekran görüntüsü alınıyor...\n");

                        screenshot_counter += 1;

                        let arc_data: alloc::sync::Arc<[u8]> = bmp_data.into();

                        if fs_mgr.vfs.write_large_file(&path, arc_data) {

                            desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            terminal_app.push_line(gui::ui_text("Ekran görüntüsü kaydedildi: /desktop/", "Screenshot saved to: /desktop/"), gui::Color::ACCENT_GREEN);

                            compositor.push_toast(

                                gui::ToastKind::Success,

                                gui::ui_text("Ekran Görüntüsü Alındı", "Screenshot Captured"),

                                gui::ui_text("Görüntü /desktop/ klasörüne kaydedildi.", "Image saved to /desktop/ folder."),

                                monotonic_ms(),

                                4000,

                            );

                            force_render = true;

                            serial_print("[SCREENSHOT] Başarıyla kaydedildi: ");

                            serial_print(&path);

                            serial_print("\n");

                        } else {

                            compositor.push_toast(

                                gui::ToastKind::Error,

                                gui::ui_text("Ekran Görüntüsü Hatası", "Screenshot Error"),

                                gui::ui_text("Dosya diske yazılamadı!", "Failed to write file to disk!"),

                                monotonic_ms(),

                                4000,

                            );

                            serial_print("[SCREENSHOT] HATA: VFS dosyası yazılamadı!\n");

                        }

                    }

                }

            }

        }



        // 4. Fare Tekerlek (Scroll)

        let scroll_val = mouse.take_scroll_delta();

        if scroll_val != 0 {

            // Ayarlar > Testler: tekerlek sinamasi sayaci.

            if app_is_focused(&compositor, 1) && settings_app.active_tab == 4 {

                if let Some((_, w)) = focused_window_for_app(&compositor, 1) {

                    let pad = apps::settings::SystemSettings::test_mouse_pad_rect(w.x, w.y);

                    if gui::in_rect(mx, my, pad) {

                        settings_app.note_scroll_test(scroll_val.unsigned_abs() as u32);

                    }

                }

            }

            // Baslat menusu acikken scroll wheel destegi

            if compositor.is_start_menu_open {

                let active_g = compositor.active_start_group;

                let count = if active_g == 3 {
                    1 + compositor.installed_apps_count
                } else if active_g == 4 {
                    2
                } else {
                    gui::MENU_GROUPS[active_g].2
                };

                if scroll_val > 0 {

                    compositor.selected_start_item = compositor.selected_start_item.saturating_sub(1);

                } else if compositor.selected_start_item + 1 < count {

                    compositor.selected_start_item += 1;

                }

            }

            // CMD gecmisi: yalnizca cikti panelini kaydir; giris satiri sabit kalir.

            if let Some((_, w)) = focused_window_for_app(&compositor, 10) {

                if w.is_open && w.is_focused && !w.is_minimized

                    && gui::in_rect(mouse.x as usize, mouse.y as usize, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h() + 28))) {

                    let view_lines = w.height.saturating_sub(86) / 13;

                    terminal_app.scroll_output(scroll_val as isize, view_lines);

                }

            }

            // Imlecin altindaki en ust pencerede icerik kaydirmasini guncelle.

            // Scrollbar ayni ScrollState uzerinden cizildigi icin tutamac da

            // tekerlekle birlikte aninda hareket eder.

            let (window_order, window_count) = compositor.draw_order();

            for k in (0..window_count).rev() {

                let idx = window_order[k];

                let Some(w) = compositor.windows[idx] else { continue; };

                if !w.is_open || w.is_minimized || !gui::in_rect(mx, my, (w.x, w.y, w.width, w.height)) {

                    continue;

                }

                let app_id = w.app_id as usize;

                let (cx, cy, cw, ch) = w.content_rect();

                let c_h = window_content_h(app_id, w.height, w.width, &filemanager_app, &notepad_app, &browser_app);

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

            // Goruntuleyicide tekerlek zoom kisayoludur (yukari yakinlastirir).

            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {

                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&w);

                if gui::in_rect(mx, my, (cx, cy, cw, ch)) {

                    imageviewer_app.pan_y = imageviewer_app.pan_y.saturating_add((scroll_val as isize) * 24);

                }

            }

            // cizim tuvalinde tekerlek yakinlastirmayi ayarlar.

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



        // 5. Pencere Surukleme ve Yeniden Boyutlandirma

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

                    // Windows Paint davranisi: sol tus basili tutuldukca fare

                    // hareketi surekli cizer. Tuval disina cikinca vurusu bitir.

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

            if browser_address_dragging {
                if let Some((_, w)) = focused_window_for_app(&compositor, 3) {
                    if w.is_focused {
                        browser_app.address_drag_to(mx, w.x + 100);
                    }
                }
            }

            if let Some((_, w)) = focused_window_for_app(&compositor, 5) {
                if w.is_focused {
                    if my >= w.y + 156 && my <= w.y + 172 && mx >= w.x + 54 && mx <= w.x + 190 {
                        musicplayer_app.progress = (((mx - (w.x + 54)) * 100) / 136).min(100);
                    } else if my >= w.y + 198 && my <= w.y + 214 && mx >= w.x + 150 && mx <= w.x + 240 {
                        let player_volume = (((mx - (w.x + 150)) * 100) / 90).min(100) as u8;
                        settings_app.volume = player_volume;
                        settings_app.is_muted = false;
                        musicplayer_app.volume = player_volume as usize;
                    }
                }
            }

            if start_search_dragging && compositor.is_start_menu_open {
                let (menu_x, menu_y, menu_w, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);
                let search = gui::MinGuiCompositor::start_search_rect(menu_x, menu_y, menu_w);
                compositor.start_search_drag_to(mx, search.0 + 6);
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

                let fm_win_opt = compositor.windows.iter().flatten().find(|w| w.app_id == 0 && w.is_open && !w.is_minimized);

                if let Some(fm_win) = fm_win_opt {

                    let target = filemanager_app.drop_target_at(fm_win.x, fm_win.y, fm_win.width, fm_win.height, fm_win.scroll.y, mx, my);

                    if desktop_drag_source { desktop_files.drag_target = target; }

                    else { filemanager_app.drag_target = target; }

                }

            }

            if let Some((idx, grab, thumb_h, track_h, content_h, view_h)) = vscroll_drag {

                // Tutamaci fare ile surukle → kaydirma ofseti.

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



                    // Aero Snap kenar algilama & onizleme kutusu

                    let old_snap = active_snap_preview;

                    let usable_h = height.saturating_sub(gui::taskbar_h());

                    let half_w = width / 2;

                    if my <= 4 {

                        active_snap_preview = Some((0, 0, width, usable_h));

                    } else if mx <= 4 {

                        active_snap_preview = Some((0, 0, half_w, usable_h));

                    } else if mx >= width.saturating_sub(5) {

                        active_snap_preview = Some((half_w, 0, width.saturating_sub(half_w), usable_h));

                    } else {

                        active_snap_preview = None;

                    }



                    let new_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);

                    let mut dirty = match drag_prev_box {

                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),

                    };

                    dirty = drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24));

                    // Hem eski hem yeni Aero Snap onizlemesini kirli bolgeye kat.

                    if let Some(p) = old_snap { dirty = drivers::video::rect_union(dirty, p); }

                    if let Some(p) = active_snap_preview { dirty = drivers::video::rect_union(dirty, p); }

                    drag_dirty_rect = Some(dirty);

                    drag_prev_box = Some(new_box);

                }

            } else if let Some(idx) = resizing_win {

                let edge = resize_edge.unwrap_or(gui::ResizeEdge::BottomRight);

                if let Some(w) = compositor.windows[idx].as_mut() {

                    use gui::ResizeEdge::*;

                    let (ox, oy, ow, oh) = resize_orig;

                    let left = ox + ow;   // sol kenarin sabit sag referansi

                    let right = ox;       // sag kenarin sabit sol referansi

                    let top = oy + oh;    // ust kenarin sabit alt referansi

                    let bottom = oy;      // alt kenarin sabit ust referansi



                    let mut nx = w.x;

                    let mut ny = w.y;

                    let mut nw = w.width;

                    let mut nh = w.height;



                    // Sag kenar surukleniyor

                    if matches!(edge, Right | TopRight | BottomRight) {

                        nw = mx.saturating_sub(right).max(w.min_width)

                            .min(w.max_width.min(width.saturating_sub(w.x)));

                    }

                    // Sol kenar surukleniyor

                    if matches!(edge, Left | TopLeft | BottomLeft) {

                        let desired = mx.min(left);

                        nw = left.saturating_sub(desired).max(w.min_width).min(w.max_width).min(left);

                        nx = left.saturating_sub(nw).min(width.saturating_sub(nw));

                    }

                    // Alt kenar surukleniyor

                    if matches!(edge, Bottom | BottomLeft | BottomRight) {

                        let max_h = height.saturating_sub(gui::taskbar_h()).saturating_sub(w.y);

                        nh = my.saturating_sub(bottom).max(w.min_height).min(w.max_height).min(max_h);

                    }

                    // ust kenar surukleniyor

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

                    let mut dirty = match drag_prev_box {

                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),

                    };

                    dirty = drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24));

                    if let Some(p) = active_snap_preview { dirty = drivers::video::rect_union(dirty, p); }

                    drag_dirty_rect = Some(dirty);

                    drag_prev_box = Some(new_box);

                }

            }

        }

        if mouse_released {
            if let Some((idx, control)) = pending_window_control.take() {
                if let Some(win) = compositor.windows.get(idx).and_then(|slot| *slot) {
                    let release_rect = match control {
                        0 => win.close_rect(),
                        1 => win.max_rect(),
                        _ => win.min_rect(),
                    };
                    if win.is_open && !win.is_minimized && gui::in_rect(mx, my, release_rect) {
                        match control {
                            0 => terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,
                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,
                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],
                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app),
                            1 => {
                                compositor.focus_window(idx);
                                compositor.toggle_maximize(idx, width, height);
                            }
                            _ => {
                                if let Some(w) = compositor.windows[idx].as_mut() {
                                    w.is_minimized = true;
                                }
                                compositor.focus_top_window();
                            }
                        }
                    }
                }
            }
            image_drag_last = None;
        }



        if mouse_released {

            drag_prev_box = None;

            drag_dirty_rect = None;

            viewer3d_app.handle_mouse_up();

            if let Some(paint_idx) = compositor.active_window_idx {

                if let Some(paint) = paint_apps[paint_idx].as_mut() { paint.end_stroke(); }

            }

            notepad_dragging = false;
            browser_address_dragging = false;
            start_search_dragging = false;

            let (is_drag_active, drag_item, drag_target) = if desktop_drag_source {

                (desktop_files.drag_active, desktop_files.drag_item, desktop_files.drag_target)

            } else { (filemanager_app.drag_active, filemanager_app.drag_item, filemanager_app.drag_target) };

            if is_drag_active {

                if let Some(src_idx) = drag_item {

                    if let Some(target) = drag_target {

                        let src_app = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };

                        match src_app.execute_drop(src_idx, target, keyboard.ctrl, &mut fs_mgr.vfs) {

                            Ok(_) => {

                                desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            }

                            Err(err) => {

                                if desktop_drag_source { desktop_files.set_status_text(err); }

                                else { filemanager_app.set_status_text(err); }

                            }

                        }

                    } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                        let source = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };

                        let indices = source.drag_items;

                        let source_count = source.file_count;

                        let mut done_count = 0;

                        for i in 0..source_count {

                            if !(indices[i] || i == src_idx) { continue; }

                            let name = source.files[i].display_name();

                            if name == ".." { continue; }

                            let mut src_path = alloc::string::String::from(source.path());

                            if !src_path.ends_with('/') { src_path.push('/'); }

                            src_path.push_str(name);

                            if keyboard.ctrl {

                                if fs_mgr.vfs.copy_item(&src_path, "/desktop").is_ok() { done_count += 1; }

                            } else {

                                if fs_mgr.vfs.move_item(&src_path, "/desktop").is_ok() { done_count += 1; }

                            }

                        }

                        if done_count > 0 {

                            desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            let msg = if keyboard.ctrl {

                                gui::ui_text("Masaüstüne kopyalandı.", "Copied to Desktop.")

                            } else {

                                gui::ui_text("Masaüstüne taşındı.", "Moved to Desktop.")

                            };

                            if desktop_drag_source { desktop_files.set_status_text(msg); }

                            else { filemanager_app.set_status_text(msg); }

                        }

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

            // Modern Aero Snap: baslik cubugu ekran kenarlarina suruklenip birakildiginda:

            // - Ekranin ustu: Tam ekran (Maximize)

            // - Ekranin solu: Sol 50% bolunmus ekran

            // - Ekranin sagi: Sag 50% bolunmus ekran

            if let Some(idx) = dragging_win {

                compositor.focus_window(idx);

                if my <= 4 {

                    compositor.snap_window(idx, gui::SnapPosition::Top, width, height);

                } else if mx <= 4 {

                    compositor.snap_window(idx, gui::SnapPosition::Left, width, height);

                } else if mx >= width.saturating_sub(5) {

                    compositor.snap_window(idx, gui::SnapPosition::Right, width, height);

                }

            }

            active_snap_preview = None;

            dragging_win = None;

            resizing_win = None;

            resize_edge = None;

            vscroll_drag = None;

        }



        // Alt tusu birakildiginda Alt+Tab penceresini kapat

        if !keyboard.alt && compositor.is_alt_tab_open {

            compositor.finish_alt_tab();

        }



        // 6. Klavye karakter girisleri

        while let Some(ch) = keyboard.take_last_char() {

            if ch == '\n' && enter_char_consumed {

                enter_char_consumed = false;

                continue;

            }

            // Baslat menusu acikken klavye kisayollari menu SIRASINI kullanir

            if run_dialog_open {

                if ch == '\x1b' {

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else if ch == '\x08' {

                    run_dialog_input.pop();

                } else if ch == '\n' {

                    let cmd = run_dialog_input.as_str().trim();

                    let app_id = match cmd {

                        "cmd" | "terminal" | "term" | "bash" | "sh" => Some(10),

                        "not" | "notepad" | "text" | "edit" => Some(2),

                        "ayar" | "settings" | "config" => Some(1),

                        "task" | "taskmgr" | "top" | "ps" => Some(7),

                        "calc" | "hesap" => Some(8),

                        "web" | "browser" | "www" => Some(3),

                        "file" | "files" | "fm" | "dosya" => Some(0),

                        "paint" | "cizim" => Some(9),

                        "resim" | "image" | "img" => Some(4),

                        "muzik" | "music" => Some(5),

                        "video" => Some(6),

                        "kur" | "install" => Some(11),

                        "3d" => Some(12),

                        "panel" | "kontrol" | "control" => Some(14),

                        "pkg" | "minpak" | "paket" | "pkgmgr" => Some(13),

                        _ => None,

                    };

                    if let Some(target) = app_id {

                        audio_dev.play_beep(1200, 30);

                        open_app(&mut compositor, target);

                    } else {

                        audio_dev.play_beep(440, 60);

                    }

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else if ch >= ' ' && ch != '\x7f' && run_dialog_input.len() < 30 {

                    let mut b = [0u8; 4];

                    let s = ch.encode_utf8(&mut b);

                    run_dialog_input.push_str(s);

                }

                continue;

            }



            // (pencere indeksini degil) — start_menu_app_idx ile eslenir.

            // özellikler modali acikken ESC ve Enter modali kapatir.

            if properties_open {

                if ch == '\x1b' || ch == '\n' {

                    properties_open = false;

                    properties_name.clear();

                    properties_type.clear();

                    properties_size.clear();

                    properties_location.clear();

                    properties_path.clear();

                    force_render = true;

                }

                continue;

            }

            if ch == '\x1b' && compositor.is_start_menu_open {

                compositor.close_start_menu();

            } else if ch == '\x1b' && compositor.is_context_menu_open {

                compositor.is_context_menu_open = false;

                compositor.is_context_submenu_open = false;

            } else if ch == '\x1b' && taskmgr_app.actions_menu_open {

                taskmgr_app.actions_menu_open = false;

            } else if ch == '\x1b' && taskmgr_app.launch_menu_open {

                taskmgr_app.launch_menu_open = false;

            } else if ch == '\x1b' && notepad_app.find_active && app_is_focused(&compositor, 2) {
                notepad_app.close_find();
            } else if ch == '\x1b' && notepad_app.menu != 0 {

                notepad_app.menu = 0;

            } else if ch == '\x1b' && app_is_focused(&compositor, 3) && browser_app.toolbar_menu_open {

                browser_app.toolbar_menu_open = false;

            } else if ch == '\x1b' && app_is_focused(&compositor, 3) && browser_app.find_active {

                browser_app.close_find();

            } else if ch == '\x08' && app_is_focused(&compositor, 3) && browser_app.find_active {

                browser_app.find_backspace();

            } else if keyboard.ctrl && (ch == 'f' || ch == 'F') && app_is_focused(&compositor, 3) {

                // Ctrl+F is handled by the FindText keyboard action.

            } else if app_is_focused(&compositor, 3) && browser_app.find_active && ch >= ' ' && ch != '\x7f' {

                browser_app.find_char(ch);

            } else if app_is_focused(&compositor, 1) && settings_app.static_ip_edit && ch == '\x1b' {

                settings_app.static_ip_edit = false;

            } else if app_is_focused(&compositor, 1) && settings_app.static_ip_edit && ch == '\x08' {

                settings_app.static_ip_backspace();

            } else if app_is_focused(&compositor, 1) && settings_app.static_ip_edit && ch == '\t' {

                settings_app.static_ip_next_octet();

            } else if app_is_focused(&compositor, 1) && settings_app.static_ip_edit && ch.is_ascii_digit() {

                settings_app.static_ip_input_char(ch);

            } else if ch == '\x1b' && app_is_focused(&compositor, 0) && filemanager_app.search_len > 0 {

                filemanager_app.clear_search();

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

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

            } else if app_is_focused(&compositor, 0) && ch == '\x08' {

                filemanager_app.search_backspace();

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if desktop_files.rename_active {

                if ch == '\x1b' { desktop_files.rename_active = false; }

                else if ch == '\x08' { desktop_files.rename_backspace(); }

                else if ch == '\n' { desktop_files.commit_rename(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                else if ch >= ' ' { desktop_files.rename_char(ch); }

            } else if (ch == 'g' || ch == 'G') && keyboard.ctrl && app_is_focused(&compositor, 0) {

                // Ctrl+G: liste <-> izgara gorunumu.

                filemanager_app.toggle_view_mode();

                filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if app_is_focused(&compositor, 0) && ch >= ' ' && ch != '\x7f' {

                filemanager_app.search_char(ch);

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if ch == ' ' && app_is_focused(&compositor, 0)

                && filemanager_app.selected_idx < filemanager_app.file_count {

                // Space: secili metin/resim icin hizli onizlemeyi ac/kapat.

                filemanager_app.toggle_preview();

                filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                filemanager_app.update_preview(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if compositor.is_start_menu_open && ch == '\x08' {

                compositor.start_search_focused = true;
                compositor.backspace_start_search();

            } else if compositor.is_start_menu_open && ch.is_ascii_alphanumeric() {

                compositor.start_search_focused = true;

                compositor.push_start_search(ch);

            } else if compositor.is_start_menu_open && (ch >= '1' && ch <= '9') {

                let menu_index = (ch as usize) - ('1' as usize);

                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {

                    open_app(&mut compositor, app_idx);

                    compositor.close_start_menu();

                }

            } else if compositor.is_start_menu_open && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B') {

                let menu_index = if ch == 'a' || ch == 'A' { 10 } else { 11 };

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
                    } else if notepad_app.find_active {
                        if ch == '\x08' {
                            notepad_app.find_input_backspace();
                        } else if ch == '\t' {
                            if notepad_app.replace_active {
                                notepad_app.find_focused_replace = !notepad_app.find_focused_replace;
                            }
                        } else if ch == '\n' {
                            if notepad_app.replace_active && notepad_app.find_focused_replace {
                                notepad_app.replace_current();
                            } else {
                                notepad_app.find_next(true);
                            }
                            notepad_ensure_cursor_visible(&mut compositor, &notepad_app);
                        } else if ch >= ' ' {
                            notepad_app.find_input_char(ch);
                        }
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

                // Terminalde Enter'a basildiysa bekleyen komutu VFS/PMM baglaminda yurut.

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

                        else if ch == 'e' || ch == 'E' { paint.tool = if paint.tool == apps::paint::PaintTool::Eraser { apps::paint::PaintTool::Brush } else { apps::paint::PaintTool::Eraser }; }

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

                let pkg_focused = app_is_focused(&compositor, 13);

                if pkg_focused {

                    if ch == '\x08' {

                        pkgmgr_app.backspace_search();

                    } else if ch == '\x1b' {

                        pkgmgr_app.clear_search();

                    } else if ch >= ' ' {

                        pkgmgr_app.insert_search_char(ch);

                    }

                }

                if !run_dialog_open && !compositor.is_start_menu_open && !compositor.is_context_menu_open {

                    if let Some(act_idx) = compositor.active_window_idx {

                        if let Some(w) = compositor.windows[act_idx] {

                            let dirty = (w.x.saturating_sub(8), w.y.saturating_sub(8), w.width + 16, w.height + 16);

                            _app_dirty_rect = Some(match _app_dirty_rect {

                                Some(d) => drivers::video::rect_union(d, dirty),

                                None => dirty,

                            });

                        }

                    }

                }

            }

        }



        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;

        if mouse_moved && compositor.is_start_menu_open {
            let (_, menu_y, _, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);
            for g in 0..5 {
                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);
                if gui::in_rect(mx, my, cat_r) {
                    if compositor.active_start_group != g {
                        compositor.active_start_group = g;
                        compositor.selected_start_item = 0;
                        compositor.start_menu_focus_submenu = false;
                        compositor.confirm_shutdown = false;
                        compositor.confirm_restart = false;
                    }
                    break;
                }
            }
        }

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



        // Ag kartindan gelen paketleri isle (ARP cevaplari, ICMP vs.).

        drivers::e1000::poll();

        // Run one short cooperative task step per event-loop turn. The
        // scheduler budget is one callback; a callback itself is not preempted.
        task::poll_tasks();

        browser_app.poll_network();



        // Bes parcali, HDA PCM tabanli melodi yurutucusu. (HDA yoksa PIT/PC speaker fallback).

        audio_dev.set_volume(settings_app.volume);

        audio_dev.is_muted = settings_app.is_muted;

        musicplayer_app.volume = settings_app.volume as usize;

        // Video kare sayaci gercek zamana bagli (~30 FPS) ve yalnizca oynatirken ilerler.
        if videoplayer_app.is_playing {

            let video_now_ms = monotonic_ms();

            if video_now_ms.wrapping_sub(last_video_frame_ms) >= 33 {

                last_video_frame_ms = video_now_ms;

                videoplayer_app.frame = videoplayer_app.frame.wrapping_add(1);

            }

        } else {

            last_video_frame_ms = monotonic_ms();

        }

        if musicplayer_app.is_playing && !audio_dev.is_muted && audio_dev.volume_left > 0 {

            if anim_tick.wrapping_sub(last_music_beat) >= 24 {

                const NOTES: [u32; 8] = [440, 523, 587, 659, 587, 523, 494, 392];

                let track_offset = musicplayer_app.current_track as u32 * 22;

                let freq = NOTES[music_note % NOTES.len()] + track_offset;

                // HDA hazir ve DMA ilerliyorsa gercek PCM ornegi uret ve gonder.

                if audio_dev.is_hda_active() {

                    let sample_count = 1920; // 40 ms

                    let mut pcm: [i16; 512] = [0; 512]; // 256 L + 256 R cifti

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

                    // Otomatik Fallback: HDA yok veya sanallastirma ortaminda DMA duraksadiysa PC Speaker PWM tonlama yap!

                    audio_dev.set_pc_speaker_tone(freq);

                    speaker_on = true;

                }

                music_note = (music_note + 1) % NOTES.len();

                last_music_beat = anim_tick;

            }

            // Parca 3:30 (210 sn) oldugu icin cubuk gercek zamanda her 2.1 sn'de %1 ilerler.
            let progress_now_ms = monotonic_ms();
            if progress_now_ms.wrapping_sub(last_music_progress_ms) >= 2_100 {
                last_music_progress_ms = progress_now_ms;
                musicplayer_app.progress = (musicplayer_app.progress + 1) % 101;
            }

        } else if speaker_on || audio_dev.is_muted || audio_dev.volume_left == 0 {

            drivers::hda::silence();

            audio_dev.set_pc_speaker_tone(0);

            speaker_on = false;

            last_music_beat = anim_tick;

            last_music_progress_ms = monotonic_ms();

        }



        let night_light_active = settings_app.night_light;

        // Ayarlar > Testler: fare test verisini canli besle.

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

        // Secim/konum degistiyse Hizli onizleme icerigini tazele. Kimlik anahtari

        // ayniyken cagri ucuzdur ve VFS okumasi yapmaz.

        if filemanager_app.preview_open {

            filemanager_app.update_preview(&fs_mgr.vfs);

        }

        let full_redraw = input_update.keyboard || buttons_changed || scroll_val != 0

            || force_render || busy_dirty || (mouse_moved && (is_interacting || night_light_active || settings_test_active || compositor.is_start_menu_open))

            || (timer_tick_fired && media_animating);



        if full_redraw {

            let tm_open = compositor.windows.iter().flatten().any(|w| w.app_id == 7 && w.is_open && !w.is_minimized);

            if tm_open {

                // Gorev yoneticisi istatistiklerini her frame yerine periyodik guncelle.

                use core::sync::atomic::{AtomicUsize, Ordering};

                static TM_UPDATE_TICK: AtomicUsize = AtomicUsize::new(0);

                let tick = TM_UPDATE_TICK.fetch_add(1, Ordering::Relaxed);

                if tick % 30 == 0 {

                    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);

                }

            }

            keep_windows_on_screen(&mut compositor, width, height);



            // Menuler veya modal pencereler kapaliyken tek pencere ici klavye girisi veya

            // pencere surukleme yapiliyorsa yalnizca kirli dikdortgeni (dirty rect) cizerek

            // CPU yukunu ve gereksiz VRAM kopyalamasini %60-70 azalt.

            let target_dirty = if force_render || busy_dirty || compositor.is_start_menu_open || compositor.is_context_menu_open || compositor.is_alt_tab_open || run_dialog_open || night_light_active {

                None

            } else if let Some(drag) = drag_dirty_rect {

                Some(drag)

            } else if let Some(app_dirty) = _app_dirty_rect {

                Some(app_dirty)

            } else {

                None

            };



            render_full_frame(

                &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,

                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,

                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app,

                &installer_app, &viewer3d_app, &pkgmgr_app,

                &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, memory_mgr.pmm.used_memory_bytes, anim_tick, &mut app_render_cycles,

                (run_dialog_open, &run_dialog_input),

                (properties_open, &properties_name, &properties_type, &properties_size, &properties_location, &properties_path),

                active_snap_preview,

                target_dirty,

            );

            drag_dirty_rect = None;

            _app_dirty_rect = None;

        } else if mouse_moved {

            // Imlec hareketinde uygulamalari ve tum ekrani yeniden cizmek yerine

            // sadece onceki ve yeni imlec alanlarini birlesik tek bir dikdortgen olarak guncelle.

            let old_rect = fb.restore_cursor_background();

            compositor.draw_mouse_cursor(&mut fb, mx, my, mouse.left_button);

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

            // Bosta bekleme: port 0x80 VM cikislari olmadan spin_loop ile bekle.

            // PS/2 giris tamponunda veri algilandigi anda gecikmesiz donguden cik.

            let wait_start = read_cpu_cycles();

            let wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(2_500);

            while read_cpu_cycles().wrapping_sub(wait_start) < wait_cycles {

                if drivers::ps2::has_input() || drivers::usb_hid::has_event() {

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

            let bsp_pct = if cpu_sample_total_cycles == 0 { 0 } else {

                (cpu_sample_active_cycles.saturating_mul(100) / cpu_sample_total_cycles).min(100) as usize

            };

            taskmgr_app.cpu_usage_pct = bsp_pct;



            let cpus = arch::x86_64::smp::cpu_count().min(arch::x86_64::smp::MAX_CPUS);

            taskmgr_app.core_count = cpus;

            taskmgr_app.core_usage_pct[0] = bsp_pct;



            for c in 1..cpus {

                let hb = arch::x86_64::smp::heartbeat(c);

                let diff = hb.wrapping_sub(last_ap_heartbeats[c]);

                last_ap_heartbeats[c] = hb;

                // AP dongusu her 500 bosta beklemede bir kalp atisi artirir.

                // Aktif bir is varsa poll_job yapilir veya kalp atislari canlilik gosterir.

                taskmgr_app.core_usage_pct[c] = if arch::x86_64::smp::is_ap_online(c) {

                    if diff > 0 { ((diff.min(50) * 100) / 50).min(100) as usize } else { 0 }

                } else {

                    0

                };

            }



            cpu_sample_start = now_cycles;

            cpu_sample_total_cycles = 0;

            cpu_sample_active_cycles = 0;

        }

    }

}


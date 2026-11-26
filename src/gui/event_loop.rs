use crate::apps;
use crate::arch;
use crate::boot;
use crate::boot::console::*;
use crate::boot::progress::*;
use crate::boot::time::*;
use crate::desktop;
use crate::desktop::helpers::*;
use crate::drivers;
use crate::gui;
use crate::gui::desktop_loop_state::DesktopLoopState;
use crate::gui::event_render;
use crate::gui::event_syscall;
use crate::gui::event_window;
use crate::handle_terminal_open_request;
use crate::task;
use crate::watchdog;
use crate::{alloc_pmm_frame_global, MEMORY_MGR};

/// Animasyonlu oyun pencerelerinin her tikinde yalnızca ilgili pencere
/// bölgesini (gölge payı dahil) kirli işaretler. Böylece `state.force_render`'ın
/// tetiklediği tam ekran yeniden çizim yerine kirli-dikdörtgen çizimi
/// yapılır; masaüstü arka planı ve diğer pencereler yeniden rasterize
/// edilmez.
fn track_app_window_dirty(compositor: &mut gui::MinGuiCompositor, app_id: usize) {
    let Some((idx, win)) = compositor
        .windows
        .iter()
        .enumerate()
        .find(|(_, w)| {
            w.as_ref().is_some_and(|win| {
                win.app_id as usize == app_id && win.is_open && !win.is_minimized
            })
        })
        .map(|(idx, w)| (idx, w.as_ref().unwrap()))
    else {
        return;
    };
    let x = win.x.saturating_sub(8);
    let y = win.y.saturating_sub(8);
    let x1 = win.x.saturating_add(win.width).saturating_add(8);
    let y1 = win.y.saturating_add(win.height).saturating_add(8);
    compositor.dirty_tracker.track(
        idx,
        gui::dirty_rect::DirtyRect::new(x, y, x1.saturating_sub(x), y1.saturating_sub(y)),
    );
}

/// Statik yapı boyutu ile dinamik tampon kapasitesini birleştirip uygulama
/// bellek kaydına yayınlar. `size_of_val` çağrıları çağıran tarafça yapılır;
/// böylece paint gibi birden çok örneği olan uygulamalar da aynı kimlik altında
/// toplanabilir.
fn publish_app_memory(app_id: usize, static_bytes: usize, dynamic_bytes: usize) {
    apps::memory::publish(app_id, static_bytes.saturating_add(dynamic_bytes));
}

/// Yerleşik uygulamaların son bellek ölçümünü kayda yazar ve seri bağlantıya
/// `[APP-MEM]` satırını gönderir. Çalışma zamanında periyodik çağrılır;
/// Görev Yöneticisi aynı kaydı "Bellek" sütununda kullanır.
fn publish_runtime_app_memory(
    filemanager_app: &apps::filemanager::FileManagerApp,
    desktop_files: &apps::filemanager::FileManagerApp,
    settings_app: &apps::settings::SystemSettings,
    notepad_app: &Option<apps::notepad::NotepadApp>,
    browser_app: &apps::browser::BrowserApp,
    imageviewer_app: &Option<apps::imageviewer::ImageViewerApp>,
    musicplayer_app: &apps::musicplayer::MusicPlayerApp,
    videoplayer_app: &Option<apps::videoplayer::VideoPlayerApp>,
    taskmgr_app: &apps::taskmgr::TaskManagerApp,
    calculator_app: &Option<apps::calculator::CalculatorApp>,
    paint_apps: &[Option<apps::paint::PaintApp>],
    terminal_app: &Option<apps::terminal::TerminalApp>,
    installer_app: &Option<apps::installer::InstallerApp>,
    viewer3d_app: &Option<apps::viewer3d::Viewer3DApp>,
    pkgmgr_app: &Option<apps::pkgmgr::PackageManagerApp>,
    fps_game_app: &Option<apps::game::GameApp>,
    fps_3d_app: &Option<apps::fps_game::FpsGameApp>,
    city_drive_app: &Option<apps::city_drive::CityDriveApp>,
) {
    // 0: Dosya Yöneticisi (masaüstü görünümü aynı kimliğe dahil).
    publish_app_memory(
        0,
        core::mem::size_of_val(filemanager_app),
        filemanager_app
            .memory_usage_bytes()
            .saturating_add(desktop_files.memory_usage_bytes()),
    );
    // 1..=17: kalan yerleşik uygulamalar. Dinamik tampon bildirmeyenler için
    // `size_of_val` güvenli alt sınırdır. Bellekte olmayan (None) uygulamalar
    // 0 bayt olarak raporlanır, böylece [APP-MEM] satırında görünmezler.
    publish_app_memory(1, core::mem::size_of_val(settings_app), 0);
    publish_app_memory(
        2,
        notepad_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        notepad_app
            .as_ref()
            .map(|a| a.memory_usage_bytes())
            .unwrap_or(0),
    );
    publish_app_memory(
        3,
        core::mem::size_of_val(browser_app),
        browser_app.memory_usage_bytes(),
    );
    publish_app_memory(
        4,
        imageviewer_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        imageviewer_app
            .as_ref()
            .map(|a| a.memory_usage_bytes())
            .unwrap_or(0),
    );
    publish_app_memory(5, core::mem::size_of_val(musicplayer_app), 0);
    publish_app_memory(
        6,
        videoplayer_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );
    publish_app_memory(
        7,
        core::mem::size_of_val(taskmgr_app),
        taskmgr_app.memory_usage_bytes(),
    );
    publish_app_memory(
        8,
        calculator_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );
    let paint_static = core::mem::size_of::<apps::paint::PaintApp>();
    let paint_dynamic = paint_apps
        .iter()
        .flatten()
        .map(|app| app.memory_usage_bytes())
        .sum::<usize>();
    publish_app_memory(9, paint_static, paint_dynamic);
    publish_app_memory(
        10,
        terminal_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        terminal_app
            .as_ref()
            .map(|a| a.memory_usage_bytes())
            .unwrap_or(0),
    );
    publish_app_memory(
        11,
        installer_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );
    publish_app_memory(
        12,
        viewer3d_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );
    publish_app_memory(
        13,
        pkgmgr_app.as_ref().map(core::mem::size_of_val).unwrap_or(0),
        0,
    );
    publish_app_memory(14, 0, 0); // Kontrol Paneli durumu global.
    publish_app_memory(
        15,
        fps_game_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );
    publish_app_memory(
        16,
        fps_3d_app.as_ref().map(core::mem::size_of_val).unwrap_or(0),
        0,
    );
    publish_app_memory(
        17,
        city_drive_app
            .as_ref()
            .map(core::mem::size_of_val)
            .unwrap_or(0),
        0,
    );

    // Seri döküm pahalıdır: ~18 satır 115200 baud'da yüzlerce ms UART
    // meşguliyeti ve SERIAL_LOCK tutma süresi demektir. Kayıt defteri her
    // `apps::memory` anlık görüntüsü Görev Yöneticisi tarafından doğrudan okunur.
}

/// `FileManagerApp` onay iletişim kutusu açıkken Enter onayını ilgili diyalog
/// türüne dağıtır. Diyalog tüketildiyse `true` döner.
fn fm_confirm_accept(
    app: &mut apps::filemanager::FileManagerApp,
    vfs: &mut crate::fs::vfs::VirtualFileSystem,
) -> bool {
    use apps::filemanager::ConfirmDialog;
    match app.confirm_dialog {
        ConfirmDialog::NewFile => app.confirm_new_file(vfs),
        ConfirmDialog::NewFolder => app.confirm_new_folder(vfs),
        ConfirmDialog::Rename => app.confirm_rename(vfs),
        ConfirmDialog::Delete | ConfirmDialog::DeletePermanent => {
            app.confirm_delete(vfs);
        }
        ConfirmDialog::None => return false,
    }
    true
}

/// `FileManagerApp` onay iletişim kutusu açıkken tuş eylemini diyaloğa
/// yönlendirir; olay tüketildiyse `true` döner.
fn fm_confirm_key(
    app: &mut apps::filemanager::FileManagerApp,
    vfs: &mut crate::fs::vfs::VirtualFileSystem,
    action: drivers::keyboard::KeyAction,
) -> bool {
    use apps::filemanager::ConfirmDialog;
    if app.confirm_dialog == ConfirmDialog::None {
        return false;
    }
    match action {
        drivers::keyboard::KeyAction::EnterKey => fm_confirm_accept(app, vfs),
        drivers::keyboard::KeyAction::ArrowLeft => {
            app.confirm_rename_move_cursor(false);
            true
        }
        drivers::keyboard::KeyAction::ArrowRight => {
            app.confirm_rename_move_cursor(true);
            true
        }
        drivers::keyboard::KeyAction::Delete => {
            app.confirm_rename_delete();
            true
        }
        _ => true,
    }
}

/// `FileManagerApp` onay iletişim kutusu açıkken karakter girişini diyaloğa
/// yönlendirir; olay tüketildiyse `true` döner.
fn fm_confirm_char(
    app: &mut apps::filemanager::FileManagerApp,
    vfs: &mut crate::fs::vfs::VirtualFileSystem,
    ch: char,
) -> bool {
    use apps::filemanager::ConfirmDialog;
    if app.confirm_dialog == ConfirmDialog::None {
        return false;
    }
    match ch {
        '\x1b' => app.cancel_confirm(),
        '\x08' => app.confirm_rename_backspace(),
        '\n' | '\r' => {
            fm_confirm_accept(app, vfs);
        }
        ch if ch >= ' ' && ch != '\x7f' => app.confirm_rename_char(ch),
        _ => {}
    }
    true
}

/// Kapanış/yeniden başlatma akışı: görünür ilerleme, VFS senkronizasyonu,
/// sürücü kapatma ve DONANIM reseti. Tüm kapatma giriş noktaları burayı
/// kullanır; böylece kullanıcı "kaydediliyor / boşaltılıyor / kapatılıyor"
/// adımlarını görür ve diskteki değişiklikler yazılmadan güç kesilmez.
fn finish_system(
    fb: &mut drivers::video::Framebuffer,
    fs_mgr: &crate::fs::FileSystemManager,
    width: usize,
    height: usize,
    restart: bool,
) -> ! {
    serial_print(if restart {
        "[SHUTDOWN] Yeniden başlatma isteği alındı.\n"
    } else {
        "[SHUTDOWN] Sistem kapatma isteği alındı.\n"
    });

    // 0: değişiklikler, 1: disk önbelleği, 2: sürücüler, 3: son adım.
    for phase in 0..2 {
        gui::render_shutdown_screen(fb, width, height, phase, restart);
        let start = monotonic_ms();
        while monotonic_ms().saturating_sub(start) < 120 {
            core::hint::spin_loop();
        }
    }

    let (dirs, files) = fs_mgr.vfs.sync_all_to_disk();
    let mut sync_line = gui::TextBuf::new();
    sync_line.push_str("[SHUTDOWN] VFS disk senkronizasyonu: ");
    sync_line.push_usize(dirs);
    sync_line.push_str(" dizin, ");
    sync_line.push_usize(files);
    sync_line.push_str(" dosya\n");
    serial_print(sync_line.as_str());

    gui::render_shutdown_screen(fb, width, height, 2, restart);
    let start = monotonic_ms();
    while monotonic_ms().saturating_sub(start) < 120 {
        core::hint::spin_loop();
    }

    drivers::framework::shutdown_all();
    serial_print("[SHUTDOWN] Suruculer kapatildi.\n");
    gui::render_shutdown_screen(fb, width, height, 3, restart);
    let start = monotonic_ms();
    while monotonic_ms().saturating_sub(start) < 120 {
        core::hint::spin_loop();
    }

    if restart {
        restart_system();
    }

    // ACPI PM1a yazılım yolu (FADT PM1a_CNT_BLK); yazım başarısızsa
    // veya makine kapanmazsa aşağıdaki QEMU/legacy fallback çalışır.
    drivers::acpi_power::shutdown();

    unsafe {
        // QEMU/Bochs ACPI poweroff (0x604), ardından legacy fallback.
        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
        core::arch::asm!("out dx, ax", in("dx") 0x404u16, in("ax") 0x3400u16);
        core::arch::asm!("cli; hlt");
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Önyükleme sonrası masaüstü çalışma zamanı ve ana olay döngüsü.
/// `main.rs` içindeki eski `_start` gövdesinden taşındı.
pub fn run_desktop(display: boot::start::BootDisplay) -> ! {
    let boot::start::BootDisplay {
        mut fb,
        fb_ptr,
        mut width,
        mut height,
        pitch: _,
        bpp,
    } = display;

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

    serial_print("[BOOT] Preparing window manager and applications...\n");
    let mut boot_timer = crate::boot::timing::PhaseTimer::new();

    // Kurulum `Box` döner; içerikler ayrılan (heap) yerde açıldığı için
    // ~110 KiB'lık yapılar yığın çerçevesine kopyalanmaz ve dönüş değerinin
    // (RVO'nun) yığın kullanması gerekmez.
    let gui::setup::AppSetup {
        mut apps,
        mut compositor,
    } = *gui::setup::init_apps(&mut fs_mgr, width, height);

    let mut desktop_drag_source = false;
    let mut file_clipboard = apps::filemanager::FileClipboard::new();

    // Eger Limine onyukleme komut satirinda res=WxH secilmisse oncelik ver:
    if let Some(cmd_res) = boot::limine::cmdline_resolution() {
        apps.settings_app.screen_res = cmd_res;
    }

    // Secili cozunurluk su anki ekrandan farkliysa ve GPU tarafindan
    // destekleniyorsa acilista uygula:
    let target_res = apps.settings_app.screen_res;
    if target_res != (fb.width, fb.height)
        && drivers::gpu::mode_supported(target_res.0, target_res.1)
    {
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
                    boot::text_console::update_framebuffer(drivers::video::Framebuffer::new(
                        fb_ptr, width, height, p, bpp,
                    ));
                }
                apps.settings_app.display_res = target_res;
                serial_print("[BOOT] Secili cozunurluk basariyla uygulandi: ");
                serial_print_dec(width as u64);
                serial_print("x");
                serial_print_dec(height as u64);
                serial_print("\n");
            }
        }
    }

    gui::setup::apply_user_settings(&apps.settings_app, &fb, mouse, keyboard, width, height);

    gui::setup::register_default_windows(&mut compositor, width, height);
    // Kurulu sistemse welcome ekranini atla, live ortam ise goster.

    serial_print("[BOOT] Preparing welcome screen; running graphics self-checks.\n");

    // Widget/Component agaci mimarisini (yerlesim, Tab odagi, olaylar) dogrula.

    if !gui::widget::self_test() {
        boot::text_console::fatal("Grafik arayuz oz-denetimi basarisiz; GUI acilmadi");
    }

    // Açılış/kapanış/hata ekranlarının geometrisi host testlerinde de
    // doğrulanan saf modülle aynı kaynaktan gelir; bozulursa erken durdur.
    if !gui::screen_layout::self_test() {
        boot::text_console::fatal("Ekran yerlesim oz-denetimi basarisiz; GUI acilmadi");
    }
    serial_print("[GUI-SMOKE] ekran geometrisi ve ekran metinleri: BASARILI\n");

    serial_print("[BOOT] Grafik arayüz denetimleri tamamlandı.\n");
    boot_timer.mark("gui-kurulum");

    serial_print("[BOOT] CRITICAL_CHECKS_COMPLETE - Kritik denetimler tamamlandı; grafik arayüz başlatılıyor.\n");

    boot::text_console::disable();

    if installed {
        serial_print("[BOOT] Installed system detected; preparing desktop...\n");

        for phase in 0..4 {
            // PNG duvar kağıdı küçük resimlerini önyükleme sırasında tek seferlik
            // çöz; böylece ekran ayarları açıldığında fare takılmaz.
            crate::gui::screens::set_cursor_busy(true);
            match phase {
                0 => crate::gui::wallpaper::ensure_thumb(0),
                1 => crate::gui::wallpaper::ensure_thumb(1),
                2 => crate::gui::wallpaper::ensure_thumb(2),
                _ => crate::gui::wallpaper::ensure_all_thumbs(),
            }
            crate::gui::screens::set_cursor_busy(false);

            gui::render_loading_transition(&mut fb, width, height, phase);

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 25 {
                core::hint::spin_loop();
            }
        }
    } else {
        for phase in 0..8 {
            crate::gui::screens::set_cursor_busy(true);
            if phase == 0 {
                crate::gui::wallpaper::ensure_all_thumbs();
            }
            crate::gui::screens::set_cursor_busy(false);

            gui::render_loading_transition(&mut fb, width, height, phase);

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 {
                core::hint::spin_loop();
            }
        }
    }

    if !installed {
        desktop::welcome::run_welcome_screen(
            &mut fb,
            width,
            height,
            mouse,
            keyboard,
            &mut apps.settings_app,
            &compositor,
            &mut fs_mgr,
            memory_mgr,
        );
        serial_print("[BOOT] Welcome screen accepted; switching to desktop.\n");
        boot_timer.mark("hos-geldin");
    }

    // Welcome ekranindan masaustune gecisi de ayni animasyonla yumusat.

    // Sadece live ortamda goster; kurulu sistemde atla.

    if !installed {
        for phase in 0..6 {
            gui::render_loading_transition(&mut fb, width, height, phase + 2);

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 {
                core::hint::spin_loop();
            }
        }
    }

    // RTC - gercek zamanli saat

    let mut now = drivers::rtc::DateTime::read_for_region(apps.settings_app.region_offset());

    compositor.cal_year = now.year;

    compositor.cal_month = now.month;

    #[cfg(minos_boot_log)]
    serial_print("[minOs] RTC saati etkin.\n");

    // Save once the boot transcript is complete, including numeric fields.

    save_boot_log(&mut fs_mgr.vfs);

    // Acilis melodisi (Chime)

    audio_dev.set_volume(apps.settings_app.volume);

    audio_dev.is_muted = apps.settings_app.is_muted;

    audio_dev.play_chime();

    // Etkileşim durumu

    let mut state = DesktopLoopState::new(now);

    // Uygulama bazlı bellek ölçümü: ilk turda da yayınla.
    apps.taskmgr_app.update(
        &memory_mgr.pmm,
        &fs_mgr.vfs,
        &compositor.windows,
        &state.app_render_cycles,
    );

    // Tam kare cizimi gui::frame::render_full_frame icinde yasar.

    // Karsilama bildirimi (Modern Toast)

    let welcome_title = gui::ui_text("minOs'a Hoş Geldiniz", "Welcome to minOs");

    let welcome_msg = gui::ui_text(
        "Sistem hazır. Başlat menüsü ve kısayollar etkin.",
        "System is ready. Start menu and shortcuts active.",
    );

    compositor.push_toast(
        gui::ToastKind::Info,
        welcome_title,
        welcome_msg,
        monotonic_ms(),
        5500,
    );

    // Ilk kareyi ciz (tum ekran). Uygulama icerigi icin bir kurtarma noktasi
    // acilir; hatali bir uygulama acilistaki kareyi dusuremez.
    let initial_context = compositor
        .active_window_idx
        .and_then(|idx| compositor.windows[idx].map(|w| (w.app_id as usize, idx)))
        .unwrap_or((apps::guard::NO_APP, apps::guard::NO_WINDOW));

    let (initial_guard, initial_start) = apps::guard::prepare(initial_context.0, initial_context.1);

    if unsafe { apps::guard::begin(initial_start) } {
        let _initial_guard = initial_guard;

        gui::frame::render_full_frame(
            &mut fb,
            &compositor,
            &apps.filemanager_app,
            &apps.desktop_files,
            &apps.settings_app,
            &mut apps.notepad_app,
            &apps.browser_app,
            &mut apps.imageviewer_app,
            &apps.musicplayer_app,
            &mut apps.videoplayer_app,
            &apps.taskmgr_app,
            &mut apps.calculator_app,
            &apps.paint_apps,
            &mut apps.terminal_app,
            &mut apps.installer_app,
            &mut apps.viewer3d_app,
            &mut apps.pkgmgr_app,
            &mut apps.fps_game_app,
            &mut apps.fps_3d_app,
            &mut apps.city_drive_app,
            &state.now,
            mouse,
            keyboard,
            memory_mgr.pmm.total_memory_bytes,
            memory_mgr.pmm.used_memory_bytes,
            state.anim_tick,
            &mut state.app_render_cycles,
            &state.app_render_time_ms,
            (state.run_dialog_open, &mut state.run_dialog_input),
            (
                state.properties_open,
                &state.properties_name,
                &state.properties_type,
                &state.properties_size,
                &state.properties_location,
                &state.properties_path,
            ),
            None,
            None,
        );

        // İzleme damgası: ilk karede açık pencereleri canlı olarak işaretle.
        for (idx, slot) in compositor.windows.iter().enumerate().take(gui::MAX_WINDOWS) {
            if let Some(win) = slot {
                if win.is_open && !win.is_minimized {
                    watchdog::heartbeat(idx, win.app_id as usize);
                }
            }
        }
    }

    state.last_workspace = compositor.active_workspace;

    // Oyun bütçesi sabitleri
    const GAME_UPDATE_BUDGET_MS: u64 = 50;
    const GAME_BUDGET_MAX_STRIKES: u32 = 2;
    const GAME_FPS_LIMIT_MS: u64 = 33;
    const FPS3D_FPS_LIMIT_MS: u64 = 16;
    // Oyun tick zamanları (döngü içinde kullanılır)
    let mut last_game_tick_ms: u64 = monotonic_ms();
    let mut last_fps3d_tick_ms: u64 = monotonic_ms();
    let mut last_city_tick_ms: u64 = monotonic_ms();

    // 7. Ana Masaustu Etkilesim ve Olay Dongusu

    serial_print("[BOOT] Desktop loop started.\n");

    // Önyükleme komut satırında `browserurl=<url>` varsa tarayıcı bu adresle
    // açılır. Canlı doğrulama (QEMU + gerçek sunucu) için kullanılır: gerçek
    // bir sitede sayfa yükleme, kaynak indirme ve keep-alive akışı ölçülür.
    if let Some(cmdline) = crate::boot::limine::limine_boot_cmdline() {
        for tok in cmdline.split_whitespace() {
            if let Some(url) = tok.strip_prefix("browserurl=") {
                if !url.is_empty() {
                    apps.browser_app.set_url(url.as_bytes());
                    apps.browser_app.navigate();
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 3);
                    serial_print("[BOOT] browserurl=");
                    serial_print(url);
                    serial_print("\n");
                    break;
                }
            }
        }
    }

    // Önyükleme komut satırında `perftest` varsa Kontrol Paneli "Testler"
    // sekmesi açılır ve performans testi otomatik başlatılır. Sonuç hem
    // ekranda hem seri konsolda `[PERF]` satırı olarak yayınlanır; sanal
    // makine ve gerçek PC doğrulaması için kullanılır.
    if crate::boot::limine::limine_boot_cmdline()
        .map(|c| crate::boot::limine::cmdline_has_token(c, "perftest"))
        .unwrap_or(false)
    {
        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 14);
        apps::controlpanel::select_tab(3);
        apps::controlpanel::start_perf_test();
        // Performans testi sekmesi için pencere boyutunu içeriğe göre ayarla.
        if let Some((cidx, _)) = focused_window_for_app(&compositor, 14) {
            if let Some(w) = compositor.windows[cidx].as_mut() {
                let (cw, ch) = apps::controlpanel::content_extent();
                let titlebar_h = crate::gui::scaled_titlebar_h();
                let taskbar_h = crate::gui::scaled_taskbar_h();
                let usable_h = height.saturating_sub(taskbar_h);
                w.width = cw.min(width).max(w.width);
                w.height = ch
                    .saturating_add(titlebar_h + 1)
                    .min(usable_h)
                    .max(w.height);
                if w.y + w.height > usable_h {
                    w.y = usable_h.saturating_sub(w.height);
                }
            }
        }
        serial_print("[BOOT] perftest=1: performans testi baslatildi.\n");
    }

    // Uygulama bazlı bellek raporu döngünün ilk turunda da yayınlanır; ancak
    // TCG altında ilk kare yavaş olabildiği için masaüstüne geçişte bir kez
    // hemen yayınlanır. Böylece `[APP-MEM]` işareti sistemin gerçekten
    // çalıştığını doğrulayan testler tarafından geç yakalanır.
    publish_runtime_app_memory(
        &apps.filemanager_app,
        &apps.desktop_files,
        &apps.settings_app,
        &apps.notepad_app,
        &apps.browser_app,
        &apps.imageviewer_app,
        &apps.musicplayer_app,
        &apps.videoplayer_app,
        &apps.taskmgr_app,
        &apps.calculator_app,
        &apps.paint_apps,
        &apps.terminal_app,
        &apps.installer_app,
        &apps.viewer3d_app,
        &apps.pkgmgr_app,
        &apps.fps_game_app,
        &apps.fps_3d_app,
        &apps.city_drive_app,
    );

    // Açılış süresi özeti: Limine'den devralmadan masaüstüne kadar geçen süre.
    boot_timer.mark("masaustu");
    crate::boot::timing::report_total();

    // Boşta güç tüketimi: BSP kesmelerini aç ve periyodik APIC timerinin
    // gerçekten teslim edildiğini doğrula. Doğrulanırsa ana döngü boşta
    // `hlt` ile uyur (CPU ~%0); aksi halde yoklamalı bekleme korunur.
    if arch::x86_64::apic::enable_bsp_idle() {
        serial_print("[IDLE] BSP timer interrupt verified; idle HLT enabled.\n");
    } else {
        serial_print("[IDLE] WARNING: BSP timer interrupt unavailable; idle polling mode.\n");
    }

    loop {
        // Timer doğrulansa da doğrulanmasa da güvenli idle fallback'i kullan.
        arch::x86_64::apic::idle_pause();
        // Bir onceki turda yakalanan uygulama hatasini raporla ve yalnizca
        // hatali uygulamayi kapat; masaustu ve diger uygulamalar yasamaya
        // devam eder.
        if let Some(fault) = apps::guard::take_pending() {
            let crash = apps::errors::AppError::new("MinOs", apps::errors::AppErrorKind::Crashed);
            let mut line = gui::TextBuf::new();
            line.push_str("[GUARD] ");
            line.push_str(crash.kind.code());
            line.push_str(" ");
            line.push_str(fault.kind.as_str());
            line.push_str(" vec=");
            line.push_usize(fault.vector as usize);
            line.push_str(" app=");
            line.push_usize(fault.app_id);
            line.push_str(" win=");
            line.push_usize(fault.window_idx);
            line.push_str(" rip=0x");
            line.push_hex64(fault.rip);
            line.push_str(" err=0x");
            line.push_hex64(fault.error_code);
            line.push_str(" cr2=0x");
            line.push_hex64(fault.cr2);
            line.push_str("\n");
            serial_print(line.as_str());

            if fault.kind == apps::guard::FaultKind::Panic && !fault.file.is_empty() {
                serial_print("[GUARD] Konum: ");
                serial_print(fault.file);
                serial_print(":");
                serial_print_dec(fault.line as u64);
                serial_print("\n");
            }
            serial_print("[GUARD] ");
            serial_print(crash.kind.code());
            serial_print(" ");
            serial_print(crash.recovery(gui::ui_english()));
            serial_print("\n");

            let window_idx = if fault.window_idx < gui::MAX_WINDOWS
                && compositor.windows[fault.window_idx].is_some_and(|w| w.is_open)
            {
                Some(fault.window_idx)
            } else if fault.app_id != apps::guard::NO_APP {
                compositor.windows.iter().position(|slot| {
                    slot.is_some_and(|w| w.is_open && w.app_id as usize == fault.app_id)
                })
            } else {
                None
            };

            if let Some(idx) = window_idx {
                if fault.app_id != apps::guard::NO_APP {
                    terminate_app(idx, &mut compositor, &mut apps);
                }
            }

            let mut title = gui::TextBuf::new();
            title.push_str(crash.kind.code());
            title.push_str(" · ");
            title.push_str(crash.title(gui::ui_english()));

            // Bildirim gövdesinde somut kurtarma adımı; uygulama adı seri
            // günlüğünde ve [APP-MEM] kaydında zaten yer alıyor.
            let mut message = gui::TextBuf::new();
            message.push_str(crash.recovery(gui::ui_english()));
            compositor.push_toast(
                gui::ToastKind::Error,
                title.as_str(),
                message.as_str(),
                monotonic_ms(),
                6000,
            );
        }

        // Uygulama bazlı bellek ölçümü: her 90 turda bir yayınla. Böylece
        // Görev Yöneticisi ve `[APP-MEM]` seri satırları aynı kaynağı görür.
        static APP_MEM_TICK: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        if APP_MEM_TICK.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % 90 == 0 {
            publish_runtime_app_memory(
                &apps.filemanager_app,
                &apps.desktop_files,
                &apps.settings_app,
                &apps.notepad_app,
                &apps.browser_app,
                &apps.imageviewer_app,
                &apps.musicplayer_app,
                &apps.videoplayer_app,
                &apps.taskmgr_app,
                &apps.calculator_app,
                &apps.paint_apps,
                &apps.terminal_app,
                &apps.installer_app,
                &apps.viewer3d_app,
                &apps.pkgmgr_app,
                &apps.fps_game_app,
                &apps.fps_3d_app,
                &apps.city_drive_app,
            );
        }

        let focused_context = compositor
            .active_window_idx
            .and_then(|idx| compositor.windows[idx].map(|w| (w.app_id as usize, idx)))
            .unwrap_or((apps::guard::NO_APP, apps::guard::NO_WINDOW));

        // Bu turun tamami, odakli uygulamanin baglamiyla korunur. `prepare`
        // baglami kurar; `begin` dogrudan bu fonksiyonun canli yigin
        // cercevesinden kurtarma noktasi alir. Hata olursa `begin` false
        // doner ve tur atlanir; hata bir sonraki turda raporlanir.
        let (runtime_guard, runtime_start) =
            apps::guard::prepare(focused_context.0, focused_context.1);

        if !unsafe { apps::guard::begin(runtime_start) } {
            continue;
        }

        let _runtime_guard = runtime_guard;

        apps::browser::begin_turn();

        mouse.speed = apps.settings_app.mouse_speed;

        mouse.reverse_scroll = apps.settings_app.reverse_scroll;

        gui::anim::set_reduced_motion(apps.settings_app.reduced_motion);
        gui::set_transparent_windows(apps.settings_app.transparent_windows);

        let mut input_update =
            drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);

        let usb_input_update = drivers::usb_hid::poll_input(
            &mut *mouse,
            &mut *keyboard,
            width as i32,
            height as i32,
            &mut memory_mgr.pmm,
        );

        input_update.keyboard |= usb_input_update.keyboard;

        input_update.mouse |= usb_input_update.mouse;

        // PS/2 kesme kuyruğu: IRQ1/IRQ12 olaylarını durum makinelerine uygula.
        let irq_update = drivers::ps2::drain_input_events(
            &mut *mouse,
            &mut *keyboard,
            width as i32,
            height as i32,
        );

        input_update.keyboard |= irq_update.keyboard;

        input_update.mouse |= irq_update.mouse;

        // Update activity timestamps for focused application on input
        if input_update.keyboard || input_update.mouse {
            if let Some(idx) = compositor.active_window_idx {
                if idx < gui::MAX_WINDOWS {
                    if let Some(win) = compositor.windows[idx] {
                        watchdog::heartbeat(idx, win.app_id as usize);
                    }
                }
            }
        }

        state.loop_counter = state.loop_counter.wrapping_add(1);

        let mx = mouse.x as usize;

        let my = mouse.y as usize;

        // MouseKeys sentetik tıklama darbesini gerçek fare sinyali gibi üret:
        // bir tur basılı, ertesi tur bırakılı görünsün ki basma/bırakma
        // kenarları (mouse_clicked / mouse_released) normal yoldan işlensin.
        match state.mousekeys_click_left {
            1 => {
                mouse.left_button = true;
                state.mousekeys_click_left = 2;
            }
            2 => {
                mouse.left_button = false;
                state.mousekeys_click_left = 0;
            }
            _ => {}
        }
        match state.mousekeys_click_right {
            1 => {
                mouse.right_button = true;
                state.mousekeys_click_right = 2;
            }
            2 => {
                mouse.right_button = false;
                state.mousekeys_click_right = 0;
            }
            _ => {}
        }

        gui::MinGuiCompositor::set_pointer_state(mx, my, mouse.left_button);

        let mouse_clicked = mouse.left_button && !state.last_buttons.0;

        let mouse_released = !mouse.left_button && state.last_buttons.0;

        let right_clicked = mouse.right_button && !state.last_buttons.1;

        let l = gui::MinGuiCompositor::tray_layout(width, height);

        let tb_y = l.tb_y;

        // RTC saatini islem dongusu sayisina gore degil, gecen gercek zamana gore yenile.

        state.force_render = false;

        // USB Hotplug (tak/çıkar) bildirimleri ve donanım envanteri canlı güncellemesi
        while let Some(ev) = drivers::usb_hid::pop_hotplug_event() {
            state.force_render = true;
            drivers::framework::broadcast_hotplug(match ev.action {
                drivers::usb_hid::HotplugAction::Connected => drivers::HotplugEvent::Added,
                drivers::usb_hid::HotplugAction::Disconnected => drivers::HotplugEvent::Removed,
            });
            let is_en = apps.settings_app.language.is_english();
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
                (drivers::usb_hid::HidKind::Touchscreen, false) => "USB Dokunmatik Ekran",
                (drivers::usb_hid::HidKind::Touchscreen, true) => "USB Touchscreen",
            };
            let toast_title = if is_en {
                "USB Device Event"
            } else {
                "USB Aygıt Olayı"
            };
            let mut toast_msg = [0u8; 96];
            let mut msg_len = 0;
            for b in dev_str.as_bytes() {
                if msg_len < toast_msg.len() {
                    toast_msg[msg_len] = *b;
                    msg_len += 1;
                }
            }
            if msg_len < toast_msg.len() {
                toast_msg[msg_len] = b' ';
                msg_len += 1;
            }
            for b in action_str.as_bytes() {
                if msg_len < toast_msg.len() {
                    toast_msg[msg_len] = *b;
                    msg_len += 1;
                }
            }
            let msg_text = core::str::from_utf8(&toast_msg[..msg_len]).unwrap_or("USB Event");
            compositor.push_toast(
                if ev.action == drivers::usb_hid::HotplugAction::Connected {
                    gui::ToastKind::Success
                } else {
                    gui::ToastKind::Info
                },
                toast_title,
                msg_text,
                monotonic_ms(),
                3500,
            );
        }
        if drivers::usb_hid::check_and_clear_hotplug_dirty() {
            state.force_render = true;
        }

        // USB depolama (ATA köprülü bellek) sıcak takma taraması.
        // IDENTIFY port I/O'su ağır olduğu için ~2 sn'de bir çalışır.
        //drivers::usb::poll_storage_hotplug(2_000);
        while let Some(ev) = drivers::usb::pop_storage_hotplug_event() {
            state.force_render = true;
            // Disk envanteri değişti: dosya yöneticisi ve masaüstü
            // simgeleri yeni `/media/usb0` durumuyla yenilenir.
            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
            apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

            let is_en = apps.settings_app.language.is_english();
            let action_str = match (ev.action, is_en) {
                (drivers::usb::StorageHotplugAction::Connected, false) => "takıldı",
                (drivers::usb::StorageHotplugAction::Connected, true) => "connected",
                (drivers::usb::StorageHotplugAction::Disconnected, false) => "çıkarıldı",
                (drivers::usb::StorageHotplugAction::Disconnected, true) => "disconnected",
            };
            let dev_str = if is_en { "USB Disk" } else { "USB Bellek" };
            let toast_title = if is_en {
                "USB Storage Event"
            } else {
                "USB Depolama Olayı"
            };
            let mut toast_msg = [0u8; 96];
            let mut msg_len = 0;
            for b in dev_str.as_bytes() {
                if msg_len < toast_msg.len() {
                    toast_msg[msg_len] = *b;
                    msg_len += 1;
                }
            }
            if msg_len < toast_msg.len() {
                toast_msg[msg_len] = b' ';
                msg_len += 1;
            }
            for b in action_str.as_bytes() {
                if msg_len < toast_msg.len() {
                    toast_msg[msg_len] = *b;
                    msg_len += 1;
                }
            }
            let msg_text = core::str::from_utf8(&toast_msg[..msg_len]).unwrap_or("USB Disk");
            compositor.push_toast(
                if ev.action == drivers::usb::StorageHotplugAction::Connected {
                    gui::ToastKind::Success
                } else {
                    gui::ToastKind::Info
                },
                toast_title,
                msg_text,
                monotonic_ms(),
                3500,
            );
        }

        let clock_now_ms = monotonic_ms();

        if clock_now_ms.wrapping_sub(state.last_clock_update_ms) >= 1_000 {
            state.last_clock_update_ms = clock_now_ms;

            now = drivers::rtc::DateTime::read_for_region(apps.settings_app.region_offset());

            // Otomatik gundus/gece temasini senkronla: aktifse saati bazli tema
            // uygulanir; tema degisikligi varsa yeniden cizim (state.force_render) gerekir.
            let auto_theme_id = gui::theme::sync_auto_theme_at(now.hour, now.minute);
            if auto_theme_id != gui::theme::current() {
                gui::theme::set_theme(auto_theme_id);
                state.force_render = true;
            }

            // Saat güncellemesi tam ekran yerine yalnızca görev çubuğu
            // saat bölgesini kirli işaretler; state.force_render devre dışı.
            // Clock update marks only the taskbar clock rect dirty instead
            // of forcing a full-screen redraw.
            let clock_rect =
                gui::dirty_rect::DirtyRect::new(l.clock.0, l.clock.1, l.clock.2, l.clock.3);
            compositor.dirty_tracker.track(0, clock_rect);
        }

        // Toast bildirim surelerini guncelle

        let had_toasts = compositor.toasts.iter().any(|t| t.is_some());

        // Fare bir bildirimin üzerindeyken otomatik kapanma sayacını
        // duraklat; uzun mesajlar okunurken kaybolmaz.
        compositor.hover_toasts(width, tb_y, mx, my, clock_now_ms);

        compositor.update_toasts(clock_now_ms);

        let has_toasts = compositor.toasts.iter().any(|t| t.is_some());

        if had_toasts != has_toasts {
            state.force_render = true;
        }

        // Giriş/çıkış animasyonundaki toast'lar için sağ üst bölgeyi her
        // karede kirli işaretle; animasyon kendi kendine ilerlesin.
        let toast_animating = compositor.toasts.iter().flatten().any(|t| {
            t.exit_ms != 0
                || clock_now_ms.wrapping_sub(t.created_ms)
                    < gui::MinGuiCompositor::TOAST_SLIDE_IN_MS
        });
        if toast_animating {
            let region = gui::sp(320);
            compositor.dirty_tracker.track(
                0,
                crate::gui::dirty_rect::DirtyRect::new(
                    width.saturating_sub(region),
                    tb_y.saturating_sub(region),
                    region,
                    region,
                ),
            );
        }

        // Ses cihazini ayarlarla senkronla

        audio_dev.set_volume(apps.settings_app.volume);

        audio_dev.is_muted = apps.settings_app.is_muted;

        apps.musicplayer_app.volume = apps.settings_app.volume as usize;

        // 1. Fare Sag Tik — tiklanan yere gore baglam menusu

        if right_clicked {
            compositor.is_calendar_open = false;

            compositor.is_volume_popup_open = false;

            compositor.close_start_menu();

            compositor.is_context_menu_open = true;

            compositor.context_menu_open_ms = monotonic_ms();

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

                    let inside = mx >= win.x
                        && mx < win.x + win.width
                        && my >= win.y
                        && my < win.y + win.height;

                    if !inside {
                        continue;
                    }

                    compositor.context_window = Some(idx);

                    if my < win.y + gui::scaled_titlebar_h() {
                        hit_window = Some((idx, gui::ContextKind::Window));
                    } else if win.app_id == 15 {
                        // Oyunlar: Mayın Tarlası'nda sağ tık bayrak koyar.
                        // Dikkat: `idx` pencere yuvası indeksidir; uygulama
                        // kimliğiyle karıştırılırsa başka bir uygulamanın
                        // penceresi oyun sanılır ve `fps_game_app` `None`
                        // iken unwrap panik üretir.
                        compositor.focus_window(idx);
                        let (ax, ay, aw, ah) = app_layout_rect(15, &win);
                        if apps.fps_game_app.as_mut().unwrap().right_click(
                            mx.saturating_sub(ax),
                            my.saturating_sub(ay),
                            aw,
                            ah,
                        ) {
                            compositor.is_context_menu_open = false;
                            state.force_render = true;
                            break;
                        }
                        hit_window = Some((idx, gui::ContextKind::Window));
                    } else if win.app_id == 10 {
                        // Terminal / CMD: Sağ tık seçimi panoya kopyalar veya panoyu yapıştırır
                        compositor.focus_window(idx);
                        if apps.terminal_app.as_mut().unwrap().has_selection() {
                            apps.terminal_app.as_mut().unwrap().copy_selection();
                            apps.terminal_app.as_mut().unwrap().clear_selection();
                            compositor.is_context_menu_open = false;
                            break;
                        } else if crate::gui::clipboard::len() > 0 {
                            apps.terminal_app.as_mut().unwrap().paste_clipboard();
                            compositor.is_context_menu_open = false;
                            break;
                        } else {
                            hit_window = Some((idx, gui::ContextKind::Window));
                        }
                    } else if win.app_id == 3 {
                        // Tarayıcı: Seçili metin varsa sağ tık panoya kopyalar
                        compositor.focus_window(idx);
                        if apps.browser_app.has_page_selection() {
                            apps.browser_app.copy_page_selection();
                            compositor.is_context_menu_open = false;
                            break;
                        } else {
                            hit_window = Some((idx, gui::ContextKind::Window));
                        }
                    } else if win.app_id == 2 {
                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::Notepad));
                    } else if win.app_id == 0 {
                        if let Some(i) = apps.filemanager_app.hit_test(
                            win.x,
                            win.y,
                            win.width,
                            win.height,
                            mx,
                            my,
                            win.scroll.y,
                        ) {
                            compositor.context_file_idx = Some(i);

                            apps.filemanager_app.selected_idx = i;
                        }

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::FileManager));
                    } else if win.app_id == 7 {
                        if apps.taskmgr_app.active_tab == 0 {
                            // Görev yöneticisi içeriği kaydırılabildiği için
                            // isabet testi çizimle aynı ötelenmiş orijini kullanır.
                            let row_x = win.x.saturating_sub(win.scroll.x);
                            let row_y = win.y.saturating_sub(win.scroll.y);
                            if let Some(row) =
                                apps.taskmgr_app.row_at(mx, my, row_x, row_y, win.width)
                            {
                                apps.taskmgr_app.selected = row;

                                apps.taskmgr_app.focused_control = 4 + row;

                                hit_window = Some((idx, gui::ContextKind::TaskManager));
                            } else {
                                hit_window = Some((idx, gui::ContextKind::Window));
                            }
                        } else {
                            hit_window = Some((idx, gui::ContextKind::Window));
                        }

                        compositor.focus_window(idx);
                    } else if win.app_id == 9 {
                        // Paint: tuval üzerinde sağ tık silgi darbesi başlatır;
                        // bağlam menüsü açılmaz. Şerit üzerinde sağ tık
                        // genel pencere menüsüne düşer.
                        compositor.focus_window(idx);
                        let (ax, ay, aw, ah) = app_layout_rect(9, &win);
                        let (cx, cy, cw, ch) = apps::paint::PaintApp::canvas_rect(aw, ah);
                        let in_canvas = mx >= ax + cx
                            && mx < ax + cx + cw
                            && my >= ay + cy
                            && my < ay + cy + ch;
                        if in_canvas {
                            // hit_window None kalır → menü açılmaz.
                        } else {
                            hit_window = Some((idx, gui::ContextKind::Window));
                        }
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
                let selected = apps.desktop_files.selected_items;

                let click_idx = apps.desktop_files.last_click_idx;

                let click_ms = apps.desktop_files.last_click_ms;

                apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                apps.desktop_files.selected_items = selected;

                apps.desktop_files.last_click_idx = click_idx;

                apps.desktop_files.last_click_ms = click_ms;

                if let Some((desk_idx, _)) =
                    desktop_item_at(mx, my, &apps.desktop_files, width, height)
                {
                    compositor.context_file_idx = Some(desk_idx);

                    apps.desktop_files
                        .select_row(desk_idx, keyboard.ctrl, keyboard.shift);

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
                    menu_y,
                    compositor.active_start_group,
                    tb_y,
                    width,
                );

                if !gui::in_rect(mx, my, (menu_x, menu_y, menu_w + 6, menu_h))
                    && !gui::in_rect(mx, my, submenu)
                {
                    compositor.close_start_menu();
                }
            }

            if compositor.is_context_menu_open {
                let hit_main = compositor.context_hit(width, height, mx, my).is_some();

                let hit_sub = compositor
                    .context_submenu_hit(width, height, mx, my)
                    .is_some();

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

            // Görev Yöneticisi penceresi her zaman 7. yuvada değildir;
            // uygulama kimliğine göre bulunmalıdır.
            let tm_win = focused_window_for_app(&compositor, 7).map(|(_, w)| w);

            if apps.taskmgr_app.launch_menu_open {
                let in_menu = tm_win.is_some_and(|w| {
                    gui::in_rect(
                        mx,
                        my,
                        apps::taskmgr::TaskManagerApp::launch_menu_rect(
                            w.x, w.y, w.width, w.height,
                        ),
                    )
                });

                let in_button = tm_win.is_some_and(|w| {
                    gui::in_rect(
                        mx,
                        my,
                        apps::taskmgr::TaskManagerApp::launch_button_rect(w.x, w.y, w.width),
                    )
                });

                if !in_menu && !in_button {
                    apps.taskmgr_app.launch_menu_open = false;
                }
            }

            if apps.taskmgr_app.actions_menu_open {
                let in_menu = tm_win.is_some_and(|w| {
                    gui::in_rect(
                        mx,
                        my,
                        apps::taskmgr::TaskManagerApp::actions_menu_rect(
                            w.x, w.y, w.width, w.height,
                        ),
                    )
                });
                let in_button = tm_win.is_some_and(|w| {
                    gui::in_rect(
                        mx,
                        my,
                        apps::taskmgr::TaskManagerApp::actions_button_rect(w.x, w.y, w.width),
                    )
                });
                if !in_menu && !in_button {
                    apps.taskmgr_app.actions_menu_open = false;
                }
            }

            if state.run_dialog_open {
                let rbox = (
                    width.saturating_sub(320) / 2,
                    height.saturating_sub(110) / 2,
                    320,
                    110,
                );

                if !gui::in_rect(mx, my, rbox) {
                    state.run_dialog_open = false;

                    state.run_dialog_input.clear();
                } else {
                    consumed = true;
                }
            }

            // özellikler (Properties) modali: modal acikken tum tiklamalari modal ustlenir.

            // X / Tamam butonlari ve modal disina tiklama pencereyi kapatir.

            if state.properties_open {
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
                    state.properties_open = false;

                    state.properties_name.clear();

                    state.properties_type.clear();

                    state.properties_size.clear();

                    state.properties_location.clear();

                    state.properties_path.clear();
                }

                consumed = true;

                state.force_render = true;
            }

            if !consumed && compositor.toast_click(width, tb_y, mx, my) {
                consumed = true;

                state.force_render = true;
            }

            // Kaydedilmemiş değişiklikler diyalogu (Not Defteri)
            if let Some(notepad) = apps.notepad_app.as_mut() {
                if notepad.unsaved_dialog_pending() {
                    if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                        let btn_rects = crate::apps::notepad::NotepadApp::unsaved_dialog_btn_rects(
                            w.width, w.height,
                        );
                        // Butonlar: [0]=Kaydet, [1]=Kaydetme, [2]=İptal
                        if gui::in_rect(mx - w.x, my - w.y, btn_rects[0]) {
                            // Kaydet
                            notepad.handle_unsaved_dialog_click(
                                crate::apps::notepad::UnsavedDialogAction::Save,
                            );
                            consumed = true;
                            state.force_render = true;
                        } else if gui::in_rect(mx - w.x, my - w.y, btn_rects[1]) {
                            // Kaydetme
                            notepad.handle_unsaved_dialog_click(
                                crate::apps::notepad::UnsavedDialogAction::DontSave,
                            );
                            consumed = true;
                            state.force_render = true;
                        } else if gui::in_rect(mx - w.x, my - w.y, btn_rects[2]) {
                            // İptal
                            notepad.handle_unsaved_dialog_click(
                                crate::apps::notepad::UnsavedDialogAction::Cancel,
                            );
                            consumed = true;
                            state.force_render = true;
                        }
                        // Diyalog açıkken diğer tıklamaları yut
                        if consumed {
                            // Diyalog alanının dışına tıklandıysa da İptal say
                            let dialog_w = crate::gui::sp(300).min(w.width.saturating_sub(20));
                            let dialog_h = crate::gui::sp(120);
                            let dialog_x = (w.width.saturating_sub(dialog_w)) / 2;
                            let dialog_y = (w.height.saturating_sub(dialog_h)) / 2;
                            let dialog_rect = (w.x + dialog_x, w.y + dialog_y, dialog_w, dialog_h);
                            if !gui::in_rect(mx, my, dialog_rect) {
                                notepad.handle_unsaved_dialog_click(
                                    crate::apps::notepad::UnsavedDialogAction::Cancel,
                                );
                            }
                        }
                    }
                }
            }

            // Kaydedilmemiş değişiklikler diyalog aksiyonunu işle
            if let Some((idx, _)) = focused_window_for_app(&compositor, 2) {
                match apps
                    .notepad_app
                    .as_mut()
                    .unwrap()
                    .take_unsaved_dialog_action()
                {
                    crate::apps::notepad::UnsavedDialogAction::Save => {
                        // Kaydet ve sonra kapat
                        if with_busy_progress(
                            &mut fb,
                            mx,
                            my,
                            gui::ui_text("Dosya kaydediliyor...", "Saving file..."),
                            || {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .save_file(&mut fs_mgr.vfs)
                            },
                        ) {
                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                            // Kaydedildiyse pencereyi kapat
                            terminate_app(idx, &mut compositor, &mut apps);
                        }
                    }
                    crate::apps::notepad::UnsavedDialogAction::DontSave => {
                        // Kaydetmeden kapat
                        terminate_app(idx, &mut compositor, &mut apps);
                    }
                    crate::apps::notepad::UnsavedDialogAction::Cancel => {
                        // İptal - hiçbir şey yapma, diyalog zaten kapandı
                    }
                    crate::apps::notepad::UnsavedDialogAction::None => {}
                }
            }

            // Not Defteri metin alani: imleci konumlandir, cift tikta kelime sec,

            // tek tikta secimi temizle ve surukleyerek secimi baslat.

            if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                if apps.notepad_app.as_mut().unwrap().menu != 0 {
                    let in_menu_box = gui::in_rect(
                        mx,
                        my,
                        (
                            w.x + crate::apps::notepad::NotepadApp::menu_box_x(
                                apps.notepad_app.as_mut().unwrap().menu,
                                w.width,
                            ),
                            w.y + crate::apps::notepad::NotepadApp::menu_box_y(
                                apps.notepad_app.as_mut().unwrap().menu,
                                w.height,
                            ),
                            crate::apps::notepad::NotepadApp::menu_box_w(),
                            crate::apps::notepad::NotepadApp::menu_box_h(
                                apps.notepad_app.as_mut().unwrap().menu,
                            ),
                        ),
                    );

                    let overflow = crate::apps::notepad::NotepadApp::overflow_button_rect(w.width);
                    let in_tabs = gui::in_rect(
                        mx,
                        my,
                        (
                            w.x + overflow.0,
                            w.y + 30 + overflow.1,
                            overflow.2,
                            overflow.3,
                        ),
                    );

                    if !in_menu_box && !in_tabs {
                        // Menu acikken disari tiklandi -> menuyu kapat, alttaki metni tiklama

                        apps.notepad_app.as_mut().unwrap().menu = 0;

                        state.notepad_dragging = false;

                        consumed = true;
                    }

                    // Menu kutusu veya sekmeler tiklanmissa pencere olay isleyicisi isleyecek
                } else if w.is_focused
                    && gui::in_rect(
                        mx,
                        my,
                        (
                            w.x + 6,
                            w.y + 58,
                            w.width.saturating_sub(12),
                            w.height.saturating_sub(86),
                        ),
                    )
                {
                    let at = apps.notepad_app.as_mut().unwrap().index_at_scrolled(
                        mx - w.x,
                        my - w.y,
                        w.width,
                        w.scroll.y,
                    );

                    let now_ms = monotonic_ms() as usize;
                    let click_count = apps.notepad_app.as_mut().unwrap().record_click(
                        now_ms,
                        (mx, my),
                        apps.settings_app.double_click_ms as usize,
                    );

                    if click_count == 3 {
                        apps.notepad_app.as_mut().unwrap().select_paragraph_at(at);
                    } else if click_count == 2 {
                        apps.notepad_app.as_mut().unwrap().select_word_at(at);
                    } else {
                        apps.notepad_app.as_mut().unwrap().cursor = at;

                        if keyboard.shift {
                            let anchor = apps
                                .notepad_app
                                .as_mut()
                                .unwrap()
                                .selection_anchor
                                .unwrap_or(at);

                            if anchor == at {
                                apps.notepad_app.as_mut().unwrap().selection_start = None;

                                apps.notepad_app.as_mut().unwrap().selection_end = None;
                            } else {
                                apps.notepad_app.as_mut().unwrap().selection_start =
                                    Some(anchor.min(at));

                                apps.notepad_app.as_mut().unwrap().selection_end =
                                    Some(anchor.max(at));
                            }
                        } else {
                            apps.notepad_app.as_mut().unwrap().selection_anchor = Some(at);

                            apps.notepad_app.as_mut().unwrap().selection_start = None;

                            apps.notepad_app.as_mut().unwrap().selection_end = None;
                        }

                        state.notepad_dragging = true;
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
                            let (y, m) =
                                step_month(compositor.cal_year, compositor.cal_month, false);

                            compositor.cal_year = y;

                            compositor.cal_month = m;
                        } else if mx > cx + cw - 34 {
                            let (y, m) =
                                step_month(compositor.cal_year, compositor.cal_month, true);

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
                        apps.settings_app.is_muted = !apps.settings_app.is_muted;
                    } else {
                        let track = compositor.volume_track_rect(&l);

                        if gui::in_rect(mx, my, track) {
                            apps.settings_app
                                .set_volume_from_range(mx, track.0, track.2);
                        }
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
                                0 => {
                                    // Metin Belgesi (.txt)

                                    apps.desktop_files.new_file(&mut fs_mgr.vfs);

                                    apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                }

                                1 => {
                                    // Klasor

                                    apps.desktop_files.new_folder(&mut fs_mgr.vfs);

                                    apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                }

                                2 => {
                                    // cizim / Resim (.bmp)

                                    apps.desktop_files.new_custom_file(
                                        "cizim_",
                                        ".bmp",
                                        &[],
                                        &mut fs_mgr.vfs,
                                    );

                                    apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                }

                                _ => {
                                    // Bos Belge

                                    apps.desktop_files.new_custom_file(
                                        "belge_",
                                        ".dat",
                                        &[],
                                        &mut fs_mgr.vfs,
                                    );

                                    apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                }
                            }
                        }

                        gui::ContextKind::FileManager => {
                            match sub_item {
                                0 => {
                                    // Metin Belgesi (.txt)

                                    apps.filemanager_app.new_file(&mut fs_mgr.vfs);
                                }

                                1 => {
                                    // Klasor

                                    apps.filemanager_app.new_folder(&mut fs_mgr.vfs);
                                }

                                2 => {
                                    // cizim / Resim (.bmp)

                                    apps.filemanager_app.new_custom_file(
                                        "cizim_",
                                        ".bmp",
                                        &[],
                                        &mut fs_mgr.vfs,
                                    );
                                }

                                _ => {
                                    // Bos Belge

                                    apps.filemanager_app.new_custom_file(
                                        "belge_",
                                        ".dat",
                                        &[],
                                        &mut fs_mgr.vfs,
                                    );
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
                        gui::ContextKind::Desktop => clicked_item == 0,

                        gui::ContextKind::DesktopItem => clicked_item == 5,

                        gui::ContextKind::FileManager => clicked_item == 5,

                        _ => false,
                    };

                    if is_sub_parent {
                        compositor.is_context_submenu_open = !compositor.is_context_submenu_open;

                        consumed = true;
                    } else {
                        match compositor.context_kind {
                            gui::ContextKind::Desktop => match clicked_item {
                                1 => {
                                    apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                }

                                2 => open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 7),

                                _ => {}
                            },

                            gui::ContextKind::DesktopItem => {
                                match clicked_item {
                                    0 => {
                                        if let Some(i) = compositor.context_file_idx {
                                            let mut path = alloc::string::String::from("/desktop/");
                                            path.push_str(
                                                apps.desktop_files.files[i].display_name(),
                                            );

                                            if apps.desktop_files.files[i].is_dir {
                                                apps.filemanager_app.set_path(&path);
                                                apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                                open_app(
                                                    &mut compositor,
                                                    &mut apps,
                                                    &fs_mgr.vfs,
                                                    0,
                                                );
                                            } else {
                                                let tag = apps.desktop_files.files[i].icon_tag;

                                                if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                    if let Some(data) = fs_mgr.vfs.read_file(&path)
                                                    {
                                                        apps.load(2, &fs_mgr.vfs);
                                                        apps.notepad_app
                                                            .as_mut()
                                                            .unwrap()
                                                            .open_file(&path, data);
                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            2,
                                                        );
                                                    }
                                                } else if tag == "IMG" {
                                                    open_image_file(
                                                        &path,
                                                        &fs_mgr.vfs,
                                                        &apps.paint_apps,
                                                        &mut apps.imageviewer_app,
                                                    );
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        4,
                                                    );
                                                } else if tag == "SND" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        5,
                                                    );
                                                } else if tag == "VID" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        6,
                                                    );
                                                } else if tag == "WEB" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        3,
                                                    );
                                                } else if tag == "3D" {
                                                    if let Some(data) = fs_mgr.vfs.read_file(&path)
                                                    {
                                                        apps.load(12, &fs_mgr.vfs);
                                                        apps.viewer3d_app
                                                            .as_mut()
                                                            .unwrap()
                                                            .load_file(&path, data);
                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            12,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    1 => {
                                        if let Some(i) = compositor.context_file_idx {
                                            let mut path = alloc::string::String::from("/desktop/");
                                            path.push_str(
                                                apps.desktop_files.files[i].display_name(),
                                            );
                                            if let Some(data) = fs_mgr.vfs.read_file(&path) {
                                                apps.load(2, &fs_mgr.vfs);
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .open_file(&path, data);
                                                open_app(
                                                    &mut compositor,
                                                    &mut apps,
                                                    &fs_mgr.vfs,
                                                    2,
                                                );
                                            }
                                        }
                                    }

                                    2 => {
                                        apps.desktop_files.copy_selection(&mut file_clipboard, true)
                                    }

                                    3 => apps
                                        .desktop_files
                                        .copy_selection(&mut file_clipboard, false),

                                    4 => {
                                        apps.desktop_files
                                            .paste_selection(&mut file_clipboard, &mut fs_mgr.vfs);
                                    }

                                    5 => {} // Yeni ▶ submenu is handled above.

                                    6 => apps.desktop_files.begin_rename(),

                                    7 => {
                                        apps.desktop_files.delete_selected();
                                        apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                                    }

                                    8 => {
                                        if let Some(file_idx) = compositor.context_file_idx {
                                            if file_idx < apps.desktop_files.file_count {
                                                apps.desktop_files.selected_idx = file_idx;
                                                let file = &apps.desktop_files.files[file_idx];
                                                state.properties_name.clear();
                                                state.properties_name.push_str(file.display_name());
                                                state.properties_type.clear();
                                                state.properties_type.push_str(file.file_type);
                                                state.properties_size.clear();
                                                state
                                                    .properties_size
                                                    .push_str(file.size_str.as_str());
                                                state.properties_location.clear();
                                                state
                                                    .properties_location
                                                    .push_str(apps.desktop_files.path());
                                                state.properties_path.clear();
                                                let full_path =
                                                    apps.desktop_files.selected_full_path();
                                                state.properties_path.push_str(&full_path);
                                                state.properties_open = true;
                                            }
                                        }
                                    }

                                    _ => {}
                                }
                            }

                            gui::ContextKind::FileManager => {
                                match clicked_item {
                                    0 => {
                                        // Ac / Klasore Gir

                                        if let Some(file_idx) = compositor.context_file_idx {
                                            apps.filemanager_app.selected_idx = file_idx;

                                            if let Some((tag, path)) =
                                                apps.filemanager_app.enter_selected(&fs_mgr.vfs)
                                            {
                                                if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                                    let data = with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "Dosya açılıyor...",
                                                            "Opening file...",
                                                        ),
                                                        || fs_mgr.vfs.read_file(&path),
                                                    );

                                                    if let Some(data) = data {
                                                        apps.load(2, &fs_mgr.vfs);
                                                        apps.notepad_app
                                                            .as_mut()
                                                            .unwrap()
                                                            .open_file(&path, data);
                                                    }

                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        2,
                                                    );
                                                } else if tag == "IMG" {
                                                    with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "Resim açılıyor...",
                                                            "Opening image...",
                                                        ),
                                                        || {
                                                            open_image_file(
                                                                &path,
                                                                &fs_mgr.vfs,
                                                                &apps.paint_apps,
                                                                &mut apps.imageviewer_app,
                                                            )
                                                        },
                                                    );

                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        4,
                                                    );
                                                } else if tag == "SND" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        5,
                                                    );
                                                } else if tag == "VID" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        6,
                                                    );
                                                } else if tag == "WEB" {
                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        3,
                                                    );
                                                } else if tag == "3D" {
                                                    let data = with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "3D model açılıyor...",
                                                            "Opening 3D model...",
                                                        ),
                                                        || fs_mgr.vfs.read_file(&path),
                                                    );

                                                    if let Some(data) = data {
                                                        apps.load(12, &fs_mgr.vfs);
                                                        apps.viewer3d_app
                                                            .as_mut()
                                                            .unwrap()
                                                            .load_file(&path, data);
                                                    }

                                                    open_app(
                                                        &mut compositor,
                                                        &mut apps,
                                                        &fs_mgr.vfs,
                                                        12,
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    1 => {
                                        // Not Defterinde Duzenle

                                        if let Some(file_idx) = compositor.context_file_idx {
                                            apps.filemanager_app.selected_idx = file_idx;

                                            let path = apps.filemanager_app.selected_full_path();

                                            let data = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "Dosya açılıyor...",
                                                    "Opening file...",
                                                ),
                                                || fs_mgr.vfs.read_file(&path),
                                            );

                                            if let Some(data) = data {
                                                apps.load(2, &fs_mgr.vfs);
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .open_file(&path, data);
                                            }

                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 2);
                                        }
                                    }

                                    2 => {
                                        apps.filemanager_app
                                            .copy_selection(&mut file_clipboard, true);
                                    }

                                    3 => {
                                        apps.filemanager_app
                                            .copy_selection(&mut file_clipboard, false);
                                    }

                                    4 => {
                                        apps.filemanager_app
                                            .paste_selection(&mut file_clipboard, &mut fs_mgr.vfs);
                                    }

                                    5 => {} // Yeni ▶

                                    6 => {
                                        // Adini degistir

                                        if let Some(file_idx) = compositor.context_file_idx {
                                            apps.filemanager_app.selected_idx = file_idx;

                                            apps.filemanager_app.begin_rename();
                                        }
                                    }

                                    7 => {
                                        // Dosyayi / Klasoru Sil

                                        if let Some(file_idx) = compositor.context_file_idx {
                                            apps.filemanager_app.selected_idx = file_idx;

                                            apps.filemanager_app.delete_selected();
                                        }
                                    }

                                    8 => {
                                        // özellikler

                                        if let Some(file_idx) = compositor.context_file_idx {
                                            apps.filemanager_app.selected_idx = file_idx;

                                            if file_idx < apps.filemanager_app.file_count {
                                                let file = &apps.filemanager_app.files[file_idx];

                                                state.properties_name.clear();

                                                state.properties_name.push_str(file.display_name());

                                                state.properties_type.clear();

                                                state.properties_type.push_str(file.file_type);

                                                state.properties_size.clear();

                                                state
                                                    .properties_size
                                                    .push_str(file.size_str.as_str());

                                                state.properties_location.clear();

                                                state
                                                    .properties_location
                                                    .push_str(apps.filemanager_app.path());

                                                state.properties_path.clear();

                                                let full_path =
                                                    apps.filemanager_app.selected_full_path();

                                                state.properties_path.push_str(&full_path);

                                                state.properties_open = true;
                                            }
                                        }
                                    }

                                    _ => {}
                                }
                            }

                            gui::ContextKind::Notepad => match clicked_item {
                                0 => apps.notepad_app.as_mut().unwrap().cut_selection(),

                                1 => apps.notepad_app.as_mut().unwrap().copy_selection(),

                                2 => apps.notepad_app.as_mut().unwrap().paste_clipboard(),

                                3 => apps.notepad_app.as_mut().unwrap().delete_selection(),

                                4 => apps.notepad_app.as_mut().unwrap().select_all(),

                                _ => apps.notepad_app.as_mut().unwrap().clear(),
                            },

                            gui::ContextKind::TaskManager => {
                                if clicked_item == 0 {
                                    // Sonlandir (Del): secili sureci kapat

                                    if let Some(target) = apps.taskmgr_app.selected_app_idx() {
                                        apps.taskmgr_app.status = gui::ui_text(
                                            "Uygulama sonlandırıldı.",
                                            "Application terminated.",
                                        );

                                        if let Some(_wi) = compositor.context_window {
                                            terminate_app(target, &mut compositor, &mut apps);
                                        }
                                    }
                                }
                            }

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
                                                    .saturating_sub(gui::scaled_taskbar_h())
                                                    .saturating_sub(w.height)
                                                    / 2;
                                            }
                                        }

                                        _ => {
                                            if let Some(w) = compositor.windows[wi].as_mut() {
                                                w.is_open = false;
                                            }

                                            watchdog::unwatch(wi);
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

                let mut sub_rect =
                    gui::MinGuiCompositor::start_menu_submenu_rect(menu_y, active_g, tb_y, width);

                if compositor.start_search_len > 0 {
                    let mut matches = [0; 32];

                    let count = compositor.start_search_matches(
                        apps.settings_app.language.is_english(),
                        &mut matches,
                    );

                    sub_rect = gui::MinGuiCompositor::start_search_results_rect(
                        menu_y,
                        active_g,
                        tb_y,
                        count.max(1),
                        width,
                    );
                }

                if compositor.start_search_len > 0 && gui::in_rect(mx, my, sub_rect) {
                    let mut matches = [0; 32];

                    let count = compositor.start_search_matches(
                        apps.settings_app.language.is_english(),
                        &mut matches,
                    );

                    for (row, app_id) in matches[..count].iter().enumerate() {
                        let row_rect = (
                            sub_rect.0 + gui::sp(6),
                            sub_rect.1 + gui::sp(30) + row * gui::sp(28),
                            sub_rect.2.saturating_sub(gui::sp(12)),
                            gui::sp(25),
                        );

                        if gui::in_rect(mx, my, row_rect) {
                            if *app_id >= 100 {
                                let app_name = alloc::string::String::from(
                                    compositor.installed_app_name(*app_id - 100),
                                );
                                if !app_name.is_empty() {
                                    desktop::helpers::launch_installed_pkg(
                                        &app_name,
                                        &mut compositor,
                                        &mut apps,
                                        &mut fs_mgr.vfs,
                                        &mut memory_mgr.pmm,
                                        &now,
                                    );
                                }
                            } else {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, *app_id);
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
                        mx,
                        my,
                        search_rect.0 + 6,
                        monotonic_ms(),
                        apps.settings_app.double_click_ms as u64,
                    );
                    state.start_search_dragging = true;

                    consumed = true;
                }

                let mut cat_clicked = consumed;

                if gui::in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22)) {
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 1);

                    apps.settings_app.active_tab = 0;

                    compositor.close_start_menu();

                    consumed = true;

                    cat_clicked = true;
                }

                for g in 0..5 {
                    if cat_clicked {
                        break;
                    }

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
                        let i = if active_g < 3 {
                            gui::MENU_GROUPS[active_g].1 + k
                        } else {
                            0
                        };

                        let rect = gui::MinGuiCompositor::start_menu_subitem_rect(
                            menu_y, active_g, k, tb_y, width,
                        );

                        if gui::in_rect(mx, my, rect) {
                            item_clicked = true;

                            if active_g == 4 {
                                if k == 0 {
                                    if compositor.confirm_shutdown {
                                        #[cfg(minos_boot_log)]
                                        serial_print(
                                            "[minOs] Başlat menüsünden sistem kapatma onaylandı.\n",
                                        );

                                        finish_system(&mut fb, &fs_mgr, width, height, false);
                                    } else {
                                        compositor.confirm_shutdown = true;

                                        compositor.confirm_restart = false;
                                    }

                                    break;
                                }

                                if k == 1 {
                                    if compositor.confirm_restart {
                                        finish_system(&mut fb, &fs_mgr, width, height, true);
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
                                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 13);
                                } else {
                                    let app_name = alloc::string::String::from(
                                        compositor.installed_app_name(k - 1),
                                    );
                                    if !app_name.is_empty() {
                                        desktop::helpers::launch_installed_pkg(
                                            &app_name,
                                            &mut compositor,
                                            &mut apps,
                                            &mut fs_mgr.vfs,
                                            &mut memory_mgr.pmm,
                                            &now,
                                        );
                                    }
                                }
                            } else if let Some(app_idx) =
                                gui::MinGuiCompositor::start_menu_app_idx(i)
                            {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);
                            }

                            compositor.close_start_menu();

                            break;
                        }
                    }

                    if !item_clicked {
                        if gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h))
                            || gui::in_rect(mx, my, sub_rect)
                        {
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

                        compositor.mark_start_menu_open(monotonic_ms());
                    } else {
                        compositor.close_start_menu();
                    }

                    consumed = true;
                } else if gui::in_rect(mx, my, l.clock) {
                    compositor.is_calendar_open = !compositor.is_calendar_open;

                    if compositor.is_calendar_open {
                        compositor.cal_year = now.year;

                        compositor.cal_month = now.month;

                        compositor.calendar_open_ms = monotonic_ms();
                    }

                    consumed = true;
                } else if gui::in_rect(mx, my, l.desktop) {
                    // "Masaüstünü Göster" şeridi: yalnızca etkin sanal
                    // masaüstündeki pencereleri küçült/geri getir; diğer
                    // masaüstlerinin pencere durumuna dokunma.
                    // "Show Desktop" strip: only toggle windows on the
                    // active workspace; leave other desktops untouched.
                    let all_minimized = compositor
                        .windows
                        .iter()
                        .flatten()
                        .filter(|w| w.workspace == compositor.active_workspace)
                        .all(|w| !w.is_open || w.is_minimized);
                    for w in compositor.windows.iter_mut().flatten() {
                        if w.is_open && w.workspace == compositor.active_workspace {
                            w.is_minimized = !all_minimized;
                            w.is_focused = false;
                        }
                    }
                    if all_minimized {
                        compositor.focus_top_window();
                    } else {
                        compositor.active_window_idx = None;
                    }
                    consumed = true;
                } else if l.volume.2 > 0 && gui::in_rect(mx, my, l.volume) {
                    compositor.is_volume_popup_open = !compositor.is_volume_popup_open;

                    if compositor.is_volume_popup_open {
                        compositor.volume_popup_open_ms = monotonic_ms();
                    }

                    consumed = true;
                } else if l.network.2 > 0 && gui::in_rect(mx, my, l.network) {
                    // Ethernet simgesi ag ayarlari sekmesini acar.

                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 1);

                    apps.settings_app.active_tab = 2;

                    apps.settings_app.status_msg =
                        gui::ui_text("Ağ ayarları açıldı.", "Network settings opened.");

                    consumed = true;
                } else if gui::in_rect(mx, my, l.kbd) {
                    keyboard.toggle_layout();

                    apps.settings_app.keyboard_layout = keyboard.layout;

                    consumed = true;
                } else if let Some(badge) = compositor.workspace_badge_rect(width, height) {
                    // "Masaüstü n/N" göstergesi: tıklayınca sonraki sanal
                    // masaüstüne geçilir (4'ten sonra 1'e sarar).
                    // Workspace badge: click advances to the next virtual
                    // desktop (wraps from 4 back to 1).
                    if gui::in_rect(mx, my, badge) {
                        compositor.switch_workspace(1);

                        state.force_render = true;

                        consumed = true;
                    }
                }
            }

            // F) Gorev cubugu uygulama sekmeleri (font olcegine duyarli)

            let tab_h = gui::MinGuiCompositor::taskbar_tab_rect_at(0, tb_y).3;

            if !consumed && my >= tb_y + 5 && my < tb_y + 5 + tab_h {
                let open_count = compositor
                    .windows
                    .iter()
                    .filter(|slot| {
                        slot.is_some_and(|win| {
                            win.is_open && win.workspace == compositor.active_workspace
                        })
                    })
                    .count();

                let (tab_start, tab_width, tab_step, shown_tabs, tab_slots) =
                    gui::MinGuiCompositor::taskbar_tabs_layout(width, height, open_count);

                let mut tab_slot = 0usize;

                for idx in 0..gui::MAX_WINDOWS {
                    if let Some(win) = compositor.windows[idx] {
                        if win.is_open && win.workspace == compositor.active_workspace {
                            if tab_slot >= shown_tabs {
                                break;
                            }

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

                    let badge_w = tab_width.min(
                        gui::MinGuiCompositor::tray_layout(width, height)
                            .kbd
                            .0
                            .saturating_sub(gui::sp(4))
                            .saturating_sub(badge_x),
                    );

                    if mx >= badge_x && mx < badge_x + badge_w {
                        // +N alani gizli sekmeler arasinda sirayla dolasir.

                        let mut ordinal = 0usize;

                        let mut first_hidden = None;

                        let mut focused_hidden_ordinal = None;

                        for (hidden_idx, slot) in compositor.windows.iter().enumerate() {
                            if let Some(hidden) = slot {
                                if hidden.is_open && hidden.workspace == compositor.active_workspace
                                {
                                    if ordinal >= shown_tabs {
                                        if first_hidden.is_none() {
                                            first_hidden = Some(hidden_idx);
                                        }

                                        if hidden.is_focused {
                                            focused_hidden_ordinal = Some(ordinal);
                                        }
                                    }

                                    ordinal += 1;
                                }
                            }
                        }

                        let mut next_hidden = None;

                        if let Some(active_ordinal) = focused_hidden_ordinal {
                            let mut ordinal = 0usize;

                            for (hidden_idx, slot) in compositor.windows.iter().enumerate() {
                                if let Some(hidden) = slot {
                                    if hidden.is_open
                                        && hidden.workspace == compositor.active_workspace
                                    {
                                        if ordinal > active_ordinal {
                                            next_hidden = Some(hidden_idx);
                                            break;
                                        }

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

                let saved_click_idx = apps.desktop_files.last_click_idx;

                let saved_click_ms = apps.desktop_files.last_click_ms;

                apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                apps.desktop_files.last_click_idx = saved_click_idx;

                apps.desktop_files.last_click_ms = saved_click_ms;

                if let Some((i, _)) = desktop_item_at(mx, my, &apps.desktop_files, width, height) {
                    apps.desktop_files
                        .select_row(i, keyboard.ctrl, keyboard.shift);

                    compositor.active_window_idx = None;

                    let now_ms = monotonic_ms();

                    // Masaustunun kendi tiklama durumu kullanilir (dosya yoneticisi

                    // penceresinin durumu degil); aksi halde cift tik hic tutmaz.

                    let is_double = apps.desktop_files.last_click_idx == Some(i)
                        && now_ms.wrapping_sub(apps.desktop_files.last_click_ms)
                            <= apps.settings_app.double_click_ms as u64;

                    if is_double {
                        apps.desktop_files.last_click_idx = None;

                        apps.desktop_files.drag_item = None;

                        let mut p = alloc::string::String::from("/desktop/");

                        p.push_str(apps.desktop_files.files[i].display_name());

                        if apps.desktop_files.files[i].is_dir {
                            apps.filemanager_app.set_path(&p);
                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 0);
                        } else {
                            let tag = apps.desktop_files.files[i].icon_tag;

                            if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                let data = with_busy_progress(
                                    &mut fb,
                                    mx,
                                    my,
                                    gui::ui_text("Dosya açılıyor...", "Opening file..."),
                                    || fs_mgr.vfs.read_file(&p),
                                );

                                if let Some(data) = data {
                                    // Uygulama tembel yuklenir; open_app'ten once
                                    // bellekte olmasi gerekir.
                                    apps.load(2, &fs_mgr.vfs);
                                    apps.notepad_app.as_mut().unwrap().open_file(&p, data);
                                }

                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 2);
                            } else if tag == "IMG" {
                                with_busy_progress(
                                    &mut fb,
                                    mx,
                                    my,
                                    gui::ui_text("Resim açılıyor...", "Opening image..."),
                                    || {
                                        open_image_file(
                                            &p,
                                            &fs_mgr.vfs,
                                            &apps.paint_apps,
                                            &mut apps.imageviewer_app,
                                        )
                                    },
                                );

                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 4);
                            } else if tag == "SND" {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 5);
                            } else if tag == "VID" {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 6);
                            } else if tag == "WEB" {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 3);
                            } else if tag == "3D" {
                                let data = with_busy_progress(
                                    &mut fb,
                                    mx,
                                    my,
                                    gui::ui_text("3D model açılıyor...", "Opening 3D model..."),
                                    || fs_mgr.vfs.read_file(&p),
                                );

                                if let Some(data) = data {
                                    apps.load(12, &fs_mgr.vfs);
                                    apps.viewer3d_app.as_mut().unwrap().load_file(&p, data);
                                }

                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 12);
                            }
                        }
                    } else {
                        apps.desktop_files.last_click_idx = Some(i);

                        apps.desktop_files.last_click_ms = now_ms;

                        apps.desktop_files.drag_item = Some(i);

                        apps.desktop_files.drag_start = (mx, my);

                        desktop_drag_source = true;
                    }

                    consumed = true;
                }
            }

            // Masaustu sol serit hizli simgeleri (cizimle ayni kanonik geometri).

            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {
                let desk_apps = [0, 3, 1, 11];

                // Kurulu sistemde kaldirilan kurulum kisayolunu (app 11) atla.

                let desk_icon_count = gui::MinGuiCompositor::desktop_quick_count();

                for (i, &app_idx) in desk_apps.iter().enumerate().take(desk_icon_count) {
                    if gui::in_rect(mx, my, gui::MinGuiCompositor::desktop_quick_rect(i)) {
                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);

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

                    if gui::in_rect(mx, my, (wx, wy, ww, wh)) {
                        compositor.focus_window(idx);
                    }

                    // Window controls activate on release so their pressed state is visible.
                    // Hit alanları düğme basılıyken görsel olarak küçülse dahi tam boyutta kalır.
                    if let Some(control) = event_window::window_control_at(mx, my, &win) {
                        state.pending_window_control = Some((idx, control));
                        compositor.focus_window(idx);
                        break;
                    }

                    // Kaydirma cubuklari (sag kenar dikey + alt kenar yatay).
                    // Cizimle ayni `ScrollbarLayout` kullanildigi icin tutamac
                    // nerede gorunuyorsa oradan yakalanir.

                    {
                        let (ccx, ccy, ccw, cch) = win.content_rect();

                        let app_id = win.app_id as usize;

                        let (c_w, c_h) = window_content_size(
                            app_id,
                            ww,
                            wh,
                            win.content_w,
                            win.content_h,
                            &apps.filemanager_app,
                            &apps.notepad_app,
                            &apps.browser_app,
                            apps.settings_app.active_tab,
                        );

                        let lay =
                            gui::scrollbar::layout((ccx, ccy, ccw, cch), c_w, c_h, win.scroll);

                        let mut handled = false;

                        if let Some(g) = lay.vertical {
                            let track = g.track;
                            let hit = (track.0.saturating_sub(4), track.1, track.2 + 4, track.3);

                            if gui::in_rect(mx, my, hit) {
                                if gui::in_rect(mx, my, g.thumb) {
                                    // Tutamac uzerinde: tiklama noktasini koru.
                                    let grab = my.saturating_sub(g.thumb.1);
                                    state.vscroll_drag =
                                        Some((idx, grab, g.thumb.3, track.3, c_h, lay.view_h));
                                } else {
                                    // Iz uzerinde: tutamac merkezini tiklamaya getir.
                                    let thumb_h = g.thumb.3.max(16);
                                    let click_y = my.saturating_sub(track.1);
                                    let new_top = click_y.saturating_sub(thumb_h / 2);
                                    let max_thumb_travel = track.3.saturating_sub(thumb_h);
                                    let ratio = if max_thumb_travel == 0 {
                                        0.0
                                    } else {
                                        (new_top as f32 / max_thumb_travel as f32).clamp(0.0, 1.0)
                                    };
                                    let max_off = gui::scrollbar::max_offset(c_h, lay.view_h);
                                    let new_scroll_y =
                                        ((ratio * max_off as f32) as usize).min(max_off);

                                    if let Some(wm) = compositor.windows[idx].as_mut() {
                                        wm.scroll_anim.cancel();
                                        wm.scroll.y = new_scroll_y;
                                        let (dx, dy, dw, dh) = wm.content_rect();
                                        compositor.dirty_tracker.mark_window(
                                            idx,
                                            crate::gui::dirty_rect::DirtyRect::new(dx, dy, dw, dh),
                                        );
                                    }
                                    if app_id == 3 {
                                        apps.browser_app.sync_external_scroll(new_scroll_y);
                                    }

                                    let grab = (thumb_h / 2).min(thumb_h);
                                    state.vscroll_drag =
                                        Some((idx, grab, thumb_h, track.3, c_h, lay.view_h));
                                }

                                compositor.focus_window(idx);
                                handled = true;
                            }
                        }

                        if !handled {
                            if let Some(g) = lay.horizontal {
                                let track = g.track;
                                let hit =
                                    (track.0, track.1.saturating_sub(4), track.2, track.3 + 4);

                                if gui::in_rect(mx, my, hit) {
                                    if gui::in_rect(mx, my, g.thumb) {
                                        let grab = mx.saturating_sub(g.thumb.0);
                                        state.hscroll_drag =
                                            Some((idx, grab, g.thumb.2, track.2, c_w, lay.view_w));
                                    } else {
                                        let thumb_w = g.thumb.2.max(16);
                                        let click_x = mx.saturating_sub(track.0);
                                        let new_left = click_x.saturating_sub(thumb_w / 2);
                                        let max_thumb_travel = track.2.saturating_sub(thumb_w);
                                        let ratio = if max_thumb_travel == 0 {
                                            0.0
                                        } else {
                                            (new_left as f32 / max_thumb_travel as f32)
                                                .clamp(0.0, 1.0)
                                        };
                                        let max_off = gui::scrollbar::max_offset(c_w, lay.view_w);
                                        let new_scroll_x =
                                            ((ratio * max_off as f32) as usize).min(max_off);

                                        if let Some(wm) = compositor.windows[idx].as_mut() {
                                            wm.scroll_anim.cancel();
                                            wm.scroll.x = new_scroll_x;
                                            let (dx, dy, dw, dh) = wm.content_rect();
                                            compositor.dirty_tracker.mark_window(
                                                idx,
                                                crate::gui::dirty_rect::DirtyRect::new(
                                                    dx, dy, dw, dh,
                                                ),
                                            );
                                        }

                                        let grab = (thumb_w / 2).min(thumb_w);
                                        state.hscroll_drag =
                                            Some((idx, grab, thumb_w, track.2, c_w, lay.view_w));
                                    }

                                    compositor.focus_window(idx);
                                    handled = true;
                                }
                            }
                        }

                        if handled {
                            break;
                        }
                    }

                    // Pencere kenar ve koselerinden boyutlandirma. Scrollbar

                    // alani yukarida tuketildi; geri kalan kenar alani resize'a ait.

                    if !win.is_maximized {
                        if let Some(edge) = win.resize_edge_at(mx, my) {
                            compositor.focus_window(idx);

                            state.resizing_win = Some(idx);

                            state.resize_edge = Some(edge);

                            state.resize_orig = (wx, wy, ww, wh);

                            break;
                        }
                    }

                    // Baslik cubugu — surukle / cift tik ile maksimize

                    if gui::in_rect(mx, my, win.title_drag_rect()) {
                        let now_ms = monotonic_ms();

                        let dbl = state.last_title_idx == Some(idx)
                            && now_ms.wrapping_sub(state.last_title_click_ms)
                                <= apps.settings_app.double_click_ms as u64;

                        compositor.focus_window(idx);

                        if dbl {
                            compositor.toggle_maximize(idx, width, height);

                            state.dragging_win = None;

                            // cift tik tuketildikten sonra eslesmeyi temizle;

                            // ucuncu kisa tik yeni bir cift tik baslatabilsin.

                            state.last_title_idx = None;
                        } else {
                            state.last_title_idx = Some(idx);

                            state.last_title_click_ms = now_ms;

                            if !win.is_maximized {
                                state.dragging_win = Some(idx);

                                state.drag_offset_x = mx.saturating_sub(wx);

                                state.drag_offset_y = my.saturating_sub(wy);
                            }
                        }

                        break;
                    }

                    // Pencere govdesi icindeki uygulama dugmeleri

                    if mx >= wx && mx < wx + ww && my >= wy && my < wy + wh {
                        let app_id = compositor.app_id_for_window(idx);

                        // Tıklama işleyicileri uygulama kodu çalıştırır; olası
                        // bir bellek hatası bu uygulamaya atfedilir.
                        let _app_context = apps::guard::ContextScope::new(app_id, idx);

                        // Kendi kaydırmasını yapmayan uygulamalarda içerik,
                        // pencere kaydırma ofseti kadar ötelenerek çizilir.
                        // İsabet testinin çizimle aynı yere düşmesi için
                        // uygulamaya aynı ötelenmiş pencere geometrisi verilir.
                        let (wx, wy, ww, wh) = if app_scrolls_natively(app_id) {
                            (wx, wy, ww, wh)
                        } else {
                            let (c_w, c_h) = window_content_size(
                                app_id,
                                ww,
                                wh,
                                win.content_w,
                                win.content_h,
                                &apps.filemanager_app,
                                &apps.notepad_app,
                                &apps.browser_app,
                                apps.settings_app.active_tab,
                            );
                            (
                                wx.saturating_sub(win.scroll.x),
                                wy.saturating_sub(win.scroll.y),
                                c_w.saturating_add(2).max(ww),
                                c_h.saturating_add(gui::scaled_titlebar_h() + 1).max(wh),
                            )
                        };

                        match app_id {
                            0 => {
                                let menu_button =
                                    apps::filemanager::FileManagerApp::actions_button_rect(
                                        wx, wy, ww,
                                    );
                                let mut menu_action = None;
                                if gui::in_rect(mx, my, menu_button) {
                                    apps.filemanager_app.actions_menu_open =
                                        !apps.filemanager_app.actions_menu_open;
                                    apps.filemanager_app.focused_control = 0;
                                } else if apps.filemanager_app.actions_menu_open {
                                    menu_action = (0..7).find(|&i| gui::in_rect(mx, my,
                                        apps::filemanager::FileManagerApp::actions_menu_item_rect(wx, wy, ww, wh, i)));
                                    apps.filemanager_app.actions_menu_open = false;
                                }

                                if gui::in_rect(mx, my, menu_button) {
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::filemanager::FileManagerApp::search_rect(wx, wy, ww),
                                ) {
                                    apps.filemanager_app.focused_control = 6;
                                } else if menu_action == Some(0) {
                                    apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                } else if menu_action == Some(1) {
                                    apps.filemanager_app.new_file(&mut fs_mgr.vfs);
                                } else if menu_action == Some(2) {
                                    apps.filemanager_app.new_folder(&mut fs_mgr.vfs);
                                } else if menu_action == Some(3) {
                                    apps.filemanager_app.delete_selected();
                                } else if menu_action == Some(4) {
                                    // Geri Donusum Kutusu'ndayken "Ac" dugmesi ogeyi geri yukler.

                                    if apps.filemanager_app.path() == crate::fs::vfs::TRASH_DIR {
                                        apps.filemanager_app.restore_selected(&mut fs_mgr.vfs);
                                    } else if let Some((tag, path)) =
                                        apps.filemanager_app.enter_selected(&fs_mgr.vfs)
                                    {
                                        if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                            let data = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "Dosya açılıyor...",
                                                    "Opening file...",
                                                ),
                                                || fs_mgr.vfs.read_file(&path),
                                            );

                                            if let Some(data) = data {
                                                apps.load(2, &fs_mgr.vfs);
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .open_file(&path, data);
                                            }

                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 2);
                                        } else if tag == "IMG" {
                                            with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "Resim açılıyor...",
                                                    "Opening image...",
                                                ),
                                                || {
                                                    open_image_file(
                                                        &path,
                                                        &fs_mgr.vfs,
                                                        &apps.paint_apps,
                                                        &mut apps.imageviewer_app,
                                                    )
                                                },
                                            );

                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 4);
                                        } else if tag == "SND" {
                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 5);
                                        } else if tag == "VID" {
                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 6);
                                        } else if tag == "WEB" {
                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 3);
                                        } else if tag == "3D" {
                                            let data = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "3D model açılıyor...",
                                                    "Opening 3D model...",
                                                ),
                                                || fs_mgr.vfs.read_file(&path),
                                            );

                                            if let Some(data) = data {
                                                apps.load(12, &fs_mgr.vfs);
                                                apps.viewer3d_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .load_file(&path, data);
                                            }

                                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 12);
                                        }
                                    }
                                } else if menu_action == Some(5) {
                                    apps.filemanager_app.toggle_view_mode();

                                    apps.filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(
                                        &mut compositor,
                                        &apps.filemanager_app,
                                    );
                                } else if menu_action == Some(6) {
                                    apps.filemanager_app.toggle_preview();

                                    apps.filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    apps.filemanager_app.update_preview(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(
                                        &mut compositor,
                                        &apps.filemanager_app,
                                    );
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::filemanager::FileManagerApp::header_name_rect(wx, wy, ww),
                                ) || gui::in_rect(
                                    mx,
                                    my,
                                    apps::filemanager::FileManagerApp::header_size_rect(wx, wy, ww),
                                ) || gui::in_rect(
                                    mx,
                                    my,
                                    apps::filemanager::FileManagerApp::header_type_rect(wx, wy, ww),
                                ) {
                                    apps.filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(
                                        &mut compositor,
                                        &apps.filemanager_app,
                                    );
                                } else if mx >= wx + 6
                                    && mx < wx + 116
                                    && my >= wy + 66
                                    && my < wy + 66 + 10 * 22
                                {
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

                                    apps.filemanager_app.set_path(target_path);

                                    apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 {
                                    let sy =
                                        compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0);

                                    let list_top = wy + apps::filemanager::LIST_TOP;

                                    let list_bottom = wy + wh.saturating_sub(30);

                                    for i in 0..apps.filemanager_app.file_count {
                                        if my >= list_top
                                            && my < list_bottom
                                            && gui::in_rect(
                                                mx,
                                                my,
                                                apps.filemanager_app.item_rect(wx, wy, ww, i, sy),
                                            )
                                        {
                                            apps.filemanager_app.select_row(
                                                i,
                                                keyboard.ctrl,
                                                keyboard.shift,
                                            );

                                            let now_ms = monotonic_ms();

                                            let is_double_click =
                                                apps.filemanager_app.last_click_idx == Some(i)
                                                    && now_ms.wrapping_sub(
                                                        apps.filemanager_app.last_click_ms,
                                                    ) <= apps.settings_app.double_click_ms
                                                        as u64;

                                            if is_double_click {
                                                apps.filemanager_app.last_click_idx = None;

                                                apps.filemanager_app.drag_item = None;

                                                apps.filemanager_app.drag_active = false;

                                                apps.filemanager_app.drag_target = None;

                                                if let Some((tag, path)) =
                                                    apps.filemanager_app.enter_selected(&fs_mgr.vfs)
                                                {
                                                    if tag == "TXT" || tag == "LOG" || tag == "CFG"
                                                    {
                                                        let data = with_busy_progress(
                                                            &mut fb,
                                                            mx,
                                                            my,
                                                            gui::ui_text(
                                                                "Dosya açılıyor...",
                                                                "Opening file...",
                                                            ),
                                                            || fs_mgr.vfs.read_file(&path),
                                                        );

                                                        if let Some(data) = data {
                                                            apps.load(2, &fs_mgr.vfs);
                                                            apps.notepad_app
                                                                .as_mut()
                                                                .unwrap()
                                                                .open_file(&path, data);
                                                        }

                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            2,
                                                        );
                                                    } else if tag == "IMG" {
                                                        with_busy_progress(
                                                            &mut fb,
                                                            mx,
                                                            my,
                                                            gui::ui_text(
                                                                "Resim açılıyor...",
                                                                "Opening image...",
                                                            ),
                                                            || {
                                                                open_image_file(
                                                                    &path,
                                                                    &fs_mgr.vfs,
                                                                    &apps.paint_apps,
                                                                    &mut apps.imageviewer_app,
                                                                )
                                                            },
                                                        );

                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            4,
                                                        );
                                                    } else if tag == "SND" {
                                                        if let Some(data) = with_busy_progress(
                                                            &mut fb,
                                                            mx,
                                                            my,
                                                            gui::ui_text(
                                                                "Ses dosyası açılıyor...",
                                                                "Opening audio file...",
                                                            ),
                                                            || fs_mgr.vfs.read_file(&path),
                                                        ) {
                                                            // Çalma listesi yerine tam olarak çift tıklanan dosyayı etkinleştir.
                                                            apps.musicplayer_app
                                                                .load_wav(&path, data);
                                                        }
                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            5,
                                                        );
                                                    } else if tag == "VID" {
                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            6,
                                                        );
                                                    } else if tag == "WEB" {
                                                        let file_data = fs_mgr
                                                            .vfs
                                                            .read_file(&path)
                                                            .map(alloc::vec::Vec::from);
                                                        if let Some(data) = file_data {
                                                            let wrap = compositor
                                                                .windows
                                                                .iter()
                                                                .flatten()
                                                                .find(|w| w.app_id == 3u8)
                                                                .map_or(700, |w| {
                                                                    w.width.saturating_sub(40)
                                                                });
                                                            apps.browser_app
                                                                .set_url(path.as_bytes());
                                                            apps.browser_app.load_html(
                                                                &data,
                                                                wrap,
                                                                256,
                                                                Some(&mut fs_mgr.vfs),
                                                            );
                                                        }

                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            3,
                                                        );
                                                    } else if tag == "3D" {
                                                        let data = with_busy_progress(
                                                            &mut fb,
                                                            mx,
                                                            my,
                                                            gui::ui_text(
                                                                "3D model açılıyor...",
                                                                "Opening 3D model...",
                                                            ),
                                                            || fs_mgr.vfs.read_file(&path),
                                                        );

                                                        if let Some(data) = data {
                                                            apps.load(12, &fs_mgr.vfs);
                                                            apps.viewer3d_app
                                                                .as_mut()
                                                                .unwrap()
                                                                .load_file(&path, data);
                                                        }

                                                        open_app(
                                                            &mut compositor,
                                                            &mut apps,
                                                            &fs_mgr.vfs,
                                                            12,
                                                        );
                                                    }
                                                }
                                            } else {
                                                apps.filemanager_app.last_click_idx = Some(i);

                                                apps.filemanager_app.last_click_ms = now_ms;

                                                if apps.filemanager_app.files[i].display_name()
                                                    != ".."
                                                {
                                                    apps.filemanager_app.drag_item = Some(i);

                                                    apps.filemanager_app.drag_start = (mx, my);

                                                    desktop_drag_source = false;

                                                    apps.filemanager_app.drag_active = false;

                                                    apps.filemanager_app.drag_target = None;
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
                                    if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::tab_rect(wx, wy, t),
                                    ) {
                                        apps.settings_app.active_tab = t;
                                    }
                                }

                                if gui::in_rect(
                                    mx,
                                    my,
                                    apps::settings::SystemSettings::save_btn_rect(wx, wy, wh),
                                ) {
                                    let utc_valid = !apps.settings_app.utc_editing
                                        || apps.settings_app.commit_utc_edit().is_some();
                                    if utc_valid {
                                        now = drivers::rtc::DateTime::read_for_region(
                                            apps.settings_app.region_offset(),
                                        );
                                        compositor.cal_year = now.year;
                                        compositor.cal_month = now.month;
                                        with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text(
                                                "Ayarlar kaydediliyor...",
                                                "Saving settings...",
                                            ),
                                            || {
                                                apps.settings_app
                                                    .save_all(&compositor, &mut fs_mgr.vfs)
                                            },
                                        );
                                    }
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh),
                                ) {
                                    apps.settings_app.reset_defaults();

                                    apps.settings_app.apply_runtime(
                                        &mut *keyboard,
                                        &mut compositor,
                                        &mut now,
                                    );

                                    apps.settings_app.write_config(&mut fs_mgr.vfs);
                                } else if apps.settings_app.active_tab == 2
                                    && (0..4).any(|octet| {
                                        gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::static_ip_octet_rect(
                                                wx, wy, octet,
                                            ),
                                        )
                                    })
                                {
                                    for octet in 0..4 {
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::static_ip_octet_rect(
                                                wx, wy, octet,
                                            ),
                                        ) {
                                            apps.settings_app.static_ip_octet = octet;
                                            apps.settings_app.static_ip_value = 0;
                                            apps.settings_app.static_ip_digits = 0;
                                            apps.settings_app.static_ip_edit = true;
                                            apps.settings_app.dhcp_enabled = false;
                                            apps.settings_app.focused_control = 7 + octet;
                                            break;
                                        }
                                    }
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::dhcp_btn_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.static_ip_edit = false;

                                    apps.settings_app.dhcp_enabled =
                                        !apps.settings_app.dhcp_enabled;

                                    apps.settings_app.status_msg = if apps.settings_app.dhcp_enabled
                                    {
                                        gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.")
                                    } else {
                                        gui::ui_text("Statik IP seçildi.", "Static IP selected.")
                                    };
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::wifi_scan_btn_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.wifi_scanned = true;
                                    apps.settings_app.wifi_msg =
                                        gui::ui_text("Ağlar taranıyor...", "Scanning networks...");
                                    crate::drivers::wifi::scan_networks();
                                    apps.settings_app.wifi_msg = "";
                                } else if apps.settings_app.active_tab == 2
                                    && (0..5).any(|row| {
                                        gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::wifi_ap_row_rect(
                                                wx, wy, row,
                                            ),
                                        )
                                    })
                                {
                                    apps.settings_app.wifi_pass_edit = false;
                                    for row in 0..5 {
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::wifi_ap_row_rect(
                                                wx, wy, row,
                                            ),
                                        ) {
                                            apps.settings_app.wifi_select_ap(row);
                                            break;
                                        }
                                    }
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::wifi_pass_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.wifi_pass_edit = true;
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::wifi_connect_btn_rect(
                                            wx, wy,
                                        ),
                                    )
                                {
                                    apps.settings_app.wifi_pass_edit = false;
                                    apps.settings_app.wifi_connect();
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::wifi_disconnect_btn_rect(
                                            wx, wy,
                                        ),
                                    )
                                {
                                    apps.settings_app.wifi_pass_edit = false;
                                    apps.settings_app.wifi_disconnect();
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_net_btn_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.static_ip_edit = false;

                                    with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text(
                                            "Ağ testi yapılıyor...",
                                            "Running network test...",
                                        ),
                                        || apps.settings_app.run_network_test(),
                                    );
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_ping_btn_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.static_ip_edit = false;

                                    with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text("Ping gönderiliyor...", "Sending ping..."),
                                        || apps.settings_app.run_ping_test(),
                                    );
                                } else if apps.settings_app.active_tab == 2
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_http_btn_rect(wx, wy),
                                    )
                                {
                                    apps.settings_app.static_ip_edit = false;

                                    with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text(
                                            "HTTP bağlantısı test ediliyor...",
                                            "Testing HTTP connection...",
                                        ),
                                        || apps.settings_app.run_http_test(),
                                    );
                                } else if apps.settings_app.active_tab == 0 {
                                    let mut swatch_hit = false;

                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count()
                                    {
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::theme_swatch_rect(
                                                wx, wy, i,
                                            ),
                                        ) {
                                            apps.settings_app.theme = gui::theme::ThemeId::ALL[i];

                                            gui::theme::set_theme(apps.settings_app.theme);

                                            compositor.push_toast(
                                                gui::ToastKind::Info,
                                                gui::ui_text("Tema değiştirildi", "Theme changed"),
                                                apps.settings_app.theme.name(),
                                                monotonic_ms(),
                                                3000,
                                            );

                                            swatch_hit = true;

                                            break;
                                        }
                                    }

                                    if swatch_hit {

                                        // Tema paletinden dogrudan secim yapildi.
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::kbd_btn_rect(wx, wy, ww),
                                    ) {
                                        keyboard.toggle_layout();

                                        apps.settings_app.keyboard_layout = keyboard.layout;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::language_btn_rect(
                                            wx, wy, ww,
                                        ),
                                    ) {
                                        apps.settings_app.language.toggle();

                                        apps.settings_app.status_msg = gui::ui_text(
                                            "Arayüz dili Türkçe olarak ayarlandı.",
                                            "Interface language set to English.",
                                        );
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::utc_dec_btn_rect(wx, wy),
                                    ) {
                                        let offset = apps.settings_app.step_utc_offset(-1);
                                        now = drivers::rtc::DateTime::read_for_region(offset);
                                        compositor.cal_year = now.year;
                                        compositor.cal_month = now.month;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::utc_inc_btn_rect(wx, wy),
                                    ) {
                                        let offset = apps.settings_app.step_utc_offset(1);
                                        now = drivers::rtc::DateTime::read_for_region(offset);
                                        compositor.cal_year = now.year;
                                        compositor.cal_month = now.month;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::region_btn_rect(wx, wy),
                                    ) {
                                        apps.settings_app.begin_utc_edit();
                                        apps.settings_app.focused_control = 7;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::mute_btn_rect(wx, wy),
                                    ) {
                                        apps.settings_app.is_muted = !apps.settings_app.is_muted;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::volume_track_rect(wx, wy),
                                    ) {
                                        let track =
                                            apps::settings::SystemSettings::volume_track_rect(
                                                wx, wy,
                                            );
                                        apps.settings_app
                                            .set_volume_from_range(mx, track.0, track.2);
                                    }
                                }

                                if apps.settings_app.active_tab == 1 {
                                    let mut swatch_hit = false;

                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count()
                                    {
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::theme_swatch_rect(
                                                wx, wy, i,
                                            ),
                                        ) {
                                            apps.settings_app.theme = gui::theme::ThemeId::ALL[i];

                                            gui::theme::set_theme(apps.settings_app.theme);

                                            compositor.push_toast(
                                                gui::ToastKind::Info,
                                                gui::ui_text("Tema değiştirildi", "Theme changed"),
                                                apps.settings_app.theme.name(),
                                                monotonic_ms(),
                                                3000,
                                            );

                                            swatch_hit = true;

                                            break;
                                        }
                                    }

                                    if swatch_hit {

                                        // Tema paletinden dogrudan secim yapildi.
                                    } else {
                                        let mut handled = false;
                                        for i in 0..3 {
                                            if gui::in_rect(
                                                mx,
                                                my,
                                                apps::settings::SystemSettings::zoom_btn_rect(
                                                    wx, wy, i,
                                                ),
                                            ) {
                                                apps.settings_app.zoom = [100, 125, 150][i];
                                                gui::set_font_zoom(apps.settings_app.zoom);
                                                apps.settings_app.status_msg = gui::ui_text(
                                                    "Yazı yakınlaştırması değiştirildi.",
                                                    "Font zoom changed.",
                                                );
                                                handled = true;
                                                break;
                                            }
                                        }

                                        if !handled {
                                            for i in 0..7 {
                                                if gui::in_rect(mx,my,apps::settings::SystemSettings::resolution_btn_rect(wx,wy,i)) {
                                                let requested = match i { 0 => (800,600), 1 => (1024,768), 2 => (1280,720), 3 => (1920,1080), 4 => (1920,1200), 5 => (2560,1440), 6 => (2560,1600), _ => (1920,1080) };
                                                if requested == (fb.width, fb.height) {
                                                    apps.settings_app.screen_res = requested;
                                                    apps.settings_app.display_res = requested;
                                                    apps.settings_app.status_msg = gui::ui_text("Bu çözünürlük zaten etkin.", "This resolution is already active.");
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
                                                                                                                        apps.settings_app.screen_res = requested;
                                                            apps.settings_app.display_res = requested;
                                                            crate::gui::wallpaper::apply(apps.settings_app.wallpaper, apps.settings_app.wall_color);
                                                            keep_windows_on_screen(&mut compositor, width, height);
                                                            state.force_render = true;
                                                            apps.settings_app.status_msg = gui::ui_text("Çözünürlük değiştirildi (BGA).", "Resolution changed (BGA).");
                                                        } else {
                                                            apps.settings_app.status_msg = gui::ui_text("Çözünürlük uygulanamadı (bellek sınırı).", "Resolution failed (memory limit).");
                                                        }
                                                    } else {
                                                        apps.settings_app.screen_res = (fb.width, fb.height);
                                                        apps.settings_app.status_msg = gui::ui_text("Çözünürlük açılışta Limine tarafından belirlenir.", "Resolution is set by Limine at boot.");
                                                    }
                                                }
                                                handled = true;
                                                break;
                                            }
                                            }
                                        }

                                        if !handled
                                            && gui::in_rect(
                                                mx,
                                                my,
                                                apps::settings::SystemSettings::night_light_rect(
                                                    wx, wy,
                                                ),
                                            )
                                        {
                                            apps.settings_app.night_light =
                                                !apps.settings_app.night_light;

                                            apps.settings_app.status_msg =
                                                if apps.settings_app.night_light {
                                                    gui::ui_text(
                                                        "Mavi ışık filtresi açıldı.",
                                                        "Blue light filter enabled.",
                                                    )
                                                } else {
                                                    gui::ui_text(
                                                        "Mavi ışık filtresi kapatıldı.",
                                                        "Blue light filter disabled.",
                                                    )
                                                };
                                        }

                                        let nl_track =
                                            apps::settings::SystemSettings::night_light_track_rect(
                                                wx, wy,
                                            );

                                        if gui::in_rect(mx, my, nl_track) {
                                            let span = nl_track.2.saturating_sub(1).max(1);

                                            apps.settings_app.night_light_level =
                                                ((mx - nl_track.0).min(span) * 100 / span).min(100)
                                                    as u8;

                                            apps.settings_app.night_light = true;
                                        }

                                        for i in
                                            0..apps::settings::SystemSettings::wallpaper_count()
                                        {
                                            if gui::in_rect(
                                                mx,
                                                my,
                                                apps::settings::SystemSettings::wallpaper_btn_rect(
                                                    wx, wy, i,
                                                ),
                                            ) {
                                                apps.settings_app.wallpaper = i as u8;

                                                crate::gui::wallpaper::apply(
                                                    apps.settings_app.wallpaper,
                                                    apps.settings_app.wall_color,
                                                );

                                                apps.settings_app.status_msg = gui::ui_text(
                                                    "Arka plan değiştirildi.",
                                                    "Background changed.",
                                                );
                                            }
                                        }

                                        // Animasyon anahtarı (Ekran sekmesi, Satır 11).
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::anim_toggle_rect(
                                                wx, wy,
                                            ),
                                        ) {
                                            apps.settings_app.reduced_motion =
                                                !apps.settings_app.reduced_motion;
                                            gui::anim::set_reduced_motion(
                                                apps.settings_app.reduced_motion,
                                            );
                                            apps.settings_app.status_msg = gui::ui_text(
                                                "Animasyon tercihi kaydedildi.",
                                                "Animation preference saved.",
                                            );
                                        }

                                        // Saydam pencere anahtarı (Satır 12).
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            apps::settings::SystemSettings::transparent_windows_rect(
                                                wx, wy,
                                            ),
                                        ) {
                                            apps.settings_app.transparent_windows =
                                                !apps.settings_app.transparent_windows;
                                            gui::set_transparent_windows(
                                                apps.settings_app.transparent_windows,
                                            );
                                            apps.settings_app.status_msg = gui::ui_text(
                                                "Saydam pencere tercihi kaydedildi.",
                                                "Transparent window preference saved.",
                                            );
                                        }
                                    } // tema else kapatma
                                }

                                if apps.settings_app.active_tab == 0 {
                                    let speed_track =
                                        apps::settings::SystemSettings::mouse_speed_track_rect(
                                            wx, wy,
                                        );

                                    let mut mouse_setting_changed = true;

                                    if gui::in_rect(mx, my, speed_track) {
                                        let span = speed_track.2.saturating_sub(1).max(1);

                                        apps.settings_app.mouse_speed = (1
                                            + (mx.saturating_sub(speed_track.0).min(span) * 9
                                                / span))
                                            as u8;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::double_click_dec_rect(
                                            wx, wy,
                                        ),
                                    ) {
                                        apps.settings_app.double_click_ms = apps
                                            .settings_app
                                            .double_click_ms
                                            .saturating_sub(50)
                                            .max(150);
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::double_click_inc_rect(
                                            wx, wy,
                                        ),
                                    ) {
                                        apps.settings_app.double_click_ms = apps
                                            .settings_app
                                            .double_click_ms
                                            .saturating_add(50)
                                            .min(600);
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::reverse_scroll_rect(wx, wy),
                                    ) {
                                        apps.settings_app.reverse_scroll =
                                            !apps.settings_app.reverse_scroll;
                                    } else {
                                        mouse_setting_changed = false;
                                    }

                                    if mouse_setting_changed {
                                        apps.settings_app.status_msg = gui::ui_text("Fare ayarları değiştirildi. Kalıcı olması için Kaydet'e tıklayın.", "Mouse settings changed. Click Save to keep them.");
                                    }
                                }

                                if apps.settings_app.active_tab == 4 {
                                    if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_ram_btn_rect(wx, wy),
                                    ) {
                                        apps.settings_app.run_ram_test();
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_disk_btn_rect(wx, wy),
                                    ) {
                                        with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text(
                                                "Disk testi çalışıyor...",
                                                "Running disk test...",
                                            ),
                                            || apps.settings_app.run_disk_test(),
                                        );
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::settings::SystemSettings::test_audio_btn_rect(wx, wy),
                                    ) {
                                        apps.settings_app.run_audio_test(&mut audio_dev);
                                    }
                                }
                            }

                            2 => {
                                // Not Defteri: Dosya/Duzen/Yardim arac cubugu ve menu aksiyonlari

                                // Tiklama testleri, menu cizimiyle ayni geometri

                                // yardimcilarini (apps::notepad) kullanir; boylece font

                                // olcegi 100'den farkliyken de koordinatlar ortusur.

                                let overflow =
                                    crate::apps::notepad::NotepadApp::overflow_button_rect(ww);
                                let overflow_rect = (
                                    wx + overflow.0,
                                    wy + 30 + overflow.1,
                                    overflow.2,
                                    overflow.3,
                                );

                                if gui::in_rect(mx, my, overflow_rect) {
                                    apps.notepad_app.as_mut().unwrap().menu =
                                        if apps.notepad_app.as_mut().unwrap().menu == 4 {
                                            0
                                        } else {
                                            4
                                        };
                                } else if apps.notepad_app.as_mut().unwrap().menu != 0
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        (
                                            wx + crate::apps::notepad::NotepadApp::menu_box_x(
                                                apps.notepad_app.as_mut().unwrap().menu,
                                                ww,
                                            ),
                                            wy + crate::apps::notepad::NotepadApp::menu_box_y(
                                                apps.notepad_app.as_mut().unwrap().menu,
                                                wh,
                                            ),
                                            crate::apps::notepad::NotepadApp::menu_box_w(),
                                            crate::apps::notepad::NotepadApp::menu_box_h(
                                                apps.notepad_app.as_mut().unwrap().menu,
                                            ),
                                        ),
                                    )
                                {
                                    let rel_y = my.saturating_sub(
                                        wy + crate::apps::notepad::NotepadApp::menu_box_y(
                                            apps.notepad_app.as_mut().unwrap().menu,
                                            wh,
                                        ) + crate::apps::notepad::NotepadApp::menu_pad(),
                                    );

                                    let item_idx =
                                        rel_y / crate::apps::notepad::NotepadApp::menu_item_h();

                                    let items = crate::apps::notepad::NotepadApp::menu_items(
                                        apps.notepad_app.as_mut().unwrap().menu,
                                    );
                                    let menu_was_overflow =
                                        apps.notepad_app.as_mut().unwrap().menu == 4;

                                    if let Some(def) = items.get(item_idx) {
                                        match def.action {
                                            crate::apps::notepad::NotepadMenuAction::FileCategory => apps.notepad_app.as_mut().unwrap().menu = 1,
                                            crate::apps::notepad::NotepadMenuAction::EditCategory => apps.notepad_app.as_mut().unwrap().menu = 2,
                                            crate::apps::notepad::NotepadMenuAction::HelpCategory => apps.notepad_app.as_mut().unwrap().menu = 3,

                                            crate::apps::notepad::NotepadMenuAction::Open => {

                                                let path = alloc::string::String::from(apps.notepad_app.as_mut().unwrap().file_path_str());

                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                if let Some(data) = data {

                                                    apps.notepad_app.as_mut().unwrap().open_file(&path, data);

                                                }

                                            }

                                            crate::apps::notepad::NotepadMenuAction::Save => {

                                                if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || apps.notepad_app.as_mut().unwrap().save_file(&mut fs_mgr.vfs)) {

                                                    apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                }

                                            }

                                            crate::apps::notepad::NotepadMenuAction::About => {

                                                apps.notepad_app.as_mut().unwrap().execute_action(def.action, &mut fs_mgr.vfs);

                                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 1);

                                                apps.settings_app.active_tab = 3;

                                            }

                                            _ => {

                                                apps.notepad_app.as_mut().unwrap().execute_action(def.action, &mut fs_mgr.vfs);

                                            }

                                        }
                                    }

                                    if !(menu_was_overflow
                                        && (1..=3)
                                            .contains(&apps.notepad_app.as_mut().unwrap().menu))
                                    {
                                        apps.notepad_app.as_mut().unwrap().menu = 0;
                                    }
                                } else if apps.notepad_app.as_mut().unwrap().menu != 0 {
                                    // Menu acikken menu disina tiklandiginda menuyu kapat

                                    apps.notepad_app.as_mut().unwrap().menu = 0;
                                } else if mx >= wx + ww.saturating_sub(130)
                                    && mx <= wx + ww.saturating_sub(74)
                                    && my >= wy + 32
                                    && my <= wy + 52
                                {
                                    apps.notepad_app.as_mut().unwrap().clear();
                                } else if mx >= wx + ww.saturating_sub(68)
                                    && mx <= wx + ww.saturating_sub(12)
                                    && my >= wy + 32
                                    && my <= wy + 52
                                {
                                    if with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text("Dosya kaydediliyor...", "Saving file..."),
                                        || {
                                            apps.notepad_app
                                                .as_mut()
                                                .unwrap()
                                                .save_file(&mut fs_mgr.vfs)
                                        },
                                    ) {
                                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                } else if apps.notepad_app.as_mut().unwrap().find_active {
                                    let bar_r = apps::notepad::NotepadApp::find_bar_area_rect(
                                        ww,
                                        apps.notepad_app.as_mut().unwrap().replace_active,
                                    );
                                    if gui::in_rect(
                                        mx,
                                        my,
                                        (wx + bar_r.0, wy + bar_r.1, bar_r.2, bar_r.3),
                                    ) {
                                        // Bul/Değiştir çubuğu içinde — düğme ve giriş kutuları.
                                        let close_r =
                                            apps::notepad::NotepadApp::find_close_rect(ww);
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            (wx + close_r.0, wy + close_r.1, close_r.2, close_r.3),
                                        ) {
                                            apps.notepad_app.as_mut().unwrap().close_find();
                                        } else {
                                            let fin_r =
                                                apps::notepad::NotepadApp::find_input_rect();
                                            if gui::in_rect(
                                                mx,
                                                my,
                                                (wx + fin_r.0, wy + fin_r.1, fin_r.2, fin_r.3),
                                            ) {
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .find_focused_replace = false;
                                            } else {
                                                let nxt_r =
                                                    apps::notepad::NotepadApp::find_next_rect();
                                                if gui::in_rect(
                                                    mx,
                                                    my,
                                                    (wx + nxt_r.0, wy + nxt_r.1, nxt_r.2, nxt_r.3),
                                                ) {
                                                    apps.notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .find_next(true);
                                                    notepad_ensure_cursor_visible(
                                                        &mut compositor,
                                                        apps.notepad_app.as_ref().unwrap(),
                                                    );
                                                } else {
                                                    let prv_r =
                                                        apps::notepad::NotepadApp::find_prev_rect();
                                                    if gui::in_rect(
                                                        mx,
                                                        my,
                                                        (
                                                            wx + prv_r.0,
                                                            wy + prv_r.1,
                                                            prv_r.2,
                                                            prv_r.3,
                                                        ),
                                                    ) {
                                                        apps.notepad_app
                                                            .as_mut()
                                                            .unwrap()
                                                            .find_next(false);
                                                        notepad_ensure_cursor_visible(
                                                            &mut compositor,
                                                            apps.notepad_app.as_ref().unwrap(),
                                                        );
                                                    } else if apps
                                                        .notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .replace_active
                                                    {
                                                        let rin_r = apps::notepad::NotepadApp::replace_input_rect();
                                                        if gui::in_rect(
                                                            mx,
                                                            my,
                                                            (
                                                                wx + rin_r.0,
                                                                wy + rin_r.1,
                                                                rin_r.2,
                                                                rin_r.3,
                                                            ),
                                                        ) {
                                                            apps.notepad_app
                                                                .as_mut()
                                                                .unwrap()
                                                                .find_focused_replace = true;
                                                        } else {
                                                            let rpl_r = apps::notepad::NotepadApp::replace_btn_rect();
                                                            if gui::in_rect(
                                                                mx,
                                                                my,
                                                                (
                                                                    wx + rpl_r.0,
                                                                    wy + rpl_r.1,
                                                                    rpl_r.2,
                                                                    rpl_r.3,
                                                                ),
                                                            ) {
                                                                apps.notepad_app
                                                                    .as_mut()
                                                                    .unwrap()
                                                                    .replace_current();
                                                                notepad_ensure_cursor_visible(
                                                                    &mut compositor,
                                                                    apps.notepad_app
                                                                        .as_ref()
                                                                        .unwrap(),
                                                                );
                                                            } else {
                                                                let rall_r = apps::notepad::NotepadApp::replace_all_rect();
                                                                if gui::in_rect(
                                                                    mx,
                                                                    my,
                                                                    (
                                                                        wx + rall_r.0,
                                                                        wy + rall_r.1,
                                                                        rall_r.2,
                                                                        rall_r.3,
                                                                    ),
                                                                ) {
                                                                    apps.notepad_app
                                                                        .as_mut()
                                                                        .unwrap()
                                                                        .replace_all();
                                                                    notepad_ensure_cursor_visible(
                                                                        &mut compositor,
                                                                        apps.notepad_app
                                                                            .as_ref()
                                                                            .unwrap(),
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        // Çubuğun dışına tıklandı: bul penceresini kapat ve metin alanına geç.
                                        apps.notepad_app.as_mut().unwrap().close_find();
                                        if gui::in_rect(
                                            mx,
                                            my,
                                            (
                                                wx + 6,
                                                wy + apps.notepad_app.as_mut().unwrap().text_y()
                                                    - 4,
                                                ww.saturating_sub(12),
                                                wh.saturating_sub(
                                                    apps.notepad_app.as_mut().unwrap().text_y()
                                                        + 28,
                                                ),
                                            ),
                                        ) {
                                            let at = apps
                                                .notepad_app
                                                .as_mut()
                                                .unwrap()
                                                .index_at_scrolled(
                                                    mx - wx,
                                                    my - wy,
                                                    ww,
                                                    compositor.windows[idx]
                                                        .map(|w| w.scroll.y)
                                                        .unwrap_or(0),
                                                );
                                            apps.notepad_app.as_mut().unwrap().cursor = at;
                                            if keyboard.shift {
                                                let anchor = apps
                                                    .notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .selection_anchor
                                                    .unwrap_or(at);
                                                if anchor == at {
                                                    apps.notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .selection_start = None;
                                                    apps.notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .selection_end = None;
                                                } else {
                                                    apps.notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .selection_start = Some(anchor.min(at));
                                                    apps.notepad_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .selection_end = Some(anchor.max(at));
                                                }
                                            } else {
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .selection_anchor = Some(at);
                                                apps.notepad_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .selection_start = None;
                                                apps.notepad_app.as_mut().unwrap().selection_end =
                                                    None;
                                            }
                                            apps.notepad_app.as_mut().unwrap().menu = 0;
                                        }
                                    }
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    (
                                        wx + 6,
                                        wy + apps.notepad_app.as_mut().unwrap().text_y() - 4,
                                        ww.saturating_sub(12),
                                        wh.saturating_sub(
                                            apps.notepad_app.as_mut().unwrap().text_y() + 28,
                                        ),
                                    ),
                                ) {
                                    let at = apps.notepad_app.as_mut().unwrap().index_at_scrolled(
                                        mx - wx,
                                        my - wy,
                                        ww,
                                        compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0),
                                    );

                                    apps.notepad_app.as_mut().unwrap().cursor = at;

                                    if keyboard.shift {
                                        let anchor = apps
                                            .notepad_app
                                            .as_mut()
                                            .unwrap()
                                            .selection_anchor
                                            .unwrap_or(at);

                                        if anchor == at {
                                            apps.notepad_app.as_mut().unwrap().selection_start =
                                                None;

                                            apps.notepad_app.as_mut().unwrap().selection_end = None;
                                        } else {
                                            apps.notepad_app.as_mut().unwrap().selection_start =
                                                Some(anchor.min(at));

                                            apps.notepad_app.as_mut().unwrap().selection_end =
                                                Some(anchor.max(at));
                                        }
                                    } else {
                                        apps.notepad_app.as_mut().unwrap().selection_anchor =
                                            Some(at);

                                        apps.notepad_app.as_mut().unwrap().selection_start = None;

                                        apps.notepad_app.as_mut().unwrap().selection_end = None;
                                    }

                                    apps.notepad_app.as_mut().unwrap().menu = 0;
                                }
                            }

                            3 => {
                                let status_rect =
                                    apps::browser::BrowserApp::status_bar_rect(wx, wy, ww, wh);
                                if !gui::in_rect(mx, my, status_rect) {
                                    apps.browser_app.reset_status_click_streak();
                                }
                                if apps.browser_app.find_active
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::browser::BrowserApp::find_bar_rect(wx, wy, ww),
                                    )
                                {
                                    apps.browser_app.find_next();
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::browser::BrowserApp::toolbar_menu_button_rect(wx, wy, ww),
                                ) {
                                    apps.browser_app.toolbar_menu_open =
                                        !apps.browser_app.toolbar_menu_open;
                                } else if apps.browser_app.toolbar_menu_open {
                                    let item = (0..5).find(|&i| {
                                        gui::in_rect(
                                            mx,
                                            my,
                                            apps::browser::BrowserApp::toolbar_menu_item_rect(
                                                wx, wy, ww, wh, i,
                                            ),
                                        )
                                    });
                                    apps.browser_app.toolbar_menu_open = false;
                                    match item {
                                        Some(0) => apps.browser_app.show_history(),
                                        Some(1) => {
                                            let current = core::str::from_utf8(
                                                &apps.browser_app.url[..apps.browser_app.url_len],
                                            )
                                            .unwrap_or("");
                                            if apps::browser::BOOKMARKS
                                                .iter()
                                                .chain(apps.browser_app.bookmarks_extra.iter())
                                                .any(|b| b.url == current)
                                            {
                                                apps.browser_app.remove_bookmark_current();
                                                let _ = apps
                                                    .browser_app
                                                    .save_bookmarks_to_vfs(&mut fs_mgr.vfs);
                                            } else {
                                                apps.browser_app.add_bookmark_current();
                                                let _ = apps
                                                    .browser_app
                                                    .save_bookmarks_to_vfs(&mut fs_mgr.vfs);
                                            }
                                        }
                                        Some(2) => {
                                            apps.browser_app.save_current_download(&mut fs_mgr.vfs);
                                        }
                                        Some(3) => apps.browser_app.navigate(),
                                        Some(4) => apps.browser_app.show_console(),
                                        _ => {}
                                    }
                                } else if gui::in_rect(mx, my, status_rect) {
                                    if apps.browser_app.status_click_at(
                                        mx,
                                        my,
                                        monotonic_ms(),
                                        apps.settings_app.double_click_ms as u64,
                                    ) {
                                        apps.browser_app.show_console();
                                    }
                                } else if gui::in_rect(mx, my, (wx + 12, wy + 32, 22, 20)) {
                                    apps.browser_app.blur_toolbar_focus();
                                    apps.browser_app.back();
                                } else if gui::in_rect(mx, my, (wx + 38, wy + 32, 22, 20)) {
                                    apps.browser_app.blur_toolbar_focus();
                                    apps.browser_app.forward();
                                } else if gui::in_rect(mx, my, (wx + 64, wy + 32, 22, 20)) {
                                    apps.browser_app.blur_toolbar_focus();
                                    apps.browser_app.navigate();
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    (wx + 94, wy + 32, ww.saturating_sub(142), 20),
                                ) {
                                    apps.browser_app.address_click_at(
                                        mx,
                                        my,
                                        wx + 94,
                                        ww.saturating_sub(142),
                                        monotonic_ms(),
                                        apps.settings_app.double_click_ms as u64,
                                    );
                                    state.browser_address_dragging = true;
                                } else if let Some(url) =
                                    apps.browser_app.click_bookmark_at(mx, my, wx, wy, ww)
                                {
                                    apps.browser_app.set_url(url.as_bytes());

                                    apps.browser_app.navigate();
                                } else if let Some(url) = apps.browser_app.click_link_at(mx, my) {
                                    // Sayfa icindeki baglantiya tiklandi: gecmise kaydet ve ac.

                                    apps.browser_app.set_url(url.as_bytes());

                                    apps.browser_app.navigate();
                                } else if let Some(cache_path) =
                                    apps.browser_app.click_image_at(mx, my)
                                {
                                    // Resim üzerine çift tıkla: görüntüleyicide aç.
                                    let now_ms = monotonic_ms();
                                    let is_double_click = state.browser_last_img_click
                                        == Some((mx, my))
                                        && now_ms.wrapping_sub(state.browser_last_img_click_ms)
                                            <= apps.settings_app.double_click_ms as u64;
                                    state.browser_last_img_click = Some((mx, my));
                                    state.browser_last_img_click_ms = now_ms;
                                    if is_double_click {
                                        state.browser_last_img_click = None;
                                        open_image_file(
                                            &cache_path,
                                            &fs_mgr.vfs,
                                            &apps.paint_apps,
                                            &mut apps.imageviewer_app,
                                        );
                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 4);
                                    }
                                } else {
                                    apps.browser_app.blur_toolbar_focus();
                                    if apps.browser_app.active_tab == 3 {
                                        let doc_x = wx + 8;
                                        let doc_y = wy + 38;
                                        let doc_w = ww.saturating_sub(16);
                                        let doc_h = wh.saturating_sub(46);
                                        apps.browser_app.start_page_selection_at(
                                            mx,
                                            my,
                                            doc_x,
                                            doc_y,
                                            doc_w,
                                            doc_h,
                                            win.scroll.y,
                                        );
                                        state.browser_page_dragging = true;
                                    }
                                }
                            }

                            4 => {
                                // Resim: yakinlastir / uzaklastir
                                let is_compact = ww < 300;
                                if is_compact {
                                    if my >= wy + 32 && my <= wy + 52 {
                                        if mx >= wx + 8 && mx <= wx + 32 {
                                            if let Some(rgb) = apps
                                                .imageviewer_app
                                                .as_mut()
                                                .unwrap()
                                                .wallpaper_rgb()
                                            {
                                                crate::gui::wallpaper::set_custom_image(
                                                    rgb,
                                                    apps.imageviewer_app.as_mut().unwrap().width,
                                                    apps.imageviewer_app.as_mut().unwrap().height,
                                                );
                                                apps.imageviewer_app.as_mut().unwrap().status =
                                                    gui::ui_text(
                                                        "Masaüstü arka planı ayarlandı",
                                                        "Desktop wallpaper set",
                                                    );
                                            }
                                        } else if mx >= wx + ww.saturating_sub(60)
                                            && mx <= wx + ww.saturating_sub(38)
                                        {
                                            apps.imageviewer_app.as_mut().unwrap().zoom_in();
                                        } else if mx >= wx + ww.saturating_sub(32)
                                            && mx <= wx + ww.saturating_sub(10)
                                        {
                                            apps.imageviewer_app.as_mut().unwrap().zoom_out();
                                        }
                                    }
                                } else {
                                    if my >= wy + 32
                                        && my <= wy + 52
                                        && mx >= wx + 8
                                        && mx <= wx + 150
                                    {
                                        if let Some(rgb) =
                                            apps.imageviewer_app.as_mut().unwrap().wallpaper_rgb()
                                        {
                                            crate::gui::wallpaper::set_custom_image(
                                                rgb,
                                                apps.imageviewer_app.as_mut().unwrap().width,
                                                apps.imageviewer_app.as_mut().unwrap().height,
                                            );
                                            apps.imageviewer_app.as_mut().unwrap().status =
                                                gui::ui_text(
                                                    "Masaüstü arka planı ayarlandı",
                                                    "Desktop wallpaper set",
                                                );
                                        }
                                    } else if my >= wy + 32 && my <= wy + 52 {
                                        if mx >= wx + ww - 180 && mx <= wx + ww - 130 {
                                            apps.imageviewer_app.as_mut().unwrap().zoom_in();
                                        } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {
                                            apps.imageviewer_app.as_mut().unwrap().zoom_out();
                                        }
                                    }
                                }
                            }

                            5 => {
                                // Muzik calar: onceki / oynat / sonraki

                                let ctrl_y = wy + 180;

                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 10 && mx <= wx + 46 {
                                        apps.musicplayer_app.current_track =
                                            apps.musicplayer_app.current_track.saturating_sub(1);

                                        apps.musicplayer_app.progress = 0;
                                    } else if mx >= wx + 52 && mx <= wx + 98 {
                                        apps.musicplayer_app.is_playing =
                                            !apps.musicplayer_app.is_playing;
                                    } else if mx >= wx + 104 && mx <= wx + 140 {
                                        apps.musicplayer_app.current_track =
                                            (apps.musicplayer_app.current_track + 1) % 5;

                                        apps.musicplayer_app.progress = 0;
                                    } else if mx >= wx + 146 && mx < wx + 240 {
                                        let player_volume =
                                            (((mx - (wx + 146)) * 100) / 94).min(100) as u8;

                                        apps.settings_app.volume = player_volume;

                                        apps.settings_app.is_muted = false;

                                        apps.musicplayer_app.volume = player_volume as usize;
                                    } else if my >= wy + 198
                                        && my <= wy + 212
                                        && mx >= wx + 150
                                        && mx <= wx + 240
                                    {
                                        let player_volume =
                                            (((mx - (wx + 150)) * 100) / 90).min(100) as u8;

                                        apps.settings_app.volume = player_volume;

                                        apps.settings_app.is_muted = false;

                                        apps.musicplayer_app.volume = player_volume as usize;
                                    }
                                } else if mx >= wx + 250
                                    && mx < wx + ww - 8
                                    && my >= wy + 96
                                    && my < wy + 194
                                {
                                    apps.musicplayer_app.current_track =
                                        ((my - (wy + 96)) / 18).min(4);

                                    apps.musicplayer_app.progress = 0;

                                    apps.musicplayer_app.is_playing = true;
                                } else if my >= wy + 156
                                    && my <= wy + 170
                                    && mx >= wx + 54
                                    && mx <= wx + 190
                                {
                                    apps.musicplayer_app.progress =
                                        (((mx - (wx + 54)) * 100) / 136).min(100);
                                }
                            }

                            6 => {
                                // Video: oynat / durdur / -10s / +10s / tam ekran

                                let ctrl_y = wy + apps::videoplayer::VideoPlayerApp::CONTROLS_Y;

                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 12 && mx <= wx + 54 {
                                        apps.videoplayer_app.as_mut().unwrap().is_playing =
                                            !apps.videoplayer_app.as_mut().unwrap().is_playing;
                                    } else if mx >= wx + 58 && mx <= wx + 98 {
                                        apps.videoplayer_app.as_mut().unwrap().is_playing = false;

                                        apps.videoplayer_app.as_mut().unwrap().frame = 0;
                                    } else if mx >= wx + 102 && mx <= wx + 150 {
                                        let fps = apps::videoplayer::VideoPlayerApp::FPS;
                                        if apps.videoplayer_app.as_mut().unwrap().frame >= fps * 10
                                        {
                                            apps.videoplayer_app.as_mut().unwrap().frame -=
                                                fps * 10;
                                        } else {
                                            apps.videoplayer_app.as_mut().unwrap().frame = 0;
                                        }
                                    } else if mx >= wx + 154 && mx <= wx + 202 {
                                        let fps = apps::videoplayer::VideoPlayerApp::FPS;
                                        let dur = apps::videoplayer::VideoPlayerApp::DURATION_S;
                                        let total = fps * dur;
                                        apps.videoplayer_app.as_mut().unwrap().frame =
                                            (apps.videoplayer_app.as_mut().unwrap().frame
                                                + fps * 10)
                                                .min(total.saturating_sub(1));
                                    } else if let Some(w) = compositor.windows[idx].as_ref() {
                                        if mx >= wx + w.width.saturating_sub(76)
                                            && mx <= wx + w.width.saturating_sub(12)
                                        {
                                            compositor.toggle_maximize(idx, width, height);
                                        }
                                    }
                                }
                            }

                            7 => {
                                if gui::in_rect(
                                    mx,
                                    my,
                                    apps::taskmgr::TaskManagerApp::applications_tab_rect(wx, wy),
                                ) {
                                    apps.taskmgr_app.active_tab = 0;

                                    apps.taskmgr_app.focused_control = 0;

                                    apps.taskmgr_app.launch_menu_open = false;
                                    apps.taskmgr_app.actions_menu_open = false;
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::taskmgr::TaskManagerApp::performance_tab_rect(wx, wy),
                                ) {
                                    apps.taskmgr_app.active_tab = 1;

                                    apps.taskmgr_app.focused_control = 1;

                                    apps.taskmgr_app.launch_menu_open = false;
                                    apps.taskmgr_app.actions_menu_open = false;
                                } else if apps.taskmgr_app.launch_menu_open {
                                    if let Some(item) =
                                        apps::taskmgr::TaskManagerApp::launch_item_at(
                                            mx, my, wx, wy, ww, wh,
                                        )
                                    {
                                        apps.taskmgr_app.launch_selected = item;

                                        apps.taskmgr_app.focused_control = 2;

                                        let target = apps.taskmgr_app.launch_app_id();

                                        apps.taskmgr_app.launch_menu_open = false;

                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, target);
                                    } else {
                                        apps.taskmgr_app.launch_menu_open = false;
                                    }
                                } else if apps.taskmgr_app.active_tab == 0 {
                                    if gui::in_rect(
                                        mx,
                                        my,
                                        apps::taskmgr::TaskManagerApp::actions_button_rect(
                                            wx, wy, ww,
                                        ),
                                    ) {
                                        apps.taskmgr_app.focused_control = 2;

                                        apps.taskmgr_app.actions_menu_open =
                                            !apps.taskmgr_app.actions_menu_open;
                                    } else if gui::in_rect(
                                        mx,
                                        my,
                                        apps::taskmgr::TaskManagerApp::kill_btn_rect(wx, wy, ww),
                                    ) {
                                        // Force-kill button pressed
                                        if let Some(target) = apps.taskmgr_app.selected_app_idx() {
                                            apps.taskmgr_app.focused_control = 3;
                                            apps.taskmgr_app.status = gui::ui_text(
                                                "Uygulama zorla kapatıldı.",
                                                "Application force-killed.",
                                            );
                                            terminate_app(target, &mut compositor, &mut apps);
                                        }
                                    } else if apps.taskmgr_app.actions_menu_open {
                                        let action = (0..2).find(|&i| gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::actions_menu_item_rect(wx, wy, ww, wh, i)));
                                        apps.taskmgr_app.actions_menu_open = false;
                                        match action {
                                            Some(0) => {
                                                apps.taskmgr_app.launch_menu_open = true;
                                                apps.taskmgr_app.launch_selected = 0;
                                            }
                                            Some(1) => {
                                                if let Some(target) =
                                                    apps.taskmgr_app.selected_app_idx()
                                                {
                                                    apps.taskmgr_app.focused_control = 3;
                                                    apps.taskmgr_app.status = gui::ui_text(
                                                        "Uygulama sonlandırıldı.",
                                                        "Application terminated.",
                                                    );
                                                    terminate_app(
                                                        target,
                                                        &mut compositor,
                                                        &mut apps,
                                                    );
                                                }
                                            }
                                            _ => {}
                                        }
                                    } else if let Some(i) =
                                        apps.taskmgr_app.row_at(mx, my, wx, wy, ww)
                                    {
                                        apps.taskmgr_app.selected = i;

                                        apps.taskmgr_app.focused_control = 4 + i;
                                    }
                                }
                            }

                            8 => {
                                if let Some(key) =
                                    apps::calculator::CalculatorApp::key_at(mx, my, wx, wy, ww)
                                {
                                    apps.calculator_app.as_mut().unwrap().press(key);
                                }
                            }

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
                                let r = apps::paint::PaintApp::brush_tool_rect();
                                let brush_tool_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::eraser_rect();

                                let eraser_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::brush_minus_rect(ww);

                                let brush_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::brush_plus_rect(ww);

                                let brush_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::zoom_minus_rect(ww);

                                let zoom_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::zoom_plus_rect(ww);

                                let zoom_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let file_btn = apps::paint::PaintApp::file_menu_rect(ww);

                                let file_btn_rect =
                                    (wx + file_btn.0, wy + file_btn.1, file_btn.2, file_btn.3);

                                // Acilir menu ogesi seciliyse once onu isle ve menuyu kapat.

                                let mut menu_handled = false;

                                if apps.paint_apps[idx]
                                    .as_ref()
                                    .is_some_and(|p| p.file_menu_open)
                                {
                                    for item in 0..apps::paint::PaintApp::FILE_MENU_COUNT {
                                        let r = apps::paint::PaintApp::file_menu_item_rect(
                                            ww, wh, item,
                                        );

                                        let item_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                        if gui::in_rect(mx, my, item_rect) {
                                            if apps.paint_apps[idx].is_none() {
                                                apps.paint_apps[idx] =
                                                    Some(apps::paint::PaintApp::new());
                                            }

                                            apps.paint_apps[idx].as_mut().unwrap().file_menu_open =
                                                false;

                                            let (_, _, cw, ch) =
                                                apps::paint::PaintApp::canvas_rect(ww, wh);

                                            match item {
                                                0 => apps.paint_apps[idx]
                                                    .as_mut()
                                                    .unwrap()
                                                    .new_document(),

                                                1 => {
                                                    with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "BMP açılıyor...",
                                                            "Opening BMP...",
                                                        ),
                                                        || {
                                                            apps.paint_apps[idx]
                                                                .as_mut()
                                                                .unwrap()
                                                                .open_bmp(&fs_mgr.vfs)
                                                        },
                                                    );
                                                }

                                                2 => {
                                                    if with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "Çizim kaydediliyor...",
                                                            "Saving drawing...",
                                                        ),
                                                        || {
                                                            apps.paint_apps[idx]
                                                                .as_mut()
                                                                .unwrap()
                                                                .save_bmp(&mut fs_mgr.vfs, cw, ch)
                                                        },
                                                    ) {
                                                        apps.filemanager_app
                                                            .refresh_from_vfs(&fs_mgr.vfs);
                                                    }
                                                }

                                                3 => {
                                                    // Farkli Kaydet: dosya adini turetip yeni dosyaya yaz.

                                                    let name = {
                                                        let cur = apps.paint_apps[idx]
                                                            .as_ref()
                                                            .unwrap()
                                                            .current_name();

                                                        apps::paint::PaintApp::derive_save_as_name(
                                                            cur,
                                                        )
                                                    };

                                                    if with_busy_progress(
                                                        &mut fb,
                                                        mx,
                                                        my,
                                                        gui::ui_text(
                                                            "Farklı kaydediliyor...",
                                                            "Saving as...",
                                                        ),
                                                        || {
                                                            apps.paint_apps[idx]
                                                                .as_mut()
                                                                .unwrap()
                                                                .save_as(
                                                                    &mut fs_mgr.vfs,
                                                                    cw,
                                                                    ch,
                                                                    name.as_str(),
                                                                )
                                                        },
                                                    ) {
                                                        apps.filemanager_app
                                                            .refresh_from_vfs(&fs_mgr.vfs);
                                                    }
                                                }

                                                4 => {
                                                    apps.paint_apps[idx].as_mut().unwrap().undo();
                                                }

                                                5 => {
                                                    apps.paint_apps[idx].as_mut().unwrap().redo();
                                                }

                                                _ => {
                                                    apps.paint_apps[idx].as_mut().unwrap().clear();
                                                }
                                            }

                                            menu_handled = true;

                                            break;
                                        }
                                    }

                                    // Menu acikken disina tiklandiysa sadece menuyu kapat.

                                    if !menu_handled {
                                        apps.paint_apps[idx].as_mut().unwrap().file_menu_open =
                                            false;

                                        menu_handled = true;
                                    }
                                }

                                // Renk kutularından birine basıldı mı? (if zinciri
                                // dışında hesaplanır; blok koşul olarak
                                // kullanılamaz).
                                let swatch_hit = {
                                    let mut hit = false;
                                    for i in 0..4 {
                                        let r = apps::paint::PaintApp::swatch_rect(i);
                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                    hit
                                };

                                // Renk seçici: hücre ve popup isabeti önceden
                                // hesaplanır. Popup yalnızca tuval alanını
                                // örttüğü için seçici açıkken araç çubuğu
                                // düğmeleri normal şekilde çalışmaya devam
                                // eder; tuvale tıklamak seçiciyi kapatır.
                                let mut picker_cell_hit = None;
                                let mut picker_inside_popup = false;
                                if apps.paint_apps[idx].as_ref().is_some_and(|p| p.picker_open) {
                                    let pr = apps::paint::PaintApp::picker_rect();
                                    picker_inside_popup =
                                        gui::in_rect(mx, my, (wx + pr.0, wy + pr.1, pr.2, pr.3));
                                    if picker_inside_popup {
                                        for i in 0..apps::paint::PICKER_COLORS.len() {
                                            let r = apps::paint::PaintApp::picker_cell_rect(i);
                                            if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3))
                                            {
                                                picker_cell_hit = Some(i);
                                                break;
                                            }
                                        }
                                    }
                                }

                                if menu_handled {

                                    // Menu islemi tamamlandi; diger isabet testleri atlanir.
                                } else if gui::in_rect(mx, my, file_btn_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let p = apps.paint_apps[idx].as_mut().unwrap();

                                    p.file_menu_open = !p.file_menu_open;
                                } else if gui::in_rect(mx, my, open_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text("BMP açılıyor...", "Opening BMP..."),
                                        || {
                                            apps.paint_apps[idx]
                                                .as_mut()
                                                .unwrap()
                                                .open_bmp(&fs_mgr.vfs)
                                        },
                                    );
                                } else if gui::in_rect(mx, my, save_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);

                                    if with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."),
                                        || {
                                            apps.paint_apps[idx].as_mut().unwrap().save_bmp(
                                                &mut fs_mgr.vfs,
                                                cw,
                                                ch,
                                            )
                                        },
                                    ) {
                                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                } else if gui::in_rect(mx, my, clear_rect) {
                                    if let Some(paint) = apps.paint_apps[idx].as_mut() {
                                        paint.clear();
                                    }
                                } else if gui::in_rect(mx, my, undo_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let _ = apps.paint_apps[idx].as_mut().unwrap().undo();
                                } else if gui::in_rect(mx, my, redo_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let _ = apps.paint_apps[idx].as_mut().unwrap().redo();
                                } else if gui::in_rect(mx, my, tool_line_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().tool =
                                        apps::paint::PaintTool::Line;
                                    apps.paint_apps[idx].as_mut().unwrap().picker_open = false;
                                } else if gui::in_rect(mx, my, tool_rect_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().tool =
                                        apps::paint::PaintTool::Rect;
                                    apps.paint_apps[idx].as_mut().unwrap().picker_open = false;
                                } else if gui::in_rect(mx, my, tool_circle_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().tool =
                                        apps::paint::PaintTool::Circle;
                                    apps.paint_apps[idx].as_mut().unwrap().picker_open = false;
                                } else if gui::in_rect(mx, my, picker_btn_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().picker_open =
                                        !apps.paint_apps[idx].as_mut().unwrap().picker_open;
                                } else if gui::in_rect(mx, my, brush_tool_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let p = apps.paint_apps[idx].as_mut().unwrap();
                                    p.tool = apps::paint::PaintTool::Brush;
                                    p.picker_open = false;
                                } else if gui::in_rect(mx, my, eraser_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let p = apps.paint_apps[idx].as_mut().unwrap();
                                    p.tool = apps::paint::PaintTool::Eraser;
                                    p.picker_open = false;
                                } else if gui::in_rect(mx, my, brush_minus_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().adjust_brush(-1);
                                } else if gui::in_rect(mx, my, brush_plus_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().adjust_brush(1);
                                } else if gui::in_rect(mx, my, zoom_minus_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().adjust_zoom(-1);
                                } else if gui::in_rect(mx, my, zoom_plus_rect) {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    apps.paint_apps[idx].as_mut().unwrap().adjust_zoom(1);
                                } else if let Some(i) = picker_cell_hit {
                                    // Renk seçici hücresi.
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }
                                    let p = apps.paint_apps[idx].as_mut().unwrap();
                                    p.color = apps::paint::PICKER_COLORS[i];
                                    p.tool = apps::paint::PaintTool::Brush;
                                    p.picker_open = false;
                                } else if picker_inside_popup {
                                    // Popup içindeki boşluğa tıklama: kapat.
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }
                                    apps.paint_apps[idx].as_mut().unwrap().picker_open = false;
                                } else if swatch_hit {
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let paint = apps.paint_apps[idx].as_mut().unwrap();

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
                                    if apps.paint_apps[idx].is_none() {
                                        apps.paint_apps[idx] = Some(apps::paint::PaintApp::new());
                                    }

                                    let paint = apps.paint_apps[idx].as_mut().unwrap();

                                    // Tuvale tıklamak açık renk seçiciyi kapatır.
                                    paint.picker_open = false;

                                    if let Some((cxp, cyp)) =
                                        paint.to_canvas(ww, wh, mx - wx, my - wy)
                                    {
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

                                let sel_rect =
                                    apps::installer::InstallerApp::drive_select_rect(wx, wy, ww);

                                let radio_fat32 =
                                    apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 0);

                                let radio_ext4 =
                                    apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 1);

                                let _radio_exfat =
                                    apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 2);

                                let fmt_rect =
                                    apps::installer::InstallerApp::format_btn_rect(wx, wy, ww, wh);

                                let btn_rect =
                                    apps::installer::InstallerApp::install_btn_rect(wx, wy, ww, wh);

                                if gui::in_rect(mx, my, sel_rect) {
                                    apps.installer_app.as_mut().unwrap().select_next_drive();
                                } else if gui::in_rect(mx, my, radio_fat32) {
                                    apps.installer_app
                                        .as_mut()
                                        .unwrap()
                                        .set_format_type(apps::installer::FormatFsType::Fat32);
                                } else if gui::in_rect(mx, my, radio_ext4) {
                                    apps.installer_app
                                        .as_mut()
                                        .unwrap()
                                        .set_format_type(apps::installer::FormatFsType::Ext4);
                                }
                                #[cfg(feature = "exfat")]
                                if gui::in_rect(mx, my, _radio_exfat) {
                                    apps.installer_app
                                        .as_mut()
                                        .unwrap()
                                        .set_format_type(apps::installer::FormatFsType::Exfat);
                                }
                                if gui::in_rect(mx, my, fmt_rect) {
                                    // 2. Adim: Sadece bicimlendir. Disk silinecekse
                                    // güçlü onay diyaloğu isteği AÇILIR (yazılı onay).

                                    if apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Formatting
                                        && apps.installer_app.as_mut().unwrap().state
                                            != apps::installer::InstallState::Copying
                                    {
                                        if apps.installer_app.as_mut().unwrap().disk_formatted {
                                            let formatted = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "Disk biçimlendiriliyor...",
                                                    "Formatting disk...",
                                                ),
                                                || {
                                                    apps.installer_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .format_selected_drive()
                                                },
                                            );

                                            if formatted {
                                                apps.filemanager_app.set_path("/media/hdd0");

                                                apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                            }
                                        } else {
                                            // Disk silinecek: güçlü (yazılı) onay iste.
                                            apps.installer_app
                                                .as_mut()
                                                .unwrap()
                                                .open_format_confirm();
                                        }
                                    }
                                } else if gui::in_rect(mx, my, btn_rect) {
                                    // 3. Adim: Onayli tek tikla kurulum.

                                    if apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Formatting
                                        && apps.installer_app.as_mut().unwrap().state
                                            != apps::installer::InstallState::Copying
                                    {
                                        if apps.installer_app.as_mut().unwrap().disk_formatted {
                                            let installed = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "minOs kuruluyor...",
                                                    "Installing minOs...",
                                                ),
                                                || {
                                                    apps.installer_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .perform_install(&mut fs_mgr.vfs)
                                                },
                                            );

                                            if installed {
                                                apps.filemanager_app.set_path("/media/hdd0");
                                            }

                                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                        } else if apps
                                            .installer_app
                                            .as_mut()
                                            .unwrap()
                                            .confirm_install
                                        {
                                            // Onay verildi: bicimlendirip kur.

                                            let installed = with_busy_progress(
                                                &mut fb,
                                                mx,
                                                my,
                                                gui::ui_text(
                                                    "minOs kuruluyor...",
                                                    "Installing minOs...",
                                                ),
                                                || {
                                                    apps.installer_app
                                                        .as_mut()
                                                        .unwrap()
                                                        .perform_install(&mut fs_mgr.vfs)
                                                },
                                            );

                                            if installed {
                                                apps.filemanager_app.set_path("/media/hdd0");
                                            }

                                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                        } else {
                                            // Ilk tiklama: onay iste (diski bicimlendirecek).

                                            apps.installer_app.as_mut().unwrap().confirm_install =
                                                true;

                                            apps.installer_app.as_mut().unwrap().status_msg = (

                                                "UYARI: Disk silinip biçimlendirilecek. Onaylamak için tekrar tıklayın.",

                                                "WARNING: Disk will be erased and formatted. Click again to confirm.",

                                            );
                                        }
                                    }
                                }
                            }

                            10 => {
                                apps.terminal_app
                                    .as_mut()
                                    .unwrap()
                                    .start_selection_at(mx, my, wx, wy, ww, wh);
                                state.terminal_dragging = true;
                            }

                            12 => {
                                apps.viewer3d_app.as_mut().unwrap().handle_mouse_down(
                                    mx.saturating_sub(wx),
                                    my.saturating_sub(wy),
                                    ww,
                                    wh,
                                );
                            }

                            13 => {
                                apps.pkgmgr_app.as_mut().unwrap().handle_click(
                                    mx.saturating_sub(wx),
                                    my.saturating_sub(wy),
                                    ww,
                                    wh,
                                    &mut fs_mgr.vfs,
                                );
                                compositor.sync_installed_apps(&fs_mgr.vfs);
                                if let Some(app_name) =
                                    apps.pkgmgr_app.as_mut().unwrap().take_pending_launch()
                                {
                                    desktop::helpers::launch_installed_pkg(
                                        &app_name,
                                        &mut compositor,
                                        &mut apps,
                                        &mut fs_mgr.vfs,
                                        &mut memory_mgr.pmm,
                                        &now,
                                    );
                                }
                            }

                            14 => {
                                for tab in 0..apps::controlpanel::TAB_COUNT {
                                    if gui::in_rect(
                                        mx,
                                        my,
                                        apps::controlpanel::tab_rect(wx, wy, ww, tab),
                                    ) {
                                        apps::controlpanel::select_tab(tab);

                                        apps::controlpanel::focus_tab(tab);

                                        // Sekme değiştiğinde pencere boyutunu yeni içeriğe göre ayarla,
                                        // Ağ Testi ve Performans Testi düğmeleri ekranın altında kalmasın.
                                        if let Some(w) = compositor.windows[idx].as_mut() {
                                            let (cw, ch) = apps::controlpanel::content_extent();
                                            let titlebar_h = crate::gui::scaled_titlebar_h();
                                            let taskbar_h = crate::gui::scaled_taskbar_h();
                                            let usable_h = height.saturating_sub(taskbar_h);
                                            w.width = cw.min(width).max(w.width);
                                            w.height = ch
                                                .saturating_add(titlebar_h + 1)
                                                .min(usable_h)
                                                .max(w.height);
                                            if w.y + w.height > usable_h {
                                                w.y = usable_h.saturating_sub(w.height);
                                            }
                                            state.force_render = true;
                                        }

                                        break;
                                    }
                                }

                                if gui::in_rect(
                                    mx,
                                    my,
                                    apps::controlpanel::refresh_btn_rect(wx, wy, ww, wh),
                                ) {
                                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 1);
                                } else if gui::in_rect(
                                    mx,
                                    my,
                                    apps::controlpanel::net_test_btn_rect(wx, wy, ww, wh),
                                ) {
                                    apps::controlpanel::start_network_test();
                                } else if apps::controlpanel::active_tab() == 3
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::controlpanel::perf_btn_rect(wx, wy, ww, wh),
                                    )
                                {
                                    apps::controlpanel::click_perf_btn(mx, my, wx, wy, ww, wh);
                                    state.force_render = true;
                                } else if apps::controlpanel::active_tab() == 5
                                    && gui::in_rect(
                                        mx,
                                        my,
                                        apps::controlpanel::rescan_btn_rect(wx, wy, ww, wh),
                                    )
                                {
                                    apps::controlpanel::rescan_drivers();
                                    state.force_render = true;
                                }
                            }

                            15 => {
                                // Oyunlar: dama, mayın tarlası ve satranç bölümleri.
                                if let Some((_, w)) = focused_window_for_app(&compositor, 15) {
                                    let (ax, ay, aw, ah) = app_layout_rect(15, &w);
                                    let changed = apps.fps_game_app.as_mut().unwrap().click(
                                        mx.saturating_sub(ax),
                                        my.saturating_sub(ay),
                                        aw,
                                        ah,
                                    );
                                    if changed {
                                        state.force_render = true;
                                    }
                                }
                            }

                            16 => {
                                // FPS 3D oyun: fare ile bakış, WASD hareket, ateş.
                                // Fare butonu değiştiyse ateş et (focus bağımsızğı)
                                if mouse.left_button != state.last_buttons.0 {
                                    let changed = apps
                                        .fps_3d_app
                                        .as_mut()
                                        .unwrap()
                                        .handle_mouse_click(&mut audio_dev);
                                    if changed {
                                        state.force_render = true;
                                    }
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
            // Dosya yöneticisi onay iletişim kutusu açıkken tuşlar önce diyaloğa gider.
            let fm_handled = fm_confirm_key(&mut apps.desktop_files, &mut fs_mgr.vfs, action)
                || (app_is_focused(&compositor, 0)
                    && fm_confirm_key(&mut apps.filemanager_app, &mut fs_mgr.vfs, action));
            if fm_handled {
                if action == drivers::keyboard::KeyAction::EnterKey {
                    enter_char_consumed = true;
                }
                continue;
            }
            match action {
                drivers::keyboard::KeyAction::ToggleStartMenu => {
                    compositor.is_start_menu_open = !compositor.is_start_menu_open;

                    if compositor.is_start_menu_open {
                        compositor.active_start_group = 0;

                        compositor.clear_start_search();

                        compositor.mark_start_menu_open(monotonic_ms());
                    } else {
                        compositor.close_start_menu();
                    }
                }

                drivers::keyboard::KeyAction::CloseActiveWindow => {
                    let active_window = compositor.active_window_idx.filter(|&idx| {
                        compositor.windows[idx].is_some_and(|w| w.is_open && !w.is_minimized)
                    });

                    if let Some(idx) = active_window {
                        // Check if it's Notepad with unsaved changes
                        if let Some(win) = compositor.windows[idx] {
                            if win.app_id == 2 && win.dirty {
                                // Show unsaved changes dialog
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .show_unsaved_changes_dialog();
                            } else {
                                terminate_app(idx, &mut compositor, &mut apps);
                            }
                        }
                    } else {
                        // Masaustundeyken Alt+F4 guc menusunu acar.

                        compositor.is_start_menu_open = true;

                        compositor.active_start_group = 4;

                        compositor.mark_start_menu_open(monotonic_ms());

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

                    compositor.alt_tab_open_ms = monotonic_ms();

                    compositor.cycle_focus(keyboard.shift);
                }

                drivers::keyboard::KeyAction::WorkspacePrev => {
                    compositor.switch_workspace(-1);

                    state.force_render = true;
                }

                drivers::keyboard::KeyAction::WorkspaceNext => {
                    compositor.switch_workspace(1);

                    state.force_render = true;
                }

                drivers::keyboard::KeyAction::MoveWindowWorkspacePrev => {
                    compositor.move_active_window_workspace(-1);

                    state.force_render = true;
                }

                drivers::keyboard::KeyAction::MoveWindowWorkspaceNext => {
                    compositor.move_active_window_workspace(1);

                    state.force_render = true;
                }

                drivers::keyboard::KeyAction::WinShowDesktop => {
                    // Win+D yalnızca etkin sanal masaüstünü etkiler; diğer
                    // masaüstlerindeki pencereler açık kalır.
                    // Win+D only affects the active virtual desktop; windows
                    // on other workspaces stay visible.
                    let show_desktop = compositor.windows.iter().flatten().any(|w| {
                        w.is_open && !w.is_minimized && w.workspace == compositor.active_workspace
                    });

                    for w in compositor.windows.iter_mut().flatten() {
                        if w.is_open && w.workspace == compositor.active_workspace {
                            w.is_minimized = show_desktop;
                            w.is_focused = false;
                        }
                    }

                    if show_desktop {
                        compositor.active_window_idx = None;
                    } else {
                        compositor.focus_top_window();
                    }
                }

                drivers::keyboard::KeyAction::WinMinimizeAll => {
                    // Yalnızca etkin sanal masaüstündeki pencereleri
                    // küçült; diğer masaüstleri etkilenmez.
                    // Minimize only windows on the active virtual desktop;
                    // other workspaces are unaffected.
                    for w in compositor.windows.iter_mut().flatten() {
                        if w.is_open && w.workspace == compositor.active_workspace {
                            w.is_minimized = true;
                            w.is_focused = false;
                        }
                    }

                    compositor.active_window_idx = None;
                }

                drivers::keyboard::KeyAction::WinOpenFiles => {
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 0);

                    compositor.close_start_menu();
                }

                drivers::keyboard::KeyAction::RenameSelected => {
                    if app_is_focused(&compositor, 0) {
                        apps.filemanager_app.begin_rename();
                    } else if apps.desktop_files.selected_idx < apps.desktop_files.file_count
                        && apps.desktop_files.selected_items[apps.desktop_files.selected_idx]
                    {
                        apps.desktop_files.begin_rename();
                    }
                }

                drivers::keyboard::KeyAction::VolumeUp => {
                    apps.settings_app.volume = apps.settings_app.volume.saturating_add(5).min(100);

                    apps.settings_app.is_muted = false;

                    apps.settings_app.status_msg =
                        gui::ui_text("Ses artırıldı.", "Volume increased.");
                }

                drivers::keyboard::KeyAction::VolumeDown => {
                    apps.settings_app.volume = apps.settings_app.volume.saturating_sub(5);

                    apps.settings_app.status_msg =
                        gui::ui_text("Ses azaltıldı.", "Volume decreased.");
                }

                drivers::keyboard::KeyAction::VolumeMute => {
                    apps.settings_app.is_muted = !apps.settings_app.is_muted;

                    apps.settings_app.status_msg = if apps.settings_app.is_muted {
                        gui::ui_text("Ses kapatıldı.", "Muted.")
                    } else {
                        gui::ui_text("Ses açıldı.", "Unmuted.")
                    };
                }

                drivers::keyboard::KeyAction::MediaPlayPause => {
                    apps.musicplayer_app.is_playing = !apps.musicplayer_app.is_playing;
                }

                drivers::keyboard::KeyAction::MediaPrevious => {
                    apps.musicplayer_app.current_track =
                        apps.musicplayer_app.current_track.saturating_sub(1);

                    apps.musicplayer_app.progress = 0;
                }

                drivers::keyboard::KeyAction::MediaNext => {
                    apps.musicplayer_app.current_track =
                        (apps.musicplayer_app.current_track + 1) % 5;

                    apps.musicplayer_app.progress = 0;
                }

                drivers::keyboard::KeyAction::OpenTaskManager => {
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 7);

                    compositor.close_start_menu();
                }

                drivers::keyboard::KeyAction::ArrowUp => {
                    // Win + Yukari Ok: pencereyi tam ekrana getir (maximize)
                    // Win + Shift + Yukari Ok: ust-sol ceyrege yasla (koshe snap)
                    if keyboard.super_key && keyboard.shift {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(
                                    idx,
                                    gui::SnapPosition::TopLeft,
                                    width,
                                    height,
                                );
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if keyboard.super_key {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.toggle_maximize(idx, width, height);
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if compositor.is_start_menu_open {
                        if compositor.start_search_len > 0 {
                            compositor.move_start_search_selection(
                                apps.settings_app.language.is_english(),
                                false,
                            );
                        } else if !compositor.start_menu_focus_submenu {
                            let group_count = gui::MENU_GROUPS.len();
                            if group_count > 0 {
                                compositor.active_start_group =
                                    if compositor.active_start_group == 0 {
                                        group_count - 1
                                    } else {
                                        compositor.active_start_group - 1
                                    };
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
                            if count > 0 {
                                compositor.selected_start_item = if compositor.selected_start_item
                                    == 0
                                    || compositor.selected_start_item >= count
                                {
                                    count - 1
                                } else {
                                    compositor.selected_start_item - 1
                                };
                            }
                        }
                    } else if compositor.is_context_menu_open {
                        // Sag tik / Baglam menusu gezintisi

                        if compositor.context_file_idx.unwrap_or(0) > 0 {
                            compositor.context_file_idx =
                                Some(compositor.context_file_idx.unwrap() - 1);
                        }
                    } else if app_is_focused(&compositor, 3) && !apps.browser_app.address_focused {
                        // Arrow up scrolls the browser page
                        browser_scroll_by(
                            &mut compositor,
                            &mut apps.browser_app,
                            -(gui::sp(30) as isize),
                        );
                    } else if app_is_focused(&compositor, 17) {
                        // Şehir sürüşü: ok tuşları gaz (yukarı) / fren (aşağı).
                        apps.city_drive_app
                            .as_mut()
                            .unwrap()
                            .handle_key_action(drivers::keyboard::KeyAction::ArrowUp);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 16) {
                        apps.fps_3d_app
                            .as_mut()
                            .unwrap()
                            .handle_arrow_up_down(true, false);
                        state.force_render = true;
                    } else {
                        let np_focused = app_is_focused(&compositor, 2);

                        let terminal_focused = app_is_focused(&compositor, 10);

                        let tm_focused = app_is_focused(&compositor, 7);

                        let fm_focused = app_is_focused(&compositor, 0);

                        let set_focused = app_is_focused(&compositor, 1);

                        if terminal_focused {
                            apps.terminal_app.as_mut().unwrap().history_up();
                        } else if tm_focused && apps.taskmgr_app.launch_menu_open {
                            apps.taskmgr_app.select_launch_prev();
                        } else if np_focused {
                            if apps.notepad_app.as_mut().unwrap().menu == 0 {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_vertical_ext(true, keyboard.shift);

                                notepad_ensure_cursor_visible(
                                    &mut compositor,
                                    apps.notepad_app.as_ref().unwrap(),
                                );
                            }
                        } else if tm_focused && apps.taskmgr_app.active_tab == 0 {
                            apps.taskmgr_app.select_up();
                        } else if fm_focused {
                            let win_w = focused_window_for_app(&compositor, 0)
                                .map(|(_, w)| w.width)
                                .unwrap_or(0);

                            apps.filemanager_app.move_selection_up_ext(
                                win_w,
                                apps::filemanager::SelectMod::from_keys(
                                    keyboard.shift,
                                    keyboard.ctrl,
                                ),
                            );

                            fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                        } else if set_focused && apps.settings_app.focused_control < 5 {
                            apps.settings_app.select_tab_prev();
                        } else if app_is_focused(&compositor, 13) {
                            let win_h = focused_window_for_app(&compositor, 13)
                                .map(|(_, w)| w.height)
                                .unwrap_or(0);

                            apps.pkgmgr_app.as_mut().unwrap().move_selection(-1, win_h);
                        } else if app_is_focused(&compositor, 12) {
                            apps.viewer3d_app.as_mut().unwrap().rot_x -= 0.1;
                        } else if app_is_focused(&compositor, 15) {
                            // Oyunlar: sol/sağ ok bölüm (sekme) değiştirir.
                            let tab = if apps.fps_game_app.as_mut().unwrap().tab == 0 {
                                apps::game::TAB_COUNT - 1
                            } else {
                                apps.fps_game_app.as_mut().unwrap().tab - 1
                            };
                            apps.fps_game_app.as_mut().unwrap().select_tab(tab);
                            state.force_render = true;
                        }
                    }
                }

                drivers::keyboard::KeyAction::ArrowDown => {
                    // Win + Asagi Ok: pencereyi kucult/restore et
                    // Win + Shift + Asagi Ok: ust-sag ceyrege yasla (koshe snap)
                    if keyboard.super_key && keyboard.shift {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(
                                    idx,
                                    gui::SnapPosition::TopRight,
                                    width,
                                    height,
                                );
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if keyboard.super_key {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                if let Some(win) = compositor.windows[idx].as_mut() {
                                    if win.is_maximized {
                                        if let Some((x, y, w, h)) = win.restore {
                                            win.x = x;
                                            win.y = y;
                                            win.width = w;
                                            win.height = h;
                                            win.is_maximized = false;
                                        }
                                    } else {
                                        win.is_minimized = true;
                                    }
                                }
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if compositor.is_start_menu_open {
                        if compositor.start_search_len > 0 {
                            compositor.move_start_search_selection(
                                apps.settings_app.language.is_english(),
                                true,
                            );
                        } else if !compositor.start_menu_focus_submenu {
                            let group_count = gui::MENU_GROUPS.len();
                            if group_count > 0 {
                                compositor.active_start_group =
                                    if compositor.active_start_group + 1 >= group_count {
                                        0
                                    } else {
                                        compositor.active_start_group + 1
                                    };
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

                            if count > 0 {
                                compositor.selected_start_item =
                                    if compositor.selected_start_item >= count - 1 {
                                        0
                                    } else {
                                        compositor.selected_start_item + 1
                                    };
                            }
                        }
                    } else if compositor.is_context_menu_open {
                        let items_len = compositor.context_items().len();

                        let cur = compositor.context_file_idx.unwrap_or(0);

                        if cur + 1 < items_len {
                            compositor.context_file_idx = Some(cur + 1);
                        }
                    } else if app_is_focused(&compositor, 3) && !apps.browser_app.address_focused {
                        // Arrow down scrolls the browser page
                        browser_scroll_by(
                            &mut compositor,
                            &mut apps.browser_app,
                            gui::sp(30) as isize,
                        );
                    } else if app_is_focused(&compositor, 17) {
                        // Şehir sürüşü: ok aşağı frendir.
                        apps.city_drive_app
                            .as_mut()
                            .unwrap()
                            .handle_key_action(drivers::keyboard::KeyAction::ArrowDown);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 16) {
                        apps.fps_3d_app
                            .as_mut()
                            .unwrap()
                            .handle_arrow_up_down(false, true);
                        state.force_render = true;
                    } else {
                        let np_focused = app_is_focused(&compositor, 2);

                        let terminal_focused = app_is_focused(&compositor, 10);

                        let tm_focused = app_is_focused(&compositor, 7);

                        let fm_focused = app_is_focused(&compositor, 0);

                        let set_focused = app_is_focused(&compositor, 1);

                        if terminal_focused {
                            apps.terminal_app.as_mut().unwrap().history_down();
                        } else if tm_focused && apps.taskmgr_app.launch_menu_open {
                            apps.taskmgr_app.select_launch_next();
                        } else if np_focused {
                            if apps.notepad_app.as_mut().unwrap().menu == 0 {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_vertical_ext(false, keyboard.shift);

                                notepad_ensure_cursor_visible(
                                    &mut compositor,
                                    apps.notepad_app.as_ref().unwrap(),
                                );
                            }
                        } else if tm_focused && apps.taskmgr_app.active_tab == 0 {
                            apps.taskmgr_app.select_down();
                        } else if fm_focused {
                            let win_w = focused_window_for_app(&compositor, 0)
                                .map(|(_, w)| w.width)
                                .unwrap_or(0);

                            apps.filemanager_app.move_selection_down_ext(
                                win_w,
                                apps::filemanager::SelectMod::from_keys(
                                    keyboard.shift,
                                    keyboard.ctrl,
                                ),
                            );

                            fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                        } else if set_focused && apps.settings_app.focused_control < 5 {
                            apps.settings_app.select_tab_next();
                        } else if app_is_focused(&compositor, 13) {
                            let win_h = focused_window_for_app(&compositor, 13)
                                .map(|(_, w)| w.height)
                                .unwrap_or(0);

                            apps.pkgmgr_app.as_mut().unwrap().move_selection(1, win_h);
                        } else if app_is_focused(&compositor, 12) {
                            apps.viewer3d_app.as_mut().unwrap().rot_x += 0.1;
                        } else if app_is_focused(&compositor, 15) {
                            let tab = (apps.fps_game_app.as_mut().unwrap().tab + 1)
                                % apps::game::TAB_COUNT;
                            apps.fps_game_app.as_mut().unwrap().select_tab(tab);
                            state.force_render = true;
                        }
                    }
                }

                drivers::keyboard::KeyAction::ArrowLeft => {
                    // Win + Sol Ok: pencereyi sol yarısına yasla (Aero Snap)
                    // Win + Shift + Sol Ok: alt-sol ceyrege yasla (koshe snap)
                    if keyboard.super_key && keyboard.shift {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(
                                    idx,
                                    gui::SnapPosition::BottomLeft,
                                    width,
                                    height,
                                );
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if keyboard.super_key {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(idx, gui::SnapPosition::Left, width, height);
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if apps.desktop_files.confirm_dialog == apps::filemanager::ConfirmDialog::Rename
                    {
                        apps.desktop_files.confirm_rename_move_cursor(false);
                    } else if app_is_focused(&compositor, 0)
                        && apps.filemanager_app.confirm_dialog
                            == apps::filemanager::ConfirmDialog::Rename
                    {
                        apps.filemanager_app.confirm_rename_move_cursor(false);
                    } else if compositor.is_start_menu_open {
                        if compositor.start_search_focused {
                            let target = if keyboard.ctrl {
                                let mut i = compositor.start_search_cursor;
                                while i > 0 && compositor.start_search[i - 1] == b' ' {
                                    i -= 1;
                                }
                                while i > 0
                                    && compositor.start_search[i - 1].is_ascii_alphanumeric()
                                {
                                    i -= 1;
                                }
                                i
                            } else {
                                compositor.start_search_cursor.saturating_sub(1)
                            };
                            compositor.start_search_move_to(target, keyboard.shift);
                        } else {
                            compositor.start_menu_focus_submenu = false;
                        }
                    } else if app_is_focused(&compositor, 0) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 0) {
                            apps.filemanager_app.move_selection_left_ext(
                                w.width,
                                apps::filemanager::SelectMod::from_keys(
                                    keyboard.shift,
                                    keyboard.ctrl,
                                ),
                            );
                        }

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        if keyboard.ctrl {
                            apps.browser_app.address_move_word_left_ext(keyboard.shift);
                        } else {
                            apps.browser_app.address_move_left_ext(keyboard.shift);
                        }
                    } else if app_is_focused(&compositor, 2) {
                        if apps.notepad_app.as_mut().unwrap().menu != 0 {
                            apps.notepad_app.as_mut().unwrap().menu =
                                match apps.notepad_app.as_mut().unwrap().menu {
                                    1 => 3,

                                    2 => 1,

                                    _ => 2,
                                };
                        } else {
                            if keyboard.ctrl {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_word_left_ext(keyboard.shift);
                            } else {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_left_ext(keyboard.shift);
                            }

                            notepad_ensure_cursor_visible(
                                &mut compositor,
                                apps.notepad_app.as_ref().unwrap(),
                            );
                        }
                    } else if app_is_focused(&compositor, 7) && !apps.taskmgr_app.launch_menu_open {
                        apps.taskmgr_app.active_tab = 0;

                        apps.taskmgr_app.focused_control = 0;
                    } else if app_is_focused(&compositor, 1) {
                        apps.settings_app.select_tab_prev();
                    } else if app_is_focused(&compositor, 14) {
                        apps::controlpanel::select_tab_step(false);
                        if let Some((cidx, _)) = focused_window_for_app(&compositor, 14) {
                            if let Some(w) = compositor.windows[cidx].as_mut() {
                                let (cw, ch) = apps::controlpanel::content_extent();
                                let titlebar_h = crate::gui::scaled_titlebar_h();
                                let taskbar_h = crate::gui::scaled_taskbar_h();
                                let usable_h = height.saturating_sub(taskbar_h);
                                w.width = cw.min(width).max(w.width);
                                w.height = ch
                                    .saturating_add(titlebar_h + 1)
                                    .min(usable_h)
                                    .max(w.height);
                                if w.y + w.height > usable_h {
                                    w.y = usable_h.saturating_sub(w.height);
                                }
                                state.force_render = true;
                            }
                        }
                    } else if app_is_focused(&compositor, 12) {
                        apps.viewer3d_app.as_mut().unwrap().rot_y -= 0.1;
                    } else if app_is_focused(&compositor, 15) {
                        let tab = if apps.fps_game_app.as_mut().unwrap().tab == 0 {
                            apps::game::TAB_COUNT - 1
                        } else {
                            apps.fps_game_app.as_mut().unwrap().tab - 1
                        };
                        apps.fps_game_app.as_mut().unwrap().select_tab(tab);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 17) {
                        // Şehir sürüşü: ok sola direksiyon.
                        apps.city_drive_app
                            .as_mut()
                            .unwrap()
                            .handle_key_action(drivers::keyboard::KeyAction::ArrowLeft);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 16) {
                        apps.fps_3d_app.as_mut().unwrap().handle_arrow(true, false);
                        state.force_render = true;
                    }
                }

                drivers::keyboard::KeyAction::ArrowRight => {
                    // Win + Sag Ok: pencereyi sag yarısına yasla (Aero Snap)
                    // Win + Shift + Sag Ok: alt-sag ceyrege yasla (koshe snap)
                    if keyboard.super_key && keyboard.shift {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(
                                    idx,
                                    gui::SnapPosition::BottomRight,
                                    width,
                                    height,
                                );
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if keyboard.super_key {
                        if let Some(idx) = compositor.active_window_idx {
                            if idx < gui::MAX_WINDOWS {
                                compositor.snap_window(
                                    idx,
                                    gui::SnapPosition::Right,
                                    width,
                                    height,
                                );
                                state.force_render = true;
                            }
                        }
                        continue;
                    }
                    if apps.desktop_files.confirm_dialog == apps::filemanager::ConfirmDialog::Rename
                    {
                        apps.desktop_files.confirm_rename_move_cursor(true);
                    } else if app_is_focused(&compositor, 0)
                        && apps.filemanager_app.confirm_dialog
                            == apps::filemanager::ConfirmDialog::Rename
                    {
                        apps.filemanager_app.confirm_rename_move_cursor(true);
                    } else if compositor.is_start_menu_open {
                        if compositor.start_search_focused {
                            let target = if keyboard.ctrl {
                                let mut i = compositor.start_search_cursor;
                                while i < compositor.start_search_len
                                    && compositor.start_search[i].is_ascii_alphanumeric()
                                {
                                    i += 1;
                                }
                                while i < compositor.start_search_len
                                    && compositor.start_search[i] == b' '
                                {
                                    i += 1;
                                }
                                i
                            } else {
                                (compositor.start_search_cursor + 1)
                                    .min(compositor.start_search_len)
                            };
                            compositor.start_search_move_to(target, keyboard.shift);
                        } else {
                            compositor.start_menu_focus_submenu = true;

                            compositor.selected_start_item = 0;
                        }
                    } else if app_is_focused(&compositor, 0) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 0) {
                            apps.filemanager_app.move_selection_right_ext(
                                w.width,
                                apps::filemanager::SelectMod::from_keys(
                                    keyboard.shift,
                                    keyboard.ctrl,
                                ),
                            );
                        }

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        if keyboard.ctrl {
                            apps.browser_app.address_move_word_right_ext(keyboard.shift);
                        } else {
                            apps.browser_app.address_move_right_ext(keyboard.shift);
                        }
                    } else if app_is_focused(&compositor, 2) {
                        if apps.notepad_app.as_mut().unwrap().menu != 0 {
                            apps.notepad_app.as_mut().unwrap().menu =
                                match apps.notepad_app.as_mut().unwrap().menu {
                                    1 => 2,

                                    2 => 3,

                                    _ => 1,
                                };
                        } else {
                            if keyboard.ctrl {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_word_right_ext(keyboard.shift);
                            } else {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .move_right_ext(keyboard.shift);
                            }

                            notepad_ensure_cursor_visible(
                                &mut compositor,
                                apps.notepad_app.as_ref().unwrap(),
                            );
                        }
                    } else if app_is_focused(&compositor, 14) {
                        apps::controlpanel::select_tab_step(true);
                        if let Some((cidx, _)) = focused_window_for_app(&compositor, 14) {
                            if let Some(w) = compositor.windows[cidx].as_mut() {
                                let (cw, ch) = apps::controlpanel::content_extent();
                                let titlebar_h = crate::gui::scaled_titlebar_h();
                                let taskbar_h = crate::gui::scaled_taskbar_h();
                                let usable_h = height.saturating_sub(taskbar_h);
                                w.width = cw.min(width).max(w.width);
                                w.height = ch
                                    .saturating_add(titlebar_h + 1)
                                    .min(usable_h)
                                    .max(w.height);
                                if w.y + w.height > usable_h {
                                    w.y = usable_h.saturating_sub(w.height);
                                }
                                state.force_render = true;
                            }
                        }
                    } else if app_is_focused(&compositor, 12) {
                        apps.viewer3d_app.as_mut().unwrap().rot_y += 0.1;
                    } else if app_is_focused(&compositor, 15) {
                        let tab =
                            (apps.fps_game_app.as_mut().unwrap().tab + 1) % apps::game::TAB_COUNT;
                        apps.fps_game_app.as_mut().unwrap().select_tab(tab);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 17) {
                        // Şehir sürüşü: ok sağa direksiyon.
                        apps.city_drive_app
                            .as_mut()
                            .unwrap()
                            .handle_key_action(drivers::keyboard::KeyAction::ArrowRight);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 16) {
                        apps.fps_3d_app.as_mut().unwrap().handle_arrow(false, true);
                        state.force_render = true;
                    } else if app_is_focused(&compositor, 7) && !apps.taskmgr_app.launch_menu_open {
                        apps.taskmgr_app.active_tab = 1;

                        apps.taskmgr_app.focused_control = 1;
                    } else if app_is_focused(&compositor, 1) {
                        apps.settings_app.select_tab_next();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        // Yalnızca adres çubuğu odaklıyken imleci taşı.
                        if keyboard.ctrl {
                            apps.browser_app.address_move_word_right_ext(keyboard.shift);
                        } else {
                            apps.browser_app.address_move_right_ext(keyboard.shift);
                        }
                    }
                }

                drivers::keyboard::KeyAction::Properties => {
                    // Alt+Enter: secili dosyanin özelliklerini durum cubugunda goster.

                    if app_is_focused(&compositor, 0) {
                        if let Some(item) =
                            apps.filemanager_app.row(apps.filemanager_app.selected_idx)
                        {
                            let mut info = gui::TextBuf::new();

                            info.push_str(item.display_name());

                            info.push_str("  |  ");

                            info.push_str(item.file_type);

                            info.push_str("  |  ");

                            info.push_str(item.size_str.as_str());

                            apps.filemanager_app.set_status_text(info.as_str());
                        }
                    }
                }

                drivers::keyboard::KeyAction::EnterKey => {
                    if app_is_focused(&compositor, 3) && apps.browser_app.find_active {
                        apps.browser_app.find_next();
                        enter_char_consumed = true;
                    }

                    // Alt+Enter: özellikler penceresini ac

                    if keyboard.alt
                        && app_is_focused(&compositor, 0)
                        && apps.filemanager_app.selected_idx < apps.filemanager_app.file_count
                    {
                        let file_idx = apps.filemanager_app.selected_idx;

                        let file = &apps.filemanager_app.files[file_idx];

                        state.properties_name.clear();

                        state.properties_name.push_str(file.display_name());

                        state.properties_type.clear();

                        state.properties_type.push_str(file.file_type);

                        state.properties_size.clear();

                        state.properties_size.push_str(file.size_str.as_str());

                        state.properties_location.clear();

                        state
                            .properties_location
                            .push_str(apps.filemanager_app.path());

                        state.properties_path.clear();

                        let full_path = apps.filemanager_app.selected_full_path();

                        state.properties_path.push_str(&full_path);

                        state.properties_open = true;

                        enter_char_consumed = true;
                    }

                    if compositor.is_start_menu_open {
                        enter_char_consumed = true;

                        if compositor.start_search_len > 0 {
                            if let Some(app_idx) = compositor.selected_start_search_result(
                                apps.settings_app.language.is_english(),
                            ) {
                                if app_idx >= 100 {
                                    let app_name = alloc::string::String::from(
                                        compositor.installed_app_name(app_idx - 100),
                                    );
                                    if !app_name.is_empty() {
                                        desktop::helpers::launch_installed_pkg(
                                            &app_name,
                                            &mut compositor,
                                            &mut apps,
                                            &mut fs_mgr.vfs,
                                            &mut memory_mgr.pmm,
                                            &now,
                                        );
                                    }
                                } else {
                                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);
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

                        let i = if active_g < 3 {
                            gui::MENU_GROUPS[active_g].1 + item_k
                        } else {
                            0
                        };

                        if active_g == 4 && item_k == 0 {
                            if compositor.confirm_shutdown {
                                #[cfg(minos_boot_log)]
                                serial_print("[minOs] Klavyeden sistem kapatma onaylandı.\n");

                                finish_system(&mut fb, &fs_mgr, width, height, false);
                            } else {
                                compositor.confirm_shutdown = true;
                            }
                        } else if active_g == 4 && item_k == 1 {
                            if compositor.confirm_restart {
                                finish_system(&mut fb, &fs_mgr, width, height, true);
                            }

                            compositor.confirm_restart = true;
                        } else {
                            if active_g == 3 {
                                if item_k == 0 {
                                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 13);
                                } else {
                                    let app_name = alloc::string::String::from(
                                        compositor.installed_app_name(item_k - 1),
                                    );
                                    if !app_name.is_empty() {
                                        desktop::helpers::launch_installed_pkg(
                                            &app_name,
                                            &mut compositor,
                                            &mut apps,
                                            &mut fs_mgr.vfs,
                                            &mut memory_mgr.pmm,
                                            &now,
                                        );
                                    }
                                }
                            } else if let Some(app_idx) =
                                gui::MinGuiCompositor::start_menu_app_idx(i)
                            {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);
                            }

                            compositor.close_start_menu();
                        }
                    } else if compositor.is_context_menu_open {
                        enter_char_consumed = true;

                        compositor.is_context_menu_open = false;
                    } else if app_is_focused(&compositor, 7) && apps.taskmgr_app.launch_menu_open {
                        enter_char_consumed = true;

                        let target = apps.taskmgr_app.launch_app_id();

                        apps.taskmgr_app.launch_menu_open = false;

                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, target);
                    } else if app_is_focused(&compositor, 7) {
                        enter_char_consumed = true;

                        match apps.taskmgr_app.focused_control {
                            0 => apps.taskmgr_app.active_tab = 0,

                            1 => apps.taskmgr_app.active_tab = 1,

                            2 => {
                                apps.taskmgr_app.launch_menu_open = true;

                                apps.taskmgr_app.launch_selected = 0;
                            }

                            3 => {
                                if let Some(target) = apps.taskmgr_app.selected_app_idx() {
                                    apps.taskmgr_app.status = gui::ui_text(
                                        "Uygulama sonlandırıldı.",
                                        "Application terminated.",
                                    );

                                    terminate_app(target, &mut compositor, &mut apps);
                                }
                            }

                            _ => {}
                        }
                    } else if app_is_focused(&compositor, 0) {
                        enter_char_consumed = true;

                        match apps.filemanager_app.focused_control {
                            0 => apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs),

                            1 => apps.filemanager_app.new_file(&mut fs_mgr.vfs),

                            2 => apps.filemanager_app.new_folder(&mut fs_mgr.vfs),

                            3 => {
                                apps.filemanager_app.delete_selected();
                            }

                            4 | 5 => {
                                if apps.filemanager_app.path() == crate::fs::vfs::TRASH_DIR {
                                    apps.filemanager_app.restore_selected(&mut fs_mgr.vfs);
                                } else if let Some((tag, path)) =
                                    apps.filemanager_app.enter_selected(&fs_mgr.vfs)
                                {
                                    if tag == "TXT" || tag == "LOG" || tag == "CFG" {
                                        let data = with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text("Dosya açılıyor...", "Opening file..."),
                                            || fs_mgr.vfs.read_file(&path),
                                        );

                                        if let Some(data) = data {
                                            apps.load(2, &fs_mgr.vfs);
                                            apps.notepad_app
                                                .as_mut()
                                                .unwrap()
                                                .open_file(&path, data);
                                        }

                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 2);
                                    } else if tag == "IMG" {
                                        with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text("Resim açılıyor...", "Opening image..."),
                                            || {
                                                open_image_file(
                                                    &path,
                                                    &fs_mgr.vfs,
                                                    &apps.paint_apps,
                                                    &mut apps.imageviewer_app,
                                                )
                                            },
                                        );

                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 4);
                                    } else if tag == "SND" {
                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 5);
                                    } else if tag == "VID" {
                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 6);
                                    } else if tag == "WEB" {
                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 3);
                                    } else if tag == "3D" {
                                        let data = with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text(
                                                "3D model açılıyor...",
                                                "Opening 3D model...",
                                            ),
                                            || fs_mgr.vfs.read_file(&path),
                                        );

                                        if let Some(data) = data {
                                            apps.load(12, &fs_mgr.vfs);
                                            apps.viewer3d_app
                                                .as_mut()
                                                .unwrap()
                                                .load_file(&path, data);
                                        }

                                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 12);
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if app_is_focused(&compositor, 1) {
                        enter_char_consumed = true;

                        if apps.settings_app.focused_control < 5 {
                            apps.settings_app.active_tab = apps.settings_app.focused_control;
                        } else {
                            match apps.settings_app.active_tab {
                                0 => match apps.settings_app.focused_control {
                                    5 => {
                                        keyboard.toggle_layout();

                                        apps.settings_app.keyboard_layout = keyboard.layout;
                                    }

                                    6 => {
                                        apps.settings_app.language.toggle();

                                        apps.settings_app.status_msg = gui::ui_text(
                                            "Arayüz dili değiştirildi.",
                                            "Interface language changed.",
                                        );
                                    }

                                    7 => {
                                        apps.settings_app.begin_utc_edit();
                                    }

                                    8 => {
                                        apps.settings_app.is_muted = !apps.settings_app.is_muted;
                                    }

                                    9 => apps.settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    10 => {
                                        apps.settings_app.reset_defaults();
                                        apps.settings_app.apply_runtime(
                                            &mut *keyboard,
                                            &mut compositor,
                                            &mut now,
                                        );
                                        apps.settings_app.write_config(&mut fs_mgr.vfs);
                                    }

                                    _ => {}
                                },

                                1 => match apps.settings_app.focused_control {
                                    5 => apps.settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    6 => {
                                        apps.settings_app.reset_defaults();
                                        apps.settings_app.apply_runtime(
                                            &mut *keyboard,
                                            &mut compositor,
                                            &mut now,
                                        );
                                        apps.settings_app.write_config(&mut fs_mgr.vfs);
                                    }

                                    _ => {}
                                },

                                2 => match apps.settings_app.focused_control {
                                    4 => {
                                        apps.settings_app.dhcp_enabled =
                                            !apps.settings_app.dhcp_enabled;

                                        apps.settings_app.status_msg = if apps
                                            .settings_app
                                            .dhcp_enabled
                                        {
                                            gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.")
                                        } else {
                                            gui::ui_text(
                                                "Statik IP seçildi.",
                                                "Static IP selected.",
                                            )
                                        };
                                    }

                                    5 => apps.settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    6 => {
                                        apps.settings_app.reset_defaults();
                                        apps.settings_app.apply_runtime(
                                            &mut *keyboard,
                                            &mut compositor,
                                            &mut now,
                                        );
                                        apps.settings_app.write_config(&mut fs_mgr.vfs);
                                    }

                                    _ => {}
                                },

                                _ => match apps.settings_app.focused_control {
                                    4 => apps.settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    5 => {
                                        apps.settings_app.reset_defaults();
                                        apps.settings_app.apply_runtime(
                                            &mut *keyboard,
                                            &mut compositor,
                                            &mut now,
                                        );
                                        apps.settings_app.write_config(&mut fs_mgr.vfs);
                                    }

                                    _ => {}
                                },
                            }
                        }
                    } else if app_is_focused(&compositor, 3) {
                        enter_char_consumed = true;

                        if apps.browser_app.find_active {
                            apps.browser_app.find_next();
                        } else {
                            // Geri/İleri (< >) düğmeleri odak almaz; odak yalnızca
                            // adres çubuğu, yenile ve sayfa içeriği arasında dolaşır.
                            match apps.browser_app.focused_control {
                                apps::browser::BrowserApp::FOCUS_REFRESH
                                | apps::browser::BrowserApp::FOCUS_ADDRESS => {
                                    apps.browser_app.navigate()
                                }
                                3 => apps.browser_app.show_history(),
                                _ => {}
                            }
                        }
                    } else if app_is_focused(&compositor, 8) {
                        enter_char_consumed = true;

                        apps.calculator_app.as_mut().unwrap().press('=');
                    } else if app_is_focused(&compositor, 10) {
                        enter_char_consumed = true;

                        if apps.terminal_app.as_mut().unwrap().has_selection() {
                            apps.terminal_app.as_mut().unwrap().copy_selection();
                            apps.terminal_app.as_mut().unwrap().clear_selection();
                        } else {
                            apps.terminal_app.as_mut().unwrap().submit();

                            if let Some(cmd) = apps.terminal_app.as_mut().unwrap().take_pending() {
                                let n = apps.terminal_app.as_mut().unwrap().pending_len();

                                let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");

                                if let Some(app_idx) = apps.terminal_app.as_mut().unwrap().execute(
                                    line,
                                    &mut fs_mgr.vfs,
                                    &mut memory_mgr.pmm,
                                    &now,
                                ) {
                                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);
                                }

                                handle_terminal_open_request(
                                    &mut apps,
                                    &mut fs_mgr.vfs,
                                    &mut compositor,
                                );

                                apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                            }
                        }
                    } else if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        enter_char_consumed = true;

                        apps.notepad_app.as_mut().unwrap().insert_char('\n');
                    } else if app_is_focused(&compositor, 2) {
                        enter_char_consumed = true;

                        apps.notepad_app.as_mut().unwrap().menu = 0;
                    } else if app_is_focused(&compositor, 11) {
                        enter_char_consumed = true;

                        match apps
                            .installer_app
                            .as_mut()
                            .unwrap()
                            .activate_focused_control()
                        {
                            apps::installer::InstallerAction::None => {}
                            apps::installer::InstallerAction::Format => {
                                if apps.installer_app.as_mut().unwrap().state
                                    != apps::installer::InstallState::Formatting
                                    && apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Copying
                                {
                                    let formatted = with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text(
                                            "Disk biçimlendiriliyor...",
                                            "Formatting disk...",
                                        ),
                                        || {
                                            apps.installer_app
                                                .as_mut()
                                                .unwrap()
                                                .format_selected_drive()
                                        },
                                    );
                                    if formatted {
                                        apps.filemanager_app.set_path("/media/hdd0");
                                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                }
                            }
                            apps::installer::InstallerAction::Install => {
                                if apps.installer_app.as_mut().unwrap().state
                                    != apps::installer::InstallState::Formatting
                                    && apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Copying
                                {
                                    if apps.installer_app.as_mut().unwrap().disk_formatted
                                        || apps.installer_app.as_mut().unwrap().confirm_install
                                    {
                                        let installed = with_busy_progress(
                                            &mut fb,
                                            mx,
                                            my,
                                            gui::ui_text(
                                                "minOs kuruluyor...",
                                                "Installing minOs...",
                                            ),
                                            || {
                                                apps.installer_app
                                                    .as_mut()
                                                    .unwrap()
                                                    .perform_install(&mut fs_mgr.vfs)
                                            },
                                        );
                                        if installed {
                                            apps.filemanager_app.set_path("/media/hdd0");
                                        }
                                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    } else {
                                        apps.installer_app.as_mut().unwrap().confirm_install = true;
                                        // Kurulum onay diyaloğunu görünür hale getir.
                                        apps.installer_app.as_mut().unwrap().confirm_dialog = 2;
                                        apps.installer_app.as_mut().unwrap().confirm_focus = 0;
                                        apps.installer_app.as_mut().unwrap().confirm_typed_clear();
                                        apps.installer_app.as_mut().unwrap().status_msg = (
                                            "UYARI: Disk silinip biçimlendirilecek. Onaylamak için tekrar tıklayın.",
                                            "WARNING: Disk will be erased and formatted. Click again to confirm.",
                                        );
                                    }
                                }
                            }
                            apps::installer::InstallerAction::Cancel
                            | apps::installer::InstallerAction::ConfirmYes
                            | apps::installer::InstallerAction::ConfirmNo => {}
                        }
                    } else if app_is_focused(&compositor, 13) {
                        enter_char_consumed = true;

                        let (win_w, win_h) = focused_window_for_app(&compositor, 13)
                            .map(|(_, w)| (w.width, w.height))
                            .unwrap_or((640, 420));

                        apps.pkgmgr_app.as_mut().unwrap().activate_focused(
                            win_w,
                            win_h,
                            &mut fs_mgr.vfs,
                        );
                    } else if app_is_focused(&compositor, 14) {
                        enter_char_consumed = true;

                        match apps::controlpanel::activate_focused() {
                            apps::controlpanel::CpAction::OpenSettings => {
                                open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 1);
                            }
                            apps::controlpanel::CpAction::NetTest => {
                                apps::controlpanel::start_network_test();
                            }
                            apps::controlpanel::CpAction::PerfTest => {
                                if apps::controlpanel::perf_phase()
                                    != apps::controlpanel::PerfPhase::Collecting
                                {
                                    apps::controlpanel::start_perf_test();
                                }
                            }
                            apps::controlpanel::CpAction::RescanDrivers => {
                                apps::controlpanel::rescan_drivers();
                            }
                            apps::controlpanel::CpAction::None => {}
                        }
                    }
                }

                drivers::keyboard::KeyAction::ShutdownOS => {
                    #[cfg(minos_boot_log)]
                    serial_print("[minOs] Sistem kapatma isteği alındı.\n");

                    finish_system(&mut fb, &fs_mgr, width, height, false);
                }

                drivers::keyboard::KeyAction::CopyText => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().copy_selection();
                    } else if app_is_focused(&compositor, 3) {
                        if apps.browser_app.address_focused {
                            apps.browser_app.address_copy();
                        } else if apps.browser_app.has_page_selection() {
                            apps.browser_app.copy_page_selection();
                        }
                    } else if compositor.is_start_menu_open && compositor.start_search_focused {
                        compositor.copy_start_search();
                    } else if app_is_focused(&compositor, 0) {
                        apps.filemanager_app
                            .copy_selection(&mut file_clipboard, false);
                    } else if apps.desktop_files.selected_items.iter().any(|&s| s)
                        || apps.desktop_files.selected_idx < apps.desktop_files.file_count
                    {
                        apps.desktop_files
                            .copy_selection(&mut file_clipboard, false);
                    } else if app_is_focused(&compositor, 8) {
                        apps.calculator_app.as_mut().unwrap().copy_result();
                    } else if app_is_focused(&compositor, 10) {
                        apps.terminal_app.as_mut().unwrap().copy_input();
                    }
                }

                drivers::keyboard::KeyAction::PasteText => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().paste_clipboard();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_paste();
                    } else if compositor.is_start_menu_open && compositor.start_search_focused {
                        compositor.paste_start_search();
                    } else if app_is_focused(&compositor, 0) {
                        let pasted_count = apps
                            .filemanager_app
                            .paste_selection(&mut file_clipboard, &mut fs_mgr.vfs);
                        // Sonucu bildirime dönüştür: başarı → yeşil ✓, hata → kırmızı ✕.
                        if pasted_count > 0 {
                            gui::toast_success(
                                &mut compositor,
                                monotonic_ms(),
                                gui::ui_text("Yapıştırma tamamlandı", "Paste completed"),
                                gui::ui_text(
                                    "Seçili öğeler hedef klasöre taşındı.",
                                    "Selected items were pasted.",
                                ),
                            );
                        } else if apps.filemanager_app.status_buf.is_empty() {
                            gui::toast_warning(
                                &mut compositor,
                                monotonic_ms(),
                                gui::ui_text("Yapıştırılamadı", "Paste failed"),
                                gui::ui_text(
                                    "Hedef CD-ROM salt okunur veya öğe yok.",
                                    "Target is read-only or nothing to paste.",
                                ),
                            );
                        } else {
                            gui::toast_error(
                                &mut compositor,
                                monotonic_ms(),
                                gui::ui_text("Kopyalama hatası", "Copy error"),
                                apps.filemanager_app.status_buf.as_str(),
                            );
                        }
                    } else if app_is_focused(&compositor, 8) {
                        apps.calculator_app.as_mut().unwrap().paste_expression();
                    } else if app_is_focused(&compositor, 10) {
                        apps.terminal_app.as_mut().unwrap().paste_clipboard();
                    } else {
                        apps.desktop_files
                            .paste_selection(&mut file_clipboard, &mut fs_mgr.vfs);
                        apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                    }
                }

                drivers::keyboard::KeyAction::CutText => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().cut_selection();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_cut();
                    } else if compositor.is_start_menu_open && compositor.start_search_focused {
                        compositor.cut_start_search();
                    } else if app_is_focused(&compositor, 0) {
                        apps.filemanager_app
                            .copy_selection(&mut file_clipboard, true);
                    } else if apps.desktop_files.selected_items.iter().any(|&s| s)
                        || apps.desktop_files.selected_idx < apps.desktop_files.file_count
                    {
                        apps.desktop_files.copy_selection(&mut file_clipboard, true);
                    } else if app_is_focused(&compositor, 8) {
                        apps.calculator_app.as_mut().unwrap().cut_expression();
                    }
                }

                drivers::keyboard::KeyAction::SelectAllText => {
                    let focused = app_is_focused(&compositor, 2);

                    if focused {
                        apps.notepad_app.as_mut().unwrap().select_all();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_select_all();
                    } else if compositor.is_start_menu_open && compositor.start_search_focused {
                        compositor.select_all_start_search();
                    }
                }

                drivers::keyboard::KeyAction::Undo => {
                    let focused = app_is_focused(&compositor, 2);

                    if focused {
                        apps.notepad_app.as_mut().unwrap().undo();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_undo_edit();
                    } else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                                let _ = paint.undo();
                            }
                        }
                    }
                }

                drivers::keyboard::KeyAction::Redo => {
                    let focused = app_is_focused(&compositor, 2);

                    if focused {
                        apps.notepad_app.as_mut().unwrap().redo();
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_redo_edit();
                    } else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                                let _ = paint.redo();
                            }
                        }
                    }
                }

                drivers::keyboard::KeyAction::SaveDocument => {
                    let saved = if app_is_focused(&compositor, 2) {
                        with_busy_progress(
                            &mut fb,
                            mx,
                            my,
                            gui::ui_text("Dosya kaydediliyor...", "Saving file..."),
                            || {
                                apps.notepad_app
                                    .as_mut()
                                    .unwrap()
                                    .save_file(&mut fs_mgr.vfs)
                            },
                        )
                    } else if app_is_focused(&compositor, 3) {
                        with_busy_progress(
                            &mut fb,
                            mx,
                            my,
                            gui::ui_text("Sayfa kaydediliyor...", "Saving page..."),
                            || apps.browser_app.save_current_page(&mut fs_mgr.vfs),
                        )
                    } else if app_is_focused(&compositor, 9) {
                        match focused_window_for_app(&compositor, 9) {
                            Some((paint_idx, pwin)) => {
                                if apps.paint_apps[paint_idx].is_none() {
                                    apps.paint_apps[paint_idx] = Some(apps::paint::PaintApp::new());
                                }
                                let (_, _, ww, wh) = app_layout_rect(9, &pwin);
                                let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);
                                apps.paint_apps[paint_idx].as_mut().is_some_and(|p| {
                                    with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."),
                                        || p.save_bmp(&mut fs_mgr.vfs, cw, ch),
                                    )
                                })
                            }
                            None => false,
                        }
                    } else {
                        false
                    };
                    if saved {
                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                    }
                }

                drivers::keyboard::KeyAction::NewDocument => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().clear();

                        apps.notepad_app.as_mut().unwrap().status_msg =
                            gui::ui_text("Yeni belge oluşturuldu", "New document created");
                    } else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if apps.paint_apps[paint_idx].is_none() {
                                apps.paint_apps[paint_idx] = Some(apps::paint::PaintApp::new());
                            }
                            if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                                paint.new_document();
                            }
                        }
                    }
                }

                drivers::keyboard::KeyAction::OpenDocument => {
                    if app_is_focused(&compositor, 2) {
                        let path = alloc::string::String::from(
                            apps.notepad_app.as_mut().unwrap().file_path_str(),
                        );
                        let data = with_busy_progress(
                            &mut fb,
                            mx,
                            my,
                            gui::ui_text("Dosya açılıyor...", "Opening file..."),
                            || fs_mgr.vfs.read_file(&path),
                        );
                        if let Some(data) = data {
                            apps.notepad_app.as_mut().unwrap().open_file(&path, data);
                        }
                    } else if app_is_focused(&compositor, 3) {
                        // Ctrl+O: kaydedilmiş sayfaların klasörünü Dosya
                        // Yöneticisi'nde aç; .html dosyaları tarayıcıda açılır.
                        apps.filemanager_app.set_path("/browser/pages");
                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, 0);
                    } else if app_is_focused(&compositor, 9) {
                        if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                            if apps.paint_apps[paint_idx].is_none() {
                                apps.paint_apps[paint_idx] = Some(apps::paint::PaintApp::new());
                            }
                            with_busy_progress(
                                &mut fb,
                                mx,
                                my,
                                gui::ui_text("BMP açılıyor...", "Opening BMP..."),
                                || {
                                    apps.paint_apps[paint_idx]
                                        .as_mut()
                                        .unwrap()
                                        .open_bmp(&fs_mgr.vfs)
                                },
                            );
                        }
                    }
                }

                drivers::keyboard::KeyAction::FindText => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().open_find();
                    } else if app_is_focused(&compositor, 3) {
                        apps.browser_app.open_find();
                    }
                }

                drivers::keyboard::KeyAction::ReplaceText => {
                    if app_is_focused(&compositor, 2) {
                        apps.notepad_app.as_mut().unwrap().open_replace();
                    }
                }

                drivers::keyboard::KeyAction::HomeKey => {
                    if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        if keyboard.ctrl {
                            apps.notepad_app
                                .as_mut()
                                .unwrap()
                                .move_document_home(keyboard.shift);
                        } else {
                            apps.notepad_app
                                .as_mut()
                                .unwrap()
                                .move_home_ext(keyboard.shift);
                        }

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_home_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 3) {
                        // Home: scroll to top of page
                        browser_scroll_edge(&mut compositor, &mut apps.browser_app, false);
                    } else if app_is_focused(&compositor, 0) {
                        apps.filemanager_app.select_first_ext(
                            apps::filemanager::SelectMod::from_keys(keyboard.shift, keyboard.ctrl),
                        );

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 7) {
                        apps.taskmgr_app.select_first();
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);

                            apps.terminal_app.as_mut().unwrap().scroll_top(vl);
                        }
                    }
                }

                drivers::keyboard::KeyAction::EndKey => {
                    if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        if keyboard.ctrl {
                            apps.notepad_app
                                .as_mut()
                                .unwrap()
                                .move_document_end(keyboard.shift);
                        } else {
                            apps.notepad_app
                                .as_mut()
                                .unwrap()
                                .move_end_ext(keyboard.shift);
                        }

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_end_ext(keyboard.shift);
                    } else if app_is_focused(&compositor, 3) {
                        // End: scroll to bottom of page
                        browser_scroll_edge(&mut compositor, &mut apps.browser_app, true);
                    } else if app_is_focused(&compositor, 0) {
                        apps.filemanager_app.select_last_ext(
                            apps::filemanager::SelectMod::from_keys(keyboard.shift, keyboard.ctrl),
                        );

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 7) {
                        apps.taskmgr_app.select_last();
                    } else if app_is_focused(&compositor, 10) {
                        apps.terminal_app.as_mut().unwrap().scroll_bottom();
                    }
                }

                drivers::keyboard::KeyAction::PageUpKey => {
                    if app_is_focused(&compositor, 0) {
                        let rows = focused_window_for_app(&compositor, 0)
                            .map(|(_, w)| fm_page_rows(&w, &apps.filemanager_app))
                            .unwrap_or(8);

                        apps.filemanager_app.select_page_ext(
                            false,
                            rows,
                            apps::filemanager::SelectMod::from_keys(keyboard.shift, keyboard.ctrl),
                        );

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        let lines = focused_window_for_app(&compositor, 2)
                            .map(|(_, w)| apps.notepad_app.as_mut().unwrap().page_lines(w.height))
                            .unwrap_or(10);

                        apps.notepad_app.as_mut().unwrap().move_page_ext(
                            true,
                            lines,
                            keyboard.shift,
                        );

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) {
                        let page_h = focused_window_for_app(&compositor, 3)
                            .map(|(_, w)| w.height.saturating_sub(100))
                            .unwrap_or(300);
                        browser_scroll_by(
                            &mut compositor,
                            &mut apps.browser_app,
                            -(page_h as isize),
                        );
                    } else if app_is_focused(&compositor, 7) {
                        apps.taskmgr_app.select_page(false, 8);
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);

                            apps.terminal_app
                                .as_mut()
                                .unwrap()
                                .scroll_output(vl as isize, vl);
                        }
                    }
                }

                drivers::keyboard::KeyAction::PageDownKey => {
                    if app_is_focused(&compositor, 0) {
                        let rows = focused_window_for_app(&compositor, 0)
                            .map(|(_, w)| fm_page_rows(&w, &apps.filemanager_app))
                            .unwrap_or(8);

                        apps.filemanager_app.select_page_ext(
                            true,
                            rows,
                            apps::filemanager::SelectMod::from_keys(keyboard.shift, keyboard.ctrl),
                        );

                        fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
                    } else if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        let lines = focused_window_for_app(&compositor, 2)
                            .map(|(_, w)| apps.notepad_app.as_mut().unwrap().page_lines(w.height))
                            .unwrap_or(10);

                        apps.notepad_app.as_mut().unwrap().move_page_ext(
                            false,
                            lines,
                            keyboard.shift,
                        );

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) {
                        let page_h = focused_window_for_app(&compositor, 3)
                            .map(|(_, w)| w.height.saturating_sub(100))
                            .unwrap_or(300);
                        browser_scroll_by(&mut compositor, &mut apps.browser_app, page_h as isize);
                    } else if app_is_focused(&compositor, 7) {
                        apps.taskmgr_app.select_page(true, 8);
                    } else if app_is_focused(&compositor, 10) {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                            let vl = terminal_view_lines(&w);

                            apps.terminal_app
                                .as_mut()
                                .unwrap()
                                .scroll_output(-(vl as isize), vl);
                        }
                    }
                }

                drivers::keyboard::KeyAction::ToggleOverwrite => {
                    if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        apps.notepad_app.as_mut().unwrap().toggle_overwrite();
                    }
                }

                drivers::keyboard::KeyAction::Delete => {
                    // Gorev Yoneticisi odakliysa secili sureci sonlandir.

                    let tm_focused = app_is_focused(&compositor, 7);

                    if tm_focused {
                        if let Some(target) = apps.taskmgr_app.selected_app_idx() {
                            apps.taskmgr_app.status =
                                gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                            terminate_app(target, &mut compositor, &mut apps);
                        }
                    } else if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        apps.notepad_app.as_mut().unwrap().delete_action();

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_delete();
                    } else if app_is_focused(&compositor, 0) {
                        apps.filemanager_app.delete_selected();
                    } else if apps.desktop_files.selected_items.iter().any(|&s| s)
                        || (apps.desktop_files.selected_idx < apps.desktop_files.file_count
                            && apps.desktop_files.selected_items[apps.desktop_files.selected_idx])
                    {
                        apps.desktop_files.delete_selected();

                        apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                    }
                }

                drivers::keyboard::KeyAction::DeleteWordBack => {
                    if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        apps.notepad_app.as_mut().unwrap().delete_word_backward();

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_delete_word_back();
                    } else if app_is_focused(&compositor, 10) {
                        apps.terminal_app.as_mut().unwrap().delete_word();
                    }
                }

                drivers::keyboard::KeyAction::DeleteWordForward => {
                    if app_is_focused(&compositor, 2)
                        && apps.notepad_app.as_mut().unwrap().menu == 0
                    {
                        apps.notepad_app.as_mut().unwrap().delete_word_forward();

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if app_is_focused(&compositor, 3) && apps.browser_app.address_focused {
                        apps.browser_app.address_delete_word_forward();
                    }
                }

                drivers::keyboard::KeyAction::Refresh => {
                    // F5: odakli uygulamanin icerigini yenile.

                    if app_is_focused(&compositor, 0) {
                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                    } else if app_is_focused(&compositor, 3) {
                        apps.browser_app.navigate();
                    } else if app_is_focused(&compositor, 13) {
                        apps.pkgmgr_app
                            .as_mut()
                            .unwrap()
                            .refresh_installed(&fs_mgr.vfs);
                    } else if app_is_focused(&compositor, 7) {
                        apps.taskmgr_app.status = gui::ui_text(
                            "Görev listesi canlı güncelleniyor.",
                            "Task list updates live.",
                        );
                    } else if compositor.active_window_idx.is_none() {
                        apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);
                    }
                }

                drivers::keyboard::KeyAction::BrowserAddress => {
                    let focused = app_is_focused(&compositor, 3);

                    if focused {
                        apps.browser_app.focus_address_end();
                    }
                }

                drivers::keyboard::KeyAction::BrowserRefresh => {
                    let focused = app_is_focused(&compositor, 3);

                    if focused {
                        apps.browser_app.navigate();
                    }
                }

                drivers::keyboard::KeyAction::BrowserBack => {
                    let focused = app_is_focused(&compositor, 3);

                    if focused {
                        apps.browser_app.back();
                    }
                }

                drivers::keyboard::KeyAction::BrowserForward => {
                    let focused = app_is_focused(&compositor, 3);

                    if focused {
                        apps.browser_app.forward();
                    }
                }

                // --- MouseKeys: fare çalışmadığında tuş takımıyla imleç/tıklama ---
                drivers::keyboard::KeyAction::ToggleMouseKeys => {
                    keyboard.mouse_keys = !keyboard.mouse_keys;
                    compositor.push_toast(
                        gui::ToastKind::Info,
                        gui::ui_text("Fare Tuşları", "Mouse Keys"),
                        if keyboard.mouse_keys {
                            gui::ui_text("Tuş takımı ile fare açık", "Numpad controls mouse")
                        } else {
                            gui::ui_text("Tuş takımı ile fare kapalı", "Numpad controls mouse")
                        },
                        monotonic_ms(),
                        2500,
                    );
                    state.force_render = true;
                }
                drivers::keyboard::KeyAction::MouseNumpadUp
                | drivers::keyboard::KeyAction::MouseNumpadDown
                | drivers::keyboard::KeyAction::MouseNumpadLeft
                | drivers::keyboard::KeyAction::MouseNumpadRight => {
                    // İmleci ekran sınırları içinde taşı (hız ayarıyla orantılı).
                    let step = (mouse.speed.clamp(1, 10) as i32 + 3) * 2;
                    let (dx, dy) = match action {
                        drivers::keyboard::KeyAction::MouseNumpadUp => (0, -step),
                        drivers::keyboard::KeyAction::MouseNumpadDown => (0, step),
                        drivers::keyboard::KeyAction::MouseNumpadLeft => (-step, 0),
                        _ => (step, 0),
                    };
                    mouse.x = (mouse.x + dx).clamp(0, width.saturating_sub(1) as i32);
                    mouse.y = (mouse.y + dy).clamp(0, height.saturating_sub(1) as i32);
                }
                drivers::keyboard::KeyAction::MouseClickLeft => {
                    // Basış bu turda değil, bir sonraki turda kenar olarak üretilir;
                    // aksi halde bırakma kenarı asla oluşmaz ve düğme takılırdı.
                    if state.mousekeys_click_left == 0 {
                        state.mousekeys_click_left = 1;
                    }
                    state.force_render = true;
                }
                drivers::keyboard::KeyAction::MouseClickRight => {
                    if state.mousekeys_click_right == 0 {
                        state.mousekeys_click_right = 1;
                    }
                    state.force_render = true;
                }
                drivers::keyboard::KeyAction::MouseSpeedUp => {
                    mouse.speed = mouse.speed.saturating_add(1).min(10);
                }
                drivers::keyboard::KeyAction::MouseSpeedDown => {
                    mouse.speed = mouse.speed.saturating_sub(1).max(1);
                }

                drivers::keyboard::KeyAction::RunDialog => {
                    state.run_dialog_open = !state.run_dialog_open;

                    state.run_dialog_input.clear();

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

                        if let Some(term) = apps.terminal_app.as_mut() {
                            term.push_line(
                                gui::ui_text(
                                    "Ekran görüntüsü alınamadı: Yetersiz bellek",
                                    "Screenshot failed: Out of memory",
                                ),
                                gui::Color::ACCENT_RED,
                            );
                        }
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

                            bmp_data.extend(core::iter::repeat_n(0, row_bytes - sw * 3));
                        }

                        let mut path = alloc::string::String::from("/desktop/ekran_");

                        let mut num = gui::TextBuf::new();

                        num.push_usize(state.screenshot_counter);

                        path.push_str(num.as_str());

                        path.push_str(".bmp");

                        serial_print("[SCREENSHOT] Ekran görüntüsü alınıyor...\n");

                        state.screenshot_counter += 1;

                        let arc_data: alloc::sync::Arc<[u8]> = bmp_data.into();

                        if fs_mgr.vfs.write_large_file(&path, arc_data) {
                            apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            if let Some(term) = apps.terminal_app.as_mut() {
                                term.push_line(
                                    gui::ui_text(
                                        "Ekran görüntüsü kaydedildi: /desktop/",
                                        "Screenshot saved to: /desktop/",
                                    ),
                                    gui::Color::ACCENT_GREEN,
                                );
                            }

                            compositor.push_toast(
                                gui::ToastKind::Success,
                                gui::ui_text("Ekran Görüntüsü Alındı", "Screenshot Captured"),
                                gui::ui_text(
                                    "Görüntü /desktop/ klasörüne kaydedildi.",
                                    "Image saved to /desktop/ folder.",
                                ),
                                monotonic_ms(),
                                4000,
                            );

                            state.force_render = true;

                            serial_print("[SCREENSHOT] Başarıyla kaydedildi: ");

                            serial_print(&path);

                            serial_print("\n");
                        } else {
                            compositor.push_toast(
                                gui::ToastKind::Error,
                                gui::ui_text("Ekran Görüntüsü Hatası", "Screenshot Error"),
                                gui::ui_text(
                                    "Dosya diske yazılamadı!",
                                    "Failed to write file to disk!",
                                ),
                                monotonic_ms(),
                                4000,
                            );

                            serial_print("[SCREENSHOT] HATA: VFS dosyası yazılamadı!\n");
                        }
                    }
                }

                drivers::keyboard::KeyAction::ShowHelp => {
                    // F1 - Yardım ekranı göster
                    let _ = state.force_render;
                }
            }
        }

        // 4. Fare Tekerlek (Scroll)

        let scroll_val = mouse.take_scroll_delta();

        if scroll_val != 0 {
            // Ayarlar > Testler: tekerlek sinamasi sayaci.

            if app_is_focused(&compositor, 1) && apps.settings_app.active_tab == 4 {
                if let Some((_, w)) = focused_window_for_app(&compositor, 1) {
                    let pad = apps::settings::SystemSettings::test_mouse_pad_rect(w.x, w.y);

                    if gui::in_rect(mx, my, pad) {
                        apps.settings_app
                            .note_scroll_test(scroll_val.unsigned_abs() as u32);
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
                    compositor.selected_start_item =
                        compositor.selected_start_item.saturating_sub(1);
                } else if compositor.selected_start_item + 1 < count {
                    compositor.selected_start_item += 1;
                }
            }

            // CMD gecmisi: yalnizca cikti panelini kaydir; giris satiri sabit kalir.

            if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                if w.is_open
                    && w.is_focused
                    && !w.is_minimized
                    && gui::in_rect(
                        mouse.x as usize,
                        mouse.y as usize,
                        (
                            w.x,
                            w.y + gui::scaled_titlebar_h(),
                            w.width,
                            w.height.saturating_sub(gui::scaled_titlebar_h() + 28),
                        ),
                    )
                {
                    let view_lines = w.height.saturating_sub(86) / 13;

                    apps.terminal_app
                        .as_mut()
                        .unwrap()
                        .scroll_output(scroll_val as isize, view_lines);
                }
            }

            // Imlecin altindaki en ust pencerede icerik kaydirmasini guncelle.

            // Scrollbar ayni ScrollState uzerinden cizildigi icin tutamac da

            // tekerlekle birlikte aninda hareket eder.

            let (window_order, window_count) = compositor.draw_order();

            for k in (0..window_count).rev() {
                let idx = window_order[k];

                let Some(w) = compositor.windows[idx] else {
                    continue;
                };

                if !w.is_open
                    || w.is_minimized
                    || !gui::in_rect(mx, my, (w.x, w.y, w.width, w.height))
                {
                    continue;
                }

                let app_id = w.app_id as usize;

                let (cx, cy, cw, ch) = w.content_rect();

                let (c_w, c_h) = window_content_size(
                    app_id,
                    w.width,
                    w.height,
                    w.content_w,
                    w.content_h,
                    &apps.filemanager_app,
                    &apps.notepad_app,
                    &apps.browser_app,
                    apps.settings_app.active_tab,
                );

                let wheel_lay = gui::scrollbar::layout((cx, cy, cw, ch), c_w, c_h, w.scroll);
                let max_y = gui::scrollbar::max_offset(c_h, wheel_lay.view_h);
                let max_x = gui::scrollbar::max_offset(c_w, wheel_lay.view_w);

                let over_content = gui::in_rect(mx, my, (cx, cy, cw, ch));

                if over_content && (max_y > 0 || max_x > 0) {
                    if app_id == 3 {
                        // Tarayıcı: tekerlek yumuşak kaydırma animasyonunu besler.
                        let current = w.scroll.y.min(max_y);
                        let step = gui::sp(56) as isize;
                        let delta = (scroll_val as isize) * step;
                        let target = if delta >= 0 {
                            current.saturating_add(delta as usize)
                        } else {
                            current.saturating_sub(delta.unsigned_abs())
                        }
                        .min(max_y);
                        apps.browser_app
                            .scroll_to(current, target, max_y, monotonic_ms());
                    } else {
                        let step = if app_id == 2 {
                            gui::sp(16)
                        } else {
                            gui::sp(24)
                        };

                        let delta = (scroll_val as isize) * step.max(1) as isize;

                        if let Some(wm) = compositor.windows[idx].as_mut() {
                            // Shift + tekerlek yatay kaydırır (içerik genişse).
                            // Tekerlek "hedefe git" tarzı bir girdidir: ani
                            // sıçrama yerine kısa ease-out animasyonu uygulanır
                            // (tarayıcı hariç — kendi kaydırma sistemini kullanır).
                            let mut target = wm.scroll;
                            if keyboard.shift && max_x > 0 {
                                target.x = ((target.x as isize - delta).max(0) as usize).min(max_x);
                            } else {
                                target.y = ((target.y as isize - delta).max(0) as usize).min(max_y);
                            }
                            // Azaltılmış harekette yumuşak kaydırma da
                            // anında tamamlanır; hareket animasyonu sayılmaz.
                            if gui::anim::reduced_motion() {
                                wm.scroll = target;
                            } else {
                                wm.scroll_anim.start(
                                    wm.scroll,
                                    target,
                                    max_x,
                                    max_y,
                                    monotonic_ms(),
                                );
                                // Animasyon hedefi mevcut ofsetle aynıysa ilk
                                // adımda kapanır; yine de ofseti garantiye al.
                                if !wm.scroll_anim.is_active() {
                                    wm.scroll = target;
                                }
                            }
                            let (dx, dy, dw, dh) = wm.content_rect();
                            // `track` global listeye de işler → tekerlek
                            // karesi tam ekran yerine içerik bölgesiyle
                            // sınırlı kalır.
                            compositor
                                .dirty_tracker
                                .track(idx, crate::gui::dirty_rect::DirtyRect::new(dx, dy, dw, dh));
                        }
                    }
                }

                break;
            }

            // Goruntuleyicide tekerle ile yakinlastirma / uzaklastirma.

            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {
                let (ax, ay, aw, ah) = app_layout_rect(4, &w);
                let mut vw = w;
                vw.x = ax;
                vw.y = ay;
                vw.width = aw;
                vw.height = ah;
                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&vw);

                if gui::in_rect(mx, my, (cx, cy, cw, ch)) {
                    if scroll_val > 0 {
                        apps.imageviewer_app.as_mut().unwrap().zoom_in();
                    } else if scroll_val < 0 {
                        apps.imageviewer_app.as_mut().unwrap().zoom_out();
                    }
                }
            }

            // cizim tuvalinde tekerlek yakinlastirmayi ayarlar.

            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {
                let (ax, ay, aw, ah) = app_layout_rect(12, &w);
                if gui::in_rect(mx, my, (ax, ay, aw, ah)) {
                    apps.viewer3d_app
                        .as_mut()
                        .unwrap()
                        .handle_scroll(scroll_val);
                }
            }

            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {
                if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                    let (ax, ay, aw, ah) = app_layout_rect(9, &w);
                    if paint
                        .to_canvas(aw, ah, mx.saturating_sub(ax), my.saturating_sub(ay))
                        .is_some()
                    {
                        paint.adjust_zoom(scroll_val);
                    }
                }
            }
        }

        // 5. Pencere Surukleme ve Yeniden Boyutlandirma

        if mouse.left_button {
            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {
                let (ax, ay, aw, ah) = app_layout_rect(4, &w);
                let mut vw = w;
                vw.x = ax;
                vw.y = ay;
                vw.width = aw;
                vw.height = ah;
                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&vw);

                if w.is_focused && gui::in_rect(mx, my, (cx, cy, cw, ch)) {
                    if let Some((last_x, last_y)) = state.image_drag_last {
                        let iv = apps.imageviewer_app.as_mut().unwrap();
                        iv.pan_x = iv.pan_x.saturating_add(mx as isize - last_x as isize);
                        iv.pan_y = iv.pan_y.saturating_add(my as isize - last_y as isize);
                    }

                    state.image_drag_last = Some((mx, my));
                } else {
                    state.image_drag_last = None;
                }
            } else {
                state.image_drag_last = None;
            }

            if let Some((_, w)) = focused_window_for_app(&compositor, 1) {
                if w.is_open && w.is_focused && !w.is_minimized {
                    // Ayarlar içeriği kaydırılabilir; çizimle aynı orijini kullan.
                    let ox = w.x.saturating_sub(w.scroll.x);
                    let oy = w.y.saturating_sub(w.scroll.y);
                    if apps.settings_app.active_tab == 0 {
                        let track = apps::settings::SystemSettings::volume_track_rect(ox, oy);

                        if gui::in_rect(
                            mx,
                            my,
                            (track.0, track.1.saturating_sub(5), track.2, track.3 + 10),
                        ) {
                            apps.settings_app
                                .set_volume_from_range(mx, track.0, track.2);
                        }
                    } else if apps.settings_app.active_tab == 1 {
                        let track = apps::settings::SystemSettings::night_light_track_rect(ox, oy);

                        if gui::in_rect(
                            mx,
                            my,
                            (track.0, track.1.saturating_sub(5), track.2, track.3 + 10),
                        ) {
                            let span = track.2.saturating_sub(1).max(1);

                            apps.settings_app.night_light_level =
                                ((mx - track.0).min(span) * 100 / span).min(100) as u8;

                            apps.settings_app.night_light = true;
                        }
                    }
                }
            }

            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {
                if w.is_focused {
                    let (ax, ay, _, _) = app_layout_rect(12, &w);
                    apps.viewer3d_app
                        .as_mut()
                        .unwrap()
                        .handle_mouse_move(mx.saturating_sub(ax), my.saturating_sub(ay));
                }
            }

            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {
                if w.is_focused {
                    // Windows Paint davranisi: sol tus basili tutuldukca fare

                    // hareketi surekli cizer. Tuval disina cikinca vurusu bitir.

                    if apps.paint_apps[paint_idx].is_none() {
                        apps.paint_apps[paint_idx] = Some(apps::paint::PaintApp::new());
                    }

                    let paint = apps.paint_apps[paint_idx].as_mut().unwrap();

                    let (ax, ay, aw, ah) = app_layout_rect(9, &w);

                    if let Some((cxp, cyp)) =
                        paint.to_canvas(aw, ah, mx.saturating_sub(ax), my.saturating_sub(ay))
                    {
                        paint.paint(cxp, cyp);
                    } else {
                        paint.end_stroke();
                    }
                }
            }

            if compositor.is_volume_popup_open {
                let track = compositor.volume_track_rect(&l);

                if gui::in_rect(
                    mx,
                    my,
                    (
                        track.0.saturating_sub(8),
                        track.1.saturating_sub(8),
                        track.2 + 16,
                        track.3 + 16,
                    ),
                ) {
                    apps.settings_app
                        .set_volume_from_range(mx, track.0, track.2);
                }
            }

            if state.notepad_dragging {
                // Uygulama tembel yüklenir; sürükleme sırasında pencere
                // kapanmış/uygulama düşürülmüş olabilir. Bu durumda sürükleme
                // durumunu temizle, unwrap ile panik üretme.
                match apps.notepad_app.as_mut() {
                    None => state.notepad_dragging = false,
                    Some(np) if np.menu == 0 => {
                        if let Some((_, w)) = focused_window_for_app(&compositor, 2) {
                            let at = np.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);

                            np.cursor = at;

                            let anchor = np.selection_anchor.unwrap_or(at);

                            if anchor == at {
                                np.selection_start = None;

                                np.selection_end = None;
                            } else {
                                np.selection_start = Some(anchor.min(at));

                                np.selection_end = Some(anchor.max(at));
                            }
                        }
                    }
                    Some(_) => {}
                }
            }

            if state.browser_address_dragging {
                if let Some((_, w)) = focused_window_for_app(&compositor, 3) {
                    if w.is_focused {
                        apps.browser_app.address_drag_to(mx, w.x + 94);
                    }
                }
            }

            if state.browser_page_dragging {
                if let Some((_, w)) = focused_window_for_app(&compositor, 3) {
                    if w.is_focused {
                        let doc_x = w.x + 8;
                        let doc_y = w.y + 38;
                        let doc_w = w.width.saturating_sub(16);
                        let doc_h = w.height.saturating_sub(46);
                        apps.browser_app.update_page_selection_at(
                            mx, my, doc_x, doc_y, doc_w, doc_h, w.scroll.y,
                        );
                    }
                }
            }

            if state.terminal_dragging {
                if let Some((_, w)) = focused_window_for_app(&compositor, 10) {
                    if w.is_focused {
                        apps.terminal_app
                            .as_mut()
                            .unwrap()
                            .update_selection_at(mx, my, w.x, w.y, w.width, w.height);
                    }
                }
            }

            if let Some((_, w)) = focused_window_for_app(&compositor, 5) {
                if w.is_focused {
                    let ox = w.x.saturating_sub(w.scroll.x);
                    let oy = w.y.saturating_sub(w.scroll.y);
                    if my >= oy + 156 && my <= oy + 172 && mx >= ox + 54 && mx <= ox + 190 {
                        apps.musicplayer_app.progress = (((mx - (ox + 54)) * 100) / 136).min(100);
                    } else if my >= oy + 198 && my <= oy + 214 && mx >= ox + 150 && mx <= ox + 240 {
                        let player_volume = (((mx - (ox + 150)) * 100) / 90).min(100) as u8;
                        apps.settings_app.volume = player_volume;
                        apps.settings_app.is_muted = false;
                        apps.musicplayer_app.volume = player_volume as usize;
                    }
                }
            }

            if state.start_search_dragging && compositor.is_start_menu_open {
                let (menu_x, menu_y, menu_w, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);
                let search = gui::MinGuiCompositor::start_search_rect(menu_x, menu_y, menu_w);
                compositor.start_search_drag_to(mx, search.0 + 6);
            }

            let (drag_item, drag_start, drag_active) = if desktop_drag_source {
                (
                    apps.desktop_files.drag_item,
                    apps.desktop_files.drag_start,
                    apps.desktop_files.drag_active,
                )
            } else {
                (
                    apps.filemanager_app.drag_item,
                    apps.filemanager_app.drag_start,
                    apps.filemanager_app.drag_active,
                )
            };

            if let Some(src_idx) = drag_item {
                if !drag_active && (mx.abs_diff(drag_start.0) > 4 || my.abs_diff(drag_start.1) > 4)
                {
                    if desktop_drag_source {
                        apps.desktop_files.drag_active = true;

                        if !apps.desktop_files.selected_items[src_idx] {
                            apps.desktop_files.selected_items =
                                [false; apps::filemanager::MAX_ITEMS];
                            apps.desktop_files.selected_items[src_idx] = true;
                        }
                    } else {
                        apps.filemanager_app.drag_active = true;

                        apps.filemanager_app.last_click_idx = None;

                        if !apps.filemanager_app.selected_items[src_idx] {
                            apps.filemanager_app.selected_items =
                                [false; apps::filemanager::MAX_ITEMS];
                            apps.filemanager_app.selected_items[src_idx] = true;
                        }
                    }
                }

                let fm_win_opt = compositor
                    .windows
                    .iter()
                    .flatten()
                    .find(|w| w.app_id == 0 && w.is_open && !w.is_minimized);

                if let Some(fm_win) = fm_win_opt {
                    let target = apps.filemanager_app.drop_target_at(
                        fm_win.x,
                        fm_win.y,
                        fm_win.width,
                        fm_win.height,
                        fm_win.scroll.y,
                        mx,
                        my,
                    );

                    if desktop_drag_source {
                        apps.desktop_files.drag_target = target;
                    } else {
                        apps.filemanager_app.drag_target = target;
                    }
                }
            }

            if let Some((idx, grab, thumb_h, track_h, content_h, view_h)) = state.vscroll_drag {
                // Tutamaci fare ile surukle → kaydirma ofseti.

                let track_top = compositor.windows[idx]
                    .map(|w| w.content_rect().1)
                    .unwrap_or(0);

                let max_thumb_travel = track_h.saturating_sub(thumb_h);

                let desired_top = my
                    .saturating_sub(grab)
                    .max(track_top)
                    .min(track_top + max_thumb_travel);

                let ratio = if max_thumb_travel == 0 {
                    0.0
                } else {
                    (desired_top - track_top) as f32 / max_thumb_travel as f32
                };

                let max_off = content_h.saturating_sub(view_h);

                let new_off = ((ratio * max_off as f32) as usize).min(max_off);

                if let Some(wm) = compositor.windows[idx].as_mut() {
                    wm.scroll_anim.cancel();
                    wm.scroll.y = new_off;
                    let (cx, cy, cw, ch) = wm.content_rect();
                    state.drag_dirty_rect = Some((cx, cy, cw, ch));
                    // Drag bölgesini per pencere render listesine işle: kare
                    // çizim skip kararı bu listeden beslenir.
                    compositor
                        .dirty_tracker
                        .mark_window(idx, crate::gui::dirty_rect::DirtyRect::new(cx, cy, cw, ch));
                }
                if compositor.windows[idx].map(|w| w.app_id as usize) == Some(3) {
                    apps.browser_app.sync_external_scroll(new_off);
                }
            } else if let Some((idx, grab, thumb_w, track_w, content_w, view_w)) =
                state.hscroll_drag
            {
                // Yatay tutamaci fare ile surukle → kaydirma ofseti.

                let track_left = compositor.windows[idx]
                    .map(|w| w.content_rect().0)
                    .unwrap_or(0);

                let max_thumb_travel = track_w.saturating_sub(thumb_w);

                let desired_left = mx
                    .saturating_sub(grab)
                    .max(track_left)
                    .min(track_left + max_thumb_travel);

                let ratio = if max_thumb_travel == 0 {
                    0.0
                } else {
                    (desired_left - track_left) as f32 / max_thumb_travel as f32
                };

                let max_off = gui::scrollbar::max_offset(content_w, view_w);
                let new_off = ((ratio * max_off as f32) as usize).min(max_off);

                if let Some(wm) = compositor.windows[idx].as_mut() {
                    wm.scroll_anim.cancel();
                    wm.scroll.x = new_off;
                    let (cx, cy, cw, ch) = wm.content_rect();
                    state.drag_dirty_rect = Some((cx, cy, cw, ch));
                    compositor
                        .dirty_tracker
                        .mark_window(idx, crate::gui::dirty_rect::DirtyRect::new(cx, cy, cw, ch));
                }
            } else if let Some(idx) = state.dragging_win {
                if let Some(w) = compositor.windows[idx].as_mut() {
                    let max_x = width.saturating_sub(w.width);

                    let max_y = height.saturating_sub(w.height + gui::scaled_taskbar_h());

                    let prev_box = (
                        w.x.saturating_sub(4),
                        w.y.saturating_sub(4),
                        w.width + 12,
                        w.height + 12,
                    );

                    w.x = mx.saturating_sub(state.drag_offset_x).min(max_x);

                    w.y = my.saturating_sub(state.drag_offset_y).min(max_y);

                    // Aero Snap kenar/köşe algılama & önizleme kutusu.
                    // Bölge mantığı `gui::snap` modülündedir; bırakma yolu da
                    // aynı fonksiyonu kullandığı için önizleme ile sonuç
                    // birebir aynıdır.

                    let old_snap = state.active_snap_preview;

                    let usable_h = height.saturating_sub(gui::scaled_taskbar_h());

                    state.active_snap_preview = gui::snap::snap_zone(mx, my, width, usable_h)
                        .map(|zone| gui::snap::snap_rect(zone, width, usable_h));

                    let new_box = (
                        w.x.saturating_sub(4),
                        w.y.saturating_sub(4),
                        w.width + 12,
                        w.height + 12,
                    );

                    let mut dirty = match state.drag_prev_box {
                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),
                    };

                    dirty = drivers::video::rect_union(
                        dirty,
                        (mx.saturating_sub(8), my.saturating_sub(8), 24, 24),
                    );

                    // Hem eski hem yeni Aero Snap onizlemesini kirli bolgeye kat.

                    if let Some(p) = old_snap {
                        dirty = drivers::video::rect_union(dirty, p);
                    }

                    if let Some(p) = state.active_snap_preview {
                        dirty = drivers::video::rect_union(dirty, p);
                    }

                    state.drag_dirty_rect = Some(dirty);

                    // Aero Snap onizleme/tasarruf kirli bolgesini per pencere
                    // render listesine isle.
                    compositor.dirty_tracker.mark_window(
                        idx,
                        crate::gui::dirty_rect::DirtyRect::new(dirty.0, dirty.1, dirty.2, dirty.3),
                    );

                    state.drag_prev_box = Some(new_box);
                }
            } else if let Some(idx) = state.resizing_win {
                let edge = state.resize_edge.unwrap_or(gui::ResizeEdge::BottomRight);

                if let Some(w) = compositor.windows[idx].as_mut() {
                    use gui::ResizeEdge::*;

                    let (ox, oy, ow, oh) = state.resize_orig;

                    let left = ox + ow; // sol kenarin sabit sag referansi

                    let right = ox; // sag kenarin sabit sol referansi

                    let top = oy + oh; // ust kenarin sabit alt referansi

                    let bottom = oy; // alt kenarin sabit ust referansi

                    let mut nx = w.x;

                    let mut ny = w.y;

                    let mut nw = w.width;

                    let mut nh = w.height;

                    // Sag kenar surukleniyor

                    if matches!(edge, Right | TopRight | BottomRight) {
                        nw = mx
                            .saturating_sub(right)
                            .max(w.min_width)
                            .min(w.max_width.min(width.saturating_sub(w.x)));
                    }

                    // Sol kenar surukleniyor

                    if matches!(edge, Left | TopLeft | BottomLeft) {
                        let desired = mx.min(left);

                        nw = left
                            .saturating_sub(desired)
                            .max(w.min_width)
                            .min(w.max_width)
                            .min(left);

                        nx = left.saturating_sub(nw).min(width.saturating_sub(nw));
                    }

                    // Alt kenar surukleniyor

                    if matches!(edge, Bottom | BottomLeft | BottomRight) {
                        let max_h = height
                            .saturating_sub(gui::scaled_taskbar_h())
                            .saturating_sub(w.y);

                        nh = my
                            .saturating_sub(bottom)
                            .max(w.min_height)
                            .min(w.max_height)
                            .min(max_h);
                    }

                    // ust kenar surukleniyor

                    if matches!(edge, Top | TopLeft | TopRight) {
                        let desired = my.min(top);

                        nh = top
                            .saturating_sub(desired)
                            .max(w.min_height)
                            .min(w.max_height)
                            .min(top);

                        ny = top.saturating_sub(nh);
                    }

                    let prev_box = (
                        w.x.saturating_sub(4),
                        w.y.saturating_sub(4),
                        w.width + 12,
                        w.height + 12,
                    );

                    w.x = nx;

                    w.y = ny;

                    w.width = nw;

                    w.height = nh;

                    w.is_maximized = false;

                    // Küçülen pencerede kaydırma ofseti içeriğin dışına taşmasın.
                    let (c_w, c_h) = window_content_size(
                        w.app_id as usize,
                        w.width,
                        w.height,
                        w.content_w,
                        w.content_h,
                        &apps.filemanager_app,
                        &apps.notepad_app,
                        &apps.browser_app,
                        apps.settings_app.active_tab,
                    );
                    let (_, _, vcw, vch) = w.content_rect();
                    w.scroll_anim.cancel();
                    w.scroll.x = w.scroll.x.min(gui::scrollbar::max_offset(c_w, vcw));
                    w.scroll.y = w.scroll.y.min(gui::scrollbar::max_offset(c_h, vch));

                    let new_box = (
                        w.x.saturating_sub(4),
                        w.y.saturating_sub(4),
                        w.width + 12,
                        w.height + 12,
                    );

                    let mut dirty = match state.drag_prev_box {
                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),
                    };

                    dirty = drivers::video::rect_union(
                        dirty,
                        (mx.saturating_sub(8), my.saturating_sub(8), 24, 24),
                    );

                    if let Some(p) = state.active_snap_preview {
                        dirty = drivers::video::rect_union(dirty, p);
                    }

                    state.drag_dirty_rect = Some(dirty);

                    // Pencere yeniden boyutlandirma (Aero Snap/resizing) kirli
                    // bolgesini per pencere render listesine isle.
                    compositor.dirty_tracker.mark_window(
                        idx,
                        crate::gui::dirty_rect::DirtyRect::new(dirty.0, dirty.1, dirty.2, dirty.3),
                    );

                    state.drag_prev_box = Some(new_box);
                }
            }
        }

        // Sag fare dugmesi basiliyken Paint tuvalinde surukleme: Windows
        // Paint davranisi olarak sag tiklayip suruklemek silgi darbesi
        // baslatir. Sol tus darbesiyle ayni sekilde birakinca sonlanir.
        if mouse.right_button && !mouse.left_button {
            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {
                if w.is_focused {
                    if apps.paint_apps[paint_idx].is_none() {
                        apps.paint_apps[paint_idx] = Some(apps::paint::PaintApp::new());
                    }

                    let paint = apps.paint_apps[paint_idx].as_mut().unwrap();

                    let (ax, ay, aw, ah) = app_layout_rect(9, &w);

                    if let Some((cxp, cyp)) =
                        paint.to_canvas(aw, ah, mx.saturating_sub(ax), my.saturating_sub(ay))
                    {
                        paint.erase_drag(cxp, cyp);
                    } else {
                        paint.end_erase_drag();
                    }
                }
            }
        }

        if mouse_released {
            if let Some((idx, control)) = state.pending_window_control.take() {
                if control == 0 {
                    // Kapatma: terminate_app ile işlenir (çoğu parametre gerektirir).
                    if let Some(win) = compositor.windows.get(idx).and_then(|slot| *slot) {
                        if win.is_open
                            && !win.is_minimized
                            && gui::in_rect(mx, my, win.close_hit_rect())
                        {
                            terminate_app(idx, &mut compositor, &mut apps);
                        }
                    }
                } else {
                    let _ = event_window::handle_window_control_release(
                        &mut compositor,
                        idx,
                        control,
                        mx,
                        my,
                        width,
                        height,
                    );
                }
            }
            state.image_drag_last = None;
        }

        if mouse_released {
            state.drag_prev_box = None;

            state.drag_dirty_rect = None;

            if let Some(viewer3d) = apps.viewer3d_app.as_mut() {
                viewer3d.handle_mouse_up();
            }

            if let Some(paint_idx) = compositor.active_window_idx {
                if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                    paint.end_stroke();
                    paint.end_erase_drag();
                }
            }

            state.notepad_dragging = false;
            state.browser_address_dragging = false;
            state.browser_page_dragging = false;
            state.terminal_dragging = false;
            state.start_search_dragging = false;

            let (is_drag_active, drag_item, drag_target) = if desktop_drag_source {
                (
                    apps.desktop_files.drag_active,
                    apps.desktop_files.drag_item,
                    apps.desktop_files.drag_target,
                )
            } else {
                (
                    apps.filemanager_app.drag_active,
                    apps.filemanager_app.drag_item,
                    apps.filemanager_app.drag_target,
                )
            };

            if is_drag_active {
                if let Some(src_idx) = drag_item {
                    if let Some(target) = drag_target {
                        let src_app = if desktop_drag_source {
                            &mut apps.desktop_files
                        } else {
                            &mut apps.filemanager_app
                        };

                        match src_app.execute_drop(src_idx, target, keyboard.ctrl, &mut fs_mgr.vfs)
                        {
                            Ok(_) => {
                                apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                            }

                            Err(err) => {
                                if desktop_drag_source {
                                    apps.desktop_files.set_status_text(err.as_str());
                                } else {
                                    apps.filemanager_app.set_status_text(err.as_str());
                                }
                            }
                        }
                    } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {
                        let source = if desktop_drag_source {
                            &mut apps.desktop_files
                        } else {
                            &mut apps.filemanager_app
                        };

                        let indices = source.drag_items;

                        let source_count = source.file_count;

                        let mut done_count = 0;

                        for i in 0..source_count {
                            if !(indices[i] || i == src_idx) {
                                continue;
                            }

                            let name = source.files[i].display_name();

                            if name == ".." {
                                continue;
                            }

                            let mut src_path = alloc::string::String::from(source.path());

                            if !src_path.ends_with('/') {
                                src_path.push('/');
                            }

                            src_path.push_str(name);

                            if keyboard.ctrl {
                                if fs_mgr.vfs.copy_item(&src_path, "/desktop").is_ok() {
                                    done_count += 1;
                                }
                            } else {
                                if fs_mgr.vfs.move_item(&src_path, "/desktop").is_ok() {
                                    done_count += 1;
                                }
                            }
                        }

                        if done_count > 0 {
                            apps.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            let msg = if keyboard.ctrl {
                                gui::ui_text("Masaüstüne kopyalandı.", "Copied to Desktop.")
                            } else {
                                gui::ui_text("Masaüstüne taşındı.", "Moved to Desktop.")
                            };

                            if desktop_drag_source {
                                apps.desktop_files.set_status_text(msg);
                            } else {
                                apps.filemanager_app.set_status_text(msg);
                            }
                        }
                    }
                }
            }

            desktop_drag_source = false;

            apps.desktop_files.drag_item = None;

            apps.desktop_files.drag_active = false;

            apps.desktop_files.drag_target = None;

            apps.filemanager_app.drag_item = None;

            apps.filemanager_app.drag_active = false;

            apps.filemanager_app.drag_target = None;

            // Modern Aero Snap: baslik cubugu ekran kenarlarina suruklenip birakildiginda:

            // - Ekranin ustu: Tam ekran (Maximize)
            // - Ekranin kenarları: Yarı ekran
            // - Ekranin köşeleri: Çeyrek ekran

            if let Some(idx) = state.dragging_win {
                compositor.focus_window(idx);
                let usable_h = height.saturating_sub(gui::scaled_taskbar_h());

                // Bölge mantığı önizleme ile aynı kaynaktan gelir.
                if let Some(zone) = gui::snap::snap_zone(mx, my, width, usable_h) {
                    compositor.snap_window(idx, zone, width, height);
                }
            }

            state.active_snap_preview = None;

            state.dragging_win = None;

            state.resizing_win = None;

            state.resize_edge = None;

            state.vscroll_drag = None;

            state.hscroll_drag = None;
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

            // Dosya yöneticisi onay iletişim kutusu açıkken karakterler diyaloğa gider.
            if fm_confirm_char(&mut apps.desktop_files, &mut fs_mgr.vfs, ch)
                || (app_is_focused(&compositor, 0)
                    && fm_confirm_char(&mut apps.filemanager_app, &mut fs_mgr.vfs, ch))
            {
                continue;
            }

            // Baslat menusu acikken klavye kisayollari menu SIRASINI kullanir

            if state.run_dialog_open {
                if ch == '\x1b' {
                    state.run_dialog_open = false;

                    state.run_dialog_input.clear();
                } else if ch == '\x08' {
                    state.run_dialog_input.pop();
                } else if ch == '\n' {
                    let cmd = state.run_dialog_input.as_str().trim();

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
                        "fps" => Some(15),
                        "oyun" => Some(15),

                        "panel" | "kontrol" | "control" => Some(14),

                        "pkg" | "minpak" | "paket" | "pkgmgr" => Some(13),

                        _ => None,
                    };

                    if let Some(target) = app_id {
                        audio_dev.play_beep(1200, 30);

                        open_app(&mut compositor, &mut apps, &fs_mgr.vfs, target);
                    } else {
                        audio_dev.play_beep(440, 60);
                    }

                    state.run_dialog_open = false;

                    state.run_dialog_input.clear();
                } else if ch >= ' ' && ch != '\x7f' && state.run_dialog_input.len() < 30 {
                    let mut b = [0u8; 4];

                    let s = ch.encode_utf8(&mut b);

                    state.run_dialog_input.push_str(s);
                }

                continue;
            }

            // (pencere indeksini degil) — start_menu_app_idx ile eslenir.

            // özellikler modali acikken ESC ve Enter modali kapatir.

            if state.properties_open {
                if ch == '\x1b' || ch == '\n' {
                    state.properties_open = false;

                    state.properties_name.clear();

                    state.properties_type.clear();

                    state.properties_size.clear();

                    state.properties_location.clear();

                    state.properties_path.clear();

                    state.force_render = true;
                }

                continue;
            }

            if ch == '\x1b' && compositor.is_start_menu_open {
                compositor.close_start_menu();
            } else if ch == '\x1b' && compositor.dismiss_toasts() > 0 {
                // Esc: açık bildirimleri kapat (başka bir katman açık değilken).
                state.force_render = true;
            } else if ch == '\x1b' && compositor.is_context_menu_open {
                compositor.is_context_menu_open = false;

                compositor.is_context_submenu_open = false;
            } else if ch == '\x1b' && apps.taskmgr_app.actions_menu_open {
                apps.taskmgr_app.actions_menu_open = false;
            } else if ch == '\x1b' && apps.taskmgr_app.launch_menu_open {
                apps.taskmgr_app.launch_menu_open = false;
            } else if ch == '\x1b' && app_is_focused(&compositor, 2) {
                // Notepad lazy yüklenir; pencere/odak kaydı uygulama None
                // olduktan sonra bir tur gecikebilir. Escape bu durumda
                // kernel panic üretmeden yok sayılmalıdır.
                if let Some(notepad) = apps.notepad_app.as_mut() {
                    if notepad.find_active {
                        notepad.close_find();
                    } else if notepad.menu != 0 {
                        notepad.menu = 0;
                    }
                }
            } else if ch == '\x1b'
                && app_is_focused(&compositor, 3)
                && apps.browser_app.toolbar_menu_open
            {
                apps.browser_app.toolbar_menu_open = false;
            } else if ch == '\x1b' && app_is_focused(&compositor, 3) && apps.browser_app.find_active
            {
                apps.browser_app.close_find();
            } else if ch == '\x08' && app_is_focused(&compositor, 3) && apps.browser_app.find_active
            {
                apps.browser_app.find_backspace();
            } else if ch == '\x08'
                && app_is_focused(&compositor, 3)
                && !apps.browser_app.address_focused
                && !apps.browser_app.find_active
            {
                // Backspace: git geri (tarayıcıda standart davranış).
                apps.browser_app.back();
            } else if keyboard.ctrl && (ch == 'f' || ch == 'F') && app_is_focused(&compositor, 3) {

                // Ctrl+F is handled by the FindText keyboard action.
            } else if app_is_focused(&compositor, 3)
                && apps.browser_app.find_active
                && ch >= ' '
                && ch != '\x7f'
            {
                apps.browser_app.find_char(ch);
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.utc_editing
                && ch == '\x1b'
            {
                apps.settings_app.cancel_utc_edit();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.utc_editing
                && ch == '\x08'
            {
                apps.settings_app.utc_input_backspace();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.utc_editing
                && (ch == '\r' || ch == '\n')
            {
                if let Some(offset) = apps.settings_app.commit_utc_edit() {
                    now = drivers::rtc::DateTime::read_for_region(offset);
                    compositor.cal_year = now.year;
                    compositor.cal_month = now.month;
                    apps.settings_app.status_msg =
                        gui::ui_text("UTC farkı güncellendi.", "UTC offset updated.");
                }
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.utc_editing
                && (ch == '-' || ch.is_ascii_digit())
            {
                apps.settings_app.utc_input_char(ch);
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.static_ip_edit
                && ch == '\x1b'
            {
                apps.settings_app.static_ip_edit = false;
            } else if ch == '\x1b'
                && app_is_focused(&compositor, 11)
                && apps.installer_app.as_mut().unwrap().confirm_install
            {
                apps.installer_app.as_mut().unwrap().confirm_install = false;
                apps.installer_app.as_mut().unwrap().status_msg = (
                    "Hedef diski seçip önce 'Biçimlendir', sonra 'Tek Tıkla Kur' basınız.",
                    "Select target disk, click 'Format' first, then 'One-Click Install'.",
                );
            } else if ch == '\x1b' && app_is_focused(&compositor, 14) {
                // Kontrol Paneli standart sözleşmesi: birincil durum yoksa Esc
                // paneli varsayılanına (Sistem sekmesi, odak "Ayarlar") döndürür.
                apps::controlpanel::handle_key(apps::controlpanel::CpKey::Escape, false);
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.static_ip_edit
                && ch == '\x08'
            {
                apps.settings_app.static_ip_backspace();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.static_ip_edit
                && ch == '\t'
            {
                apps.settings_app.static_ip_next_octet();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.static_ip_edit
                && ch.is_ascii_digit()
            {
                apps.settings_app.static_ip_input_char(ch);
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.wifi_pass_edit
                && ch == '\x1b'
            {
                apps.settings_app.wifi_pass_cancel();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.wifi_pass_edit
                && ch == '\x08'
            {
                apps.settings_app.wifi_pass_backspace();
            } else if app_is_focused(&compositor, 1)
                && apps.settings_app.wifi_pass_edit
                && ch >= ' '
                && ch != '\x7f'
            {
                apps.settings_app.wifi_pass_char(ch);
            } else if ch == '\x1b'
                && app_is_focused(&compositor, 0)
                && apps.filemanager_app.actions_menu_open
            {
                apps.filemanager_app.actions_menu_open = false;
            } else if ch == '\x1b'
                && app_is_focused(&compositor, 0)
                && apps.filemanager_app.search_len > 0
            {
                apps.filemanager_app.clear_search();

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if ch == '\x1b'
                && (apps.filemanager_app.drag_active || apps.desktop_files.drag_active)
            {
                apps.filemanager_app.drag_item = None;

                apps.filemanager_app.drag_active = false;

                apps.filemanager_app.drag_target = None;

                apps.desktop_files.drag_item = None;

                apps.desktop_files.drag_active = false;

                apps.desktop_files.drag_target = None;
            } else if app_is_focused(&compositor, 0)
                && ch == '\x08'
                && apps.filemanager_app.focused_control == 6
            {
                // Filtre kutusu odaktayken Backspace: filtre metnini siler.
                apps.filemanager_app.search_backspace();

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if app_is_focused(&compositor, 0)
                && ch == '\x08'
                && apps.filemanager_app.focused_control == 5
            {
                // Liste odaktayken Backspace = üst klasör (kök dizinde etkisiz).
                if apps.filemanager_app.go_up() {
                    apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                }

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if (ch == 'a' || ch == 'A') && keyboard.ctrl && app_is_focused(&compositor, 0) {
                // Ctrl+A: tüm görünür öğeleri seç, Ctrl+Shift+A: seçimi temizle.
                if keyboard.shift {
                    apps.filemanager_app.clear_selection();
                } else if apps.filemanager_app.select_all() > 0 {
                    apps.filemanager_app.set_status_text(gui::ui_text(
                        "Tüm öğeler seçildi.",
                        "All items selected.",
                    ));
                }
            } else if (ch == 'g' || ch == 'G') && keyboard.ctrl && app_is_focused(&compositor, 0) {
                // Ctrl+G: liste <-> izgara gorunumu.

                apps.filemanager_app.toggle_view_mode();

                apps.filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if app_is_focused(&compositor, 0)
                && apps.filemanager_app.focused_control == 6
                && ch >= ' '
                && ch != '\x7f'
            {
                // Yalnızca filtre kutusu odaktayken yazılan karakterler filtreler;
                // listede yazı yazmak kısayollarla çakışmaz.
                apps.filemanager_app.search_char(ch);

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if ch == ' '
                && app_is_focused(&compositor, 3)
                && !apps.browser_app.address_focused
            {
                // Space: page down in browser when not in address bar
                let page_h = focused_window_for_app(&compositor, 3)
                    .map(|(_, w)| w.height.saturating_sub(100))
                    .unwrap_or(300);
                browser_scroll_by(&mut compositor, &mut apps.browser_app, page_h as isize);
            } else if (ch == 'd' || ch == 'D')
                && keyboard.ctrl
                && keyboard.shift
                && app_is_focused(&compositor, 3)
                && !apps.browser_app.address_focused
            {
                // Ctrl+Shift+D: toggle DevTools
                apps.browser_app.devtools.toggle();
            } else if ch == ' '
                && app_is_focused(&compositor, 0)
                && apps.filemanager_app.selected_idx < apps.filemanager_app.file_count
            {
                // Space: secili metin/resim icin hizli onizlemeyi ac/kapat.

                apps.filemanager_app.toggle_preview();

                apps.filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                apps.filemanager_app.update_preview(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &apps.filemanager_app);
            } else if compositor.is_start_menu_open && ch == '\x08' {
                compositor.start_search_focused = true;
                compositor.backspace_start_search();
            } else if compositor.is_start_menu_open
                && (ch.is_ascii_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' || ch == '.')
            {
                compositor.start_search_focused = true;

                compositor.push_start_search(ch);
            } else if compositor.is_start_menu_open && ('1'..='9').contains(&ch) {
                let menu_index = (ch as usize) - ('1' as usize);

                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);

                    compositor.close_start_menu();
                }
            } else if compositor.is_start_menu_open
                && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B')
            {
                let menu_index = if ch == 'a' || ch == 'A' { 10 } else { 11 };

                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {
                    open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);

                    compositor.close_start_menu();
                }
            } else if compositor.is_start_menu_open && (ch == 'r' || ch == 'R') {
                if compositor.confirm_restart {
                    finish_system(&mut fb, &fs_mgr, width, height, true);
                }

                compositor.confirm_restart = true;

                compositor.confirm_shutdown = false;
            } else if compositor.is_start_menu_open && ch == '0' {
                if compositor.confirm_shutdown {
                    finish_system(&mut fb, &fs_mgr, width, height, false);
                }

                compositor.confirm_shutdown = true;

                compositor.confirm_restart = false;
            } else if ch == '\t' && app_is_focused(&compositor, 10) {
                apps.terminal_app.as_mut().unwrap().complete();
            } else if ch == '\t' && app_is_focused(&compositor, 7) {
                if keyboard.shift {
                    apps.taskmgr_app.focus_prev_control();
                } else {
                    apps.taskmgr_app.focus_next_control();
                }
            } else if ch == '\t' && app_is_focused(&compositor, 1) {
                if keyboard.shift {
                    apps.settings_app.focus_prev_control();
                } else {
                    apps.settings_app.focus_next_control();
                }
            } else if ch == '\t' && app_is_focused(&compositor, 3) {
                if keyboard.shift {
                    apps.browser_app.focus_prev_control();
                } else {
                    apps.browser_app.focus_next_control();
                }
            } else if ch == '\t' && app_is_focused(&compositor, 0) {
                if keyboard.shift {
                    apps.filemanager_app.focus_prev_control();
                } else {
                    apps.filemanager_app.focus_next_control();
                }
            } else if ch == '\t' && app_is_focused(&compositor, 13) {
                apps.pkgmgr_app
                    .as_mut()
                    .unwrap()
                    .focus_next_control(keyboard.shift);
            } else if ch == '\t' && app_is_focused(&compositor, 14) {
                apps::controlpanel::focus_next_control(keyboard.shift);
            } else if ch == '\t' && app_is_focused(&compositor, 11) {
                apps.installer_app
                    .as_mut()
                    .unwrap()
                    .focus_next_control(keyboard.shift);
            } else if app_is_focused(&compositor, 11)
                && apps.installer_app.as_mut().unwrap().confirm_dialog != 0
            {
                // Kurulum onay diyaloğu açıkken klavye kısayolları
                match ch {
                    'y' | 'Y' | 'e' | 'E' | '\n' | '\r' | ' ' => {
                        apps.installer_app.as_mut().unwrap().confirm_focus = 0; // Evet
                        enter_char_consumed = true;
                        match apps
                            .installer_app
                            .as_mut()
                            .unwrap()
                            .activate_focused_control()
                        {
                            apps::installer::InstallerAction::Format => {
                                if apps.installer_app.as_mut().unwrap().state
                                    != apps::installer::InstallState::Formatting
                                    && apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Copying
                                {
                                    let formatted = with_busy_progress(
                                        &mut fb,
                                        mx,
                                        my,
                                        gui::ui_text(
                                            "Disk biçimlendiriliyor...",
                                            "Formatting disk...",
                                        ),
                                        || {
                                            apps.installer_app
                                                .as_mut()
                                                .unwrap()
                                                .format_selected_drive()
                                        },
                                    );
                                    if formatted {
                                        apps.filemanager_app.set_path("/media/hdd0");
                                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                                    }
                                }
                            }
                            apps::installer::InstallerAction::Install
                                if apps.installer_app.as_mut().unwrap().state
                                    != apps::installer::InstallState::Formatting
                                    && apps.installer_app.as_mut().unwrap().state
                                        != apps::installer::InstallState::Copying =>
                            {
                                let installed = with_busy_progress(
                                    &mut fb,
                                    mx,
                                    my,
                                    gui::ui_text("minOs kuruluyor...", "Installing minOs..."),
                                    || {
                                        apps.installer_app
                                            .as_mut()
                                            .unwrap()
                                            .perform_install(&mut fs_mgr.vfs)
                                    },
                                );
                                if installed {
                                    apps.filemanager_app.set_path("/media/hdd0");
                                }
                                apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                            }
                            _ => {}
                        }
                    }
                    'n' | 'N' | 'h' | 'H' | '\x1b' => {
                        // Esc veya N/H
                        apps.installer_app.as_mut().unwrap().confirm_focus = 1; // Hayır
                        apps.installer_app.as_mut().unwrap().confirm_cancel();
                        enter_char_consumed = true;
                    }
                    '\t' => {
                        apps.installer_app
                            .as_mut()
                            .unwrap()
                            .confirm_focus_next(false);
                        enter_char_consumed = true;
                    }
                    '\x08' => {
                        // Güçlü onay kelimesinde geri silme.
                        if apps.installer_app.as_mut().unwrap().confirm_dialog == 1 {
                            apps.installer_app
                                .as_mut()
                                .unwrap()
                                .confirm_typed_backspace();
                        }
                        enter_char_consumed = true;
                    }
                    _ => {
                        // Biçimlendirme onayındaki harf/rakamlar onay
                        // kelimesine yazılır (güçlü biçimlendirme onayı).
                        if apps.installer_app.as_mut().unwrap().confirm_dialog == 1
                            && ch.is_ascii_alphanumeric()
                        {
                            apps.installer_app.as_mut().unwrap().confirm_focus = 0;
                            apps.installer_app.as_mut().unwrap().confirm_typed_push(ch);
                            enter_char_consumed = true;
                        }
                    }
                }
            } else {
                let browser_focused = app_is_focused(&compositor, 3);

                if browser_focused && apps.browser_app.address_focused {
                    if ch == '\x08' {
                        apps.browser_app.address_backspace();
                    } else if ch == '\n' {
                        apps.browser_app.navigate();
                    } else if ch >= ' ' {
                        apps.browser_app.address_char(ch);
                    }
                }

                let notepad_focused = app_is_focused(&compositor, 2);
                if notepad_focused {
                    if apps.notepad_app.as_mut().unwrap().menu != 0 {
                        apps.notepad_app.as_mut().unwrap().menu = 0;
                    } else if apps.notepad_app.as_mut().unwrap().find_active {
                        if ch == '\x08' {
                            apps.notepad_app.as_mut().unwrap().find_input_backspace();
                        } else if ch == '\t' {
                            if apps.notepad_app.as_mut().unwrap().replace_active {
                                apps.notepad_app.as_mut().unwrap().find_focused_replace =
                                    !apps.notepad_app.as_mut().unwrap().find_focused_replace;
                            }
                        } else if ch == '\n' {
                            if apps.notepad_app.as_mut().unwrap().replace_active
                                && apps.notepad_app.as_mut().unwrap().find_focused_replace
                            {
                                apps.notepad_app.as_mut().unwrap().replace_current();
                            } else {
                                apps.notepad_app.as_mut().unwrap().find_next(true);
                            }
                            notepad_ensure_cursor_visible(
                                &mut compositor,
                                apps.notepad_app.as_ref().unwrap(),
                            );
                        } else if ch >= ' ' {
                            apps.notepad_app.as_mut().unwrap().find_input_char(ch);
                        }
                    } else if ch == '\x08' {
                        apps.notepad_app.as_mut().unwrap().backspace();
                    } else if ch == '\t' {
                        // Tab = girinti ekle, Shift+Tab = girintiyi geri al.

                        if keyboard.shift {
                            apps.notepad_app.as_mut().unwrap().unindent();
                        } else {
                            apps.notepad_app.as_mut().unwrap().insert_indent();
                        }

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    } else if ch == '\n' || ch >= ' ' {
                        apps.notepad_app.as_mut().unwrap().insert_char(ch);

                        notepad_ensure_cursor_visible(
                            &mut compositor,
                            apps.notepad_app.as_ref().unwrap(),
                        );
                    }
                }

                let calc_focused = app_is_focused(&compositor, 8);

                let term_focused = app_is_focused(&compositor, 10);

                if calc_focused {
                    if ch == '\x08' {
                        apps.calculator_app.as_mut().unwrap().backspace();
                    } else if ch == '\x1b' {
                        apps.calculator_app.as_mut().unwrap().clear();
                    } else if ch == '=' {
                        apps.calculator_app.as_mut().unwrap().press('=');
                    } else if ch.is_ascii_digit()
                        || ch == '+'
                        || ch == '-'
                        || ch == '*'
                        || ch == '/'
                        || ch == '.'
                    {
                        apps.calculator_app.as_mut().unwrap().insert(ch);
                    }
                }

                if term_focused {
                    // Odak kaydı, lazy uygulama kapatılırken bir kare gecikmeli
                    // kalabilir. Bu durumda pencere görünür olsa bile uygulama
                    // `None` olabilir; klavye olayı kernel panic üretmemeli.
                    if let Some(term) = apps.terminal_app.as_mut() {
                        if ch == '\x08' {
                            term.clear_selection();
                            term.backspace();
                        } else if ch == '\n' {
                            if term.has_selection() {
                                term.copy_selection();
                                term.clear_selection();
                            } else {
                                term.submit();
                            }
                        } else if ch >= ' ' {
                            term.clear_selection();
                            term.insert(ch);
                        }
                    }
                }

                // Terminalde Enter'a basildiysa bekleyen komutu VFS/PMM baglaminda yurut.
                // `terminal_app` tembel yuklenir: uygulama kapaliysa (None) klavye
                // olaylari cekirdek paniki uretmemeli.

                if term_focused {
                    let pending = apps.terminal_app.as_mut().and_then(|term| {
                        term.take_pending().map(|cmd| {
                            let n = term.pending_len().min(cmd.len());
                            (cmd, n)
                        })
                    });
                    if let Some((cmd, n)) = pending {
                        let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");
                        let app_idx = apps.terminal_app.as_mut().and_then(|term| {
                            term.execute(line, &mut fs_mgr.vfs, &mut memory_mgr.pmm, &now)
                        });
                        if let Some(app_idx) = app_idx {
                            open_app(&mut compositor, &mut apps, &fs_mgr.vfs, app_idx);
                        }

                        handle_terminal_open_request(&mut apps, &mut fs_mgr.vfs, &mut compositor);
                        apps.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);
                    }
                }

                if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {
                    if let Some(paint) = apps.paint_apps[paint_idx].as_mut() {
                        if ch == 'c' || ch == 'C' {
                            paint.clear();
                        } else if ch == 'e' || ch == 'E' {
                            paint.tool = if paint.tool == apps::paint::PaintTool::Eraser {
                                apps::paint::PaintTool::Brush
                            } else {
                                apps::paint::PaintTool::Eraser
                            };
                        }
                    }
                }

                let image_focused = app_is_focused(&compositor, 4);

                if image_focused {
                    if ch == '+' || ch == '=' {
                        apps.imageviewer_app.as_mut().unwrap().zoom_in();
                    } else if ch == '-' || ch == '_' {
                        apps.imageviewer_app.as_mut().unwrap().zoom_out();
                    } else if ch == '0' {
                        apps.imageviewer_app.as_mut().unwrap().zoom_level = 4;
                    }
                }

                let v3d_focused = app_is_focused(&compositor, 12);

                if v3d_focused {
                    if ch == ' ' || ch == 'r' || ch == 'R' {
                        apps.viewer3d_app.as_mut().unwrap().toggle_auto_rotate();
                    } else if ch == 'w' || ch == 'W' {
                        apps.viewer3d_app.as_mut().unwrap().toggle_wireframe();
                    } else if ch == '+' || ch == '=' {
                        apps.viewer3d_app.as_mut().unwrap().zoom_in();
                    } else if ch == '-' || ch == '_' {
                        apps.viewer3d_app.as_mut().unwrap().zoom_out();
                    } else if ch == '1' {
                        apps.viewer3d_app
                            .as_mut()
                            .unwrap()
                            .set_shape(apps::viewer3d::ShapeType::Cube);
                    } else if ch == '2' {
                        apps.viewer3d_app
                            .as_mut()
                            .unwrap()
                            .set_shape(apps::viewer3d::ShapeType::Cylinder);
                    } else if ch == '3' {
                        apps.viewer3d_app
                            .as_mut()
                            .unwrap()
                            .set_shape(apps::viewer3d::ShapeType::Sphere);
                    } else if ch == '4' {
                        apps.viewer3d_app
                            .as_mut()
                            .unwrap()
                            .set_shape(apps::viewer3d::ShapeType::Pyramid);
                    }
                }

                let pkg_focused = app_is_focused(&compositor, 13);

                if pkg_focused {
                    if ch == '\x08' {
                        apps.pkgmgr_app.as_mut().unwrap().backspace_search();
                    } else if ch == '\x1b' {
                        apps.pkgmgr_app.as_mut().unwrap().clear_search();
                    } else if ch >= ' ' {
                        apps.pkgmgr_app.as_mut().unwrap().insert_search_char(ch);
                    }
                }

                let fps_focused = app_is_focused(&compositor, 15);

                if fps_focused && apps.fps_game_app.as_mut().unwrap().handle_key(ch) {
                    state.force_render = true;
                }

                let fps3d_focused = app_is_focused(&compositor, 16);

                if fps3d_focused
                    && apps
                        .fps_3d_app
                        .as_mut()
                        .unwrap()
                        .handle_key(ch, &mut audio_dev)
                {
                    state.force_render = true;
                }

                // Şehir sürüşü: gaz/fren/direksiyon tuşları ve menü komutları.
                let city_focused = app_is_focused(&compositor, 17);

                if city_focused && apps.city_drive_app.as_mut().unwrap().handle_key(ch) {
                    state.force_render = true;
                }

                if !state.run_dialog_open
                    && !compositor.is_start_menu_open
                    && !compositor.is_context_menu_open
                {
                    if let Some(act_idx) = compositor.active_window_idx {
                        if let Some(w) = compositor.windows[act_idx] {
                            let dirty = crate::gui::dirty_rect::DirtyRect::new(
                                w.x.saturating_sub(8),
                                w.y.saturating_sub(8),
                                w.width.saturating_add(16),
                                w.height.saturating_add(16),
                            );
                            compositor.dirty_tracker.track(act_idx, dirty);
                        }
                    }
                }
            }
        }

        let mouse_moved = mouse.x != state.last_mouse_x || mouse.y != state.last_mouse_y;

        // FPS 3D oyun oynanırken (pencere odaklı ve Playing durumunda) fare
        // imlecini gizle; Esc ile menüye dönüldüğünde veya pencere (Alt+F4)
        // kapatıldığında otomatik geri göster. Durum değişiminde yeniden çizim
        // tetiklenir ki gizle/göster hemen ekrana yansısın.
        let fps3d_playing = app_is_focused(&compositor, 16)
            && apps
                .fps_3d_app
                .as_ref()
                .map(|a| a.is_playing())
                .unwrap_or(false);
        let was_cursor_hidden = crate::gui::screens::is_cursor_hidden();
        crate::gui::screens::set_cursor_hidden(fps3d_playing);
        if was_cursor_hidden != fps3d_playing {
            state.force_render = true;
        }

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

        // Görev çubuğu hover geri bildirimi: yalnızca hover edilen öğe
        // değiştiğinde şeridi kirli işaretle. İmleç aynı düğme/sekme üzerinde
        // gezinirken tam genişlikte görev çubuğunu her fare olayında yeniden
        // çizmek belirgin takılmaya yol açıyordu.
        if mouse_moved {
            let strip_top = height.saturating_sub(gui::scaled_taskbar_h());
            let hover_key = compositor.taskbar_hover_key(width, height, mx, my);
            if hover_key != state.last_taskbar_hover_key {
                state.last_taskbar_hover_key = hover_key;
                compositor.dirty_tracker.track(
                    0,
                    crate::gui::dirty_rect::DirtyRect::new(
                        0,
                        strip_top,
                        width,
                        gui::scaled_taskbar_h(),
                    ),
                );
            }

            // Başlık çubuğu hover'i: pencere düğmeleri (kapat/büyüt/küçült)
            // dışında başlık çubuğunun görünümü hover'a bağlı değildir. Bu
            // yüzden yalnızca hover edilen düğme değiştiğinde eski ve yeni
            // pencerenin başlık şeridini kirli işaretle.
            let (order, order_count) = compositor.draw_order();
            let mut title_hover = None;
            for k in (0..order_count).rev() {
                let idx = order[k];
                let Some(w) = compositor.windows[idx] else {
                    continue;
                };
                if !w.is_open
                    || w.is_minimized
                    || w.workspace != compositor.active_workspace
                    || !gui::in_rect(mx, my, (w.x, w.y, w.width, w.height))
                {
                    continue;
                }
                let button = if gui::in_rect(mx, my, w.close_hit_rect()) {
                    1
                } else if gui::in_rect(mx, my, w.max_hit_rect()) {
                    2
                } else if gui::in_rect(mx, my, w.min_hit_rect()) {
                    3
                } else {
                    0
                };
                if button != 0 {
                    title_hover = Some((idx, button));
                }

                // Kaydırma çubuğu hover'i: imleç içerik alanının kenar
                // şeridine (dikey: sağ, yatay: alt) girip çıkarken kirli
                // işaretle; tutamak vurgusu bir sonraki karede görünür.
                // Yalnızca geçiş anında işaretlenir — şerit üzerinde her
                // harekette değil.
                let (cx, cy, cw, ch) = w.content_rect();
                let (c_w, c_h) = window_content_size(
                    w.app_id as usize,
                    w.width,
                    w.height,
                    w.content_w,
                    w.content_h,
                    &apps.filemanager_app,
                    &apps.notepad_app,
                    &apps.browser_app,
                    apps.settings_app.active_tab,
                );
                let sb = gui::scrollbar::SCROLLBAR_THICKNESS;
                let want_v = c_h > ch && ch > 0;
                let want_h = c_w > cw && cw > 0;
                let mut gx = 0usize;
                let mut gy = 0usize;
                let mut gw = 0usize;
                let mut gh = 0usize;
                if want_v {
                    gx = cx + cw.saturating_sub(sb);
                    gy = cy;
                    gw = sb;
                    gh = ch;
                }
                if want_h {
                    let (hx, hy, hw, hh) = (cx, cy + ch.saturating_sub(sb), cw, sb);
                    if gw == 0 {
                        gx = hx;
                        gy = hy;
                        gw = hw;
                        gh = hh;
                    } else {
                        // Dikey şerit zaten kapsıyor; yatay şeridi birleşik
                        // sınırlayıcı kutuyla genişlet (tekrar işaretleme
                        // zaten birleşir, basit tut).
                        let x1 = gx.max(hx);
                        let y1 = gy.max(hy);
                        let x2 = (gx + gw).max(hx + hw);
                        let y2 = (gy + gh).max(hy + hh);
                        gx = x1;
                        gy = y1;
                        gw = x2.saturating_sub(x1);
                        gh = y2.saturating_sub(y1);
                    }
                }
                if gw > 0 && gh > 0 {
                    let gutter = (gx, gy, gw, gh);
                    let was_in_sb = gui::in_rect(
                        state.last_mouse_x as usize,
                        state.last_mouse_y as usize,
                        gutter,
                    );
                    let in_sb = gui::in_rect(mx, my, gutter);
                    if was_in_sb != in_sb {
                        compositor
                            .dirty_tracker
                            .track(idx, crate::gui::dirty_rect::DirtyRect::new(gx, gy, gw, gh));
                    }
                }
                break;
            }

            if title_hover != state.last_titlebar_hover {
                // Eski ve yeni pencerenin başlık çubuğunu kirli işaretle;
                // aksi halde eski düğme vurgusu ekranda kalır.
                for (idx, _) in state.last_titlebar_hover.iter().copied().chain(title_hover) {
                    if let Some(w) = compositor.windows.get(idx).and_then(|slot| *slot) {
                        compositor.dirty_tracker.track(
                            idx,
                            crate::gui::dirty_rect::DirtyRect::new(
                                w.x,
                                w.y,
                                w.width,
                                gui::scaled_titlebar_h(),
                            ),
                        );
                    }
                }
                state.last_titlebar_hover = title_hover;
            }
        }

        // FPS 3D oyun fare bakışı (sadece pencere odaklıyken).
        // Fare `mouse` değişkeni ile toplandığı için önceki konum tur başında
        // güncellenmeli; aksi hâlde imleç pencere kenarına dayanınca bakış durur.
        // Mouse look only when FPS-3D window is focused; avoids unnecessary
        // redraws when another window has focus.
        if mouse_moved && app_is_focused(&compositor, 16) {
            let dx = mouse.x as f32 - state.last_mouse_x as f32;
            if dx.abs() > 0.0 {
                apps.fps_3d_app.as_mut().unwrap().handle_mouse_look(dx);
            }
            if apps.fps_3d_app.as_ref().is_some_and(|g| g.is_playing()) {
                mouse.x = (width as i32) / 2;
                mouse.y = (height as i32) / 2;
            }
        }
        // Delta hesaplandıktan sonra referans konumunu güncelle. Önceki sıra
        // odaklı pencerede delta'yı her zaman sıfırlıyordu; ayrıca odak dışı
        // hareketin sonraki FPS karesine taşınmasını engeller.
        if mouse_moved {
            state.last_mouse_x = mouse.x;
            state.last_mouse_y = mouse.y;
        }

        let buttons_changed = (mouse.left_button, mouse.right_button) != state.last_buttons;

        let v3d_dragging = mouse.left_button
            && apps
                .viewer3d_app
                .as_ref()
                .map(|a| a.drag_active)
                .unwrap_or(false);

        let is_dragging = state.dragging_win.is_some()
            || state.resizing_win.is_some()
            || state.vscroll_drag.is_some()
            || state.hscroll_drag.is_some()
            || state.notepad_dragging
            || state.browser_page_dragging
            || state.terminal_dragging
            || apps.filemanager_app.drag_active
            || v3d_dragging;

        let paint_active =
            (mouse.left_button || mouse.right_button) && app_is_focused(&compositor, 9);

        let is_interacting = is_dragging || paint_active;

        let viewer3d_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 12 && w.is_open && !w.is_minimized);

        let viewer3d_animating = viewer3d_open
            && (apps
                .viewer3d_app
                .as_ref()
                .map(|a| a.auto_rotate)
                .unwrap_or(false)
                || apps
                    .viewer3d_app
                    .as_ref()
                    .map(|a| a.drag_active)
                    .unwrap_or(false));

        let media_animating = (apps.musicplayer_app.is_playing && !audio_dev.is_muted)
            || apps
                .videoplayer_app
                .as_ref()
                .map(|a| a.is_playing)
                .unwrap_or(false)
            || viewer3d_animating;

        // FPS 3D oyunu açıkken ana döngü 60 Hz'e çıkar; oyunun kendi
        // güncelleme kapısı da buna göre ayarlanır. Şehir sürüşü de aynı
        // 16 ms'lik tavanı kullandığı için kendi pencere döngüsü de 60 Hz
        // çalışabilir.
        let fps3d_running = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 16 && w.is_open && !w.is_minimized);
        let city_running = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 17 && w.is_open && !w.is_minimized);

        let frame_interval_ms = crate::gui::frame::frame_interval_ms(
            is_interacting || viewer3d_animating || fps3d_running || city_running,
            media_animating,
            crate::gui::frame::vsync_enabled(),
        );

        let timer_tick_fired =
            monotonic_ms().wrapping_sub(state.last_frame_ms) >= frame_interval_ms;

        if timer_tick_fired {
            state.last_frame_ms = monotonic_ms();

            state.anim_tick = state.anim_tick.wrapping_add(1);

            if viewer3d_open {
                let _app_context = apps::guard::ContextScope::new(12, apps::guard::NO_WINDOW);
                apps.viewer3d_app.as_mut().unwrap().tick();
            }

            // Ana döngü yaşadıkça açık pencereleri canlı işaretle. APIC
            // zamanlayıcısı bu işaretleri kesme bağlamında denetler; burada
            // yalnızca ertelenmiş dondurma olayları güvenli şekilde tüketilir.
            for (idx, slot) in compositor.windows.iter().enumerate() {
                if let Some(win) = slot {
                    if win.is_open && !win.is_minimized {
                        watchdog::heartbeat(idx, win.app_id as usize);
                    }
                }
            }
            // Bellek baskısı yoklaması: PMM boş oranı eşik geçişlerinde
            // seri kayıt + kayıtlı geri çağrıyı tetikler.
            crate::kernel::heap::poll_pressure();

            let frozen = watchdog::drain_frozen();
            for fw in frozen.iter() {
                crate::serial_print("[WATCHDOG] Frozen app detected: app_id=");
                crate::serial_print_dec(fw.app_id as u64);
                crate::serial_print(" window_idx=");
                crate::serial_print_dec(fw.idx as u64);
                crate::serial_print(" idle=");
                crate::serial_print_dec(fw.idle_ms);
                crate::serial_print(" ms\n");
                // Watchdog yalnızca donmayı bildirir; zaman aşımına uğrayan
                // uygulama kullanıcı açıkça kapatana kadar çalışmaya devam eder.
                watchdog::acknowledge_frozen(fw.idx);
            }
        }

        // --- Kullanıcı modu GUI syscall isteklerini işle ---
        // Olay döngüsü, kullanıcı modu süreçlerinden gelen pencere oluşturma,
        // çizim ve girdi isteklerini tüketir ve kompozitörle eşler.
        event_syscall::drain_and_handle_gui_syscalls(&mut compositor);

        // Oyun motoru: dama yapay zekâsı, bulmaca gösterisi ve performans testi
        // yalnızca pencere açık ve zamanlayıcı işareti geldiğinde ilerler.
        // FPS limiti (30 FPS = 33ms) uygulanır.
        // Guard koruması: oyun içi hata/panic sistem donutmemesini sağlar,
        // yalnızca bu uygulama kapatılır.
        let game_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 15 && w.is_open && !w.is_minimized);
        if game_open && timer_tick_fired {
            let now_ms = crate::monotonic_ms();
            let time_since_last_tick = now_ms.saturating_sub(last_game_tick_ms);
            if time_since_last_tick >= GAME_FPS_LIMIT_MS {
                let (game_guard, game_start) =
                    apps::guard::prepare_recoverable(15, apps::guard::NO_WINDOW);
                if unsafe { apps::guard::begin(game_start) } {
                    let game_start_ms = crate::monotonic_ms();
                    let changed = apps.fps_game_app.as_mut().unwrap().tick();
                    let game_elapsed_ms = crate::monotonic_ms().saturating_sub(game_start_ms);
                    last_game_tick_ms = now_ms;
                    // Update activity timestamp for watchdog
                    let game_idx = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 15));
                    if let Some(idx) = game_idx {
                        watchdog::heartbeat(idx, 15);
                    }
                    if game_elapsed_ms > GAME_UPDATE_BUDGET_MS {
                        state.game_budget_strikes = state.game_budget_strikes.saturating_add(1);
                        if state.game_budget_strikes >= GAME_BUDGET_MAX_STRIKES {
                            crate::serial_print(
                                "[GAMES] Guard: oyun bütçeyi sürekli aşıyor, sonlandırılıyor\n",
                            );
                            if let Some(idx) = game_idx {
                                terminate_app(idx, &mut compositor, &mut apps);
                            }
                        } else {
                            crate::serial_print("[GAMES] Uyarı: güncelleme bütçesi aştı (");
                            crate::serial_print_dec(game_elapsed_ms);
                            crate::serial_print(" ms > ");
                            crate::serial_print_dec(GAME_UPDATE_BUDGET_MS);
                            crate::serial_print(" ms)\n");
                        }
                    } else {
                        state.game_budget_strikes = 0;
                    }
                    if changed {
                        state.force_render = true;
                    }
                } else {
                    // Guard recovery: oyun içinde hata oluştu, uygulama kapatılacak
                    crate::serial_print("[GAMES] Guard recovery: oyun hata verdi, kapatılıyor\n");
                    if let Some(idx) = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 15))
                    {
                        if let Some(fault) = apps::guard::take_pending() {
                            crate::serial_print("[GAMES] Hata türü: ");
                            crate::serial_print(fault.kind.as_str());
                            crate::serial_print("\n");
                        }
                        terminate_app(idx, &mut compositor, &mut apps);
                    }
                }
                drop(game_guard);
            }
        }

        // Kontrol paneli performans testi örneklemesi (Testler sekmesi).
        // Ölçüm çerçeve başına değil, iki zamanlayıcı işareti arasındaki süre
        // üzerinden yapılır; bu yüzden TSC yalnızca tick anında güncellenir.
        let controlpanel_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 14 && w.is_open && !w.is_minimized);
        if controlpanel_open && timer_tick_fired {
            let now_cycles = read_cpu_cycles();
            if apps::controlpanel::active_tab() == 3 {
                let _app_context = apps::guard::ContextScope::new(14, apps::guard::NO_WINDOW);
                let delta = now_cycles.wrapping_sub(state.cp_last_cycles);
                let delta_us = delta / unsafe { TSC_CYCLES_PER_US };
                let target_us = frame_interval_ms.saturating_mul(1_000);
                if apps::controlpanel::perf_sample(delta_us.min(1_000_000), target_us) {
                    state.force_render = true;
                }
            }
            state.cp_last_cycles = now_cycles;
        }

        // FPS 3D oyun motoru: raycasting, hareket, düşman AI, ses.
        // FPS limiti (30 FPS = 33ms) uygulanır.
        // Guard koruması: oyun içi hata/panic sistem donutmemesini sağlar,
        // yalnızca bu uygulama kapatılır.
        let fps3d_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 16 && w.is_open && !w.is_minimized);
        if fps3d_open && timer_tick_fired {
            let now_ms = crate::monotonic_ms();
            let time_since_last_tick = now_ms.saturating_sub(last_fps3d_tick_ms);
            if time_since_last_tick >= FPS3D_FPS_LIMIT_MS {
                let (fps3d_guard, fps3d_start) =
                    apps::guard::prepare_recoverable(16, apps::guard::NO_WINDOW);
                if unsafe { apps::guard::begin(fps3d_start) } {
                    let fps3d_start_ms = crate::monotonic_ms();
                    if app_is_focused(&compositor, 16) {
                        apps.fps_3d_app.as_mut().unwrap().sync_held_keys(keyboard);
                    }
                    let fps3d_completed = apps
                        .fps_3d_app
                        .as_mut()
                        .unwrap()
                        .update_with_budget(&mut audio_dev, GAME_UPDATE_BUDGET_MS);
                    let fps3d_elapsed_ms = crate::monotonic_ms().saturating_sub(fps3d_start_ms);
                    last_fps3d_tick_ms = now_ms;
                    if !fps3d_completed {
                        crate::serial_print(
                            "[FPS-3D] Uyarı: güncelleme bütçesi aştı, kare atlandı\n",
                        );
                    }
                    // Update activity timestamp for watchdog
                    let fps3d_idx = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 16));
                    if let Some(idx) = fps3d_idx {
                        watchdog::heartbeat(idx, 16);
                    }
                    if fps3d_elapsed_ms > GAME_UPDATE_BUDGET_MS {
                        state.fps3d_budget_strikes = state.fps3d_budget_strikes.saturating_add(1);
                        if state.fps3d_budget_strikes >= GAME_BUDGET_MAX_STRIKES {
                            crate::serial_print(
                                "[FPS-3D] Guard: oyun bütçeyi sürekli aşıyor, sonlandırılıyor\n",
                            );
                            if let Some(idx) = fps3d_idx {
                                terminate_app(idx, &mut compositor, &mut apps);
                            }
                        } else {
                            crate::serial_print("[FPS-3D] Uyarı: güncelleme bütçesi aştı (");
                            crate::serial_print_dec(fps3d_elapsed_ms);
                            crate::serial_print(" ms > ");
                            crate::serial_print_dec(GAME_UPDATE_BUDGET_MS);
                            crate::serial_print(" ms)\n");
                        }
                    } else {
                        state.fps3d_budget_strikes = 0;
                    }
                } else {
                    // Guard recovery: oyun içinde hata oluştu, uygulama kapatılacak
                    crate::serial_print("[FPS-3D] Guard recovery: oyun hata verdi, kapatılıyor\n");
                    if let Some(idx) = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 16))
                    {
                        if let Some(fault) = apps::guard::take_pending() {
                            crate::serial_print("[FPS-3D] Hata türü: ");
                            crate::serial_print(fault.kind.as_str());
                            crate::serial_print("\n");
                        }
                        terminate_app(idx, &mut compositor, &mut apps);
                    }
                }
                drop(fps3d_guard);
            }
            // Tam ekran yerine yalnızca oyun penceresi bölgesi kirli
            // işaretlenir; kirli-dikdörtgen çizimi masaüstünü atlar.
            track_app_window_dirty(&mut compositor, 16);
        }

        // Şehir sürüşü: araç fiziği, trafik hareketi ve motor sesi.
        // Aynı 30 FPS sınırı ve guard koruması uygulanır.
        let city_open = city_running;
        if city_open && timer_tick_fired {
            let now_ms = crate::monotonic_ms();
            let time_since_last_tick = now_ms.saturating_sub(last_city_tick_ms);
            if time_since_last_tick >= FPS3D_FPS_LIMIT_MS {
                let (city_guard, city_start) =
                    apps::guard::prepare_recoverable(17, apps::guard::NO_WINDOW);
                if unsafe { apps::guard::begin(city_start) } {
                    let dt_ms = time_since_last_tick.min(200);
                    apps.city_drive_app
                        .as_mut()
                        .unwrap()
                        .update(dt_ms, keyboard);
                    apps.city_drive_app
                        .as_mut()
                        .unwrap()
                        .play_engine_sound(&mut audio_dev);
                    last_city_tick_ms = now_ms;
                    if let Some(idx) = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 17))
                    {
                        watchdog::heartbeat(idx, 17);
                    }
                    track_app_window_dirty(&mut compositor, 17);
                } else {
                    crate::serial_print("[CITY] Guard recovery: oyun hata verdi, kapatılıyor\n");
                    if let Some(idx) = compositor
                        .windows
                        .iter()
                        .position(|w| w.is_some_and(|w| w.app_id == 17))
                    {
                        terminate_app(idx, &mut compositor, &mut apps);
                    }
                }
                drop(city_guard);
            }
        }

        // Ag kartindan gelen paketleri isle (ARP cevaplari, ICMP vs.).

        drivers::e1000::poll();

        // Run one short cooperative task step per event-loop turn. The
        // scheduler budget is one callback; a callback itself is not preempted.
        task::poll_tasks();

        // Kullanıcı süreçleri: hazır bir Ring3 süreci varsa bir zaman dilimi
        // çalıştır (yoksa hızlı döner). APIC timer önalımı ve SYS_YIELD ile
        // dilimler arasında çekirdeğe dönülür.
        arch::x86_64::ring3::schedule_user_processes();

        {
            let _browser_context = apps::guard::ContextScope::new(3, apps::guard::NO_WINDOW);

            apps.browser_app.poll_network(&mut fs_mgr.vfs);

            // Tarayıcı medya oynatıcılarının ses çıkışını render et.
            apps.browser_app.render_audio(&mut audio_dev);
        }

        // Paket Yöneticisi uzak indirmesi: tarayıcıyla aynı şekilde kare kare
        // ilerler; tek-bağlantılı TCP yolunu sahiplik (lease) ile korur.
        // Uygulama henüz yüklenmemişse (None) bu kareyi atla; .unwrap() paniğine
        // yol açardı ve masaüstü donmasına neden oluyordu.
        if let Some(pkgmgr) = apps.pkgmgr_app.as_mut() {
            let _pkg_context = apps::guard::ContextScope::new(13, apps::guard::NO_WINDOW);
            pkgmgr.poll_download(&mut fs_mgr.vfs);
        }

        // Bes parcali, HDA PCM tabanli melodi yurutucusu. (HDA yoksa PIT/PC speaker fallback).

        audio_dev.set_volume(apps.settings_app.volume);

        audio_dev.is_muted = apps.settings_app.is_muted;

        apps.musicplayer_app.volume = apps.settings_app.volume as usize;

        // Oyun ses efektleri: uygulama yalnızca istek bırakır, ton burada çalınır.
        // HDA DMA aktifse kısa bir PCM darbesi gönderilir; aksi halde PC Speaker yedeği.

        // no_std için hızlı sinüs (Taylor serisi).
        fn fast_sin(mut x: f32) -> f32 {
            while x > core::f32::consts::PI {
                x -= core::f32::consts::TAU;
            }
            while x < -core::f32::consts::PI {
                x += core::f32::consts::TAU;
            }
            let x2 = x * x;
            let x3 = x * x2;
            let x5 = x3 * x2;
            let x7 = x5 * x2;
            x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
        }

        state.game_perf_us = state.game_perf_us.wrapping_add(1);
        if game_open {
            if let Some((freq, duration_ms)) =
                apps::game::GameApp::sound_tone(apps.fps_game_app.as_mut().unwrap().take_sound())
            {
                if !audio_dev.is_muted && audio_dev.volume_left > 0 {
                    if audio_dev.is_hda_active() {
                        let frames = (48u32 * duration_ms.max(1)).min(4_800) as usize;
                        let mut pcm = [0i16; 512];
                        let chunks = frames.div_ceil(256);
                        for chunk in 0..chunks {
                            let count = (frames - chunk * 256).min(256);
                            for k in 0..count {
                                let angle = core::f32::consts::TAU
                                    * freq as f32
                                    * ((chunk * 256 + k) as f32 / 48_000.0);
                                let sample = fast_sin(angle) as i16;
                                pcm[k * 2] = sample;
                                pcm[k * 2 + 1] = sample;
                            }
                            drivers::hda::push_pcm(&pcm[..count * 2]);
                        }
                    } else {
                        audio_dev.set_pc_speaker_tone(freq);
                        state.game_sound_until_ms =
                            crate::monotonic_ms().saturating_add(duration_ms as u64);
                    }
                    state.force_render = true;
                }
            }
        }

        // 3-Masa oyunu PC Speaker sesi süresi dolduysa otomatik olarak sustur.
        if state.game_sound_until_ms > 0 && crate::monotonic_ms() >= state.game_sound_until_ms {
            audio_dev.set_pc_speaker_tone(0);
            state.game_sound_until_ms = 0;
        }

        // FPS 3D oyun ses efektleri.
        if fps3d_open {
            if let Some((freq, _duration)) = apps::fps_game::FpsGameApp::sound_tone(
                apps.fps_3d_app.as_mut().unwrap().take_sound(),
            ) {
                if !audio_dev.is_muted && audio_dev.volume_left > 0 && freq > 0 {
                    audio_dev.set_pc_speaker_tone(freq);
                    state.force_render = true;
                }
            }
        }

        // Video kare sayaci gercek zamana bagli (~30 FPS) ve yalnizca oynatirken ilerler.
        let videoplayer_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 6 && w.is_open && !w.is_minimized);
        if videoplayer_open {
            if apps.videoplayer_app.as_mut().unwrap().is_playing {
                let video_now_ms = monotonic_ms();

                if video_now_ms.wrapping_sub(state.last_video_frame_ms) >= 33 {
                    state.last_video_frame_ms = video_now_ms;

                    apps.videoplayer_app.as_mut().unwrap().frame =
                        apps.videoplayer_app.as_mut().unwrap().frame.wrapping_add(1);
                }
            }
        } else {
            state.last_video_frame_ms = monotonic_ms();
        }

        if apps.musicplayer_app.is_playing && !audio_dev.is_muted && audio_dev.volume_left > 0 {
            if state.anim_tick.wrapping_sub(state.last_music_beat) >= 24 {
                // Her liste öğesi ayrı bir melodi kalıbına sahip olsun. Sadece
                // frekansı biraz kaydırmak parçaların aynı duyulmasına neden
                // oluyordu; artık ritmik/notsal dizileri de birbirinden farklı.
                const TRACK_NOTES: [[u32; 8]; 5] = [
                    [440, 523, 587, 659, 587, 523, 494, 392],
                    [330, 392, 440, 494, 523, 494, 440, 392],
                    [262, 330, 392, 523, 392, 330, 294, 262],
                    [523, 659, 784, 659, 587, 494, 587, 659],
                    [392, 494, 587, 784, 659, 587, 494, 392],
                ];

                let track = apps
                    .musicplayer_app
                    .current_track
                    .min(TRACK_NOTES.len() - 1);
                let freq = TRACK_NOTES[track][state.music_note % TRACK_NOTES[track].len()];

                // HDA hazir ve DMA ilerliyorsa gercek PCM ornegi uret ve gonder.

                if audio_dev.is_hda_active() {
                    if !apps.musicplayer_app.wav_data.is_empty() {
                        let mut pcm = [0i16; 512];
                        for frame in 0..256 {
                            let sample =
                                apps.musicplayer_app.wav_data[apps.musicplayer_app.wav_position
                                    % apps.musicplayer_app.wav_data.len()];
                            pcm[frame * 2] = sample;
                            pcm[frame * 2 + 1] = sample;
                            apps.musicplayer_app.wav_position += 1;
                        }
                        drivers::hda::push_pcm(&pcm);
                        if apps.musicplayer_app.wav_position >= apps.musicplayer_app.wav_data.len()
                        {
                            apps.musicplayer_app.wav_position = 0;
                            apps.musicplayer_app.is_playing = false;
                        }
                        state.last_music_beat = state.anim_tick;
                        continue;
                    }
                    let sample_count = 1920; // 40 ms

                    let mut pcm: [i16; 512] = [0; 512]; // 256 L + 256 R cifti

                    let volume_scale = (audio_dev.volume_left as i32).min(100) as i16;

                    let mut idx = 0;

                    while idx < sample_count {
                        let n = (sample_count - idx).min(pcm.len() / 2);

                        for k in 0..n {
                            let t = (idx + k) as f32 / 48_000.0;

                            let angle = core::f32::consts::TAU * freq as f32 * t;

                            let x = angle
                                - core::f32::consts::TAU
                                    * ((angle / core::f32::consts::TAU) as i32) as f32;

                            let x2 = x * x;

                            let v =
                                x - x2 * x / 6.0 + x2 * x2 * x / 120.0 - x2 * x2 * x2 * x / 5040.0;

                            let sample = (v * 0.3 * volume_scale as f32 / 100.0 * 32767.0) as i16;

                            pcm[k * 2] = sample;

                            pcm[k * 2 + 1] = sample;
                        }

                        drivers::hda::push_pcm(&pcm[..n * 2]);

                        idx += n;
                    }

                    if state.speaker_on {
                        audio_dev.set_pc_speaker_tone(0);

                        state.speaker_on = false;
                    }
                } else {
                    // Otomatik Fallback: HDA yok veya sanallastirma ortaminda DMA duraksadiysa PC Speaker PWM tonlama yap!

                    audio_dev.set_pc_speaker_tone(freq);

                    state.speaker_on = true;
                }

                state.music_note = (state.music_note + 1) % TRACK_NOTES[track].len();

                state.last_music_beat = state.anim_tick;
            }

            // Parca 3:30 (210 sn) oldugu icin cubuk gercek zamanda her 2.1 sn'de %1 ilerler.
            let progress_now_ms = monotonic_ms();
            if progress_now_ms.wrapping_sub(state.last_music_progress_ms) >= 2_100 {
                state.last_music_progress_ms = progress_now_ms;
                apps.musicplayer_app.progress = (apps.musicplayer_app.progress + 1) % 101;
            }
        } else if state.speaker_on || audio_dev.is_muted || audio_dev.volume_left == 0 {
            drivers::hda::silence();

            audio_dev.set_pc_speaker_tone(0);

            state.speaker_on = false;

            state.last_music_beat = state.anim_tick;

            state.last_music_progress_ms = monotonic_ms();
        }

        event_render::render_frame(
            &mut state,
            &mut fb,
            &mut compositor,
            &mut apps.filemanager_app,
            &mut apps.desktop_files,
            &mut apps.settings_app,
            &mut apps.notepad_app,
            &mut apps.browser_app,
            &mut apps.imageviewer_app,
            &mut apps.musicplayer_app,
            &mut apps.videoplayer_app,
            &mut apps.taskmgr_app,
            &mut apps.calculator_app,
            &mut apps.paint_apps,
            &mut apps.terminal_app,
            &mut apps.installer_app,
            &mut apps.viewer3d_app,
            &mut apps.pkgmgr_app,
            &mut apps.fps_game_app,
            &mut apps.fps_3d_app,
            &mut apps.city_drive_app,
            &now,
            mouse,
            keyboard,
            memory_mgr,
            &mut fs_mgr,
            &input_update,
            timer_tick_fired,
            mouse_moved,
            buttons_changed,
            is_interacting,
            media_animating,
            scroll_val as isize,
            mx,
            my,
            width,
            height,
        );
    }
}

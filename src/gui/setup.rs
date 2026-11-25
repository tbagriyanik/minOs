use crate::apps;
use crate::drivers;
use crate::fs;
use crate::gui;
use crate::gui::metrics::scaled_taskbar_h;

/// Holds all application instances for the GUI.
///
/// Yerleşik (shell) uygulamalar — dosya yöneticisi, masaüstü dosyaları ve
/// ayarlar — her zaman bellekte kalır; diğer kullanıcı uygulamaları yalnızca
/// tıklanıp açıldığında yüklenir ve kapanınca bellekten düşürülür
/// (`Option<App>`: `None` = bellekte yok).
pub struct AppInstances {
    /// Kabuk uygulamaları: her zaman yüklü.
    pub filemanager_app: apps::filemanager::FileManagerApp,
    pub desktop_files: apps::filemanager::FileManagerApp,
    pub settings_app: apps::settings::SystemSettings,
    /// Kullanıcı uygulamaları: `None` bellekte yok demektir.
    pub notepad_app: Option<apps::notepad::NotepadApp>,
    pub browser_app: apps::browser::BrowserApp,
    pub imageviewer_app: Option<apps::imageviewer::ImageViewerApp>,
    pub musicplayer_app: apps::musicplayer::MusicPlayerApp,
    pub videoplayer_app: Option<apps::videoplayer::VideoPlayerApp>,
    pub taskmgr_app: apps::taskmgr::TaskManagerApp,
    pub calculator_app: Option<apps::calculator::CalculatorApp>,
    pub paint_apps: alloc::vec::Vec<Option<apps::paint::PaintApp>>,
    pub terminal_app: Option<apps::terminal::TerminalApp>,
    pub installer_app: Option<apps::installer::InstallerApp>,
    pub viewer3d_app: Option<apps::viewer3d::Viewer3DApp>,
    pub pkgmgr_app: Option<apps::pkgmgr::PackageManagerApp>,
    pub fps_game_app: Option<apps::game::GameApp>,
    pub fps_3d_app: Option<apps::fps_game::FpsGameApp>,
    pub city_drive_app: Option<apps::city_drive::CityDriveApp>,
}

impl AppInstances {
    /// Uygulama kimliğine göre örneğin yüklü olup olmadığını döner.
    /// Kabuk uygulamaları (0, 1, 3, 5, 7) ve kontrol paneli (14, durum global)
    /// her zaman yüklü sayılır; boya pencereleri (9) ayrı `paint_apps` dizisinde
    /// tutulur.
    pub fn is_loaded(&self, app_id: usize) -> bool {
        match app_id {
            0 | 1 | 3 | 5 | 7 | 14 => true,
            2 => self.notepad_app.is_some(),
            4 => self.imageviewer_app.is_some(),
            6 => self.videoplayer_app.is_some(),
            8 => self.calculator_app.is_some(),
            10 => self.terminal_app.is_some(),
            11 => self.installer_app.is_some(),
            12 => self.viewer3d_app.is_some(),
            13 => self.pkgmgr_app.is_some(),
            15 => self.fps_game_app.is_some(),
            16 => self.fps_3d_app.is_some(),
            17 => self.city_drive_app.is_some(),
            _ => false,
        }
    }

    /// Uygulamayı ilk kez belleğe yükler (constructor çağrısı). Zaten yüklüyse
    /// hiçbir şey yapmaz. VFS gerektiren başlatma (paket listesi, sürücüler,
    /// imişaretleri) burada yapılır.
    pub fn load(&mut self, app_id: usize, vfs: &crate::fs::vfs::VirtualFileSystem) {
        if self.is_loaded(app_id) {
            return;
        }
        match app_id {
            2 => self.notepad_app = Some(apps::notepad::NotepadApp::new()),
            4 => self.imageviewer_app = Some(apps::imageviewer::ImageViewerApp::new()),
            6 => self.videoplayer_app = Some(apps::videoplayer::VideoPlayerApp::new()),
            8 => self.calculator_app = Some(apps::calculator::CalculatorApp::new()),
            10 => {
                let mut terminal = apps::terminal::TerminalApp::new();
                terminal.init_banner();
                self.terminal_app = Some(terminal);
            }
            11 => {
                let mut installer = apps::installer::InstallerApp::new();
                installer.refresh_drives();
                self.installer_app = Some(installer);
            }
            12 => self.viewer3d_app = Some(apps::viewer3d::Viewer3DApp::new()),
            13 => {
                let mut pkgmgr = apps::pkgmgr::PackageManagerApp::new();
                pkgmgr.refresh_installed(vfs);
                self.pkgmgr_app = Some(pkgmgr);
            }
            15 => self.fps_game_app = Some(apps::game::GameApp::new()),
            16 => self.fps_3d_app = Some(apps::fps_game::FpsGameApp::new()),
            17 => self.city_drive_app = Some(apps::city_drive::CityDriveApp::new()),
            _ => {}
        }
    }

    /// Uygulama örneğini bellekten düşür (serbest bırak). Son pencere kapatıldığında
    /// çağrılır; kabuk uygulamaları için hiçbir şey yapmaz.
    pub fn unload(&mut self, app_id: usize) {
        match app_id {
            2 => self.notepad_app = None,
            4 => self.imageviewer_app = None,
            6 => self.videoplayer_app = None,
            8 => self.calculator_app = None,
            10 => self.terminal_app = None,
            11 => self.installer_app = None,
            12 => self.viewer3d_app = None,
            13 => self.pkgmgr_app = None,
            15 => self.fps_game_app = None,
            16 => self.fps_3d_app = None,
            17 => self.city_drive_app = None,
            _ => {}
        }
    }
}

/// Kurulum çıktısı: tüm uygulama örnekleri ve pencere yöneticisi.
///
/// `Box` ile kurulur: iki yapı birlikte ~110 KiB tutar ve yığın çerçevesinde
/// taşınması çekirdek yığınını zorlar.
pub struct AppSetup {
    pub apps: AppInstances,
    pub compositor: gui::MinGuiCompositor,
}

/// Initializes all applications and registers them with the compositor.
///
/// Dönüş değeri `Box<AppSetup>`'dur: `AppInstances` ve `MinGuiCompositor`
/// birlikte ~110 KiB yer kaplar; yığın çerçevesinde birleştirilirse çekirdek
/// yığınını zorlar.
pub fn init_apps(
    fs_mgr: &mut fs::FileSystemManager,
    width: usize,
    height: usize,
) -> alloc::boxed::Box<AppSetup> {
    // Yerinde kurulum: yapılar doğrudan ayrılan (heap) bellekte oluşturulur,
    // böylece ne dönüş değeri ne de ara adımlar ~110 KiB'lık yığın çerçevesi
    // gerektirir.
    let mut setup = alloc::boxed::Box::new(AppSetup {
        apps: AppInstances {
            filemanager_app: apps::filemanager::FileManagerApp::new(),
            desktop_files: apps::filemanager::FileManagerApp::new(),
            settings_app: apps::settings::SystemSettings::default(),
            notepad_app: None,
            browser_app: apps::browser::BrowserApp::new(),
            imageviewer_app: None,
            musicplayer_app: apps::musicplayer::MusicPlayerApp::new(),
            videoplayer_app: None,
            taskmgr_app: apps::taskmgr::TaskManagerApp::new(),
            calculator_app: None,
            paint_apps: (0..gui::MAX_WINDOWS).map(|_| None).collect(),
            terminal_app: None,
            installer_app: None,
            viewer3d_app: None,
            pkgmgr_app: None,
            fps_game_app: None,
            fps_3d_app: None,
            city_drive_app: None,
        },
        compositor: gui::MinGuiCompositor::new(),
    });
    let compositor = &mut setup.compositor;
    let apps_all = &mut setup.apps;

    // Yerleşik uygulamaları kayıt defterine ekle; `/bin` taraması
    // `sync_installed_apps` içinde harici/hot-load girdilerini ekler.
    for (id, name) in [
        (0u32, "Dosyalar"),
        (1, "Ayarlar"),
        (2, "Notlar"),
        (3, "Web"),
        (4, "Resim"),
        (5, "Muzik"),
        (6, "Video"),
        (7, "Gorevler"),
        (8, "Hesap"),
        (9, "cizim"),
        (10, "Komut"),
        (11, "Kurulum"),
        (12, "3D"),
        (13, "Paketler"),
        (14, "Kontrol"),
        (15, "Oyun"),
        (16, "FPS3D"),
        (17, "Sehir"),
    ] {
        compositor.app_registry.register_builtin(id, name);
    }

    apps_all.filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

    apps_all.desktop_files.set_path("/desktop");
    apps_all.desktop_files.refresh_from_vfs(&fs_mgr.vfs);

    let settings_app = &mut apps_all.settings_app;
    settings_app.display_res = (width, height);
    settings_app.screen_res = (width, height);

    // Load persistent settings
    if fs::diskfs::load_file("settings.cfg") > 0 {
        let cfg_bytes = fs::diskfs::content();
        let _ = fs_mgr.vfs.write_file("settings.cfg", cfg_bytes);
    }
    settings_app.load_from_vfs(&fs_mgr.vfs);

    let _ = apps_all.browser_app.load_bookmarks_from_vfs(&fs_mgr.vfs);

    // terminal/installer/pkgmgr başlatımı (banner, sürücüler, paket listesi)
    // artık ilk açılışta `AppInstances::load` ile yapılır; bellekte tutulmazlar.
    compositor.sync_installed_apps(&fs_mgr.vfs);

    setup
}

/// Creates and registers all default application windows.
pub fn register_default_windows(
    compositor: &mut gui::MinGuiCompositor,
    width: usize,
    height: usize,
) {
    // 0: File Manager
    let files_width = 720.min(width.saturating_sub(20)).max(200);
    let files_height = 500
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "Dosya Yoneticisi",
        "Dosyalar",
        width.saturating_sub(files_width) / 2,
        height.saturating_sub(files_height + scaled_taskbar_h()) / 2,
        files_width,
        files_height,
        false,
    ));

    // 1: Settings
    let settings_width = 700.min(width.saturating_sub(20)).max(200);
    let settings_height = 540
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "Sistem Ayarlari",
        "Ayarlar",
        width.saturating_sub(settings_width) / 2,
        height.saturating_sub(settings_height + scaled_taskbar_h()) / 2,
        settings_width,
        settings_height,
        false,
    ));

    // 2: Notepad
    compositor.add_window(gui::Window::new(
        "Not Defteri",
        "Notlar",
        80,
        70,
        520,
        330,
        false,
    ));

    // 3: Browser
    let browser_w = 760.min(width.saturating_sub(20)).max(300);
    let browser_h = 490
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(200);
    compositor.add_window(gui::Window::new(
        "Web Gezgini",
        "Web",
        width.saturating_sub(browser_w) / 2,
        height.saturating_sub(browser_h + scaled_taskbar_h()) / 2,
        browser_w,
        browser_h,
        false,
    ));

    // 4: Image Viewer
    compositor.add_window(gui::Window::new(
        "Resim Gosterici",
        "Resim",
        120,
        90,
        500,
        330,
        false,
    ));

    // 5: Music Player
    compositor.add_window(gui::Window::new(
        "Muzik calar",
        "Muzik",
        560,
        80,
        460,
        245,
        false,
    ));

    // 6: Video Player
    compositor.add_window(gui::Window::new(
        "Video Oynatici",
        "Video",
        150,
        110,
        540,
        330,
        false,
    ));

    // 7: Task Manager
    compositor.add_window(gui::Window::new(
        "Gorev Yoneticisi",
        "Gorevler",
        220,
        60,
        480,
        460,
        false,
    ));

    // 8: Calculator
    compositor.add_window(gui::Window::new(
        "Hesap Makinesi",
        "Hesap",
        260,
        120,
        360,
        340,
        false,
    ));

    // 9: Paint — iki şeritli araç çubuğu için geniş bir varsayılan boyut;
    // ekranı aşmayacak şekilde kelepçelenir.
    let paint_w = 680.min(width.saturating_sub(20)).max(360);
    let paint_h = 480
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(240);
    let mut paint_win = gui::Window::new(
        "cizim Tahtasi",
        "cizim",
        width.saturating_sub(paint_w) / 2,
        height.saturating_sub(paint_h + scaled_taskbar_h()) / 2,
        paint_w,
        paint_h,
        false,
    );
    // İki şeritli araç çubuğu küçük pencerede üst üste binmesin diye
    // yeniden boyutlandırma alt sınırı ölçekli taban birimle verilir.
    paint_win.min_width = gui::metrics::sp(580);
    paint_win.min_height = gui::metrics::sp(280);
    compositor.add_window(paint_win);

    // 10: Terminal
    compositor.add_window(gui::Window::new(
        "Komut Istemi",
        "Komut",
        220,
        100,
        560,
        300,
        false,
    ));

    // 11: Installer
    let inst_w = 540.min(width.saturating_sub(20)).max(200);
    let inst_h = 460
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "minOs Tek Tikla Kur",
        "Kurulum",
        width.saturating_sub(inst_w) / 2,
        height.saturating_sub(inst_h + scaled_taskbar_h()) / 2,
        inst_w,
        inst_h,
        false,
    ));

    // 12: 3D Viewer
    let v3d_w = 600.min(width.saturating_sub(20)).max(200);
    let v3d_h = 440
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "3D Gosterici",
        "3D",
        width.saturating_sub(v3d_w) / 2,
        height.saturating_sub(v3d_h + scaled_taskbar_h()) / 2,
        v3d_w,
        v3d_h,
        false,
    ));

    // 13: Package Manager
    let pkg_w = 620.min(width.saturating_sub(20)).max(200);
    let pkg_h = 440
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "Paket Yoneticisi",
        "Paketler",
        width.saturating_sub(pkg_w) / 2,
        height.saturating_sub(pkg_h + scaled_taskbar_h()) / 2,
        pkg_w,
        pkg_h,
        false,
    ));

    // 14: Control Panel
    let cp_w = 620.min(width.saturating_sub(20)).max(200);
    let cp_h = 560
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(120);
    compositor.add_window(gui::Window::new(
        "Kontrol Paneli",
        "Kontrol",
        width.saturating_sub(cp_w) / 2,
        height.saturating_sub(cp_h + scaled_taskbar_h()) / 2,
        cp_w,
        cp_h,
        false,
    ));

    // 15: Games
    let fps_w = 700.min(width.saturating_sub(20)).max(300);
    let fps_h = 540
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(200);
    compositor.add_window(gui::Window::new(
        "Oyunlar",
        "Oyun",
        width.saturating_sub(fps_w) / 2,
        height.saturating_sub(fps_h + scaled_taskbar_h()) / 2,
        fps_w,
        fps_h,
        false,
    ));

    // 16: FPS 3D Game
    let fps3d_w = 700.min(width.saturating_sub(20)).max(300);
    let fps3d_h = 540
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(200);
    compositor.add_window(gui::Window::new(
        "FPS 3D Oyun",
        "FPS3D",
        width.saturating_sub(fps3d_w) / 2,
        height.saturating_sub(fps3d_h + scaled_taskbar_h()) / 2,
        fps3d_w,
        fps3d_h,
        false,
    ));

    // 17: City Drive (3D sehir surusu)
    let city_w = 760.min(width.saturating_sub(20)).max(320);
    let city_h = 520
        .min(height.saturating_sub(scaled_taskbar_h() + 10))
        .max(220);
    compositor.add_window(gui::Window::new(
        "Sehir Surusu",
        "Sehir",
        width.saturating_sub(city_w) / 2,
        height.saturating_sub(city_h + scaled_taskbar_h()) / 2,
        city_w,
        city_h,
        false,
    ));

    // Start with clean desktop - no active window
    compositor.active_window_idx = None;
}

/// Applies user settings to the GUI and devices.
pub fn apply_user_settings(
    settings: &apps::settings::SystemSettings,
    _fb: &drivers::video::Framebuffer,
    mouse: &mut drivers::mouse::MouseState,
    keyboard: &mut drivers::keyboard::KeyboardState,
    _width: usize,
    _height: usize,
) {
    crate::gui::wallpaper::apply(settings.wallpaper, settings.wall_color);
    mouse.speed = settings.mouse_speed;
    mouse.reverse_scroll = settings.reverse_scroll;
    crate::gui::anim::set_reduced_motion(settings.reduced_motion);
    crate::gui::set_transparent_windows(settings.transparent_windows);
    keyboard.layout = settings.keyboard_layout;
    // Otomatik gundus/gece temasini uygula (ayar/etkin); pasifse manuel tema.
    crate::gui::theme::set_auto_theme(settings.auto_theme);
    if settings.auto_theme {
        let now = drivers::rtc::DateTime::read_for_region(settings.region_offset());
        let effective = crate::gui::theme::sync_auto_theme_at(now.hour, now.minute);
        if effective != crate::gui::theme::current() {
            crate::gui::theme::set_theme(effective);
        }
    } else {
        crate::gui::theme::set_theme(settings.theme);
    }

    // Apply resolution from boot command line if set
    if let Some(_cmd_res) = crate::boot::limine::cmdline_resolution() {
        // Already applied earlier in boot
    }
}

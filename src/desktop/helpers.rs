//! Masaustu ve pencere yardimcilari: uygulama acma/kapama, masaustu simge geometrisi,
//! kaydirma konumlandirma ve takvim gezinme.

use crate::{apps, drivers, fs, gui, mm};

/// Pencereye bagli calisan surec adlari (gorev yoneticisi surec tablosu icin).
pub fn app_process_name(app_id: usize) -> &'static str {
    match app_id {
        0 => "dosya_yoneticisi",
        1 => "sistem_ayarlari",
        2 => "not_defteri",
        3 => "web_gezgini",
        4 => "resim_gosterici",
        5 => "muzik_calar",
        6 => "video_oynatici",
        7 => "gorev_yoneticisi",
        8 => "hesap_makinesi",
        9 => "cizim_tahtasi",
        10 => "komut_istemi",
        11 => "kurulum_sihirbazi",
        12 => "3d_gosterici",
        13 => "paket_yoneticisi",
        14 => "kontrol_paneli",
        15 => "oyunlar",
        16 => "fps_3d_oyun",
        17 => "sehir_surusu",
        _ => "uygulama",
    }
}

/// Bir pencere acildiginda ona karsilik gelen surec kaydini olusturur.
fn ensure_window_process(idx: usize, app_id: usize) {
    unsafe {
        let mgr = &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR);
        let exists = mgr.proc_mgr.processes.iter().any(|p| {
            p.state != mm::process::ProcessState::Unused
                && p.state != mm::process::ProcessState::Terminated
                && p.resources.window_idx == Some(idx)
        });
        if !exists {
            let _ = mgr.proc_mgr.spawn(
                app_process_name(app_id),
                0,
                0,
                app_id,
                Some(idx),
                &mut mgr.pmm,
            );
        }
    }
}

/// Bir pencereye bagli sureci sonlandirir ve kaynaklarini serbest birakir.
pub fn terminate_window_process(idx: usize) -> bool {
    unsafe {
        let mgr = &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR);
        // The task manager must use the process table's kill path.  Closing a
        // GUI slot is only presentation state; it must never be the thing
        // that defines whether the process is alive.
        mgr.proc_mgr.terminate_by_window(idx, &mut mgr.pmm)
    }
}

/// Uygulama başlatma isteğinde yeni örnek açar; tekil uygulamalar var olan pencereye gider.
pub fn open_app(compositor: &mut gui::MinGuiCompositor, app_id: usize) {
    // Sürücü hatası: ilgili uygulama güvenli biçimde pasifleşir; kullanıcıya
    // nedeni toast ile bildirilir (ör. ağ/sürücü hatası → tarayıcı açılmaz).
    if let Some(reason) = crate::drivers::status::blocked_reason(app_id, crate::gui::ui_english()) {
        crate::boot::console::serial_print("[APP-BLOCK] app=");
        crate::boot::console::serial_print_dec(app_id as u64);
        crate::boot::console::serial_print(" reason=");
        crate::boot::console::serial_print(reason);
        crate::boot::console::serial_print("\n");
        compositor.push_toast(
            gui::ToastKind::Warning,
            gui::ui_text("Sürücü Hatası", "Driver Error"),
            reason,
            crate::boot::time::monotonic_ms(),
            5000,
        );
        return;
    }
    if let Some(idx) = compositor.launch_app_window(app_id) {
        if let Some(win) = compositor.windows[idx].as_mut() {
            win.is_open = true;
            win.is_minimized = false;
            win.is_maximized = false;
        }
        ensure_window_process(idx, app_id);
        compositor.focus_window(idx);

        // Klavye kısayolları toast bildirimi göster
        show_app_shortcuts_toast(compositor, app_id);

        if app_id == 16 {
            crate::boot::console::serial_print("[FPS3D] Uygulama açıldı / Application opened\n");
        }
        if app_id == 17 {
            crate::boot::console::serial_print("[CITY] Uygulama açıldı / Application opened\n");
        }
    }
}

/// Uygulama açıldığında temel klavye kısayollarını toast olarak gösterir.
fn show_app_shortcuts_toast(compositor: &mut gui::MinGuiCompositor, app_id: usize) {
    let (title, message) = match app_id {
        0 => (
            // Dosya Yöneticisi
            gui::ui_text("Dosya Yöneticisi", "File Manager"),
            gui::ui_text(
                "F2: Yeniden Adlandır | F5: Yenile | Del: Sil | Ctrl+N: Yeni | Enter: Aç",
                "F2: Rename | F5: Refresh | Del: Delete | Ctrl+N: New | Enter: Open",
            ),
        ),
        1 => (
            // Sistem Ayarları
            gui::ui_text("Sistem Ayarları", "System Settings"),
            gui::ui_text(
                "Tab: Sekme Değiştir | Sol/Sağ: Değer Değiştir | Enter: Onayla",
                "Tab: Switch Tab | Left/Right: Change Value | Enter: Confirm",
            ),
        ),
        2 => (
            // Not Defteri
            gui::ui_text("Not Defteri", "Notepad"),
            gui::ui_text(
                "Ctrl+S: Kaydet | Ctrl+O: Aç | Ctrl+N: Yeni | Ctrl+Z: Geri Al | Ctrl+F: Bul",
                "Ctrl+S: Save | Ctrl+O: Open | Ctrl+N: New | Ctrl+Z: Undo | Ctrl+F: Find",
            ),
        ),
        3 => (
            // Web Tarayıcısı
            gui::ui_text("Web Tarayıcısı", "Web Browser"),
            gui::ui_text(
                "Ctrl+L: Adres | F5: Yenile | Ctrl+Shift+D: DevTools | Alt+←/→: Geri/İleri",
                "Ctrl+L: Address | F5: Reload | Ctrl+Shift+D: DevTools | Alt+←/→: Back/Fwd",
            ),
        ),
        4 => (
            // Resim Görüntüleyici
            gui::ui_text("Resim Görüntüleyici", "Image Viewer"),
            gui::ui_text(
                "+/-: Yakınlaştır/Uzaklaştır | 0: Sıfırla | Fare Tekerleği: Zoom | ESC: Kapat",
                "+/-: Zoom In/Out | 0: Reset | Mouse Wheel: Zoom | ESC: Close",
            ),
        ),
        5 => (
            // Müzik Çalar
            gui::ui_text("Müzik Çalar", "Music Player"),
            gui::ui_text(
                "Space: Oynat/Duraklat | ←/→: Önceki/Sonraki | ↑/↓: Ses",
                "Space: Play/Pause | ←/→: Prev/Next | ↑/↓: Volume",
            ),
        ),
        6 => (
            // Video Oynatıcı
            gui::ui_text("Video Oynatıcı", "Video Player"),
            gui::ui_text(
                "Space: Oynat/Duraklat | ←/→: İleri/Geri Sar | F: Tam Ekran",
                "Space: Play/Pause | ←/→: Seek | F: Fullscreen",
            ),
        ),
        7 => (
            // Görev Yöneticisi
            gui::ui_text("Görev Yöneticisi", "Task Manager"),
            gui::ui_text(
                "F5: Yenile | Del: İşlemi Sonlandır | ↑/↓: Gezin",
                "F5: Refresh | Del: End Process | ↑/↓: Navigate",
            ),
        ),
        8 => (
            // Hesap Makinesi
            gui::ui_text("Hesap Makinesi", "Calculator"),
            gui::ui_text(
                "Sayı tuşları | + - * / | Enter: Hesapla | Esc: Temizle",
                "Number keys | + - * / | Enter: Calculate | Esc: Clear",
            ),
        ),
        9 => (
            // Çizim Tahtası
            gui::ui_text("Çizim Tahtası", "Paint"),
            gui::ui_text(
                "C: Temizle | E: Silgi Aç/Kapat | Sol Tık: Çiz | Kaydet: Ctrl+S",
                "C: Clear | E: Toggle Eraser | Left Click: Draw | Save: Ctrl+S",
            ),
        ),
        10 => (
            // Terminal
            gui::ui_text("Terminal", "Terminal"),
            gui::ui_text(
                "help: Komutlar | Tab: Tamamla | Yukarı/Aşağı: Geçmiş | Ctrl+C: İptal",
                "help: Commands | Tab: Complete | Up/Down: History | Ctrl+C: Cancel",
            ),
        ),
        14 => (
            // Kontrol Paneli
            gui::ui_text("Kontrol Paneli", "Control Panel"),
            gui::ui_text(
                "Tab: Sekme | Sol/Sağ: Sekme Değiştir | Enter: Seç | Esc: Kapat",
                "Tab: Tab | Left/Right: Switch Tab | Enter: Select | Esc: Close",
            ),
        ),
        _ => return,
    };
    let now = crate::boot::time::monotonic_ms();
    compositor.push_toast(gui::ToastKind::Info, title, message, now, 5000);
}

pub fn desktop_item_rect(idx: usize) -> (usize, usize, usize, usize) {
    // Dosya ızgarası sol şerit hızlı simgelerin sağından başlar; iki grup
    // asla üst üste binmez (çizim + tıklama aynı kanonik konumu kullanır).
    let origin_x = gui::MinGuiCompositor::desktop_strip_right();
    let col = idx / 12;
    let row = idx % 12;
    (origin_x + col * 150, 30 + row * 54, 138, 46)
}

pub fn rects_overlap(a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Masaüstü simgesinin gerçekten çizildiği (görünür) alan mı?
/// Çizim döngüsü ile tıklama testi bu kuralı paylaşır: ekran dışına taşan,
/// görev çubuğuna gömülen veya sağ üst filigrana binen hücreler hem
/// çizilmez hem tıklanamaz.
pub fn desktop_item_visible(
    rect: (usize, usize, usize, usize),
    width: usize,
    height: usize,
) -> bool {
    if rect.0 + rect.2 > width || rect.1 + rect.3 >= height.saturating_sub(gui::taskbar_h()) {
        return false;
    }
    !rects_overlap(rect, gui::MinGuiCompositor::desktop_watermark_rect(width))
}

pub fn desktop_item_at(
    x: usize,
    y: usize,
    fm: &apps::filemanager::FileManagerApp,
    width: usize,
    height: usize,
) -> Option<(usize, (usize, usize, usize, usize))> {
    for i in 0..fm.file_count {
        let rect = desktop_item_rect(i);
        if !desktop_item_visible(rect, width, height) {
            continue;
        }
        if gui::in_rect(x, y, rect) {
            return Some((i, rect));
        }
    }
    None
}

/// Belirli bir pencere örneğini açar ve odaklar.
pub fn focus_app_window(compositor: &mut gui::MinGuiCompositor, idx: usize) {
    if let Some(win) = compositor.windows.get_mut(idx).and_then(Option::as_mut) {
        win.is_open = true;
        win.is_minimized = false;
        win.is_maximized = false;
    }
    compositor.focus_window(idx);
}

pub fn open_image_file(
    path: &str,
    vfs: &fs::vfs::VirtualFileSystem,
    _paints: &[Option<apps::paint::PaintApp>],
    viewer: &mut apps::imageviewer::ImageViewerApp,
) {
    // Görüntü çözme (PNG/JPEG/GIF/BMP) uzun sürebilir; imleç kum saati olsun.
    let _busy = gui::screens::BusyScope::new();
    let display_name = if path.ends_with("cizim.bmp") {
        "cizim.bmp"
    } else if path.ends_with(".png") {
        "Resim.png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "Resim.jpg"
    } else if path.ends_with(".gif") {
        "Resim.gif"
    } else {
        "Resim.bmp"
    };
    // Large VFS entries intentionally have an empty inline `data` vector;
    // check the shared payload store before `read_file`, which can return that
    // empty slice for the metadata row.
    if let Some(data) = vfs.read_large_file(path) {
        if !viewer.open_image(data, display_name) {
            viewer.clear_image("cizim.bmp");
        }
    } else if let Some(data) = vfs.read_file(path).filter(|data| !data.is_empty()) {
        let data: alloc::sync::Arc<[u8]> = alloc::sync::Arc::from(data);
        if !viewer.open_image(data, display_name) {
            viewer.clear_image("cizim.bmp");
        }
    } else {
        viewer.clear_image(if path.ends_with("cizim.bmp") {
            "cizim.bmp"
        } else {
            "Image unavailable"
        });
    }
}

pub fn keep_windows_on_screen(
    compositor: &mut gui::MinGuiCompositor,
    screen_w: usize,
    screen_h: usize,
) {
    let screen_w = screen_w.max(1);
    let usable_h = screen_h.saturating_sub(gui::taskbar_h()).max(1);
    for window in compositor.windows.iter_mut().flatten() {
        if !window.is_open {
            continue;
        }
        window.width = window.width.min(screen_w).max(1);
        window.height = window.height.min(usable_h).max(1);
        window.x = window.x.min(screen_w.saturating_sub(window.width));
        window.y = window.y.min(usable_h.saturating_sub(window.height));
    }
}

pub fn terminate_app(
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
    pkgmgr: &mut apps::pkgmgr::PackageManagerApp,
    fps: &mut apps::game::GameApp,
    fps_3d: &mut apps::fps_game::FpsGameApp,
    city: &mut apps::city_drive::CityDriveApp,
) {
    let app_id = compositor.app_id_for_window(idx);
    crate::watchdog::unwatch(idx);
    // Pencereyi kapatmakla kalma: pencereye bagli sureci de gercekten sonlandir.
    let _was_running = terminate_window_process(idx);
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

    let another_instance_open = compositor
        .windows
        .iter()
        .any(|slot| slot.is_some_and(|window| window.app_id as usize == app_id && window.is_open));
    if !another_instance_open {
        match app_id {
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
            13 => *pkgmgr = apps::pkgmgr::PackageManagerApp::new(),
            14 => apps::controlpanel::reset(),
            15 => *fps = apps::game::GameApp::new(),
            16 => *fps_3d = apps::fps_game::FpsGameApp::new(),
            17 => {
                *city = apps::city_drive::CityDriveApp::new();
                // Motor sesi kalsin diye PC speaker susturulur.
                let mut audio = drivers::audio::AudioDevice::new();
                audio.set_pc_speaker_tone(0);
            }
            _ => return,
        }
    }
    compositor.focus_top_window();
}

pub fn app_is_focused(compositor: &gui::MinGuiCompositor, app_id: usize) -> bool {
    compositor.windows.iter().any(|slot| {
        slot.is_some_and(|window| {
            window.app_id as usize == app_id
                && window.is_open
                && !window.is_minimized
                && window.is_focused
        })
    })
}

pub fn focused_window_for_app(
    compositor: &gui::MinGuiCompositor,
    app_id: usize,
) -> Option<(usize, gui::Window)> {
    // 1. Birincil yol: `is_focused` bayrağı taşıyan açık pencere.
    let focused = compositor
        .windows
        .iter()
        .enumerate()
        .find_map(|(idx, slot)| {
            slot.filter(|window| {
                window.app_id as usize == app_id
                    && window.is_open
                    && !window.is_minimized
                    && window.is_focused
            })
            .map(|window| (idx, window))
        });
    if focused.is_some() {
        return focused;
    }

    // 2. Yedek yol: odak bayrakları uyumsuz kaldığında bile girdi kaybolmasın
    //    diye `active_window_idx`'deki pencere app_id eşleşiyorsa kullanılır.
    let idx = compositor.active_window_idx?;
    let window = compositor.windows.get(idx)?.as_ref()?;
    if window.app_id as usize == app_id && window.is_open && !window.is_minimized {
        Some((idx, *window))
    } else {
        None
    }
}

/// İçeriğini kendi çizen (kaydırma ofsetini `render_content` içinde uygulayan)
/// uygulamalar. Bu uygulamalarda pencere kaydırması ayrıca uygulanmaz; aksi
/// halde içerik iki kez kaydırılır.
#[inline(always)]
pub fn app_scrolls_natively(app_id: usize) -> bool {
    matches!(app_id, 0 | 2 | 3)
}

/// Bir uygulamanın **çizimde kullanılan** pencere dikdörtgenini döndürür.
///
/// Kendi kaydırmasını yapan uygulamalarda gerçek pencere dikdörtgeni; diğer
/// uygulamalarda ise kaydırma ofseti kadar ötelenmiş ve doğal (tam) boyuta
/// genişletilmiş dikdörtgen döner. Fare etkileşimleri bu dikdörtgenle
/// eşleştirilmelidir; aksi halde kaydırılmış içerikte tıklama noktaları
/// kayar.
#[inline]
pub fn app_layout_rect(app_id: usize, win: &gui::Window) -> (usize, usize, usize, usize) {
    if app_scrolls_natively(app_id) {
        (win.x, win.y, win.width, win.height)
    } else {
        (
            win.x.saturating_sub(win.scroll.x),
            win.y.saturating_sub(win.scroll.y),
            win.content_w.saturating_add(2).max(win.width),
            win.content_h
                .saturating_add(gui::titlebar_h() + 1)
                .max(win.height),
        )
    }
}

/// Bir pencerenin kaydırılabilir içerik boyutunu **içerik alanı** biriminde
/// (başlık çubuğu hariç) döndürür.
///
/// - Kendi kaydırmasını yapan uygulamalarda gerçek içerik ölçümü kullanılır.
/// - Diğer uygulamalarda pencere ilk açıldığındaki doğal boyut (`natural_w`,
///   `natural_h`) alt sınır kabul edilir. Pencere bu boyutun altına
///   küçültüldüğünde içerik kaydırılabilir hale gelir.
///
/// Dönen değer her zaman görünür alandan küçük değildir; çağıran taraf
/// `max_offset` ile karşılaştırarak kaydırma çubuğu gerekip gerekmediğine
/// karar verir.
pub fn window_content_size(
    app_id: usize,
    win_w: usize,
    win_h: usize,
    natural_w: usize,
    natural_h: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
    br: &apps::browser::BrowserApp,
) -> (usize, usize) {
    let view_w = win_w.saturating_sub(2);
    let view_h = win_h.saturating_sub(gui::titlebar_h() + 1);

    let (need_w, need_h) = match app_id {
        // Dosya Yöneticisi: geçerli görünümün (liste/ızgara) içerik yüksekliği.
        0 => {
            let full_h = apps::filemanager::list_top() + fm.content_height(win_w) + 30;
            (view_w, full_h.saturating_sub(gui::titlebar_h()))
        }
        // Not Defteri: satır kaydırma (wrap) sonrası metin yüksekliği.
        // Satır yüksekliği ve karakter genişliği font ölçeğine duyarlı olmalı.
        2 => {
            let char_w = gui::sp(12);
            let line_h = gui::sp(16);
            let text_w = win_w.saturating_sub(gui::sp(12) + gui::sp(32) + gui::sp(16));
            let wrap_cols = (text_w / char_w.max(1)).max(1);
            let (lines, _) = np.text_extent(wrap_cols);
            let full_h = lines * line_h + gui::sp(58) + gui::sp(22);
            (view_w, full_h.saturating_sub(gui::titlebar_h()))
        }
        // Web Tarayıcısı: yalnızca HTML/lik geçmiş içeriği kaydırılabilir.
        3 => (view_w, br.content_height()),
        // Görev Yöneticisi: başlık + süreç satırları + bellek grafiği + durum.
        7 => (view_w, 275),
        // Diğer uygulamalar: doğal (ilk açılış) boyut alt sınırı.
        _ => (natural_w.max(view_w), natural_h.max(view_h)),
    };

    (need_w.max(view_w), need_h.max(view_h))
}

/// Bir pencerenin (uygulama indeksine göre) dikey içerik yüksekliğini hesaplar.
/// Eski çağrı noktaları için geriye dönük sarmalayıcı.
pub fn window_content_h(
    app_id: usize,
    win_h: usize,
    win_w: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
    br: &apps::browser::BrowserApp,
) -> usize {
    let view_h = win_h.saturating_sub(gui::titlebar_h() + 1);
    let (_, need_h) = window_content_size(
        app_id,
        win_w,
        win_h,
        win_w.saturating_sub(2),
        view_h,
        fm,
        np,
        br,
    );
    need_h
}

/// Defter imlecini her zaman görünür kılacak dikey kaydırmayı uygular.
pub fn notepad_ensure_cursor_visible(
    compositor: &mut gui::MinGuiCompositor,
    np: &apps::notepad::NotepadApp,
) {
    let Some((idx, w)) = focused_window_for_app(compositor, 2) else {
        return;
    };
    let desired = np.ensure_cursor_visible(w.width, w.height, w.scroll.y);
    if let Some(wm) = compositor.windows[idx].as_mut() {
        wm.scroll.y = desired;
    }
}

/// Tarayıcı içeriğini `delta` piksel yumuşak kaydırma hedefiyle kaydırır.
/// Kaydırma ofseti pencere kaydırmasında (`scroll.y`) tutulur; çizim ve isabet
/// alanları aynı değeri kullandığı için kısayollar görünür şekilde çalışır.
pub fn browser_scroll_by(
    compositor: &mut gui::MinGuiCompositor,
    br: &mut apps::browser::BrowserApp,
    delta: isize,
) {
    let Some((_, w)) = focused_window_for_app(compositor, 3) else {
        return;
    };
    let view_h = w.content_rect().3;
    let max_y = br.content_height().saturating_sub(view_h);
    let current = w.scroll.y.min(max_y);
    let target = if delta >= 0 {
        current.saturating_add(delta as usize)
    } else {
        current.saturating_sub(delta.unsigned_abs())
    };
    br.scroll_to(current, target, max_y, crate::boot::time::monotonic_ms());
}

/// Tarayıcı içeriğini en üste (`bottom == false`) veya en alta kaydırır.
pub fn browser_scroll_edge(
    compositor: &mut gui::MinGuiCompositor,
    br: &mut apps::browser::BrowserApp,
    bottom: bool,
) {
    let Some((_, w)) = focused_window_for_app(compositor, 3) else {
        return;
    };
    let view_h = w.content_rect().3;
    let max_y = br.content_height().saturating_sub(view_h);
    let current = w.scroll.y.min(max_y);
    let target = if bottom { max_y } else { 0 };
    br.scroll_to(current, target, max_y, crate::boot::time::monotonic_ms());
}

/// Dosya Yöneticisi listesinde seçili satırı görünür tutacak kaydırmayı uygular.
pub fn fm_ensure_selection_visible(
    compositor: &mut gui::MinGuiCompositor,
    fm: &apps::filemanager::FileManagerApp,
) {
    let Some((idx, w)) = focused_window_for_app(compositor, 0) else {
        return;
    };
    let view_h = w.height.saturating_sub(apps::filemanager::list_top() + 30);
    if view_h == 0 {
        return;
    }
    let sy = fm.ensure_selection_visible(w.width, view_h, w.scroll.y);
    if let Some(wm) = compositor.windows[idx].as_mut() {
        wm.scroll.y = sy;
    }
}

/// Dosya Yöneticisi sayfa adımı: listede görünür satır sayısı,
/// ızgarada görünür satır x sütun sayısı (PageUp/PageDown).
pub fn fm_page_rows(w: &gui::Window, fm: &apps::filemanager::FileManagerApp) -> usize {
    let view_h = w.height.saturating_sub(apps::filemanager::list_top() + 30);
    fm.page_step(w.width, view_h.max(1))
}

/// Terminal çıktı panelinde görünen satır sayısı (tekerlek ve sayfa gezinmesine ortak).
pub fn terminal_view_lines(w: &gui::Window) -> usize {
    (w.height.saturating_sub(86) / 13).max(1)
}

pub fn image_viewer_canvas_rect(w: &gui::Window) -> (usize, usize, usize, usize) {
    let inner_x = w.x + 10;
    let inner_w = w.width.saturating_sub(20).max(1);
    let content_y = w.y + 60;
    let status_y = w.y + w.height.saturating_sub(24);
    let content_h = status_y.saturating_sub(content_y + 6).max(1);
    if inner_w < 360 {
        (inner_x, content_y, inner_w, (content_h * 2 / 3).max(1))
    } else {
        let panel_w = (inner_w / 3)
            .clamp(130, 220)
            .min(inner_w.saturating_sub(13));
        (
            inner_x,
            content_y,
            inner_w.saturating_sub(panel_w + 12).max(1),
            content_h,
        )
    }
}

/// Takvimde ay ileri/geri gezinme.
pub fn step_month(year: u16, month: u8, forward: bool) -> (u16, u8) {
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

/// Kurulu paketi veya sistem ikilisini başlatır (Terminal veya ilgili pencere).
pub fn launch_installed_pkg(
    app_name: &str,
    compositor: &mut gui::MinGuiCompositor,
    terminal: &mut apps::terminal::TerminalApp,
    _notepad: &mut apps::notepad::NotepadApp,
    _calc: &mut apps::calculator::CalculatorApp,
    _paint: &mut alloc::vec::Vec<Option<apps::paint::PaintApp>>,
    vfs: &mut fs::vfs::VirtualFileSystem,
    pmm: &mut crate::mm::pmm::PhysicalMemoryManager,
    now: &crate::drivers::rtc::DateTime,
) {
    if app_name.is_empty() {
        return;
    }
    let clean = app_name
        .strip_suffix(".pkg")
        .or_else(|| app_name.strip_suffix(".elf"))
        .unwrap_or(app_name);
    let lower = clean.to_ascii_lowercase();
    if lower == "notepad" || lower == "not defteri" || lower == "tiny-editor" {
        open_app(compositor, 2);
    } else if lower == "calculator" || lower == "hesap makinesi" {
        open_app(compositor, 8);
    } else if lower == "paint" || lower == "cizim tahtasi" {
        open_app(compositor, 9);
    } else if lower == "browser" || lower == "web tarayicisi" {
        open_app(compositor, 3);
    } else {
        open_app(compositor, 10);
        let path = if app_name.ends_with(".elf") || app_name.ends_with(".pkg") {
            alloc::format!("/bin/{}", app_name)
        } else if vfs
            .read_file(&alloc::format!("/bin/{}.pkg", app_name))
            .is_some()
        {
            alloc::format!("/bin/{}.pkg", app_name)
        } else {
            alloc::format!("/bin/{}.elf", app_name)
        };
        terminal.push_line(
            crate::gui::ui_text(
                "--- UYGULAMA BASLATILIYOR ---",
                "--- LAUNCHING APPLICATION ---",
            ),
            gui::Color::ACCENT_PINK,
        );
        let mut msg = gui::TextBuf::new();
        msg.push_str(crate::gui::ui_text(
            "[minPak] Çalıştırılıyor: ",
            "[minPak] Executing: ",
        ));
        msg.push_str(&path);
        terminal.push_line(msg.as_str(), gui::Color::ACCENT_GREEN);

        let exec_cmd = alloc::format!("exec {}", path);
        terminal.execute(&exec_cmd, vfs, pmm, now);
    }
}

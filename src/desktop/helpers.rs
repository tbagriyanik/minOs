//! Masaustu ve pencere yardimcilari: uygulama acma/kapama, masaustu simge geometrisi,
//! kaydirma konumlandirma ve takvim gezinme.

use crate::{apps, drivers, fs, gui, mm};

/// Pencereye bagli calisan surec adlari (gorev yoneticisi surec tablosu icin).
fn app_process_name(app_id: usize) -> &'static str {
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
            let _ = mgr
                .proc_mgr
                .spawn(app_process_name(app_id), 0, 0, 0, app_id, Some(idx));
        }
    }
}

/// Bir pencereye bagli sureci sonlandirir ve kaynaklarini serbest birakir.
pub fn terminate_window_process(idx: usize) {
    unsafe {
        let mgr = &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR);
        let pid = mgr
            .proc_mgr
            .processes
            .iter()
            .find(|p| {
                p.state != mm::process::ProcessState::Unused
                    && p.state != mm::process::ProcessState::Terminated
                    && p.resources.window_idx == Some(idx)
            })
            .map(|p| p.pid);
        if let Some(pid) = pid {
            let _ = mgr.proc_mgr.terminate(pid, 0, &mut mgr.pmm);
        }
    }
}

/// Uygulama başlatma isteğinde yeni örnek açar; tekil uygulamalar var olan pencereye gider.
pub fn open_app(compositor: &mut gui::MinGuiCompositor, app_id: usize) {
    if let Some(idx) = compositor.launch_app_window(app_id) {
        if let Some(win) = compositor.windows[idx].as_mut() {
            win.is_open = true;
            win.is_minimized = false;
            win.is_maximized = false;
        }
        ensure_window_process(idx, app_id);
        compositor.focus_window(idx);
    }
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
    // Large VFS entries intentionally have an empty inline `data` vector;
    // check the shared payload store before `read_file`, which can return that
    // empty slice for the metadata row.
    if let Some(data) = vfs.read_large_file(path) {
        if !viewer.open_image(
            data,
            if path.ends_with("cizim.bmp") {
                "cizim.bmp"
            } else {
                "Resim.bmp"
            },
        ) {
            viewer.clear_image("cizim.bmp");
        }
    } else if let Some(data) = vfs.read_file(path).filter(|data| !data.is_empty()) {
        let data: alloc::sync::Arc<[u8]> = alloc::sync::Arc::from(data);
        if !viewer.open_image(
            data,
            if path.ends_with("cizim.bmp") {
                "cizim.bmp"
            } else {
                "Resim.bmp"
            },
        ) {
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
) {
    let app_id = compositor.app_id_for_window(idx);
    // Pencereyi kapatmakla kalma: pencereye bagli sureci de gercekten sonlandir.
    terminate_window_process(idx);
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
        slot.map_or(false, |window| {
            window.app_id as usize == app_id && window.is_open
        })
    });
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
            _ => return,
        }
    }
    compositor.focus_top_window();
}

pub fn app_is_focused(compositor: &gui::MinGuiCompositor, app_id: usize) -> bool {
    compositor.windows.iter().any(|slot| {
        slot.map_or(false, |window| {
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
    compositor
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
        })
}

/// Bir pencerenin (uygulama indeksine göre) dikey içerik yüksekliğini hesaplar.
/// Kaydırma çubuğunun gerekip gerekmediğini ve tutamaç boyutunu belirlemek için
/// hem çizim hem de tıklama kod yollarında ortak kullanılır.
pub fn window_content_h(
    app_id: usize,
    _win_h: usize,
    win_w: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
    br: &apps::browser::BrowserApp,
) -> usize {
    match app_id {
        // Dosya Yöneticisi: geçerli görünümün (liste/ızgara) içerik yüksekliği.
        0 => apps::filemanager::list_top() + fm.content_height(win_w) + 30,
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
        // Web Tarayıcısı: yalnızca HTML/lik geçmiş içeriği kaydırılabilir.
        3 => br.content_height(),
        // Görev Yöneticisi: başlık + süreç satırları + bellek grafiği + durum.
        7 => 275,
        // Diğer uygulamalar sabit düzenlidir: kaydırma gerekmez, içerik pencereye
        // kırpılır (clip). `0` döndürmek "kaydırma yok" anlamına gelir.
        _ => 0,
    }
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

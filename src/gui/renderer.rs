//! Ayrıştırılmış rendering katmanı.
//!
//! `main.rs`'teki büyük `render_full_frame` kapamasından (closure) ortak
//! çizim desenleri ayrıştırılmış, yeniden kullanılabilir fonksiyonlara dönüştürülmüştür.
//! Bu sayede ana döngü daha okunur ve render mantığı tek bir yerde toplanır.

use super::*;
use crate::apps;
use crate::desktop::helpers::{desktop_item_rect, desktop_item_visible, window_content_h};
use crate::drivers;
use crate::gui;

/// Masaüstü simgelerini çizer.
pub fn render_desktop_icons(
    compositor: &MinGuiCompositor,
    fb: &mut drivers::video::Framebuffer,
    desktop_fm: &apps::filemanager::FileManagerApp,
    width: usize,
    height: usize,
) {
    compositor.render_desktop_bg(fb);

    for i in 0..desktop_fm.file_count {
        let item = &desktop_fm.files[i];
        let (x, y, w, h) = desktop_item_rect(i);

        if !desktop_item_visible((x, y, w, h), width, height) {
            continue;
        }

        if desktop_fm.is_row_selected(i) {
            fb.draw_rounded_rect_alpha(
                x,
                y,
                w,
                h,
                5,
                MinGuiCompositor::shade(Color::ACCENT_BLUE, 28),
                160,
            );
            fb.draw_rounded_rect_outline(x, y, w, h, 5, Color::ACCENT_BLUE);
        } else {
            fb.draw_rounded_rect_alpha(
                x,
                y,
                w,
                h,
                5,
                MinGuiCompositor::shade(Color::WINDOW_BG, 8),
                128,
            );
            fb.draw_rounded_rect_outline(x, y, w, h, 5, Color::BORDER);
        }

        if item.is_dir {
            apps::filemanager::FileManagerApp::draw_folder_icon(fb, x + 5, y + 5, false);
        } else {
            apps::filemanager::FileManagerApp::draw_file_icon(fb, x + 5, y + 5, item.icon_tag);
        }

        let item_fg = if desktop_fm.is_row_selected(i) {
            MinGuiCompositor::text_on(MinGuiCompositor::shade(Color::ACCENT_BLUE, 28))
        } else {
            Color::TEXT_MAIN
        };

        MinGuiCompositor::draw_string_clipped(
            fb,
            x + 30,
            y + 10,
            item.display_name(),
            item_fg,
            (x + 30, y + 2, w.saturating_sub(34), h.saturating_sub(4)),
        );
    }
}

/// Tek bir pencerenin çerçevesini ve içeriğini çizer.
pub fn render_window_content(
    fb: &mut drivers::video::Framebuffer,
    compositor: &MinGuiCompositor,
    win: &Window,
    idx: usize,
    _mouse: &drivers::mouse::MouseState,
    fm: &apps::filemanager::FileManagerApp,
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
    fps: &apps::game::GameApp,
    fps_3d: &apps::fps_game::FpsGameApp,
    settings: &apps::settings::SystemSettings,
    kbd_layout: drivers::keyboard::KeyboardLayout,
    now: &drivers::rtc::DateTime,
    total_ram_bytes: u64,
    cpu_vendor: &'static str,
    cpu_logical_count: u32,
    audio_driver: &'static str,
    network_driver: &'static str,
    network_status: &'static str,
    _dirty_rect: Option<(usize, usize, usize, usize)>,
) -> (usize, usize, bool, bool) {
    let app_id = compositor.app_id_for_window(idx);

    let (cx, cy, cw, ch) = win.content_rect();

    fb.set_clip(cx, cy, cw, ch);

    let content_h = window_content_h(app_id, win.height, win.width, fm, np, br);

    let content_w = cw;

    let want_v = content_h > ch;
    let want_h = content_w > cw;

    let v_gutter = if want_v {
        crate::gui::scrollbar::SCROLLBAR_THICKNESS
    } else {
        0
    };
    let h_gutter = if want_h {
        crate::gui::scrollbar::SCROLLBAR_THICKNESS
    } else {
        0
    };

    let content_area = (
        cx,
        cy,
        cw.saturating_sub(v_gutter).max(1),
        ch.saturating_sub(h_gutter).max(1),
    );

    fb.set_clip(
        content_area.0,
        content_area.1,
        content_area.2,
        content_area.3,
    );

    let render_start = crate::boot::time::read_cpu_cycles();

    match app_id {
        0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll, false),
        1 => apps::settings::SystemSettings::render_content(
            settings,
            fb,
            win.x,
            win.y,
            win.width,
            win.height,
            kbd_layout,
            now,
            total_ram_bytes,
            cpu_vendor,
            cpu_logical_count,
            audio_driver,
            network_driver,
            network_status,
        ),
        2 => np.render_content(fb, win.x, win.y, win.width, win.height, 0, &win.scroll),
        3 => br.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),
        4 => iv.render_content(fb, win.x, win.y, win.width, win.height),
        5 => mp.render_content(fb, win.x, win.y, win.width, win.height, 0),
        6 => vp.render_content(fb, win.x, win.y, win.width, win.height, 0),
        7 => tm.render_content(fb, win.x, win.y, win.width, win.height),
        8 => calc.render_content(fb, win.x, win.y, win.width, win.height),
        9 => {
            if let Some(app) = paint[idx].as_ref() {
                app.render_content(fb, win.x, win.y, win.width, win.height)
            }
        }
        10 => term.render_content(fb, win.x, win.y, win.width, win.height, win.is_focused),
        11 => inst.render_content(fb, win.x, win.y, win.width, win.height),
        12 => v3d.render_content(fb, win.x, win.y, win.width, win.height),
        13 => pkg.render_content(fb, win.x, win.y, win.width, win.height),
        14 => apps::controlpanel::render_content(
            fb,
            win.x,
            win.y,
            win.width,
            win.height,
            now,
            total_ram_bytes,
            0,
            kbd_layout,
        ),
        15 => fps.render_content(fb, win.x, win.y, win.width, win.height),
        16 => fps_3d.render_content(fb, win.x, win.y, win.width, win.height),
        _ => {}
    }

    let _render_elapsed = crate::boot::time::monotonic_ms().saturating_sub(
        crate::boot::time::read_cpu_cycles().wrapping_sub(render_start) / 1_000_000,
    );

    fb.reset_clip();

    let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;

    if want_v {
        let v_h = ch.saturating_sub(if want_h { sb } else { 0 });
        if v_h > 0 {
            let track = (cx + cw - sb, cy, sb, v_h);
            let g = crate::gui::scrollbar::v_geom(track, content_h, v_h, win.scroll.y);
            crate::gui::scrollbar::draw_vertical(fb, &g);
        }
    }

    if want_h {
        let h_w = cw.saturating_sub(if want_v { sb } else { 0 });
        if h_w > 0 {
            let track = (cx, cy + ch - sb, h_w, sb);
            let g = crate::gui::scrollbar::h_geom(track, content_w, h_w, win.scroll.x);
            crate::gui::scrollbar::draw_horizontal(fb, &g);
        }
    }

    (content_h, content_w, want_v, want_h)
}

/// Özellikler (Properties) modalı'nı çizer.
pub fn render_properties_modal(
    fb: &mut drivers::video::Framebuffer,
    properties_state: (bool, &TextBuf, &TextBuf, &TextBuf, &TextBuf, &TextBuf),
    width: usize,
    height: usize,
) {
    if !properties_state.0 {
        return;
    }

    let pw = 340usize.min(width.saturating_sub(20));
    let ph = 260usize.min(height.saturating_sub(40));
    let px = width.saturating_sub(pw) / 2;
    let py = height.saturating_sub(ph) / 2;

    fb.draw_shadow(px, py, pw, ph, 0x000000);
    fb.draw_rounded_rect(px, py, pw, ph, 8, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(px, py, pw, ph, 8, Color::ACCENT_BLUE);

    fb.draw_rect(px, py, pw, 28, Color::TITLE_BAR);

    MinGuiCompositor::draw_string(
        fb,
        px + 10,
        py + 6,
        gui::ui_text("Özellikler", "Properties"),
        Color::WHITE,
    );

    let start_y = py + 40;
    let row_gap = 30;

    let rows = [
        ("Adı", properties_state.1.as_str()),
        ("Tür", properties_state.2.as_str()),
        ("Boyut", properties_state.3.as_str()),
        ("Konum", properties_state.4.as_str()),
        ("Yol", properties_state.5.as_str()),
    ];

    for (i, (label, value)) in rows.iter().enumerate() {
        let ry = start_y + row_gap * i;
        MinGuiCompositor::draw_string(fb, px + 20, ry, *label, Color::TEXT_MUTED);
        MinGuiCompositor::draw_string_clipped(
            fb,
            px + 90,
            ry,
            *value,
            Color::TEXT_MAIN,
            (px + 90, ry, pw.saturating_sub(100), 20),
        );
    }

    let ok_btn_w = 76;
    let ok_btn_h = 24;
    let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;
    let ok_btn_y = py + ph.saturating_sub(34);

    fb.draw_rounded_rect(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, Color::TITLE_BAR);
    fb.draw_rounded_rect_outline(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, Color::BORDER);

    let ok_text = gui::ui_text("Tamam", "OK");
    let tw = MinGuiCompositor::text_width(ok_text, 1);
    MinGuiCompositor::draw_string(
        fb,
        ok_btn_x + (ok_btn_w.saturating_sub(tw)) / 2,
        ok_btn_y + 4,
        ok_text,
        Color::WHITE,
    );
}

/// Hızlı çalıştır (Run) diyalogunu çizer.
pub fn render_run_dialog(
    fb: &mut drivers::video::Framebuffer,
    run_dialog_state: (bool, &TextBuf),
    width: usize,
    height: usize,
    tick: usize,
) {
    if !run_dialog_state.0 {
        return;
    }

    let dw = 320usize;
    let dh = 110usize;
    let dx = width.saturating_sub(dw) / 2;
    let dy = height.saturating_sub(dh) / 2;

    fb.draw_shadow(dx, dy, dw, dh, 0x000000);
    fb.draw_rounded_rect(dx, dy, dw, dh, 6, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(dx, dy, dw, dh, 6, Color::ACCENT_BLUE);
    fb.draw_rect(dx, dy, dw, 24, Color::TITLE_BAR);

    MinGuiCompositor::draw_string(
        fb,
        dx + 10,
        dy + 6,
        gui::ui_text("çalıştır (Run)", "Run Program"),
        Color::WHITE,
    );

    MinGuiCompositor::draw_string(
        fb,
        dx + 12,
        dy + 34,
        gui::ui_text("Komut veya uygulama adı:", "Command or app name:"),
        Color::TEXT_MUTED,
    );

    let bx = dx + 12;
    let by = dy + 54;
    let bw = dw - 24;
    let bh = 24;

    fb.draw_rect(bx, by, bw, bh, Color::DARK_BG);
    fb.draw_rect_outline(bx, by, bw, bh, Color::BORDER);

    let input_text = run_dialog_state.1.as_str();
    MinGuiCompositor::draw_string(fb, bx + 6, by + 6, input_text, Color::TEXT_MAIN);

    let cur_x = bx + 6 + MinGuiCompositor::text_width(input_text, 1);
    if (tick / 20) % 2 == 0 {
        fb.draw_rect(cur_x, by + 4, 2, 16, Color::ACCENT_YELLOW);
    }

    MinGuiCompositor::draw_string(
        fb,
        dx + 12,
        dy + 88,
        gui::ui_text(
            "cmd, not, ayar, task, calc, web  |  [Enter] Aç  [Esc] çık",
            "cmd, not, ayar, task, calc, web  |  [Enter] Open  [Esc] Cancel",
        ),
        Color::TEXT_MUTED,
    );
}

/// Hoş geldin toast bildirimini çizer.
pub fn render_welcome_toast(compositor: &mut MinGuiCompositor, now_ms: u64) {
    let welcome_title = ui_text("minOs'a Hoş Geldiniz", "Welcome to minOs");
    let welcome_msg = ui_text(
        "Sistem hazır. Başlat menüsü ve kısayollar etkin.",
        "System is ready. Start menu and shortcuts active.",
    );
    compositor.push_toast(ToastKind::Info, welcome_title, welcome_msg, now_ms, 5500);
}

/// Aero Snap önizleme çerçevesini çizer.
pub fn render_snap_preview(
    fb: &mut drivers::video::Framebuffer,
    snap_preview: Option<(usize, usize, usize, usize)>,
) {
    if let Some((sx, sy, sw, sh)) = snap_preview {
        fb.draw_rounded_rect_alpha(sx, sy, sw, sh, 4, Color::ACCENT_BLUE, 55);
        fb.draw_rounded_rect_outline(sx, sy, sw, sh, 4, Color::ACCENT_BLUE);
        fb.draw_rounded_rect_outline(
            sx + 1,
            sy + 1,
            sw.saturating_sub(2),
            sh.saturating_sub(2),
            4,
            Color::WHITE,
        );
    }
}

use crate::apps;
use crate::drivers;
use crate::gui;

/// State for the main event loop.
pub struct EventLoopState {
    pub last_mouse_x: usize,
    pub last_mouse_y: usize,
    pub last_buttons: (bool, bool),
    pub pending_window_control: Option<(usize, u8)>,
    pub dragging_win: Option<usize>,
    pub resizing_win: Option<usize>,
    pub resize_edge: Option<gui::ResizeEdge>,
    pub resize_orig: (usize, usize, usize, usize),
    pub drag_offset_x: isize,
    pub drag_offset_y: isize,
    pub active_snap_preview: Option<(usize, usize, usize, usize)>,
    pub vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)>,
    pub anim_tick: usize,
    pub last_frame_ms: u64,
    pub loop_counter: u64,
    pub last_clock_update_ms: u64,
    pub game_perf_us: u64,
    pub last_music_beat: usize,
    pub last_music_progress_ms: u64,
    pub last_video_frame_ms: u64,
    pub cpu_sample_start: u64,
    pub cpu_sample_total_cycles: u64,
    pub cpu_sample_active_cycles: u64,
    pub last_ap_heartbeats: [u64; crate::arch::x86_64::smp::MAX_CPUS],
    pub music_note: usize,
    pub speaker_on: bool,
    pub game_sound_until_ms: u64,
    pub last_game_tick_ms: u64,
    pub last_fps3d_tick_ms: u64,
    pub app_last_activity: [u64; gui::MAX_WINDOWS],
    pub game_budget_strikes: u32,
    pub fps3d_budget_strikes: u32,
    pub app_render_time_ms: [core::cell::Cell<u64>; gui::MAX_WINDOWS],
    pub app_render_cycles: [u64; gui::MAX_WINDOWS],
    pub cpu_warn_shown: bool,
    pub ram_warn_shown: bool,
    pub run_dialog_open: bool,
    pub run_dialog_input: gui::TextBuf,
    pub properties_open: bool,
    pub properties_name: gui::TextBuf,
    pub properties_path: gui::TextBuf,
    pub properties_type: gui::TextBuf,
    pub properties_size: gui::TextBuf,
    pub properties_location: gui::TextBuf,
    pub screenshot_counter: usize,
    pub notepad_dragging: bool,
    pub browser_address_dragging: bool,
    pub browser_page_dragging: bool,
    pub browser_last_img_click: Option<(usize, usize)>,
    pub browser_last_img_click_ms: u64,
    pub terminal_dragging: bool,
    pub start_search_dragging: bool,
    pub image_drag_last: Option<(usize, usize)>,
    pub last_title_idx: Option<usize>,
    pub last_title_click_ms: u64,
}

impl Default for EventLoopState {
    fn default() -> Self {
        let mut last_ap_heartbeats = [0u64; crate::arch::x86_64::smp::MAX_CPUS];
        for c in 0..crate::arch::x86_64::smp::MAX_CPUS {
            last_ap_heartbeats[c] = crate::arch::x86_64::smp::heartbeat(c);
        }

        Self {
            last_mouse_x: 0,
            last_mouse_y: 0,
            last_buttons: (false, false),
            pending_window_control: None,
            dragging_win: None,
            resizing_win: None,
            resize_edge: None,
            resize_orig: (0, 0, 0, 0),
            drag_offset_x: 0,
            drag_offset_y: 0,
            active_snap_preview: None,
            vscroll_drag: None,
            anim_tick: 0,
            last_frame_ms: 0,
            loop_counter: 0,
            last_clock_update_ms: 0,
            game_perf_us: 0,
            last_music_beat: 0,
            last_music_progress_ms: 0,
            last_video_frame_ms: 0,
            cpu_sample_start: 0,
            cpu_sample_total_cycles: 0,
            cpu_sample_active_cycles: 0,
            last_ap_heartbeats,
            music_note: 0,
            speaker_on: false,
            game_sound_until_ms: 0,
            last_game_tick_ms: 0,
            last_fps3d_tick_ms: 0,
            app_last_activity: [0; gui::MAX_WINDOWS],
            game_budget_strikes: 0,
            fps3d_budget_strikes: 0,
            app_render_time_ms: [const { core::cell::Cell::new(0) }; gui::MAX_WINDOWS],
            app_render_cycles: [0u64; gui::MAX_WINDOWS],
            cpu_warn_shown: false,
            ram_warn_shown: false,
            run_dialog_open: false,
            run_dialog_input: gui::TextBuf::new(),
            properties_open: false,
            properties_name: gui::TextBuf::new(),
            properties_path: gui::TextBuf::new(),
            properties_type: gui::TextBuf::new(),
            properties_size: gui::TextBuf::new(),
            properties_location: gui::TextBuf::new(),
            screenshot_counter: 1,
            notepad_dragging: false,
            browser_address_dragging: false,
            browser_page_dragging: false,
            browser_last_img_click: None,
            browser_last_img_click_ms: 0,
            terminal_dragging: false,
            start_search_dragging: false,
            image_drag_last: None,
            last_title_idx: None,
            last_title_click_ms: 0,
        }
    }
}

/// Context passed to the render closure.
pub struct RenderContext<'a> {
    pub fb: &'a mut drivers::video::Framebuffer,
    pub compositor: &'a gui::MinGuiCompositor,
    pub filemanager_app: &'a apps::filemanager::FileManagerApp,
    pub desktop_files: &'a apps::filemanager::FileManagerApp,
    pub settings_app: &'a apps::settings::SystemSettings,
    pub notepad_app: &'a apps::notepad::NotepadApp,
    pub browser_app: &'a apps::browser::BrowserApp,
    pub imageviewer_app: &'a apps::imageviewer::ImageViewerApp,
    pub musicplayer_app: &'a apps::musicplayer::MusicPlayerApp,
    pub videoplayer_app: &'a apps::videoplayer::VideoPlayerApp,
    pub taskmgr_app: &'a apps::taskmgr::TaskManagerApp,
    pub calculator_app: &'a apps::calculator::CalculatorApp,
    pub paint_apps: &'a [Option<apps::paint::PaintApp>],
    pub terminal_app: &'a apps::terminal::TerminalApp,
    pub installer_app: &'a apps::installer::InstallerApp,
    pub viewer3d_app: &'a apps::viewer3d::Viewer3DApp,
    pub pkgmgr_app: &'a apps::pkgmgr::PackageManagerApp,
    pub fps_game_app: &'a apps::game::GameApp,
    pub fps_3d_app: &'a apps::fps_game::FpsGameApp,
    pub now: &'a drivers::rtc::DateTime,
    pub mouse: &'a drivers::mouse::MouseState,
    pub kbd: &'a drivers::keyboard::KeyboardState,
    pub total_ram_bytes: u64,
    pub used_ram_bytes: u64,
    pub tick: usize,
    pub app_render_cycles: &'a mut [u64; gui::MAX_WINDOWS],
    pub run_dialog_state: (bool, &'a gui::TextBuf),
    pub properties_state: (
        bool,
        &'a gui::TextBuf,
        &'a gui::TextBuf,
        &'a gui::TextBuf,
        &'a gui::TextBuf,
        &'a gui::TextBuf,
    ),
    pub snap_preview: Option<(usize, usize, usize, usize)>,
    pub dirty_rect: Option<(usize, usize, usize, usize)>,
}

/// Renders a full frame using the provided context.
pub fn render_frame(ctx: RenderContext) {
    let RenderContext {
        fb,
        compositor,
        filemanager_app,
        desktop_files,
        settings_app,
        notepad_app,
        browser_app,
        imageviewer_app,
        musicplayer_app,
        videoplayer_app,
        taskmgr_app,
        calculator_app,
        paint_apps,
        terminal_app,
        installer_app,
        viewer3d_app,
        pkgmgr_app,
        fps_game_app,
        fps_3d_app,
        now,
        mouse,
        kbd,
        total_ram_bytes,
        used_ram_bytes,
        tick,
        app_render_cycles,
        run_dialog_state,
        properties_state,
        snap_preview,
        dirty_rect,
    } = ctx;

    fb.set_frame_clip(dirty_rect);

    // 1. Desktop Background
    compositor.render_desktop_bg(fb);

    // /desktop contents as desktop icons
    for i in 0..desktop_files.file_count {
        let item = &desktop_files.files[i];
        let (x, y, w, h) = desktop_item_rect(i);

        if !desktop_item_visible((x, y, w, h), fb.width, fb.height) {
            continue;
        }

        if desktop_files.is_row_selected(i) {
            fb.draw_rounded_rect_alpha(
                x,
                y,
                w,
                h,
                5,
                gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28),
                160,
            );
            fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::ACCENT_BLUE);
        } else {
            fb.draw_rounded_rect_alpha(
                x,
                y,
                w,
                h,
                5,
                gui::MinGuiCompositor::shade(gui::Color::WINDOW_BG, 8),
                128,
            );
            fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::BORDER);
        }

        if item.is_dir {
            apps::filemanager::FileManagerApp::draw_folder_icon(fb, x + 5, y + 5, false);
        } else {
            apps::filemanager::FileManagerApp::draw_file_icon(fb, x + 5, y + 5, item.icon_tag);
        }

        let item_fg = if desktop_files.is_row_selected(i) {
            gui::MinGuiCompositor::text_on(gui::MinGuiCompositor::shade(
                gui::Color::ACCENT_BLUE,
                28,
            ))
        } else {
            gui::Color::TEXT_MAIN
        };

        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            x + 30,
            y + 10,
            item.display_name(),
            item_fg,
            (x + 30, y + 2, w - 34, h - 4),
        );
    }

    // 2. Open windows in z-order
    let (order, count) = compositor.draw_order();

    for k in 0..count {
        let idx = order[k];

        if let Some(win) = compositor.windows[idx] {
            if !win.is_open || win.is_minimized {
                continue;
            }

            // Skip off-screen windows
            if win.x >= fb.width
                || win.y >= fb.height
                || win.x.saturating_add(win.width) == win.x
                || win.y.saturating_add(win.height) == win.y
            {
                continue;
            }

            // Skip if no overlap with dirty rect
            if let Some((dx, dy, dw, dh)) = dirty_rect {
                let wx0 = win.x.saturating_sub(8);
                let wy0 = win.y.saturating_sub(8);
                let wx1 = win.x.saturating_add(win.width).saturating_add(8);
                let wy1 = win.y.saturating_add(win.height).saturating_add(8);

                let overlaps = dx < wx1
                    && wx0 < dx.saturating_add(dw)
                    && dy < wy1
                    && wy0 < dy.saturating_add(dh);

                if !overlaps {
                    continue;
                }
            }

            compositor.render_window_frame(fb, &win, mouse, idx);

            let (cx, cy, cw, ch) = win.content_rect();
            fb.set_clip(cx, cy, cw, ch);

            let app_id = compositor.app_id_for_window(idx);

            let _app_context = apps::guard::ContextScope::new(app_id, idx);

            let content_h = window_content_h(
                app_id,
                win.height,
                win.width,
                filemanager_app,
                notepad_app,
                browser_app,
            );

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
                cw.saturating_sub(v_gutter),
                ch.saturating_sub(h_gutter),
            );

            fb.set_clip(
                content_area.0,
                content_area.1,
                content_area.2.max(1),
                content_area.3.max(1),
            );

            let render_start = crate::read_cpu_cycles();

            match app_id {
                0 => filemanager_app.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    &win.scroll,
                    kbd.ctrl,
                ),

                1 => settings_app.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    kbd.layout,
                    now,
                    total_ram_bytes,
                    cpu_vendor(),
                    cpu_logical_count() as u32,
                    if drivers::hda::is_ready() {
                        if drivers::hda::is_dma_progressing() {
                            gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)")
                        } else {
                            gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)")
                        }
                    } else {
                        gui::ui_text("PC Speaker (PWM fallback)", "PC Speaker (PWM fallback)")
                    },
                    if drivers::e1000::is_ready() && drivers::net::is_isa() {
                        gui::ui_text("NE2000 (ISA)", "NE2000 (ISA)")
                    } else if drivers::e1000::is_ready() {
                        drivers::e1000::nic_name()
                    } else {
                        gui::ui_text("Bulunamadı", "Not found")
                    },
                    if drivers::e1000::is_ready() {
                        gui::ui_text("Hazir", "Ready")
                    } else {
                        gui::ui_text(
                            "Başlatılamadı / aygıt yok",
                            "Initialization failed / device missing",
                        )
                    },
                ),

                2 => notepad_app.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    tick,
                    &win.scroll,
                ),

                3 => {
                    browser_app.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll)
                }

                4 => imageviewer_app.render_content(fb, win.x, win.y, win.width, win.height),

                5 => musicplayer_app.render_content(fb, win.x, win.y, win.width, win.height, tick),

                6 => videoplayer_app.render_content(fb, win.x, win.y, win.width, win.height, tick),

                7 => taskmgr_app.render_content(fb, win.x, win.y, win.width, win.height),

                8 => calculator_app.render_content(fb, win.x, win.y, win.width, win.height),

                9 => {
                    if let Some(app) = paint_apps[idx].as_ref() {
                        app.render_content(fb, win.x, win.y, win.width, win.height)
                    }
                }

                10 => terminal_app.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    win.is_focused,
                ),

                11 => installer_app.render_content(fb, win.x, win.y, win.width, win.height),

                12 => viewer3d_app.render_content(fb, win.x, win.y, win.width, win.height),

                13 => pkgmgr_app.render_content(fb, win.x, win.y, win.width, win.height),

                14 => apps::controlpanel::render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    now,
                    total_ram_bytes,
                    used_ram_bytes,
                    kbd.layout,
                ),

                15 => fps_game_app.render_content(fb, win.x, win.y, win.width, win.height),

                16 => fps_3d_app.render_content(fb, win.x, win.y, win.width, win.height),

                _ => {}
            }

            app_render_cycles[idx] = crate::read_cpu_cycles().wrapping_sub(render_start);
            // app_render_time_ms tracking removed - structure changed

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
        }
    }

    fb.reset_clip();

    // Aero Snap preview
    if let Some((sx, sy, sw, sh)) = snap_preview {
        fb.draw_rounded_rect_alpha(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE, 55);
        fb.draw_rounded_rect_outline(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE);
        fb.draw_rounded_rect_outline(
            sx + 1,
            sy + 1,
            sw.saturating_sub(2),
            sh.saturating_sub(2),
            4,
            gui::Color::WHITE,
        );
    }

    // 3. Taskbar, overlays, cursor
    compositor.render_taskbar_and_overlays(
        fb,
        mouse,
        kbd,
        now,
        settings_app.volume,
        settings_app.is_muted,
        settings_app.language.is_english(),
        drivers::e1000::is_ready(),
    );

    // Drag badges
    if filemanager_app.drag_active {
        filemanager_app.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);
    } else if desktop_files.drag_active {
        desktop_files.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);
    }

    // Properties modal
    if properties_state.0 {
        let pw = 340usize.min(fb.width.saturating_sub(20));
        let ph = 260usize.min(fb.height.saturating_sub(40));
        let px = fb.width.saturating_sub(pw) / 2;
        let py = fb.height.saturating_sub(ph) / 2;

        fb.draw_shadow(px, py, pw, ph, 0x000000);
        fb.draw_rounded_rect(px, py, pw, ph, 8, gui::Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(px, py, pw, ph, 8, gui::Color::ACCENT_BLUE);

        fb.draw_rect(px, py, pw, 28, gui::Color::TITLE_BAR);
        let prop_title = gui::ui_text("özellikler", "Properties");
        gui::MinGuiCompositor::draw_string(fb, px + 12, py + 7, prop_title, gui::Color::WHITE);

        let close_r = (px + pw.saturating_sub(24), py + 6, 16, 16);
        let mx = mouse.x as usize;
        let my = mouse.y as usize;
        let close_hover = gui::in_rect(mx, my, close_r);
        fb.draw_rect(
            close_r.0,
            close_r.1,
            close_r.2,
            close_r.3,
            if close_hover {
                gui::Color::ACCENT_RED
            } else {
                gui::Color::TITLE_BAR
            },
        );
        gui::MinGuiCompositor::draw_string(
            fb,
            close_r.0 + 4,
            close_r.1 + 1,
            "x",
            gui::Color::WHITE,
        );

        gui::MinGuiCompositor::draw_gear_icon(fb, px + 18, py + 38);

        let item_name = properties_state.1.as_str();
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 44,
            py + 42,
            item_name,
            gui::Color::TEXT_MAIN,
            (px + 44, py + 38, pw.saturating_sub(54), 20),
        );

        fb.draw_rect(
            px + 14,
            py + 66,
            pw.saturating_sub(28),
            1,
            gui::Color::BORDER,
        );

        let label_col = gui::Color::TEXT_MUTED;
        let val_col = gui::Color::TEXT_MAIN;
        let start_y = py + 76;
        let row_gap = 26;

        // Type
        gui::MinGuiCompositor::draw_string(
            fb,
            px + 16,
            start_y,
            gui::ui_text("Tur:", "Type:"),
            label_col,
        );
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 90,
            start_y,
            properties_state.2.as_str(),
            val_col,
            (px + 90, start_y, pw.saturating_sub(100), 20),
        );

        // Size
        gui::MinGuiCompositor::draw_string(
            fb,
            px + 16,
            start_y + row_gap,
            gui::ui_text("Boyut:", "Size:"),
            label_col,
        );
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 90,
            start_y + row_gap,
            properties_state.3.as_str(),
            val_col,
            (px + 90, start_y + row_gap, pw.saturating_sub(100), 20),
        );

        // Location
        gui::MinGuiCompositor::draw_string(
            fb,
            px + 16,
            start_y + row_gap * 2,
            gui::ui_text("Konum:", "Location:"),
            label_col,
        );
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 90,
            start_y + row_gap * 2,
            properties_state.4.as_str(),
            val_col,
            (px + 90, start_y + row_gap * 2, pw.saturating_sub(100), 20),
        );

        // Full Path
        gui::MinGuiCompositor::draw_string(
            fb,
            px + 16,
            start_y + row_gap * 3,
            gui::ui_text("Tam Yol:", "Full Path:"),
            label_col,
        );
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 90,
            start_y + row_gap * 3,
            properties_state.5.as_str(),
            val_col,
            (px + 90, start_y + row_gap * 3, pw.saturating_sub(100), 20),
        );

        let ok_btn_w = 76;
        let ok_btn_h = 24;
        let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;
        let ok_btn_y = py + ph.saturating_sub(34);
        let ok_hover = gui::in_rect(mx, my, (ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h));
        fb.draw_rounded_rect(
            ok_btn_x,
            ok_btn_y,
            ok_btn_w,
            ok_btn_h,
            4,
            if ok_hover {
                gui::Color::ACCENT_BLUE
            } else {
                gui::Color::TITLE_BAR
            },
        );
        fb.draw_rounded_rect_outline(
            ok_btn_x,
            ok_btn_y,
            ok_btn_w,
            ok_btn_h,
            4,
            gui::Color::BORDER,
        );
        let ok_text = gui::ui_text("Tamam", "OK");
        let tw = gui::MinGuiCompositor::text_width(ok_text, 1);
        gui::MinGuiCompositor::draw_string(
            fb,
            ok_btn_x + (ok_btn_w.saturating_sub(tw)) / 2,
            ok_btn_y + 4,
            ok_text,
            gui::Color::WHITE,
        );
    }

    // Run Dialog
    if run_dialog_state.0 {
        let rw = 400usize.min(fb.width.saturating_sub(40));
        let rh = 80;
        let rx = fb.width.saturating_sub(rw) / 2;
        let ry = fb.height.saturating_sub(rh + gui::taskbar_h() + 20);

        fb.draw_shadow(rx, ry, rw, rh, 0x000000);
        fb.draw_rounded_rect(rx, ry, rw, rh, 8, gui::Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(rx, ry, rw, rh, 8, gui::Color::ACCENT_BLUE);

        let prompt = gui::ui_text("Çalıştır:", "Run:");
        gui::MinGuiCompositor::draw_string(fb, rx + 12, ry + 12, prompt, gui::Color::TEXT_MAIN);

        let input_x = rx + 12 + gui::MinGuiCompositor::text_width(prompt, 1) + 8;
        let input_y = ry + 12;
        let input_w = rw.saturating_sub(input_x - rx + 12);
        fb.draw_rounded_rect(input_x, input_y, input_w, 24, 4, gui::Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(input_x, input_y, input_w, 24, 4, gui::Color::BORDER);
        gui::MinGuiCompositor::draw_string(
            fb,
            input_x + 6,
            input_y + 4,
            run_dialog_state.1.as_str(),
            gui::Color::TEXT_MAIN,
        );
    }
}

// Re-export from main.rs to avoid duplication
fn window_content_h(
    app_id: usize,
    _win_height: usize,
    _win_width: usize,
    fm: &apps::filemanager::FileManagerApp,
    _np: &apps::notepad::NotepadApp,
    br: &apps::browser::BrowserApp,
) -> usize {
    match app_id {
        0 => fm.content_height(_win_width),
        2 => 0, // Notepad content height calculation removed
        3 => br.content_height(),
        _ => 0,
    }
}

fn desktop_item_rect(idx: usize) -> (usize, usize, usize, usize) {
    const ICON_W: usize = 76;
    const ICON_H: usize = 80;
    const MARGIN: usize = 12;
    let cols = 4;
    let col = idx % cols;
    let row = idx / cols;
    let x = MARGIN + col * (ICON_W + MARGIN);
    let y = MARGIN + row * (ICON_H + MARGIN);
    (x, y, ICON_W, ICON_H)
}

fn desktop_item_visible(rect: (usize, usize, usize, usize), fb_w: usize, _fb_h: usize) -> bool {
    let (x, y, _w, h) = rect;
    let taskbar_h = crate::gui::taskbar_h();
    x < fb_w && y + h > taskbar_h
}

fn cpu_vendor() -> &'static str {
    "Unknown" // cpuid modülü kullanımdan kaldırıldı
}

fn cpu_logical_count() -> usize {
    1 // online_cpus fonksiyonu kullanımdan kaldırıldı
}

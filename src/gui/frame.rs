//! Ana olay döngüsü durumu ve tam kare çizim rutinleri.
//! `gui::event_loop::run_desktop` bu modüldeki durum/çizim yardımcılarını kullanır.

use crate::apps;
use crate::boot::cpu::{cpu_logical_count, cpu_vendor};
use crate::boot::time::read_cpu_cycles;
use crate::desktop::helpers::{desktop_item_rect, desktop_item_visible, window_content_h};
use crate::drivers;
use crate::gui;

/// Uygulama çizimi için kare bütçesi (ms). Normal uygulamalar için
/// 24 ms sınırlaması yeterlidir; FPS 3D raycasting oyunu için (ID=16)
/// alt satırda özel 80 ms bütçe kullanılır — bkz. render_full_frame.
const RENDER_BUDGET_MS: u64 = 24;

// Ortak yardımcılar `crate::desktop::helpers` ve `crate::boot::cpu` içinde yaşar.

/// Tam kare cizimi. main.rs icindeki eski kapanistan (render_full_frame)
/// gui::event_loop modulune tasindi; cagri noktalari ayni sozlesmeyi kullanir.
pub fn render_full_frame(
    fb: &mut drivers::video::Framebuffer,

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

    fps: &apps::game::GameApp,

    fps_3d: &mut apps::fps_game::FpsGameApp,

    city: &mut apps::city_drive::CityDriveApp,

    now: &drivers::rtc::DateTime,

    mouse: &drivers::mouse::MouseState,

    kbd: &drivers::keyboard::KeyboardState,

    total_ram_bytes: u64,

    used_ram_bytes: u64,

    tick: usize,

    app_render_cycles: &mut [u64; gui::MAX_WINDOWS],

    app_render_time_ms: &[core::cell::Cell<u64>; gui::MAX_WINDOWS],

    run_dialog_state: (bool, &gui::TextBuf),

    properties_state: (
        bool,
        &gui::TextBuf,
        &gui::TextBuf,
        &gui::TextBuf,
        &gui::TextBuf,
        &gui::TextBuf,
    ),

    snap_preview: Option<(usize, usize, usize, usize)>,

    dirty_rect: Option<(usize, usize, usize, usize)>,
) {
    // Kirli dikdortgen verildiyse yalnizca bu bolge cizilip blit edilir;

    // aksi halde tum ekran yeniden cizilir.

    fb.set_frame_clip(dirty_rect);

    // 1. Masaustu Arka Plani

    compositor.render_desktop_bg(fb);

    // /desktop icerigini masaustu simgeleri olarak ciz.

    for i in 0..desktop_fm.file_count {
        let item = &desktop_fm.files[i];

        let (x, y, w, h) = desktop_item_rect(i);

        if !desktop_item_visible((x, y, w, h), fb.width, fb.height) {
            continue;
        }

        if desktop_fm.is_row_selected(i) {
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

        let item_fg = if desktop_fm.is_row_selected(i) {
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

    // 2. Acik pencereleri z-sirasina gore (arkadan one) ciz

    let (order, count) = compositor.draw_order();

    for &idx in order.iter().take(count) {
        if let Some(win) = compositor.windows[idx] {
            if !win.is_open || win.is_minimized {
                continue;
            }

            // Off-screen pencereleri tamamen atla (cizim maliyeti yok).

            if win.x >= fb.width
                || win.y >= fb.height
                || win.x.saturating_add(win.width) == win.x
                || win.y.saturating_add(win.height) == win.y
            {
                continue;
            }

            // Kirli bolgeyle kesismeyen pencereyi hic cizme (golge payi dahil).

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

            // Icerik alanini pencere cercevesine kirp (tasma onleme).

            let (cx, cy, cw, ch) = win.content_rect();

            fb.set_clip(cx, cy, cw, ch);

            // Uygulamaya gore olculen icerik boyutu (kaydirma cubugu gerekli mi?).

            let app_id = compositor.app_id_for_window(idx);

            // Bu pencerenin içerik ölçümü ve çizimi uygulama kodudur;
            // hata bu pencerenin uygulamasına atfedilir. Blok normal
            // biterse kapsam önceki bağlama döner.
            let _app_context = apps::guard::ContextScope::new(app_id, idx);

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

            // Icerik alani (kaydirma cubuklari kadar daraltilmis).

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

            let render_start_ms = crate::monotonic_ms();
            let render_start = read_cpu_cycles();

            match app_id {
                0 => fm.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    &win.scroll,
                    kbd.ctrl,
                ),

                1 => st.render_content(
                    fb,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    kbd.layout,
                    now,
                    total_ram_bytes,
                    cpu_vendor(),
                    cpu_logical_count(),
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

                2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick, &win.scroll),

                3 => br.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),

                4 => iv.render_content(fb, win.x, win.y, win.width, win.height),

                5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),

                6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),

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
                    used_ram_bytes,
                    kbd.layout,
                ),

                15 => fps.render_content(fb, win.x, win.y, win.width, win.height),

                16 => {
                    // FPS 3D raycasting render'ı QEMU/VM gibi yavaş ortamlarda
                    // Duvar raycast'inin ortasında kesilmesi yarım duvar
                    // bırakabildiği için 3D sahneye geniş bir bütçe verilir.
                    let fps3d_budget: u64 = 160;
                    let render_completed = fps_3d.render_content_with_budget(
                        fb,
                        win.x,
                        win.y,
                        win.width,
                        win.height,
                        fps3d_budget,
                    );
                    if render_completed {
                        let frame_ms = crate::monotonic_ms().saturating_sub(render_start_ms);
                        fps_3d.record_completed_frame(frame_ms);
                    }
                }

                17 => {
                    // Şehir sürüşü: raycasting şehir + zemin izdüşümü. Aynı
                    // şekilde 80 ms bütçe kullanılır (yavaş sanal ortamlarda
                    // yarım kare, boş ekrandan iyidir).
                    let city_budget: u64 = 80;
                    city.render_content_with_budget(
                        fb,
                        win.x,
                        win.y,
                        win.width,
                        win.height,
                        city_budget,
                    );
                }

                _ => {}
            }

            let render_elapsed_ms = crate::monotonic_ms().saturating_sub(render_start_ms);
            app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);
            app_render_time_ms[idx].set(render_elapsed_ms);

            // Render workload budget enforcement: uyarı logla.
            // UART yazımı milisaniye düzeyinde pahalıdır; aşan uygulama
            // her karede basılırsa logun kendisi takılmaya neden olur.
            // Bu yüzden uyarı en fazla saniyede bir kez yazılır.
            if render_elapsed_ms > RENDER_BUDGET_MS && idx < gui::MAX_WINDOWS {
                use core::sync::atomic::{AtomicU64, Ordering};
                static LAST_WARN_MS: AtomicU64 = AtomicU64::new(0);
                let warn_now = crate::monotonic_ms();
                if warn_now.wrapping_sub(LAST_WARN_MS.load(Ordering::Relaxed)) >= 1_000 {
                    LAST_WARN_MS.store(warn_now, Ordering::Relaxed);
                    crate::serial_print("[RENDER] Uyarı: render bütçesi aştı: ID=");
                    crate::serial_print_dec(idx as u64);
                    crate::serial_print(" (");
                    crate::serial_print_dec(render_elapsed_ms);
                    crate::serial_print(" ms > ");
                    crate::serial_print_dec(RENDER_BUDGET_MS);
                    crate::serial_print(" ms)\n");
                }
            }

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

        fb.draw_rounded_rect_outline(
            sx + 1,
            sy + 1,
            sw.saturating_sub(2),
            sh.saturating_sub(2),
            4,
            gui::Color::WHITE,
        );
    }

    // 3. Gorev cubugu, sistem tepsisi, popup'lar ve fare imleci

    compositor.render_taskbar_and_overlays(
        fb,
        mouse,
        kbd,
        now,
        st.volume,
        st.is_muted,
        st.language.is_english(),
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

        // Dosya / Klasor Ikonu

        gui::MinGuiCompositor::draw_gear_icon(fb, px + 18, py + 38);

        // oge Adi

        let item_name = properties_state.1.as_str();

        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            px + 44,
            py + 42,
            item_name,
            gui::Color::TEXT_MAIN,
            (px + 44, py + 38, pw.saturating_sub(54), 20),
        );

        // Ayirici cizgi

        fb.draw_rect(
            px + 14,
            py + 66,
            pw.saturating_sub(28),
            1,
            gui::Color::BORDER,
        );

        // Detay Satirlari

        let label_col = gui::Color::TEXT_MUTED;

        let val_col = gui::Color::TEXT_MAIN;

        let start_y = py + 76;

        let row_gap = 26;

        // Tur:

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

        // Boyut:

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

        // Konum:

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

        // Tam Yol:

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

        // Tamam (Kapat) Butonu

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

        gui::MinGuiCompositor::draw_string(
            fb,
            dx + 10,
            dy + 6,
            gui::ui_text("çalıştır (Run)", "Run Program"),
            gui::Color::WHITE,
        );

        gui::MinGuiCompositor::draw_string(
            fb,
            dx + 12,
            dy + 34,
            gui::ui_text("Komut veya uygulama adı:", "Command or app name:"),
            gui::Color::TEXT_MUTED,
        );

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

        gui::MinGuiCompositor::draw_string(
            fb,
            dx + 12,
            dy + 88,
            gui::ui_text(
                "cmd, not, ayar, task, calc, web  |  [Enter] Aç  [Esc] çık",
                "cmd, not, ayar, task, calc, web  |  [Enter] Open  [Esc] Cancel",
            ),
            gui::Color::TEXT_MUTED,
        );
    }

    // 4. Backbuffer'i VRAM'e kopyala.

    //    Kirli dikdortgen verildiyse yalnizca o bolge blit edilir ve mavi

    //    isik filtresi de sadece o bolgeye uygulanir; boylece kalan bolge

    //    onceki karedeki (filtrelenmis) hâlini korur.

    let nl = if st.night_light {
        st.night_light_level
    } else {
        0
    };

    let region = fb.base_clip();

    if dirty_rect.is_some() {
        fb.apply_night_light_rect(nl, region);

        fb.flush_rect((
            region.x0,
            region.y0,
            region.x1 - region.x0,
            region.y1 - region.y0,
        ));
    } else {
        fb.apply_night_light(nl);

        fb.flush();
    }

    fb.clear_frame_clip();
}

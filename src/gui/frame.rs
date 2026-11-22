//! Ana olay döngüsü durumu ve tam kare çizim rutinleri.
//! `gui::event_loop::run_desktop` bu modüldeki durum/çizim yardımcılarını kullanır.

use crate::apps;
use crate::boot::cpu::{cpu_logical_count, cpu_vendor};
use crate::boot::time::read_cpu_cycles;
use crate::desktop::helpers::{
    app_scrolls_natively, desktop_item_rect, desktop_item_visible, window_content_size,
};
use crate::drivers;
use crate::gui;

/// VSync tabanli kare pazarlama aralığı (~60 Hz / kare).
pub const FRAME_VSYNC_INTERVAL_MS: u64 = 16;
/// Boşta / uyku kare pazarlama aralığı (~30 FPS / kare).
pub const FRAME_IDLE_INTERVAL_MS: u64 = 33;

/// VSync (donanim paneli) takip modu etkin mi?
pub fn vsync_enabled() -> bool {
    let caps = crate::drivers::gpu::caps();
    // VirtIO-GPU framebuffer aktarımı VGA'nın 0x3DA dikey-blank portunu
    // kullanmaz. Bu portta beklemek QEMU'da her kareyi gereksiz yere
    // spin-wait'e sokarak fare/klavye gecikmesine neden olur.
    caps.vga_present && !caps.virtio_gpu_present
}

/// Aktif kare pazarlama aralığını hesaplar.
///
/// VSync takip modu, etkileşimli/yayın animasyonu veya VSync aktifse
/// kare başına 16 ms (≈60 Hz) hedefi; aksi halde boşta ~30 FPS (33 ms).
pub fn frame_interval_ms(is_interacting: bool, animating: bool, vsync: bool) -> u64 {
    if vsync || is_interacting || animating {
        FRAME_VSYNC_INTERVAL_MS
    } else {
        FRAME_IDLE_INTERVAL_MS
    }
}

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

    // Not Defteri lazy yüklenir; açık pencere yoksa None.
    np: &mut Option<apps::notepad::NotepadApp>,

    br: &apps::browser::BrowserApp,

    iv: &mut Option<apps::imageviewer::ImageViewerApp>,

    mp: &apps::musicplayer::MusicPlayerApp,

    vp: &mut Option<apps::videoplayer::VideoPlayerApp>,

    tm: &apps::taskmgr::TaskManagerApp,

    calc: &mut Option<apps::calculator::CalculatorApp>,

    paint: &[Option<apps::paint::PaintApp>],

    term: &mut Option<apps::terminal::TerminalApp>,

    inst: &mut Option<apps::installer::InstallerApp>,

    v3d: &mut Option<apps::viewer3d::Viewer3DApp>,

    pkg: &mut Option<apps::pkgmgr::PackageManagerApp>,

    fps: &mut Option<apps::game::GameApp>,

    fps_3d: &mut Option<apps::fps_game::FpsGameApp>,

    city: &mut Option<apps::city_drive::CityDriveApp>,

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

    // Onceki karenin yazilim imlecini backbuffer'dan temizle: imlec pikselleri
    // kalici degildir, her kare temiz arka plan uzerine cizilir. Eski ve yeni
    // imlec alanlari kirli bolgeye eklenir; aksi halde kismi kare guncellemesinde
    // eski imlec ekranda hayalet (ghost) olarak kalir veya imlec kaybolur.
    let cursor_old = fb.restore_cursor_background();
    let mut dirty_rect = match dirty_rect {
        Some(d) => {
            let mut region = d;
            if let Some((cx, cy, cw, ch)) = cursor_old {
                region = drivers::video::rect_union(region, (cx, cy, cw, ch));
            }
            let mx = mouse.x.max(0) as usize;
            let my = mouse.y.max(0) as usize;
            let nx = mx.saturating_sub(3).min(fb.width);
            let ny = my.saturating_sub(3).min(fb.height);
            let nw = 32usize.min(fb.width.saturating_sub(nx));
            let nh = 32usize.min(fb.height.saturating_sub(ny));
            Some(drivers::video::rect_union(region, (nx, ny, nw, nh)))
        }
        // Tam kare her seyi kapsar; eski imlec yine de backbuffer'dan silindi.
        None => None,
    };

    // Duvar kağıdı önbelleği yenilenecekse tüm ekran yeniden çizilir. Önbellek
    // yenilemesi arka planı kırpma bölgesi temizlenerek tüm ekrana çizer ve
    // backbuffer'ın kirli bölge dışındaki kısmını ezer; kısmi çizimde bu,
    // backbuffer ile ekran içeriğini ayırır ve sonraki kısmi blit'lerde
    // ekranda bozulma bırakır.
    if crate::gui::wallpaper::needs_refresh(fb) {
        dirty_rect = None;
    }

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

            // Açılış animasyonu: pencere OPEN_ANIM_MS boyunca
            // OPEN_RISE_PX kadar yukarı kayarak yerine oturur.
            // Ofset yalnızca çizimde uygulanır; durum (win.x/y) değişmez.
            let mut win = win;
            if win.open_ms != 0 && !gui::anim::reduced_motion() {
                let age = crate::monotonic_ms().wrapping_sub(win.open_ms);
                if age < gui::window::OPEN_ANIM_MS {
                    let t = age as f32 / gui::window::OPEN_ANIM_MS as f32;
                    let eased = gui::anim::ease_out_quad(t);
                    win.y = win.y.saturating_add(
                        ((1.0 - eased) * gui::window::OPEN_RISE_PX as f32) as usize,
                    );
                }
            }

            // Off-screen pencereleri tamamen atla (cizim maliyeti yok).

            if win.x >= fb.width
                || win.y >= fb.height
                || win.x.saturating_add(win.width) == win.x
                || win.y.saturating_add(win.height) == win.y
            {
                continue;
            }

            // Kirli bolge varsa yalnizca o bolgeyle kesisen pencereler
            // cizilir. Per pencere render listesi ek tetikleyici olarak
            // kullanilir; ancak listeye kayitli olmasa bile pencerenin kendi
            // sinirlari (golge payi dahil) kirli bolgeyle kesisiyorsa cizilmek
            // zorundadir — aksi halde arka plan yeniden cizimi altinda kalan
            // pencerelerin pikselleri silinmis olur (orn. surukleme sirasinda).

            if let Some((dx, dy, dw, dh)) = dirty_rect {
                let bx0 = win.x.saturating_sub(4);
                let by0 = win.y.saturating_sub(4);
                let bx1 = win.x.saturating_add(win.width).saturating_add(8);
                let by1 = win.y.saturating_add(win.height).saturating_add(8);

                let bounds_overlap = dx < bx1
                    && bx0 < dx.saturating_add(dw)
                    && dy < by1
                    && by0 < dy.saturating_add(dh);

                let tracked_overlap = compositor
                    .window_dirty_rect(idx)
                    .map(|(tx, ty, tw, th)| {
                        let tx1 = tx.saturating_add(tw).saturating_add(8);
                        let ty1 = ty.saturating_add(th).saturating_add(8);
                        dx < tx1
                            && tx < dx.saturating_add(dw)
                            && dy < ty1
                            && ty < dy.saturating_add(dh)
                    })
                    .unwrap_or(false);

                if !bounds_overlap && !tracked_overlap {
                    continue;
                }
            }

            compositor.render_window_frame(fb, &win, mouse, idx);

            // Icerik alanini pencere cercevesine kirp (tasma onleme).

            let (cx, cy, cw, ch) = win.content_rect();

            // Kirli bolge yalnizca cerceve/baslik cubuguyle kesisiyorsa
            // (orn. baslik cubugu hover'i) icerik render'ini atla:
            // uygulama cizimi en pahali kisimdir ve degismedi.
            let content_needs_render = match dirty_rect {
                Some((dx, dy, dw, dh)) => {
                    dx < cx.saturating_add(cw)
                        && cx < dx.saturating_add(dw)
                        && dy < cy.saturating_add(ch)
                        && cy < dy.saturating_add(dh)
                }
                None => true,
            };
            if !content_needs_render {
                continue;
            }

            fb.set_clip(cx, cy, cw, ch);

            // Uygulamaya gore olculen icerik boyutu (kaydirma cubugu gerekli mi?).

            let app_id = compositor.app_id_for_window(idx);

            // Bu pencerenin içerik ölçümü ve çizimi uygulama kodudur;
            // hata bu pencerenin uygulamasına atfedilir. Blok normal
            // biterse kapsam önceki bağlama döner.
            let _app_context = apps::guard::ContextScope::new(app_id, idx);

            let (content_w, content_h) = window_content_size(
                app_id,
                win.width,
                win.height,
                win.content_w,
                win.content_h,
                fm,
                np,
                br,
                st.active_tab,
            );

            // Kendi kaydırmasını yapan uygulamalar ofseti içeride uygular.
            // Diğerleri için içerik, pencere kaydırma ofseti kadar ötelenerek
            // çizilir; böylece her pencere kaydırılabilir olur.
            let native_scroll = app_scrolls_natively(app_id);

            // Kaydırma çubuğu var/yok kararı yalnızca içerik boyutuna bağlıdır.
            // Görünür alan, diğer eksendeki çubuğun kalınlığı kadar küçülür;
            // ofset sınırı da bu daralmış alana göre hesaplanmalıdır.
            let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;
            let want_v = content_h > ch && ch > 0;
            let want_h = content_w > cw && cw > 0;
            let view_w = cw.saturating_sub(if want_v { sb } else { 0 });
            let view_h = ch.saturating_sub(if want_h { sb } else { 0 });

            let max_sx = crate::gui::scrollbar::max_offset(content_w, view_w);
            let max_sy = crate::gui::scrollbar::max_offset(content_h, view_h);

            let (sx, sy) = if native_scroll {
                (0, 0)
            } else {
                (win.scroll.x.min(max_sx), win.scroll.y.min(max_sy))
            };

            let layout_w = content_w.saturating_add(2).max(win.width);
            let layout_h = content_h
                .saturating_add(gui::scaled_titlebar_h() + 1)
                .max(win.height);

            let (rx, ry, rw, rh) = if native_scroll {
                (win.x, win.y, win.width, win.height)
            } else {
                (
                    win.x.saturating_sub(sx),
                    win.y.saturating_sub(sy),
                    layout_w,
                    layout_h,
                )
            };

            let layout_scroll = if native_scroll {
                win.scroll
            } else {
                crate::gui::scrollbar::ScrollState { x: sx, y: sy }
            };

            let scroll_layout = crate::gui::scrollbar::layout(
                (cx, cy, cw, ch),
                content_w,
                content_h,
                layout_scroll,
            );

            // Icerik alani (kaydirma cubuklari kadar daraltilmis).

            let content_area = (
                cx,
                cy,
                scroll_layout.view_w.max(1),
                scroll_layout.view_h.max(1),
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
                0 => fm.render_content(fb, rx, ry, rw, rh, &win.scroll, kbd.ctrl),

                1 => st.render_content(
                    fb,
                    rx,
                    ry,
                    rw,
                    rh,
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

                2 => {
                    if let Some(np) = np.as_mut() {
                        np.render_content(fb, rx, ry, rw, rh, tick, &win.scroll);
                    }
                }

                3 => br.render_content(fb, rx, ry, rw, rh, &win.scroll),

                4 => {
                    if let Some(iv) = iv.as_mut() {
                        iv.render_content(fb, rx, ry, rw, rh);
                    }
                }

                5 => mp.render_content(fb, rx, ry, rw, rh, tick),

                6 => {
                    if let Some(vp) = vp.as_mut() {
                        vp.render_content(fb, rx, ry, rw, rh, tick);
                    }
                }

                7 => tm.render_content(fb, rx, ry, rw, rh),

                8 => {
                    if let Some(calc) = calc.as_mut() {
                        calc.render_content(fb, rx, ry, rw, rh);
                    }
                }

                9 => {
                    if let Some(app) = paint[idx].as_ref() {
                        app.render_content(fb, rx, ry, rw, rh)
                    }
                }

                10 => {
                    if let Some(term) = term.as_mut() {
                        term.render_content(fb, rx, ry, rw, rh, win.is_focused);
                    }
                }

                11 => {
                    if let Some(inst) = inst.as_mut() {
                        inst.render_content(fb, rx, ry, rw, rh);
                    }
                }

                12 => {
                    if let Some(v3d) = v3d.as_mut() {
                        v3d.render_content(fb, rx, ry, rw, rh);
                    }
                }

                13 => {
                    if let Some(pkg) = pkg.as_mut() {
                        pkg.render_content(fb, rx, ry, rw, rh);
                    }
                }

                14 => apps::controlpanel::render_content(
                    fb,
                    rx,
                    ry,
                    rw,
                    rh,
                    now,
                    total_ram_bytes,
                    used_ram_bytes,
                    kbd.layout,
                ),

                15 => {
                    if let Some(fps_app) = fps.as_mut() {
                        fps_app.render_content(fb, rx, ry, rw, rh);
                    }
                }

                16 => {
                    // FPS 3D raycasting render'ı QEMU/VM gibi yavaş ortamlarda
                    // Duvar raycast'inin ortasında kesilmesi yarım duvar
                    // bırakabildiği için 3D sahneye geniş bir bütçe verilir.
                    let fps3d_budget: u64 = 160;
                    if let Some(fps3d) = fps_3d.as_mut() {
                        let render_completed =
                            fps3d.render_content_with_budget(fb, rx, ry, rw, rh, fps3d_budget);
                        if render_completed {
                            let frame_ms = crate::monotonic_ms().saturating_sub(render_start_ms);
                            fps3d.record_completed_frame(frame_ms);
                        }
                    }
                }

                17 => {
                    // Şehir sürüşü: raycasting şehir + zemin izdüşümü. Aynı
                    // şekilde 80 ms bütçe kullanılır (yavaş sanal ortamlarda
                    // yarım kare, boş ekrandan iyidir).
                    let city_budget: u64 = 80;
                    if let Some(city_app) = city.as_mut() {
                        city_app.render_content_with_budget(fb, rx, ry, rw, rh, city_budget);
                    }
                }

                _ => {}
            }

            let render_elapsed_ms = crate::monotonic_ms().saturating_sub(render_start_ms);
            app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);
            app_render_time_ms[idx].set(render_elapsed_ms);

            // Render workload budget enforcement: uyarı logla.
            // Oyun pencerelerinin (16=FPS3D, 17=Şehir) kendi geniş
            // render bütçeleri vardır; genel 24 ms eşiği onlarda yanlış
            // pozitif üretirdi. Eşik uygulamanın fiilen verilen bütçesi.
            let app_budget_ms = match app_id {
                16 => 160,
                17 => 80,
                _ => RENDER_BUDGET_MS,
            };
            if render_elapsed_ms > app_budget_ms && idx < gui::MAX_WINDOWS {
                static mut LAST_WARN_MS: [u64; gui::MAX_WINDOWS] = [0; gui::MAX_WINDOWS];
                let now_ms = crate::monotonic_ms();
                let last = unsafe { LAST_WARN_MS[idx] };
                if now_ms.wrapping_sub(last) >= 1_000 {
                    unsafe {
                        LAST_WARN_MS[idx] = now_ms;
                    }
                    crate::serial_print("[RENDER] Uyarı: render bütçesi aştı: ID=");
                    crate::serial_print_dec(idx as u64);
                    crate::serial_print(" (");
                    crate::serial_print_dec(render_elapsed_ms);
                    crate::serial_print(" ms > ");
                    crate::serial_print_dec(app_budget_ms);
                    crate::serial_print(" ms)\n");
                }
            }

            // Kirpmayi kaldir ve kaydirma cubuklarini pencerenin icinde ciz.

            fb.reset_clip();

            // Çizim ve isabet testi aynı `ScrollbarLayout` üzerinden yapılır.
            // Tutamak üzerine gelince vurgulanır (hover geri bildirimi).
            let smx = mouse.x as usize;
            let smy = mouse.y as usize;
            let sb_hover = |r: (usize, usize, usize, usize)| {
                smx >= r.0 && smx < r.0 + r.2 && smy >= r.1 && smy < r.1 + r.3
            };
            if let Some(g) = scroll_layout.vertical {
                crate::gui::scrollbar::draw_vertical(fb, &g, sb_hover(g.thumb));
            }
            if let Some(g) = scroll_layout.horizontal {
                crate::gui::scrollbar::draw_horizontal(fb, &g, sb_hover(g.thumb));
            }
        }
    }

    // Pencere cizimi kirli bolgeyle kismiydi; artik ust katmanlar (gorev
    // cubugu, popup'lar, modaller) tam ekranda cizilmesi icin kiralma
    // tamamen kaldirilir. Kirli bolge sadece flush aninda uygulanir.
    fb.clear_frame_clip();

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
        tick,
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
        let dw = 320usize.min(fb.width.saturating_sub(16)).max(160);
        let dh = 110usize.min(fb.height.saturating_sub(16)).max(80);
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

        if dh >= 100 {
            gui::MinGuiCompositor::draw_string_clipped(
                fb,
                dx + 12,
                dy + 34,
                gui::ui_text("Komut veya uygulama adı:", "Command or app name:"),
                gui::Color::TEXT_MUTED,
                (dx + 6, dy + 32, dw.saturating_sub(12), 16),
            );
        }

        // Giris kutusu
        let bx = dx + 12;
        let by = if dh < 100 { dy + 30 } else { dy + 54 };
        let bw = dw.saturating_sub(24);
        let bh = 24;

        fb.draw_rect(bx, by, bw, bh, gui::Color::DARK_BG);
        fb.draw_rect_outline(bx, by, bw, bh, gui::Color::BORDER);

        let input_text = run_dialog_state.1.as_str();
        gui::MinGuiCompositor::draw_string_clipped(
            fb,
            bx + 6,
            by + 6,
            input_text,
            gui::Color::TEXT_MAIN,
            (bx + 4, by + 2, bw.saturating_sub(8), 20),
        );

        // Imlec
        let cur_x = bx + 6 + gui::MinGuiCompositor::text_width(input_text, 1);
        if cur_x + 2 <= bx + bw && crate::gui::textedit::caret_visible(tick) {
            fb.draw_rect(cur_x, by + 4, 2, 16, gui::Color::ACCENT_YELLOW);
        }

        // Ipucu
        if dh >= 100 {
            gui::MinGuiCompositor::draw_string_clipped(
                fb,
                dx + 12,
                dy + 88,
                gui::ui_text(
                    "cmd, not, ayar, task, calc, web  |  [Enter] Aç  [Esc] çık",
                    "cmd, not, ayar, task, calc, web  |  [Enter] Open  [Esc] Cancel",
                ),
                gui::Color::TEXT_MUTED,
                (dx + 6, dy + 86, dw.saturating_sub(12), 16),
            );
        }
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

    // Kirli bolge varsa cikis oncesi kiralmayi o bolgeye kısıtla: overlay'ler
    // tam ekranda cizildi ama VRAM'e sadece degisen bolge aktarilacak.
    if let Some((dx, dy, dw, dh)) = dirty_rect {
        fb.set_frame_clip(Some((dx, dy, dw, dh)));
    }

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

    // Fare imleci kare başına **tek yerde** ve en son çizilir: kırpma bölgesi
    // temizlendikten ve gece ışığı filtresi uygulandıktan sonra. Böylece
    // imleç ne kırpma bölgesine takılır ne de filtreli/filtersiz iki kez
    // ekrana basılır.
    //
    // Tam/kirli kare çizimi eski yazılım imlecini geri yükler. İmleci burada
    // yeniden çizmeden çıkılırsa imleç bir sonraki fare olayına kadar kaybolur;
    // daha kötüsü, sonraki kısmi kare eski arka plan kaydını yanlış bölgeye
    // uygulayarak iz ve ekran bozulması üretebilir. İçeriği flush ettikten
    // sonra imleci ekranın mevcut backbuffer'ı üzerine kaydet ve yalnızca
    // imleç alanını aktar.
    compositor.draw_mouse_cursor(
        fb,
        mouse.x.max(0) as usize,
        mouse.y.max(0) as usize,
        mouse.left_button,
    );
    if let Some(cursor_rect) = fb.cursor_rect() {
        fb.flush_rect(cursor_rect);
    }

    // Frame pacing: donanim VSync bayragini bekle (vsync_wait); VSync takip
    // modunda kare vblank uyumunda ~60 Hz kare pazarlanir. Desteklenmeyen
    // olayda mevcut donanim yolu devre disi kalir, kisa spin ust siniri de.

    if vsync_enabled() {
        crate::drivers::gpu::vsync_wait();
    }
}

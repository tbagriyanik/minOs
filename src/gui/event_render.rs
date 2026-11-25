//! Render bölümü: tam kare çizimi, kirli bölge takimi, imlec guncellemesi ve
//! bosta bekleme mantigini icerir.

use crate::apps;
use crate::boot::progress::BUSY_DIRTY;
use crate::boot::time::{read_cpu_cycles, TSC_CYCLES_PER_US};
use crate::desktop::helpers::{app_is_focused, focused_window_for_app, keep_windows_on_screen};
use crate::drivers;
use crate::drivers::ps2::InputUpdate;
use crate::gui;
use crate::mm::MemoryManager;
use crate::watchdog;

const CPU_WARN_PCT: usize = 90;
const RAM_WARN_PCT: usize = 90;

/// Tam kare cizimi ve imlec guncellemesini gerceklestirir.
///
/// Bu fonksiyon, olay dongusunun render kismini (full_redraw hesaplama,
/// render_full_frame cagrisi, imlec-only guncelleme, bosta bekleme ve CPU
/// ornekleme) tek bir fonksiyon olarak toplar.
///
/// # Parametreler
/// * `state` - Döngü durumu (DesktopLoopState)
/// * `fb` - Framebuffer
/// * `compositor` - GUI kompozitörü
/// * Tüm uygulama referansları
/// * `now` - Şu anki tarih/saat
/// * `mouse`, `keyboard` - Girdi durumları
/// * `memory_mgr` - Fiziksel bellek yöneticisi
/// * `input_update` - Girdi güncellemesi
/// * `timer_tick_fired` - Zamanlayıcı işareti
/// * `mouse_moved` - Fare hareketi işareti
/// * `buttons_changed` - Fare düğmesi değişimi işareti
/// * `is_interacting` - Etkileşim devam ediyor mu
/// * `media_animating` - Medya animasyonu devam ediyor mu
/// * `scroll_val` - Fare tekerlek değeri
/// * `mx`, `my` - Fare konumu
/// * `width`, `height` - Ekran boyutları
/// * `active_snap_preview` - Aero Snap önizleme
pub fn render_frame(
    state: &mut crate::gui::desktop_loop_state::DesktopLoopState,
    fb: &mut drivers::video::Framebuffer,
    compositor: &mut gui::MinGuiCompositor,
    filemanager_app: &mut apps::filemanager::FileManagerApp,
    desktop_files: &mut apps::filemanager::FileManagerApp,
    settings_app: &mut apps::settings::SystemSettings,
    notepad_app: &mut Option<apps::notepad::NotepadApp>,
    browser_app: &mut apps::browser::BrowserApp,
    imageviewer_app: &mut Option<apps::imageviewer::ImageViewerApp>,
    musicplayer_app: &mut apps::musicplayer::MusicPlayerApp,
    videoplayer_app: &mut Option<apps::videoplayer::VideoPlayerApp>,
    taskmgr_app: &mut apps::taskmgr::TaskManagerApp,
    calculator_app: &mut Option<apps::calculator::CalculatorApp>,
    paint_apps: &mut [Option<apps::paint::PaintApp>],
    terminal_app: &mut Option<apps::terminal::TerminalApp>,
    installer_app: &mut Option<apps::installer::InstallerApp>,
    viewer3d_app: &mut Option<apps::viewer3d::Viewer3DApp>,
    pkgmgr_app: &mut Option<apps::pkgmgr::PackageManagerApp>,
    fps_game_app: &mut Option<apps::game::GameApp>,
    fps_3d_app: &mut Option<apps::fps_game::FpsGameApp>,
    city_drive_app: &mut Option<apps::city_drive::CityDriveApp>,
    now: &crate::drivers::rtc::DateTime,
    mouse: &crate::drivers::mouse::MouseState,
    keyboard: &crate::drivers::keyboard::KeyboardState,
    memory_mgr: &mut MemoryManager,
    fs_mgr: &mut crate::fs::FileSystemManager,
    input_update: &InputUpdate,
    timer_tick_fired: bool,
    mouse_moved: bool,
    buttons_changed: bool,
    is_interacting: bool,
    media_animating: bool,
    scroll_val: isize,
    mx: usize,
    my: usize,
    width: usize,
    height: usize,
) {
    let night_light_active = settings_app.night_light;

    // Ayarlar > Testler: fare test verisini canli besle.
    let settings_test_active = app_is_focused(compositor, 1) && settings_app.active_tab == 4;

    if settings_test_active {
        if let Some((_, w)) = focused_window_for_app(compositor, 1) {
            settings_app.capture_mouse_test(
                mx,
                my,
                mouse.left_button,
                mouse.right_button,
                mouse.middle_button,
                w.x.saturating_sub(w.scroll.x),
                w.y.saturating_sub(w.scroll.y),
            );
        }
    }

    let busy_dirty = unsafe {
        let dirty = BUSY_DIRTY;
        BUSY_DIRTY = false;
        dirty
    };

    // Secim/konum degistiyse Hizli onizleme icerigini tazele.
    if filemanager_app.preview_open {
        filemanager_app.update_preview(&fs_mgr.vfs);
    }

    // Tarayıcı: yeni sayfa yüklendiğinde kaydırmayı başa al.
    let browser_scroll_reset = browser_app.scroll_reset;
    if browser_app.scroll_reset {
        browser_app.scroll_reset = false;
        browser_app.scroll_animating = false;
        browser_app.scroll_target = 0;
        if let Some((idx, w)) = focused_window_for_app(compositor, 3) {
            if let Some(wm) = compositor.windows[idx].as_mut() {
                wm.scroll.y = 0;
            }
            if let Some(y) = browser_app.pending_anchor_y.take() {
                let view_h = w.content_rect().3;
                let max_y = browser_app.content_height().saturating_sub(view_h);
                browser_app.scroll_to(0, y, max_y, crate::monotonic_ms());
            }
        } else {
            browser_app.pending_anchor_y = None;
        }
    }

    // Tarayıcı: yumuşak içerik kaydırmasını kare hızında ilerlet.
    let mut browser_scroll_step = false;
    if browser_app.scroll_animating && timer_tick_fired {
        if let Some((idx, w)) = focused_window_for_app(compositor, 3) {
            let view_h = w.content_rect().3;
            let max_y = browser_app.content_height().saturating_sub(view_h);
            if let Some(y) = browser_app.scroll_anim_step(crate::monotonic_ms(), max_y) {
                browser_scroll_step = true;
                if let Some(wm) = compositor.windows[idx].as_mut() {
                    wm.scroll.y = y.min(max_y);
                }
            }
        } else {
            browser_app.scroll_animating = false;
        }
    }
    let browser_scrolling = browser_scroll_step || browser_app.scroll_animating;

    // Genel yumuşak kaydırma.
    let mut generic_scrolling = false;
    if timer_tick_fired {
        let now_ms = crate::monotonic_ms();
        for (idx, slot) in compositor.windows.iter_mut().enumerate() {
            let Some(wm) = slot.as_mut() else {
                continue;
            };
            if !wm.scroll_anim.is_active()
                || !wm.is_open
                || wm.is_minimized
                || wm.workspace != compositor.active_workspace
                || wm.app_id == 3
            {
                continue;
            }
            let app_id = wm.app_id as usize;
            let (win_w, win_h, rep_w, rep_h) = (wm.width, wm.height, wm.content_w, wm.content_h);
            let (content_w, content_h) = crate::desktop::helpers::window_content_size(
                app_id,
                win_w,
                win_h,
                rep_w,
                rep_h,
                filemanager_app,
                notepad_app,
                browser_app,
                settings_app.active_tab,
            );
            let (_, _, cw, ch) = wm.content_rect();
            let sb = gui::scrollbar::SCROLLBAR_THICKNESS;
            let want_v = content_h > ch && ch > 0;
            let want_h = content_w > cw && cw > 0;
            let view_w = cw.saturating_sub(if want_v { sb } else { 0 });
            let view_h = ch.saturating_sub(if want_h { sb } else { 0 });
            let max_x = gui::scrollbar::max_offset(content_w, view_w);
            let max_y = gui::scrollbar::max_offset(content_h, view_h);
            if let Some(s) = wm.scroll_anim.step(now_ms, max_x, max_y) {
                wm.scroll = s;
                let (dx, dy, dw, dh) = wm.content_rect();
                generic_scrolling = true;
                compositor
                    .dirty_tracker
                    .track(idx, crate::gui::dirty_rect::DirtyRect::new(dx, dy, dw, dh));
            }
        }
    }

    // Tooltip durumu.
    compositor.tooltip_tick(crate::monotonic_ms(), width, height);

    // Yeni açılan pencereleri açılış animasyonu için damgala.
    {
        let now_ms = crate::monotonic_ms();
        for w in compositor.windows.iter_mut().flatten() {
            if w.is_open && w.open_ms == 0 {
                w.open_ms = now_ms;
            }
        }
    }

    // Başlat menüsü açılış animasyonu.
    let start_menu_animating = compositor.is_start_menu_open
        && crate::monotonic_ms().wrapping_sub(compositor.start_menu_open_ms)
            < gui::MinGuiCompositor::START_MENU_OPEN_MS;

    // Pencere açılış animasyonu.
    let window_open_animating = !gui::anim::reduced_motion()
        && compositor.windows.iter().any(|slot| {
            slot.is_some_and(|w| {
                w.is_open
                    && w.open_ms != 0
                    && crate::monotonic_ms().wrapping_sub(w.open_ms) < gui::window::OPEN_ANIM_MS
            })
        });

    // Bağlam menüsü, Alt+Tab ve tepsi açılır pencereleri.
    let popup_anim_ms = crate::gui::window::OPEN_ANIM_MS;
    let overlay_animating = !gui::anim::reduced_motion()
        && ((compositor.is_context_menu_open
            && crate::monotonic_ms().wrapping_sub(compositor.context_menu_open_ms)
                < popup_anim_ms)
            || (compositor.is_alt_tab_open
                && crate::monotonic_ms().wrapping_sub(compositor.alt_tab_open_ms) < popup_anim_ms)
            || (compositor.is_calendar_open
                && crate::monotonic_ms().wrapping_sub(compositor.calendar_open_ms)
                    < popup_anim_ms)
            || (compositor.is_volume_popup_open
                && crate::monotonic_ms().wrapping_sub(compositor.volume_popup_open_ms)
                    < popup_anim_ms));

    let full_redraw = input_update.keyboard
        || buttons_changed
        || scroll_val != 0
        || browser_scrolling
        || generic_scrolling
        || start_menu_animating
        || window_open_animating
        || overlay_animating
        || browser_scroll_reset
        || state.force_render
        || busy_dirty
        || !compositor.dirty_tracker.is_empty()
        || (mouse_moved
            && (is_interacting
                || night_light_active
                || settings_test_active
                || compositor.is_start_menu_open))
        || (timer_tick_fired && media_animating);

    if full_redraw {
        let tm_open = compositor
            .windows
            .iter()
            .flatten()
            .any(|w| w.app_id == 7 && w.is_open && !w.is_minimized);

        if tm_open {
            use core::sync::atomic::{AtomicUsize, Ordering};

            static TM_UPDATE_TICK: AtomicUsize = AtomicUsize::new(0);

            let tick = TM_UPDATE_TICK.fetch_add(1, Ordering::Relaxed);

            if tick % 30 == 0 {
                let _app_context = apps::guard::ContextScope::new(7, apps::guard::NO_WINDOW);
                taskmgr_app.update(
                    &memory_mgr.pmm,
                    &fs_mgr.vfs,
                    &compositor.windows,
                    &state.app_render_cycles,
                );
            }
        }

        keep_windows_on_screen(compositor, width, height);

        let tracked_dirty = compositor.take_dirty_region();

        let target_dirty = if state.force_render
            || busy_dirty
            || compositor.is_start_menu_open
            || compositor.is_context_menu_open
            || compositor.is_alt_tab_open
            || compositor.active_workspace != state.last_workspace
            || state.run_dialog_open
            || state.properties_open
            || night_light_active
        {
            None
        } else {
            // Sürükleme dikdörtgeni ile izlenen kirli bölge BİRLİKTE
            // kullanılır. Yalnızca biri seçilirse diğerindeki değişiklik
            // bu karede ekrana yansımaz ve pencere parça parça güncellenir.
            let merged = crate::gui::dirty_rect::merge_regions(
                state
                    .drag_dirty_rect
                    .map(|r| crate::gui::dirty_rect::DirtyRect::new(r.0, r.1, r.2, r.3)),
                tracked_dirty.map(|r| crate::gui::dirty_rect::DirtyRect::new(r.0, r.1, r.2, r.3)),
            );
            merged.map(|r| (r.x, r.y, r.width, r.height))
        };
        state.last_workspace = compositor.active_workspace;

        gui::frame::render_full_frame(
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
            city_drive_app,
            now,
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
            state.active_snap_preview,
            target_dirty,
        );

        // İzleme damgası: her karede açık pencereleri canlı olarak işaretle.
        for (idx, slot) in compositor.windows.iter().enumerate() {
            if let Some(win) = slot {
                if win.is_open && !win.is_minimized {
                    watchdog::heartbeat(idx, win.app_id as usize);
                }
            }
        }

        state.drag_dirty_rect = None;
    } else if mouse_moved {
        // Imleç hareketinde sadece imleç alanları güncellenir.
        //
        // `restore_cursor_background`, imleç çizilmeden *önce* saklanan temiz
        // arka planı geri yükler (eski imleç ekrandan silinir). Ardından yeni
        // imleç çizilir ve yeni konumun temiz arka planı saklanır. Bu iki
        // adım tek karede yalnızca bir kez çalışmalıdır; imleç iki kez
        // çizilirse arka plan yedeğine imleç pikselleri karışır ve ekranda
        // hayalet (ghost) imleç izleri oluşur.
        let old_rect = fb.restore_cursor_background();

        compositor.draw_mouse_cursor(fb, mx, my, mouse.left_button);

        let new_rect = fb.cursor_rect();

        match (old_rect, new_rect) {
            (Some(o), Some(n)) => {
                let u = drivers::video::rect_union(o, n);
                // LFB'ye yazmadan önce dikey-blank'ı bekle: yalnızca fare
                // hareket ederken de yazma taranan ekranın ortasına
                // düşerse ekran yırtılması oluşur.
                if crate::drivers::gpu::vblank_available() || crate::gui::frame::vsync_enabled() {
                    crate::drivers::gpu::vsync_wait();
                }
                fb.flush_rect(u);
            }
            (Some(o), None) => fb.flush_rect(o),
            (None, Some(n)) => fb.flush_rect(n),
            _ => {}
        }
    }

    state.last_mouse_x = mouse.x;
    state.last_mouse_y = mouse.y;
    state.last_buttons = (mouse.left_button, mouse.right_button);

    // Bosta bekleme.
    if !input_update.mouse
        && !input_update.keyboard
        && !mouse_moved
        && !buttons_changed
        && !state.force_render
    {
        let wait_start = read_cpu_cycles();

        let browser_waiting = browser_app.take_net_idle_hint();

        if !browser_waiting {
            crate::arch::x86_64::apic::idle_pause();
        } else {
            let idle_wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(20_000);
            let busy_wait_cycles = if browser_waiting {
                unsafe { TSC_CYCLES_PER_US }.saturating_mul(2_000)
            } else {
                idle_wait_cycles
            };

            while read_cpu_cycles().wrapping_sub(wait_start) < busy_wait_cycles {
                if drivers::ps2::has_input() || drivers::usb_hid::has_event() {
                    break;
                }
                core::hint::spin_loop();
            }
        }

        let loop_end_cycles = read_cpu_cycles();
        let elapsed = loop_end_cycles.wrapping_sub(wait_start);

        state.cpu_sample_total_cycles = state.cpu_sample_total_cycles.saturating_add(elapsed);
        state.cpu_sample_active_cycles = state.cpu_sample_active_cycles.saturating_add(elapsed);
    } else {
        let active_wait_start = read_cpu_cycles();
        // Girdi geldikten sonra uzun spin-wait fare raporlarını biriktirip
        // imleci geciktiriyordu. Bir sonraki USB/PS2 raporunu beklemek için
        // yalnızca kısa bir pencere bırak; render döngüsü geri kalan zamanı
        // doğrudan yeni girdiyi toplamaya ayırsın.
        let active_wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(20);
        while read_cpu_cycles().wrapping_sub(active_wait_start) < active_wait_cycles {
            core::hint::spin_loop();
        }

        let elapsed = read_cpu_cycles().wrapping_sub(active_wait_start);

        state.cpu_sample_total_cycles = state.cpu_sample_total_cycles.saturating_add(elapsed);
        state.cpu_sample_active_cycles = state.cpu_sample_active_cycles.saturating_add(elapsed);
    }

    let now_cycles = read_cpu_cycles();
    let sample_window = unsafe { TSC_CYCLES_PER_US }.saturating_mul(250_000);

    if now_cycles.wrapping_sub(state.cpu_sample_start) >= sample_window {
        let bsp_pct = state
            .cpu_sample_active_cycles
            .saturating_mul(100)
            .checked_div(state.cpu_sample_total_cycles)
            .unwrap_or(0)
            .min(100) as usize;

        taskmgr_app.cpu_usage_pct = bsp_pct;

        let cpus = crate::arch::x86_64::smp::cpu_count().min(crate::arch::x86_64::smp::MAX_CPUS);

        taskmgr_app.core_count = cpus;
        taskmgr_app.core_usage_pct[0] = bsp_pct;

        for c in 1..cpus {
            let hb = crate::arch::x86_64::smp::heartbeat(c);
            let diff = hb.wrapping_sub(state.last_ap_heartbeats[c]);
            state.last_ap_heartbeats[c] = hb;

            taskmgr_app.core_usage_pct[c] = if crate::arch::x86_64::smp::is_ap_online(c) {
                if diff > 0 {
                    ((diff.min(50) * 100) / 50).min(100) as usize
                } else {
                    0
                }
            } else {
                0
            };
        }

        state.cpu_sample_start = now_cycles;
        state.cpu_sample_total_cycles = 0;
        state.cpu_sample_active_cycles = 0;

        // --- Kaynak uyarıları: CPU ve RAM ---
        if bsp_pct >= CPU_WARN_PCT {
            if !state.cpu_warn_shown {
                crate::serial_print("[WARN] CPU kullanımı yüksek: ");
                crate::serial_print_dec(bsp_pct as u64);
                crate::serial_print("%\n");
                state.cpu_warn_shown = true;
            }
        } else {
            state.cpu_warn_shown = false;
        }

        let total_ram = memory_mgr.pmm.total_memory_bytes;
        let used_ram = memory_mgr.pmm.used_memory_bytes;
        if total_ram > 0 {
            let ram_pct = used_ram
                .saturating_mul(100)
                .checked_div(total_ram)
                .unwrap_or(0) as usize;
            if ram_pct >= RAM_WARN_PCT {
                if !state.ram_warn_shown {
                    crate::serial_print("[WARN] RAM kullanımı yüksek: ");
                    crate::serial_print_dec(ram_pct as u64);
                    crate::serial_print("%\n");
                    state.ram_warn_shown = true;
                }
            } else {
                state.ram_warn_shown = false;
            }
        }
    }
}

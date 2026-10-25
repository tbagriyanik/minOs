use crate::apps;
use crate::boot;
use crate::boot::console::{serial_print, serial_print_dec};
use crate::boot::time::monotonic_ms;
use crate::drivers;
use crate::drivers::video::Framebuffer;
use crate::fs;
use crate::gui;
use crate::mm;

pub fn run_welcome_screen(
    fb: &mut Framebuffer,
    width: usize,
    height: usize,
    mouse: &mut drivers::mouse::MouseState,
    keyboard: &mut drivers::keyboard::KeyboardState,
    settings_app: &mut apps::settings::SystemSettings,
    compositor: &gui::MinGuiCompositor,
    fs_mgr: &mut fs::FileSystemManager,
    memory_mgr: &mut mm::MemoryManager,
) {
    let mut welcome_was_left_down = false;
    let mut welcome_dirty = true;
    let mut welcome_mouse_pos = (mouse.x, mouse.y);
    let mut welcome_accepted = false;
    let mut welcome_announced = false;
    let mut welcome_diag_last = monotonic_ms();
    let mut _welcome_diag_iters: u64 = 0;
    let mut welcome_diag_last_trace: u64 = 0;
    let mut welcome_start_time = monotonic_ms();
    let welcome_timeout_ms: u64 = 5_000; // 5 saniye içinde etkileşim olmazsa masaüstüne otomatik geç

    loop {
        if !welcome_announced {
            serial_print("[BOOT] Hoş geldin döngüsü: Enter/ESC/tıklama bekleniyor (5 sn zaman aşımı etkin).\n");
            welcome_announced = true;
        }

        if welcome_dirty && !boot::text_console::is_active() {
            gui::render_welcome_screen(
                fb,
                width,
                height,
                settings_app.language.is_english(),
                settings_app.theme,
                mouse,
            );
            welcome_dirty = false;
        }

        let mut welcome_update =
            drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);

        let welcome_usb_update = drivers::usb_hid::poll_input(
            &mut *mouse,
            &mut *keyboard,
            width as i32,
            height as i32,
            &mut memory_mgr.pmm,
        );

        welcome_update.keyboard |= welcome_usb_update.keyboard;
        welcome_update.mouse |= welcome_usb_update.mouse;

        if welcome_update.keyboard || welcome_update.mouse {
            welcome_start_time = monotonic_ms();
        }

        let (_tr_st, _tr_d, tr_n) = drivers::ps2::trace_last();
        if tr_n != welcome_diag_last_trace {
            welcome_diag_last_trace = tr_n;
        }

        _welcome_diag_iters += 1;
        let welcome_now = monotonic_ms();
        if welcome_now.wrapping_sub(welcome_diag_last) >= 5000 {
            welcome_diag_last = welcome_now;
            serial_print("[BOOT] Hoş geldin döngüsü: geçen süre (ms)=");
            serial_print_dec(welcome_now.wrapping_sub(welcome_start_time) as u64);
            serial_print(" yineleme=");
            serial_print_dec(_welcome_diag_iters as u64);
            serial_print("\n");
        }

        // 5 saniye zaman aşımı: USB klavye veya fare algılanamasa bile sistemi kilitleme, doğrudan masaüstüne geç.
        if welcome_now.wrapping_sub(welcome_start_time) >= welcome_timeout_ms {
            serial_print("[BOOT] Hoş geldin zaman aşımı; otomatik olarak masaüstüne geçiliyor.\n");
            welcome_accepted = true;
        }

        // Enter veya Escape ile hoş geldin ekranını geç.
        while let Some(action) = keyboard.take_last_action() {
            if matches!(action, drivers::keyboard::KeyAction::EnterKey) {
                welcome_accepted = true;
            }
        }

        while let Some(ch) = keyboard.take_last_char() {
            if ch == 'e' || ch == 'E' {
                // E tuşu ile boot mesajlarını göster/gizle.
                unsafe {
                    if !boot::text_console::is_active() {
                        boot::text_console::save_screen();
                        boot::text_console::toggle_active();
                        serial_print(
                            "[BOOT] Önyükleme mesajları gösteriliyor (E tuşuyla geri dön)...\n",
                        );
                    } else {
                        boot::text_console::toggle_active();
                        boot::text_console::restore_screen();
                        serial_print("[BOOT] GUI'ye geri dönüldü.\n");
                        welcome_dirty = true;
                    }
                }
            }
            if ch == '\x1b' || ch == ' ' {
                welcome_accepted = true;
            }
        }

        if welcome_accepted {
            settings_app.save_all(compositor, &mut fs_mgr.vfs);
            break;
        }

        if welcome_mouse_pos != (mouse.x, mouse.y) {
            welcome_mouse_pos = (mouse.x, mouse.y);
            welcome_dirty = true;
        }

        let clicked = mouse.left_button && !welcome_was_left_down;
        welcome_was_left_down = mouse.left_button;
        if !clicked {
            continue;
        }

        let mx = mouse.x.max(0) as usize;
        let my = mouse.y.max(0) as usize;
        let point_in = |rect: (usize, usize, usize, usize)| {
            mx >= rect.0 && my >= rect.1 && mx < rect.0 + rect.2 && my < rect.1 + rect.3
        };

        let mut selection_changed = false;
        for i in 0..2 {
            if point_in(gui::welcome_language_rect(width, height, i)) {
                settings_app.language = if i == 0 {
                    apps::settings::UiLanguage::Turkish
                } else {
                    apps::settings::UiLanguage::English
                };
                gui::set_ui_language_english(settings_app.language.is_english());
                selection_changed = true;
                break;
            }
        }

        if !selection_changed {
            for (i, id) in gui::theme::ThemeId::ALL.iter().enumerate() {
                if point_in(gui::welcome_theme_rect(width, height, i)) {
                    settings_app.theme = *id;
                    gui::theme::set_theme(*id);
                    selection_changed = true;
                    break;
                }
            }
        }

        if selection_changed {
            welcome_dirty = true;
        } else if point_in(gui::welcome_continue_rect(width, height)) {
            settings_app.save_all(compositor, &mut fs_mgr.vfs);
            break;
        }

        if !welcome_update.mouse && !welcome_update.keyboard {
            for _ in 0..1_000 {
                core::hint::spin_loop();
            }
        }
    }
}

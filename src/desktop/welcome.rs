use crate::apps;
use crate::boot;
use crate::boot::console::{serial_print, serial_print_dec};
use crate::boot::time::monotonic_ms;
use crate::drivers;
use crate::drivers::video::Framebuffer;
use crate::fs;
use crate::gui;
use crate::mm;

fn activate_welcome_control(
    focused_control: usize,
    settings_app: &mut apps::settings::SystemSettings,
    accepted: &mut bool,
) {
    if focused_control < 2 {
        settings_app.language = if focused_control == 0 {
            apps::settings::UiLanguage::Turkish
        } else {
            apps::settings::UiLanguage::English
        };
        gui::set_ui_language_english(settings_app.language.is_english());
    } else if let Some(theme) = gui::theme::ThemeId::ALL.get(focused_control - 2) {
        settings_app.theme = *theme;
        gui::theme::set_theme(*theme);
    } else {
        *accepted = true;
    }
}

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
    let original_language = settings_app.language;
    let original_theme = settings_app.theme;
    let mut welcome_was_left_down = false;
    let mut welcome_dirty = true;
    let mut welcome_focus = 0;
    let welcome_focus_count = 3 + gui::theme::ThemeId::ALL.len();
    let mut welcome_mouse_pos = (mouse.x, mouse.y);
    let mut welcome_accepted = false;
    let mut welcome_cancelled = false;
    let mut welcome_announced = false;
    let mut welcome_diag_last = monotonic_ms();
    let mut _welcome_diag_iters: u64 = 0;
    let mut welcome_diag_last_trace: u64 = 0;
    let mut welcome_start_time = monotonic_ms();
    let welcome_timeout_ms: u64 = 5_000; // 5 saniye içinde etkileşim olmazsa masaüstüne otomatik geç

    loop {
        if !welcome_announced {
            serial_print(
                "[BOOT] Welcome loop: waiting for Enter/ESC/click (5s timeout enabled).\n",
            );
            welcome_announced = true;
        }

        if welcome_dirty && !boot::text_console::is_active() {
            gui::render_welcome_screen(
                fb,
                width,
                height,
                settings_app.language.is_english(),
                settings_app.theme,
                welcome_focus,
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

        // PS/2 kesme kuyruğu: IRQ modunda klavye/fare olayları burada boşalır.
        let irq_update = drivers::ps2::drain_input_events(
            &mut *mouse,
            &mut *keyboard,
            width as i32,
            height as i32,
        );
        welcome_update.keyboard |= irq_update.keyboard;
        welcome_update.mouse |= irq_update.mouse;

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
            serial_print_dec(welcome_now.wrapping_sub(welcome_start_time));
            serial_print(" yineleme=");
            serial_print_dec(_welcome_diag_iters);
            serial_print("\n");
        }

        // 5 saniye zaman aşımı: USB klavye veya fare algılanamasa bile sistemi kilitleme, doğrudan masaüstüne geç.
        // Boot logu incelenirken (konsol açıkken) otomatik geçiş yapılmaz; log okunurken ekran kaybolmamalı.
        if welcome_now.wrapping_sub(welcome_start_time) >= welcome_timeout_ms
            && !boot::text_console::is_active()
        {
            serial_print("[BOOT] Welcome timeout; switching to desktop automatically.\n");
            welcome_accepted = true;
        }

        let mut enter_requested = false;
        while let Some(action) = keyboard.take_last_action() {
            // Boot logu açıkken PageUp/PageDown ve ok tuşları logu kaydırır.
            if boot::text_console::is_active() && boot::text_console::handle_key_action(action) {
                continue;
            }
            if matches!(action, drivers::keyboard::KeyAction::EnterKey) {
                enter_requested = true;
            }
        }

        let mut enter_char_seen = false;
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
            if ch == '\t' {
                welcome_focus = if keyboard.shift {
                    (welcome_focus + welcome_focus_count - 1) % welcome_focus_count
                } else {
                    (welcome_focus + 1) % welcome_focus_count
                };
                welcome_dirty = true;
            } else if ch == '\n' || ch == '\r' {
                enter_char_seen = true;
                activate_welcome_control(welcome_focus, settings_app, &mut welcome_accepted);
                welcome_dirty = true;
            } else if ch == '\x1b' {
                settings_app.language = original_language;
                settings_app.theme = original_theme;
                gui::set_ui_language_english(original_language.is_english());
                gui::theme::set_theme(original_theme);
                welcome_cancelled = true;
                welcome_accepted = true;
            } else if ch == ' ' {
                welcome_accepted = true;
            }
        }

        if enter_requested && !enter_char_seen {
            activate_welcome_control(welcome_focus, settings_app, &mut welcome_accepted);
            welcome_dirty = true;
        }

        if welcome_accepted {
            if !welcome_cancelled {
                settings_app.save_all(compositor, &mut fs_mgr.vfs);
            }
            break;
        }

        if welcome_mouse_pos != (mouse.x, mouse.y) {
            welcome_mouse_pos = (mouse.x, mouse.y);
            welcome_dirty = true;
        }

        let clicked = mouse.left_button && !welcome_was_left_down;
        welcome_was_left_down = mouse.left_button;
        // Boşta bekleme: PS/2 veya USB'de olay yoksa CPU'yu spin ile bekle.
        // hlt kullanılmaz çünkü PIC devre dışı bırakılmış ve periyodik timer
        // kesmesi yapılandırılmamış; VMware gibi hipervizörlerde hlt sonsuza
        // kadar bekleyip sistemi donabilir.
        if !welcome_update.mouse && !welcome_update.keyboard {
            for _ in 0..1_000 {
                if drivers::ps2::has_input() || drivers::usb_hid::has_event() {
                    break;
                }
                core::hint::spin_loop();
            }
        }

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
    }
}

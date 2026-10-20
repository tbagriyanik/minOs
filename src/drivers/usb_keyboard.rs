#![allow(dead_code)]

//! USB HID Klavye Sürücüsü (USB Keyboard Driver - USB HID Class 0x03, Subclass 0x01, Protocol 0x01).
//!
//! Bu modül, USB üzerinden bağlanan standart ve multimedya klavyelerin 8 baytlık
//! HID raporlarını ayrıştırır, tuş basma/bırakma olaylarını, modifier tuşlarını
//! (Shift, Ctrl, Alt, GUI/Win) ve tüketici/medya tuşlarını minOs çekirdek
//! `KeyboardState` katmanına aktarır.

use crate::drivers::keyboard::KeyboardState;

// ---------------------------------------------------------------------------
// USB HID Tuş Kodları (Usage Page 0x07 - Keyboard / Keypad)
// ---------------------------------------------------------------------------

pub const HID_KEY_NONE: u8 = 0x00;
pub const HID_KEY_ERROR_ROLLOVER: u8 = 0x01;
pub const HID_KEY_A: u8 = 0x04;
pub const HID_KEY_Z: u8 = 0x1D;
pub const HID_KEY_1: u8 = 0x1E;
pub const HID_KEY_0: u8 = 0x27;
pub const HID_KEY_ENTER: u8 = 0x28;
pub const HID_KEY_ESCAPE: u8 = 0x29;
pub const HID_KEY_BACKSPACE: u8 = 0x2A;
pub const HID_KEY_TAB: u8 = 0x2B;
pub const HID_KEY_SPACE: u8 = 0x2C;
pub const HID_KEY_MINUS: u8 = 0x2D;
pub const HID_KEY_EQUAL: u8 = 0x2E;
pub const HID_KEY_LEFTBRACE: u8 = 0x2F;
pub const HID_KEY_RIGHTBRACE: u8 = 0x30;
pub const HID_KEY_BACKSLASH: u8 = 0x31;
pub const HID_KEY_SEMICOLON: u8 = 0x32;
pub const HID_KEY_APOSTROPHE: u8 = 0x33;
pub const HID_KEY_GRAVE: u8 = 0x34;
pub const HID_KEY_COMMA: u8 = 0x35;
pub const HID_KEY_DOT: u8 = 0x36;
pub const HID_KEY_SLASH: u8 = 0x37;
pub const HID_KEY_CAPSLOCK: u8 = 0x38;

pub const HID_KEY_F1: u8 = 0x39;
pub const HID_KEY_F2: u8 = 0x3A;
pub const HID_KEY_F3: u8 = 0x3B;
pub const HID_KEY_F4: u8 = 0x3C;
pub const HID_KEY_F5: u8 = 0x3D;
pub const HID_KEY_F6: u8 = 0x3E;
pub const HID_KEY_F7: u8 = 0x3F;
pub const HID_KEY_F8: u8 = 0x40;
pub const HID_KEY_F9: u8 = 0x41;
pub const HID_KEY_F10: u8 = 0x42;
pub const HID_KEY_F11: u8 = 0x43;
pub const HID_KEY_F12: u8 = 0x44;

pub const HID_KEY_SYSRQ: u8 = 0x46; // Print Screen
pub const HID_KEY_SCROLLLOCK: u8 = 0x47;
pub const HID_KEY_PAUSE: u8 = 0x48;
pub const HID_KEY_INSERT: u8 = 0x49;
pub const HID_KEY_HOME: u8 = 0x4A;
pub const HID_KEY_PAGEUP: u8 = 0x4B;
pub const HID_KEY_DELETE: u8 = 0x4C;
pub const HID_KEY_END: u8 = 0x4D;
pub const HID_KEY_PAGEDOWN: u8 = 0x4E;
pub const HID_KEY_RIGHT: u8 = 0x4F;
pub const HID_KEY_LEFT: u8 = 0x50;
pub const HID_KEY_DOWN: u8 = 0x51;
pub const HID_KEY_UP: u8 = 0x52;

pub const HID_KEY_NUMLOCK: u8 = 0x53;
pub const HID_KEY_KPSLASH: u8 = 0x54;
pub const HID_KEY_KPASTERISK: u8 = 0x55;
pub const HID_KEY_KPMINUS: u8 = 0x56;
pub const HID_KEY_KPPLUS: u8 = 0x57;
pub const HID_KEY_KPENTER: u8 = 0x58;
pub const HID_KEY_KP1: u8 = 0x59;
pub const HID_KEY_KP0: u8 = 0x62;
pub const HID_KEY_KPDOT: u8 = 0x63;
pub const HID_KEY_102ND: u8 = 0x64; // Non-US \ and |

// Medya Tuşları (Usage Page 0x0C - Consumer Controls)
pub const HID_CONSUMER_MUTE: u8 = 0x7F;
pub const HID_CONSUMER_VOLUP: u8 = 0x80;
pub const HID_CONSUMER_VOLDOWN: u8 = 0x81;

// Modifier Bitleri (Rapor Bayt 0)
pub const MOD_LEFT_CTRL: u8   = 1 << 0;
pub const MOD_LEFT_SHIFT: u8  = 1 << 1;
pub const MOD_LEFT_ALT: u8    = 1 << 2;
pub const MOD_LEFT_GUI: u8    = 1 << 3; // Windows Tuşu
pub const MOD_RIGHT_CTRL: u8  = 1 << 4;
pub const MOD_RIGHT_SHIFT: u8 = 1 << 5;
pub const MOD_RIGHT_ALT: u8   = 1 << 6; // AltGr
pub const MOD_RIGHT_GUI: u8   = 1 << 7;

// Klavye LED Durumları (Output Report)
pub const LED_NUM_LOCK: u8    = 1 << 0;
pub const LED_CAPS_LOCK: u8   = 1 << 1;
pub const LED_SCROLL_LOCK: u8 = 1 << 2;

// ---------------------------------------------------------------------------
// USB Klavye Aygıt Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct UsbKeyboardDevice {
    pub active: bool,
    pub prev_modifiers: u8,
    pub prev_keys: [u8; 6],
    pub leds: u8,
}

impl UsbKeyboardDevice {
    pub const fn new() -> Self {
        Self {
            active: false,
            prev_modifiers: 0,
            prev_keys: [0; 6],
            leds: 0,
        }
    }

    pub fn reset(&mut self) {
        self.prev_modifiers = 0;
        self.prev_keys = [0; 6];
        self.leds = 0;
    }
}

// ---------------------------------------------------------------------------
// HID Usage ID -> PS/2 Set-1 Tarama Kodu Dönüşümü
// ---------------------------------------------------------------------------

/// HID kullanım kimliğini standart PS/2 Set-1 tarama koduna dönüştürür.
/// İkinci dönüş değeri (bool) genişletilmiş (0xE0) kod olup olmadığını belirtir.
pub fn hid_usage_to_scancode(usage: u8) -> Option<(u8, bool)> {
    Some(match usage {
        0x04 => (0x1E, false), // A
        0x05 => (0x30, false), // B
        0x06 => (0x2E, false), // C
        0x07 => (0x20, false), // D
        0x08 => (0x12, false), // E
        0x09 => (0x21, false), // F
        0x0A => (0x22, false), // G
        0x0B => (0x23, false), // H
        0x0C => (0x17, false), // I
        0x0D => (0x24, false), // J
        0x0E => (0x25, false), // K
        0x0F => (0x26, false), // L
        0x10 => (0x32, false), // M
        0x11 => (0x31, false), // N
        0x12 => (0x18, false), // O
        0x13 => (0x19, false), // P
        0x14 => (0x10, false), // Q
        0x15 => (0x13, false), // R
        0x16 => (0x1F, false), // S
        0x17 => (0x14, false), // T
        0x18 => (0x16, false), // U
        0x19 => (0x2F, false), // V
        0x1A => (0x11, false), // W
        0x1B => (0x2D, false), // X
        0x1C => (0x15, false), // Y
        0x1D => (0x2C, false), // Z
        0x1E => (0x02, false), // 1
        0x1F => (0x03, false), // 2
        0x20 => (0x04, false), // 3
        0x21 => (0x05, false), // 4
        0x22 => (0x06, false), // 5
        0x23 => (0x07, false), // 6
        0x24 => (0x08, false), // 7
        0x25 => (0x09, false), // 8
        0x26 => (0x0A, false), // 9
        0x27 => (0x0B, false), // 0
        0x28 => (0x1C, false), // Enter
        0x29 => (0x01, false), // Escape
        0x2A => (0x0E, false), // Backspace
        0x2B => (0x0F, false), // Tab
        0x2C => (0x39, false), // Space
        0x2D => (0x0C, false), // - / _
        0x2E => (0x0D, false), // = / +
        0x2F => (0x1A, false), // [ / {
        0x30 => (0x1B, false), // ] / }
        0x31 => (0x2B, false), // \ / |
        0x32 => (0x27, false), // ; / :
        0x33 => (0x28, false), // ' / "
        0x34 => (0x29, false), // ` / ~
        0x35 => (0x33, false), // , / <
        0x36 => (0x34, false), // . / >
        0x37 => (0x35, false), // / / ?
        0x38 => (0x3A, false), // Caps Lock

        // Fonksiyon Tuşları
        0x39 => (0x3B, false), // F1
        0x3A => (0x3C, false), // F2
        0x3B => (0x3D, false), // F3
        0x3C => (0x3E, false), // F4
        0x3D => (0x3F, false), // F5
        0x3E => (0x40, false), // F6
        0x3F => (0x41, false), // F7
        0x40 => (0x42, false), // F8
        0x41 => (0x43, false), // F9
        0x42 => (0x44, false), // F10
        0x43 => (0x57, false), // F11
        0x44 => (0x58, false), // F12

        0x46 => (0x37, true),  // Print Screen
        0x47 => (0x46, false), // Scroll Lock
        0x49 => (0x52, true),  // Insert
        0x4A => (0x47, true),  // Home
        0x4B => (0x49, true),  // Page Up
        0x4C => (0x53, true),  // Delete
        0x4D => (0x4F, true),  // End
        0x4E => (0x51, true),  // Page Down
        0x4F => (0x4D, true),  // Arrow Right
        0x50 => (0x4B, true),  // Arrow Left
        0x51 => (0x50, true),  // Arrow Down
        0x52 => (0x48, true),  // Arrow Up

        // Sayısal Tuş Takımı (Keypad)
        0x53 => (0x45, false), // Num Lock
        0x54 => (0x35, true),  // Keypad /
        0x55 => (0x37, false), // Keypad *
        0x56 => (0x4A, false), // Keypad -
        0x57 => (0x4E, false), // Keypad +
        0x58 => (0x1C, true),  // Keypad Enter
        0x59 => (0x4F, false), // Keypad 1
        0x5A => (0x50, false), // Keypad 2
        0x5B => (0x51, false), // Keypad 3
        0x5C => (0x4B, false), // Keypad 4
        0x5D => (0x4C, false), // Keypad 5
        0x5E => (0x4D, false), // Keypad 6
        0x5F => (0x47, false), // Keypad 7
        0x60 => (0x48, false), // Keypad 8
        0x61 => (0x49, false), // Keypad 9
        0x62 => (0x52, false), // Keypad 0
        0x63 => (0x53, false), // Keypad .
        0x64 => (0x56, false), // Non-US < > |

        // Multimedya ve Ses Kontrol
        0x7F => (0x20, true),  // Mute
        0x80 => (0x30, true),  // Volume Up
        0x81 => (0x2E, true),  // Volume Down
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Rapor Ayrıştırma ve Olay Üretimi
// ---------------------------------------------------------------------------

#[inline]
fn emit_key_event(kb: &mut KeyboardState, usage: u8, pressed: bool) {
    if let Some((code, extended)) = hid_usage_to_scancode(usage) {
        let scancode = if pressed { code } else { code | 0x80 };
        if extended {
            kb.handle_scancode(0xE0);
        }
        kb.handle_scancode(scancode);
    }
}

#[inline]
fn emit_modifier_event(kb: &mut KeyboardState, bit: u8, pressed: bool) {
    let (code, extended) = match bit {
        0 => (0x1D, false), // Sol Ctrl
        1 => (0x2A, false), // Sol Shift
        2 => (0x38, false), // Sol Alt
        3 => (0x5B, true),  // Sol Windows (GUI)
        4 => (0x1D, true),  // Sağ Ctrl
        5 => (0x36, false), // Sağ Shift
        6 => (0x38, true),  // Sağ Alt (AltGr)
        7 => (0x5C, true),  // Sağ Windows (GUI)
        _ => return,
    };
    let scancode = if pressed { code } else { code | 0x80 };
    if extended {
        kb.handle_scancode(0xE0);
    }
    kb.handle_scancode(scancode);
}

/// Standart 8 baytlık USB HID Boot Keyboard raporunu ayrıştırır.
///
/// Rapor Yapısı:
/// - Bayt 0: Modifier tuş bitmask'i
/// - Bayt 1: Ayrılmış (0x00)
/// - Bayt 2..7: Aynı anda basılı 6 adede kadar tuş kodu (Usage ID)
pub fn process_keyboard_report(
    dev: &mut UsbKeyboardDevice,
    report: &[u8],
    kb: &mut KeyboardState,
) -> bool {
    if report.len() < 8 {
        return false;
    }

    let modifiers = report[0];
    let mut current_keys = [0u8; 6];
    current_keys.copy_from_slice(&report[2..8]);

    // Rollover kontrolü (Aynı anda izin verilenden fazla tuş basıldığında 0x01 gelir)
    let is_rollover = current_keys.iter().any(|&k| k == HID_KEY_ERROR_ROLLOVER);
    if is_rollover {
        return false;
    }

    // 1. Bırakılan Tuşları Tespit Et ve Bırakma Olayı Üret
    for i in 0..6 {
        let old_key = dev.prev_keys[i];
        if old_key >= 0x04 && !current_keys.contains(&old_key) {
            emit_key_event(kb, old_key, false);
        }
    }

    // 2. Değişen Modifier Tuşları (Shift, Ctrl, Alt, GUI)
    let mod_changed = dev.prev_modifiers ^ modifiers;
    if mod_changed != 0 {
        for bit in 0..8u8 {
            if mod_changed & (1 << bit) != 0 {
                let pressed = (modifiers & (1 << bit)) != 0;
                emit_modifier_event(kb, bit, pressed);
            }
        }
    }

    // 3. Yeni Basılan Tuşları Tespit Et ve Basma Olayı Üret
    for i in 0..6 {
        let new_key = current_keys[i];
        if new_key >= 0x04 && !dev.prev_keys.contains(&new_key) {
            emit_key_event(kb, new_key, true);
        }
    }

    dev.prev_keys = current_keys;
    dev.prev_modifiers = modifiers;
    true
}

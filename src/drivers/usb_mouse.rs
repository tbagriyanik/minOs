#![allow(dead_code)]

//! USB HID Fare ve Tablet Sürücüsü (USB Mouse & Absolute Pointer Driver).
//!
//! Bu modül, USB üzerinden bağlanan standart göreceli (relative) farelerin
//! (3 ve 5 butonlu, kaydırma tekerlekli) ve mutlak (absolute) işaretçilerin
//! (USB Grafik Tablet / QEMU `usb-tablet` aygıtı) HID raporlarını ayrıştırır
//! ve minOs masaüstü `MouseState` katmanına aktarır.

use crate::drivers::mouse::MouseState;

// ---------------------------------------------------------------------------
// USB Fare Buton Sabitleri
// ---------------------------------------------------------------------------

pub const MOUSE_BTN_LEFT: u8 = 1 << 0;
pub const MOUSE_BTN_RIGHT: u8 = 1 << 1;
pub const MOUSE_BTN_MIDDLE: u8 = 1 << 2;
pub const MOUSE_BTN_SIDE: u8 = 1 << 3; // Geri (Back)
pub const MOUSE_BTN_EXTRA: u8 = 1 << 4; // İleri (Forward)

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsbPointerMode {
    Relative, // Standart göreceli USB fare (dX, dY)
    Absolute, // Mutlak koordinatlı USB Tablet (0..32767 X/Y)
}

#[derive(Clone, Copy)]
pub struct UsbMouseDevice {
    pub active: bool,
    pub mode: UsbPointerMode,
    pub last_buttons: u8,
}

impl UsbMouseDevice {
    pub const fn new() -> Self {
        Self {
            active: false,
            mode: UsbPointerMode::Relative,
            last_buttons: 0,
        }
    }

    pub fn reset(&mut self) {
        self.last_buttons = 0;
    }
}

// ---------------------------------------------------------------------------
// USB Fare ve Tablet Rapor Ayrıştırma
// ---------------------------------------------------------------------------

/// Standart USB HID Göreceli Fare veya Mutlak USB Tablet Raporunu Ayrıştırır.
///
/// Desteklenen Rapor Formatları:
/// 1. Standart Boot Mouse (3 bayt): `[Buttons, dX, dY]`
/// 2. IntelliMouse USB (4 bayt): `[Buttons, dX, dY, Wheel]`
/// 3. 5-Butonlu Fare (5 bayt): `[Buttons, dX, dY, Wheel, Pan]`
/// 4. Mutlak USB Tablet (6 bayt): `[Buttons, X_Lo, X_Hi, Y_Lo, Y_Hi, Wheel]`
///
/// Mutlak/göreli ayrımı `dev.mode` ile yapılır; bu değer xHCI sürücüsünde rapor
/// tanımlayıcısındaki Input öğesinin Absolute bitinden türetilir. Rapor
/// uzunluğuna bakarak karar vermek 6 baytlık göreceli fareleri yanlış sınıflar.
pub fn process_mouse_report(
    dev: &mut UsbMouseDevice,
    report: &[u8],
    mouse: &mut MouseState,
    screen_w: i32,
    screen_h: i32,
) -> bool {
    if report.len() < 3 {
        return false;
    }

    // 1. Mutlak (Absolute) USB Tablet Raporu
    // Yalnızca aygıtın rapor tanımlayıcısı mutlak eksen bildirdiğinde bu yol
    // kullanılır. Rapor uzunluğuna bakarak karar vermek, 6 baytlık göreceli
    // fareleri (5 düğme + 16-bit dX/dY) yanlışlıkla tablet sanmaya yol açıyordu.
    if dev.mode == UsbPointerMode::Absolute {
        if report.len() < 5 {
            return false;
        }
        let buttons = report[0];
        let raw_x = (report[1] as u16) | ((report[2] as u16) << 8);
        let raw_y = (report[3] as u16) | ((report[4] as u16) << 8);

        // 0..32767 mutlak koordinatlarını ekran çözünürlüğüne orantıla
        if screen_w > 0 && screen_h > 0 {
            mouse.x = (((raw_x as i32 * screen_w) + 16383) / 32767).clamp(0, screen_w - 1);
            mouse.y = (((raw_y as i32 * screen_h) + 16383) / 32767).clamp(0, screen_h - 1);
        }

        mouse.left_button = (buttons & MOUSE_BTN_LEFT) != 0;
        mouse.right_button = (buttons & MOUSE_BTN_RIGHT) != 0;
        mouse.middle_button = (buttons & MOUSE_BTN_MIDDLE) != 0;

        if report.len() >= 6 {
            let wheel = report[5] as i8;
            if wheel != 0 {
                mouse.scroll_delta = mouse.scroll_delta.saturating_sub(wheel);
            }
        }

        dev.last_buttons = buttons;
        return true;
    }

    // 2. Standart Göreceli (Relative) Fare Raporu (3..5 bayt)
    let buttons = report[0] & 0x07;
    let dx = report[1] as i8;
    // USB HID standardında pozitif Y aşağı doğrudur (PS/2 tersinedir).
    // `mouse.update_packet` fonksiyonu PS/2 uyumluluğu için Y'yi çıkardığından
    // Y eksenini ters çevirerek iletiyoruz.
    let dy = (report[2] as i8).wrapping_neg();

    mouse.update_packet(buttons, dx, dy, screen_w, screen_h);

    // Dikey Tekerlek (Scroll Wheel) Desteği
    if report.len() >= 4 {
        let wheel = report[3] as i8;
        if wheel != 0 {
            mouse.scroll_delta = mouse.scroll_delta.saturating_sub(wheel);
        }
    }

    dev.last_buttons = buttons;
    true
}

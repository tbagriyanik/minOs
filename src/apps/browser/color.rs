//! Renk sabitleri ve dönüştürme yardımcıları (no_std).
//! Taşındı: host-browser/src/color.rs

#![allow(dead_code)]

pub const BLACK: u32 = 0x000000;
pub const WHITE: u32 = 0xFFFFFF;
pub const RED: u32 = 0xFF0000;
pub const GREEN: u32 = 0x00FF00;
pub const BLUE: u32 = 0x0000FF;
pub const YELLOW: u32 = 0xFFFF00;
pub const CYAN: u32 = 0x00FFFF;
pub const MAGENTA: u32 = 0xFF00FF;
pub const GRAY: u32 = 0x808080;
pub const DARK_GRAY: u32 = 0x404040;
pub const LIGHT_GRAY: u32 = 0xC0C0C0;

pub const DARK_BG: u32 = 0x1A1A2E;
pub const WINDOW_BG: u32 = 0xFFFFFF;
pub const TITLE_BAR: u32 = 0x2C3E50;
pub const TASKBAR_BG: u32 = 0x2C3E50;
pub const ACCENT_BLUE: u32 = 0x2050C0;
pub const ACCENT_GREEN: u32 = 0x00AA00;
pub const ACCENT_RED: u32 = 0xD03030;
pub const ACCENT_YELLOW: u32 = 0xFFD700;
pub const ACCENT_PINK: u32 = 0xFF69B4;
pub const TEXT_MAIN: u32 = 0x000000;
pub const TEXT_MUTED: u32 = 0x666666;
pub const BORDER: u32 = 0x999999;

#[allow(dead_code)]
pub fn u32_to_rgb(c: u32) -> (u8, u8, u8) {
    (
        ((c >> 16) & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        (c & 0xFF) as u8,
    )
}

#[allow(dead_code)]
pub fn color32_to_u32(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub mod font;

use crate::drivers::video::Framebuffer;
use crate::drivers::mouse::MouseState;

pub struct Color;

impl Color {
    pub const BLACK: u32 = 0x000000;
    pub const WHITE: u32 = 0xFFFFFF;
    pub const DARK_BG: u32 = 0x181825;
    pub const TITLE_BAR: u32 = 0x313244;
    pub const TITLE_BAR_INACTIVE: u32 = 0x1E1E2E;
    pub const ACCENT_BLUE: u32 = 0x89B4FA;
    pub const ACCENT_GREEN: u32 = 0xA6E3A1;
    pub const ACCENT_RED: u32 = 0xF38BA8;
    pub const ACCENT_YELLOW: u32 = 0xF9E2AF;
    pub const ACCENT_PINK: u32 = 0xF5C2E7;
    pub const ACCENT_CAT_EARS: u32 = 0xF2CDCD;
    pub const TEXT_MAIN: u32 = 0xCDD6F4;
    pub const TEXT_MUTED: u32 = 0x6C7086;
    pub const TASKBAR_BG: u32 = 0x11111B;
    pub const WINDOW_BG: u32 = 0x1E1E2E;
}

pub struct Window {
    pub title: &'static str,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub is_focused: bool,
    pub is_minimized: bool,
}

impl Window {
    pub fn new(title: &'static str, x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            title,
            x,
            y,
            width,
            height,
            is_focused: false,
            is_minimized: false,
        }
    }
}


pub struct MinGuiCompositor {
    pub windows: [Option<Window>; 4],
    pub active_window_idx: Option<usize>,
}

impl MinGuiCompositor {
    pub const fn new() -> Self {
        Self {
            windows: [None, None, None, None],
            active_window_idx: None,
        }
    }

    pub fn add_window(&mut self, win: Window) -> bool {
        for slot in self.windows.iter_mut() {
            if slot.is_none() {
                *slot = Some(win);
                return true;
            }
        }
        false
    }

    pub fn draw_string(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
        let mut cur_x = x;
        for c in text.chars() {
            if let Some(tr_glyph) = font::get_turkish_glyph(c) {
                for row in 0..8 {
                    for col in 0..8 {
                        if (tr_glyph[row] & (1 << (7 - col))) != 0 {
                            fb.put_pixel(cur_x + col, y + row, color);
                        }
                    }
                }
            } else {
                let b = c as u32;
                if b >= 32 && b <= 126 {
                    let glyph = font::FONT_8X8_BASIC[(b - 32) as usize];
                    for row in 0..8 {
                        for col in 0..8 {
                            if (glyph[row] & (1 << (7 - col))) != 0 {
                                fb.put_pixel(cur_x + col, y + row, color);
                            }
                        }
                    }
                }
            }
            cur_x += 8;
        }
    }

    /// MinOS Özel Kedi (Cat) Logosu ve Simgesi Çizici (16x16)
    pub fn draw_cat_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        // 16x16 Şirin Kedi Maskotu Deseni (Kulaklar, Gözler, Burun, Bıyıklar)
        const CAT_ICON_MAP: [u16; 16] = [
            0b0110000000000110, // Kulak uçları
            0b1111000000001111, // Kulaklar
            0b1111111111111111, // Baş üstü
            0b1111111111111111, // Alın
            0b1100111111110011, // Gözler
            0b1100111111110011, // Gözler
            0b1111111111111111, // Yanaklar
            0b1001111001111001, // Bıyıklar ve Burun
            0b1111110000111111, // Ağız
            0b0111111111111110, // Çene
            0b0011111111111100, // Boyun
            0b0011111111111100, // Gövde
            0b0111001111001110, // Patiler
            0b0111001111001110, // Patiler
            0b0011111111111100, // Alt taban
            0b0000000000000000,
        ];

        for (row, &mask) in CAT_ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let pixel_color = if (row == 4 || row == 5) && (col == 2 || col == 3 || col == 12 || col == 13) {
                        Color::ACCENT_BLUE // Parlak Mavi Gözler
                    } else if row == 8 && (col == 7 || col == 8) {
                        Color::ACCENT_PINK // Pembe Burun
                    } else if row <= 1 {
                        Color::ACCENT_CAT_EARS // Pembe/Açık Kulak içi
                    } else {
                        Color::TEXT_MAIN // Kedi gövdesi
                    };
                    fb.put_pixel(x + col, y + row, pixel_color);
                }
            }
        }
    }


    pub fn render_desktop(&mut self, fb: &mut Framebuffer, mouse: &MouseState, kbd: &crate::drivers::keyboard::KeyboardState) {
        // 1. Masaüstü Arka Planı
        fb.clear(Color::DARK_BG);

        // Masaüstü Sağ Üst MinOS Kedi Logosu ve Başlığı (Watermark)
        Self::draw_cat_icon(fb, fb.width.saturating_sub(180), 30);
        Self::draw_string(fb, fb.width.saturating_sub(155), 35, "MinOS Cat Edition", Color::ACCENT_PINK);

        // 2. Açık Pencerelerin Çizimi
        for opt_win in self.windows.iter() {
            if let Some(win) = opt_win {
                if win.is_minimized { continue; }
                
                // Pencere Gövdesi
                fb.draw_rect(win.x, win.y, win.width, win.height, Color::WINDOW_BG);

                // Başlık Çubuğu
                let title_color = if win.is_focused { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE };
                fb.draw_rect(win.x, win.y, win.width, 24, title_color);

                // Pencere Başlığı yanında mini kedi ikonu
                Self::draw_cat_icon(fb, win.x + 6, win.y + 4);
                Self::draw_string(fb, win.x + 28, win.y + 8, win.title, Color::TEXT_MAIN);

                // Kontrol Butonları
                fb.draw_rect(win.x + win.width - 20, win.y + 4, 14, 14, Color::ACCENT_RED);
                fb.draw_rect(win.x + win.width - 38, win.y + 4, 14, 14, Color::ACCENT_YELLOW);
            }
        }

        // 3. Görev Çubuğu (Taskbar)
        let tb_height = 36;
        let tb_y = fb.height.saturating_sub(tb_height);
        fb.draw_rect(0, tb_y, fb.width, tb_height, Color::TASKBAR_BG);

        // Kedi Simgeli Başlat Menüsü Butonu
        fb.draw_rect(6, tb_y + 4, 90, 28, Color::ACCENT_BLUE);
        Self::draw_cat_icon(fb, 12, tb_y + 10);
        Self::draw_string(fb, 34, tb_y + 14, "MinOS", Color::TASKBAR_BG);

        // Sistem Saati & Dil Seçeneği (TR / EN)
        Self::draw_string(fb, fb.width.saturating_sub(75), tb_y + 14, "12:00", Color::TEXT_MAIN);
        
        // TR / EN Klavye Rozeti (Badge)
        let kbd_badge_color = match kbd.layout {
            crate::drivers::keyboard::KeyboardLayout::TurkishQ => Color::ACCENT_RED,
            crate::drivers::keyboard::KeyboardLayout::EnglishUS => Color::ACCENT_BLUE,
        };
        fb.draw_rect(fb.width.saturating_sub(130), tb_y + 6, 45, 24, Color::TITLE_BAR);
        Self::draw_string(fb, fb.width.saturating_sub(122), tb_y + 14, kbd.layout_tag(), kbd_badge_color);

        Self::draw_string(fb, fb.width.saturating_sub(190), tb_y + 14, "[ETH0]", Color::ACCENT_GREEN);
        Self::draw_string(fb, fb.width.saturating_sub(240), tb_y + 14, "[VOL]", Color::ACCENT_BLUE);

        // 4. Donanım Fare İmleci
        self.draw_mouse_cursor(fb, mouse.x as usize, mouse.y as usize);
    }

    fn draw_mouse_cursor(&self, fb: &mut Framebuffer, mx: usize, my: usize) {
        const CURSOR_PATTERN: [u8; 12] = [
            0b10000000,
            0b11000000,
            0b11100000,
            0b11110000,
            0b11111000,
            0b11111100,
            0b11111110,
            0b11110000,
            0b10011000,
            0b00011000,
            0b00001100,
            0b00001100,
        ];

        for (row, &byte) in CURSOR_PATTERN.iter().enumerate() {
            for col in 0..8 {
                if (byte & (1 << (7 - col))) != 0 {
                    fb.put_pixel(mx + col, my + row, Color::WHITE);
                }
            }
        }
    }
}

//! Bitmap icon drawing.

use super::*;

impl MinGuiCompositor {
    /// minOs kedi logosunu alan örneklemeli yumuşatma ile hedef boyuta taşır.
    fn draw_icon_bitmap(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        size: usize,
        source: &[u32; 256],
    ) {
        let mut resolved = [0u32; 256];
        for (output, input) in resolved.iter_mut().zip(source.iter()) {
            if *input != 0 {
                *output = theme::resolve(*input);
            }
        }
        for dy in 0..size {
            let sy0 = dy * 16;
            let sy1 = (dy + 1) * 16;
            let row_start = sy0 / size;
            let row_end = ((sy1 - 1) / size).min(15);
            for dx in 0..size {
                let sx0 = dx * 16;
                let sx1 = (dx + 1) * 16;
                let col_start = sx0 / size;
                let col_end = ((sx1 - 1) / size).min(15);
                let mut covered = 0u32;
                let mut red = 0u32;
                let mut green = 0u32;
                let mut blue = 0u32;
                for row in row_start..=row_end {
                    let oy = sy1
                        .min((row + 1) * size)
                        .saturating_sub(sy0.max(row * size));
                    for col in col_start..=col_end {
                        let color = resolved[row * 16 + col];
                        if color == 0 {
                            continue;
                        }
                        let ox = sx1
                            .min((col + 1) * size)
                            .saturating_sub(sx0.max(col * size));
                        let weight = (ox * oy) as u32;
                        covered += weight;
                        red += ((color >> 16) & 0xFF) * weight;
                        green += ((color >> 8) & 0xFF) * weight;
                        blue += (color & 0xFF) * weight;
                    }
                }
                if covered == 0 {
                    continue;
                }
                let color = ((red / covered) << 16) | ((green / covered) << 8) | (blue / covered);
                let alpha = ((covered * 255 + 128) / 256) as u8;
                if alpha == 255 {
                    fb.put_pixel(x + dx, y + dy, color);
                } else {
                    fb.put_subpixel_text(x + dx, y + dy, color, [alpha; 3]);
                }
            }
        }
    }

    pub(crate) fn draw_antialiased_line(
        fb: &mut Framebuffer,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: u32,
        width: f32,
    ) {
        let radius = width / 2.0;
        let radius_sq = radius * radius;
        let feather = 0.5f32;
        let max_r = radius + feather;
        let max_r_sq = max_r * max_r;
        let min_x = (x0.min(x1) - max_r).max(0.0) as usize;
        let min_y = (y0.min(y1) - max_r).max(0.0) as usize;
        let max_x = (x0.max(x1) + max_r) as usize;
        let max_y = (y0.max(y1) + max_r) as usize;
        let vx = x1 - x0;
        let vy = y1 - y0;
        let length_sq = (vx * vx + vy * vy).max(0.0001);
        let inv_len_sq = 1.0 / length_sq;

        for py in min_y..=max_y {
            let sy = py as f32 + 0.5;
            let dy0 = sy - y0;
            for px in min_x..=max_x {
                let sx = px as f32 + 0.5;
                let dx0 = sx - x0;
                let t = ((dx0 * vx + dy0 * vy) * inv_len_sq).clamp(0.0, 1.0);
                let dx = sx - (x0 + t * vx);
                let dy = sy - (y0 + t * vy);
                let d_sq = dx * dx + dy * dy;
                if d_sq <= max_r_sq {
                    if d_sq <= radius_sq {
                        fb.put_pixel(px, py, color);
                    } else {
                        // Fast inverse square root to avoid libm dependency in no_std
                        let inv_d = {
                            let i = d_sq.to_bits();
                            let i = 0x5f3759df - (i >> 1);
                            let y = f32::from_bits(i);
                            let y = y * (1.5 - 0.5 * d_sq * y * y);
                            y * (1.5 - 0.5 * d_sq * y * y)
                        };
                        let d = d_sq * inv_d;
                        let alpha = (((max_r - d) / feather).clamp(0.0, 1.0) * 255.0) as u8;
                        if alpha > 0 {
                            fb.put_subpixel_text(px, py, color, [alpha; 3]);
                        }
                    }
                }
            }
        }
    }

    /// Aktif UI ölçeğinde bir ikonun kenar uzunluğu (px). 20px temel alınır ve
    /// `sp()` ile ölçeklenir; böylece ikon konumu ile boyutu tutarlıdır.
    #[inline(always)]
    pub(crate) fn icon_size() -> usize {
        sp(20)
    }

    pub fn draw_cat_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        Self::draw_cat_icon_scaled(fb, x, y)
    }

    pub fn draw_cat_icon_scaled(fb: &mut Framebuffer, x: usize, y: usize) {
        Self::draw_cat_icon_at_size(fb, x, y, Self::icon_size());
    }

    pub(crate) fn draw_cat_icon_at_size(fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let mut bitmap = [0u32; 256];
        const CAT_ICON_MAP: [u16; 16] = [
            0b0110000000000110,
            0b1111000000001111,
            0b1111111111111111,
            0b1111111111111111,
            0b1100111111110011,
            0b1100111111110011,
            0b1111111111111111,
            0b1001111001111001,
            0b1111110000111111,
            0b0111111111111110,
            0b0011111111111100,
            0b0011111111111100,
            0b0111001111001110,
            0b0111001111001110,
            0b0011111111111100,
            0b0000000000000000,
        ];

        for (row, &mask) in CAT_ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let pixel_color = if (row == 4 || row == 5)
                        && (col == 2 || col == 3 || col == 12 || col == 13)
                    {
                        Color::ACCENT_BLUE
                    } else if row == 8 && (col == 7 || col == 8) {
                        Color::ACCENT_PINK
                    } else if theme::current() == theme::ThemeId::Daylight {
                        0xE67E22
                    } else if row <= 1 {
                        Color::ACCENT_CAT_EARS
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = pixel_color;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, size, &bitmap);
    }

    /// Bilgisayarım / Dosya Yöneticisi Simgesi (16x16)
    pub fn draw_computer_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0111111111111100,
            0b1000000000000010,
            0b1011111111111010,
            0b1010000000001010,
            0b1010111011101010,
            0b1010000000001010,
            0b1011111111111010,
            0b1000000000000010,
            0b0111111111111100,
            0b0000011110000000,
            0b0000001100000000,
            0b0000111111000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 2 && row <= 6 && col >= 2 && col <= 13 {
                        Color::ACCENT_BLUE
                    } else if row >= 9 {
                        Color::TEXT_MUTED
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Notlar / Not Defteri Simgesi (16x16)
    pub fn draw_notepad_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0011111111000000,
            0b0010000001100000,
            0b0010000001010000,
            0b0010111111110000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0010111110010000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0011111111110000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c =
                        if (row == 5 || row == 7 || row == 9 || row == 11) && col >= 4 && col <= 10
                        {
                            Color::ACCENT_BLUE
                        } else if row <= 2 && col >= 9 {
                            Color::ACCENT_YELLOW
                        } else {
                            Color::TEXT_MAIN
                        };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Web Tarayıcısı / Ağ Simgesi (16x16)
    pub fn draw_browser_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011110000000,
            0b0001111111100000,
            0b0011001100110000,
            0b0110001100011000,
            0b0111111111111000,
            0b1100001100001100,
            0b1100001100001100,
            0b1111111111111100,
            0b1100001100001100,
            0b1100001100001100,
            0b0111111111111000,
            0b0110001100011000,
            0b0011001100110000,
            0b0001111111100000,
            0b0000011110000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row == 4 || row == 7 || row == 10 {
                        Color::ACCENT_GREEN
                    } else {
                        Color::ACCENT_BLUE
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Müzik Çalar / Çalma Listesi Simgesi (16x16)
    pub fn draw_music_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011111110000,
            0b0000011111110000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0001110001110000,
            0b0011111001111100,
            0b0011111001111100,
            0b0001110001110000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 8 {
                        Color::ACCENT_PINK
                    } else {
                        Color::ACCENT_YELLOW
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Sistem Ayarları / Dişli Çark Simgesi (16x16)
    pub fn draw_gear_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011001100000,
            0b0000011001100000,
            0b0011001111001100,
            0b0011011111101100,
            0b0001110000111000,
            0b1111100000011111,
            0b1111000110001111,
            0b0011001111001100,
            0b0011001111001100,
            0b1111000110001111,
            0b1111100000011111,
            0b0001110000111000,
            0b0011011111101100,
            0b0011001111001100,
            0b0000011001100000,
            0b0000011001100000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 6 && row <= 9 && col >= 6 && col <= 9 {
                        Color::ACCENT_BLUE
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// 16x16 3D İzometrik Paket / Koli Kutusu simgesi (Paket Yöneticisi ve Kurulu Uygulamalar için).
    pub fn draw_package_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 16 * 16];

        // 3D Kutu Renk Paleti:
        let c_top = 0xFFE5C07B; // Üst yüzey (aydınlık karton)
        let c_tape = 0xFFD19A66; // Üst koli bandı / katlama çizgisi
        let c_left = 0xFFC89248; // Sol ön yüz (orta ton karton)
        let c_right = 0xFF9E6822; // Sağ ön yüz (gölgeli karton)
        let c_edge = 0xFF3E2006; // 3D kutu dış konturu ve köşe çizgileri
        let c_label = 0xFFF0F0F0; // Paket üzerindeki kargo etiketi
        let c_bar = 0xFF2980B9; // Etiket üzerindeki mavi barkod/şerit

        let mut set_px = |r: usize, c: usize, col: u32| {
            if r < 16 && c < 16 {
                bitmap[r * 16 + c] = col;
            }
        };

        // Row 1: Tepe noktası
        for c in 7..=8 {
            set_px(1, c, c_edge);
        }

        // Row 2
        set_px(2, 5, c_edge);
        set_px(2, 6, c_top);
        set_px(2, 7, c_tape);
        set_px(2, 8, c_tape);
        set_px(2, 9, c_top);
        set_px(2, 10, c_edge);

        // Row 3
        set_px(3, 3, c_edge);
        for c in 4..=6 {
            set_px(3, c, c_top);
        }
        set_px(3, 7, c_tape);
        set_px(3, 8, c_tape);
        for c in 9..=11 {
            set_px(3, c, c_top);
        }
        set_px(3, 12, c_edge);

        // Row 4
        set_px(4, 1, c_edge);
        for c in 2..=6 {
            set_px(4, c, c_top);
        }
        set_px(4, 7, c_tape);
        set_px(4, 8, c_tape);
        for c in 9..=13 {
            set_px(4, c, c_top);
        }
        set_px(4, 14, c_edge);

        // Row 5: Üst yüzeyin yan yüzeylerle birleşimi
        set_px(5, 1, c_edge);
        for c in 2..=5 {
            set_px(5, c, c_left);
        }
        set_px(5, 6, c_top);
        set_px(5, 7, c_tape);
        set_px(5, 8, c_tape);
        set_px(5, 9, c_top);
        for c in 10..=13 {
            set_px(5, c, c_right);
        }
        set_px(5, 14, c_edge);

        // Row 6..7: Kutu gövdesi ve orta dikey kenar
        for r in 6..=7 {
            set_px(r, 1, c_edge);
            for c in 2..=6 {
                set_px(r, c, c_left);
            }
            set_px(r, 7, c_edge);
            set_px(r, 8, c_edge);
            for c in 9..=13 {
                set_px(r, c, c_right);
            }
            set_px(r, 14, c_edge);
        }

        // Row 8: Beyaz kargo etiketi
        set_px(8, 1, c_edge);
        set_px(8, 2, c_left);
        for c in 3..=5 {
            set_px(8, c, c_label);
        }
        set_px(8, 6, c_left);
        set_px(8, 7, c_edge);
        set_px(8, 8, c_edge);
        for c in 9..=13 {
            set_px(8, c, c_right);
        }
        set_px(8, 14, c_edge);

        // Row 9: Etiket barkod çizgisi
        set_px(9, 1, c_edge);
        set_px(9, 2, c_left);
        set_px(9, 3, c_label);
        set_px(9, 4, c_bar);
        set_px(9, 5, c_label);
        set_px(9, 6, c_left);
        set_px(9, 7, c_edge);
        set_px(9, 8, c_edge);
        for c in 9..=13 {
            set_px(9, c, c_right);
        }
        set_px(9, 14, c_edge);

        // Row 10..11: Alt gövde
        for r in 10..=11 {
            set_px(r, 1, c_edge);
            for c in 2..=6 {
                set_px(r, c, c_left);
            }
            set_px(r, 7, c_edge);
            set_px(r, 8, c_edge);
            for c in 9..=13 {
                set_px(r, c, c_right);
            }
            set_px(r, 14, c_edge);
        }

        // Row 12: Alt taban eğimi
        set_px(12, 3, c_edge);
        for c in 4..=6 {
            set_px(12, c, c_left);
        }
        set_px(12, 7, c_edge);
        set_px(12, 8, c_edge);
        for c in 9..=11 {
            set_px(12, c, c_right);
        }
        set_px(12, 12, c_edge);

        // Row 13: Alt taban ucu
        set_px(13, 5, c_edge);
        set_px(13, 6, c_left);
        set_px(13, 7, c_edge);
        set_px(13, 8, c_edge);
        set_px(13, 9, c_right);
        set_px(13, 10, c_edge);

        // Row 14: Taban tepe noktası
        for c in 7..=8 {
            set_px(14, c, c_edge);
        }

        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// İndekse göre ilgili simgeyi çizer (aktif UI ölçeğinde).
    pub fn draw_icon_by_index(fb: &mut Framebuffer, x: usize, y: usize, index: usize) {
        Self::draw_icon_by_index_scaled(fb, x, y, index)
    }

    /// `draw_icon_by_index` ile aynıdır; ikonlar içeride `icon_scale()` ile
    /// ölçeklendiği için ölçek uygulaması ikon fonksiyonlarının içindedir.
    pub fn draw_icon_by_index_scaled(fb: &mut Framebuffer, x: usize, y: usize, index: usize) {
        match index {
            0 => Self::draw_computer_icon(fb, x, y),
            1 => Self::draw_notepad_icon(fb, x, y),
            2 => Self::draw_browser_icon(fb, x, y),
            3 => Self::draw_music_icon(fb, x, y),
            4 => Self::draw_gear_icon(fb, x, y),
            5 => Self::draw_package_icon(fb, x, y),
            _ => Self::draw_cat_icon(fb, x, y),
        }
    }
}

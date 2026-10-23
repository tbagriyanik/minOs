use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct VideoPlayerApp {
    pub is_playing: bool,
    pub frame: usize,
}

impl VideoPlayerApp {
    /// Zaman çizelgesinin pencere içi üst kenarı (px).
    pub const SCRUB_Y: usize = 205;
    /// Oynatıcı denetim düğmelerinin pencere içi üst kenarı (px).
    pub const CONTROLS_Y: usize = 223;

    pub const fn new() -> Self {
        Self {
            is_playing: true,
            frame: 0,
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize, _tick: usize) {
        // Araç Çubuğu / Başlık Bilgisi
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 34, "retro_drive.mp4 - 30 FPS MinVideo Engine", Color::TEXT_MAIN);
        // Durum etiketi oynatma durumuna göre değişir (sağa hizalı).
        let (status_text, status_color) = if self.is_playing {
            (crate::gui::ui_text("● OYNATILIYOR", "● PLAYING"), Color::ACCENT_GREEN)
        } else {
            (crate::gui::ui_text("DURAKLATILDI", "PAUSED"), Color::TEXT_MUTED)
        };
        let status_w = MinGuiCompositor::text_width(status_text, 1);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(status_w + 12), win_y + 34, status_text, status_color);

        // Video Ekranı (Viewport)
        let vp_x = win_x + 8;
        let vp_y = win_y + 52;
        let vp_w = win_w - 16;
        let vp_h = 145;
        
        // 1. Gökyüzü (Koyu Mor / Gece)
        let sky_color = 0x181028;
        fb.draw_rect(vp_x, vp_y, vp_w, vp_h / 2, sky_color);

        // 2. Neon Synthwave Güneşi (Ufuk çizgisinde)
        let sun_cx = vp_x + vp_w / 2;
        let sun_cy = vp_y + vp_h / 2;
        let sun_r = 24;
        for dy in -sun_r..=0 {
            let line_y = (sun_cy as isize + dy) as usize;
            if line_y >= vp_y && line_y < vp_y + vp_h {
                // Güneş çizgileri (Scanlines)
                if (dy.abs() % 4) != 0 {
                    // Yaklaşık genişlik
                    let hw = match dy.abs() {
                        0..=6 => 22,
                        7..=12 => 20,
                        13..=18 => 16,
                        _ => 10,
                    };
                    for px in (sun_cx.saturating_sub(hw))..=(sun_cx + hw) {
                        if px >= vp_x && px < vp_x + vp_w {
                            let sun_color = if dy < -12 { Color::ACCENT_YELLOW } else { Color::ACCENT_PINK };
                            fb.put_pixel(px, line_y, sun_color);
                        }
                    }
                }
            }
        }

        // 3. Ufuk Çizgisi (Neon Pembe)
        let horizon_y = vp_y + vp_h / 2;
        fb.draw_rect(vp_x, horizon_y, vp_w, 2, Color::ACCENT_PINK);

        // 4. Izgara Zemin (Yol / Retro Grid)
        let road_color = 0x0A0612;
        fb.draw_rect(vp_x, horizon_y + 2, vp_w, vp_h - (vp_h / 2) - 2, road_color);

        // Dinamik Akan Yatay Izgara Çizgileri
        // `frame` yalnızca oynatma açıkken ilerlediği için görüntü duraklatınca donar.
        let ground_h = vp_h - (vp_h / 2) - 2;
        let speed = (self.frame * 2) % 20;

        for step in 0..6 {
            let offset_y = ((step * 14 + speed) * (step * 14 + speed)) / 45;
            if offset_y < ground_h {
                let gy = horizon_y + 2 + offset_y;
                if gy < vp_y + vp_h {
                    fb.draw_rect(vp_x, gy, vp_w, 1, 0x5C2D91);
                }
            }
        }

        // Dikey Perspektif Çizgileri (Ufuk noktasından aşağıya doğru açılan çizgiler)
        let vanish_x = sun_cx;
        for i in -4..=4 {
            // Basit çizgi projeksiyonu
            let mut y = horizon_y + 2;
            while y < vp_y + vp_h {
                let progress = y - horizon_y;
                let px = (vanish_x as isize + (i * 40 * progress as isize) / ground_h as isize) as usize;
                if px >= vp_x && px < vp_x + vp_w {
                    fb.put_pixel(px, y, 0x482070);
                }
                y += 2;
            }
        }

        // 5. Yolda İlerleyen Retro Siber Araç / Kedi Gemisi
        let car_x = sun_cx.saturating_sub(18);
        let car_y = vp_y + vp_h - 22;
        fb.draw_rect(car_x, car_y, 36, 12, Color::TITLE_BAR);
        fb.draw_rect(car_x + 6, car_y - 6, 24, 7, Color::WINDOW_BG); // Tavan / Cam
        fb.draw_rect(car_x + 8, car_y - 4, 20, 4, Color::ACCENT_BLUE); // Cam ışıltısı
        // Arka neon stop lambaları
        fb.draw_rect(car_x + 2, car_y + 4, 8, 4, Color::ACCENT_RED);
        fb.draw_rect(car_x + 26, car_y + 4, 8, 4, Color::ACCENT_RED);

        // Video İlerleme Çubuğu (Timeline)
        const FPS: usize = 30;
        const DURATION_S: usize = 90; // 01:30
        let scrub_y = win_y + Self::SCRUB_Y;
        let elapsed_s = (self.frame / FPS) % DURATION_S;
        let progress = elapsed_s * 100 / DURATION_S;
        let mut elapsed_text = crate::gui::TextBuf::new();
        elapsed_text.push_u2((elapsed_s / 60) as u32);
        elapsed_text.push_str(":");
        elapsed_text.push_u2((elapsed_s % 60) as u32);
        MinGuiCompositor::draw_string(fb, win_x + 12, scrub_y, elapsed_text.as_str(), Color::TEXT_MAIN);
        let scrub_w = win_w - 110;
        let scrub_x = win_x + 54;
        fb.draw_rect(scrub_x, scrub_y + 2, scrub_w, 6, Color::DARK_BG);
        let filled_w = (scrub_w * progress) / 100;
        fb.draw_rect(scrub_x, scrub_y + 2, filled_w, 6, Color::ACCENT_RED);
        fb.draw_rect(scrub_x + filled_w.saturating_sub(2), scrub_y, 5, 10, Color::WHITE);
        MinGuiCompositor::draw_string(fb, scrub_x + scrub_w + 8, scrub_y, "01:30", Color::TEXT_MUTED);

        // Oynatıcı Kontrolleri
        let ctrl_y = win_y + Self::CONTROLS_Y;
        fb.draw_rect(win_x + 12, ctrl_y, 42, 22, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + 18, ctrl_y + 6, if self.is_playing { "||" } else { ">" }, MinGuiCompositor::text_on(Color::ACCENT_GREEN));

        fb.draw_rect(win_x + 58, ctrl_y, 40, 22, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 64, ctrl_y + 6, crate::gui::ui_text("Durdur", "Stop"), Color::TEXT_MAIN);

        fb.draw_rect(win_x + 102, ctrl_y, 48, 22, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 108, ctrl_y + 6, "-10s", Color::TEXT_MUTED);

        fb.draw_rect(win_x + 154, ctrl_y, 48, 22, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 160, ctrl_y + 6, "+10s", Color::TEXT_MUTED);

        fb.draw_rect(win_x + win_w - 76, ctrl_y, 64, 22, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 70, ctrl_y + 6, crate::gui::ui_text("Tam Ekran", "Full Screen"), MinGuiCompositor::text_on(Color::ACCENT_BLUE));

        // Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 250, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 254, crate::gui::ui_text("Donanım Çözücü: SSE2 Optimize | YUV to RGB Framebuffer", "Hardware Decoder: SSE2 Optimized | YUV to RGB Framebuffer"), Color::TEXT_MUTED);
    }
}

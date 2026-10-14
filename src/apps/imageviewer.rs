use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct ImageViewerApp {
    pub zoom_level: usize,
    pub image: alloc::vec::Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub top_down: bool,
    pub file_name: &'static str,
    pub status: &'static str,
}

impl ImageViewerApp {
    pub const fn new() -> Self {
        Self { zoom_level: 4, image: alloc::vec::Vec::new(), width: 0, height: 0, top_down: false, file_name: "kedi_maskotu.bmp", status: "Ready" }
    }

    pub fn open_bmp(&mut self, data: &[u8]) -> bool {
        if data.len() < 54 || &data[0..2] != b"BM" || u16::from_le_bytes([data[28], data[29]]) != 24 {
            self.status = crate::gui::ui_text("BMP açılamadı", "Could not open BMP");
            return false;
        }
        let raw_w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let raw_h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        if raw_w == 0 || raw_h == 0 { self.status = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        let width = raw_w.unsigned_abs() as usize;
        let height = raw_h.unsigned_abs() as usize;
        let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
        let row_bytes = match width.checked_mul(3).and_then(|n| n.checked_add(3)) {
            Some(n) => n & !3,
            None => { self.status = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        };
        let bytes = match row_bytes.checked_mul(height) {
            Some(n) if offset >= 54 && offset.checked_add(n).map_or(false, |end| end <= data.len()) => n,
            _ => { self.status = crate::gui::ui_text("Eksik BMP verisi", "Incomplete BMP data"); return false; }
        };
        if self.image.try_reserve_exact(bytes).is_err() {
            self.status = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
            return false;
        }
        self.image.clear();
        self.image.extend_from_slice(&data[offset..offset + bytes]);
        self.width = width;
        self.height = height;
        self.top_down = raw_h < 0;
        self.file_name = "cizim.bmp";
        self.status = crate::gui::ui_text("BMP açıldı", "BMP opened");
        true
    }

    pub fn zoom_percent(&self) -> usize { self.zoom_level.saturating_mul(100) / 4 }
    pub fn zoom_in(&mut self) { self.zoom_level = (self.zoom_level + 1).min(8); }
    pub fn zoom_out(&mut self) { self.zoom_level = self.zoom_level.saturating_sub(1).max(1); }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        // Araç Çubuğu
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 24, Color::TITLE_BAR);
        MinGuiCompositor::draw_string_clipped(fb, win_x + 12, win_y + 36,
            self.file_name, Color::TEXT_MAIN,
            (win_x + 8, win_y + 32, win_w.saturating_sub(200), 20));

        // Yakınlaştırma ve Düğmeler
        fb.draw_rect(win_x + win_w - 180, win_y + 32, 50, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 170, win_y + 36, crate::gui::ui_text("Yakın+", "Zoom+"), Color::ACCENT_GREEN);

        fb.draw_rect(win_x + win_w - 124, win_y + 32, 50, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 116, win_y + 36, crate::gui::ui_text("Uzak-", "Zoom-"), Color::ACCENT_YELLOW);

        fb.draw_rect(win_x + win_w - 68, win_y + 32, 56, 20, Color::ACCENT_BLUE);
        let mut zoom = crate::gui::TextBuf::new();
        zoom.push_byte(b'%');
        zoom.push_u2(self.zoom_percent() as u32);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 60, win_y + 36, zoom.as_str(), MinGuiCompositor::text_on(Color::ACCENT_BLUE));

        // Resim Tuvali Alanı (Canvas Area with checkerboard background)
        let canvas_x = win_x + 10;
        let canvas_y = win_y + 60;
        let canvas_w = 210;
        let canvas_h = 180;
        fb.draw_rect(canvas_x, canvas_y, canvas_w, canvas_h, Color::TASKBAR_BG);

        // Dama tahtası deseni (Şeffaflık / Tuval hissi)
        let tile_size = 10;
        for ty in (0..canvas_h).step_by(tile_size) {
            for tx in (0..canvas_w).step_by(tile_size) {
                if ((tx / tile_size) + (ty / tile_size)) % 2 == 1 {
                    fb.draw_rect(canvas_x + tx, canvas_y + ty, tile_size, tile_size, Color::DARK_BG);
                }
            }
        }

        let scale = self.zoom_level;
        // 16x16 resim; konum ve ölçü zoom ile birlikte hesaplanır.
        let art_size = 16 * scale;
        let art_ox = canvas_x + canvas_w.saturating_sub(art_size) / 2;
        let art_oy = canvas_y + canvas_h.saturating_sub(art_size) / 2;

        if self.width > 0 && self.height > 0 {
            let scale = self.zoom_level.max(1);
            let (src_w, src_h) = (self.width, self.height);
            let row_bytes = (src_w * 3 + 3) & !3;
            let fit_scale = scale.min((canvas_w / src_w.max(1)).max(1)).min((canvas_h / src_h.max(1)).max(1));
            let draw_w = src_w.saturating_mul(fit_scale).min(canvas_w);
            let draw_h = src_h.saturating_mul(fit_scale).min(canvas_h);
            let art_ox = canvas_x + canvas_w.saturating_sub(draw_w) / 2;
            let art_oy = canvas_y + canvas_h.saturating_sub(draw_h) / 2;
            for dy in 0..draw_h {
                let sy = dy / fit_scale;
                let bmp_y = if self.top_down { sy } else { src_h - 1 - sy };
                for dx in 0..draw_w {
                    let sx = dx / fit_scale;
                    let i = bmp_y * row_bytes + sx * 3;
                    if i + 2 < self.image.len() {
                        let color = (self.image[i + 2] as u32) << 16 | (self.image[i + 1] as u32) << 8 | self.image[i] as u32;
                        fb.draw_rect(art_ox + dx, art_oy + dy, 1, 1, color);
                    }
                }
            }
        } else {
        const ART_DATA: [u16; 16] = [
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

        for (row, &mask) in ART_DATA.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if (row == 4 || row == 5) && (col == 2 || col == 3 || col == 12 || col == 13) {
                        0x00FFFF // Parlak Neon Camgöbeği Gözler
                    } else if row == 8 && (col == 7 || col == 8) {
                        Color::ACCENT_PINK // Pembe Burun
                    } else if row <= 1 {
                        Color::ACCENT_CAT_EARS // Pembe Kulaklar
                    } else if row >= 10 && row <= 11 {
                        Color::ACCENT_BLUE // Siber Tasma
                    } else {
                        Color::TEXT_MAIN // Sevimli Kedi Gövdesi
                    };
                    fb.draw_rect(art_ox + col * scale, art_oy + row * scale, scale, scale, c);
                }
            }
        }
        }

        // Sağ Bilgi Paneli
        let info_x = win_x + canvas_w + 24;
        let info_y = win_y + 60;
        fb.draw_rect(info_x, info_y, win_w - canvas_w - 34, canvas_h, Color::TITLE_BAR_INACTIVE);

        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 10, crate::gui::ui_text("RESİM BİLGİSİ", "IMAGE INFO"), Color::ACCENT_BLUE);
        let (image_w, image_h) = if self.width > 0 { (self.width, self.height) } else { (16, 16) };
        let mut resolution = crate::gui::TextBuf::new();
        resolution.push_str(crate::gui::ui_text("Çözünürlük: ", "Resolution: "));
        resolution.push_usize(image_w); resolution.push_byte(b'x'); resolution.push_usize(image_h);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 32, resolution.as_str(), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 52, "Format: BMP (RGB32)", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 72, crate::gui::ui_text("Boyut: 1.024 B", "Size: 1,024 B"), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 92, crate::gui::ui_text("Renk: 5 renk", "Colors: 5"), Color::TEXT_MAIN);
        let mut zoom_info = crate::gui::TextBuf::new();
        zoom_info.push_str(crate::gui::ui_text("Yakınlaştırma: ", "Zoom: "));
        zoom_info.push_byte(b'%');
        zoom_info.push_u2(self.zoom_percent() as u32);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 112, zoom_info.as_str(), Color::ACCENT_YELLOW);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 132, self.status, Color::ACCENT_GREEN);

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 246, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 250, crate::gui::ui_text("Görüntüleyici: MinViewer v1.0 | 60 FPS Render", "Viewer: MinViewer v1.0 | 60 FPS Render"), Color::TEXT_MUTED);
    }
}

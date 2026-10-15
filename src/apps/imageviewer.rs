use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct ImageViewerApp {
    pub zoom_level: usize,
    pub image: alloc::sync::Arc<[u8]>,
    pub width: usize,
    pub height: usize,
    pub top_down: bool,
    pub data_offset: usize,
    pub file_name: &'static str,
    pub status: &'static str,
    pub pan_x: isize,
    pub pan_y: isize,
}

impl ImageViewerApp {
    pub fn new() -> Self {
        Self { zoom_level: 4, image: alloc::sync::Arc::from([]), width: 0, height: 0, top_down: false, data_offset: 0, file_name: "kedi_maskotu.bmp", status: "Ready", pan_x: 0, pan_y: 0 }
    }

    pub fn open_bmp(&mut self, data: alloc::sync::Arc<[u8]>) -> bool {
        let bytes_data = &data[..];
        if bytes_data.len() < 54 || &bytes_data[0..2] != b"BM" || u16::from_le_bytes([bytes_data[28], bytes_data[29]]) != 24 {
            self.status = crate::gui::ui_text("BMP açılamadı", "Could not open BMP");
            return false;
        }
        let raw_w = i32::from_le_bytes([bytes_data[18], bytes_data[19], bytes_data[20], bytes_data[21]]);
        let raw_h = i32::from_le_bytes([bytes_data[22], bytes_data[23], bytes_data[24], bytes_data[25]]);
        if raw_w == 0 || raw_h == 0 { self.status = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        let width = raw_w.unsigned_abs() as usize;
        let height = raw_h.unsigned_abs() as usize;
        let offset = u32::from_le_bytes([bytes_data[10], bytes_data[11], bytes_data[12], bytes_data[13]]) as usize;
        let row_bytes = match width.checked_mul(3).and_then(|n| n.checked_add(3)) {
            Some(n) => n & !3,
            None => { self.status = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        };
        let bytes = match row_bytes.checked_mul(height) {
            Some(n) if offset >= 54 && offset.checked_add(n).map_or(false, |end| end <= bytes_data.len()) => n,
            _ => { self.status = crate::gui::ui_text("Eksik BMP verisi", "Incomplete BMP data"); return false; }
        };
        let _ = bytes;
        self.image = data;
        self.pan_x = 0;
        self.pan_y = 0;
        self.data_offset = offset;
        self.width = width;
        self.height = height;
        self.top_down = raw_h < 0;
        self.file_name = "cizim.bmp";
        self.status = crate::gui::ui_text("BMP açıldı", "BMP opened");
        true
    }

    pub fn clear_image(&mut self, file_name: &'static str) {
        self.image = alloc::sync::Arc::from([]);
        self.pan_x = 0;
        self.pan_y = 0;
        self.width = 0;
        self.height = 0;
        self.data_offset = 0;
        self.file_name = file_name;
        self.status = crate::gui::ui_text("Resim yüklenemedi", "Image could not be loaded");
    }

    pub fn zoom_percent(&self) -> usize { self.zoom_level.saturating_mul(100) / 4 }
    pub fn zoom_in(&mut self) { self.zoom_level = (self.zoom_level + 1).min(8); }
    pub fn zoom_out(&mut self) { self.zoom_level = self.zoom_level.saturating_sub(1).max(1); }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize) {
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

        // Responsive image and information panes. Narrow windows stack the
        // panes; wider windows share the available content width.
        let margin = 10usize;
        let gap = 12usize;
        let inner_x = win_x + margin;
        let inner_w = win_w.saturating_sub(margin * 2).max(1);
        let content_y = win_y + 60;
        let status_y = win_y + win_h.saturating_sub(24);
        let content_h = status_y.saturating_sub(content_y + 6).max(1);
        let stacked = inner_w < 360;
        let (canvas_x, canvas_y, canvas_w, canvas_h, info_x, info_y, info_w, info_h) = if stacked {
            let canvas_h = (content_h * 2 / 3).max(1);
            let info_y = content_y + canvas_h + gap.min(content_h.saturating_sub(canvas_h));
            let info_h = status_y.saturating_sub(info_y + 4).max(1);
            (inner_x, content_y, inner_w, canvas_h, inner_x, info_y, inner_w, info_h)
        } else {
            let panel_w = (inner_w / 3).clamp(130, 220).min(inner_w.saturating_sub(gap + 1));
            let canvas_w = inner_w.saturating_sub(panel_w + gap).max(1);
            (inner_x, content_y, canvas_w, content_h, inner_x + canvas_w + gap, content_y, panel_w, content_h)
        };

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
            let (src_w, src_h) = (self.width, self.height);
            let row_bytes = (src_w * 3 + 3) & !3;
            // Base scale fits large images to the canvas. The zoom buttons
            // then apply 25%..200% of that base (100% at level 4).
            const SCALE_UNIT: usize = 16;
            let base_scale = if src_w > canvas_w || src_h > canvas_h {
                (canvas_w.saturating_mul(SCALE_UNIT) / src_w.max(1))
                    .min(canvas_h.saturating_mul(SCALE_UNIT) / src_h.max(1))
                    .max(1)
            } else {
                SCALE_UNIT
            };
            let scale = (base_scale.saturating_mul(self.zoom_level.max(1)) / 4).max(1);
            let scaled_w = src_w.saturating_mul(scale) / SCALE_UNIT;
            let scaled_h = src_h.saturating_mul(scale) / SCALE_UNIT;
            let left = (canvas_w as isize - scaled_w as isize) / 2 + self.pan_x;
            let top = (canvas_h as isize - scaled_h as isize) / 2 + self.pan_y;

            for dy in 0..canvas_h {
                let image_y = dy as isize - top;
                if image_y < 0 || image_y as usize >= scaled_h { continue; }
                let sy = ((image_y as usize).saturating_mul(SCALE_UNIT) / scale).min(src_h - 1);
                let bmp_y = if self.top_down { sy } else { src_h - 1 - sy };
                for dx in 0..canvas_w {
                    let image_x = dx as isize - left;
                    if image_x < 0 || image_x as usize >= scaled_w { continue; }
                    let sx = ((image_x as usize).saturating_mul(SCALE_UNIT) / scale).min(src_w - 1);
                    let i = self.data_offset + bmp_y * row_bytes + sx * 3;
                    if i + 2 >= self.image.len() { continue; }
                    let color = (self.image[i + 2] as u32) << 16
                        | (self.image[i + 1] as u32) << 8
                        | self.image[i] as u32;
                    fb.draw_rect(canvas_x + dx, canvas_y + dy, 1, 1, color);
                }
            }
        } else if self.width == 0 && self.status != crate::gui::ui_text("Ready", "Ready") {
            MinGuiCompositor::draw_string(fb, canvas_x + 12, canvas_y + 12, self.status, Color::ACCENT_RED);
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

        // Sağ/alt bilgi paneli, kalan alanı tamamen doldurur.
        fb.draw_rect(info_x, info_y, info_w, info_h, Color::TITLE_BAR_INACTIVE);

        let row_step = (info_h.saturating_sub(8) / 6).clamp(14, 22);
        let info_clip = (info_x + 8, info_y + 4, info_w.saturating_sub(16), info_h.saturating_sub(8));
        MinGuiCompositor::draw_string_clipped(fb, info_x + 10, info_y + 6,
            crate::gui::ui_text("RESİM BİLGİSİ", "IMAGE INFO"), Color::ACCENT_BLUE, info_clip);
        let (image_w, image_h) = if self.width > 0 { (self.width, self.height) } else { (16, 16) };
        let mut resolution = crate::gui::TextBuf::new();
        resolution.push_str(crate::gui::ui_text("Çözünürlük: ", "Resolution: "));
        resolution.push_usize(image_w); resolution.push_byte(b'x'); resolution.push_usize(image_h);
        let info_lines = [
            resolution.as_str(),
            "Format: BMP (24-bit)",
            crate::gui::ui_text("Boyut: RGB", "Data: RGB"),
            crate::gui::ui_text("Renk: 24-bit", "Color: 24-bit"),
        ];
        for (i, line) in info_lines.iter().enumerate() {
            let y = info_y + 6 + (i + 1) * row_step;
            MinGuiCompositor::draw_string_clipped(fb, info_x + 10, y, line, Color::TEXT_MAIN,
                info_clip);
        }
        let mut zoom_info = crate::gui::TextBuf::new();
        zoom_info.push_str(crate::gui::ui_text("Yakınlaştırma: ", "Zoom: "));
        zoom_info.push_byte(b'%');
        zoom_info.push_u2(self.zoom_percent() as u32);
        let zoom_y = info_y + 6 + 5 * row_step;
        MinGuiCompositor::draw_string_clipped(fb, info_x + 10, zoom_y, zoom_info.as_str(), Color::ACCENT_YELLOW,
            info_clip);
        let status_line_y = zoom_y + row_step;
        MinGuiCompositor::draw_string_clipped(fb, info_x + 10, status_line_y, self.status, Color::ACCENT_GREEN,
            info_clip);

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, status_y, win_w.saturating_sub(12), 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string_clipped(fb, win_x + 12, status_y + 4,
            crate::gui::ui_text("Görüntüleyici: MinViewer v1.0", "Viewer: MinViewer v1.0"), Color::TEXT_MUTED,
            (win_x + 8, status_y + 2, win_w.saturating_sub(16), 14));
    }
}

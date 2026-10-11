use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct ImageViewerApp {
    pub zoom_level: usize,
}

impl ImageViewerApp {
    pub const fn new() -> Self {
        Self { zoom_level: 4 }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        // Araç Çubuğu
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 24, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 36, "kedi_maskotu.bmp (32x32)", Color::TEXT_MAIN);

        // Yakınlaştırma ve Düğmeler
        fb.draw_rect(win_x + win_w - 180, win_y + 32, 50, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 170, win_y + 36, crate::gui::ui_text("Yakın+", "Zoom+"), Color::ACCENT_GREEN);

        fb.draw_rect(win_x + win_w - 124, win_y + 32, 50, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 116, win_y + 36, crate::gui::ui_text("Uzak-", "Zoom-"), Color::ACCENT_YELLOW);

        fb.draw_rect(win_x + win_w - 68, win_y + 32, 56, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 60, win_y + 36, "%400", Color::DARK_BG);

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

        // 16x16 Cyber-Cat Pixel-Art Resmi (4x Zoom ile 64x64 Çizim)
        let art_ox = canvas_x + (canvas_w - 64) / 2;
        let art_oy = canvas_y + (canvas_h - 64) / 2;
        let scale = self.zoom_level;

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

        // Sağ Bilgi Paneli
        let info_x = win_x + canvas_w + 24;
        let info_y = win_y + 60;
        fb.draw_rect(info_x, info_y, win_w - canvas_w - 34, canvas_h, Color::TITLE_BAR_INACTIVE);

        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 10, crate::gui::ui_text("RESİM BİLGİSİ", "IMAGE INFO"), Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 32, crate::gui::ui_text("Çözünürlük: 32x32", "Resolution: 32x32"), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 52, "Format: BMP (RGB32)", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 72, crate::gui::ui_text("Boyut: 4.096 B", "Size: 4,096 B"), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 92, crate::gui::ui_text("Renk Paleti: 16 Renk", "Color Palette: 16 Colors"), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 112, crate::gui::ui_text("Yakınlaştırma: aktif", "Zoom: active"), Color::ACCENT_YELLOW);
        MinGuiCompositor::draw_string(fb, info_x + 10, info_y + 132, crate::gui::ui_text("Durum: Başarılı", "Status: Ready"), Color::ACCENT_GREEN);

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 246, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 250, crate::gui::ui_text("Görüntüleyici: MinViewer v1.0 | 60 FPS Render", "Viewer: MinViewer v1.0 | 60 FPS Render"), Color::TEXT_MUTED);
    }
}

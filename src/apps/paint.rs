use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

/// Tuval üzerinde tutulabilecek en fazla nokta.
const MAX_POINTS: usize = 4096;
/// Fırça karesinin kenar uzunluğu (px) — kalemin merkezi bu kareye ortalanır.
const BRUSH_SIZE: usize = 7;
/// Silginin yarıçapı (px).
const ERASER_RADIUS: usize = 14;
/// Araç çubuğu yüksekliği (px). Pencere içi tuvalin üst boşluğu buradan gelir.
const TOOLBAR_H: usize = 58;
const EMPTY_POINT: PaintPoint = PaintPoint { x: 0, y: 0, color: Color::ACCENT_BLUE };

const BMP_W: usize = 800;
const BMP_H: usize = 600;

#[derive(Clone, Copy)]
pub struct PaintPoint {
    pub x: usize,
    pub y: usize,
    pub color: u32,
}

pub struct PaintApp {
    pub len: usize,
    pub points: alloc::boxed::Box<[PaintPoint]>,
    pub color: u32,
    pub eraser: bool,
    pub last: Option<(usize, usize)>,
    pub status_msg: &'static str,
    /// Son üretilen BMP; VFS'nin RAM kopyası olmadan kaydedilip tekrar açılır.
    pub saved_bmp: alloc::sync::Arc<[u8]>,
}

impl PaintApp {
    pub fn new() -> Self {
        Self {
            len: 0,
            points: alloc::vec![EMPTY_POINT; MAX_POINTS].into_boxed_slice(),
            color: Color::ACCENT_BLUE,
            eraser: false,
            last: None,
            status_msg: "",
            saved_bmp: alloc::sync::Arc::from([]),
        }
    }

    /// Tuvalin başlangıç X koordinatı (pencere içi).
    pub fn canvas_x() -> usize { 8 }
    /// Tuvalin başlangıç Y koordinatı (pencere içi, araç çubuğunun altı).
    pub fn canvas_y() -> usize { TOOLBAR_H }

    /// Tuval alanının (x, y, w, h) pencere içi ölçülerini hesaplar.
    pub fn canvas_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let x = Self::canvas_x();
        let y = Self::canvas_y();
        let cw = w.saturating_sub(16);
        let ch = h.saturating_sub(TOOLBAR_H + 10);
        (x, y, cw, ch)
    }

    /// Pencere içi (px, py) koordinatını tuval koordinatına çevirir.
    /// Tuval dışındaysa `None` döner.
    pub fn to_canvas(&self, w: usize, h: usize, px: usize, py: usize) -> Option<(usize, usize)> {
        let (cx, cy, cw, ch) = Self::canvas_rect(w, h);
        if cw == 0 || ch == 0 || px < cx || py < cy || px >= cx + cw || py >= cy + ch {
            return None;
        }
        Some((px - cx, py - cy))
    }

    /// Verilen tuval noktasına fırça uygular. Windows Paint gibi: hızlı fare
    /// hareketlerinde iki nokta arasını enterpolasyonla doldurarak sürekli ve
    /// akıcı bir çizgi üretir (noktalı/kesik çizgi oluşmaz).
    pub fn paint(&mut self, x: usize, y: usize) {
        if self.eraser {
            self.erase(x, y);
            self.last = Some((x, y));
            return;
        }

        let (sx, sy) = self.last.unwrap_or((x, y));
        let dx = x as isize - sx as isize;
        let dy = y as isize - sy as isize;
        let steps = dx.unsigned_abs().max(dy.unsigned_abs()).max(1);
        for i in 1..=steps {
            if self.len >= MAX_POINTS {
                break;
            }
            let px = (sx as isize + dx * i as isize / steps as isize).max(0) as usize;
            let py = (sy as isize + dy * i as isize / steps as isize).max(0) as usize;
            self.points[self.len] = PaintPoint { x: px, y: py, color: self.color };
            self.len += 1;
        }
        self.last = Some((x, y));
    }

    /// Silgi: merkez çevresindeki noktaları dairesel olarak kaldırır.
    pub fn erase(&mut self, x: usize, y: usize) {
        let mut out = 0;
        for i in 0..self.len {
            let point = self.points[i];
            let ddx = point.x.abs_diff(x) as isize;
            let ddy = point.y.abs_diff(y) as isize;
            let remove = ddx * ddx + ddy * ddy <= (ERASER_RADIUS * ERASER_RADIUS) as isize;
            if !remove {
                self.points[out] = point;
                out += 1;
            }
        }
        self.len = out;
    }

    pub fn end_stroke(&mut self) {
        self.last = None;
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.last = None;
    }

    pub fn save_bmp(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem, canvas_w: usize, canvas_h: usize) -> bool {
        let w = canvas_w.min(BMP_W).max(1); let h = canvas_h.min(BMP_H).max(1);
        let row_bytes = (w * 3 + 3) & !3;
        let image_size = row_bytes * h;
        let file_size = 54 + image_size;
        let mut header = [0u8; 54];
        header[0..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        header[10..14].copy_from_slice(&(54u32).to_le_bytes());
        header[14..18].copy_from_slice(&(40u32).to_le_bytes());
        header[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        header[22..26].copy_from_slice(&(h as i32).to_le_bytes());
        header[26..28].copy_from_slice(&(1u16).to_le_bytes());
        header[28..30].copy_from_slice(&(24u16).to_le_bytes());
        header[34..38].copy_from_slice(&(image_size as u32).to_le_bytes());
        // Build the BMP in one buffer. Keeping separate `image` and `bmp`
        // vectors temporarily needs nearly 3 MiB, larger than the kernel heap.
        let mut bmp = alloc::vec::Vec::new();
        if bmp.try_reserve_exact(file_size).is_err() {
            self.status_msg = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
            return false;
        }
        bmp.resize(file_size, 0xFF);
        bmp[..54].copy_from_slice(&header);
        for i in 0..self.len {
            let p = self.points[i]; if p.x >= w || p.y >= h { continue; }
            let base = 54 + (h - 1 - p.y) * row_bytes + p.x * 3;
            bmp[base] = (p.color & 0xFF) as u8;
            bmp[base + 1] = ((p.color >> 8) & 0xFF) as u8;
            bmp[base + 2] = ((p.color >> 16) & 0xFF) as u8;
        }
        // Retain the bytes in the Paint instance and let VFS index them without
        // cloning a ~1.4 MiB file into the small kernel heap.
        let saved_bmp: alloc::sync::Arc<[u8]> = bmp.into();
        let ok = vfs.write_large_file("/root/Documents/cizim.bmp", saved_bmp.clone());
        if ok { self.saved_bmp = saved_bmp; }
        self.status_msg = if ok { crate::gui::ui_text("Çizim kaydedildi", "Drawing saved") } else { crate::gui::ui_text("Kaydetme başarısız", "Save failed") };
        ok
    }

    pub fn open_bmp(&mut self, vfs: &crate::fs::vfs::VirtualFileSystem) -> bool {
        let stored;
        let data: &[u8] = if !self.saved_bmp.is_empty() {
            &self.saved_bmp
        } else {
            stored = match vfs.read_large_file("/root/Documents/cizim.bmp") {
                Some(d) => d,
                None => {
                    match vfs.read_file("/root/Documents/cizim.bmp") {
                        Some(_) => { self.status_msg = crate::gui::ui_text("BMP açılamadı", "Could not open BMP"); return false; }
                        None => { self.status_msg = crate::gui::ui_text("BMP bulunamadı", "BMP not found"); return false; }
                    }
                }
            };
            &stored
        };
        if data.len() < 54 || &data[0..2] != b"BM" || u16::from_le_bytes([data[28], data[29]]) != 24 { self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        let w = (i32::from_le_bytes([data[18],data[19],data[20],data[21]])).unsigned_abs() as usize;
        let h = (i32::from_le_bytes([data[22],data[23],data[24],data[25]])).unsigned_abs() as usize;
        let top_down = i32::from_le_bytes([data[22],data[23],data[24],data[25]]) < 0;
        let offset = u32::from_le_bytes([data[10],data[11],data[12],data[13]]) as usize;
        let row_bytes = (w.checked_mul(3).and_then(|n| n.checked_add(3)).unwrap_or(usize::MAX)) & !3;
        if offset < 54 || row_bytes == usize::MAX || h > (data.len().saturating_sub(offset) / row_bytes.max(1)) + 1 {
            self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false;
        }
        self.len = 0;
        for y in 0..h.min(BMP_H) { for x in 0..w.min(BMP_W) {
            let sy = if top_down { y } else { h - 1 - y }; let i = offset + sy * row_bytes + x * 3;
            if i + 2 >= data.len() { continue; }
            let color = (data[i + 2] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i] as u32;
            if color != 0xFFFFFF && self.len < MAX_POINTS { self.points[self.len] = PaintPoint { x, y, color }; self.len += 1; }
        }
        }
        self.last = None;
        self.status_msg = crate::gui::ui_text("BMP açıldı", "BMP opened");
        true
    }

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let (cx, cy, cw, ch) = Self::canvas_rect(w, h);
        let canvas_x = x + cx;
        let canvas_y = y + cy;
        let canvas_w = cw;
        let canvas_h = ch;
        fb.draw_rect(canvas_x, canvas_y, canvas_w, canvas_h, Color::WHITE);
        fb.draw_rect_outline(canvas_x, canvas_y, canvas_w, canvas_h, Color::BORDER);
        for i in 0..self.len {
            let point = self.points[i];
            if point.x < canvas_w && point.y < canvas_h {
                // Noktayı fırça boyutuna göre ortala (kalem merkezli çizim).
                let px = canvas_x + point.x.saturating_sub(BRUSH_SIZE / 2);
                let py = canvas_y + point.y.saturating_sub(BRUSH_SIZE / 2);
                fb.draw_rect(px, py, BRUSH_SIZE, BRUSH_SIZE, point.color);
            }
        }

        let toolbar_width = w.saturating_sub(16);
        let palette_start = if toolbar_width >= 258 { x + 58 } else { x + 40 };
        MinGuiCompositor::draw_string(fb, x + 14, y + 40, crate::gui::ui_text("Kalem:", "Pen:"), Color::TEXT_MAIN);
        for (i, color) in [Color::ACCENT_BLUE, Color::ACCENT_RED, Color::ACCENT_GREEN, Color::BLACK]
            .iter()
            .enumerate()
        {
            let swatch_x = palette_start + i * 22;
            fb.draw_rect(swatch_x, y + 36, 16, 16, *color);
            if !self.eraser && self.color == *color {
                fb.draw_rect(swatch_x.saturating_sub(2), y + 34, 20, 2, Color::TEXT_MAIN);
                fb.draw_rect(swatch_x.saturating_sub(2), y + 52, 20, 2, Color::TEXT_MAIN);
            }
        }
        let eraser_x = palette_start + 92;
        fb.draw_rect(eraser_x, y + 36, 58, 16, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(
            fb,
            eraser_x + 6,
            y + 40,
            crate::gui::ui_text("Silgi", "Eraser"),
            Color::TEXT_MAIN,
        );
        if self.eraser {
            fb.draw_rect(eraser_x.saturating_sub(2), y + 34, 62, 2, Color::TEXT_MAIN);
            fb.draw_rect(eraser_x.saturating_sub(2), y + 52, 62, 2, Color::TEXT_MAIN);
        }

        let clear_x = x + w.saturating_sub(66);
        fb.draw_rect(clear_x, y + 34, 58, 22, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(
            fb,
            clear_x + 6,
            y + 40,
            crate::gui::ui_text("Temizle", "Clear"),
            Color::TEXT_MAIN,
        );
        let save_x = x + w.saturating_sub(132);
        fb.draw_rect(save_x, y + 34, 58, 22, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, save_x + 6, y + 40, crate::gui::ui_text("Kaydet", "Save"), MinGuiCompositor::text_on(Color::ACCENT_BLUE));
        let open_x = x + w.saturating_sub(198);
        fb.draw_rect(open_x, y + 34, 58, 22, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, open_x + 6, y + 40, crate::gui::ui_text("Aç", "Open"), Color::TEXT_MAIN);
        if !self.status_msg.is_empty() { MinGuiCompositor::draw_string(fb, x + 14, y + h.saturating_sub(18), self.status_msg, Color::ACCENT_GREEN); }
    }
}

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

static mut POINTS: [PaintPoint; MAX_POINTS] = [EMPTY_POINT; MAX_POINTS];

#[derive(Clone, Copy)]
pub struct PaintPoint {
    pub x: usize,
    pub y: usize,
    pub color: u32,
}

pub struct PaintApp {
    pub len: usize,
    pub color: u32,
    pub eraser: bool,
    pub last: Option<(usize, usize)>,
}

impl PaintApp {
    pub const fn new() -> Self {
        Self {
            len: 0,
            color: Color::ACCENT_BLUE,
            eraser: false,
            last: None,
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
            unsafe { (*core::ptr::addr_of_mut!(POINTS))[self.len] = PaintPoint { x: px, y: py, color: self.color }; }
            self.len += 1;
        }
        self.last = Some((x, y));
    }

    /// Silgi: merkez çevresindeki noktaları dairesel olarak kaldırır.
    pub fn erase(&mut self, x: usize, y: usize) {
        let points = core::ptr::addr_of_mut!(POINTS);
        let mut out = 0;
        for i in 0..self.len {
            let point = unsafe { (*points)[i] };
            let ddx = point.x.abs_diff(x) as isize;
            let ddy = point.y.abs_diff(y) as isize;
            let remove = ddx * ddx + ddy * ddy <= (ERASER_RADIUS * ERASER_RADIUS) as isize;
            if !remove {
                unsafe { (*points)[out] = point; }
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

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let (cx, cy, cw, ch) = Self::canvas_rect(w, h);
        let canvas_x = x + cx;
        let canvas_y = y + cy;
        let canvas_w = cw;
        let canvas_h = ch;
        fb.draw_rect(canvas_x, canvas_y, canvas_w, canvas_h, Color::WHITE);
        fb.draw_rect_outline(canvas_x, canvas_y, canvas_w, canvas_h, Color::BORDER);
        let points = core::ptr::addr_of!(POINTS);
        for i in 0..self.len {
            let point = unsafe { (*points)[i] };
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
        fb.draw_rect(clear_x, y + 36, 58, 16, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(
            fb,
            clear_x + 6,
            y + 40,
            crate::gui::ui_text("Temizle", "Clear"),
            Color::TEXT_MAIN,
        );
    }
}

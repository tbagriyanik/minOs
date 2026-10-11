use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

const MAX_POINTS: usize = 8192;
const BRUSH_SIZE: usize = 7;
const ERASER_RADIUS: usize = 10;

#[derive(Clone, Copy)]
pub struct PaintPoint {
    pub x: usize,
    pub y: usize,
    pub color: u32,
}

pub struct PaintApp {
    pub points: [PaintPoint; MAX_POINTS],
    pub len: usize,
    pub color: u32,
    pub eraser: bool,
    pub last: Option<(usize, usize)>,
}

impl PaintApp {
    pub const fn new() -> Self {
        Self {
            points: [PaintPoint { x: 0, y: 0, color: Color::ACCENT_BLUE }; MAX_POINTS],
            len: 0,
            color: Color::ACCENT_BLUE,
            eraser: false,
            last: None,
        }
    }

    pub fn paint(&mut self, x: usize, y: usize) {
        if self.eraser {
            let mut out = 0;
            for i in 0..self.len {
                let point = self.points[i];
                if point.x.abs_diff(x) > ERASER_RADIUS || point.y.abs_diff(y) > ERASER_RADIUS {
                    self.points[out] = point;
                    out += 1;
                }
            }
            self.len = out;
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

    pub fn end_stroke(&mut self) {
        self.last = None;
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.last = None;
    }

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let canvas_x = x + 8;
        let canvas_y = y + 58;
        let canvas_w = w.saturating_sub(16);
        let canvas_h = h.saturating_sub(68);
        fb.draw_rect(canvas_x, canvas_y, canvas_w, canvas_h, Color::WHITE);
        for i in 0..self.len {
            let point = self.points[i];
            if point.x < canvas_w && point.y < canvas_h {
                fb.draw_rect(canvas_x + point.x, canvas_y + point.y, BRUSH_SIZE, BRUSH_SIZE, point.color);
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

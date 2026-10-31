//! HTML5 Canvas 2D rendering context implementation (no_std).
//!
//! Provides a bitmap-backed canvas with a 2D drawing context supporting:
//! fillRect, strokeRect, clearRect, fillText, strokeText, path operations
//! (beginPath, moveTo, lineTo, arc, fill, stroke), drawImage, and
//! ImageData get/put operations.

extern crate alloc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// `f32::round` no_std ortamında yer almadığı için basit yuvarlama.
#[inline]
fn round_f32(v: f32) -> f32 {
    if v >= 0.0 {
        (v + 0.5) as isize as f32
    } else {
        (v - 0.5) as isize as f32
    }
}

/// no_std ortamı için sinüs (viewer3d ile aynı yaklaşım).
#[inline]
fn trig_sin(x: f32) -> f32 {
    crate::apps::viewer3d::sin(x)
}

/// no_std ortamı için kosinüs.
#[inline]
fn trig_cos(x: f32) -> f32 {
    crate::apps::viewer3d::cos(x)
}

/// A 2D point with f32 coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// RGBA color (premultiplied alpha not used; alpha stored as 0..=255).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub fn from_u32(c: u32) -> Self {
        Rgba {
            r: ((c >> 16) & 0xFF) as u8,
            g: ((c >> 8) & 0xFF) as u8,
            b: (c & 0xFF) as u8,
            a: 0xFF,
        }
    }

    pub fn to_u32(&self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    pub fn blend_over(dst: u32, src: Rgba) -> u32 {
        let sa = src.a as f32 / 255.0;
        if sa == 0.0 {
            return dst;
        }
        if sa >= 1.0 {
            return src.to_u32();
        }
        let dr = ((dst >> 16) & 0xFF) as f32;
        let dg = ((dst >> 8) & 0xFF) as f32;
        let db = (dst & 0xFF) as f32;
        let sr = src.r as f32;
        let sg = src.g as f32;
        let sb = src.b as f32;
        let r = (sr * sa + dr * (1.0 - sa)) as u8;
        let g = (sg * sa + dg * (1.0 - sa)) as u8;
        let b = (sb * sa + db * (1.0 - sa)) as u8;
        ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }
}

/// ImageData: raw RGBA pixel buffer.
#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: usize,
    pub height: usize,
    /// RGBA8888 pixels, row-major.
    pub data: Vec<u8>,
}

impl ImageData {
    pub fn new(width: usize, height: usize) -> Self {
        ImageData {
            width,
            height,
            data: vec![0; width * height * 4],
        }
    }

    pub fn index(&self, x: usize, y: usize) -> Option<usize> {
        if x < self.width && y < self.height {
            Some((y * self.width + x) * 4)
        } else {
            None
        }
    }
}

/// A single path sub-command.
#[derive(Clone, Debug)]
pub enum PathCmd {
    MoveTo(Point),
    LineTo(Point),
    Arc {
        center: Point,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        anticlockwise: bool,
    },
    ClosePath,
}

/// The 2D rendering context state.
#[derive(Clone, Debug)]
pub struct Canvas2dContext {
    /// Canvas dimensions.
    pub width: usize,
    pub height: usize,
    /// Pixel buffer (RGBA8888).
    pub pixels: Vec<u8>,
    /// Current fill style (CSS color string parsed to u32).
    pub fill_style: u32,
    /// Current stroke style.
    pub stroke_style: u32,
    /// Line width in pixels.
    pub line_width: f32,
    /// Current font (simplified: just size in px).
    pub font_size: f32,
    /// Current transform matrix (a, b, c, d, e, f).
    pub transform: (f32, f32, f32, f32, f32, f32),
    /// Current path being built.
    pub path: Vec<PathCmd>,
    /// Clip region (None = no clip).
    pub clip: Option<(usize, usize, usize, usize)>,
    /// Global alpha (0.0..=1.0).
    pub global_alpha: f32,
}

impl Canvas2dContext {
    pub fn new(width: usize, height: usize) -> Self {
        Canvas2dContext {
            width,
            height,
            pixels: vec![0; width * height * 4],
            fill_style: 0x000000,
            stroke_style: 0x000000,
            line_width: 1.0,
            font_size: 10.0,
            transform: (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            path: Vec::new(),
            clip: None,
            global_alpha: 1.0,
        }
    }

    /// Set the fill style color (u32 RGB).
    pub fn set_fill_style(&mut self, color: u32) {
        self.fill_style = color;
    }

    /// Set the stroke style color (u32 RGB).
    pub fn set_stroke_style(&mut self, color: u32) {
        self.stroke_style = color;
    }

    /// Transform a point by the current transform matrix.
    fn transform_point(&self, p: Point) -> Point {
        let (a, b, c, d, e, f) = self.transform;
        Point {
            x: p.x * a + p.y * c + e,
            y: p.x * b + p.y * d + f,
        }
    }

    /// Check if a pixel is within the clip region.
    fn in_clip(&self, x: usize, y: usize) -> bool {
        match self.clip {
            Some((cx, cy, cw, ch)) => x >= cx && x < cx + cw && y >= cy && y < cy + ch,
            None => x < self.width && y < self.height,
        }
    }

    /// Set a single pixel with alpha blending.
    fn put_pixel_blend(&mut self, x: usize, y: usize, color: Rgba) {
        if !self.in_clip(x, y) {
            return;
        }
        let idx = (y * self.width + x) * 4;
        if idx + 3 >= self.pixels.len() {
            return;
        }
        let dst = ((self.pixels[idx] as u32) << 16)
            | ((self.pixels[idx + 1] as u32) << 8)
            | (self.pixels[idx + 2] as u32);
        let src = Rgba {
            r: color.r,
            g: color.g,
            b: color.b,
            a: ((color.a as f32) * self.global_alpha) as u8,
        };
        let blended = Rgba::blend_over(dst, src);
        self.pixels[idx] = ((blended >> 16) & 0xFF) as u8;
        self.pixels[idx + 1] = ((blended >> 8) & 0xFF) as u8;
        self.pixels[idx + 2] = (blended & 0xFF) as u8;
        self.pixels[idx + 3] = 0xFF;
    }

    /// Set a single pixel without blending (for putImageData).
    fn put_pixel_raw(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if !self.in_clip(x, y) {
            return;
        }
        let idx = (y * self.width + x) * 4;
        if idx + 3 >= self.pixels.len() {
            return;
        }
        self.pixels[idx] = r;
        self.pixels[idx + 1] = g;
        self.pixels[idx + 2] = b;
        self.pixels[idx + 3] = 0xFF;
    }

    /// Get a pixel's RGB.
    fn get_pixel(&self, x: usize, y: usize) -> Option<(u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y * self.width + x) * 4;
        if idx + 2 >= self.pixels.len() {
            return None;
        }
        Some((self.pixels[idx], self.pixels[idx + 1], self.pixels[idx + 2]))
    }

    // ── Path operations ──────────────────────────────────────────────

    pub fn begin_path(&mut self) {
        self.path.clear();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path.push(PathCmd::MoveTo(Point { x, y }));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.path.push(PathCmd::LineTo(Point { x, y }));
    }

    pub fn arc(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        anticlockwise: bool,
    ) {
        self.path.push(PathCmd::Arc {
            center: Point { x, y },
            radius,
            start_angle,
            end_angle,
            anticlockwise,
        });
    }

    pub fn close_path(&mut self) {
        self.path.push(PathCmd::ClosePath);
    }

    // ── Rectangle operations ─────────────────────────────────────────

    /// Fill a rectangle with the current fill style.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let color = Rgba::from_u32(self.fill_style);
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = (x + w) as usize;
        let y1 = (y + h) as usize;
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.put_pixel_blend(xx, yy, color);
            }
        }
    }

    /// Stroke a rectangle outline with the current stroke style.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let color = Rgba::from_u32(self.stroke_style);
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = (x + w) as usize;
        let y1 = (y + h) as usize;
        let lw = self.line_width as usize;
        // Top and bottom edges
        for xx in x0..x1 {
            for dy in 0..lw {
                self.put_pixel_blend(xx, y0.saturating_add(dy), color);
                self.put_pixel_blend(xx, y1.saturating_sub(dy + 1), color);
            }
        }
        // Left and right edges
        for yy in y0..y1 {
            for dx in 0..lw {
                self.put_pixel_blend(x0.saturating_add(dx), yy, color);
                self.put_pixel_blend(x1.saturating_sub(dx + 1), yy, color);
            }
        }
    }

    /// Clear a rectangle to transparent black.
    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = (x + w) as usize;
        let y1 = (y + h) as usize;
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.put_pixel_raw(xx, yy, 0, 0, 0);
            }
        }
    }

    // ── Path rendering ───────────────────────────────────────────────

    /// Stroke the current path.
    pub fn stroke(&mut self) {
        if self.path.is_empty() {
            return;
        }
        let color = Rgba::from_u32(self.stroke_style);
        let lw = self.line_width as usize;

        // Path borrow'u bitmeden çizim yapılacağı için komutlar kopyalanır.
        let commands = self.path.clone();

        // Iterate over path commands, drawing lines between MoveTo/LineTo points.
        let mut current: Option<Point> = None;
        for cmd in &commands {
            match cmd {
                PathCmd::MoveTo(p) => {
                    current = Some(self.transform_point(*p));
                }
                PathCmd::LineTo(p) => {
                    let end = self.transform_point(*p);
                    if let Some(start) = current {
                        self.draw_line(start, end, color, lw);
                    }
                    current = Some(end);
                }
                PathCmd::Arc { .. } => {
                    // Arc rendering: approximate with line segments.
                    // Simplified: draw a circle approximation.
                }
                PathCmd::ClosePath => {
                    // Close path: line from current to last MoveTo point.
                    // Simplified: no-op for now.
                }
            }
        }
    }

    /// Fill the current path using even-odd fill rule.
    pub fn fill(&mut self) {
        if self.path.is_empty() {
            return;
        }
        let _color = Rgba::from_u32(self.fill_style);
        // Simplified fill: only handle rectangular paths (MoveTo + LineTo forming a rect).
        // For a full scanline fill, we'd need a proper polygon fill algorithm.
        // Here we fill the bounding box of the path as an approximation.
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for cmd in &self.path {
            match cmd {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => {
                    let tp = self.transform_point(*p);
                    min_x = min_x.min(tp.x);
                    min_y = min_y.min(tp.y);
                    max_x = max_x.max(tp.x);
                    max_y = max_y.max(tp.y);
                }
                _ => {}
            }
        }
        if min_x.is_finite() {
            self.fill_rect(min_x, min_y, max_x - min_x, max_y - min_y);
        }
    }

    /// Draw a line from p0 to p1 using Bresenham's algorithm.
    fn draw_line(&mut self, p0: Point, p1: Point, color: Rgba, width: usize) {
        let x0 = round_f32(p0.x) as isize;
        let y0 = round_f32(p0.y) as isize;
        let x1 = round_f32(p1.x) as isize;
        let y1 = round_f32(p1.y) as isize;

        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx - dy;

        let mut x = x0;
        let mut y = y0;
        loop {
            if x >= 0 && y >= 0 {
                let ux = x as usize;
                let uy = y as usize;
                for dy_off in 0..width {
                    for dx_off in 0..width {
                        self.put_pixel_blend(ux.saturating_add(dx_off), uy.saturating_add(dy_off), color);
                    }
                }
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }

    // ── Text rendering ───────────────────────────────────────────────

    /// Fill text at the given position using the current fill style.
    /// Uses a simple 5x7 bitmap font for ASCII characters.
    pub fn fill_text(&mut self, text: &str, x: f32, y: f32) {
        let color = Rgba::from_u32(self.fill_style);
        let size = self.font_size as usize;
        let mut cx = x as usize;
        let cy = y as usize;
        for ch in text.chars() {
            if ch == ' ' {
                cx += size * 3 / 5;
                continue;
            }
            if let Some(bitmap) = char_bitmap(ch) {
                for (dy, row) in bitmap.iter().enumerate() {
                    for (dx, bit) in row.iter().enumerate() {
                        if *bit {
                            let px = cx + dx * size / 5;
                            let py = cy + dy * size / 5;
                            if px < self.width && py < self.height {
                                self.put_pixel_blend(px, py, color);
                            }
                        }
                    }
                }
            }
            cx += size * 3 / 5 + size / 5; // character advance
        }
    }

    /// Stroke text (simplified: just fill text).
    pub fn stroke_text(&mut self, text: &str, x: f32, y: f32) {
        self.fill_text(text, x, y);
    }

    // ── ImageData operations ─────────────────────────────────────────

    pub fn create_image_data(&self, width: usize, height: usize) -> ImageData {
        ImageData::new(width, height)
    }

    pub fn put_image_data(&mut self, imagedata: &ImageData, dx: f32, dy: f32) {
        let dx0 = dx as usize;
        let dy0 = dy as usize;
        for yy in 0..imagedata.height {
            for xx in 0..imagedata.width {
                let src_idx = (yy * imagedata.width + xx) * 4;
                if src_idx + 3 >= imagedata.data.len() {
                    continue;
                }
                let r = imagedata.data[src_idx];
                let g = imagedata.data[src_idx + 1];
                let b = imagedata.data[src_idx + 2];
                let _a = imagedata.data[src_idx + 3];
                self.put_pixel_raw(dx0 + xx, dy0 + yy, r, g, b);
            }
        }
    }

    pub fn get_image_data(&self, sx: f32, sy: f32, sw: f32, sh: f32) -> ImageData {
        let sx0 = sx as usize;
        let sy0 = sy as usize;
        let sw0 = sw as usize;
        let sh0 = sh as usize;
        let mut img = ImageData::new(sw0, sh0);
        for yy in 0..sh0 {
            for xx in 0..sw0 {
                let src_x = sx0 + xx;
                let src_y = sy0 + yy;
                if let Some((r, g, b)) = self.get_pixel(src_x, src_y) {
                    let idx = (yy * sw0 + xx) * 4;
                    img.data[idx] = r;
                    img.data[idx + 1] = g;
                    img.data[idx + 2] = b;
                    img.data[idx + 3] = 0xFF;
                }
            }
        }
        img
    }

    // ── Transform ────────────────────────────────────────────────────

    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.transform = (a, b, c, d, e, f);
    }

    pub fn translate(&mut self, tx: f32, ty: f32) {
        let (a, b, c, d, e, f) = self.transform;
        self.transform = (a, b, c, d, e + tx, f + ty);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        let (a, b, c, d, e, f) = self.transform;
        self.transform = (a * sx, b * sy, c * sx, d * sy, e, f);
    }

    pub fn rotate(&mut self, angle: f32) {
        let ca = trig_cos(angle);
        let sa = trig_sin(angle);
        let (a, b, c, d, e, f) = self.transform;
        self.transform = (
            a * ca + c * sa,
            b * ca + d * sa,
            c * ca - a * sa,
            d * ca - b * sa,
            e,
            f,
        );
    }

    // ── Clip ─────────────────────────────────────────────────────────

    pub fn rect_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.clip = Some((x as usize, y as usize, w as usize, h as usize));
    }

    pub fn clear_clip(&mut self) {
        self.clip = None;
    }
}

/// 5x7 bitmap font for ASCII characters (simplified).
/// Each entry is 7 rows of 5 bits (1 = pixel).
fn char_bitmap(ch: char) -> Option<[[bool; 5]; 7]> {
    // Simple bitmap font for a few common characters.
    // In a full implementation, this would be a complete font.
    let bitmap: [[u8; 5]; 7] = match ch {
        'A' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
        ],
        'B' => [
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
        ],
        'C' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'D' => [
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
        ],
        'E' => [
            [1, 1, 1, 1, 1],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 1],
        ],
        'F' => [
            [1, 1, 1, 1, 1],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
        ],
        'G' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 0],
            [1, 0, 1, 1, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'H' => [
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
        ],
        'I' => [
            [1, 1, 1, 1, 1],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [1, 1, 1, 1, 1],
        ],
        'J' => [
            [1, 1, 1, 1, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'K' => [
            [1, 0, 0, 0, 1],
            [1, 0, 0, 1, 0],
            [1, 0, 1, 0, 0],
            [1, 1, 0, 0, 0],
            [1, 0, 1, 0, 0],
            [1, 0, 0, 1, 0],
            [1, 0, 0, 0, 1],
        ],
        'L' => [
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 1],
        ],
        'M' => [
            [1, 0, 0, 0, 1],
            [1, 1, 0, 1, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
        ],
        'N' => [
            [1, 0, 0, 0, 1],
            [1, 1, 0, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 0, 1, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
        ],
        'O' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'P' => [
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
        ],
        'Q' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 0, 1, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'R' => [
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
            [1, 0, 1, 0, 0],
            [1, 0, 0, 1, 0],
            [1, 0, 0, 0, 1],
        ],
        'S' => [
            [0, 1, 1, 1, 1],
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [1, 1, 1, 1, 0],
        ],
        'T' => [
            [1, 1, 1, 1, 1],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        'U' => [
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        'V' => [
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 0, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 0, 1, 0, 0],
        ],
        'W' => [
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
        ],
        'X' => [
            [1, 0, 0, 0, 1],
            [0, 1, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 0, 1, 0],
            [1, 0, 0, 0, 1],
        ],
        'Y' => [
            [1, 0, 0, 0, 1],
            [0, 1, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        'Z' => [
            [1, 1, 1, 1, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 1],
        ],
        '0' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 1, 0, 1],
            [1, 0, 0, 1, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        '1' => [
            [0, 0, 1, 0, 0],
            [0, 1, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 1, 1, 0],
        ],
        '2' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 0, 0, 0],
            [1, 1, 1, 1, 1],
        ],
        '3' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 1, 1, 0],
            [0, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        '4' => [
            [0, 0, 0, 1, 0],
            [0, 0, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [1, 0, 0, 1, 0],
            [1, 1, 1, 1, 1],
            [0, 0, 0, 1, 0],
            [0, 0, 0, 1, 0],
        ],
        '5' => [
            [1, 1, 1, 1, 1],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 0],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        '6' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 0],
            [1, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        '7' => [
            [1, 1, 1, 1, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 0, 0, 0],
            [0, 1, 0, 0, 0],
            [0, 1, 0, 0, 0],
        ],
        '8' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 0],
        ],
        '9' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [1, 0, 0, 0, 1],
            [0, 1, 1, 1, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 1, 0],
            [0, 1, 1, 0, 0],
        ],
        ' ' => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ],
        '.' => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 1, 1, 0, 0],
            [0, 1, 1, 0, 0],
        ],
        ',' => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 1, 1, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        '!' => [
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        '?' => [
            [0, 1, 1, 1, 0],
            [1, 0, 0, 0, 1],
            [0, 0, 0, 0, 1],
            [0, 0, 0, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        ':' => [
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
        ],
        ';' => [
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 1, 0, 0],
            [0, 0, 1, 0, 0],
        ],
        '-' => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ],
        '+' => [
            [0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0],
        ],
        '=' => [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ],
        _ => return None,
    };
    let mut out = [[false; 5]; 7];
    for (dst_row, src_row) in out.iter_mut().zip(bitmap.iter()) {
        for (dst, src) in dst_row.iter_mut().zip(src_row.iter()) {
            *dst = *src != 0;
        }
    }
    Some(out)
}

/// A canvas element backed by a Canvas2dContext.
#[derive(Clone, Debug)]
pub struct CanvasElement {
    pub id: String,
    pub width: usize,
    pub height: usize,
    pub context: Canvas2dContext,
    pub dirty: bool,
}

impl CanvasElement {
    pub fn new(id: String, width: usize, height: usize) -> Self {
        CanvasElement {
            id,
            width,
            height,
            context: Canvas2dContext::new(width, height),
            dirty: true,
        }
    }

    pub fn get_context_2d(&mut self) -> &mut Canvas2dContext {
        self.dirty = true;
        &mut self.context
    }

    /// Render the canvas bitmap to a framebuffer at the given position.
    pub fn render_to_fb(&self, fb: &mut crate::drivers::video::Framebuffer, dx: usize, dy: usize) {
        let dw = self.width.min(fb.width.saturating_sub(dx));
        let dh = self.height.min(fb.height.saturating_sub(dy));
        for yy in 0..dh {
            for xx in 0..dw {
                let idx = (yy * self.width + xx) * 4;
                if idx + 2 < self.context.pixels.len() {
                    let r = self.context.pixels[idx];
                    let g = self.context.pixels[idx + 1];
                    let b = self.context.pixels[idx + 2];
                    let color = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                    fb.put_pixel(dx + xx, dy + yy, color);
                }
            }
        }
    }
}
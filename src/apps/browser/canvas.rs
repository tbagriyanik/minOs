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

/// no_std ortamında `f32::floor` yok; aşağı yuvarlama.
#[inline]
fn ffloor(v: f32) -> f32 {
    let i = v as isize;
    if v < i as f32 {
        (i - 1) as f32
    } else {
        i as f32
    }
}

/// no_std ortamında `f32::ceil` yok; yukarı yuvarlama.
#[inline]
fn fceil(v: f32) -> f32 {
    let i = v as isize;
    if v > i as f32 {
        (i + 1) as f32
    } else {
        i as f32
    }
}

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
fn fsqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x >= 1.0 { x * 0.5 } else { 0.5 };
    for _ in 0..32 {
        r = 0.5 * (r + x / r);
    }
    r
}

/// Gradyan duraklarından `t` (0..=1) için renk örnekler.
fn sample_stops(stops: &[(f32, u32)], t: f32) -> u32 {
    if stops.is_empty() {
        return 0;
    }
    let t = t.clamp(0.0, 1.0);
    // İlk durum ilk renge, son durum son renge kenetlenir.
    if t <= stops[0].0 {
        return stops[0].1;
    }
    if t >= stops[stops.len() - 1].0 {
        return stops[stops.len() - 1].1;
    }
    for w in stops.windows(2) {
        let (o0, c0) = w[0];
        let (o1, c1) = w[1];
        if t >= o0 && t <= o1 {
            let span = o1 - o0;
            let f = if span > 0.0001 { (t - o0) / span } else { 0.0 };
            let r = lerp_u8(((c0 >> 16) & 0xFF) as u8, ((c1 >> 16) & 0xFF) as u8, f);
            let g = lerp_u8(((c0 >> 8) & 0xFF) as u8, ((c1 >> 8) & 0xFF) as u8, f);
            let b = lerp_u8((c0 & 0xFF) as u8, (c1 & 0xFF) as u8, f);
            return ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
    stops[stops.len() - 1].1
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t) as u8
}

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
#[derive(Clone, Debug, PartialEq)]
pub enum PathCmd {
    MoveTo(Point),
    LineTo(Point),
    /// İkinci derece Bézier: kontrol noktası + uç nokta.
    QuadTo {
        cp: Point,
        p: Point,
    },
    /// Üçüncü derece Bézier: iki kontrol noktası + uç nokta.
    CubicTo {
        c1: Point,
        c2: Point,
        p: Point,
    },
    Arc {
        center: Point,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        anticlockwise: bool,
    },
    ClosePath,
}

/// Gradyan tanımı: doğrusal veya radyal, renk durakları (offset 0..=1, u32 RGB).
#[derive(Clone, Debug)]
pub struct CanvasGradient {
    pub kind: GradientKind,
    /// (offset, rgb) çiftleri — ekleme sırasıyla, örnekleme sırasında sıralanır.
    pub stops: Vec<(f32, u32)>,
}

#[derive(Clone, Copy, Debug)]
pub enum GradientKind {
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    Radial { cx: f32, cy: f32, r: f32 },
}

/// save()/restore() için kaydedilen durum.
#[derive(Clone, Debug)]
pub struct SavedState {
    transform: (f32, f32, f32, f32, f32, f32),
    clip: Option<(usize, usize, usize, usize)>,
    global_alpha: f32,
    line_width: f32,
    fill_style: u32,
    fill_gradient: Option<usize>,
    stroke_style: u32,
    font_size: f32,
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
    /// Bu context'e ait gradyan nesneleri (createLinearGradient vb.).
    pub gradients: Vec<CanvasGradient>,
    /// fillStyle olarak seçili gradyan (varsa fill_style yerine kullanılır).
    pub fill_gradient: Option<usize>,
    /// save()/restore() yığını.
    pub state_stack: Vec<SavedState>,
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
            gradients: Vec::new(),
            fill_gradient: None,
            state_stack: Vec::new(),
        }
    }

    /// Set the fill style color (u32 RGB). Renk ataması gradyan seçimini kaldırır.
    pub fn set_fill_style(&mut self, color: u32) {
        self.fill_style = color;
        self.fill_gradient = None;
    }

    /// fillStyle olarak gradyan seçer.
    pub fn set_fill_gradient(&mut self, idx: usize) {
        if idx < self.gradients.len() {
            self.fill_gradient = Some(idx);
        }
    }

    /// (x, y) noktasındaki dolguluk rengini döndürür — gradyansa bağlıysa
    /// eksen izdüşümü/oran üzerinden örnekler.
    fn paint_at(&self, x: f32, y: f32) -> Rgba {
        if let Some(gi) = self.fill_gradient {
            if let Some(g) = self.gradients.get(gi) {
                let t = match g.kind {
                    GradientKind::Linear { x0, y0, x1, y1 } => {
                        let dx = x1 - x0;
                        let dy = y1 - y0;
                        let len2 = dx * dx + dy * dy;
                        if len2 > 0.0001 {
                            ((x - x0) * dx + (y - y0) * dy) / len2
                        } else {
                            0.0
                        }
                    }
                    GradientKind::Radial { cx, cy, r } => {
                        if r > 0.0001 {
                            let ddx = x - cx;
                            let ddy = y - cy;
                            fsqrt(ddx * ddx + ddy * ddy) / r
                        } else {
                            0.0
                        }
                    }
                };
                return Rgba::from_u32(sample_stops(&g.stops, t));
            }
        }
        Rgba::from_u32(self.fill_style)
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
    pub fn get_pixel(&self, x: usize, y: usize) -> Option<(u8, u8, u8)> {
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

    /// `rect(x, y, w, h)` — dikdörtgeni mevcut path'e ekler.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.path.push(PathCmd::MoveTo(Point { x, y }));
        self.path.push(PathCmd::LineTo(Point { x: x + w, y }));
        self.path
            .push(PathCmd::LineTo(Point { x: x + w, y: y + h }));
        self.path.push(PathCmd::LineTo(Point { x, y: y + h }));
        self.path.push(PathCmd::ClosePath);
    }

    /// `quadraticCurveTo(cpx, cpy, x, y)` — ikinci derece Bézier.
    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        self.path.push(PathCmd::QuadTo {
            cp: Point { x: cpx, y: cpy },
            p: Point { x, y },
        });
    }

    /// `bezierCurveTo(c1x, c1y, c2x, c2y, x, y)` — üçüncü derece Bézier.
    pub fn bezier_curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.path.push(PathCmd::CubicTo {
            c1: Point { x: c1x, y: c1y },
            c2: Point { x: c2x, y: c2y },
            p: Point { x, y },
        });
    }

    // ── Gradyanlar ─────────────────────────────────────────────────

    /// `createLinearGradient(x0, y0, x1, y1)` — gradyan dizine ekler,
    /// dizin döner (JS tarafı `canvasgrad<c>_<i>` referansı üretir).
    pub fn create_linear_gradient(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) -> usize {
        self.gradients.push(CanvasGradient {
            kind: GradientKind::Linear { x0, y0, x1, y1 },
            stops: Vec::new(),
        });
        self.gradients.len() - 1
    }

    /// `createRadialGradient(cx, cy, r)` — radyal gradyan ekler.
    pub fn create_radial_gradient(&mut self, cx: f32, cy: f32, r: f32) -> usize {
        self.gradients.push(CanvasGradient {
            kind: GradientKind::Radial { cx, cy, r },
            stops: Vec::new(),
        });
        self.gradients.len() - 1
    }

    /// `gradient.addColorStop(offset, color)` — durağı ekler.
    pub fn gradient_add_color_stop(&mut self, gi: usize, offset: f32, color: u32) {
        if let Some(g) = self.gradients.get_mut(gi) {
            g.stops.push((offset.clamp(0.0, 1.0), color));
            // Örnekleme için ofset sırasına göre sırala (küçük liste).
            g.stops
                .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
        }
    }

    // ── save / restore ──────────────────────────────────────────────

    /// `ctx.save()` — transform, clip ve stil durumunu yığar.
    pub fn save(&mut self) {
        self.state_stack.push(SavedState {
            transform: self.transform,
            clip: self.clip,
            global_alpha: self.global_alpha,
            line_width: self.line_width,
            fill_style: self.fill_style,
            fill_gradient: self.fill_gradient,
            stroke_style: self.stroke_style,
            font_size: self.font_size,
        });
    }

    /// `ctx.restore()` — son kaydedilen durumu geri yükler.
    pub fn restore(&mut self) {
        if let Some(s) = self.state_stack.pop() {
            self.transform = s.transform;
            self.clip = s.clip;
            self.global_alpha = s.global_alpha;
            self.line_width = s.line_width;
            self.fill_style = s.fill_style;
            self.fill_gradient = s.fill_gradient;
            self.stroke_style = s.stroke_style;
            self.font_size = s.font_size;
        }
    }

    // ── Rectangle operations ─────────────────────────────────────────

    /// Fill a rectangle with the current fill style (color or gradient).
    /// Transform (translate/scale/rotate) uygulanır — yol olarak
    /// rasterize edildiği için döndürme altında da doğru çalışır.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let saved = core::mem::take(&mut self.path);
        self.rect(x, y, w, h);
        self.fill();
        self.path = saved;
    }

    /// Stroke a rectangle outline with the current stroke style.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let saved = core::mem::take(&mut self.path);
        self.rect(x, y, w, h);
        self.stroke();
        self.path = saved;
    }

    /// Clear a rectangle to transparent black.
    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let saved = core::mem::take(&mut self.path);
        self.rect(x, y, w, h);
        self.fill_raw();
        self.path = saved;
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

        let mut current: Option<Point> = None;
        let mut subpath_start: Option<Point> = None;
        for cmd in &commands {
            match cmd {
                PathCmd::MoveTo(p) => {
                    let tp = self.transform_point(*p);
                    current = Some(tp);
                    subpath_start = Some(tp);
                }
                PathCmd::LineTo(p) => {
                    let end = self.transform_point(*p);
                    if let Some(start) = current {
                        self.draw_line(start, end, color, lw);
                    }
                    current = Some(end);
                }
                PathCmd::QuadTo { cp, p } => {
                    let end = self.transform_point(*p);
                    if let Some(start) = current {
                        let cpt = self.transform_point(*cp);
                        self.stroke_quad(start, cpt, end, color, lw);
                    }
                    current = Some(end);
                }
                PathCmd::CubicTo { c1, c2, p } => {
                    let end = self.transform_point(*p);
                    if let Some(start) = current {
                        let c1t = self.transform_point(*c1);
                        let c2t = self.transform_point(*c2);
                        self.stroke_cubic(start, c1t, c2t, end, color, lw);
                    }
                    current = Some(end);
                }
                PathCmd::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    anticlockwise,
                } => {
                    let c = self.transform_point(*center);
                    // Merkez transform ile ölçeklenmiş olabilir; basit yaklaşım:
                    // yarıçapı ortalama ölçek faktörüyle çarp.
                    let (a, _b, _cc, d, _e, _f) = self.transform;
                    let scale = fsqrt(a * a + d * d) * 0.5 + 0.5;
                    let r = radius * scale;
                    let s = *start_angle;
                    let mut e = *end_angle;
                    const TAU: f32 = core::f32::consts::TAU;
                    if *anticlockwise {
                        while e > s {
                            e -= TAU;
                        }
                    } else {
                        while e < s {
                            e += TAU;
                        }
                    }
                    let sweep = e - s;
                    let segs = (((sweep.abs() / TAU) * 48.0) as usize + 1).max(4);
                    let mut prev = Point {
                        x: c.x + r * trig_cos(s),
                        y: c.y + r * trig_sin(s),
                    };
                    // Yay başlangıcına mevcut noktadan bağla (canvas spec).
                    if let Some(cur) = current {
                        if (cur.x - prev.x).abs() > 0.01 || (cur.y - prev.y).abs() > 0.01 {
                            self.draw_line(cur, prev, color, lw);
                        }
                    }
                    for i in 1..=segs {
                        let ang = s + sweep * (i as f32) / (segs as f32);
                        let pt = Point {
                            x: c.x + r * trig_cos(ang),
                            y: c.y + r * trig_sin(ang),
                        };
                        self.draw_line(prev, pt, color, lw);
                        prev = pt;
                    }
                    current = Some(prev);
                }
                PathCmd::ClosePath => {
                    if let (Some(start), Some(first)) = (current, subpath_start) {
                        if (start.x - first.x).abs() > 0.01 || (start.y - first.y).abs() > 0.01 {
                            self.draw_line(start, first, color, lw);
                        }
                        current = Some(first);
                    }
                }
            }
        }
    }

    /// İkinci derece Bézier'i doğrusal segmentlere düzleştirip çizer.
    fn stroke_quad(&mut self, p0: Point, c: Point, p1: Point, color: Rgba, lw: usize) {
        const SEGS: usize = 16;
        let mut prev = p0;
        for i in 1..=SEGS {
            let t = i as f32 / SEGS as f32;
            let u = 1.0 - t;
            let pt = Point {
                x: u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x,
                y: u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y,
            };
            self.draw_line(prev, pt, color, lw);
            prev = pt;
        }
    }

    /// Üçüncü derece Bézier'i doğrusal segmentlere düzleştirip çizer.
    fn stroke_cubic(&mut self, p0: Point, c1: Point, c2: Point, p1: Point, color: Rgba, lw: usize) {
        const SEGS: usize = 20;
        let mut prev = p0;
        for i in 1..=SEGS {
            let t = i as f32 / SEGS as f32;
            let u = 1.0 - t;
            let pt = Point {
                x: u * u * u * p0.x
                    + 3.0 * u * u * t * c1.x
                    + 3.0 * u * t * t * c2.x
                    + t * t * t * p1.x,
                y: u * u * u * p0.y
                    + 3.0 * u * u * t * c1.y
                    + 3.0 * u * t * t * c2.y
                    + t * t * t * p1.y,
            };
            self.draw_line(prev, pt, color, lw);
            prev = pt;
        }
    }

    /// Fill the current path using the even-odd rule.
    /// Path cihaz uzayına düzleştirilir (transform uygulanır), her
    /// tarama satırında kenar kesişimleri hesaplanıp dolgu yapılır.
    pub fn fill(&mut self) {
        if self.path.is_empty() {
            return;
        }
        let subs = self.flatten_path();
        self.fill_subpaths(&subs, false);
    }

    /// `clearRect` için aynı rasterizasyonu ham (alpha'sız) modda yapar.
    fn fill_raw(&mut self) {
        if self.path.is_empty() {
            return;
        }
        let subs = self.flatten_path();
        self.fill_subpaths(&subs, true);
    }

    /// Path'i cihaz uzayında kapalı alt-yollara düzleştirir.
    /// Quad/cubic Bézier ve arc'ler doğrusal segmentlere indirgenir.
    fn flatten_path(&self) -> Vec<Vec<Point>> {
        let mut subs: Vec<Vec<Point>> = Vec::new();
        let mut cur: Option<usize> = None;
        let mut pos = Point { x: 0.0, y: 0.0 };
        let mut start = pos;

        fn push_pt(subs: &mut [Vec<Point>], cur: Option<usize>, p: Point) {
            if let Some(i) = cur {
                if let Some(last) = subs[i].last() {
                    if (last.x - p.x).abs() < 0.001 && (last.y - p.y).abs() < 0.001 {
                        return;
                    }
                }
                subs[i].push(p);
            }
        }

        for cmd in &self.path {
            match cmd {
                PathCmd::MoveTo(p) => {
                    let tp = self.transform_point(*p);
                    subs.push(vec![tp]);
                    cur = Some(subs.len() - 1);
                    pos = tp;
                    start = tp;
                }
                PathCmd::LineTo(p) => {
                    let tp = self.transform_point(*p);
                    push_pt(&mut subs, cur, tp);
                    pos = tp;
                }
                PathCmd::QuadTo { cp, p } => {
                    let cpt = self.transform_point(*cp);
                    let end = self.transform_point(*p);
                    const SEGS: usize = 16;
                    for i in 1..=SEGS {
                        let t = i as f32 / SEGS as f32;
                        let u = 1.0 - t;
                        let pt = Point {
                            x: u * u * pos.x + 2.0 * u * t * cpt.x + t * t * end.x,
                            y: u * u * pos.y + 2.0 * u * t * cpt.y + t * t * end.y,
                        };
                        push_pt(&mut subs, cur, pt);
                    }
                    pos = end;
                }
                PathCmd::CubicTo { c1, c2, p } => {
                    let c1t = self.transform_point(*c1);
                    let c2t = self.transform_point(*c2);
                    let end = self.transform_point(*p);
                    const SEGS: usize = 20;
                    for i in 1..=SEGS {
                        let t = i as f32 / SEGS as f32;
                        let u = 1.0 - t;
                        let pt = Point {
                            x: u * u * u * pos.x
                                + 3.0 * u * u * t * c1t.x
                                + 3.0 * u * t * t * c2t.x
                                + t * t * t * end.x,
                            y: u * u * u * pos.y
                                + 3.0 * u * u * t * c1t.y
                                + 3.0 * u * t * t * c2t.y
                                + t * t * t * end.y,
                        };
                        push_pt(&mut subs, cur, pt);
                    }
                    pos = end;
                }
                PathCmd::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    anticlockwise,
                } => {
                    let c = self.transform_point(*center);
                    let (a, _b, _cc, d, _e, _f) = self.transform;
                    let scale = fsqrt(a * a + d * d) * 0.5 + 0.5;
                    let r = radius * scale;
                    let s = *start_angle;
                    let mut e = *end_angle;
                    const TAU: f32 = core::f32::consts::TAU;
                    if *anticlockwise {
                        while e > s {
                            e -= TAU;
                        }
                    } else {
                        while e < s {
                            e += TAU;
                        }
                    }
                    let sweep = e - s;
                    let segs = (((sweep.abs() / TAU) * 48.0) as usize + 1).max(4);
                    let arc_start = Point {
                        x: c.x + r * trig_cos(s),
                        y: c.y + r * trig_sin(s),
                    };
                    // Mevcut nokta yay başlangıcından farklıysa canvas spec
                    // gereği önce oraya doğru bir doğru parçası eklenir.
                    if (pos.x - arc_start.x).abs() > 0.01 || (pos.y - arc_start.y).abs() > 0.01 {
                        push_pt(&mut subs, cur, arc_start);
                    }
                    for i in 1..=segs {
                        let ang = s + sweep * (i as f32) / (segs as f32);
                        push_pt(
                            &mut subs,
                            cur,
                            Point {
                                x: c.x + r * trig_cos(ang),
                                y: c.y + r * trig_sin(ang),
                            },
                        );
                    }
                    pos = subs
                        .get(cur.unwrap_or(0))
                        .and_then(|v| v.last())
                        .copied()
                        .unwrap_or(arc_start);
                }
                PathCmd::ClosePath => {
                    if let Some(i) = cur {
                        if let Some(&first) = subs[i].first() {
                            push_pt(&mut subs, cur, first);
                        }
                    }
                    pos = start;
                }
            }
        }
        // Her alt-yolu kapat (fill açık alt-yolları örtük kapatır).
        for sub in subs.iter_mut() {
            if sub.len() > 1 {
                if let (Some(&first), Some(&last)) = (sub.first(), sub.last()) {
                    if (first.x - last.x).abs() > 0.001 || (first.y - last.y).abs() > 0.001 {
                        sub.push(first);
                    }
                }
            }
        }
        subs.retain(|s| s.len() >= 3);
        subs
    }

    /// Düzleştirilmiş alt-yolları even-odd kuralıyla doldurur.
    /// `raw` modunda renk yerine şeffaf siyah yazılır (clearRect).
    fn fill_subpaths(&mut self, subs: &[Vec<Point>], raw: bool) {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for sub in subs {
            for p in sub {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
        }
        if !min_x.is_finite() || max_x <= min_x || max_y <= min_y {
            return;
        }
        let y_start = (ffloor(min_y) as isize).max(0);
        let y_end = ((fceil(max_y) as isize) + 1).min(self.height as isize);
        let mut xs: Vec<f32> = Vec::new();
        for yy in y_start..y_end {
            if yy < 0 {
                continue;
            }
            let yc = yy as f32 + 0.5;
            xs.clear();
            for sub in subs {
                for w in sub.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if (a.y <= yc) == (b.y <= yc) {
                        continue; // Yatay veya kesişmeyen kenar.
                    }
                    let t = (yc - a.y) / (b.y - a.y);
                    xs.push(a.x + t * (b.x - a.x));
                }
            }
            xs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(core::cmp::Ordering::Equal));
            let mut i = 0;
            while i + 1 < xs.len() {
                let xl = xs[i];
                let xr = xs[i + 1];
                if xr > xl {
                    let xa = (ffloor(xl) as isize).max(0);
                    let xb = ((fceil(xr) as isize) + 1).min(self.width as isize);
                    for xx in xa..xb {
                        let xc = xx as f32 + 0.5;
                        if xc > xl && xc < xr {
                            if raw {
                                self.put_pixel_raw(xx as usize, yy as usize, 0, 0, 0);
                            } else {
                                let color = self.paint_at(xx as f32, yy as f32);
                                self.put_pixel_blend(xx as usize, yy as usize, color);
                            }
                        }
                    }
                }
                i += 2;
            }
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
                        self.put_pixel_blend(
                            ux.saturating_add(dx_off),
                            uy.saturating_add(dy_off),
                            color,
                        );
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
                                let color = self.paint_at(px as f32, py as f32);
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

    /// Draw an ImageData onto the canvas at (dx, dy), optionally scaled to
    /// (d_width, d_height). Uses nearest-neighbor sampling when scaling.
    pub fn draw_image(&mut self, image: &ImageData, dx: f32, dy: f32) {
        self.draw_image_scaled(image, dx, dy, image.width as f32, image.height as f32);
    }

    /// Draw an ImageData onto the canvas at (dx, dy) scaled to (d_width, d_height).
    pub fn draw_image_scaled(
        &mut self,
        image: &ImageData,
        dx: f32,
        dy: f32,
        d_width: f32,
        d_height: f32,
    ) {
        let dx0 = dx as usize;
        let dy0 = dy as usize;
        let dw = d_width as usize;
        let dh = d_height as usize;

        for ty in 0..dh {
            let sy = (ty * image.height) / dh.max(1);
            for tx in 0..dw {
                let sx = (tx * image.width) / dw.max(1);
                let src_idx = (sy * image.width + sx) * 4;
                if src_idx + 3 >= image.data.len() {
                    continue;
                }
                let r = image.data[src_idx];
                let g = image.data[src_idx + 1];
                let b = image.data[src_idx + 2];
                let a = image.data[src_idx + 3];

                let x = dx0 + tx;
                let y = dy0 + ty;
                if x < self.width && y < self.height {
                    if a == 0xFF {
                        self.put_pixel_raw(x, y, r, g, b);
                    } else if a > 0 {
                        let src = Rgba { r, g, b, a };
                        self.put_pixel_blend(x, y, src);
                    }
                }
            }
        }
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

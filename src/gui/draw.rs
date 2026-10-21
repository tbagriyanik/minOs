//! Text and colour drawing helpers.

use super::*;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static POINTER_X: AtomicUsize = AtomicUsize::new(0);
static POINTER_Y: AtomicUsize = AtomicUsize::new(0);
static POINTER_DOWN: AtomicBool = AtomicBool::new(false);

impl MinGuiCompositor {
    pub fn set_pointer_state(x: usize, y: usize, down: bool) {
        POINTER_X.store(x, Ordering::Relaxed);
        POINTER_Y.store(y, Ordering::Relaxed);
        POINTER_DOWN.store(down, Ordering::Relaxed);
    }

    /// Siyah veya beyaz metinden, verilen zemin üzerinde daha okunaklı olanı seçer.
    pub fn text_on(background: u32) -> u32 {
        let c = theme::resolve(background);
        let r = ((c >> 16) & 0xFF) as u32;
        let g = ((c >> 8) & 0xFF) as u32;
        let b = (c & 0xFF) as u32;
        if r * 299 + g * 587 + b * 114 >= 140_000 { Color::BLACK } else { Color::WHITE }
    }
// ---------------------------------------------------------------------------
// Çizim yardımcıları
// ---------------------------------------------------------------------------
    /// Aktif temadan okunan RGB ile bir rengi aydınlatır/koyulaştırır.
    pub fn shade(color: u32, delta: i32) -> u32 {
        let c = theme::resolve(color);
        let r = (((c >> 16) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        let g = (((c >> 8) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        let b = ((c & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        (r << 16) | (g << 8) | b
    }

    /// Aktif zoom ile ölçeklenmiş font boyutu (önbelleğe alınmış).
    #[inline]
    pub fn scaled_font_size() -> f32 {
        use core::sync::atomic::{AtomicU32, Ordering};
        static CACHED_ZOOM: AtomicU32 = AtomicU32::new(0);
        static CACHED_SIZE: AtomicU32 = AtomicU32::new(0);
        let zoom = font_zoom() as u32;
        if CACHED_ZOOM.load(Ordering::Relaxed) == zoom && zoom != 0 {
            return f32::from_bits(CACHED_SIZE.load(Ordering::Relaxed));
        }
        let size = BASE_FONT_SIZE * zoom as f32 / 100.0;
        CACHED_ZOOM.store(zoom, Ordering::Relaxed);
        CACHED_SIZE.store(size.to_bits(), Ordering::Relaxed);
        size
    }

    /// Inria Sans ile genel arayüz metni.
    pub fn draw_string(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
        let size = Self::scaled_font_size();
        font_render::draw(fb, font_render::Face::Inria, x, y, text, color, size);
    }

    /// Monospace metni CMD ekranında Geist Mono ile çizer.
    pub fn draw_terminal_string(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
        let size = Self::scaled_font_size();
        font_render::draw(fb, font_render::Face::Geist, x, y, text, color, size);
    }

    /// Monospace metni belirli bir kırpma alanı içinde çizer.
    pub fn draw_terminal_string_clipped(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        rect: (usize, usize, usize, usize),
    ) {
        let old_clip = fb.clip();
        let clip_x = old_clip.x0.max(rect.0);
        let clip_y = old_clip.y0.max(rect.1);
        let clip_right = old_clip.x1.min(rect.0.saturating_add(rect.2));
        let clip_bottom = old_clip.y1.min(rect.1.saturating_add(rect.3));
        fb.set_clip(clip_x, clip_y, clip_right.saturating_sub(clip_x), clip_bottom.saturating_sub(clip_y));
        Self::draw_terminal_string(fb, x, y, text, color);
        fb.set_clip(old_clip.x0, old_clip.y0, old_clip.x1.saturating_sub(old_clip.x0), old_clip.y1.saturating_sub(old_clip.y0));
    }

    pub fn terminal_text_width(text: &str) -> usize {
        let size = Self::scaled_font_size();
        font_render::text_width(font_render::Face::Geist, text, size)
    }

    /// Aktif ölçekte bir metin satırının görsel yüksekliği (px).
    /// `blit_glyph` hedef yüksekliğiyle ve dikey ortalama hesaplarıyla tutarlıdır.
    #[inline(always)]
    pub fn text_height() -> usize {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static CACHED: AtomicUsize = AtomicUsize::new(0);
        static LAST_ZOOM: AtomicUsize = AtomicUsize::new(0);
        let zoom = font_zoom() as usize;
        if zoom != 0 && LAST_ZOOM.load(Ordering::Relaxed) == zoom {
            return CACHED.load(Ordering::Relaxed);
        }
        let h = (font_render::text_height(font_render::Face::Inria, BASE_FONT_SIZE) * zoom + 50) / 100;
        if zoom != 0 {
            CACHED.store(h, Ordering::Relaxed);
            LAST_ZOOM.store(zoom, Ordering::Relaxed);
        }
        h
    }

    /// Ölçeklenebilir Inria Sans metni.
    pub fn draw_string_scaled(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32, scale: usize) {
        let size = Self::scaled_font_size() * scale.max(1) as f32;
        font_render::draw(fb, font_render::Face::Inria, x, y, text, color, size);
    }

    /// Gölgeli/vurgulu metin — tek geçişli temiz çizim yaparak font bulanıklığını ve gölge çakışmasını önler.
    pub fn draw_string_shadow(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        _shadow: u32,
        scale: usize,
    ) {
        Self::draw_string_scaled(fb, x, y, text, color, scale);
    }

    /// Inria Sans kullanarak metin genişliğini hesaplar.
    pub fn text_width(text: &str, scale: usize) -> usize {
        let zoomed_size = Self::scaled_font_size() * scale.max(1) as f32;
        font_render::text_width(font_render::Face::Inria, text, zoomed_size)
    }

    /// Etiketli, hover durumuna duyarlı düğme (Yuvarlatılmış köşeli).
    pub fn draw_button(
        fb: &mut Framebuffer,
        r: (usize, usize, usize, usize),
        bg: u32,
        label: &str,
        _label_color: u32,
        hovered: bool,
    ) {
        let px = POINTER_X.load(Ordering::Relaxed);
        let py = POINTER_Y.load(Ordering::Relaxed);
        let pointer_over = px >= r.0 && px < r.0.saturating_add(r.2)
            && py >= r.1 && py < r.1.saturating_add(r.3);
        let hovered = hovered || pointer_over;
        let pressed = POINTER_DOWN.load(Ordering::Relaxed) && pointer_over;
        let bg = if pressed {
            Self::shade(bg, -48)
        } else if hovered {
            Self::shade(bg, 40)
        } else {
            bg
        };
        let rect = if pressed {
            (r.0 + 2, r.1 + 2, r.2.saturating_sub(4), r.3.saturating_sub(4))
        } else {
            r
        };
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 5, bg);
        fb.draw_rounded_rect_outline(rect.0, rect.1, rect.2, rect.3, 5,
            if pressed { Color::WHITE } else { Color::BORDER });
        let tw = Self::text_width(label, 1).saturating_sub(1);
        let tx = rect.0 + rect.2.saturating_sub(tw) / 2;
        let text_h = Self::text_height();
        let ty = rect.1 + rect.3.saturating_sub(text_h) / 2;
        let readable_label_color = Self::text_on(bg);
        Self::draw_string_clipped(fb, tx, ty, label, readable_label_color,
            (rect.0 + 2, rect.1 + 1, rect.2.saturating_sub(4), rect.3.saturating_sub(2)));
    }

    /// Metni belirtilen dikdörtgenin içinde kırparak çizer.
    pub fn draw_string_clipped(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        rect: (usize, usize, usize, usize),
    ) {
        let old_clip = fb.clip();
        let clip_x = old_clip.x0.max(rect.0);
        let clip_y = old_clip.y0.max(rect.1);
        let clip_right = old_clip.x1.min(rect.0.saturating_add(rect.2));
        let clip_bottom = old_clip.y1.min(rect.1.saturating_add(rect.3));
        fb.set_clip(clip_x, clip_y, clip_right.saturating_sub(clip_x), clip_bottom.saturating_sub(clip_y));
        Self::draw_string(fb, x, y, text, color);
        fb.set_clip(old_clip.x0, old_clip.y0, old_clip.x1.saturating_sub(old_clip.x0), old_clip.y1.saturating_sub(old_clip.y0));
    }

    /// Metni ölçekli ve belirtilen dikdörtgenin içinde kırparak çizer.
    pub fn draw_string_scaled_clipped(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        scale: usize,
        rect: (usize, usize, usize, usize),
    ) {
        let old_clip = fb.clip();
        let clip_x = old_clip.x0.max(rect.0);
        let clip_y = old_clip.y0.max(rect.1);
        let clip_right = old_clip.x1.min(rect.0.saturating_add(rect.2));
        let clip_bottom = old_clip.y1.min(rect.1.saturating_add(rect.3));
        fb.set_clip(clip_x, clip_y, clip_right.saturating_sub(clip_x), clip_bottom.saturating_sub(clip_y));
        Self::draw_string_scaled(fb, x, y, text, color, scale);
        fb.set_clip(old_clip.x0, old_clip.y0, old_clip.x1.saturating_sub(old_clip.x0), old_clip.y1.saturating_sub(old_clip.y0));
    }
}

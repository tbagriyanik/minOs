//! Yeniden kullanılabilir kaydırma çubuğu (scrollbar) ve kaydırma durumu.
//!
//! Her uygulama bir `ScrollState` tutar; pencere yeniden boyutlandığında
//! içerik alanı küçülürse taşma yerine kaydırma çubuğu devreye girer ve
//! içerik asla pencere dışına taşmaz (bkz. `Framebuffer::set_clip`).

use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

/// Kaydırma çubuğu genişliği/yüksekliği (px).
pub const SCROLLBAR_THICKNESS: usize = 10;

/// Yatay ve dikey kaydırma durumu. Ofsetler piksel cinsindendir.
#[derive(Copy, Clone, Debug, Default)]
pub struct ScrollState {
    pub x: usize,
    pub y: usize,
}

impl ScrollState {
    pub const fn new() -> Self {
        Self { x: 0, y: 0 }
    }

    /// Ofsetleri geçerli aralığa kısıtlar.
    pub fn clamp(&mut self, max_x: usize, max_y: usize) {
        self.x = self.x.min(max_x);
        self.y = self.y.min(max_y);
    }
}

/// Bir kaydırma çubuğu ekseni için geometri (iz + tutamaç).
#[derive(Copy, Clone, Debug)]
pub struct ScrollbarGeom {
    /// Çubuğun (track) dikdörtgeni: (x, y, w, h).
    pub track: (usize, usize, usize, usize),
    /// Tutamacın (thumb) dikdörtgeni.
    pub thumb: (usize, usize, usize, usize),
    /// Tutamaç track boyunca 0..=1 arası normalize konumu (kaydırma oranı).
    pub ratio: f32,
}

/// Dikey kaydırma çubuğu geometrisini hesaplar.
/// `view` = görünür pencere (uzunluk), `content` = toplam içerik uzunluğu,
/// `offset` = mevcut kaydırma ofseti.
pub fn v_geom(
    track: (usize, usize, usize, usize),
    content: usize,
    view: usize,
    offset: usize,
) -> ScrollbarGeom {
    let (tx, ty, tw, th) = track;
    if content <= view || view == 0 {
        return ScrollbarGeom {
            track,
            thumb: (tx, ty, tw, 0),
            ratio: 0.0,
        };
    }
    let thumb_h = ((th * view) / content).max(16).min(th);
    let max_off = content - view;
    let pos = ((th - thumb_h) * offset) / max_off.max(1);
    let ratio = offset as f32 / max_off as f32;
    ScrollbarGeom {
        track,
        thumb: (tx, ty + pos, tw, thumb_h),
        ratio,
    }
}

/// Yatay kaydırma çubuğu geometrisini hesaplar.
pub fn h_geom(
    track: (usize, usize, usize, usize),
    content: usize,
    view: usize,
    offset: usize,
) -> ScrollbarGeom {
    let (tx, ty, tw, th) = track;
    if content <= view || view == 0 {
        return ScrollbarGeom {
            track,
            thumb: (tx, ty, 0, th),
            ratio: 0.0,
        };
    }
    let thumb_w = ((tw * view) / content).max(16).min(tw);
    let max_off = content - view;
    let pos = ((tw - thumb_w) * offset) / max_off.max(1);
    let ratio = offset as f32 / max_off as f32;
    ScrollbarGeom {
        track,
        thumb: (tx + pos, ty, thumb_w, th),
        ratio,
    }
}

/// Dikey kaydırma çubuğunun track dikdörtgenini, verilen içerik alanına göre üretir.
/// İçerik alanının sağ kenarına yerleşir.
pub fn v_track_for(
    content_rect: (usize, usize, usize, usize),
    needed: bool,
    h_scroll_present: bool,
) -> (usize, usize, usize, usize) {
    let (cx, cy, cw, ch) = content_rect;
    if !needed {
        return (cx + cw, cy, 0, 0);
    }
    let h = ch.saturating_sub(if h_scroll_present {
        SCROLLBAR_THICKNESS
    } else {
        0
    });
    (
        cx + cw.saturating_sub(SCROLLBAR_THICKNESS),
        cy,
        SCROLLBAR_THICKNESS,
        h,
    )
}

/// Yatay kaydırma çubuğunun track dikdörtgenini üretir. İçerik alanının alt kenarına yerleşir.
pub fn h_track_for(
    content_rect: (usize, usize, usize, usize),
    needed: bool,
    v_scroll_present: bool,
) -> (usize, usize, usize, usize) {
    let (cx, cy, cw, ch) = content_rect;
    if !needed {
        return (cx, cy + ch, 0, 0);
    }
    let w = cw.saturating_sub(if v_scroll_present {
        SCROLLBAR_THICKNESS
    } else {
        0
    });
    (
        cx,
        cy + ch.saturating_sub(SCROLLBAR_THICKNESS),
        w,
        SCROLLBAR_THICKNESS,
    )
}

/// Kaydırma çubuğunu çizer (track + thumb + kenarlık).
pub fn draw_vertical(fb: &mut Framebuffer, g: &ScrollbarGeom) {
    let (tx, ty, tw, th) = g.track;
    if tw == 0 || th == 0 {
        return;
    }
    fb.draw_rect(tx, ty, tw, th, Color::TITLE_BAR_INACTIVE);
    fb.draw_rect_outline(tx, ty, tw, th, Color::BORDER);
    if g.thumb.3 > 0 {
        let (hx, hy, hw, hh) = g.thumb;
        fb.draw_rect(
            hx + 1,
            hy + 1,
            hw.saturating_sub(2),
            hh.saturating_sub(2),
            Color::ACCENT_BLUE,
        );
    }
}

pub fn draw_horizontal(fb: &mut Framebuffer, g: &ScrollbarGeom) {
    let (tx, ty, tw, th) = g.track;
    if tw == 0 || th == 0 {
        return;
    }
    fb.draw_rect(tx, ty, tw, th, Color::TITLE_BAR_INACTIVE);
    fb.draw_rect_outline(tx, ty, tw, th, Color::BORDER);
    if g.thumb.2 > 0 {
        let (hx, hy, hw, hh) = g.thumb;
        fb.draw_rect(
            hx + 1,
            hy + 1,
            hw.saturating_sub(2),
            hh.saturating_sub(2),
            Color::ACCENT_BLUE,
        );
    }
}

/// Görsel kolaylık: içerik alanını kısıtlayıp (clip) kırpılmış çerçeve çizer.
/// `content_rect` = (x, y, w, h) pencerenin içerik bölgesi (kaydırma alanı hariç).
pub fn clip_to(fb: &mut Framebuffer, content_rect: (usize, usize, usize, usize)) {
    let (x, y, w, h) = content_rect;
    fb.set_clip(x, y, w, h);
}

/// Metin satırını verilen genişlikte kaydırmalı biçimde çizer (yatay kaydırma).
/// Döndürülen değer, çizilen karakter sayısıdır.
pub fn draw_string_clipped(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
    h_offset: usize,
) {
    // Yatay ofset uygulanır; karakterler clip bölgesine otomatik kırpılır.
    let start_x = x.saturating_sub(h_offset);
    MinGuiCompositor::draw_string(fb, start_x, y, text, color);
}

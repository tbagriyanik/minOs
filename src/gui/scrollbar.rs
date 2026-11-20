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

/// Yumuşak kaydırma animasyonu süresi (ms); tarayıcının yerleşik
/// kaydırmasıyla aynı his için kullanılır.
pub const SCROLL_ANIM_MS: u64 = 150;

/// Fare tekerleği gibi "hedefe git" tarzı kaydırmalar için kısa süreli
/// ease-out interpolasyonu durumu.
///
/// Sürükleme ve doğrudan atama gibi anlık etkileşimlerde animasyon
/// `cancel` ile iptal edilmeli; aksi halde eski hedefe doğru "kayma"
/// kullanıcı girdisiyle çakışır.
#[derive(Copy, Clone, Debug)]
pub struct ScrollAnim {
    from: ScrollState,
    to: ScrollState,
    start_ms: u64,
    active: bool,
}

impl ScrollAnim {
    /// Animasyonsuz (boş) durum.
    pub const fn idle() -> Self {
        Self {
            from: ScrollState::new(),
            to: ScrollState::new(),
            start_ms: 0,
            active: false,
        }
    }

    /// Animasyon sürüyor mu?
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Süren animasyonu iptal eder; mevcut ofset korunur.
    #[inline(always)]
    pub fn cancel(&mut self) {
        self.active = false;
    }

    /// `current` ofsetinden `target`'a (sınırlara kısıtlanmış) animasyon
    /// başlatır. Hedef ile mevcut ofset aynıysa animasyon başlatılmaz.
    pub fn start(
        &mut self,
        current: ScrollState,
        mut target: ScrollState,
        max_x: usize,
        max_y: usize,
        now_ms: u64,
    ) {
        target.clamp(max_x, max_y);
        if target.x == current.x && target.y == current.y {
            self.active = false;
            return;
        }
        self.from = current;
        self.to = target;
        self.start_ms = now_ms;
        self.active = true;
    }

    /// Kare adımı: animasyon aktifse bu karenin ofsetini döndürür.
    ///
    /// Dönen ofset `max_x`/`max_y`'ye kısıtlanır; animasyon bitince
    /// kesin hedef döner ve durum kapatılır. Aktif değilse `None`.
    pub fn step(&mut self, now_ms: u64, max_x: usize, max_y: usize) -> Option<ScrollState> {
        if !self.active {
            return None;
        }
        let to = ScrollState {
            x: self.to.x.min(max_x),
            y: self.to.y.min(max_y),
        };
        let elapsed = now_ms.wrapping_sub(self.start_ms);
        if elapsed >= SCROLL_ANIM_MS {
            self.active = false;
            return Some(to);
        }
        let progress = crate::gui::anim::progress(elapsed, SCROLL_ANIM_MS);
        // ease-out quad: hızlı başlar, yumuşak durur (tarayıcı ile aynı).
        let eased = crate::gui::anim::ease_out_quad(progress);
        let fx = self.from.x as f32;
        let fy = self.from.y as f32;
        Some(ScrollState {
            x: (fx + (to.x as f32 - fx) * eased) as usize,
            y: (fy + (to.y as f32 - fy) * eased) as usize,
        })
    }
}

/// Bir pencere için hesaplanmış kaydırma çubuğu düzeni.
///
/// Çizim ve isabet testi aynı düzeni kullandığı için tutamaç her zaman
/// görünen konumla birebir eşleşir.
#[derive(Copy, Clone, Debug, Default)]
pub struct ScrollbarLayout {
    /// Dikey çubuk (taşma yoksa `None`).
    pub vertical: Option<ScrollbarGeom>,
    /// Yatay çubuk (taşma yoksa `None`).
    pub horizontal: Option<ScrollbarGeom>,
    /// Kaydırma çubukları düşüldükten sonra kalan görünür genişlik.
    pub view_w: usize,
    /// Kaydırma çubukları düşüldükten sonra kalan görünür yükseklik.
    pub view_h: usize,
}

impl ScrollbarLayout {
    /// Dikey çubuğun iz dikdörtgeni (yoksa sıfır boyutlu).
    pub fn v_track(&self) -> (usize, usize, usize, usize) {
        self.vertical.map(|g| g.track).unwrap_or((0, 0, 0, 0))
    }

    /// Yatay çubuğun iz dikdörtgeni (yoksa sıfır boyutlu).
    pub fn h_track(&self) -> (usize, usize, usize, usize) {
        self.horizontal.map(|g| g.track).unwrap_or((0, 0, 0, 0))
    }
}

/// İçerik alanı ve içerik boyutuna göre iki eksenin kaydırma düzenini üretir.
///
/// `content_rect` pencerenin içerik alanıdır (başlık çubuğu hariç).
/// `content_w`/`content_h` kaydırılabilir toplam içerik boyutudur.
/// `scroll` mevcut ofsetlerdir; tutamaç konumları buradan hesaplanır.
pub fn layout(
    content_rect: (usize, usize, usize, usize),
    content_w: usize,
    content_h: usize,
    scroll: ScrollState,
) -> ScrollbarLayout {
    let (cx, cy, cw, ch) = content_rect;
    let want_v = content_h > ch && ch > 0;
    let want_h = content_w > cw && cw > 0;

    let view_h = ch.saturating_sub(if want_h { SCROLLBAR_THICKNESS } else { 0 });
    let view_w = cw.saturating_sub(if want_v { SCROLLBAR_THICKNESS } else { 0 });

    let vertical = if want_v && view_h > 0 {
        let track = (
            cx + cw.saturating_sub(SCROLLBAR_THICKNESS),
            cy,
            SCROLLBAR_THICKNESS,
            view_h,
        );
        Some(v_geom(track, content_h, view_h, scroll.y))
    } else {
        None
    };

    let horizontal = if want_h && view_w > 0 {
        let track = (
            cx,
            cy + ch.saturating_sub(SCROLLBAR_THICKNESS),
            view_w,
            SCROLLBAR_THICKNESS,
        );
        Some(h_geom(track, content_w, view_w, scroll.x))
    } else {
        None
    };

    ScrollbarLayout {
        vertical,
        horizontal,
        view_w,
        view_h,
    }
}

/// Bir eksende kaydırma ofsetinin üst sınırını döndürür.
#[inline(always)]
pub fn max_offset(content: usize, view: usize) -> usize {
    content.saturating_sub(view)
}

/// Tıklanan noktayı iz üzerindeki orana (0..=1) çevirir.
#[inline(always)]
pub fn ratio_at(pos: usize, track_start: usize, track_len: usize) -> f32 {
    if track_len == 0 {
        return 0.0;
    }
    let local = pos.saturating_sub(track_start).min(track_len) as f32;
    (local / track_len as f32).clamp(0.0, 1.0)
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

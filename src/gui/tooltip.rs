//! Hover ipuçları (tooltip) — kısa gecikmeyle görünen açıklama balonları.
//!
//! Render katmanı `tooltip_offer` ile o karede hover edilen öğeyi bildirir;
//! olay döngüsü her tur `tooltip_tick` çağırarak gecikmesi dolan adayı
//! görünür tooltip'e yükseltir. Durum geçişlerinde ilgili ekran bölgesi
//! kirli işaretlenir; böylece ipucu kendi kendine belirir ve kaybolur.

use crate::drivers::video::Framebuffer;
use crate::gui::dirty_rect::DirtyRect;
use crate::gui::{Color, MinGuiCompositor};

/// Tooltip'in görünmesi için gereken hover süresi (ms).
pub const TOOLTIP_DELAY_MS: u64 = 700;

/// Tek bir hover adayı: bağlantı noktası (anchor) dikdörtgeni + metin.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TooltipCandidate {
    pub anchor: (usize, usize, usize, usize),
    pub text: &'static str,
}

/// Olay döngüsünün onayladığı hover adayı; `since_ms` gecikme saatidir.
#[derive(Clone, Copy)]
pub struct ConfirmedHover {
    pub anchor: (usize, usize, usize, usize),
    pub text: &'static str,
    pub since_ms: u64,
}

/// Ekranda çizilen tooltip kartı (ekran koordinatları).
#[derive(Clone, Copy)]
pub struct Tooltip {
    pub text: &'static str,
    pub anchor: (usize, usize, usize, usize),
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// Tek `Cell` içinde tutulan tooltip durumu (tek iş parçacıklı GUI).
#[derive(Clone, Copy, Default)]
pub struct TooltipState {
    /// Son render karesinde bildirilen hover (her kare başında sıfırlanır).
    pub hover: Option<TooltipCandidate>,
    /// Onaylanmış hover; aynı öğede kalındıkça `since_ms` korunur.
    pub candidate: Option<ConfirmedHover>,
    /// Görünen tooltip.
    pub visible: Option<Tooltip>,
}

impl TooltipState {
    /// Boş durum (compositor `const fn new` içinde kullanır).
    pub const fn empty() -> Self {
        Self {
            hover: None,
            candidate: None,
            visible: None,
        }
    }
}

/// Tooltip metninin piksel genişliği + iç boşluk.
fn tooltip_width(text: &str) -> usize {
    MinGuiCompositor::text_width(text, 1).saturating_add(16)
}

const TOOLTIP_H: usize = 24;

/// Anchor dikdörtgenine göre ekran içi çizim dikdörtgeni üretir.
/// Alt kenardaki öğelerde (görev çubuğu) balon yukarı açılır.
fn tooltip_rect(
    anchor: (usize, usize, usize, usize),
    text: &str,
    fb_w: usize,
    fb_h: usize,
) -> (usize, usize, usize, usize) {
    let tw = tooltip_width(text).min(fb_w.saturating_sub(8));
    let anchor_cx = anchor.0.saturating_add(anchor.2 / 2);
    let mut x = anchor_cx.saturating_sub(tw / 2);
    x = x.clamp(4, fb_w.saturating_sub(tw + 4).max(4));

    let below_y = anchor.1.saturating_add(anchor.3).saturating_add(4);
    let above_y = anchor.1.saturating_sub(TOOLTIP_H + 4);
    // Anchor ekranın alt yarısındaysa balonu üstte göster.
    let y = if anchor.1 > fb_h / 2 {
        if below_y + TOOLTIP_H <= fb_h.saturating_sub(4) {
            below_y
        } else {
            above_y
        }
    } else if below_y + TOOLTIP_H <= fb_h.saturating_sub(4) {
        below_y
    } else {
        above_y
    };
    let y = y.min(fb_h.saturating_sub(TOOLTIP_H + 4));
    (x, y, tw, TOOLTIP_H)
}

impl MinGuiCompositor {
    /// Bu karede hover edilen öğeyi bildirir. Render katmanı her karede
    /// çağırmalıdır; kare sonunda `tooltip_hover` sıfırlandığı için
    /// raporlanmayan öğeler adaylıktan düşer.
    pub fn tooltip_offer(&self, anchor: (usize, usize, usize, usize), text: &'static str) {
        let mut st = self.tooltip.get();
        st.hover = Some(TooltipCandidate { anchor, text });
        self.tooltip.set(st);
    }

    /// Render karesi başında hover raporunu sıfırlar.
    pub fn tooltip_frame_begin(&self) {
        let mut st = self.tooltip.get();
        st.hover = None;
        self.tooltip.set(st);
    }

    /// Olay döngüsü her turunda çağrılır: onaylı adayı günceller, gecikmesi
    /// dolanı görünür kılar, kaybolanı kaldırır ve geçiş bölgelerini kirli
    /// işaretler.
    pub fn tooltip_tick(&mut self, now_ms: u64, fb_w: usize, fb_h: usize) {
        let mut st = self.tooltip.get();

        // hover raporu → onaylı aday mutabakatı.
        st.candidate = match (st.hover, st.candidate) {
            // Aynı öğede: gecikme saati korunur.
            (Some(h), Some(c)) if h.anchor == c.anchor && h.text == c.text => Some(c),
            // Yeni/farklı öğe: gecikme yeniden başlar.
            (Some(h), _) => Some(ConfirmedHover {
                anchor: h.anchor,
                text: h.text,
                since_ms: now_ms,
            }),
            (None, _) => None,
        };

        // Görünür tooltip mutabakatı.
        let keep = match (st.visible, st.candidate) {
            (Some(v), Some(c)) => v.anchor == c.anchor && v.text == c.text,
            _ => false,
        };
        if !keep {
            if let Some(v) = st.visible.take() {
                self.dirty_tracker
                    .track(0, DirtyRect::new(v.x, v.y, v.w, v.h));
            }
            if let Some(c) = st.candidate {
                if now_ms.wrapping_sub(c.since_ms) >= TOOLTIP_DELAY_MS {
                    let (x, y, w, h) = tooltip_rect(c.anchor, c.text, fb_w, fb_h);
                    st.visible = Some(Tooltip {
                        text: c.text,
                        anchor: c.anchor,
                        x,
                        y,
                        w,
                        h,
                    });
                    self.dirty_tracker.track(0, DirtyRect::new(x, y, w, h));
                }
            }
        }

        self.tooltip.set(st);
    }

    /// Görünen tooltip'i (varsa) çizer. Görev çubuğu katmanlarının
    /// sonunda, imleçten önce çağrılmalıdır.
    pub fn draw_tooltip(&self, fb: &mut Framebuffer) {
        let Some(t) = self.tooltip.get().visible else {
            return;
        };
        fb.draw_shadow(t.x, t.y, t.w, t.h, Color::SHADOW);
        fb.draw_rounded_rect(t.x, t.y, t.w, t.h, 4, Color::TITLE_BAR);
        fb.draw_rounded_rect_outline(t.x, t.y, t.w, t.h, 4, Color::BORDER);
        MinGuiCompositor::draw_string(fb, t.x + 8, t.y + 5, t.text, Color::TEXT_MAIN);
    }
}

//! Ortak çizim bileşenleri: Button, Toolbar, StatusBar, Dialog.
//!
//! Bu modül, `draw.rs` ve `ui_helpers.rs`'taki tekrarlanan çizim kalıplarını
//! tek bir yerde toplar. Mevcut çağrıcılar bozulmadan, yeni kod buradan
//! yararlanabilir. Tüm renkler aktif temadan çözümlenir (`Color` jetonları).

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor};

/// Metni belirtilen dikdörtgen içinde kırparak tek seferde çizer.
pub fn draw_label(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
    rect: (usize, usize, usize, usize),
) {
    MinGuiCompositor::draw_string_clipped(fb, x, y, text, color, rect);
}

/// Standart düğme çizimi (hover/pressed durumu destekler).
///
/// `pressed` true olduğunda arkaplan koyulaştırılır; `focused` true olduğunda
/// beyaz odak halkası çizilir. Metin otomatik olarak dikdörtgenin ortasına
/// yerleştirilir.
pub fn draw_button(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    label: &str,
    bg_color: u32,
    focused: bool,
    pressed: bool,
) {
    let bg = if pressed {
        MinGuiCompositor::shade(bg_color, -40)
    } else {
        bg_color
    };
    fb.draw_rounded_rect(x, y, w, h, 6, bg);
    if focused {
        fb.draw_rounded_rect_outline(x, y, w, h, 6, Color::WHITE);
    } else {
        fb.draw_rounded_rect_outline(x, y, w, h, 6, Color::BORDER);
    }
    let tw = MinGuiCompositor::text_width(label, 1);
    let tx = x + (w.saturating_sub(tw)) / 2;
    let ty = y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2;
    MinGuiCompositor::draw_string(fb, tx, ty, label, MinGuiCompositor::text_on(bg));
}

/// Toolbar: yan yana dizilmiş düğme çubuğu. `active` bulunan öğeprimary renginde,
/// diğerleriSecondary renginde çizilir.
pub fn draw_toolbar(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    items: &[&str],
    active: Option<usize>,
) {
    if items.is_empty() {
        return;
    }
    let gap = sp(6);
    let total_gap = gap.saturating_mul(items.len().saturating_sub(1));
    let btn_w = w.saturating_sub(total_gap) / items.len();
    for (i, item) in items.iter().enumerate() {
        let bx = x + i * (btn_w + gap);
        let is_active = active == Some(i);
        draw_button(
            fb,
            bx,
            y,
            btn_w,
            h,
            item,
            if is_active {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            },
            is_active,
            false,
        );
    }
}

/// StatusBar: sol ve sağ hizalı metin bölmesi.
pub fn draw_status_bar(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    left: &str,
    right: &str,
) {
    fb.draw_rect(x, y, w, h, Color::TASKBAR_BG);
    fb.draw_rect_outline(x, y, w, h, Color::BORDER);
    let th = MinGuiCompositor::text_height();
    let ty = y + (h.saturating_sub(th)) / 2;
    MinGuiCompositor::draw_string(fb, x + sp(8), ty, left, Color::TEXT_MAIN);
    let rw = MinGuiCompositor::text_width(right, 1);
    let rx = x + w.saturating_sub(rw).saturating_sub(sp(8));
    MinGuiCompositor::draw_string(fb, rx, ty, right, Color::TEXT_MUTED);
}

/// Dialog çerçevesi: gölge, arkaplan, başlık çubuğu ve içerik alanı dış hatı.
///
/// Döndürülen değer: içerik alanının `(x, y, w, h)` koordinatlarıdır.
pub fn draw_dialog_frame(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    title: &str,
) -> (usize, usize, usize, usize) {
    fb.draw_shadow(x, y, w, h, Color::SHADOW);
    fb.draw_rect(x, y, w, h, Color::WINDOW_BG);
    fb.draw_rect_outline(x, y, w, h, Color::BORDER);

    let title_h = sp(24);
    fb.draw_rect(x, y, w, title_h, Color::TITLE_BAR);
    MinGuiCompositor::draw_string_shadow(
        fb,
        x + sp(12),
        y + 6,
        title,
        Color::TEXT_MAIN,
        Color::SHADOW,
        1,
    );

    let content_y = y + title_h + sp(8);
    let content_h = h.saturating_sub(title_h + sp(16));
    (x + sp(12), content_y, w.saturating_sub(sp(24)), content_h)
}

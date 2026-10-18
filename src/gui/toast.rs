//! Toast notification types and rendering.

use super::*;

pub const MAX_TOASTS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct ToastNotification {
    pub title: [u8; 48],
    pub title_len: usize,
    pub message: [u8; 96],
    pub message_len: usize,
    pub kind: ToastKind,
    pub created_ms: u64,
    pub duration_ms: u64,
}

impl ToastNotification {
    pub const fn empty() -> Self {
        Self {
            title: [0; 48],
            title_len: 0,
            message: [0; 96],
            message_len: 0,
            kind: ToastKind::Info,
            created_ms: 0,
            duration_ms: 0,
        }
    }
}

impl MinGuiCompositor {
    /// Yeni bir toast bildirimi ekler veya kuyruktaki en eski olanı kaydırır.
    pub fn push_toast(&mut self, kind: ToastKind, title: &str, message: &str, created_ms: u64, duration_ms: u64) {
        let mut t = ToastNotification::empty();
        t.kind = kind;
        t.created_ms = created_ms;
        t.duration_ms = if duration_ms == 0 { 4000 } else { duration_ms };

        let tb = title.as_bytes();
        t.title_len = tb.len().min(t.title.len());
        t.title[..t.title_len].copy_from_slice(&tb[..t.title_len]);

        let mb = message.as_bytes();
        t.message_len = mb.len().min(t.message.len());
        t.message[..t.message_len].copy_from_slice(&mb[..t.message_len]);

        // Boş bir slot ara
        for slot in &mut self.toasts {
            if slot.is_none() {
                *slot = Some(t);
                return;
            }
        }
        // Slotlar doluysa en eskisini (0) atıp sola kaydır, en sona ekle
        for i in 0..MAX_TOASTS - 1 {
            self.toasts[i] = self.toasts[i + 1].clone();
        }
        self.toasts[MAX_TOASTS - 1] = Some(t);
    }

    /// Süresi dolan bildirimleri temizler.
    pub fn update_toasts(&mut self, now_ms: u64) {
        for slot in &mut self.toasts {
            if let Some(t) = slot {
                if now_ms.saturating_sub(t.created_ms) >= t.duration_ms {
                    *slot = None;
                }
            }
        }
    }

    /// Fare tıklaması bir bildirime veya kapatma (X) butonuna denk geldi mi?
    pub fn toast_click(&mut self, fb_w: usize, tb_y: usize, mx: usize, my: usize) -> bool {
        let mut active_count = 0usize;
        for t in &self.toasts {
            if t.is_some() { active_count += 1; }
        }
        if active_count == 0 { return false; }

        let toast_w = sp(280).min(fb_w.saturating_sub(sp(24)));
        let toast_h = sp(60);
        let toast_margin = sp(10);
        let mut current_bottom = tb_y.saturating_sub(toast_margin);
        let toast_x = fb_w.saturating_sub(toast_w + toast_margin);

        for (_i, slot) in self.toasts.iter_mut().enumerate() {
            if slot.is_some() {
                let toast_y = current_bottom.saturating_sub(toast_h);
                if in_rect(mx, my, (toast_x, toast_y, toast_w, toast_h)) {
                    // Tıklanan bildirimi kapat
                    *slot = None;
                    return true;
                }
                current_bottom = toast_y.saturating_sub(toast_margin);
            }
        }
        false
    }

    /// Bildirimleri görev çubuğunun hemen sağ üstünde şık gölgeli modern kartlar olarak çizer.
    pub fn render_toasts(&self, fb: &mut Framebuffer, tb_y: usize, mx: usize, my: usize) {
        let mut active_count = 0usize;
        for t in &self.toasts {
            if t.is_some() { active_count += 1; }
        }
        if active_count == 0 { return; }

        let toast_w = sp(280).min(fb.width.saturating_sub(sp(24)));
        let toast_h = sp(60);
        let toast_margin = sp(10);
        let mut current_bottom = tb_y.saturating_sub(toast_margin);
        let toast_x = fb.width.saturating_sub(toast_w + toast_margin);

        for slot in &self.toasts {
            if let Some(t) = slot {
                let toast_y = current_bottom.saturating_sub(toast_h);

                // 1. Yumuşak Gölgelendirme
                fb.draw_shadow(toast_x, toast_y, toast_w, toast_h, Color::SHADOW);

                // 2. Kart Gövdesi (Modern Yarı Saydam/Koyu Zemin & İnce Çerçeve)
                let is_hover = in_rect(mx, my, (toast_x, toast_y, toast_w, toast_h));
                let bg_color = if is_hover { Self::shade(Color::TASKBAR_BG, 12) } else { Color::TASKBAR_BG };
                fb.draw_rounded_rect(toast_x, toast_y, toast_w, toast_h, 8, bg_color);
                fb.draw_rounded_rect_outline(toast_x, toast_y, toast_w, toast_h, 8, Color::BORDER);

                // 3. Durum Rengi Gösterge Çizgisi (Sol kenarda şık dikey bant)
                let accent = match t.kind {
                    ToastKind::Info => Color::ACCENT_BLUE,
                    ToastKind::Success => Color::ACCENT_GREEN,
                    ToastKind::Warning => Color::ACCENT_YELLOW,
                    ToastKind::Error => Color::ACCENT_RED,
                };
                fb.draw_rounded_rect(toast_x + 2, toast_y + 4, 4, toast_h.saturating_sub(8), 2, accent);

                // 4. Tür İkonu (16x16)
                let icon_x = toast_x + sp(12);
                let icon_y = toast_y + (toast_h.saturating_sub(16)) / 2;
                match t.kind {
                    ToastKind::Info => {
                        // Bilgi "i" dairesi
                        fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                        fb.draw_rect(icon_x + 7, icon_y + 4, 2, 2, accent);
                        fb.draw_rect(icon_x + 7, icon_y + 8, 2, 5, accent);
                    }
                    ToastKind::Success => {
                        // Başarı "✓" onay tiki
                        fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                        fb.draw_rect(icon_x + 4, icon_y + 8, 2, 3, accent);
                        fb.draw_rect(icon_x + 6, icon_y + 10, 2, 3, accent);
                        fb.draw_rect(icon_x + 8, icon_y + 7, 2, 3, accent);
                        fb.draw_rect(icon_x + 10, icon_y + 5, 2, 3, accent);
                    }
                    ToastKind::Warning => {
                        // Uyarı "!" üçgeni
                        fb.draw_rect_outline(icon_x + 1, icon_y + 1, 14, 14, accent);
                        fb.draw_rect(icon_x + 7, icon_y + 4, 2, 5, accent);
                        fb.draw_rect(icon_x + 7, icon_y + 11, 2, 2, accent);
                    }
                    ToastKind::Error => {
                        // Hata "✕" çarpı
                        fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                        fb.draw_rect(icon_x + 4, icon_y + 4, 3, 3, accent);
                        fb.draw_rect(icon_x + 9, icon_y + 9, 3, 3, accent);
                        fb.draw_rect(icon_x + 9, icon_y + 4, 3, 3, accent);
                        fb.draw_rect(icon_x + 4, icon_y + 9, 3, 3, accent);
                    }
                }

                // 5. Başlık ve Mesaj Metinleri
                let title_str = core::str::from_utf8(&t.title[..t.title_len]).unwrap_or("minOs");
                let msg_str = core::str::from_utf8(&t.message[..t.message_len]).unwrap_or("");
                let text_x = icon_x + 22;
                let text_max_w = toast_w.saturating_sub(sp(54));

                Self::draw_string_clipped(fb, text_x, toast_y + sp(10), title_str, Color::TEXT_MAIN,
                    (text_x, toast_y + 6, text_max_w, 18));
                Self::draw_string_clipped(fb, text_x, toast_y + sp(30), msg_str, Color::TEXT_MUTED,
                    (text_x, toast_y + 28, text_max_w, 20));

                // 6. Sağ üst kapatma "✕" ikonu
                let close_x = toast_x + toast_w.saturating_sub(sp(18));
                let close_y = toast_y + sp(8);
                let close_hover = in_rect(mx, my, (close_x.saturating_sub(2), close_y.saturating_sub(2), 14, 14));
                let close_col = if close_hover { Color::ACCENT_RED } else { Color::TEXT_MUTED };
                fb.draw_rect(close_x, close_y, 2, 2, close_col);
                fb.draw_rect(close_x + 6, close_y + 6, 2, 2, close_col);
                fb.draw_rect(close_x + 6, close_y, 2, 2, close_col);
                fb.draw_rect(close_x, close_y + 6, 2, 2, close_col);
                fb.draw_rect(close_x + 3, close_y + 3, 2, 2, close_col);

                current_bottom = toast_y.saturating_sub(toast_margin);
            }
        }
    }
}

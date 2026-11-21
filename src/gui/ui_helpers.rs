//! Standardized UI helpers for consistent error/status handling across apps.

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf, ToastKind};

/// Standart toast bildirim yardımcıları.
///
/// Otomatik kapanma süreleri tür başına tek yerde tanımlıdır
/// (`ToastKind::default_duration_ms`): bilgi/başarı 3 sn, uyarı 4 sn,
/// hata 5 sn. Kullanıcı her bildirimi Esc veya ✕ ile kapatabilir.
fn toast(
    compositor: &mut crate::gui::MinGuiCompositor,
    kind: ToastKind,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    compositor.push_toast(kind, title, msg, now_ms, kind.default_duration_ms());
}

/// Hata bildirimi (kırmızı ✕, 5 sn).
pub fn toast_error(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    toast(compositor, ToastKind::Error, now_ms, title, msg);
}

/// Başarı bildirimi (yeşil ✓, 3 sn).
pub fn toast_success(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    toast(compositor, ToastKind::Success, now_ms, title, msg);
}

/// Uyarı bildirimi (sarı !, 4 sn).
pub fn toast_warning(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    toast(compositor, ToastKind::Warning, now_ms, title, msg);
}

/// Bilgi bildirimi (mavi i, 3 sn).
pub fn toast_info(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    toast(compositor, ToastKind::Info, now_ms, title, msg);
}

/// Standard confirmation dialog rendering
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    None,
    Yes,
    No,
}

pub struct ConfirmDialog {
    pub title: &'static str,
    pub message: &'static str,
    pub detail: Option<&'static str>,
    pub focus_yes: bool,
    pub visible: bool,
}

impl ConfirmDialog {
    pub const fn new() -> Self {
        Self {
            title: "",
            message: "",
            detail: None,
            focus_yes: true,
            visible: false,
        }
    }

    pub fn show(
        &mut self,
        title: &'static str,
        message: &'static str,
        detail: Option<&'static str>,
    ) {
        self.title = title;
        self.message = message;
        self.detail = detail;
        self.focus_yes = true;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn handle_key(&mut self, key: u8) -> ConfirmResult {
        if !self.visible {
            return ConfirmResult::None;
        }
        match key {
            b'y' | b'Y' | b'e' | b'E' | b'\n' | b'\r' | b' ' => {
                self.visible = false;
                ConfirmResult::Yes
            }
            b'n' | b'N' | b'h' | b'H' | 0x1b => {
                // Esc
                self.visible = false;
                ConfirmResult::No
            }
            b'\t' => {
                self.focus_yes = !self.focus_yes;
                ConfirmResult::None
            }
            _ => ConfirmResult::None,
        }
    }

    pub fn render(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        if !self.visible {
            return;
        }

        // Semi-transparent overlay
        fb.draw_rect(win_x, win_y, win_w, win_h, Color::DARK_BG);

        // Dialog box
        let is_compact = win_w < 320 || win_h < 200;
        let dlg_w = win_w.saturating_sub(16).min(480);
        let dlg_h = if is_compact {
            win_h.saturating_sub(16).min(sp(140)).max(100)
        } else {
            win_h.saturating_sub(24).min(sp(160)).max(120)
        };
        let dlg_x = win_x + (win_w.saturating_sub(dlg_w)) / 2;
        let dlg_y = win_y + (win_h.saturating_sub(dlg_h)) / 2;

        fb.draw_rounded_rect(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::ACCENT_YELLOW);

        // Title
        let title_y = if is_compact {
            dlg_y + 8
        } else {
            dlg_y + sp(16)
        };
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(self.title, 1))) / 2,
            title_y,
            self.title,
            Color::ACCENT_RED,
        );

        // Message
        let msg_y = if is_compact {
            dlg_y + 28
        } else {
            dlg_y + sp(50)
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            dlg_x + (if is_compact { 10 } else { sp(20) }),
            msg_y,
            self.message,
            Color::TEXT_MAIN,
            (dlg_x + 8, msg_y, dlg_w.saturating_sub(16), 18),
        );

        // Detail
        let detail_y = if is_compact {
            dlg_y + 46
        } else {
            dlg_y + sp(70)
        };
        if let Some(detail) = self.detail {
            if !is_compact || dlg_h >= 130 {
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    dlg_x + (if is_compact { 10 } else { sp(20) }),
                    detail_y,
                    detail,
                    Color::ACCENT_YELLOW,
                    (dlg_x + 8, detail_y, dlg_w.saturating_sub(16), 18),
                );
            }
        }

        // Instructions
        let eng = crate::gui::ui_english();
        let hint = if eng {
            "Enter/Y=Yes, Esc/N=No, Tab=Switch"
        } else {
            "Enter/E=Evet, Esc/H=Hayır, Tab=Değiştir"
        };
        if !is_compact && dlg_h >= 150 {
            MinGuiCompositor::draw_string(
                fb,
                dlg_x + sp(20),
                dlg_y + sp(95),
                hint,
                Color::TEXT_MUTED,
            );
        }

        // Yes / No buttons
        let btn_w = if is_compact {
            64.min(dlg_w.saturating_sub(24) / 2)
        } else {
            sp(80)
        };
        let btn_h = if is_compact { 22 } else { sp(28) };
        let btn_gap = if is_compact { 8 } else { sp(20) };
        let btn_y = dlg_y + dlg_h.saturating_sub(btn_h + (if is_compact { 8 } else { sp(16) }));
        let total_btn_w = btn_w * 2 + btn_gap;
        let btn_start_x = dlg_x + (dlg_w.saturating_sub(total_btn_w)) / 2;

        // Yes button
        let yes_x = btn_start_x;
        let yes_bg = if self.focus_yes {
            Color::ACCENT_GREEN
        } else {
            Color::WINDOW_BG
        };
        fb.draw_rounded_rect(yes_x, btn_y, btn_w, btn_h, 6, yes_bg);
        fb.draw_rounded_rect_outline(yes_x, btn_y, btn_w, btn_h, 6, Color::WHITE);
        let yes_text = if eng { "Yes (Y)" } else { "Evet (E)" };
        let yes_tw = MinGuiCompositor::text_width(yes_text, 1);
        MinGuiCompositor::draw_string(
            fb,
            yes_x + (btn_w.saturating_sub(yes_tw)) / 2,
            btn_y + (btn_h.saturating_sub(MinGuiCompositor::text_height())) / 2,
            yes_text,
            MinGuiCompositor::text_on(yes_bg),
        );

        // No button
        let no_x = btn_start_x + btn_w + btn_gap;
        let no_bg = if !self.focus_yes {
            Color::ACCENT_RED
        } else {
            Color::WINDOW_BG
        };
        fb.draw_rounded_rect(no_x, btn_y, btn_w, btn_h, 6, no_bg);
        fb.draw_rounded_rect_outline(no_x, btn_y, btn_w, btn_h, 6, Color::WHITE);
        let no_text = if eng { "No (N)" } else { "Hayır (H)" };
        let no_tw = MinGuiCompositor::text_width(no_text, 1);
        MinGuiCompositor::draw_string(
            fb,
            no_x + (btn_w.saturating_sub(no_tw)) / 2,
            btn_y + (btn_h.saturating_sub(MinGuiCompositor::text_height())) / 2,
            no_text,
            MinGuiCompositor::text_on(no_bg),
        );
    }
}

/// Default implementation for ConfirmDialog
impl Default for ConfirmDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard progress indicator
pub struct ProgressIndicator {
    pub title: &'static str,
    pub message: &'static str,
    pub percent: usize, // 0-100
    pub state: ProgressState,
    pub visible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    Running,
    Complete,
    Failed,
}

impl ProgressIndicator {
    pub const fn new() -> Self {
        Self {
            title: "",
            message: "",
            percent: 0,
            state: ProgressState::Running,
            visible: false,
        }
    }

    pub fn show(&mut self, title: &'static str, message: &'static str) {
        self.title = title;
        self.message = message;
        self.percent = 0;
        self.state = ProgressState::Running;
        self.visible = true;
    }

    pub fn update(&mut self, percent: usize, message: Option<&'static str>) {
        self.percent = percent.min(100);
        if let Some(msg) = message {
            self.message = msg;
        }
        if self.percent >= 100 {
            self.state = ProgressState::Complete;
        }
    }

    pub fn fail(&mut self, message: &'static str) {
        self.message = message;
        self.state = ProgressState::Failed;
        self.percent = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn render(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        if !self.visible {
            return;
        }

        // Semi-transparent overlay
        fb.draw_rect(win_x, win_y, win_w, win_h, Color::DARK_BG);

        // Dialog box
        let is_compact = win_w < 320 || win_h < 200;
        let dlg_w = win_w.saturating_sub(16).min(440);
        let dlg_h = if is_compact {
            win_h.saturating_sub(16).min(sp(110)).max(90)
        } else {
            win_h.saturating_sub(24).min(sp(130)).max(100)
        };
        let dlg_x = win_x + (win_w.saturating_sub(dlg_w)) / 2;
        let dlg_y = win_y + (win_h.saturating_sub(dlg_h)) / 2;

        fb.draw_rounded_rect(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::WINDOW_BG);
        let border_color = match self.state {
            ProgressState::Failed => Color::ACCENT_RED,
            ProgressState::Complete => Color::ACCENT_GREEN,
            _ => Color::ACCENT_BLUE,
        };
        fb.draw_rounded_rect_outline(dlg_x, dlg_y, dlg_w, dlg_h, 8, border_color);

        // Title
        let title_y = if is_compact {
            dlg_y + 8
        } else {
            dlg_y + sp(14)
        };
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(self.title, 1))) / 2,
            title_y,
            self.title,
            border_color,
        );

        // Message
        let msg_y = if is_compact {
            dlg_y + 26
        } else {
            dlg_y + sp(44)
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            dlg_x + (if is_compact { 10 } else { sp(20) }),
            msg_y,
            self.message,
            Color::TEXT_MAIN,
            (dlg_x + 8, msg_y, dlg_w.saturating_sub(16), 18),
        );

        // Progress bar
        let bar_x = dlg_x + (if is_compact { 10 } else { sp(20) });
        let bar_y = if is_compact {
            dlg_y + 48
        } else {
            dlg_y + sp(70)
        };
        let bar_w = dlg_w.saturating_sub(if is_compact { 20 } else { sp(40) });
        let bar_h = if is_compact { 10 } else { sp(14) };

        fb.draw_rounded_rect(bar_x, bar_y, bar_w, bar_h, 4, Color::DARK_BG);
        fb.draw_rounded_rect_outline(bar_x, bar_y, bar_w, bar_h, 4, Color::BORDER);

        if self.percent > 0 {
            let filled_w = (bar_w * self.percent) / 100;
            let fill_color = match self.state {
                ProgressState::Failed => Color::ACCENT_RED,
                ProgressState::Complete => Color::ACCENT_GREEN,
                _ => Color::ACCENT_BLUE,
            };
            fb.draw_rounded_rect(
                bar_x + 1,
                bar_y + 1,
                filled_w.saturating_sub(2),
                bar_h - 2,
                3,
                fill_color,
            );
        }

        // Percentage text
        let mut pct_buf = TextBuf::new();
        pct_buf.push_usize(self.percent);
        pct_buf.push_str("%");
        if self.state == ProgressState::Complete {
            pct_buf.push_str(if crate::gui::ui_english() {
                " Complete"
            } else {
                " Tamam"
            });
        } else if self.state == ProgressState::Failed {
            pct_buf.push_str(if crate::gui::ui_english() {
                " Failed"
            } else {
                " Hata"
            });
        }
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(pct_buf.as_str(), 1))) / 2,
            bar_y + bar_h + sp(6),
            pct_buf.as_str(),
            Color::TEXT_MUTED,
        );
    }
}

/// Default implementation for ProgressIndicator
impl Default for ProgressIndicator {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard button styling helpers
pub fn draw_standard_button(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    label: &str,
    bg_color: u32,
    focused: bool,
) {
    fb.draw_rounded_rect(x, y, w, h, 6, bg_color);
    if focused {
        fb.draw_rounded_rect_outline(x, y, w, h, 6, Color::WHITE);
    } else {
        fb.draw_rounded_rect_outline(x, y, w, h, 6, Color::BORDER);
    }
    let tw = MinGuiCompositor::text_width(label, 1);
    let tx = x + (w.saturating_sub(tw)) / 2;
    let ty = y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2;
    MinGuiCompositor::draw_string(fb, tx, ty, label, MinGuiCompositor::text_on(bg_color));
}

/// Button color constants for consistent styling
pub mod button_colors {
    use crate::gui::Color;
    pub const PRIMARY: u32 = Color::ACCENT_BLUE;
    pub const SUCCESS: u32 = Color::ACCENT_GREEN;
    pub const WARNING: u32 = Color::ACCENT_YELLOW;
    pub const DANGER: u32 = Color::ACCENT_RED;
    pub const SECONDARY: u32 = Color::WINDOW_BG;
    pub const DISABLED: u32 = Color::TEXT_MUTED;
}

/// Standard error display inline (for forms, lists, etc.)
pub fn draw_inline_error(fb: &mut Framebuffer, x: usize, y: usize, w: usize, message: &str) {
    let h = sp(24);
    fb.draw_rounded_rect(x, y, w, h, 4, Color::ACCENT_RED);
    MinGuiCompositor::draw_string(
        fb,
        x + sp(8),
        y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2,
        message,
        Color::WHITE,
    );
}

/// Standard success display inline
pub fn draw_inline_success(fb: &mut Framebuffer, x: usize, y: usize, w: usize, message: &str) {
    let h = sp(24);
    fb.draw_rounded_rect(x, y, w, h, 4, Color::ACCENT_GREEN);
    MinGuiCompositor::draw_string(
        fb,
        x + sp(8),
        y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2,
        message,
        Color::WHITE,
    );
}

/// Standard info display inline
pub fn draw_inline_info(fb: &mut Framebuffer, x: usize, y: usize, w: usize, message: &str) {
    let h = sp(24);
    fb.draw_rounded_rect(x, y, w, h, 4, Color::ACCENT_BLUE);
    MinGuiCompositor::draw_string(
        fb,
        x + sp(8),
        y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2,
        message,
        Color::WHITE,
    );
}

/// Standard warning display inline
pub fn draw_inline_warning(fb: &mut Framebuffer, x: usize, y: usize, w: usize, message: &str) {
    let h = sp(24);
    fb.draw_rounded_rect(x, y, w, h, 4, Color::ACCENT_YELLOW);
    MinGuiCompositor::draw_string(
        fb,
        x + sp(8),
        y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2,
        message,
        Color::BLACK,
    );
}

/// Focus ring wrapper for consistent keyboard navigation indication
pub fn draw_focus_ring_std(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
    MinGuiCompositor::draw_focus_ring(fb, x, y, w, h);
}

/// Boş durum (Empty State) kartı / ekranı çizim fonksiyonu.
/// Dosya yok, seçim yok, ağ yok, resim yüklenemedi gibi durumlarda
/// simge + açıklayıcı başlık/mesaj + aksiyon butonu ("Yenile", "Tekrar dene", "Klasör oluştur").
pub fn draw_empty_state(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    icon_type: EmptyStateIcon,
    title: &str,
    subtitle: &str,
    action_label: Option<&str>,
    action_focused: bool,
) -> Option<(usize, usize, usize, usize)> {
    if w < 60 || h < 40 {
        return None;
    }

    let is_compact = w < 240 || h < 160;
    let card_pad_y = if is_compact { 6 } else { 16 };
    let icon_sz = if is_compact { 16 } else { 28 };
    let center_x = x + w / 2;
    let mut cur_y = y + card_pad_y;

    // 1. Simge / İkon
    let icon_x = center_x.saturating_sub(icon_sz / 2);
    match icon_type {
        EmptyStateIcon::Folder => {
            let s = if is_compact { 1 } else { 2 };
            crate::apps::filemanager::FileManagerApp::draw_folder_icon_sized(
                fb, icon_x, cur_y, s, false,
            );
        }
        EmptyStateIcon::NoSelection => {
            // Seçim yok: soru/belge ikonu
            let s = if is_compact { 1 } else { 2 };
            crate::apps::filemanager::FileManagerApp::draw_file_icon_sized(
                fb, icon_x, cur_y, s, "TXT",
            );
        }
        EmptyStateIcon::NoNetwork => {
            // Ağ yok: uyarı çubuğu / ağ kartı simgesi
            fb.draw_rounded_rect(icon_x, cur_y, icon_sz, icon_sz, 4, Color::ACCENT_RED);
            MinGuiCompositor::draw_string(
                fb,
                icon_x + (icon_sz.saturating_sub(8)) / 2,
                cur_y + (icon_sz.saturating_sub(12)) / 2,
                "X",
                Color::WHITE,
            );
        }
        EmptyStateIcon::ImageFailed => {
            // Resim yüklenemedi
            fb.draw_rounded_rect(icon_x, cur_y, icon_sz, icon_sz, 4, Color::DARK_BG);
            fb.draw_rounded_rect_outline(icon_x, cur_y, icon_sz, icon_sz, 4, Color::ACCENT_RED);
            MinGuiCompositor::draw_string(
                fb,
                icon_x + (icon_sz.saturating_sub(8)) / 2,
                cur_y + (icon_sz.saturating_sub(12)) / 2,
                "!",
                Color::ACCENT_YELLOW,
            );
        }
    }
    cur_y += icon_sz + (if is_compact { 4 } else { 8 });

    // 2. Başlık
    if cur_y + 14 < y + h {
        let tw = MinGuiCompositor::text_width(title, 1).min(w.saturating_sub(12));
        let tx = center_x.saturating_sub(tw / 2);
        MinGuiCompositor::draw_string_clipped(
            fb,
            tx,
            cur_y,
            title,
            Color::TEXT_MAIN,
            (x + 4, cur_y, w.saturating_sub(8), 16),
        );
        cur_y += 16;
    }

    // 3. Alt Mesaj (yeterli alan varsa)
    if !is_compact && !subtitle.is_empty() && cur_y + 14 < y + h {
        let sw = MinGuiCompositor::text_width(subtitle, 1).min(w.saturating_sub(16));
        let sx = center_x.saturating_sub(sw / 2);
        MinGuiCompositor::draw_string_clipped(
            fb,
            sx,
            cur_y,
            subtitle,
            Color::TEXT_MUTED,
            (x + 4, cur_y, w.saturating_sub(8), 14),
        );
        cur_y += 18;
    }

    // 4. Aksiyon Butonu
    if let Some(lbl) = action_label {
        let btn_h = if is_compact { 20 } else { 24 };
        if cur_y + btn_h <= y + h {
            let bw = (MinGuiCompositor::text_width(lbl, 1) + 20)
                .min(w.saturating_sub(20))
                .max(64);
            let bx = center_x.saturating_sub(bw / 2);
            let by = cur_y + (if is_compact { 4 } else { 8 });
            let rect = (bx, by, bw, btn_h);
            draw_standard_button(
                fb,
                bx,
                by,
                bw,
                btn_h,
                lbl,
                Color::ACCENT_BLUE,
                action_focused,
            );
            return Some(rect);
        }
    }

    None
}

/// Boş durum ikon türleri
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum EmptyStateIcon {
    Folder,
    NoSelection,
    NoNetwork,
    ImageFailed,
}

//! Standardized UI helpers for consistent error/status handling across apps.

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

/// Standard toast notification helpers
pub fn toast_error(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    compositor.push_toast(crate::gui::ToastKind::Error, title, msg, now_ms, 5000);
}

pub fn toast_success(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    compositor.push_toast(crate::gui::ToastKind::Success, title, msg, now_ms, 3000);
}

pub fn toast_warning(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    compositor.push_toast(crate::gui::ToastKind::Warning, title, msg, now_ms, 4000);
}

pub fn toast_info(
    compositor: &mut crate::gui::MinGuiCompositor,
    now_ms: u64,
    title: &str,
    msg: &str,
) {
    compositor.push_toast(crate::gui::ToastKind::Info, title, msg, now_ms, 3000);
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
        let dlg_w = win_w.min(480);
        let dlg_h = sp(160);
        let dlg_x = win_x + (win_w.saturating_sub(dlg_w)) / 2;
        let dlg_y = win_y + (win_h.saturating_sub(dlg_h)) / 2;

        fb.draw_rounded_rect(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::ACCENT_YELLOW);

        // Title
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(self.title, 1))) / 2,
            dlg_y + sp(16),
            self.title,
            Color::ACCENT_RED,
        );

        // Message
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + sp(20),
            dlg_y + sp(50),
            self.message,
            Color::TEXT_MAIN,
        );

        // Detail
        if let Some(detail) = self.detail {
            MinGuiCompositor::draw_string(
                fb,
                dlg_x + sp(20),
                dlg_y + sp(70),
                detail,
                Color::ACCENT_YELLOW,
            );
        }

        // Instructions
        let eng = crate::gui::ui_english();
        let hint = if eng {
            "Enter/Y=Yes, Esc/N=No, Tab=Switch"
        } else {
            "Enter/E=Evet, Esc/H=Hayır, Tab=Değiştir"
        };
        MinGuiCompositor::draw_string(fb, dlg_x + sp(20), dlg_y + sp(95), hint, Color::TEXT_MUTED);

        // Yes / No buttons
        let btn_w = sp(80);
        let btn_h = sp(28);
        let btn_gap = sp(20);
        let btn_y = dlg_y + dlg_h - btn_h - sp(16);
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
        let dlg_w = win_w.min(440);
        let dlg_h = sp(130);
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
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(self.title, 1))) / 2,
            dlg_y + sp(14),
            self.title,
            border_color,
        );

        // Message
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + sp(20),
            dlg_y + sp(44),
            self.message,
            Color::TEXT_MAIN,
        );

        // Progress bar
        let bar_x = dlg_x + sp(20);
        let bar_y = dlg_y + sp(70);
        let bar_w = dlg_w - sp(40);
        let bar_h = sp(14);

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

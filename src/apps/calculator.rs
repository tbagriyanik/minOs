use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

use super::calc_core::CalcCore;

// Six decimal places and a wide integer range, without relying on floating point.
const BUTTONS: [[char; 4]; 5] = [
    ['%', 'E', 'C', 'B'],
    ['7', '8', '9', '/'],
    ['4', '5', '6', '*'],
    ['1', '2', '3', '-'],
    ['0', '.', '=', '+'],
];
const BUTTON_TOP: usize = 100;
const BUTTON_GAP_X: usize = 6;
const BUTTON_GAP_Y: usize = 6;
const BUTTON_HEIGHT: usize = 32;

pub struct CalculatorApp {
    /// Saf aritmetik çekirdeği; host testlerinde ayrıca doğrulanır.
    pub core: CalcCore,
}

impl CalculatorApp {
    pub const fn new() -> Self {
        Self {
            core: CalcCore::new(),
        }
    }

    pub fn insert(&mut self, c: char) {
        self.core.insert(c);
    }

    pub fn backspace(&mut self) {
        self.core.backspace();
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }

    pub fn press(&mut self, key: char) {
        self.core.press(key);
    }

    /// Copy the currently displayed calculation result to the shared clipboard.
    pub fn copy_result(&self) {
        let value = self.core.value();
        let mut output = [0u8; crate::apps::calc_core::INPUT_CAP];
        let len = CalcCore::format_value(value, &mut output);
        let text = core::str::from_utf8(&output[..len]).unwrap_or("");
        crate::gui::clipboard::set_str(text);
    }

    /// Copy the current expression and clear it, matching the usual Ctrl+X behavior.
    pub fn cut_expression(&mut self) {
        crate::gui::clipboard::set_chars(self.core.expression());
        self.core.clear();
    }

    /// Replace the expression with a sanitized numeric expression from the shared clipboard.
    pub fn paste_expression(&mut self) {
        let mut clipboard = [' '; crate::gui::clipboard::CAPACITY];
        let len = crate::gui::clipboard::copy_chars(&mut clipboard);
        let mut expression = [' '; crate::apps::calc_core::INPUT_CAP];
        let mut expression_len = 0;
        for ch in clipboard[..len].iter().copied() {
            if ch.is_ascii_whitespace() {
                continue;
            }
            if !(ch.is_ascii_digit() || matches!(ch, '.' | '+' | '-' | '*' | '/')) {
                return;
            }
            if expression_len >= expression.len() - 1 {
                break;
            }
            expression[expression_len] = ch;
            expression_len += 1;
        }
        if expression_len == 0 {
            return;
        }
        self.core.clear();
        for ch in expression[..expression_len].iter().copied() {
            self.core.insert(ch);
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let display_x = x + 10;
        let display_y = y + 32;
        let display_w = w.saturating_sub(20);
        fb.draw_rect(display_x, display_y, display_w, 58, Color::DARK_BG);

        let mut expression = [0u8; 64];
        for (index, c) in self.core.expression().iter().enumerate() {
            expression[index] = *c as u8;
        }
        if let Ok(text) = core::str::from_utf8(&expression[..self.core.len]) {
            MinGuiCompositor::draw_string(
                fb,
                display_x + 8,
                display_y + 7,
                text,
                Color::TEXT_MUTED,
            );
        }
        let mut result = [0u8; 64];
        let result_len = CalcCore::format_value(self.core.value(), &mut result);
        if let Ok(text) = core::str::from_utf8(&result[..result_len]) {
            let text_width = MinGuiCompositor::text_width(text, 1);
            let result_x = display_x + display_w.saturating_sub(text_width + 8);
            MinGuiCompositor::draw_string_clipped(
                fb,
                result_x,
                display_y + 32,
                text,
                Color::ACCENT_GREEN,
                (
                    display_x + 4,
                    display_y + 28,
                    display_w.saturating_sub(8),
                    24,
                ),
            );
        }

        let gap_x = BUTTON_GAP_X;
        let button_w = display_w.saturating_sub(gap_x * 3) / 4;
        let button_y = y + BUTTON_TOP;
        for row in 0..BUTTONS.len() {
            for col in 0..4 {
                let key = BUTTONS[row][col];
                let bx = display_x + col * (button_w + gap_x);
                let by = button_y + row * (BUTTON_HEIGHT + BUTTON_GAP_Y);
                let color = match key {
                    '=' => Color::ACCENT_GREEN,
                    'C' | 'E' | 'B' | '%' => Color::TITLE_BAR_INACTIVE,
                    '+' | '-' | '*' | '/' => Color::ACCENT_BLUE,
                    _ => Color::TITLE_BAR,
                };
                fb.draw_rect(bx, by, button_w, BUTTON_HEIGHT, color);
                let label = match key {
                    'E' => "CE",
                    'B' => "BS",
                    _ => {
                        expression[0] = key as u8;
                        core::str::from_utf8(&expression[..1]).unwrap_or("")
                    }
                };
                let label_x = bx + button_w.saturating_sub(label.len() * 7) / 2;
                MinGuiCompositor::draw_string(fb, label_x, by + 11, label, Color::TEXT_MAIN);
            }
        }
        let _ = h;
    }

    pub fn key_at(x: usize, y: usize, wx: usize, wy: usize, ww: usize) -> Option<char> {
        let display_x = wx + 10;
        let display_w = ww.saturating_sub(20);
        let gap_x = BUTTON_GAP_X;
        let button_w = display_w.saturating_sub(gap_x * 3) / 4;
        let button_y = wy + BUTTON_TOP;
        if x < display_x || y < button_y {
            return None;
        }
        let row = (y - button_y) / (BUTTON_HEIGHT + BUTTON_GAP_Y);
        if row >= BUTTONS.len() || (y - button_y) % (BUTTON_HEIGHT + BUTTON_GAP_Y) >= BUTTON_HEIGHT
        {
            return None;
        }
        let rel_x = x - display_x;
        let col = rel_x / (button_w + gap_x);
        if col >= 4 || rel_x % (button_w + gap_x) >= button_w {
            return None;
        }
        Some(BUTTONS[row][col])
    }
}

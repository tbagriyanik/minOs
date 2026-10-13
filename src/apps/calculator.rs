use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

const SCALE: i32 = 1000;
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
    pub input: [char; 64],
    pub len: usize,
}

impl CalculatorApp {
    pub const fn new() -> Self {
        Self { input: [' '; 64], len: 0 }
    }

    pub fn insert(&mut self, c: char) {
        if self.len < self.input.len() - 1 {
            if c == '.' {
                let mut i = self.len;
                while i > 0 && !Self::is_operator(self.input[i - 1]) {
                    if self.input[i - 1] == '.' { return; }
                    i -= 1;
                }
            }
            if Self::is_operator(c) && self.len > 0 && Self::is_operator(self.input[self.len - 1]) {
                self.input[self.len - 1] = c;
                return;
            }
            self.input[self.len] = c;
            self.len += 1;
        }
    }

    pub fn backspace(&mut self) {
        if self.len > 0 { self.len -= 1; }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Copy the currently displayed calculation result to the shared clipboard.
    pub fn copy_result(&self) {
        let value = self.value();
        let mut output = [0u8; 24];
        let len = Self::format_value(value, &mut output);
        let text = core::str::from_utf8(&output[..len]).unwrap_or("");
        crate::gui::clipboard::set_str(text);
    }

    /// Copy the current expression and clear it, matching the usual Ctrl+X behavior.
    pub fn cut_expression(&mut self) {
        crate::gui::clipboard::set_chars(&self.input[..self.len]);
        self.clear();
    }

    /// Replace the expression with a sanitized numeric expression from the shared clipboard.
    pub fn paste_expression(&mut self) {
        let mut clipboard = [' '; crate::gui::clipboard::CAPACITY];
        let len = crate::gui::clipboard::copy_chars(&mut clipboard);
        let mut expression = [' '; 64];
        let mut expression_len = 0;
        for ch in clipboard[..len].iter().copied() {
            if ch.is_ascii_whitespace() { continue; }
            if !(ch.is_ascii_digit() || matches!(ch, '.' | '+' | '-' | '*' | '/')) { return; }
            if expression_len >= expression.len() - 1 { break; }
            expression[expression_len] = ch;
            expression_len += 1;
        }
        if expression_len == 0 { return; }
        self.clear();
        for ch in expression[..expression_len].iter().copied() {
            self.insert(ch);
        }
    }

    pub fn press(&mut self, key: char) {
        match key {
            'C' => self.clear(),
            'B' | '\x08' => self.backspace(),
            'E' => {
                while self.len > 0 && !Self::is_operator(self.input[self.len - 1]) {
                    self.len -= 1;
                }
            }
            '%' => {
                let value = self.value() / 100;
                self.set_value(value);
            }
            '=' => self.set_value(self.value()),
            '0'..='9' | '.' | '+' | '-' | '*' | '/' => self.insert(key),
            _ => {}
        }
    }

    fn is_operator(c: char) -> bool {
        matches!(c, '+' | '-' | '*' | '/')
    }

    fn value(&self) -> i32 {
        let mut total = 0i32;
        let mut term = 0i32;
        let mut sign = 1i32;
        let mut op = '+';
        let mut number = 0i32;
        let mut number_sign = 1i32;
        let mut fraction_scale = 0i32;
        let mut has_digit = false;

        for index in 0..=self.len {
            let c = if index == self.len { '+' } else { self.input[index] };
            if c.is_ascii_digit() {
                let digit = c as i32 - '0' as i32;
                if fraction_scale == 0 {
                    number = number.saturating_mul(10).saturating_add(digit.saturating_mul(SCALE));
                } else if fraction_scale > 1 {
                    number = number.saturating_add(digit.saturating_mul(fraction_scale / 10));
                    fraction_scale /= 10;
                }
                has_digit = true;
            } else if c == '.' {
                if fraction_scale == 0 { fraction_scale = SCALE; }
            } else if Self::is_operator(c) {
                if !has_digit {
                    if c == '-' { number_sign = -number_sign; }
                    continue;
                }
                number = number.saturating_mul(number_sign);
                term = match op {
                    '*' => term.saturating_mul(number) / SCALE,
                    '/' if number != 0 => term.saturating_mul(SCALE) / number,
                    '/' => term,
                    _ => number,
                };
                if c == '+' || c == '-' {
                    total = total.saturating_add(sign.saturating_mul(term));
                    term = 0;
                    sign = if c == '-' { -1 } else { 1 };
                }
                op = c;
                number = 0;
                number_sign = 1;
                fraction_scale = 0;
                has_digit = false;
            }
        }
        total.saturating_add(sign.saturating_mul(term))
    }

    fn set_value(&mut self, value: i32) {
        self.len = 0;
        let mut output = [0u8; 24];
        let len = Self::format_value(value, &mut output);
        for byte in &output[..len] {
            self.input[self.len] = *byte as char;
            self.len += 1;
        }
    }

    fn format_value(value: i32, output: &mut [u8; 24]) -> usize {
        let mut len = 0;
        let magnitude = value.saturating_abs();
        if value < 0 { output[len] = b'-'; len += 1; }
        Self::append_unsigned(output, &mut len, (magnitude / SCALE) as u32);
        let mut fraction = (magnitude % SCALE) as u32;
        if fraction != 0 {
            output[len] = b'.'; len += 1;
            output[len] = b'0' + (fraction / 100) as u8; len += 1;
            fraction %= 100;
            output[len] = b'0' + (fraction / 10) as u8; len += 1;
            output[len] = b'0' + (fraction % 10) as u8; len += 1;
            while output[len - 1] == b'0' { len -= 1; }
        }
        len
    }

    fn append_unsigned(output: &mut [u8; 24], len: &mut usize, mut value: u32) {
        let mut digits = [0u8; 12];
        let mut count = 0;
        if value == 0 { digits[count] = b'0'; count += 1; }
        while value > 0 {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
        }
        for index in (0..count).rev() {
            output[*len] = digits[index];
            *len += 1;
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let display_x = x + 10;
        let display_y = y + 32;
        let display_w = w.saturating_sub(20);
        fb.draw_rect(display_x, display_y, display_w, 58, Color::DARK_BG);

        let mut expression = [0u8; 64];
        for (index, c) in self.input[..self.len].iter().enumerate() {
            expression[index] = *c as u8;
        }
        if let Ok(text) = core::str::from_utf8(&expression[..self.len]) {
            MinGuiCompositor::draw_string(fb, display_x + 8, display_y + 7, text, Color::TEXT_MUTED);
        }
        let mut result = [0u8; 24];
        let result_len = Self::format_value(self.value(), &mut result);
        if let Ok(text) = core::str::from_utf8(&result[..result_len]) {
            MinGuiCompositor::draw_string(fb, display_x + 8, display_y + 32, text, Color::ACCENT_GREEN);
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
        if x < display_x || y < button_y { return None; }
        let row = (y - button_y) / (BUTTON_HEIGHT + BUTTON_GAP_Y);
        if row >= BUTTONS.len() || (y - button_y) % (BUTTON_HEIGHT + BUTTON_GAP_Y) >= BUTTON_HEIGHT {
            return None;
        }
        let rel_x = x - display_x;
        let col = rel_x / (button_w + gap_x);
        if col >= 4 || rel_x % (button_w + gap_x) >= button_w { return None; }
        Some(BUTTONS[row][col])
    }
}

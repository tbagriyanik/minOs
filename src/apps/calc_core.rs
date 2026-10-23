//! Hesap makinesinin çekirdekten bağımsız aritmetik çekirdeği.
//!
//! Bu modül yalnızca `core` kullanır; hem çekirdek içinde hem de host
//! testlerinde (`tests/host_feature_tests.rs`) aynı kod doğrulanır.

/// Sabit noktalı gösterimde altı ondalık basamak.
pub const SCALE: i128 = 1_000_000;
/// Girdi tamponunun kapasitesi (son bayt ayırıcı içindir).
pub const INPUT_CAP: usize = 64;

#[derive(Copy, Clone)]
pub struct CalcCore {
    pub input: [char; INPUT_CAP],
    pub len: usize,
}

impl CalcCore {
    pub const fn new() -> Self {
        Self { input: [' '; INPUT_CAP], len: 0 }
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
                // Bir operatorun hemen ardindan gelen `-` isaret degistirme
                // (unary) olarak korunur: "5*-2". Diger operatorler, ardisik
                // yazimlarda sonuncusu kazanacak sekilde degistirilir.
                if c != '-' {
                    self.input[self.len - 1] = c;
                    return;
                }
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

    pub fn set_expression(&mut self, expression: &str) {
        self.clear();
        for ch in expression.chars() { self.insert(ch); }
    }

    pub fn expression(&self) -> &[char] {
        &self.input[..self.len]
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

    pub fn is_operator(c: char) -> bool {
        matches!(c, '+' | '-' | '*' | '/')
    }

    /// Girdiyi soldan sağa, çarpma/bölme önceliğiyle değerlendirir.
    pub fn value(&self) -> i128 {
        let mut total = 0i128;
        let mut term = 0i128;
        let mut sign = 1i128;
        let mut op = '+';
        let mut number = 0i128;
        let mut number_sign = 1i128;
        let mut fraction_scale = 0i128;
        let mut has_digit = false;

        for index in 0..=self.len {
            let c = if index == self.len { '+' } else { self.input[index] };
            if c.is_ascii_digit() {
                let digit = c as i128 - '0' as i128;
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

    pub fn set_value(&mut self, value: i128) {
        self.len = 0;
        let mut output = [0u8; INPUT_CAP];
        let len = Self::format_value(value, &mut output);
        let len = len.min(self.input.len() - 1);
        for (index, byte) in output[..len].iter().enumerate() {
            self.input[index] = *byte as char;
            self.len = index + 1;
        }
    }

    /// Sabit noktalı değeri ondalık metne çevirir (sondaki sıfırlar atılır).
    pub fn format_value(value: i128, output: &mut [u8; INPUT_CAP]) -> usize {
        let mut len = 0;
        let magnitude = value.saturating_abs() as u128;
        if value < 0 { output[len] = b'-'; len += 1; }
        Self::append_unsigned(output, &mut len, magnitude / SCALE as u128);
        let mut fraction = (magnitude % SCALE as u128) as u32;
        if fraction != 0 {
            output[len] = b'.'; len += 1;
            let mut divisor = 100_000;
            while divisor > 0 {
                output[len] = b'0' + (fraction / divisor) as u8;
                len += 1;
                fraction %= divisor;
                divisor /= 10;
            }
            while len > 0 && output[len - 1] == b'0' { len -= 1; }
            if len > 0 && output[len - 1] == b'.' { len -= 1; }
        }
        len
    }

    fn append_unsigned(output: &mut [u8; INPUT_CAP], len: &mut usize, mut value: u128) {
        let mut digits = [0u8; 39];
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(expression: &str) -> alloc::string::String {
        let mut calc = CalcCore::new();
        calc.set_expression(expression);
        let mut out = [0u8; INPUT_CAP];
        let len = CalcCore::format_value(calc.value(), &mut out);
        alloc::string::String::from(core::str::from_utf8(&out[..len]).unwrap())
    }

    #[test]
    fn arithmetic_precedence_and_decimals() {
        assert_eq!(eval("2+3*4"), "14");
        assert_eq!(eval("10/4"), "2.5");
        assert_eq!(eval("1.5*2"), "3");
        assert_eq!(eval("-5+2"), "-3");
    }
}

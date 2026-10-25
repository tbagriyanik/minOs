//! Fixed-size no_std text buffer.

// ---------------------------------------------------------------------------
// Metin tamponu (no_std, sabit boyutlu) — sayı/tarih biçimlendirme için.
// ---------------------------------------------------------------------------
#[derive(Clone, Copy)]
pub struct TextBuf {
    buf: [u8; 128],
    len: usize,
}

impl TextBuf {
    pub const fn new() -> Self {
        Self { buf: [0; 128], len: 0 }
    }

    /// Sabit metinden hücreler arası (const) tampon oluşturur.
    /// Metin `buf`'a sığmazsa 128 baytta kırpılır.
    pub const fn from_str(s: &str) -> Self {
        let mut buf = [0u8; 128];
        let bytes = s.as_bytes();
        let n = if bytes.len() < 64 { bytes.len() } else { 64 };
        let mut i = 0;
        while i < n {
            buf[i] = bytes[i];
            i += 1;
        }
        Self { buf, len: n }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push_byte(&mut self, b: u8) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len > 0 {
            self.len -= 1;
            Some(self.buf[self.len])
        } else {
            None
        }
    }

    pub fn push_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.push_byte(b);
        }
    }

    /// İki haneli sayı (baştaki sıfır ile).
    pub fn push_u2(&mut self, v: u32) {
        self.push_byte(b'0' + ((v / 10) % 10) as u8);
        self.push_byte(b'0' + (v % 10) as u8);
    }

    /// Dört haneli yıl.
    pub fn push_u4(&mut self, v: u32) {
        self.push_byte(b'0' + ((v / 1000) % 10) as u8);
        self.push_byte(b'0' + ((v / 100) % 10) as u8);
        self.push_byte(b'0' + ((v / 10) % 10) as u8);
        self.push_byte(b'0' + (v % 10) as u8);
    }

    pub fn push_usize(&mut self, mut v: usize) {
        let mut tmp = [0u8; 20]; let mut n = 0;
        if v == 0 { self.push_byte(b'0'); return; }
        while v > 0 && n < tmp.len() { tmp[n] = b'0' + (v % 10) as u8; n += 1; v /= 10; }
        while n > 0 { n -= 1; self.push_byte(tmp[n]); }
    }

    pub fn push_hex64(&mut self, mut v: u64) {
        if v == 0 { self.push_byte(b'0'); return; }
        let mut tmp = [0u8; 16]; let mut n = 0;
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        while v > 0 && n < tmp.len() {
            tmp[n] = HEX[(v & 0xF) as usize];
            n += 1;
            v >>= 4;
        }
        while n > 0 { n -= 1; self.push_byte(tmp[n]); }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::TextBuf;

    #[test]
    fn keeps_complete_page_fault_diagnostics() {
        let mut text = TextBuf::new();
        text.push_str("KERNEL EXCEPTION: Vektor 14 (#PF - Sayfa Hatasi) | RIP: 0x");
        text.push_hex64(0xFFFF_FFFF_8000_1234);
        text.push_str(" | Err: 0x");
        text.push_hex64(0x02);
        text.push_str(" | CR2: 0x");
        text.push_hex64(0xFFFF_9000_0000_5678);

        assert!(text.as_str().ends_with("CR2: 0xFFFF900000005678"));
        assert!(text.as_str().contains("RIP: 0xFFFFFFFF80001234"));
    }
}

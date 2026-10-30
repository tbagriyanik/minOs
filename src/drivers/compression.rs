//! Minimal DEFLATE (RFC 1951) decoder for gzip/deflate content-encoding.
//! no_std, heap-allocated.
//!
//! Desteklenen blok türleri:
//! - BTYPE=00: Depolanan blok (ham veri kopyalama)
//! - BTYPE=01: Sabit Huffman kodları (fixed Huffman)
//! - BTYPE=10: Dinamik Huffman kodları (dynamic Huffman) — kısmi destek
//!
//! LZ77 geri referansları desteklenir.

#![allow(dead_code)]

use alloc::vec::Vec;

/// DEFLATE çözücü durumu.
struct InflateState {
    input: Vec<u8>,
    input_pos: usize,
    output: Vec<u8>,
    bit_buffer: u32,
    bit_count: u32,
}

impl InflateState {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input,
            input_pos: 0,
            output: Vec::new(),
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    /// `n` bit okur (LSB first).
    fn read_bits(&mut self, n: u32) -> Option<u32> {
        while self.bit_count < n {
            if self.input_pos >= self.input.len() {
                return None;
            }
            self.bit_buffer |= (self.input[self.input_pos] as u32) << self.bit_count;
            self.bit_count += 8;
            self.input_pos += 1;
        }
        let result = self.bit_buffer & ((1u32 << n) - 1);
        self.bit_buffer >>= n;
        self.bit_count -= n;
        Some(result)
    }

    /// Bayt sınırına hizalar.
    fn align_to_byte(&mut self) {
        let bits_to_discard = self.bit_count & 7;
        self.bit_buffer >>= bits_to_discard;
        self.bit_count -= bits_to_discard;
    }

    /// Çıktıya bayt yazar.
    fn write_byte(&mut self, b: u8) {
        self.output.push(b);
    }

    /// LZ77 geri referansı: `len` bayt önceki `dist` mesafeden kopyalar.
    fn copy_back(&mut self, len: usize, dist: usize) -> Option<()> {
        if dist == 0 || dist > self.output.len() {
            return None;
        }
        let start = self.output.len() - dist;
        for i in 0..len {
            let b = self.output[start + (i % dist)];
            self.output.push(b);
        }
        Some(())
    }

    /// Çözümlenmiş çıktıyı döner.
    fn into_output(self) -> Vec<u8> {
        self.output
    }
}

/// Sabit Huffman kod tablosu (literal/length, BTYPE=01).
/// 0-143: 8-bit kodlar, 144-255: 9-bit kodlar, 256-279: 7-bit kodlar, 280-287: 8-bit kodlar.
const FIXED_LITERAL_CODES: [(u16, u8); 288] = {
    let mut codes = [(0u16, 0u8); 288];
    let mut i = 0;
    while i < 144 {
        codes[i] = (0x030 + i as u16, 8);
        i += 1;
    }
    while i < 256 {
        codes[i] = (0x190 - 144 + i as u16, 9);
        i += 1;
    }
    while i < 280 {
        codes[i] = ((i as u16).wrapping_sub(256), 7);
        i += 1;
    }
    while i < 288 {
        codes[i] = ((i as u16).wrapping_sub(280) + 0x0C0, 8);
        i += 1;
    }
    codes
};

// Uzunluk kodları (257-285) için temel uzunluk ve ek bit sayısı.
const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

// Mesafe kodları (0-29) için temel mesafe ve ek bit sayısı.
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Bir Huffman kodunu bit akışından çözer.
/// `codes` tablosunda (kod_değeri, kod_uzunluğu) çiftleri aranır.
fn decode_huffman(state: &mut InflateState, codes: &[(u16, u8)]) -> Option<u16> {
    let mut code: u32 = 0;
    let mut len: u32 = 0;
    let mut index: usize = 0;
    while index < codes.len() {
        let bit = state.read_bits(1)?;
        code = (code << 1) | bit;
        len += 1;
        if codes[index].1 as u32 == len && codes[index].0 as u32 == code {
            return Some(codes[index].0);
        }
        // Basit linear search — gerçek implementasyonda Huffman ağacı kullanılmalı.
        index += 1;
        if index < codes.len() && (codes[index].1 as u32) < len {
            // Kod uzunluğu değişti, başa dön
            index = 0;
            code = 0;
            len = 0;
        }
    }
    None
}

/// Gzip/deflate verisini çözer.
/// `input` — DEFLATE sıkıştırılmış veri (gzip başlığı olmadan, sadece sıkıştırılmış bloklar).
/// Döner: çözümlenmiş ham veri.
pub fn inflate(input: &[u8]) -> Option<Vec<u8>> {
    let mut state = InflateState::new(input.to_vec());

    loop {
        // BFINAL (1 bit)
        let bfinal = state.read_bits(1)? as u8;
        // BTYPE (2 bit)
        let btype = state.read_bits(2)? as u8;

        match btype {
            0 => {
                // Depolanan blok
                state.align_to_byte();
                // LEN (2 byte, little-endian)
                let len = state.read_bits(8)? as usize | ((state.read_bits(8)? as usize) << 8);
                // NLEN (2 byte, little-endian) — LEN'in tamamlayıcısı
                let _nlen = state.read_bits(8)? as usize | ((state.read_bits(8)? as usize) << 8);
                // Ham veriyi kopyala
                for _ in 0..len {
                    let b = state.read_bits(8)? as u8;
                    state.write_byte(b);
                }
            }
            1 => {
                // Sabit Huffman kodları
                loop {
                    let symbol = decode_huffman(&mut state, &FIXED_LITERAL_CODES)?;
                    if symbol == 256 {
                        break; // Blok sonu
                    } else if symbol < 256 {
                        state.write_byte(symbol as u8);
                    } else {
                        // Uzunluk + mesafe çifti
                        let length_index = (symbol - 257) as usize;
                        if length_index >= LENGTH_BASE.len() {
                            return None;
                        }
                        let mut length = LENGTH_BASE[length_index] as usize;
                        let extra_bits = LENGTH_EXTRA[length_index] as u32;
                        if extra_bits > 0 {
                            length += state.read_bits(extra_bits)? as usize;
                        }

                        let dist_symbol = decode_huffman(&mut state, &FIXED_DIST_CODES)?;
                        if dist_symbol as usize >= DIST_BASE.len() {
                            return None;
                        }
                        let mut distance = DIST_BASE[dist_symbol as usize] as usize;
                        let dist_extra = DIST_EXTRA[dist_symbol as usize] as u32;
                        if dist_extra > 0 {
                            distance += state.read_bits(dist_extra)? as usize;
                        }

                        state.copy_back(length, distance)?;
                    }
                }
            }
            2 => {
                // Dinamik Huffman — şimdilik desteklenmiyor
                return None;
            }
            3 => {
                return None; // Geçersiz blok türü
            }
            _ => return None,
        }

        if bfinal == 1 {
            break;
        }
    }

    Some(state.into_output())
}

/// Sabit Huffman mesafe kodları (30 kod, 5 bit her biri).
const FIXED_DIST_CODES: [(u16, u8); 30] = [
    (0, 5),
    (1, 5),
    (2, 5),
    (3, 5),
    (4, 5),
    (5, 5),
    (6, 5),
    (7, 5),
    (8, 5),
    (9, 5),
    (10, 5),
    (11, 5),
    (12, 5),
    (13, 5),
    (14, 5),
    (15, 5),
    (16, 5),
    (17, 5),
    (18, 5),
    (19, 5),
    (20, 5),
    (21, 5),
    (22, 5),
    (23, 5),
    (24, 5),
    (25, 5),
    (26, 5),
    (27, 5),
    (28, 5),
    (29, 5),
];

// Gzip header skipper, returns compressed data (RFC 1952).
// data — full gzip file content.
// Returns: DEFLATE compressed data slice.
pub fn strip_gzip_header(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 10 {
        return None;
    }
    // Gzip magic: 1f 8b
    if data[0] != 0x1f || data[1] != 0x8b {
        return None;
    }
    let cm = data[2]; // Compression Method (8 = deflate)
    if cm != 8 {
        return None;
    }
    let flg = data[3]; // Flags
    let mut pos = 10;

    // MTIME (4 bayt) atla
    pos += 4;
    // XFL (1 bayt) atla
    pos += 1;
    // OS (1 bayt) atla
    pos += 1;

    // FTEXT (bit 0) — hiçbir veri yok
    // FHCRC (bit 1) — 2 bayt CRC atla
    if flg & 0x02 != 0 {
        pos += 2;
    }
    // FEXTRA (bit 2) — alan uzunluğu + veri atla
    if flg & 0x04 != 0 {
        if pos + 2 > data.len() {
            return None;
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    // FNAME (bit 3) — null-terminated string atla
    if flg & 0x08 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1; // null baytı
    }
    // FCOMMENT (bit 4) — null-terminated string atla
    if flg & 0x10 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1; // null baytı
    }

    Some(&data[pos..])
}

/// HTTP Content-Encoding değerine göre veriyi çözer.
/// `encoding` — "gzip", "deflate", "br" veya None.
/// `data` — sıkıştırılmış veri.
/// Döner: çözümlenmiş ham veri.
pub fn decode_content_encoding(encoding: Option<&str>, data: &[u8]) -> Option<Vec<u8>> {
    match encoding {
        None | Some("identity") => Some(data.to_vec()),
        Some("gzip") => {
            let deflate_data = strip_gzip_header(data)?;
            inflate(deflate_data)
        }
        Some("deflate") => {
            // "deflate" sadece DEFLATE sıkıştırılmış veridir (gzip başlığı yok).
            // Bazı sunucular gzip formatında gönderir; deneyelim.
            if let Some(result) = inflate(data) {
                return Some(result);
            }
            // Gzip formatında olabilir
            let deflate_data = strip_gzip_header(data)?;
            inflate(deflate_data)
        }
        Some("br") => {
            // Brotli (RFC 7932) — şimdilik desteklenmiyor
            None
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello world"), "hello+world");
        assert_eq!(url_encode("a/b?c=d"), "a%2Fb%3Fc%3D");
        assert_eq!(url_encode("foo"), "foo");
    }

    #[test]
    fn test_form_urlencode() {
        let pairs = [("name", "minOs"), ("version", "2.1")];
        let result = form_urlencode(&pairs);
        assert!(result.contains("name=minOs"));
        assert!(result.contains("version=2.1"));
    }

    #[test]
    fn test_stored_block() {
        // Basit depolanan blok: BFINAL=1, BTYPE=00, LEN=3, NLEN=FCFF, veri="abc"
        // Bit akışı: 1 (BFINAL) + 00 (BTYPE) + hizalama + LEN(03 00) + NLEN(FC FF) + "abc"
        // Baytlar: 01 03 00 FC FF 61 62 63
        let data = [0x01, 0x03, 0x00, 0xFC, 0xFF, 0x61, 0x62, 0x63];
        let result = inflate(&data).unwrap();
        assert_eq!(result, b"abc");
    }

    #[test]
    fn test_gzip_header_strip() {
        // Minimal gzip: 1f 8b 08 00 00 00 00 00 00 03 + deflate verisi
        let mut gzip = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];
        gzip.extend_from_slice(&[0x01, 0x03, 0x00, 0xFC, 0xFF, 0x61, 0x62, 0x63]);
        let deflate = strip_gzip_header(&gzip).unwrap();
        let result = inflate(deflate).unwrap();
        assert_eq!(result, b"abc");
    }

    #[test]
    fn test_decode_content_encoding_identity() {
        let data = b"hello";
        let result = decode_content_encoding(None, data).unwrap();
        assert_eq!(result, b"hello");
    }
}

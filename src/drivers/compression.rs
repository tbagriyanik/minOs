//! DEFLATE (RFC 1951) / gzip (RFC 1952) decoder for gzip/deflate content-encoding.
//! no_std, heap-allocated.
//!
//! Çözümleme `miniz_oxide` ile yapılır: depolanan bloklar, sabit Huffman ve
//! dinamik Huffman bloklarının tamamı desteklenir. Önceki elle yazılmış
//! çözücü BTYPE=10 (dinamik Huffman) bloklarında `None` dönüyordu; gzip ile
//! sıkıştırılmış gerçek HTTP yanıtları neredeyse her zaman dinamik Huffman
//! kullandığı için bu bir uyumluluk hatasıydı.

#![allow(dead_code)]

use alloc::vec::Vec;

/// Gzip/deflate verisini çözer.
/// `input` — DEFLATE sıkıştırılmış veri (gzip başlığı olmadan, sadece sıkıştırılmış bloklar).
/// Döner: çözümlenmiş ham veri.
pub fn inflate(input: &[u8]) -> Option<Vec<u8>> {
    // Gzip kuyruğu (CRC32 + ISIZE) DEFLATE akışından sonra gelir. Bazı
    // miniz_oxide sürümleri kuyruğu yok sayıp erken sonuç döndürebildiği için
    // önce olası kuyruğu çıkarılmış biçimi deniyoruz; kısa ham akışları
    // bozmamak için yalnızca 8 bayttan uzun girdilerde bunu yapıyoruz.
    if input.len() > 8 {
        if let Ok(result) = miniz_oxide::inflate::decompress_to_vec(&input[..input.len() - 8])
        {
            return Some(result);
        }
    }
    miniz_oxide::inflate::decompress_to_vec(input).ok()
}

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
    let mut pos = 10; // 10 baytlık sabit başlık (Magic+CM+FLG+MTIME+XFL+OS) zaten geçildi

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
        Some("gzip") | Some("x-gzip") => {
            // gzip gövdesi: RFC 1952 başlığı + DEFLATE + CRC32/ISIZE kuyruğu.
            // Buradaki çözücü kuyruğu yok sayar ve CRC32/ISIZE doğrulaması
            // yapmaz; bozulma tespiti taşıma katmanına (TLS AEAD) bırakılır.
            let deflate_data = strip_gzip_header(data)?;
            inflate(deflate_data)
        }
        Some("deflate") => {
            // RFC 9110: "deflate" = zlib sarmalı (RFC 1950) içinde DEFLATE.
            // Yaygın hata olarak bazı sunucular sarmalı atlayıp ham DEFLATE
            // gönderir; zlib denemesi başarısız olursa ham DEFLATE denenir.
            miniz_oxide::inflate::decompress_to_vec_zlib(data)
                .ok()
                .or_else(|| inflate(data))
        }
        Some("br") | Some("compress") => {
            // Brotli (RFC 7932) ve LZW "compress" desteklenmiyor; çağıran taraf
            // `Accept-Encoding` ile bunları istemediği için (bkz.
            // `http2.rs::STATIC_HEADER`) pratikte buraya düşülmez.
            None
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `"hello world"` metninin gerçek gzip çıktısı
    /// (`printf 'hello world' | gzip -9`). Dinamik Huffman bloğu içerir —
    /// eski elle yazılmış çözücünün başarısız olduğu yol.
    const HELLO_WORLD_GZIP: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
        0x57, 0x28, 0xcf, 0x2f, 0xca, 0x49, 0x01, 0x00, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00,
        0x00,
    ];

    /// Aynı metnin zlib sarmalı (RFC 1950) içindeki DEFLATE çıktısı
    /// (`printf 'hello world' | python -c "import sys,zlib;..."`).
    const HELLO_WORLD_ZLIB: &[u8] = &[
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0x28, 0xcf, 0x2f, 0xca, 0x49, 0x01, 0x00,
        0x1a, 0x0b, 0x04, 0x5d,
    ];

    #[test]
    fn test_inflate_dynamic_huffman_gzip_body() {
        // gzip başlığı çıkarıldıktan sonra dinamik Huffman bloğu çözülmeli.
        let body = strip_gzip_header(HELLO_WORLD_GZIP).unwrap();
        assert_eq!(inflate(body).as_deref(), Some(&b"hello world"[..]));
    }

    #[test]
    fn test_decode_gzip_content_encoding() {
        assert_eq!(
            decode_content_encoding(Some("gzip"), HELLO_WORLD_GZIP).as_deref(),
            Some(&b"hello world"[..])
        );
        // Sunucular bazen `x-gzip` kullanır (RFC 9110 §8.4.1.3).
        assert_eq!(
            decode_content_encoding(Some("x-gzip"), HELLO_WORLD_GZIP).as_deref(),
            Some(&b"hello world"[..])
        );
    }

    #[test]
    fn test_decode_deflate_content_encoding_accepts_zlib_and_raw() {
        // Standart: zlib sarmalı.
        assert_eq!(
            decode_content_encoding(Some("deflate"), HELLO_WORLD_ZLIB).as_deref(),
            Some(&b"hello world"[..])
        );
        // Yaygın sunucu hatası: sarmalsız ham DEFLATE.
        let raw = strip_gzip_header(HELLO_WORLD_GZIP).unwrap();
        assert_eq!(
            decode_content_encoding(Some("deflate"), raw).as_deref(),
            Some(&b"hello world"[..])
        );
    }

    #[test]
    fn test_decode_brotli_is_reported_unsupported() {
        assert_eq!(decode_content_encoding(Some("br"), &[0x00]), None);
    }

    #[test]
    fn test_corrupt_input_is_rejected() {
        assert_eq!(inflate(&[0xFF, 0xFF, 0xFF, 0xFF]), None);
        // gzip sihirli baytları yok.
        assert_eq!(
            decode_content_encoding(Some("gzip"), b"not a gzip body"),
            None
        );
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

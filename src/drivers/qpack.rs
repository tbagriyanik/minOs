//! QPACK (RFC 9204) codec - static table only.

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::huffman::decode as huffman_decode_pub;

// QPACK statik tablo (RFC 9204 Ek A) — kullanılan alt küme + sık başlıklar
// ---------------------------------------------------------------------------

/// (ad, değer) — indeks 0..=72. Yalnızca kesin bilinen girdiler dolduruldu;
/// bilinmeyenler boş bırakılır (çözme tarafında literal fallback yok —
/// indeksli alanlar yalnızca bilinen indekslerde kullanılır).
const STATIC_TABLE: &[(&str, &str)] = &[
    (":authority", ""),                                    // 0
    (":path", "/"),                                        // 1
    ("age", "0"),                                          // 2
    ("content-disposition", ""),                           // 3
    ("content-length", "0"),                               // 4
    ("cookie", ""),                                        // 5
    ("date", ""),                                          // 6
    ("etag", ""),                                          // 7
    ("if-modified-since", ""),                             // 8
    ("if-none-match", ""),                                 // 9
    ("last-modified", ""),                                 // 10
    ("link", ""),                                          // 11
    ("location", ""),                                      // 12
    ("referer", ""),                                       // 13
    ("set-cookie", ""),                                    // 14
    (":method", "CONNECT"),                                // 15
    (":method", "DELETE"),                                 // 16
    (":method", "GET"),                                    // 17
    (":method", "HEAD"),                                   // 18
    (":method", "OPTIONS"),                                // 19
    (":method", "POST"),                                   // 20
    (":method", "PUT"),                                    // 21
    (":scheme", "http"),                                   // 22
    (":scheme", "https"),                                  // 23
    (":status", "103"),                                    // 24
    (":status", "200"),                                    // 25
    (":status", "304"),                                    // 26
    (":status", "404"),                                    // 27
    (":status", "503"),                                    // 28
    ("accept", "*/*"),                                     // 29
    ("accept", "application/dns-message"),                 // 30
    ("accept-encoding", "gzip, deflate, br"),              // 31
    ("accept-ranges", "bytes"),                            // 32
    ("access-control-allow-headers", "cache-control"),     // 33
    ("access-control-allow-headers", "content-type"),      // 34
    ("access-control-allow-origin", "*"),                  // 35
    ("cache-control", "max-age=0"),                        // 36
    ("cache-control", "max-age=2592000"),                  // 37
    ("cache-control", "max-age=604800"),                   // 38
    ("cache-control", "no-cache"),                         // 39
    ("cache-control", "no-store"),                         // 40
    ("cache-control", "public, max-age=31536000"),         // 41
    ("content-encoding", "br"),                            // 42
    ("content-encoding", "gzip"),                          // 43
    ("content-type", "application/dns-message"),           // 44
    ("content-type", "application/javascript"),            // 45
    ("content-type", "application/json"),                  // 46
    ("content-type", "application/x-www-form-urlencoded"), // 47
    ("content-type", "image/gif"),                         // 48
    ("content-type", "image/jpeg"),                        // 49
    ("content-type", "image/png"),                         // 50
    ("content-type", "text/css"),                          // 51
    ("content-type", "text/html; charset=utf-8"),          // 52
    ("content-type", "text/plain"),                        // 53
    ("content-type", "text/plain;charset=utf-8"),          // 54
    ("range", "bytes=0-"),                                 // 55
    ("strict-transport-security", "max-age=31536000"),     // 56
    (
        "strict-transport-security",
        "max-age=31536000; includesubdomains",
    ), // 57
    (
        "strict-transport-security",
        "max-age=31536000; includesubdomains; preload",
    ), // 58
    ("vary", "accept-encoding"),                           // 59
    ("vary", "origin"),                                    // 60
    ("x-content-type-options", "nosniff"),                 // 61
    ("x-xss-protection", "1; mode=block"),                 // 62
    (":status", "100"),                                    // 63
    (":status", "204"),                                    // 64
    (":status", "206"),                                    // 65
    (":status", "302"),                                    // 66
    (":status", "400"),                                    // 67
    (":status", "403"),                                    // 68
    (":status", "421"),                                    // 69
    (":status", "425"),                                    // 70
    (":status", "500"),                                    // 71
    ("accept-language", ""),                               // 72
];

// ---------------------------------------------------------------------------
// QPACK tam sayı kodlama (HPACK ile aynı önekli tam sayı)
// ---------------------------------------------------------------------------

fn encode_int(value: u64, prefix_bits: u8, first_mask: u8, out: &mut Vec<u8>) {
    let max = (1u64 << prefix_bits) - 1;
    if value < max {
        out.push(first_mask | value as u8);
    } else {
        out.push(first_mask | max as u8);
        let mut v = value - max;
        while v >= 0x80 {
            out.push((v as u8 & 0x7f) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
}

fn decode_int(data: &[u8], pos: &mut usize, prefix_bits: u8) -> Option<u64> {
    let max = (1u64 << prefix_bits) - 1;
    let b = *data.get(*pos)?;
    *pos += 1;
    let prefix = (b & ((1 << prefix_bits) - 1)) as u64;
    if prefix < max {
        return Some(prefix);
    }
    let mut value = max;
    let mut shift = 0u32;
    loop {
        let c = *data.get(*pos)?;
        *pos += 1;
        value += ((c & 0x7f) as u64) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            break;
        }
        if shift > 63 {
            return None;
        }
    }
    Some(value)
}

/// Dizge literali: H baytı + uzunluk + baytlar. `huffman` bayrağı
/// desteklenir (çözme tarafında HPACK Huffman'ı kullanılır).
fn encode_string(s: &str, out: &mut Vec<u8>) {
    // H=0 (Huffman yok), 7-bit uzunluk öneki
    let bytes = s.as_bytes();
    if bytes.len() < 127 {
        out.push(bytes.len() as u8);
    } else {
        out.push(0x7f);
        let mut v = bytes.len() as u64 - 127;
        while v >= 0x80 {
            out.push((v as u8 & 0x7f) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    out.extend_from_slice(bytes);
}

fn decode_string(data: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let b = *data.get(*pos)?;
    let huffman = b & 0x80 != 0;
    *pos += 1;
    let mut len = (b & 0x7f) as u64;
    if len == 127 {
        // uzunluk devamı
        let mut shift = 0u32;
        loop {
            let c = *data.get(*pos)?;
            *pos += 1;
            len += ((c & 0x7f) as u64) << shift;
            shift += 7;
            if c & 0x80 == 0 {
                break;
            }
        }
    }
    let n = len as usize;
    if *pos + n > data.len() {
        return None;
    }
    let raw = &data[*pos..*pos + n];
    *pos += n;
    if huffman {
        huffman_decode_pub(raw)
    } else {
        Some(raw.to_vec())
    }
}

// ---------------------------------------------------------------------------
// QPACK kodlayıcı (yalnızca statik tablo + literal alanlar)
// ---------------------------------------------------------------------------

/// `(ad, değer)` başlık listesini QPACK blokuna kodlar.
/// Basit strateji: ad+değer çifti statik tabloda birebir varsa indeksli
/// alan satırı; ad statik tabloda varsa isim referanslı literal; değilse
/// literal isimli alan satırı.
pub fn qpack_encode(headers: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in headers {
        // 1) Tam eşleşme (ad+değer) → indeksli alan satırı
        let mut indexed = None;
        for (i, (n, v)) in STATIC_TABLE.iter().enumerate() {
            if *n == name.as_str() && *v == value.as_str() {
                indexed = Some(i as u64);
                break;
            }
        }
        if let Some(idx) = indexed {
            // Indexed Field Line: '1' T=1(static) index(6+) → ilk bayt 0xC0
            encode_int(idx, 6, 0xc0, &mut out);
            continue;
        }
        // 2) Ad statik tabloda → Literal Field Line With Name Reference
        let mut name_idx = None;
        for (i, (n, _)) in STATIC_TABLE.iter().enumerate() {
            if *n == name.as_str() {
                name_idx = Some(i as u64);
                break;
            }
        }
        if let Some(idx) = name_idx {
            // '01' N=0 T=1 index(4+) — ilk bayt 0x50 | index(4-bit)
            encode_int(idx, 4, 0x50, &mut out);
            encode_string(value, &mut out);
            continue;
        }
        // 3) Literal Field Line With Literal Name: '001' N=0 H=0 name(3+)
        // encode_int ilk baytı '001' + 3-bit önek olarak yazar.
        encode_int(name.len() as u64, 3, 0x20, &mut out);
        out.extend_from_slice(name.as_bytes());
        encode_string(value, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// QPACK çözücü
// ---------------------------------------------------------------------------

/// QPACK blok çözücü (statik tablo + literal; dinamik tablo yok).
pub struct QpackDecoder {
    /// Dinamik tablo kullanmıyoruz; yine de gelecek ekleme komutlarını
    /// atlamak için minimum izleme.
    _reserved: (),
}

impl Default for QpackDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl QpackDecoder {
    pub fn new() -> Self {
        Self { _reserved: () }
    }

    /// QPACK blokunu `(ad, değer)` listesine çözer.
    /// Desteklenen: Indexed Field Line, Literal With Name Reference,
    /// Literal With Literal Name. Dinamik tablo komutları (enc/dec stream)
    /// blok içinde görünmez; burada yalnızca alan satırları beklenir.
    pub fn decode_block(&mut self, data: &[u8]) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos < data.len() {
            let b = data[pos];
            pos += 1;
            if b & 0x80 != 0 {
                // Indexed Field Line: '1' T index(6+)
                pos -= 1;
                let idx = decode_int(data, &mut pos, 6).ok_or_else(|| String::from("qpack_int"))?;
                let (n, v) = self.static_entry(idx)?;
                out.push((String::from(n), String::from(v)));
            } else if b & 0x40 != 0 {
                // Literal Field Line With Name Reference: '01' N T index(4+)
                // İlk bayt zaten okundu; önek decode_int'in ilk baytı
                // tekrar okuması için pos geri alınır.
                pos -= 1;
                let idx = decode_int(data, &mut pos, 4).ok_or_else(|| String::from("qpack_int"))?;
                let (n, _) = self.static_entry(idx)?;
                let value =
                    decode_string(data, &mut pos).ok_or_else(|| String::from("qpack_str"))?;
                out.push((
                    String::from(n),
                    String::from_utf8_lossy(&value).into_owned(),
                ));
            } else if b & 0x20 != 0 {
                // Literal Field Line With Literal Name: '001' N H name(3+)
                pos -= 1;
                let name_len = decode_int(data, &mut pos, 3)
                    .ok_or_else(|| String::from("qpack_int"))?
                    as usize;
                if pos + name_len > data.len() {
                    return Err(String::from("qpack_name"));
                }
                let name = String::from_utf8_lossy(&data[pos..pos + name_len]).into_owned();
                pos += name_len;
                let value =
                    decode_string(data, &mut pos).ok_or_else(|| String::from("qpack_str"))?;
                out.push((name, String::from_utf8_lossy(&value).into_owned()));
            } else {
                return Err(String::from("qpack_unknown_repr"));
            }
        }
        Ok(out)
    }

    fn static_entry(&self, idx: u64) -> Result<(&'static str, &'static str), String> {
        STATIC_TABLE
            .get(idx as usize)
            .copied()
            .ok_or_else(|| String::from("qpack_static_idx"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn hdr(n: &str, v: &str) -> (String, String) {
        (n.to_string(), v.to_string())
    }

    #[test]
    fn qpack_static_indexed_roundtrip() {
        // :method GET (17), :scheme https (23), :status 200 (25) tam eşleşme.
        let headers = alloc::vec![
            hdr(":method", "GET"),
            hdr(":scheme", "https"),
            hdr(":status", "200"),
        ];
        let block = qpack_encode(&headers);
        let mut dec = QpackDecoder::new();
        let out = dec.decode_block(&block).unwrap();
        assert_eq!(out, headers);
    }

    #[test]
    fn qpack_name_ref_literal() {
        // :path /foo — ad statik tabloda (1), değer literal.
        let headers = alloc::vec![hdr(":path", "/foo/bar")];
        let block = qpack_encode(&headers);
        let mut dec = QpackDecoder::new();
        let out = dec.decode_block(&block).unwrap();
        assert_eq!(out, headers);
    }

    #[test]
    fn qpack_literal_name() {
        // Bilinmeyen başlık — literal isimli alan satırı.
        let headers = alloc::vec![hdr("x-custom-header", "değer-ğüşİ")];
        let block = qpack_encode(&headers);
        let mut dec = QpackDecoder::new();
        let out = dec.decode_block(&block).unwrap();
        assert_eq!(out, headers);
    }

    #[test]
    fn qpack_mixed_roundtrip() {
        let headers = alloc::vec![
            hdr(":method", "POST"),
            hdr(":scheme", "https"),
            hdr(":authority", "ornek.com"),
            hdr(":path", "/api/v1"),
            hdr("content-type", "application/json"),
            hdr("accept", "text/html"),
            hdr("user-agent", "minOs/2.5"),
        ];
        let block = qpack_encode(&headers);
        let mut dec = QpackDecoder::new();
        let out = dec.decode_block(&block).unwrap();
        assert_eq!(out.len(), headers.len());
        for (a, b) in out.iter().zip(headers.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn qpack_long_index_prefix() {
        // İndeks >= 63 durumunda 6-bit önek devamı: statik tablo 63+ yok
        // ama name-ref 4-bit önekte >15 indeks testi.
        // :status 500 (71) tam eşleşme → indeksli alan, 6-bit önek devamı.
        let headers = alloc::vec![hdr(":status", "500")];
        let block = qpack_encode(&headers);
        let mut dec = QpackDecoder::new();
        let out = dec.decode_block(&block).unwrap();
        assert_eq!(out, headers);
    }

    #[test]
    fn huffman_decode_known_vector() {
        // RFC 7541 Ek C.4.1: "www.example.com" Huffman kodu (12 bayt).
        let encoded: &[u8] = &[
            0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff,
        ];
        let out = huffman_decode_pub(encoded).unwrap();
        assert_eq!(out, b"www.example.com");
    }
}

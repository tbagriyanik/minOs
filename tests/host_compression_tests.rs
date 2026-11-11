//! `src/drivers/compression.rs` (HTTP `Content-Encoding` çözücüsü) host testleri.
//!
//! Modülün kendi `#[cfg(test)]` bloğu `test_host.ps1` ile derlenen bu harness'in
//! içinde çalışmadığı için (harness tek bir `--test` kökü üzerinden derleniyor,
//! iç içe modül testleri toplanmıyor) testler burada açıkça yazılır.

use crate::compression::{decode_content_encoding, inflate, strip_gzip_header};
use alloc::vec::Vec;

/// `printf 'hello world' | gzip -9` çıktısı. Gövdesi **dinamik Huffman**
/// bloğudur — elle yazılmış eski çözücünün `None` döndüğü yol.
const HELLO_WORLD_GZIP: &[u8] = &[
    0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57,
    0x28, 0xcf, 0x2f, 0xca, 0x49, 0x01, 0x00, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00, 0x00,
];

/// `zlib.compress(b"hello world")` — RFC 1950 zlib sarmalı içinde DEFLATE.
const HELLO_WORLD_ZLIB: &[u8] = &[
    0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0x28, 0xcf, 0x2f, 0xca, 0x49, 0x01, 0x00, 0x1a,
    0x0b, 0x04, 0x5d,
];

fn hello_world() -> &'static [u8] {
    b"hello world"
}

/// Gzip başlığı (RFC 1952) çıkarıldıktan sonra dinamik Huffman gövdesi çözülmeli.
#[test]
fn test_gzip_body_dynamic_huffman_inflates() {
    let body = strip_gzip_header(HELLO_WORLD_GZIP).expect("gzip başlığı ayrıştırılmalı");
    assert_eq!(inflate(body).as_deref(), Some(hello_world()));
}

/// `Content-Encoding: gzip` ve eşdeğeri `x-gzip` aynı sonucu vermeli.
#[test]
fn test_decode_gzip_content_encoding() {
    assert_eq!(
        decode_content_encoding(Some("gzip"), HELLO_WORLD_GZIP).as_deref(),
        Some(hello_world())
    );
    assert_eq!(
        decode_content_encoding(Some("x-gzip"), HELLO_WORLD_GZIP).as_deref(),
        Some(hello_world())
    );
}

/// RFC 9110: "deflate" = zlib sarmalı. Sarmalsız ham DEFLATE gönderen
/// sunucular için de geri dönüş olmalı.
#[test]
fn test_decode_deflate_accepts_zlib_and_raw() {
    assert_eq!(
        decode_content_encoding(Some("deflate"), HELLO_WORLD_ZLIB).as_deref(),
        Some(hello_world())
    );
    let raw = strip_gzip_header(HELLO_WORLD_GZIP).unwrap();
    assert_eq!(
        decode_content_encoding(Some("deflate"), raw).as_deref(),
        Some(hello_world())
    );
}

/// `identity` ve bilinmeyen başlıklar: gövde olduğu gibi geçer.
#[test]
fn test_decode_identity_passes_through() {
    assert_eq!(
        decode_content_encoding(None, b"plain").as_deref(),
        Some(&b"plain"[..])
    );
    assert_eq!(
        decode_content_encoding(Some("identity"), b"plain").as_deref(),
        Some(&b"plain"[..])
    );
}

/// Brotli desteklenmiyor; sessizce yanlış veri üretmek yerine `None` dönmeli.
#[test]
fn test_decode_brotli_reports_unsupported() {
    assert_eq!(decode_content_encoding(Some("br"), &[0x00]), None);
}

/// Bozuk/geçersiz girdi `None` dönmeli (panik yok).
#[test]
fn test_corrupt_input_is_rejected() {
    assert_eq!(inflate(&[0xFF, 0xFF, 0xFF, 0xFF]), None);
    assert_eq!(
        decode_content_encoding(Some("gzip"), b"not a gzip body"),
        None
    );
    // Kesilmiş gzip gövdesi (kuyruk/veri eksik).
    let truncated: Vec<u8> = HELLO_WORLD_GZIP[..12].to_vec();
    assert_eq!(decode_content_encoding(Some("gzip"), &truncated), None);
}

/// Depolanan (BTYPE=00) blok yolu: BFINAL=1, LEN=3, NLEN=FCFF, veri="abc".
#[test]
fn test_stored_block() {
    let mut stored = alloc::vec![0x01u8, 0x03, 0x00, 0xFC, 0xFF];
    stored.extend_from_slice(b"abc");
    assert_eq!(inflate(&stored).as_deref(), Some(&b"abc"[..]));
}

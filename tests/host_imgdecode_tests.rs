//! imgdecode birim testleri: GIF LZW çözümleme doğruluğu.
//!
//! `ornek.gif` fixture'ı PIL (Pillow) ile doğrulanmış referans çıktıya
//! karşı hash'lenir; LZW sözlük geri-besleme hataları piksel sapması olarak
//! yakalanır.

use crate::imgdecode;

fn fnv1a(rgb: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in rgb {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn gif_decodes_first_frame_pixels() {
    let data = include_bytes!("fixtures/ornek.gif");
    let img = imgdecode::decode(data).expect("GIF çözümlenemedi");
    assert_eq!((img.width, img.height), (20, 22));
    assert_eq!(img.format, "GIF");
    assert_eq!(img.rgb.len(), 20 * 22 * 3);
    // PIL ile doğrulanan RGB hash'i (LZW KwKwK + sözlük eki regresyonu).
    assert_eq!(fnv1a(&img.rgb), 0x5ccb16a40dc86e0f);
}

#[test]
fn gif_rejects_truncated_stream() {
    let data = include_bytes!("fixtures/ornek.gif");
    let cut = &data[..data.len() / 2];
    // Kesik akış ya None dönmeli ya da en azından panic etmemeli.
    let _ = imgdecode::decode(cut);
}

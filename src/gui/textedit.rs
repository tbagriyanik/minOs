//! UTF-8 guvenli sabit tampon duzenleme yardimcilari.
//!
//! Dosya adi duzenleme gibi islemler bayt tamponu uzerinde calisir. Cok baytli
//! karakterlerin (Turkce harfler dahil) ortasindan kesilmemesi icin imlec her
//! zaman bir karakter sinirinda tutulur. Modul yalnizca `core` kullanir ve host
//! testlerinde dogrudan sinanir.

/// `b` bir UTF-8 devam bayti mi?
#[inline(always)]
pub const fn is_continuation(b: u8) -> bool {
    (b & 0xC0) == 0x80
}

/// `pos` konumundan bir onceki karakterin baslangic baytini dondurur.
pub fn prev_char_boundary(buf: &[u8], pos: usize) -> usize {
    let pos = pos.min(buf.len());
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    while i > 0 && is_continuation(buf[i]) {
        i -= 1;
    }
    i
}

/// `pos` konumundan sonraki karakterin bitis baytini dondurur.
pub fn next_char_boundary(buf: &[u8], pos: usize, len: usize) -> usize {
    let len = len.min(buf.len());
    if pos >= len {
        return len;
    }
    let mut i = pos + 1;
    while i < len && is_continuation(buf[i]) {
        i += 1;
    }
    i
}

/// `limit` bir karakter sinirina denk gelecek sekilde kirpilmis uzunluk.
pub fn truncate_to_boundary(buf: &[u8], limit: usize) -> usize {
    let mut take = limit.min(buf.len());
    while take > 0 && take < buf.len() && is_continuation(buf[take]) {
        take -= 1;
    }
    take
}

/// Karakteri imlece ekler. Tampon doluysa `false` dondurur.
pub fn insert_char(buf: &mut [u8], len: &mut usize, cursor: &mut usize, c: char) -> bool {
    if *len > buf.len() || *cursor > *len {
        return false;
    }
    let mut encoded = [0u8; 4];
    let bytes = c.encode_utf8(&mut encoded).as_bytes();
    if *len + bytes.len() > buf.len() {
        return false;
    }
    buf.copy_within(*cursor..*len, *cursor + bytes.len());
    buf[*cursor..*cursor + bytes.len()].copy_from_slice(bytes);
    *cursor += bytes.len();
    *len += bytes.len();
    true
}

/// Imlecten onceki karakteri siler.
pub fn backspace(buf: &mut [u8], len: &mut usize, cursor: &mut usize) -> bool {
    if *cursor == 0 || *cursor > *len {
        return false;
    }
    let start = prev_char_boundary(&buf[..*len], *cursor);
    let removed = *cursor - start;
    buf.copy_within(*cursor..*len, start);
    *len -= removed;
    *cursor = start;
    true
}

/// Imlecten sonraki karakteri siler.
pub fn delete_forward(buf: &mut [u8], len: &mut usize, cursor: &mut usize) -> bool {
    if *cursor >= *len {
        return false;
    }
    let end = next_char_boundary(&buf[..*len], *cursor, *len);
    let removed = end - *cursor;
    buf.copy_within(end..*len, *cursor);
    *len -= removed;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_keep_multibyte_chars_whole() {
        // "ağı" -> a(1) + g-breve(2) + i-dotless(2)
        let text = "a\u{011F}\u{0131}";
        let bytes = text.as_bytes();
        assert_eq!(bytes.len(), 5);
        assert_eq!(prev_char_boundary(bytes, 5), 3);
        assert_eq!(prev_char_boundary(bytes, 3), 1);
        assert_eq!(prev_char_boundary(bytes, 1), 0);
        assert_eq!(next_char_boundary(bytes, 0, 5), 1);
        assert_eq!(next_char_boundary(bytes, 1, 5), 3);
        assert_eq!(next_char_boundary(bytes, 3, 5), 5);
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let text = "Kitapl\u{0131}k";
        let bytes = text.as_bytes();
        assert_eq!(truncate_to_boundary(bytes, 5), 5); // "Kitap"
        assert_eq!(truncate_to_boundary(bytes, 7), 6); // before the 2-byte char
        assert_eq!(truncate_to_boundary(bytes, 8), 8);
    }

    #[test]
    fn editing_round_trips_utf8() {
        let mut buf = [0u8; 16];
        let start = "a\u{011F}\u{0131}";
        buf[..start.len()].copy_from_slice(start.as_bytes());
        let mut len = start.len();
        let mut cursor = len;

        assert!(backspace(&mut buf, &mut len, &mut cursor));
        assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "a\u{011F}");
        assert_eq!(cursor, 3);

        assert!(insert_char(&mut buf, &mut len, &mut cursor, '\u{0131}'));
        assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "a\u{011F}\u{0131}");

        cursor = 0;
        assert!(delete_forward(&mut buf, &mut len, &mut cursor));
        assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "\u{011F}\u{0131}");
    }

    #[test]
    fn insert_rejects_when_full() {
        let mut buf = [0u8; 2];
        let mut len = 0;
        let mut cursor = 0;
        assert!(insert_char(&mut buf, &mut len, &mut cursor, 'a'));
        assert!(insert_char(&mut buf, &mut len, &mut cursor, 'b'));
        assert!(!insert_char(&mut buf, &mut len, &mut cursor, 'c'));
        assert!(!insert_char(&mut buf, &mut len, &mut cursor, '\u{011F}'));
        assert_eq!(len, 2);
    }
}

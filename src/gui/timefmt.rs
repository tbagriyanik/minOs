//! Süre biçimlendirme yardımcıları.
//!
//! Çekirdek içinde `TextBuf` ile çizilen süre metinleri burada tek bir
//! kaynaktan üretilir. Modül yalnızca `core` kullanır; host testlerinde
//! (`tests/host_feature_tests.rs`) doğrudan sınanır.

/// Biçimlendirilmiş süre metni için güvenli tampon boyu.
pub const MAX_TEXT: usize = 40;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DurationParts {
    pub hours: u64,
    pub minutes: u64,
    pub seconds: u64,
}

/// Toplam saniyeyi saat/dakika/saniye bileşenlerine ayırır.
pub const fn split_duration(total_seconds: u64) -> DurationParts {
    DurationParts {
        hours: total_seconds / 3600,
        minutes: (total_seconds / 60) % 60,
        seconds: total_seconds % 60,
    }
}

/// "1sa 2dk 3sn" / "1h 2m 3s" biçiminde yazar; yazılan bayt sayısını döndürür.
pub fn format_duration(total_seconds: u64, english: bool, out: &mut [u8]) -> usize {
    let parts = split_duration(total_seconds);
    let mut len = 0;
    len += push_u64(out, len, parts.hours);
    len += push_str(out, len, if english { "h " } else { "sa " });
    len += push_u64(out, len, parts.minutes);
    len += push_str(out, len, if english { "m " } else { "dk " });
    len += push_u64(out, len, parts.seconds);
    len += push_str(out, len, if english { "s" } else { "sn" });
    len
}

/// Medya oynatıcıları için "H:MM:SS" (saat sıfırsa "M:SS") biçimi.
pub fn format_clock(total_seconds: u64, out: &mut [u8]) -> usize {
    let parts = split_duration(total_seconds);
    let mut len = 0;
    if parts.hours > 0 {
        len += push_u64(out, len, parts.hours);
        len += push_str(out, len, ":");
        len += push_padded(out, len, parts.minutes, 2);
    } else {
        len += push_u64(out, len, parts.minutes);
    }
    len += push_str(out, len, ":");
    len += push_padded(out, len, parts.seconds, 2);
    len
}

pub fn duration_string(total_seconds: u64, english: bool) -> alloc::string::String {
    let mut buf = [0u8; MAX_TEXT];
    let len = format_duration(total_seconds, english, &mut buf);
    alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap_or(""))
}

pub fn clock_string(total_seconds: u64) -> alloc::string::String {
    let mut buf = [0u8; MAX_TEXT];
    let len = format_clock(total_seconds, &mut buf);
    alloc::string::String::from(core::str::from_utf8(&buf[..len]).unwrap_or(""))
}

fn push_str(out: &mut [u8], len: usize, text: &str) -> usize {
    let mut written = 0;
    for byte in text.bytes() {
        let pos = len + written;
        if pos < out.len() {
            out[pos] = byte;
            written += 1;
        }
    }
    written
}

fn push_u64(out: &mut [u8], len: usize, mut value: u64) -> usize {
    let mut digits = [0u8; 20];
    let mut count = 0;
    if value == 0 { digits[count] = b'0'; count += 1; }
    while value > 0 {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
    }
    let mut written = 0;
    for index in (0..count).rev() {
        let pos = len + written;
        if pos < out.len() {
            out[pos] = digits[index];
            written += 1;
        }
    }
    written
}

fn push_padded(out: &mut [u8], len: usize, value: u64, width: usize) -> usize {
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut v = value;
    if v == 0 { digits[count] = b'0'; count += 1; }
    while v > 0 {
        digits[count] = b'0' + (v % 10) as u8;
        count += 1;
        v /= 10;
    }
    let mut written = 0;
    for _ in count..width {
        let pos = len + written;
        if pos < out.len() { out[pos] = b'0'; written += 1; }
    }
    for index in (0..count).rev() {
        let pos = len + written;
        if pos < out.len() {
            out[pos] = digits[index];
            written += 1;
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parts_split() {
        assert_eq!(split_duration(0), DurationParts { hours: 0, minutes: 0, seconds: 0 });
        assert_eq!(split_duration(3661), DurationParts { hours: 1, minutes: 1, seconds: 1 });
        assert_eq!(split_duration(90_061), DurationParts { hours: 25, minutes: 1, seconds: 1 });
    }

    #[test]
    fn duration_text_tr_en() {
        assert_eq!(duration_string(0, false), "0sa 0dk 0sn");
        assert_eq!(duration_string(3661, false), "1sa 1dk 1sn");
        assert_eq!(duration_string(3661, true), "1h 1m 1s");
    }

    #[test]
    fn clock_text() {
        assert_eq!(clock_string(5), "0:05");
        assert_eq!(clock_string(65), "1:05");
        assert_eq!(clock_string(3661), "1:01:01");
    }
}

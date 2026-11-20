//! Ortak sürücü durum API'si — sürücü başlatma kayıtlarının standart giriş/çıkış noktaları.
//!
//! Çekirdekteki sürücüler başlatma sonuçlarını farklı yerlerden bildirir
//! (`boot::subsystems` hatları, `boot::devices` taraması, `drivers::framework`
//! kayıt defteri). Bu modül tüm bu yollar için **tek** bir kayıt sözleşmesi
//! sağlar:
//!
//! * `[DRIVER-STATUS]` ve `[DRIVER-INIT]` seri satırlarını aynı alan
//!   sırasıyla üretir (`id=.. phase=.. ms=..` / `name=.. us=.. ok=..`).
//! * Uzun isimli (≥ `MAX_TIMING_NAME`) başlatma kayıtları için ismi ölçekler
//!   (`ata`, `net`, `usb`) ki Kontrol Paneli sütunları taşmasın.
//! * Alt sistem tanılama çıktısı sürücü başına tek özet satırına indirgenir
//!   (`drivers` etiketinde tekrar eden bloklar yerine `id=.. phase=..`).
//!
//! Modül saf mantıktır; donanıma ve çizime dokunmaz, bu yüzden host
//! testlerinde (`tests/host_runtime_tests.rs`) doğrulanır.

use crate::drivers::status::{self as status, DriverId, DriverPhase, Severity};

/// Başlatma kaydı için azimuth isim uzunluğu.
pub const MAX_TIMING_NAME: usize = 16;

/// Alt sistem tanılamasında dolaşan sürücü kimlikleri.
pub const DIAG_IDS: [DriverId; DriverId::COUNT] = DriverId::ALL;

/// `record_driver_init` için isim ölçekleme.
///
/// Uzun serbest isimler (`Intel HDA`, `Network Stack`, `VirtIO Block`) yerine
/// seri günlüğünde kararlı, kısa anahtarlar kullanılır; böylece log satırı
/// uzunlukları ve UI sütun hizaları sabit kalır.
pub fn timing_name(name: &str) -> &'static str {
    if name.len() <= MAX_TIMING_NAME {
        match name {
            "Intel HDA" => "audio",
            "Network Stack" | "Netstack" => "netstack",
            _ => static_name(name),
        }
    } else {
        static_name(name)
    }
}

fn static_name(name: &str) -> &'static str {
    match name {
        "PCI" => "pci",
        "ATA" => "ata",
        "AHCI" => "ahci",
        "NVMe" => "nvme",
        "SD/MMC" => "sdmmc",
        "VirtIO Block" => "virtio-blk",
        "GPU" => "display",
        "PS/2" => "input-ps2",
        "Intel HDA" => "audio",
        "Network" | "NIC" => "net",
        "Network Stack" | "Netstack" => "netstack",
        "USB Host" | "USB" => "usb",
        "Wi-Fi" => "wifi",
        "NE2000 (ISA)" => "net-isa",
        _ => "driver",
    }
}

/// Bir kaydın kabul edilmesi gereken azami uzunluk (seri satırı + açıklama).
pub const MAX_LOG_DETAIL: usize = 96;

/// Açıklamayı seri satırı için kısaltır (mutlak adres/uzun dökümler
/// `[DRIVER-STATUS]` satırını gereksiz uzatmasın).
pub fn clamp_detail(detail: &str) -> &str {
    let bytes = detail.as_bytes();
    if bytes.len() <= MAX_LOG_DETAIL {
        return detail;
    }
    // UTF-8 sınırında kırp: karakter sınırına geri sar.
    let mut end = MAX_LOG_DETAIL;
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    core::str::from_utf8(&bytes[..end]).unwrap_or("")
}

/// Kayıt satırındaki `ms=` alanı için süreyi normalize eder.
pub fn normalize_ms(elapsed_ms: u32) -> u32 {
    elapsed_ms
}

/// Kayıt satırındaki hata kodu için: yalnızca `Error` fazında kullanılır.
pub fn log_code(phase: DriverPhase, code: u16) -> u16 {
    if phase == DriverPhase::Error {
        code
    } else {
        status::ERR_NONE
    }
}

/// Seri günlüğünde tek satırlık durum kaydı (kararlı format).
pub fn log_line(
    id: DriverId,
    phase: DriverPhase,
    elapsed_ms: u32,
    code: u16,
    detail: &str,
) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    out.push_str("[DRIVER-STATUS] id=");
    out.push_str(id.key());
    out.push_str(" phase=");
    out.push_str(phase.key());
    if phase == DriverPhase::Error {
        out.push_str(" code=");
        push_dec(&mut out, u64::from(log_code(phase, code)));
    }
    out.push_str(" ms=");
    push_dec(&mut out, u64::from(normalize_ms(elapsed_ms)));
    let detail = clamp_detail(detail);
    if !detail.is_empty() {
        out.push_str(" desc=\"");
        out.push_str(detail);
        out.push('"');
    }
    out
}

/// Başlatma süresi kaydı satırı (kararlı format).
pub fn init_line(name: &str, duration_us: u64, ok: bool) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    out.push_str("[DRIVER-INIT] name=");
    out.push_str(timing_name(name));
    out.push_str(" us=");
    push_dec(&mut out, duration_us);
    out.push_str(" ok=");
    out.push_str(if ok { "1" } else { "0" });
    out
}

/// Alt sistem/sürücü tanılaması için tek satırlık özet.
pub fn diag_line() -> alloc::string::String {
    let mut out = alloc::string::String::new();
    out.push_str("[DRIVER-STATUS] summary");
    for id in DIAG_IDS {
        let e = status::entry(id);
        out.push(' ');
        out.push_str(id.key());
        out.push('=');
        out.push_str(e.phase.key());
        if e.phase == DriverPhase::Error {
            out.push('/');
            push_dec(&mut out, u64::from(e.error_code));
        }
    }
    out
}

/// Sürücü durumunu UI rengine eşler (Kontrol Paneli ve tanılama ortak kullanır).
pub fn severity_color(phase: DriverPhase) -> Severity {
    phase.severity()
}

/// Sürücü özeti: (hazır, kullanılamıyor, hata, diğer) → tek satırlık özet.
pub fn summary_line(english: bool) -> alloc::string::String {
    let (ready, unavailable, error, other) = status::summary();
    let mut out = alloc::string::String::new();
    out.push_str(if english { "Ready: " } else { "Hazır: " });
    push_dec(&mut out, ready as u64);
    out.push_str(if english {
        "  Unavailable: "
    } else {
        "  Kullanılamıyor: "
    });
    push_dec(&mut out, unavailable as u64);
    out.push_str(if english { "  Error: " } else { "  Hata: " });
    push_dec(&mut out, error as u64);
    if other > 0 {
        out.push_str(if english {
            "  Unknown: "
        } else {
            "  Bilinmiyor: "
        });
        push_dec(&mut out, other as u64);
    }
    out
}

fn push_dec(out: &mut alloc::string::String, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if v == 0 {
        out.push('0');
        return;
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for &b in &buf[i..] {
        out.push(b as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_line_format_is_stable() {
        let line = log_line(
            DriverId::Nvme,
            DriverPhase::Error,
            42,
            status::ERR_TIMEOUT,
            "csts",
        );
        assert_eq!(
            line,
            "[DRIVER-STATUS] id=nvme phase=error code=3 ms=42 desc=\"csts\""
        );
        let ok = log_line(DriverId::Net, DriverPhase::Ready, 7, 99, "");
        assert_eq!(ok, "[DRIVER-STATUS] id=net phase=ready ms=7");
        assert_eq!(log_code(DriverPhase::Ready, 5), status::ERR_NONE);
    }

    #[test]
    fn init_line_uses_short_names() {
        assert_eq!(
            init_line("Intel HDA", 900, true),
            "[DRIVER-INIT] name=audio us=900 ok=1"
        );
        assert_eq!(
            init_line("Network", 12, false),
            "[DRIVER-INIT] name=net us=12 ok=0"
        );
        assert!(init_line("Bilinmeyen Uzun Surucu Adi 123", 1, true).contains("name=driver"));
    }

    #[test]
    fn detail_is_clamped_on_char_boundary() {
        let long = "ğ".repeat(80); // 160 bayt
        let c = clamp_detail(&long);
        assert!(c.len() <= MAX_LOG_DETAIL);
        assert!(c.chars().all(|ch| ch == 'ğ'));
        assert_eq!(clamp_detail("kısa"), "kısa");
    }

    #[test]
    fn diag_line_lists_every_driver() {
        let line = diag_line();
        assert!(line.starts_with("[DRIVER-STATUS] summary"));
        for id in DriverId::ALL {
            assert!(line.contains(id.key()), "{} eksik: {line}", id.key());
        }
    }

    #[test]
    fn summary_line_is_localized() {
        let tr = summary_line(false);
        let en = summary_line(true);
        assert!(tr.contains("Hazır:"));
        assert!(en.contains("Ready:"));
    }

    #[test]
    fn push_dec_formats_boundaries() {
        let mut s = alloc::string::String::new();
        push_dec(&mut s, 0);
        push_dec(&mut s, u64::MAX);
        assert_eq!(s, "018446744073709551615");
    }
}

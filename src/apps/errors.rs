//! Uygulama hata sınıfları, kullanıcı mesajları ve kurtarma yolları.
//!
//! Bu modül çekirdeğe bağımlı değildir: yalnızca `core` + `alloc` kullanır.
//! Aynı dosya host testlerinde (`tests/host_tests.rs`) de derlenir; böylece
//! hata mesajlarının hem Türkçe hem İngilizce varyantları ve önerilen
//! kurtarma adımları otomatik olarak doğrulanır.
//!
//! Kullanım:
//! ```ignore
//! let err = AppError::new("Dosya Yöneticisi", AppErrorKind::PermissionDenied);
//! serial_print(err.message(crate::gui::ui_english()));
//! // kurtarma yolu bildirim/durum satırında gösterilir:
//! let hint = err.recovery(crate::gui::ui_english());
//! ```

extern crate alloc;

use alloc::string::String;

/// Bilinen uygulama hata sınıfları.
///
/// Her sınıfın kararlı bir kodu, TR/EN başlığı, açıklaması ve kurtarma
/// önerisi vardır; bu değerler `ALL` üzerinden test edilir.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AppErrorKind {
    /// Dosya veya klasör bulunamadı.
    FileNotFound,
    /// Yetki/izin reddedildi (salt okunur medya dahil).
    PermissionDenied,
    /// Hedef depolama alanı doldu veya Kota aşıldı.
    DiskFull,
    /// Dosya biçimi beklenen şemaya uymuyor.
    InvalidFormat,
    /// Ağ/bağlantı kurulamadı.
    NetworkUnavailable,
    /// İşlem zaman aşımına uğradı.
    Timeout,
    /// Heap ayırma başarısız veya bellek kotası doldu.
    OutOfMemory,
    /// Kullanıcı girdisi/geçersiz argüman.
    InvalidInput,
    /// Donanım veya işletim sistemi özelliği desteklenmiyor.
    Unsupported,
    /// Uygulama çalışırken istisna/panic oluştu (guard kurtardı).
    Crashed,
}

impl AppErrorKind {
    /// Tüm hata sınıfları (test ve kapsam denetimleri için).
    pub const ALL: [Self; 10] = [
        Self::FileNotFound,
        Self::PermissionDenied,
        Self::DiskFull,
        Self::InvalidFormat,
        Self::NetworkUnavailable,
        Self::Timeout,
        Self::OutOfMemory,
        Self::InvalidInput,
        Self::Unsupported,
        Self::Crashed,
    ];

    /// Seri günlüğünde kullanılan kararlı, ASCII hata kodu.
    pub fn code(self) -> &'static str {
        match self {
            Self::FileNotFound => "APP-E01",
            Self::PermissionDenied => "APP-E02",
            Self::DiskFull => "APP-E03",
            Self::InvalidFormat => "APP-E04",
            Self::NetworkUnavailable => "APP-E05",
            Self::Timeout => "APP-E06",
            Self::OutOfMemory => "APP-E07",
            Self::InvalidInput => "APP-E08",
            Self::Unsupported => "APP-E09",
            Self::Crashed => "APP-E10",
        }
    }

    /// Kısa başlık (toast/imageview başlığı).
    pub fn title(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::FileNotFound, false) => "Dosya bulunamadı",
            (Self::FileNotFound, true) => "File not found",
            (Self::PermissionDenied, false) => "Erişim reddedildi",
            (Self::PermissionDenied, true) => "Access denied",
            (Self::DiskFull, false) => "Disk dolu",
            (Self::DiskFull, true) => "Disk full",
            (Self::InvalidFormat, false) => "Geçersiz biçim",
            (Self::InvalidFormat, true) => "Invalid format",
            (Self::NetworkUnavailable, false) => "Ağ kullanılamıyor",
            (Self::NetworkUnavailable, true) => "Network unavailable",
            (Self::Timeout, false) => "İşlem zaman aşımına uğradı",
            (Self::Timeout, true) => "Operation timed out",
            (Self::OutOfMemory, false) => "Yetersiz bellek",
            (Self::OutOfMemory, true) => "Out of memory",
            (Self::InvalidInput, false) => "Geçersiz girdi",
            (Self::InvalidInput, true) => "Invalid input",
            (Self::Unsupported, false) => "Desteklenmiyor",
            (Self::Unsupported, true) => "Not supported",
            (Self::Crashed, false) => "Uygulama durduruldu",
            (Self::Crashed, true) => "Application stopped",
        }
    }

    /// Açıklayıcı mesaj (durum satırı veya toast gövdesi).
    pub fn message(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::FileNotFound, false) => {
                "Belirtilen dosya veya klasör bulunamadı. Konum yeniden denetlenebilir."
            }
            (Self::FileNotFound, true) => {
                "The requested file or folder could not be found. Check the location and retry."
            }
            (Self::PermissionDenied, false) => {
                "Bu işlem için yazma izni yok; kaynak salt okunur olabilir."
            }
            (Self::PermissionDenied, true) => {
                "Write permission is missing for this operation; the source may be read-only."
            }
            (Self::DiskFull, false) => "Hedefte yeterli boş alan yok; dosya kaydedilemedi.",
            (Self::DiskFull, true) => {
                "There is not enough free space on the target; the file was not saved."
            }
            (Self::InvalidFormat, false) => {
                "Dosya içeriği beklenen biçimde değil; veri değiştirilmedi."
            }
            (Self::InvalidFormat, true) => {
                "The file content does not match the expected format; data was left unchanged."
            }
            (Self::NetworkUnavailable, false) => "Ağ bağlantısı kurulamadı; sunucuya ulaşılamıyor.",
            (Self::NetworkUnavailable, true) => {
                "No network connection could be established; the server is unreachable."
            }
            (Self::Timeout, false) => "İşlem beklenen sürede tamamlanmadı ve güvenle iptal edildi.",
            (Self::Timeout, true) => {
                "The operation did not finish in time and was safely cancelled."
            }
            (Self::OutOfMemory, false) => "Bellek kotası doldu; büyük veri yüklenemedi.",
            (Self::OutOfMemory, true) => {
                "The memory quota is full; the large payload could not be loaded."
            }
            (Self::InvalidInput, false) => "Girilen değer bu işlem için geçersiz.",
            (Self::InvalidInput, true) => "The entered value is not valid for this operation.",
            (Self::Unsupported, false) => "Bu özellik şu anda desteklenmiyor.",
            (Self::Unsupported, true) => "This feature is not supported at the moment.",
            (Self::Crashed, false) => {
                "Uygulamada kurtarılamayan bir hata oluştu; sistem çalışmaya devam ediyor."
            }
            (Self::Crashed, true) => {
                "The application hit an unrecoverable error; the system keeps running."
            }
        }
    }

    /// Kullanıcıya gösterilen somut kurtarma adımı.
    pub fn recovery(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::FileNotFound, false) => {
                "Kurtarma: konumu yenileyin (F5) veya farklı bir dosya seçin."
            }
            (Self::FileNotFound, true) => {
                "Recovery: refresh the location (F5) or choose a different file."
            }
            (Self::PermissionDenied, false) => {
                "Kurtarma: dosyayı yazılabilir bir konuma kopyalayıp yeniden deneyin."
            }
            (Self::PermissionDenied, true) => {
                "Recovery: copy the file to a writable location and try again."
            }
            (Self::DiskFull, false) => {
                "Kurtarma: Geri Dönüşüm Kutusu'nu boşaltın veya başka bir sürücü seçin."
            }
            (Self::DiskFull, true) => "Recovery: empty the Recycle Bin or choose another drive.",
            (Self::InvalidFormat, false) => "Kurtarma: farklı bir dosya açın; özgün dosya korundu.",
            (Self::InvalidFormat, true) => {
                "Recovery: open a different file; the original file was preserved."
            }
            (Self::NetworkUnavailable, false) => {
                "Kurtarma: kablo/Wi-Fi bağlantısını denetleyip yeniden deneyin (F5)."
            }
            (Self::NetworkUnavailable, true) => {
                "Recovery: check the cable/Wi-Fi connection and retry (F5)."
            }
            (Self::Timeout, false) => "Kurtarma: işlemi yeniden deneyin; yarım kalan veri atıldı.",
            (Self::Timeout, true) => "Recovery: retry the operation; partial data was discarded.",
            (Self::OutOfMemory, false) => {
                "Kurtarma: açık sekmeleri/pencereleri kapatıp işlemi yeniden deneyin."
            }
            (Self::OutOfMemory, true) => "Recovery: close open tabs/windows and try again.",
            (Self::InvalidInput, false) => {
                "Kurtarma: Yardım (F1) içindeki örnek sözdizimini kullanın."
            }
            (Self::InvalidInput, true) => "Recovery: use the example syntax in Help (F1).",
            (Self::Unsupported, false) => {
                "Kurtarma: desteklenen bir seçenek kullanın veya güncel sürümü denetleyin."
            }
            (Self::Unsupported, true) => {
                "Recovery: use a supported option or check for a newer release."
            }
            (Self::Crashed, false) => {
                "Kurtarma: uygulama yeniden açılabilir; diğer pencereler ve veriler korundu."
            }
            (Self::Crashed, true) => {
                "Recovery: the app can be reopened; other windows and data were preserved."
            }
        }
    }

    /// Hata yeniden denemeye uygun mu? (ağ/zaman aşımı/bellek)
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::NetworkUnavailable | Self::Timeout | Self::OutOfMemory
        )
    }

    /// Mevcut uygulama hata dizgisini sınıfa eşler.
    ///
    /// Hem çekirdek-teknik İngilizce dizgiler (`not found`, `timeout`) hem
    /// kullanıcıya gösterilen Türkçe dizgiler (`bulunamadı`, `izin`) tanınır.
    pub fn classify(text: &str) -> Self {
        let mut lower = String::with_capacity(text.len());
        for ch in text.chars() {
            for lc in ch.to_lowercase() {
                lower.push(lc);
            }
        }
        let s = lower.as_str();
        // Sıralama önemli: "kullanım/usage" genel "geçersiz/invalid" öncesi.
        if s.contains("kullanım")
            || s.contains("kullanim")
            || s.contains("usage")
            || s.contains("argüman")
            || s.contains("arguman")
            || s.contains("invalid argument")
        {
            Self::InvalidInput
        } else if s.contains("not found")
            || s.contains("bulunamad")
            || s.contains("missing")
            || s.contains("dosya yok")
            || s.contains("klasör yok")
            || s.contains("klasor yok")
        {
            Self::FileNotFound
        } else if s.contains("permission")
            || s.contains("izin")
            || s.contains("yetki")
            || s.contains("read-only")
            || s.contains("salt okunur")
        {
            Self::PermissionDenied
        } else if s.contains("disk full")
            || s.contains("no space")
            || s.contains("dolu")
            || s.contains("quota")
            || s.contains("kota")
        {
            Self::DiskFull
        } else if s.contains("timeout") || s.contains("zaman aşım") || s.contains("zaman asim") {
            Self::Timeout
        } else if s.contains("network")
            || s.contains("ağ ")
            || s.contains("baglanti")
            || s.contains("bağlantı")
            || s.contains("dns")
            || s.contains("çözümlenemedi")
            || s.contains("cozumlenemedi")
            || s.contains("unreachable")
        {
            Self::NetworkUnavailable
        } else if s.contains("out of memory")
            || s.contains("bellek")
            || s.contains("allocation failed")
            || s.contains("oom")
        {
            Self::OutOfMemory
        } else if s.contains("format")
            || s.contains("biçim")
            || s.contains("bicim")
            || s.contains("bozuk")
            || s.contains("corrupt")
            || s.contains("geçersiz")
            || s.contains("gecersiz")
            || s.contains("invalid")
        {
            Self::InvalidFormat
        } else if s.contains("panic")
            || s.contains("fault")
            || s.contains("exception")
            || s.contains("#pf")
            || s.contains("sayfa hatas")
            || s.contains("çöktü")
            || s.contains("coktu")
        {
            Self::Crashed
        } else if s.contains("unsupported")
            || s.contains("desteklenmiyor")
            || s.contains("not implemented")
        {
            Self::Unsupported
        } else {
            Self::InvalidFormat
        }
    }
}

/// Bir uygulama hatasının sınıfı ve kaynağı.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct AppError {
    /// Hatanın oluştuğu uygulamanın görünen adı.
    pub app: &'static str,
    /// Hata sınıfı.
    pub kind: AppErrorKind,
}

impl AppError {
    pub const fn new(app: &'static str, kind: AppErrorKind) -> Self {
        Self { app, kind }
    }

    pub fn title(self, english: bool) -> &'static str {
        self.kind.title(english)
    }

    pub fn message(self, english: bool) -> &'static str {
        self.kind.message(english)
    }

    pub fn recovery(self, english: bool) -> &'static str {
        self.kind.recovery(english)
    }

    pub fn retryable(self) -> bool {
        self.kind.retryable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_kind_has_stable_unique_code() {
        let mut codes = alloc::vec::Vec::new();
        for kind in AppErrorKind::ALL {
            let code = kind.code();
            assert!(code.starts_with("APP-E"), "{code}: beklenen APP-Enn kodu");
            assert!(!codes.contains(&code), "{code}: yinelenen hata kodu");
            codes.push(code);
        }
        assert_eq!(codes.len(), 10);
    }

    #[test]
    fn every_error_kind_has_localized_message_and_recovery() {
        for kind in AppErrorKind::ALL {
            for english in [false, true] {
                let title = kind.title(english);
                let message = kind.message(english);
                let recovery = kind.recovery(english);
                assert!(!title.is_empty(), "{:?}: başlık boş", kind);
                assert!(message.len() >= 12, "{:?}: açıklama çok kısa", kind);
                assert!(
                    recovery.starts_with("Kurtarma:") || recovery.starts_with("Recovery:"),
                    "{:?}: kurtarma adımı açıkça işaretlenmeli",
                    kind
                );
                assert!(
                    recovery.len() > 20,
                    "{:?}: kurtarma adımı somut değil",
                    kind
                );
            }
            assert_ne!(
                kind.title(false),
                kind.title(true),
                "{:?}: TR/EN başlığı aynı",
                kind
            );
            assert_ne!(
                kind.message(false),
                kind.message(true),
                "{:?}: TR/EN mesajı aynı",
                kind
            );
            assert_ne!(
                kind.recovery(false),
                kind.recovery(true),
                "{:?}: TR/EN kurtarma yolu aynı",
                kind
            );
        }
    }

    #[test]
    fn retryable_errors_are_only_transient_classes() {
        for kind in AppErrorKind::ALL {
            let expected = matches!(
                kind,
                AppErrorKind::NetworkUnavailable
                    | AppErrorKind::Timeout
                    | AppErrorKind::OutOfMemory
            );
            assert_eq!(
                kind.retryable(),
                expected,
                "{:?}: yanlış yeniden deneme sınıfı",
                kind
            );
        }
    }

    #[test]
    fn classify_recognizes_kernel_and_ui_error_strings() {
        let cases = [
            ("uygulama dosyasi bulunamadi", AppErrorKind::FileNotFound),
            ("destination folder not found", AppErrorKind::FileNotFound),
            ("izin reddedildi", AppErrorKind::PermissionDenied),
            ("permission denied", AppErrorKind::PermissionDenied),
            ("disk dolu", AppErrorKind::DiskFull),
            ("no space left on device", AppErrorKind::DiskFull),
            ("zaman aşımı", AppErrorKind::Timeout),
            ("network timeout", AppErrorKind::Timeout),
            ("ağ bağlantısı yok", AppErrorKind::NetworkUnavailable),
            ("DNS çözümlenemedi", AppErrorKind::NetworkUnavailable),
            ("yetersiz bellek", AppErrorKind::OutOfMemory),
            ("out of memory", AppErrorKind::OutOfMemory),
            ("Kullanım: cp <kaynak> <hedef>", AppErrorKind::InvalidInput),
            ("usage: format <disk>", AppErrorKind::InvalidInput),
            ("geçersiz biçim", AppErrorKind::InvalidFormat),
            ("panic in application", AppErrorKind::Crashed),
            ("sayfa hatası #PF", AppErrorKind::Crashed),
            ("desteklenmiyor", AppErrorKind::Unsupported),
        ];
        for (text, expected) in cases {
            assert_eq!(
                AppErrorKind::classify(text),
                expected,
                "'{text}' sınıfı beklenen {:?} değil",
                expected
            );
        }
    }

    #[test]
    fn error_carries_app_name_and_forwards_localized_text() {
        let err = AppError::new("Dosya Yöneticisi", AppErrorKind::PermissionDenied);
        assert_eq!(err.app, "Dosya Yöneticisi");
        assert_eq!(err.title(false), "Erişim reddedildi");
        assert_eq!(err.title(true), "Access denied");
        assert_eq!(
            err.message(false),
            AppErrorKind::PermissionDenied.message(false)
        );
        assert_eq!(
            err.recovery(true),
            AppErrorKind::PermissionDenied.recovery(true)
        );
        assert!(!err.retryable());
    }
}

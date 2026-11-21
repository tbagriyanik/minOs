//! Ortak sürücü durum modeli: her donanım sürücüsü `Ready`,
//! `Unavailable` (donanım yok / desteklenmiyor) veya `Error`
//! (başlatma hatası) olarak izlenir.
//!
//! Başlatma süresi ve sonuç seri günlüğe `[DRIVER-STATUS]` satırları
//! olarak yazılır; hata durumlarında kararlı bir hata kodu ve kısa
//! açıklama saklanır. Kontrol Paneli sürücü ekranı bu kayıt tablosunu
//! okur.
//!
//! Modül saf mantık + sabit boyutlu kayıt tablosu içerir; host birim
//! testlerinde (`tests/host_driver_tests.rs`) donanım olmadan durum
//! geçişleri doğrulanır.

use crate::arch::x86_64::sync::SpinMutex;

/// İzlenen donanım sürücüsü kimlikleri.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum DriverId {
    /// PCI veri yolu numaralandırıcısı.
    Pci = 0,
    /// ATA/ATAPI (IDE) disk denetleyicisi.
    Ata,
    /// AHCI SATA denetleyicisi.
    Ahci,
    /// NVMe depolama denetleyicisi.
    Nvme,
    /// SD/MMC kart denetleyicisi (SDHCI).
    Sdmmc,
    /// VirtIO blok aygıtı.
    VirtioBlk,
    /// Ağ arabirimi (e1000/VMXNET3/RTL81xx/Broadcom/NE2000).
    Net,
    /// Görüntü alt sistemi (VGA/BGA/VirtIO-gpu/AMD/NVIDIA).
    Display,
    /// Ses alt sistemi (Intel HDA; PC Speaker yedeği).
    Audio,
    /// USB ana makine denetleyicisi.
    Usb,
    /// Giriş aygıtları (USB HID + PS/2).
    Input,
}

impl DriverId {
    pub const COUNT: usize = 11;
    pub const ALL: [DriverId; Self::COUNT] = [
        DriverId::Pci,
        DriverId::Ata,
        DriverId::Ahci,
        DriverId::Nvme,
        DriverId::Sdmmc,
        DriverId::VirtioBlk,
        DriverId::Net,
        DriverId::Display,
        DriverId::Audio,
        DriverId::Usb,
        DriverId::Input,
    ];

    /// Seri günlüğünde kullanılan kararlı ASCII anahtar.
    pub fn key(self) -> &'static str {
        match self {
            DriverId::Pci => "pci",
            DriverId::Ata => "ata",
            DriverId::Ahci => "ahci",
            DriverId::Nvme => "nvme",
            DriverId::Sdmmc => "sdmmc",
            DriverId::VirtioBlk => "virtio-blk",
            DriverId::Net => "net",
            DriverId::Display => "display",
            DriverId::Audio => "audio",
            DriverId::Usb => "usb",
            DriverId::Input => "input",
        }
    }

    /// Kontrol Paneli'nde gösterilen Türkçe ad.
    pub fn label_tr(self) -> &'static str {
        match self {
            DriverId::Pci => "PCI Veri Yolu",
            DriverId::Ata => "ATA/ATAPI Disk",
            DriverId::Ahci => "AHCI SATA",
            DriverId::Nvme => "NVMe SSD",
            DriverId::Sdmmc => "SD/MMC Kart",
            DriverId::VirtioBlk => "VirtIO Blok",
            DriverId::Net => "Ağ Arabirimi",
            DriverId::Display => "Görüntü",
            DriverId::Audio => "Ses",
            DriverId::Usb => "USB Denetleyicisi",
            DriverId::Input => "Giriş Aygıtları",
        }
    }

    /// Kontrol Paneli'nde gösterilen İngilizce ad.
    pub fn label_en(self) -> &'static str {
        match self {
            DriverId::Pci => "PCI Bus",
            DriverId::Ata => "ATA/ATAPI Disk",
            DriverId::Ahci => "AHCI SATA",
            DriverId::Nvme => "NVMe SSD",
            DriverId::Sdmmc => "SD/MMC Card",
            DriverId::VirtioBlk => "VirtIO Block",
            DriverId::Net => "Network Interface",
            DriverId::Display => "Display",
            DriverId::Audio => "Audio",
            DriverId::Usb => "USB Host",
            DriverId::Input => "Input Devices",
        }
    }

    pub fn label(self, english: bool) -> &'static str {
        if english {
            self.label_en()
        } else {
            self.label_tr()
        }
    }

    /// Destek olgunluk seviyesi (docs/driver-maturity.md ile uyumlu).
    ///
    /// `0` = Tam, `1` = Kısmi, `2` = İskelet/Deneysel. UI'da renk ve
    /// etiket seçimi için kullanılır.
    pub fn support_level(self) -> u8 {
        match self {
            DriverId::Pci
            | DriverId::Ata
            | DriverId::Ahci
            | DriverId::Net
            | DriverId::Display
            | DriverId::Audio
            | DriverId::Usb
            | DriverId::Input => 0,
            // Denetleyici hazır ama blok I/O veya DMA yolu eksik:
            // NVMe/VirtIO-blk init-only, SD/MMC PIO (DMA'sız).
            DriverId::Nvme | DriverId::Sdmmc | DriverId::VirtioBlk => 1,
        }
    }

    /// `support_level` için kısa etiket.
    pub fn support_label(self, english: bool) -> &'static str {
        match (self.support_level(), english) {
            (0, false) => "Tam",
            (0, true) => "Full",
            (1, false) => "Kısmi",
            (1, true) => "Partial",
            (_, false) => "İskelet",
            (_, true) => "Skeleton",
        }
    }

    /// Sürücünün tanıdığı aygıt modelini kısa statik etiketle döndürür.
    ///
    /// Dinamik model dizgileri (ör. ATA IDENTIFY modeli) yerine sürücü
    /// ailesinin bilinen adı kullanılır; ayrıntı `entry.detail`'dedir.
    pub fn device_model(self) -> &'static str {
        match self {
            DriverId::Pci => "PCI Host Bridge",
            DriverId::Ata => "IDE/ATAPI",
            DriverId::Ahci => "AHCI SATA",
            DriverId::Nvme => "NVMe Controller",
            DriverId::Sdmmc => "SDHCI",
            DriverId::VirtioBlk => "virtio-blk",
            DriverId::Net => "Ethernet NIC",
            DriverId::Display => "Framebuffer/GPU",
            DriverId::Audio => "HDA / PC Speaker",
            DriverId::Usb => "USB Host",
            DriverId::Input => "HID / PS/2",
        }
    }
}

/// Sürücünün ortak yaşam döngüsü durumu.
///
/// `Unavailable`, donanımın hiç olmadığı veya profil tarafından
/// devre dışı bırakıldığı durumu; `Error` ise donanım bulunup
/// başlatmanın başarısız olduğu durumu ifade eder. İkisi seri günlüğünde
/// ve Kontrol Paneli'nde farklı gösterilir.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriverPhase {
    /// Henüz sorgulanmadı.
    Unknown,
    /// Başlatma/probe sürüyor.
    Probing,
    /// Sürücü kullanıma hazır.
    Ready,
    /// Donanım yok veya desteklenmiyor (başlatma hatası DEĞİL).
    Unavailable,
    /// Başlatma denendi ve başarısız oldu.
    Error,
}

impl DriverPhase {
    /// Seri günlüğünde kullanılan kararlı ASCII anahtar.
    pub fn key(self) -> &'static str {
        match self {
            DriverPhase::Unknown => "unknown",
            DriverPhase::Probing => "probing",
            DriverPhase::Ready => "ready",
            DriverPhase::Unavailable => "unavailable",
            DriverPhase::Error => "error",
        }
    }

    /// Kontrol Paneli'nde gösterilen Türkçe durum etiketi.
    pub fn label_tr(self) -> &'static str {
        match self {
            DriverPhase::Unknown => "Bilinmiyor",
            DriverPhase::Probing => "Taranıyor",
            DriverPhase::Ready => "Hazır",
            DriverPhase::Unavailable => "Kullanılamıyor",
            DriverPhase::Error => "Hata",
        }
    }

    /// Kontrol Paneli'nde gösterilen İngilizce durum etiketi.
    pub fn label_en(self) -> &'static str {
        match self {
            DriverPhase::Unknown => "Unknown",
            DriverPhase::Probing => "Probing",
            DriverPhase::Ready => "Ready",
            DriverPhase::Unavailable => "Unavailable",
            DriverPhase::Error => "Error",
        }
    }

    pub fn label(self, english: bool) -> &'static str {
        if english {
            self.label_en()
        } else {
            self.label_tr()
        }
    }

    /// Durum satırı rengi için sınıflandırma.
    pub fn severity(self) -> Severity {
        match self {
            DriverPhase::Ready => Severity::Ok,
            DriverPhase::Probing | DriverPhase::Unknown => Severity::Info,
            DriverPhase::Unavailable => Severity::Warn,
            DriverPhase::Error => Severity::Fail,
        }
    }
}

/// Durum satırının görsel önceliği (UI renk seçimi).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Info,
    Ok,
    Warn,
    Fail,
}

// --- Kararlı hata kodları (seri günlüğü + UI için) ---

pub const ERR_NONE: u16 = 0;
/// Sürücü `init` yolu `false` döndürdü veya erken çıktı.
pub const ERR_INIT_FAILED: u16 = 1;
/// DMA/halka belleği ayrılamadı.
pub const ERR_NO_DMA: u16 = 2;
/// Donanım hazır olma zaman aşımı.
pub const ERR_TIMEOUT: u16 = 3;
/// Aygıt beklenmeyen durumda (ör. reset başarısız).
pub const ERR_BAD_STATE: u16 = 4;
/// Sürücü profili tarafından devre dışı bırakıldı.
pub const ERR_DISABLED: u16 = 5;

/// Hata kodunun kısa açıklaması (TR/EN).
pub fn error_desc(code: u16, english: bool) -> &'static str {
    match code {
        ERR_INIT_FAILED => {
            if english {
                "driver init failed"
            } else {
                "sürücü başlatma hatası"
            }
        }
        ERR_NO_DMA => {
            if english {
                "DMA memory allocation failed"
            } else {
                "DMA bellek tahsisi başarısız"
            }
        }
        ERR_TIMEOUT => {
            if english {
                "hardware ready timeout"
            } else {
                "donanım hazır olma zaman aşımı"
            }
        }
        ERR_BAD_STATE => {
            if english {
                "device in unexpected state"
            } else {
                "aygıt beklenmeyen durumda"
            }
        }
        ERR_DISABLED => {
            if english {
                "disabled by boot profile"
            } else {
                "önyükleme profili tarafından devre dışı"
            }
        }
        _ => {
            if english {
                "unknown driver error"
            } else {
                "bilinmeyen sürücü hatası"
            }
        }
    }
}

/// Kayıt tablosundaki tek bir sürücü girdisi (değer kopyası).
#[derive(Clone, Copy, Debug)]
pub struct DriverStatusEntry {
    pub id: DriverId,
    pub phase: DriverPhase,
    /// Son ölçülen başlatma/probe süresi (ms).
    pub elapsed_ms: u32,
    /// `DriverPhase::Error` için kararlı hata kodu; diğer durumlarda 0.
    pub error_code: u16,
    /// Hata veya kısıt için kısa açıklama.
    pub detail: &'static str,
    /// Probe sonrasında oluşan çalışma zamanı hatalarının toplam sayısı.
    /// `note_error` ile artırılır; UI'daki "hata sayısı" alanını besler.
    pub error_count: u32,
    /// Son çalışma zamanı hatasının kodu (`note_error`); hiç hata
    /// olmadıysa `ERR_NONE`. Probe `error_code`'undan ayrıdır.
    pub last_error: u16,
}

impl DriverStatusEntry {
    const EMPTY: Self = Self {
        id: DriverId::Pci,
        phase: DriverPhase::Unknown,
        elapsed_ms: 0,
        error_code: ERR_NONE,
        detail: "",
        error_count: 0,
        last_error: ERR_NONE,
    };

    const fn for_id(id: DriverId) -> Self {
        Self { id, ..Self::EMPTY }
    }
}

const fn empty_registry() -> [DriverStatusEntry; DriverId::COUNT] {
    let mut entries = [DriverStatusEntry::EMPTY; DriverId::COUNT];
    let mut i = 0;
    while i < DriverId::COUNT {
        entries[i] = DriverStatusEntry::for_id(DriverId::ALL[i]);
        i += 1;
    }
    entries
}

static REGISTRY: SpinMutex<[DriverStatusEntry; DriverId::COUNT]> = SpinMutex::new(empty_registry());

fn update(id: DriverId, f: impl FnOnce(&mut DriverStatusEntry)) {
    let mut reg = REGISTRY.lock();
    f(&mut reg[id as usize]);
}

/// Seri günlüğüne tek satırlık durum kaydı yazar.
///
/// Format tek bir yerde tanımlıdır (`drivers::api::log_line`); böylece
/// `[DRIVER-STATUS]` satırları tüm sürücüler için aynı alan sırasını ve
/// uzunluk sınırını kullanır.
fn log_status(id: DriverId, phase: DriverPhase, elapsed_ms: u64, code: u16, detail: &str) {
    let clamped = elapsed_ms.min(u32::MAX as u64) as u32;
    let _line = crate::drivers::api::log_line(id, phase, clamped, code, detail);
    crate::kinfo!("driver", "{}\n", _line);
}

/// Şu anki monotonik zaman (ms). `probe_start` ile birlikte kullanılır.
pub fn now_ms() -> u64 {
    crate::boot::time::monotonic_ms()
}

/// `start` anından bu yana geçen süre (ms, 32-bit'e kırpılmış).
pub fn elapsed_since(start: u64) -> u32 {
    now_ms().saturating_sub(start).min(u32::MAX as u64) as u32
}

/// Bir sürücünün probe/init aşamasını başlatır: durumu `Probing` yapar,
/// seri günlüğe başlangıç satırı yazar ve zaman damgası döndürür.
pub fn probe_start(id: DriverId) -> u64 {
    update(id, |e| {
        e.phase = DriverPhase::Probing;
        e.error_code = ERR_NONE;
        e.detail = "";
    });
    log_status(id, DriverPhase::Probing, 0, ERR_NONE, "");
    now_ms()
}

/// Probe'u `Ready` olarak kapatır.
pub fn probe_ready(id: DriverId, start: u64) {
    probe_ready_detail(id, start, "");
}

/// Probe'u isteğe bağlı açıklamayla `Ready` olarak kapatır.
pub fn probe_ready_detail(id: DriverId, start: u64, detail: &'static str) {
    let ms = elapsed_since(start);
    update(id, |e| {
        e.phase = DriverPhase::Ready;
        e.elapsed_ms = ms;
        e.error_code = ERR_NONE;
        e.detail = detail;
    });
    log_status(id, DriverPhase::Ready, ms as u64, ERR_NONE, detail);
}

/// Donanımın olmadığı / desteklenmediği sonucunu kaydeder
/// (`Unavailable` — başlatma hatası değildir).
pub fn probe_unavailable(id: DriverId, start: u64, detail: &'static str) {
    let ms = elapsed_since(start);
    update(id, |e| {
        e.phase = DriverPhase::Unavailable;
        e.elapsed_ms = ms;
        e.error_code = ERR_NONE;
        e.detail = detail;
    });
    log_status(id, DriverPhase::Unavailable, ms as u64, ERR_NONE, detail);
}

/// Başlatma hatasını kararlı kod + açıklamayla kaydeder.
pub fn probe_error(id: DriverId, start: u64, code: u16, detail: &'static str) {
    let ms = elapsed_since(start);
    update(id, |e| {
        e.phase = DriverPhase::Error;
        e.elapsed_ms = ms;
        e.error_code = code;
        e.detail = detail;
    });
    log_status(id, DriverPhase::Error, ms as u64, code, detail);
}

/// Çalışma zamanı sürücü hatasını sayar ve seri günlüğe yazar.
///
/// Probe durumu (`phase`) değişmez; `error_count` artar ve son hata
/// kodu `last_error` alanına kaydedilir. Sürücüler I/O hatası, timeout
/// veya reset gerektiğinde çağırır.
pub fn note_error(id: DriverId, code: u16) {
    let count = {
        let mut reg = REGISTRY.lock();
        let e = &mut reg[id as usize];
        e.error_count = e.error_count.saturating_add(1);
        e.last_error = code;
        e.error_count
    };
    crate::kinfo!(
        "driver",
        "[DRIVER-ERROR] id={} code={} count={}\n",
        id.key(),
        code,
        count
    );
}

/// Kayıt girdisinin anlık kopyasını döndürür.
pub fn entry(id: DriverId) -> DriverStatusEntry {
    let reg = REGISTRY.lock();
    reg[id as usize]
}

/// Özet sayılar: (hazır, kullanılamıyor, hata, diğer).
pub fn summary() -> (usize, usize, usize, usize) {
    let reg = REGISTRY.lock();
    let mut ready = 0;
    let mut unavailable = 0;
    let mut error = 0;
    let mut other = 0;
    for e in reg.iter() {
        match e.phase {
            DriverPhase::Ready => ready += 1,
            DriverPhase::Unavailable => unavailable += 1,
            DriverPhase::Error => error += 1,
            _ => other += 1,
        }
    }
    (ready, unavailable, error, other)
}

/// Tüm sürücü girdilerini tek satırlık `[DRIVER-STATUS]` kayıtlarıyla
/// seri günlüğe döker. `log_status_summary` ve Sistem Bilgisi ekranı
/// tarafından kullanılır; her satır `key phase ms err errs` alanlarını
/// içerir.
pub fn log_all_entries() {
    for id in DriverId::ALL {
        let e = entry(id);
        crate::kinfo!(
            "driver",
            "[DRIVER-STATUS] {} phase={} ms={} err={} errs={}\n",
            e.id.key(),
            e.phase.key(),
            e.elapsed_ms,
            e.error_code,
            e.error_count
        );
    }
}

/// Disk ailesi sürücüleri (depolama grubu engelleme kararı için).
pub const STORAGE_IDS: [DriverId; 5] = [
    DriverId::Ata,
    DriverId::Ahci,
    DriverId::Nvme,
    DriverId::Sdmmc,
    DriverId::VirtioBlk,
];

/// `app_id` için gerekli sürücü hatadaysa ilgili `DriverId`'yi döndürür;
/// aksi halde `None`. Yalnızca `Error` fazı engeller — donanımı olmayan
/// (`Unavailable`) sistemlerde uygulamalar kendi yedek/çevrimdışı
/// davranışlarını sergiler.
pub fn blocking_driver(app_id: usize) -> Option<DriverId> {
    match app_id {
        // Web Gezgini ve Paket Yöneticisi ağ gerektirir.
        3 | 13 => {
            let e = entry(DriverId::Net);
            if e.phase == DriverPhase::Error {
                Some(DriverId::Net)
            } else {
                None
            }
        }
        // Dosya Yöneticisi ve Kurulum Sihirbazı kalıcı disk kullanır:
        // en az bir disk sürücüsü hazır değilse ve hiçbiri hazır
        // değilken en az biri hata verdiyse engelle.
        0 | 11 => {
            let any_ready = STORAGE_IDS
                .iter()
                .any(|id| entry(*id).phase == DriverPhase::Ready);
            if any_ready {
                return None;
            }
            STORAGE_IDS
                .iter()
                .copied()
                .find(|id| entry(*id).phase == DriverPhase::Error)
        }
        _ => None,
    }
}

/// `blocking_driver` için kullanıcıya gösterilecek engelleme nedeni.
pub fn blocked_reason(app_id: usize, english: bool) -> Option<&'static str> {
    let id = blocking_driver(app_id)?;
    let _ = entry(id);
    if english {
        Some(match id {
            DriverId::Net => "Network driver failed; the app was deactivated safely.",
            _ => "Storage driver failed; the app was deactivated safely.",
        })
    } else {
        Some(match id {
            DriverId::Net => "Ağ sürücüsü hatası; uygulama güvenli biçimde pasifleştirildi.",
            _ => "Depolama sürücüsü hatası; uygulama güvenli biçimde pasifleştirildi.",
        })
    }
}

// Host testlerinde `SpinMutex` yedegi gercek kilitleme yapmaz ve testler
// paralel kosar; kayit tablosunu paylasan testler bu korumayi kullanir.
#[cfg(test)]
static TEST_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Test koruma nesnesi: dustugunde kilidi birakir.
#[cfg(test)]
pub struct TestGuard;

#[cfg(test)]
impl Drop for TestGuard {
    fn drop(&mut self) {
        TEST_LOCK.store(false, core::sync::atomic::Ordering::Release);
    }
}

/// Kayit tablosunu testler arasinda serilestiren koruma nesnesi dondurur.
#[cfg(test)]
pub fn test_guard() -> TestGuard {
    while TEST_LOCK
        .compare_exchange(
            false,
            true,
            core::sync::atomic::Ordering::Acquire,
            core::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        core::hint::spin_loop();
    }
    TestGuard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_transitions_are_recorded() {
        let _guard = test_guard();
        let id = DriverId::Sdmmc;
        let start = probe_start(id);
        assert_eq!(entry(id).phase, DriverPhase::Probing);

        probe_unavailable(id, start, "test: no hw");
        let e = entry(id);
        assert_eq!(e.phase, DriverPhase::Unavailable);
        assert_eq!(e.error_code, ERR_NONE);
        assert_eq!(e.detail, "test: no hw");

        let start = probe_start(id);
        probe_error(id, start, ERR_INIT_FAILED, "test: init failed");
        let e = entry(id);
        assert_eq!(e.phase, DriverPhase::Error);
        assert_eq!(e.error_code, ERR_INIT_FAILED);
        assert_eq!(e.detail, "test: init failed");

        let start = probe_start(id);
        probe_ready(id, start);
        assert_eq!(entry(id).phase, DriverPhase::Ready);
        assert_eq!(entry(id).error_code, ERR_NONE);
    }

    #[test]
    fn unavailable_and_error_are_distinct() {
        let _guard = test_guard();
        // Unavailable: donanım yok (başlatma hiç denenmedi).
        let start = probe_start(DriverId::Ahci);
        probe_unavailable(DriverId::Ahci, start, "no controller");
        // Error: donanım var ama init başarısız.
        let start = probe_start(DriverId::Nvme);
        probe_error(DriverId::Nvme, start, ERR_TIMEOUT, "csts rdy timeout");

        assert_eq!(entry(DriverId::Ahci).phase, DriverPhase::Unavailable);
        assert_eq!(entry(DriverId::Nvme).phase, DriverPhase::Error);
        assert_ne!(
            entry(DriverId::Ahci).phase,
            entry(DriverId::Nvme).phase,
            "Unsupported hardware and boot failure must be distinguishable"
        );
    }

    #[test]
    fn error_codes_have_descriptions() {
        let _guard = test_guard();
        for code in [
            ERR_INIT_FAILED,
            ERR_NO_DMA,
            ERR_TIMEOUT,
            ERR_BAD_STATE,
            ERR_DISABLED,
        ] {
            assert!(!error_desc(code, false).is_empty());
            assert!(!error_desc(code, true).is_empty());
        }
    }

    #[test]
    fn blocking_driver_only_on_error() {
        let _guard = test_guard();
        // Net: Error → tarayıcı engellenir; Unavailable → engellenmez.
        let start = probe_start(DriverId::Net);
        probe_error(DriverId::Net, start, ERR_INIT_FAILED, "test");
        assert_eq!(blocking_driver(3), Some(DriverId::Net));
        assert_eq!(blocking_driver(13), Some(DriverId::Net));
        assert_eq!(blocking_driver(0), None);

        let start = probe_start(DriverId::Net);
        probe_unavailable(DriverId::Net, start, "no nic");
        assert_eq!(blocking_driver(3), None, "Unavailable must not block");

        // Depolama: tümü Error ve hiçbiri Ready değilse Dosya Yöneticisi engellenir.
        for id in STORAGE_IDS {
            let start = probe_start(id);
            probe_error(id, start, ERR_INIT_FAILED, "test");
        }
        assert!(blocking_driver(0).is_some());
        assert!(blocking_driver(11).is_some());

        // Bir disk sürücüsü Ready olursa engel kalkar.
        let start = probe_start(DriverId::Ata);
        probe_ready(DriverId::Ata, start);
        assert_eq!(blocking_driver(0), None);
        // Diğer uygulamalar depolama hatasından etkilenmez.
        assert_eq!(blocking_driver(2), None);
    }
}

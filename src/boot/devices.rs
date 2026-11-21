//! PS/2 giris aygitlarinin global durumlari (Asama 3'te masaustu oturumuna tasinacak)
//! ve surucu durum toplama / yeniden tarama.
//!
//! Ortak durum modeli `drivers::status` icinde tanimlidir; bu modul
//! donanim sorgularini calistirip sonuclari oraya yazar. Hem acilis
//! sirasinda (hatlarin bildirmedigi suruculer) hem de Kontrol Paneli
//! "Yeniden Tara" eyleminde ayni kod yolu kullanilir.

use crate::drivers;
use crate::drivers::status::{self, DriverId, DriverPhase};

pub static mut GLOBAL_MOUSE: drivers::mouse::MouseState = drivers::mouse::MouseState::new();
pub static mut GLOBAL_KEYBOARD: drivers::keyboard::KeyboardState =
    drivers::keyboard::KeyboardState::new();

// ---------------------------------------------------------------------------
// Surucu durum toplama
// ---------------------------------------------------------------------------

/// Limine komut satirindan surucu profilini cozer (subsystems ile ayni
/// oncelik: full > hardware > audio > safe).
fn driver_profile() -> &'static str {
    let cmdline = crate::boot::limine::limine_boot_cmdline().unwrap_or("");
    if crate::boot::limine::cmdline_has_token(cmdline, "full") {
        "full"
    } else if crate::boot::limine::cmdline_has_token(cmdline, "hardware") {
        "hardware"
    } else if crate::boot::limine::cmdline_has_token(cmdline, "audio") {
        "audio"
    } else if crate::boot::limine::cmdline_has_token(cmdline, "safe") {
        "safe"
    } else {
        "full"
    }
}

/// Hatlara bagli surucunun profil kapsaminda calisip calismadigini dondurur.
fn profile_runs(profile: &str, lane: &str) -> bool {
    matches!(
        (profile, lane),
        ("full", _) | ("audio", "audio") | ("hardware", "net" | "usb" | "hid")
    )
}

/// Acilis sirasinda bir kez cagrilir: hatlarin raporlamadigi suruculeri
/// tamamlar, PCI ve depolama denetleyicilerini probe eder.
pub fn collect_driver_statuses() {
    scan(false);
}

/// Kontrol Paneli "Yeniden Tara" eylemi: tum suruculeri yeniden sorgular.
/// DMA gerektiren hatlar (ag/ses/HID) yeniden baslatilmaz; yalnizca
/// guvenli algilama ve durum sorgulari tekrarlanir.
pub fn rescan_drivers() {
    kinfo!("boot", "[DRIVERS] Surucu yeniden taramasi baslatildi.\n");
    scan(true);
    kinfo!("boot", "[DRIVERS] Surucu yeniden taramasi tamamlandi.\n");
}

fn scan(refresh: bool) {
    let profile = driver_profile();

    // PCI veri yolu numaralandiricisi.
    if refresh || status::entry(DriverId::Pci).phase == DriverPhase::Unknown {
        let t = status::probe_start(DriverId::Pci);
        let ok = if refresh {
            drivers::pci::rescan()
        } else {
            drivers::pci::init()
        };
        if ok {
            status::probe_ready_detail(DriverId::Pci, t, "pci aygit taramasi tamam");
        } else {
            status::probe_error(
                DriverId::Pci,
                t,
                status::ERR_INIT_FAILED,
                "pci tarama hatasi",
            );
        }
    }

    // ATA/ATAPI: diskfs::init icindeki gercek baslatma suresini korumak
    // icin yalnizca henuz raporlanmadiysa (veya rescan) sorgulariz.
    if refresh || status::entry(DriverId::Ata).phase == DriverPhase::Unknown {
        let t = status::probe_start(DriverId::Ata);
        drivers::ata::init();
        if drivers::ata::drive_count() > 0 {
            status::probe_ready(DriverId::Ata, t);
        } else {
            status::probe_unavailable(DriverId::Ata, t, "ATA/ATAPI aygiti bulunamadi");
        }
    }

    probe_storage_controller(
        DriverId::Ahci,
        drivers::ahci::detect,
        drivers::ahci::init,
        "AHCI denetleyicisi yok",
        "ahci init hatasi",
        "sata portu hazir; LBA48 DMA okuma/yazma aktif",
        refresh,
    );
    // AHCI kendinden testi: yalnizca acilista (rescan'de tekrarlanmaz).
    // self_test son sektore yedek-GPT korumasi uygular; sistem disklerinde
    // test guvenle atlanir, scratch/QEMU disklerinde yaz+oku dogrulanir.
    if !refresh && drivers::ahci::ready() {
        let _ = drivers::ahci::self_test();
    }
    probe_storage_controller(
        DriverId::Nvme,
        drivers::nvme::detect,
        drivers::nvme::init,
        "NVMe denetleyicisi yok",
        "nvme init hatasi",
        "kontrolor hazir; blok okuma/yazma yolu henuz yok",
        refresh,
    );
    probe_storage_controller(
        DriverId::Sdmmc,
        drivers::sdmmc::detect,
        drivers::sdmmc::init,
        "SDHCI denetleyicisi yok",
        "sdhci init hatasi",
        "sd kart hazir; PIO blok okuma aktif, yazma desteklenmiyor",
        refresh,
    );
    probe_storage_controller(
        DriverId::VirtioBlk,
        drivers::virtio_blk::detect,
        drivers::virtio_blk::init,
        "virtio-blk aygiti yok",
        "virtio-blk init hatasi",
        "kontrolor hazir; blok okuma/yazma yolu henuz yok",
        refresh,
    );

    // Hatlara bagli suruculer: acilista lane raporlamis olmali;
    // profil disinda kaldiysalar Unavailable, kapsamdaki ama
    // raporlanmamis olanlar canli durum sorgusuyla kapanir.
    // Yeniden taramada da ayni canli sorgular tekrarlanir.
    if refresh {
        refresh_net();
        refresh_audio();
        refresh_usb();
        refresh_input();
        refresh_display();
    } else {
        composite_or_disabled(DriverId::Net, profile, "net", refresh_net);
        composite_or_disabled(DriverId::Audio, profile, "audio", refresh_audio);
        composite_or_disabled(DriverId::Usb, profile, "usb", refresh_usb);
        composite_or_disabled(DriverId::Input, profile, "hid", refresh_input);
        // Display her zaman BSP'de calisir; profil kisiti yok.
        if status::entry(DriverId::Display).phase == DriverPhase::Unknown {
            refresh_display();
        }
    }
}

/// Henuz raporlanmamis bilesik surucuyu kapatir: profil disindaysa
/// `Unavailable` (devre disi), kapsamdaysa canli durum sorgusu calisir.
fn composite_or_disabled(id: DriverId, profile: &str, lane: &str, probe: fn()) {
    if status::entry(id).phase != DriverPhase::Unknown {
        return;
    }
    if !profile_runs(profile, lane) {
        let t = status::probe_start(id);
        status::probe_unavailable(id, t, "profilde devre disi");
    } else {
        probe();
    }
}

/// detect()+init() cagrisi yapan depolama denetleyicisi probe'u.
fn probe_storage_controller(
    id: DriverId,
    detect: fn() -> bool,
    init: fn() -> bool,
    no_hw_desc: &'static str,
    init_err_desc: &'static str,
    ready_desc: &'static str,
    refresh: bool,
) {
    if !refresh && status::entry(id).phase != DriverPhase::Unknown {
        return;
    }
    let t = status::probe_start(id);
    if !detect() {
        status::probe_unavailable(id, t, no_hw_desc);
        return;
    }
    if init() {
        status::probe_ready_detail(id, t, ready_desc);
    } else {
        status::probe_error(id, t, status::ERR_INIT_FAILED, init_err_desc);
    }
}

// ---------------------------------------------------------------------------
// Yeniden tarama: canli durum sorgulari (yeniden init YOK)
// ---------------------------------------------------------------------------

fn refresh_net() {
    let t = status::probe_start(DriverId::Net);
    if drivers::e1000::is_ready() {
        status::probe_ready(DriverId::Net, t);
        return;
    }
    match drivers::e1000::init_outcome() {
        3 => status::probe_error(
            DriverId::Net,
            t,
            status::ERR_NO_DMA,
            "DMA ring havuzu ayrilamadi",
        ),
        4 => status::probe_error(
            DriverId::Net,
            t,
            status::ERR_INIT_FAILED,
            "ag surucusu baslatma hatasi",
        ),
        _ => status::probe_unavailable(DriverId::Net, t, "PCI/ISA ag adaptoru bulunamadi"),
    }
}

fn refresh_audio() {
    let t = status::probe_start(DriverId::Audio);
    if drivers::hda::is_ready() {
        status::probe_ready(DriverId::Audio, t);
        return;
    }
    match drivers::hda::init_outcome() {
        3 => status::probe_error(
            DriverId::Audio,
            t,
            status::ERR_NO_DMA,
            "HDA DMA havuzu ayrilamadi",
        ),
        4 => status::probe_error(
            DriverId::Audio,
            t,
            status::ERR_INIT_FAILED,
            "HDA denetleyici baslatma hatasi",
        ),
        _ => status::probe_unavailable(DriverId::Audio, t, "HDA yok; PC Speaker yedegi etkin"),
    }
}

fn refresh_display() {
    let t = status::probe_start(DriverId::Display);
    let caps = drivers::gpu::caps();
    if caps.vga_present {
        status::probe_ready(DriverId::Display, t);
    } else {
        status::probe_unavailable(DriverId::Display, t, "gpu aygiti bulunamadi");
    }
}

fn refresh_usb() {
    let t = status::probe_start(DriverId::Usb);
    if drivers::usb::controller_info().is_some() {
        status::probe_ready(DriverId::Usb, t);
    } else {
        status::probe_unavailable(DriverId::Usb, t, "USB host denetleyicisi bulunamadi");
    }
}

fn refresh_input() {
    let t = status::probe_start(DriverId::Input);
    if drivers::usb_hid::ready()
        || drivers::usb_hid::device_count() > 0
        || drivers::usb_touchscreen::max_x() > 0
        || drivers::ps2::present()
    {
        status::probe_ready(DriverId::Input, t);
    } else {
        status::probe_unavailable(DriverId::Input, t, "klavye/fare aygiti bulunamadi");
    }
}

/// Surucu durum ozetini seri gunluge yazar (teshis icin): once ozet
/// satiri, sonra her surucu icin tek satirlik detay ve varsa registry
/// (framework) durumlari.
pub fn log_status_summary() {
    let (ready, unavailable, error, other) = status::summary();
    kinfo!(
        "boot",
        "[DRIVER-STATUS] ozet: hazir={} kullanilamiyor={} hata={} diger={}\n",
        ready,
        unavailable,
        error,
        other
    );
    status::log_all_entries();
    drivers::framework::log_registry_states();
}

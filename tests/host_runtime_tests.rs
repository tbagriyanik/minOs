//! Host testleri: uygulama çalışma zamanı kontratları.
//!
//! Burada çekirdeğe bağlı olmayan, ancak GUI/donanım olmadan da
//! sınanabilen uygulama davranışları toplanır:
//!
//! * Ortak sürücü durum API'si (`drivers::api`) — standart log formatı,
//!   isim ölçekleme ve tanılama özeti.
//! * Kullanım ekranı (`apps::monitor`) — bellek baskısı, 30 sn geçmişi,
//!   sürücü başlatma süreleri, disk dolu / yazma hatası ve sayaç testleri
//!   `src/apps/monitor.rs` içindeki birim testlerinde kapsanır; burada
//!   geçmiş hazırlama (`history`) ile uçtan uca sayaç akışı doğrulanır.

use crate::{app_monitor as monitor, driver_api, driver_status};

#[test]
fn driver_api_log_format_matches_status_registry() {
    use driver_status::{DriverId, DriverPhase};
    // Aynı kayıt iki yoldan da aynı metni üretmeli: paylaşılan format.
    let line = driver_api::log_line(DriverId::Audio, DriverPhase::Unavailable, 12, 0, "no hda");
    assert_eq!(
        line,
        "[DRIVER-STATUS] id=audio phase=unavailable ms=12 desc=\"no hda\""
    );
    // Hata kodu yalnızca Error fazında yazılır.
    let err = driver_api::log_line(
        DriverId::Ahci,
        DriverPhase::Error,
        3,
        driver_status::ERR_NO_DMA,
        "dma",
    );
    assert!(err.contains("code=2"));
    assert!(err.ends_with("desc=\"dma\""));
    // Init satırı kısa isim kullanır ve ok alanı 0/1'dir.
    let init = driver_api::init_line("Intel HDA", 42, false);
    assert_eq!(init, "[DRIVER-INIT] name=audio us=42 ok=0");
}

#[test]
fn driver_api_summary_covers_all_drivers() {
    let line = driver_api::diag_line();
    assert!(line.starts_with("[DRIVER-STATUS] summary"));
    for id in driver_status::DriverId::ALL {
        assert!(line.contains(id.key()), "{} eksik", id.key());
    }
}

#[test]
fn monitor_history_snapshot_matches_ring_buffer() {
    // Geçmiş yığını 30 saniyelik pencerededir; `history()` dilim uzunlukları
    // her zaman tutarlı olmalı.
    let snap = monitor::history();
    assert_eq!(snap.mem().len(), snap.len);
    assert_eq!(snap.cpu().len(), snap.len);
    assert!(snap.len <= monitor::history_span_s() as usize);
    assert_eq!(
        monitor::history_span_s(),
        monitor::SAMPLE_INTERVAL_MS * 30 / 1000
    );
}

#[test]
fn sampling_counter_reset_does_not_emit_samples() {
    // Sayaç sıfırlandığında (saat geri döndü) örnek üretilmez ve
    // geçmiş yığını bozulmaz.
    assert_eq!(monitor::sample_tick(1_000_000, 500, 1_000), None);
    assert_eq!(monitor::sample_tick(u64::MAX, 0, 1_000), None);
    assert_eq!(monitor::elapsed_since(u64::MAX, 0), 0);
    // Sayaç taşması sınırı: u32 üstüne çıkmaz.
    assert_eq!(monitor::elapsed_since(0, u64::MAX), u64::from(u32::MAX));
}

#[test]
fn disk_and_memory_thresholds_use_same_unit_helpers() {
    // Bayt tabanlı bellek eşiği ve sektör tabanlı disk eşiği tutarlı
    // davranmalı: tam dolu → hata, bol alan → ok.
    let total = 512 * 1024 * 1024u64;
    assert!(monitor::check_write(total, 100, 4096).is_ok());
    // Proje sözleşmesinde %5 ve altı kritik, %10 ve altı uyarıdır.
    assert_eq!(
        monitor::pressure_level(total, total / 100),
        monitor::PressureLevel::Critical
    );
    assert_eq!(
        monitor::pressure_level(total, total / 200),
        monitor::PressureLevel::Critical
    );
    assert!(
        monitor::pressure_message(monitor::PressureLevel::Critical, false)
            .1
            .len()
            > 10
    );
}

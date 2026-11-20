//! Kullanım ekranı (Kontrol Paneli → İzleme) için ortak veri katmanı.
//!
//! Bu modül iki işi yapar:
//!
//! 1. **Ölçüm hattı:** Son 30 saniyelik RAM/CPU örneklerini (metin ve grafik
//!    çizimi için) hazırlar ve kullanım ekranının son render süresini ölçer.
//! 2. **Karar mantığı:** Bellek baskısı uyarısı, disk dolu / yazma hatası
//!    akışları, sayaç taşması normalizasyonu ve sürücü (driver) başlatma
//!    sürelerinin sıralaması gibi saf fonksiyonlar.
//!
//! Çizim ve donanım erişimi **yoktur**; bu nedenle modül host hedefinde de
//! derlenir ve deterministik olarak test edilir. Render maliyeti
//! `[MONITOR-COST]` seri satırıyla raporlanır.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[cfg(not(any(test, host_fs_tests)))]
use crate::apps::memory::{self as app_memory, history_len, HISTORY_CAPACITY};

#[cfg(any(test, host_fs_tests))]
use crate::app_memory::{self as app_memory, history_len, HISTORY_CAPACITY};

fn get_history(mem: &mut [usize; HISTORY_CAPACITY], cpu: &mut [usize; HISTORY_CAPACITY]) -> usize {
    app_memory::get_history(mem, cpu)
}

// ---------------------------------------------------------------------------
// Zaman ve sayaç yardımcıları
// ---------------------------------------------------------------------------

/// Geçmiş yığınına örnek kaydetme sıklığı (ms).
/// `HISTORY_CAPACITY` (30) örnek × 1 sn = son 30 saniye.
pub const SAMPLE_INTERVAL_MS: u64 = 1_000;

/// Depolanan örnek sayısına karşılık gelen saniye cinsinden kapsam.
pub fn history_span_s() -> u64 {
    HISTORY_CAPACITY as u64 * SAMPLE_INTERVAL_MS / 1_000
}

/// Monotonik milisaniye cinsinden şu anki zaman.
///
/// Çekirdekte TSC tabanlı zaman tabanı kullanılır; host testlerinde
/// deterministik sabit bir saat döner (testler makinenin sistem saatine
/// bağlı olmasın).
#[cfg(not(test))]
pub fn now_ms() -> u64 {
    crate::boot::time::monotonic_ms()
}

#[cfg(test)]
pub fn now_ms() -> u64 {
    // Deterministik test saati: gerçek sistem saatine bağlı değil.
    1_700_000_000_000
}

static LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);

/// Son örnekten bu yana geçen süre (ms).
///
/// Sayaç taşması ve sıfırlanma güvenlidir: `saturating_sub` ve `min` ile
/// monotonik olmayan (geri alınmış / sıfırlanmış) zaman damgaları `0`
/// döndürür, geriye dönük değerler çok büyük bir "süre" üretmez.
pub fn elapsed_since(last_ms: u64, now_ms: u64) -> u64 {
    now_ms.saturating_sub(last_ms).min(u64::from(u32::MAX))
}

/// Kareden bağımsız, sabit kadanslı örnekleme: son örnekten en az
/// `interval_ms` geçtiyse yeni örnek kaydeder ve kaydedilen süreyi döndürür.
///
/// Saat geriye dönerse (sıfırlama / taşma) sayaç yenilenir ve `None` döner;
/// böylece geçmiş yığınına sıfır süreli örnek yazılmaz.
pub fn sample_tick(last_ms: u64, now_ms: u64, interval_ms: u64) -> Option<u64> {
    if now_ms < last_ms {
        LAST_TICK_MS.store(now_ms, Ordering::Relaxed);
        return None;
    }
    if now_ms.saturating_sub(last_ms) >= interval_ms {
        LAST_TICK_MS.store(now_ms, Ordering::Relaxed);
        Some(elapsed_since(last_ms, now_ms))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Ölçüm hattı (son 30 saniye)
// ---------------------------------------------------------------------------

/// Geçmiş için tutulan RAM (MB) ve CPU (%) örnekleri.
pub struct HistorySnapshot {
    pub mem_mb: [usize; HISTORY_CAPACITY],
    pub cpu_pct: [usize; HISTORY_CAPACITY],
    /// Geçerli örnek sayısı (0..=HISTORY_CAPACITY).
    pub len: usize,
}

impl HistorySnapshot {
    pub fn empty() -> Self {
        Self {
            mem_mb: [0; HISTORY_CAPACITY],
            cpu_pct: [0; HISTORY_CAPACITY],
            len: 0,
        }
    }

    /// Geçerli örnek dilimlerini (en eski → en yeni) döndürür.
    pub fn mem(&self) -> &[usize] {
        &self.mem_mb[..self.len]
    }

    pub fn cpu(&self) -> &[usize] {
        &self.cpu_pct[..self.len]
    }
}

/// Bellek geçmişi anlık görüntüsünü hazırlar (çizim tarafı bunu tek çağrıda alır).
pub fn history() -> HistorySnapshot {
    let mut snap = HistorySnapshot::empty();
    snap.len = get_history(&mut snap.mem_mb, &mut snap.cpu_pct).min(HISTORY_CAPACITY);
    snap
}

/// Kullanım ekranının kendi render süresi (µs).
static MONITOR_RENDER_US: AtomicU64 = AtomicU64::new(0);
/// Kullanım ekranının son render maliyeti (bütçeye oranı, yüzde).
static MONITOR_RENDER_PCT: AtomicUsize = AtomicUsize::new(0);
/// Kullanım ekranının son ölçülen render maliyeti (µs, geçmiş ortalaması).
static MONITOR_AVG_US: AtomicU64 = AtomicU64::new(0);
static MONITOR_SAMPLES: AtomicU64 = AtomicU64::new(0);

/// İzleme ekranının son ölçülen render süresini döndürür (µs).
pub fn render_us() -> u64 {
    MONITOR_RENDER_US.load(Ordering::Relaxed)
}

/// İzleme ekranının tüm oturum boyunca ortalama render süresi (µs).
pub fn render_avg_us() -> u64 {
    MONITOR_AVG_US.load(Ordering::Relaxed)
}

/// İzleme ekranının render örnek sayısı.
pub fn render_samples() -> u64 {
    MONITOR_SAMPLES.load(Ordering::Relaxed)
}

/// Son render maliyetinin kare bütçesine oranı (%).
pub fn render_budget_pct() -> usize {
    MONITOR_RENDER_PCT.load(Ordering::Relaxed)
}

/// Bir kare bütçesi (µs) için render maliyetinin yüzdesini hesaplar.
pub fn cost_percent(render_us: u64, budget_us: u64) -> usize {
    let budget = budget_us.max(1);
    (render_us.saturating_mul(100) / budget).min(u32::MAX as u64) as usize
}

/// Ölçülen render süresini kaydeder ve `[MONITOR-COST]` seri satırını yazar.
///
/// En fazla 250 ms'lik değerler dikkate alınır; daha büyük değerler
/// (sayaç/ölçüm hatası) ortalama hesabını bozmasın diye kırpılır.
pub fn record_render(render_us: u64, budget_us: u64) {
    let capped = render_us.min(250_000);
    MONITOR_RENDER_US.store(capped, Ordering::Relaxed);
    let pct = cost_percent(capped, budget_us);
    MONITOR_RENDER_PCT.store(pct, Ordering::Relaxed);
    let prev_avg = MONITOR_AVG_US.load(Ordering::Relaxed);
    let n = MONITOR_SAMPLES.fetch_add(1, Ordering::Relaxed) + 1;
    // Kayan ortalama (n yerine sabit ağırlık): toplam taşmasını önler.
    let next_avg = if n <= 1 {
        capped
    } else {
        (prev_avg.saturating_mul(7).saturating_add(capped)) / 8
    };
    MONITOR_AVG_US.store(next_avg, Ordering::Relaxed);
    log_cost(capped, pct, next_avg, n);
}

fn log_cost(us: u64, pct: usize, avg: u64, n: u64) {
    #[cfg(not(test))]
    {
        crate::boot::console::serial_print("[MONITOR-COST] us=");
        crate::boot::console::serial_print_dec(us);
        crate::boot::console::serial_print(" budget_pct=");
        crate::boot::console::serial_print_dec(pct as u64);
        crate::boot::console::serial_print(" avg_us=");
        crate::boot::console::serial_print_dec(avg);
        crate::boot::console::serial_print(" samples=");
        crate::boot::console::serial_print_dec(n);
        crate::boot::console::serial_print("\n");
    }
    #[cfg(test)]
    {
        let _ = (us, pct, avg, n);
    }
}

// ---------------------------------------------------------------------------
// Bellek baskısı
// ---------------------------------------------------------------------------

/// Bellek baskı eşikleri (boş RAM yüzdesi).
pub const PRESSURE_CRITICAL_PCT: usize = 5;
pub const PRESSURE_WARN_PCT: usize = 10;

/// Bellek baskısı seviyesi.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressureLevel {
    Normal,
    Warn,
    Critical,
}

impl PressureLevel {
    pub fn label(self, english: bool) -> &'static str {
        match self {
            PressureLevel::Normal => "Normal",
            PressureLevel::Warn => {
                if english {
                    "Memory pressure"
                } else {
                    "Bellek baskısı"
                }
            }
            PressureLevel::Critical => {
                if english {
                    "Critical memory pressure"
                } else {
                    "Kritik bellek baskısı"
                }
            }
        }
    }

    pub fn is_pressured(self) -> bool {
        self != PressureLevel::Normal
    }
}

/// Toplam ve boş RAM baytından baskı seviyesini hesaplar.
///
/// Toplam bellek 0 ise (henüz raporlanmadı) baskı bildirilmez.
pub fn pressure_level(total_bytes: u64, free_bytes: u64) -> PressureLevel {
    if total_bytes == 0 {
        return PressureLevel::Normal;
    }
    let free = free_bytes.min(total_bytes);
    let free_pct = (free.saturating_mul(100) / total_bytes) as usize;
    if free_pct <= PRESSURE_CRITICAL_PCT {
        PressureLevel::Critical
    } else if free_pct <= PRESSURE_WARN_PCT {
        PressureLevel::Warn
    } else {
        PressureLevel::Normal
    }
}

static PRESSURE_ANNOUNCED: AtomicUsize = AtomicUsize::new(0);
/// Aynı baskı seviyesi için uyarının tekrar gösterilmesini engelleyen
/// bekleme süresi (ms).
pub const PRESSURE_NOTIFY_COOLDOWN_MS: u64 = 60_000;

/// Bir kez gösterilen kullanıcı uyarısının durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PressureNotice {
    pub level: PressureLevel,
    pub at_ms: u64,
}

/// Bellek baskısı için kullanıcı uyarısı gösterilmesi gerekiyorsa
/// seviyeyi döndürür.
///
/// Uyarılar başlık değiştiğinde (Normal → Warn → Critical) veya
/// `PRESSURE_NOTIFY_COOLDOWN_MS` dolduğunda yenilenir. Saat geriye dönerse
/// süre sıfırdan başlar (sayaç sıfırlanma testi).
pub fn pressure_notice(
    total_bytes: u64,
    free_bytes: u64,
    now_ms: u64,
    last: Option<PressureNotice>,
) -> Option<PressureNotice> {
    let level = pressure_level(total_bytes, free_bytes);
    if !level.is_pressured() {
        return None;
    }
    match last {
        None => Some(PressureNotice {
            level,
            at_ms: now_ms,
        }),
        Some(prev) => {
            if prev.level != level {
                return Some(PressureNotice {
                    level,
                    at_ms: now_ms,
                });
            }
            let elapsed = now_ms.saturating_sub(prev.at_ms);
            if now_ms < prev.at_ms || elapsed >= PRESSURE_NOTIFY_COOLDOWN_MS {
                Some(PressureNotice {
                    level,
                    at_ms: now_ms,
                })
            } else {
                None
            }
        }
    }
}

/// Masaüstü çekirdeğinin kullandığı durum: son bildirilen baskı seviyesi.
pub fn should_notify_pressure(total_bytes: u64, free_bytes: u64) -> bool {
    let level = pressure_level(total_bytes, free_bytes);
    let prev = PRESSURE_ANNOUNCED.load(Ordering::Relaxed);
    if !level.is_pressured() {
        if prev != 0 {
            PRESSURE_ANNOUNCED.store(0, Ordering::Relaxed);
        }
        return false;
    }
    let code = match level {
        PressureLevel::Warn => 1,
        PressureLevel::Critical => 2,
        PressureLevel::Normal => 0,
    };
    if prev == code {
        return false;
    }
    PRESSURE_ANNOUNCED.store(code, Ordering::Relaxed);
    true
}

/// Bellek baskısı bildirimi başlık/gövde metinleri (TR/EN).
pub fn pressure_message(level: PressureLevel, english: bool) -> (&'static str, &'static str) {
    match (level, english) {
        (PressureLevel::Critical, false) => (
            "Bellek baskısı kritik",
            "Boş RAM %5'in altına düştü. Kullanılmayan uygulamaları kapatın.",
        ),
        (PressureLevel::Critical, true) => (
            "Critical memory pressure",
            "Free RAM fell below 5%. Close applications you are not using.",
        ),
        (PressureLevel::Warn, _) if english => (
            "Memory pressure",
            "Free RAM fell below 10%. Consider closing applications.",
        ),
        _ => (
            "Bellek baskısı",
            "Boş RAM %10'un altına düştü. Uygulamaları kapatmayı düşünün.",
        ),
    }
}

// ---------------------------------------------------------------------------
// Disk dolu / yazma hatası akışları
// ---------------------------------------------------------------------------

/// Dosya yazımı için en az serbest alan oranı (%).
pub const DISK_FULL_RESERVE_PCT: usize = 3;
/// Küçük yazımlarda kesin dolu eşiği (bayt).
// Küçük yazımlarda korunacak tampon. 4 KiB altındaki yazımlar (ör. tek
// sektörlük metadata güncellemesi) düşük alan uyarısına rağmen denenebilir;
// daha büyük yazımlar rezerv alanı korumak için reddedilir.
pub const DISK_FULL_SLACK_BYTES: u64 = 4 * 1024;

/// Disk boş alan eşiği durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskSpace {
    Ok,
    /// Yazım yapılabilir ama %3'ün altında alan kaldı (güvenli biçimde
    /// gereken yerden fazlası yazılmamalı).
    Low,
    Full,
}

impl DiskSpace {
    pub fn label(self, english: bool) -> &'static str {
        match self {
            DiskSpace::Ok => {
                if english {
                    "Sufficient space"
                } else {
                    "Yeterli alan"
                }
            }
            DiskSpace::Low => {
                if english {
                    "Low disk space"
                } else {
                    "Disk alanı azalıyor"
                }
            }
            DiskSpace::Full => {
                if english {
                    "Disk full"
                } else {
                    "Disk dolu"
                }
            }
        }
    }
}

/// Kapasite ve serbest sektörden disk alanı durumunu hesaplar.
pub fn disk_space(total_sectors: u64, free_sectors: u64) -> DiskSpace {
    if total_sectors == 0 {
        return DiskSpace::Full;
    }
    let free = free_sectors.min(total_sectors);
    if free == 0 {
        return DiskSpace::Full;
    }
    let reserve = total_sectors.saturating_mul(DISK_FULL_RESERVE_PCT as u64) / 100;
    if free <= reserve.max(1) {
        DiskSpace::Low
    } else {
        DiskSpace::Ok
    }
}

/// Dosya yazımı için ön kontrol.
///
/// `Ok` → yazılabilir, `Err(DiskSpace)` → yazım engellenmeli. Küçük
/// yazımlar (≤ `DISK_FULL_SLACK_BYTES`) için yalnızca tamamen dolu disk
/// reddedilir; büyük yazımlarda serbest alan `DISK_FULL_RESERVE_PCT`
/// rezervinin üzerinde kalmalıdır.
pub fn check_write(
    total_sectors: u64,
    free_sectors: u64,
    write_bytes: u64,
) -> Result<(), DiskSpace> {
    let space = disk_space(total_sectors, free_sectors);
    match space {
        DiskSpace::Full => Err(DiskSpace::Full),
        DiskSpace::Low => {
            if write_bytes > DISK_FULL_SLACK_BYTES {
                Err(DiskSpace::Low)
            } else {
                Ok(())
            }
        }
        DiskSpace::Ok => {
            // `write_bytes` için çağıranların kullandığı sektör üst sınırını
            // ayrıca kontrol et. Böylece yeterli rezerv varken kapasiteyi
            // aşan büyük istekler Full olarak reddedilir.
            if write_bytes > free_sectors {
                Err(DiskSpace::Full)
            } else {
                Ok(())
            }
        }
    }
}

/// Disk yazımı hata türü.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteError {
    /// Boş alan yok.
    NoSpace,
    /// Aygıt yazmayı reddetti (salt okunur / donanım hatası).
    DeviceRefused,
    /// Cihaz yanıt vermedi (zaman aşımı, bağlantı koptu).
    Timeout,
    /// Araya giren hata (kesme, kısa yazım).
    Interrupted,
}

impl WriteError {
    pub fn message(self, english: bool) -> &'static str {
        match (self, english) {
            (WriteError::NoSpace, false) => "Disk dolu: yer açın ve yeniden deneyin.",
            (WriteError::NoSpace, true) => "Disk full: free some space and try again.",
            (WriteError::DeviceRefused, false) => {
                "Disk yazmayı reddetti: aygıt salt okunur olabilir."
            }
            (WriteError::DeviceRefused, true) => {
                "The disk refused the write: the device may be read-only."
            }
            (WriteError::Timeout, false) => {
                "Disk yanıt vermedi: bağlantı kopmuş olabilir, tekrar deneyin."
            }
            (WriteError::Timeout, true) => {
                "The disk did not respond: the connection may be lost, retry."
            }
            (WriteError::Interrupted, false) => "Yazım kesintiye uğradı: yeniden deneyin.",
            (WriteError::Interrupted, true) => "The write was interrupted: try again.",
        }
    }

    /// Kullanıcıya önerilen eylem (kurtarma yolu).
    pub fn recovery(self, english: bool) -> &'static str {
        match (self, english) {
            (WriteError::NoSpace, false) => "Geçici dosyaları veya çöp kutusunu temizleyin.",
            (WriteError::NoSpace, true) => "Delete temporary files or empty the recycle bin.",
            (WriteError::DeviceRefused, false) => {
                "Diski salt okunur olmayan biçimde yeniden bağlayın."
            }
            (WriteError::DeviceRefused, true) => "Remount the disk read-write.",
            (WriteError::Timeout, false) => {
                "Diski yeniden tarayın (Kontrol Paneli → Sürücüler → Yeniden Tara)."
            }
            (WriteError::Timeout, true) => "Rescan the disk (Control Panel → Drivers → Rescan).",
            (WriteError::Interrupted, false) => "İşlemi yeniden başlatın.",
            (WriteError::Interrupted, true) => "Restart the operation.",
        }
    }
}

/// Yazma hatası metinleri.
pub fn write_error(space: DiskSpace, _english: bool) -> WriteError {
    match space {
        DiskSpace::Full => WriteError::NoSpace,
        _ => WriteError::Interrupted,
    }
}

// ---------------------------------------------------------------------------
// Eşlik (sürücü) başlatma süreleri
// ---------------------------------------------------------------------------

/// Sürücü başlatma süresi satırı (UI ve seri rapor için).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriverTiming {
    pub name: &'static str,
    /// Başlatma süresi (µs).
    pub us: u64,
    pub ok: bool,
}

/// Eşlik sürelerinin özeti.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimingSummary {
    pub count: usize,
    pub total_us: u64,
    pub slowest_us: u64,
    /// En yavaş sürücünün adı (boşsa veri yok).
    pub slowest: &'static str,
    pub failed: usize,
    /// Sürücülerin toplam açılış süresinin bütçeye oranı (%).
    pub budget_pct: usize,
}

/// Başlatma sürelerini bütçeye göre değerlendirir.
pub fn timing_summary(list: &[DriverTiming], budget_us: u64) -> TimingSummary {
    let mut total = 0u64;
    let mut slowest = 0u64;
    let mut slowest_name = "";
    let mut failed = 0usize;
    for t in list {
        total = total.saturating_add(t.us);
        if t.us >= slowest {
            slowest = t.us;
            slowest_name = t.name;
        }
        if !t.ok {
            failed += 1;
        }
    }
    TimingSummary {
        count: list.len(),
        total_us: total,
        slowest_us: slowest,
        slowest: slowest_name,
        failed,
        budget_pct: cost_percent(total, budget_us),
    }
}

/// Başlatma süresini en yavaştan hızlıya sıralar (kopya üzerinde).
pub fn sort_slowest_first(list: &[DriverTiming]) -> alloc::vec::Vec<DriverTiming> {
    let mut out: alloc::vec::Vec<DriverTiming> = list.to_vec();
    // Küçük liste (≤16) için kararlı ekleme sıralaması yeterli ve alloc'dur.
    for i in 1..out.len() {
        let mut j = i;
        while j > 0 && out[j - 1].us < out[j].us {
            out.swap(j - 1, j);
            j -= 1;
        }
    }
    out
}

/// Süreyi µs → "1.2 ms" biçiminde yazmak için tam/salise bölümü.
pub fn split_ms(us: u64) -> (u64, u64) {
    (us / 1000, (us % 1000) / 100)
}

// ---------------------------------------------------------------------------
// Kullanım ekranı satır modeli
// ---------------------------------------------------------------------------

/// Kullanım ekranında (İzleme sekmesi) gösterilen tek satır.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonitorRow {
    pub label: &'static str,
    pub value: &'static str,
    pub severity: u8,
}

/// Uyarı öncelik sırası: 0 normal, 1 uyarı, 2 hata.
pub const SEV_NORMAL: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_FAIL: u8 = 2;

/// Bellek baskısı ve disk durumundan kullanım ekranı uyarı bandını döndürür.
pub fn banner(
    pressure: PressureLevel,
    disk: DiskSpace,
    english: bool,
) -> Option<(&'static str, u8)> {
    let critical = pressure == PressureLevel::Critical || disk == DiskSpace::Full;
    if critical {
        return Some((
            if english {
                "Warning: memory or disk is critically low"
            } else {
                "Uyarı: bellek veya disk kritik seviyede"
            },
            SEV_FAIL,
        ));
    }
    if pressure.is_pressured() || disk == DiskSpace::Low {
        return Some((
            if english {
                "Notice: resources are running low"
            } else {
                "Bildirim: kaynaklar azalıyor"
            },
            SEV_WARN,
        ));
    }
    None
}

/// Geçmişteki en yüksek değer (sparkline ölçeklemesi için).
pub fn history_max(values: &[usize]) -> usize {
    let mut max = 0usize;
    for &v in values {
        if v > max {
            max = v;
        }
    }
    max
}

/// Son `seconds` saniyeye karşılık gelen örnek penceresi (en yeni sonda).
pub fn window_len(len: usize, seconds: usize) -> usize {
    len.min(seconds).min(HISTORY_CAPACITY)
}

/// CPU yüzdesinin 0..=100 aralığına kırpılması (taşma koruması).
pub fn clamp_pct(v: usize) -> usize {
    v.min(100)
}

static SAMPLING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Örnekleme hattının çalışıp çalışmadığı (Masaüstü çekirdeği açar).
pub fn set_sampling_active(active: bool) {
    SAMPLING_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn sampling_active() -> bool {
    SAMPLING_ACTIVE.load(Ordering::Relaxed)
}

/// Son örnek zamanı (ms); kadans denetimi için paylaşılır.
pub fn last_tick_ms() -> u64 {
    LAST_TICK_MS.load(Ordering::Relaxed)
}

/// Örnek sayısı; 0 ise grafik "henüz örnek yok" durumunu gösterir.
pub fn samples() -> usize {
    history_len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_handles_counter_reset_and_overflow() {
        assert_eq!(elapsed_since(1000, 1500), 500);
        // Sayaç sıfırlandı: geriye dönük zaman negatif süre üretmez.
        assert_eq!(elapsed_since(5000, 100), 0);
        // u64 taşması: saturating ve u32 sınırı.
        assert_eq!(elapsed_since(0, u64::MAX), u64::from(u32::MAX));
    }

    #[test]
    fn sample_tick_is_rate_limited_and_reset_safe() {
        let mut last = 1_000u64;
        assert_eq!(sample_tick(last, 1_100, 1_000), None);
        let d = sample_tick(last, 2_100, 1_000);
        assert_eq!(d, Some(1_100));
        last = 2_100;
        // Saat geri döndü: örnek yok, sayaç yenilendi.
        assert_eq!(sample_tick(last, 10, 1_000), None);
        assert_eq!(last_tick_ms(), 10);
    }

    #[test]
    fn pressure_levels_follow_free_percentage() {
        let total = 100 * 1024u64;
        assert_eq!(pressure_level(total, total), PressureLevel::Normal);
        assert_eq!(pressure_level(total, total * 8 / 100), PressureLevel::Warn);
        assert_eq!(
            pressure_level(total, total * 3 / 100),
            PressureLevel::Critical
        );
        // Sıfır boş alan, bellek tükenmiş demektir.
        assert_eq!(pressure_level(total, total / 50), PressureLevel::Critical);
        assert_eq!(pressure_level(0, 0), PressureLevel::Normal);
        assert_eq!(pressure_level(total, 0), PressureLevel::Critical);
    }

    #[test]
    fn pressure_notice_cooldown_and_reset() {
        let total = 1000u64;
        let low = 50u64; // %5 → kritik
        let first = pressure_notice(total, low, 10_000, None).expect("ilk uyarı");
        assert_eq!(first.level, PressureLevel::Critical);
        // Cooldown içinde tekrar gösterilmez.
        assert!(pressure_notice(total, low, 20_000, Some(first)).is_none());
        // Cooldown doldu → yeniden gösterilir.
        let again = pressure_notice(
            total,
            low,
            10_000 + PRESSURE_NOTIFY_COOLDOWN_MS,
            Some(first),
        );
        assert!(again.is_some());
        // Seviye değişti (kritik → uyarı) → beklemeden gösterilir.
        let warn = pressure_notice(total, 80, 10_500, Some(first));
        assert_eq!(warn.map(|n| n.level), Some(PressureLevel::Warn));
        // Sıfırlanan saat: süre negatif olmaz, uyarı yeniden gösterilir.
        let reset = pressure_notice(total, low, 5, Some(first));
        assert!(reset.is_some());
        // Normal seviyede uyarı yok.
        assert!(pressure_notice(total, total, 90_000, None).is_none());
    }

    #[test]
    fn disk_space_and_write_precheck() {
        let total = 100_000u64;
        assert_eq!(disk_space(total, total / 2), DiskSpace::Ok);
        assert_eq!(disk_space(total, 100), DiskSpace::Low);
        assert_eq!(disk_space(total, 0), DiskSpace::Full);
        assert_eq!(disk_space(0, 0), DiskSpace::Full);

        // Küçük yazım %3 eşiğinde kabul edilir.
        assert_eq!(check_write(total, 100, 512), Ok(()));
        // Büyük yazım reddedilir.
        assert_eq!(check_write(total, 100, 8 * 1024), Err(DiskSpace::Low));
        // Hiç yer yok.
        assert_eq!(check_write(total, 0, 1), Err(DiskSpace::Full));
        // Yazım serbest alandan büyük.
        assert_eq!(check_write(total, 10_000, 100 * 1024), Err(DiskSpace::Full));
        // %3..%100 arasındaki serbest alanda büyük yazım kabul edilir.
        assert_eq!(check_write(total, 10_000, 8 * 1024), Ok(()));
    }

    #[test]
    fn write_error_messages_are_localized_and_helpful() {
        for err in [
            WriteError::NoSpace,
            WriteError::DeviceRefused,
            WriteError::Timeout,
            WriteError::Interrupted,
        ] {
            assert!(!err.message(false).is_empty());
            assert!(!err.message(true).is_empty());
            assert_ne!(err.message(false), err.message(true));
            assert!(!err.recovery(false).is_empty());
            assert!(!err.recovery(true).is_empty());
        }
        assert_eq!(write_error(DiskSpace::Full, false), WriteError::NoSpace);
    }

    #[test]
    fn timing_summary_orders_and_flags_failures() {
        let list = [
            DriverTiming {
                name: "PCI",
                us: 120,
                ok: true,
            },
            DriverTiming {
                name: "NVMe",
                us: 0,
                ok: false,
            },
            DriverTiming {
                name: "ATA",
                us: 4_500,
                ok: true,
            },
        ];
        let s = timing_summary(&list, 10_000);
        assert_eq!(s.count, 3);
        assert_eq!(s.total_us, 4_620);
        assert_eq!(s.slowest, "ATA");
        assert_eq!(s.failed, 1);
        assert_eq!(s.budget_pct, 46);

        let sorted = sort_slowest_first(&list);
        assert_eq!(sorted[0].name, "ATA");
        assert_eq!(sorted[1].name, "PCI");
        assert_eq!(sorted[2].name, "NVMe");
    }

    #[test]
    fn split_ms_formats_milliseconds() {
        assert_eq!(split_ms(0), (0, 0));
        assert_eq!(split_ms(999), (0, 9));
        assert_eq!(split_ms(1_250), (1, 2));
        assert_eq!(split_ms(12_000), (12, 0));
    }

    #[test]
    fn banner_prioritizes_critical_resources() {
        assert!(banner(PressureLevel::Normal, DiskSpace::Ok, false).is_none());
        let (_, sev) = banner(PressureLevel::Warn, DiskSpace::Ok, false).unwrap();
        assert_eq!(sev, SEV_WARN);
        let (_, sev) = banner(PressureLevel::Critical, DiskSpace::Ok, false).unwrap();
        assert_eq!(sev, SEV_FAIL);
        let (_, sev) = banner(PressureLevel::Normal, DiskSpace::Full, true).unwrap();
        assert_eq!(sev, SEV_FAIL);
    }

    #[test]
    fn history_window_and_max_are_bounded() {
        assert_eq!(window_len(0, 30), 0);
        assert_eq!(window_len(30, 30), 30);
        assert_eq!(window_len(10, 0), 0);
        assert_eq!(history_max(&[]), 0);
        assert_eq!(history_max(&[3, 9, 1]), 9);
        assert_eq!(clamp_pct(usize::MAX), 100);
        assert_eq!(clamp_pct(42), 42);
    }

    #[test]
    fn cost_percent_is_bounded() {
        assert_eq!(cost_percent(0, 16_000), 0);
        assert_eq!(cost_percent(8_000, 16_000), 50);
        assert_eq!(cost_percent(32_000, 16_000), 200);
        assert_eq!(cost_percent(10, 0), 1000);
        record_render(1_500, 16_000);
        assert_eq!(render_us(), 1_500);
        assert!(render_budget_pct() >= 9);
        assert!(render_samples() >= 1);
        assert!(render_avg_us() >= 1);
    }

    #[test]
    fn history_snapshot_exposes_valid_slices() {
        let snap = history();
        assert!(snap.len <= HISTORY_CAPACITY);
        assert_eq!(snap.mem().len(), snap.len);
        assert_eq!(snap.cpu().len(), snap.len);
    }
}

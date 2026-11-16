//! Açılış süresi profilleme.
//!
//! Limine çekirdeği devraldığı andan itibaren geçen süreyi ölçer ve her
//! önyükleme aşamasının ne kadar sürdüğünü seri konsola yazar:
//!
//! ```text
//! [BOOT-TIME] pmm=70ms toplam=176ms
//! [BOOT-TIME] SURUM=4070ms asama=15
//! ```
//!
//! Zaman tabanı (PIT/TSC) `init_timebase()` ile kurulduktan sonra
//! `begin()` çağrılır; bu yüzden ölçüm Limine'den devralma anını kapsar,
//! firmware süresini içermez.
//!
//! Son işaret zamanı global tutulur; böylece ölçüm farklı modüller
//! (`boot::subsystems`, `gui::event_loop`) arasında kesintisiz devam eder.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::boot::console::{serial_print, serial_print_dec};

/// `begin()` çağrısındaki monotonik zaman (ms).
static BOOT_ORIGIN_MS: AtomicU64 = AtomicU64::new(0);
/// Son işaretin monotonik zamanı (ms).
static LAST_MARK_MS: AtomicU64 = AtomicU64::new(0);
/// Zamanlama etkin mi?
static TIMING_ENABLED: AtomicU64 = AtomicU64::new(0);
/// Ölçülen aşama sayısı.
static PHASE_COUNT: AtomicU64 = AtomicU64::new(0);
/// En uzun tek aşamanın süresi (ms).
static MAX_PHASE_MS: AtomicU64 = AtomicU64::new(0);

/// Hedeflenen çekirdek açılış süresi (masaüstüne hazır).
pub const BOOT_BUDGET_MS: u64 = 3_000;
/// Bu süreyi aşan aşamalar "yavaş faz" olarak işaretlenir.
const SLOW_PHASE_MS: u64 = 500;

/// Ölçüm başlangıcını işaretler. Zaman tabanı hazır olduktan hemen sonra
/// çağrılmalıdır.
pub fn begin() {
    let now = monotonic_ms();
    BOOT_ORIGIN_MS.store(now, Ordering::Release);
    LAST_MARK_MS.store(now, Ordering::Release);
    TIMING_ENABLED.store(1, Ordering::Release);
    serial_print("[BOOT-TIME] Zamanlama basladi.\n");
}

/// Açılıştan bu yana geçen süre (ms). Ölçüm başlamadıysa `0`.
pub fn since_boot_ms() -> u64 {
    if TIMING_ENABLED.load(Ordering::Acquire) == 0 {
        return 0;
    }
    monotonic_ms().saturating_sub(BOOT_ORIGIN_MS.load(Ordering::Acquire))
}

/// Aşama sürelerini ölçen yardımcı.
///
/// Her `mark()` çağrısı o ana kadar geçen süreyi ve o aşamanın kendi
/// süresini seri konsola yazar.
pub struct PhaseTimer;

impl PhaseTimer {
    pub fn new() -> Self {
        PhaseTimer
    }

    /// `label` aşamasının bittiğini bildirir ve süreyi yazar.
    pub fn mark(&mut self, label: &str) {
        let now = since_boot_ms();
        let last = LAST_MARK_MS.swap(now, Ordering::AcqRel);
        let delta = now.saturating_sub(last);
        PHASE_COUNT.fetch_add(1, Ordering::AcqRel);
        MAX_PHASE_MS.fetch_max(delta, Ordering::AcqRel);
        serial_print("[BOOT-TIME] ");
        serial_print(label);
        serial_print("=");
        serial_print_dec(delta);
        serial_print("ms toplam=");
        serial_print_dec(now);
        serial_print("ms\n");
        if delta >= SLOW_PHASE_MS {
            serial_print("[BOOT-TIME] UYARI yavas faz: ");
            serial_print(label);
            serial_print("=");
            serial_print_dec(delta);
            serial_print("ms (Eşik=");
            serial_print_dec(SLOW_PHASE_MS);
            serial_print("ms)\n");
        }
    }

    /// Ölçülen aşama sayısı.
    pub fn phase_count(&self) -> u32 {
        PHASE_COUNT.load(Ordering::Acquire) as u32
    }
}

impl Default for PhaseTimer {
    fn default() -> Self {
        Self::new()
    }
}

/// Açılış süresinin özet satırını yazar (toplam süre, aşama sayısı,
/// en yavaş aşama ve bütçe durumu).
pub fn report_total() {
    let total = since_boot_ms();
    serial_print("[BOOT-TIME] SURUM=");
    serial_print_dec(total);
    serial_print("ms asama=");
    serial_print_dec(PHASE_COUNT.load(Ordering::Acquire));
    serial_print(" en-yavas=");
    serial_print_dec(MAX_PHASE_MS.load(Ordering::Acquire));
    serial_print("ms butce=");
    serial_print_dec(BOOT_BUDGET_MS);
    serial_print("ms durum=");
    if total <= BOOT_BUDGET_MS {
        serial_print("OK\n");
    } else {
        serial_print("ASIM\n");
    }
}

fn monotonic_ms() -> u64 {
    crate::boot::time::monotonic_ms()
}

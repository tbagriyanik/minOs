//! Ortak sürücü hata/timeout/yeniden deneme yardımcıları.
//!
//! Disk, ağ ve USB sürücüleri aynı politikayı paylaşır: sınırlı deneme
//! sayısı, üstel geri çekilme ve tek bir durum noktası. Bu modül saf
//! mantık içerir; zaman kaynağı çağırandan inject edilir, böylece host
//! birim testlerinde donanımsız doğrulanabilir.
//!
//! TR: Disk, ağ ve USB sürücüleri aynı hata/yeniden deneme politikasını
//! kullanır. Zaman enjeksiyonu sayesinde test edilebilir.

/// Tek bir işlem için yeniden deneme politikası.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// İlk deneme dahil toplam deneme üst sınırı.
    pub max_attempts: u32,
    /// İlk deneme sonrası taban bekleme süresi (ms).
    pub base_delay_ms: u64,
    /// Üstel büyümenin üst sınırı (ms).
    pub max_delay_ms: u64,
}

impl RetryPolicy {
    /// Varsayılan politika: 3 deneme, 1ms → 2ms → 4ms (üst sınır 50ms).
    pub const DEFAULT: Self = Self {
        max_attempts: 3,
        base_delay_ms: 1,
        max_delay_ms: 50,
    };

    pub const fn new(max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            max_attempts,
            base_delay_ms,
            max_delay_ms,
        }
    }

    /// `attempt` (0 tabanlı, başarısız deneme sayısı) sonrası bekleme.
    pub fn delay_for(&self, failed_attempts: u32) -> u64 {
        let shift = failed_attempts.min(20);
        let delay = self.base_delay_ms.saturating_mul(1u64 << shift);
        delay.min(self.max_delay_ms)
    }

    /// `failed_attempts` başarısız denemeden sonra yeniden denenecek mi?
    /// `failed_attempts == 0` hiçbir deneme yapılmadığı anlamına gelir;
    /// "yeniden deneme" ancak en az bir başarısız deneme sonrası söz
    /// konusu olur. `max_attempts` toplam deneme bütçesini (ilk deneme
    /// dahil) verir.
    pub fn should_retry(&self, failed_attempts: u32) -> bool {
        failed_attempts > 0 && failed_attempts < self.max_attempts
    }
}

/// Bir işlemin çalışma zamanı hata sayaçları.
///
/// Sürücüler her başarısız alt işlemde `note_error` çağırır; UI ve
/// günlük bu sayaçları okuyarak "hata sayısı" gösterebilir.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpStats {
    /// Toplam başarısız deneme sayısı.
    pub attempts: u64,
    /// Son hata kodu (sürücüye özel, örn. status::ERR_* veya BTST/USB kodu).
    pub last_error: u16,
    /// Son hatanın oluştuğu monotonik zaman (ms).
    pub last_error_ms: u64,
    /// Üst üste art arda gelen hata sayısı (başarıda sıfırlanır).
    pub consecutive_errors: u32,
}

impl OpStats {
    pub const fn new() -> Self {
        Self {
            attempts: 0,
            last_error: 0,
            last_error_ms: 0,
            consecutive_errors: 0,
        }
    }

    /// Bir hatayı kaydeder; `now_ms` çağıranın enjekte ettiği zaman.
    pub fn note_error(&mut self, code: u16, now_ms: u64) {
        self.attempts = self.attempts.saturating_add(1);
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        self.last_error = code;
        self.last_error_ms = now_ms;
    }

    /// Başarılı bir işlem art arda hata serisini keser.
    pub fn note_success(&mut self) {
        self.consecutive_errors = 0;
    }

    /// Sayaçları sıfırlar (yeniden başlatma sonrası).
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// `cond` doğru olana veya `timeout_ms` dolana kadar `now`'u yoklar.
///
/// Donanım bekleme döngüleri için: her çağrıda `cond` yoklanır, süre
/// aşılırsa `false` döner. Uyku yoktur; çağıran taraf gerekiyorsa kendi
/// `cpu_relax`/io delay'ini ekler.
///
/// `now_ms` enjeksiyonu sayesinde host testlerinde deterministik çalışır.
pub fn poll_with_timeout<F>(now_ms: impl Fn() -> u64, timeout_ms: u64, mut cond: F) -> bool
where
    F: FnMut() -> bool,
{
    let start = now_ms();
    loop {
        if cond() {
            return true;
        }
        if now_ms().saturating_sub(start) >= timeout_ms {
            return false;
        }
    }
}

/// `poll_with_timeout`'un sınırlı deneme + geri çekilmeli varyantı.
///
/// `cond` her denemede yeniden çalıştırılır; başarısız denemeler arasında
/// `delay_fn(policy.delay_for(failed))` çağrılır (gerçek uyku çağıranın
/// sorumluluğundadır — örn. `cpu_relax` döngüsü veya `hpet` uyuması).
/// Toplam süre yine `timeout_ms` ile sınırlıdır.
pub fn poll_with_retries<F, D>(
    now_ms: impl Fn() -> u64,
    policy: &RetryPolicy,
    timeout_ms: u64,
    mut cond: F,
    mut delay_fn: D,
) -> bool
where
    F: FnMut() -> bool,
    D: FnMut(u64),
{
    let start = now_ms();
    let mut failed = 0u32;
    loop {
        if cond() {
            return true;
        }
        if now_ms().saturating_sub(start) >= timeout_ms {
            return false;
        }
        // `failed` tamamlanan yeniden deneme sayısıdır; az önce başarısız
        // olan deneme failed+1'inci denemedir.
        if !policy.should_retry(failed + 1) {
            return false;
        }
        failed += 1;
        delay_fn(policy.delay_for(failed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn retry_policy_delays_grow_and_cap() {
        let p = RetryPolicy::new(4, 1, 8);
        assert_eq!(p.delay_for(0), 1);
        assert_eq!(p.delay_for(1), 2);
        assert_eq!(p.delay_for(2), 4);
        assert_eq!(p.delay_for(3), 8);
        assert_eq!(p.delay_for(10), 8, "delay must cap at max_delay_ms");
    }

    #[test]
    fn retry_policy_attempt_budget() {
        let p = RetryPolicy::new(3, 1, 50);
        // max_attempts=3 → ilk + 2 yeniden deneme.
        assert!(!p.should_retry(0)); // 0 başarısız → ilk deneme yapılır (retry değil)
        assert!(p.should_retry(1)); // 1 başarısız → tekrar dene
        assert!(p.should_retry(2)); // 2 başarısız → son tekrar
        assert!(!p.should_retry(3)); // bütçe doldu
    }

    #[test]
    fn op_stats_tracks_errors_and_resets() {
        let mut s = OpStats::new();
        s.note_error(3, 100);
        s.note_error(4, 200);
        assert_eq!(s.attempts, 2);
        assert_eq!(s.last_error, 4);
        assert_eq!(s.last_error_ms, 200);
        assert_eq!(s.consecutive_errors, 2);
        s.note_success();
        assert_eq!(s.consecutive_errors, 0);
        s.note_error(5, 300);
        s.reset();
        assert_eq!(s.attempts, 0);
        assert_eq!(s.last_error, 0);
    }

    #[test]
    fn poll_with_timeout_respects_budget() {
        // Deterministik saat: her çağrıda 10ms ilerler.
        let now = Cell::new(0u64);
        let now_fn = || {
            now.set(now.get() + 10);
            now.get()
        };
        // Koşul hiç sağlanmaz → timeout sonrası false.
        assert!(!poll_with_timeout(now_fn, 25, || false));

        let now2 = Cell::new(0u64);
        let now_fn2 = || {
            now2.set(now2.get() + 10);
            now2.get()
        };
        // 3. yoklamada koşul sağlanır (30ms > 25ms? hayır: 10,20,30 → 30>=25).
        let mut calls = 0u32;
        let ok = poll_with_timeout(now_fn2, 100, || {
            calls += 1;
            calls >= 3
        });
        assert!(ok);
        assert_eq!(calls, 3);
    }

    #[test]
    fn poll_with_retries_budget_exhausted_before_timeout() {
        // max_attempts=1: tek deneme hakkı var, cond hiç sağlanmaz ve
        // zaman budget'i geniş olduğundan "bütçe doldu" nedeniyle false
        // dönmeli; delay_fn asla çağrılmamalı.
        let policy = RetryPolicy::new(1, 1, 50);
        let mut calls = 0u32;
        let mut delays = 0u64;
        let ok = poll_with_retries(
            || 0,
            &policy,
            1_000_000,
            || {
                calls += 1;
                false
            },
            |d| {
                delays += d;
            },
        );
        assert!(!ok);
        assert_eq!(calls, 1);
        assert_eq!(delays, 0, "bütçe tükenince gecikme beklemesi olmaz");
    }

    #[test]
    fn poll_with_retries_succeeds_after_retries() {
        // max_attempts=3: ilk iki deneme başarısız, üçüncü denemede koşul
        // sağlanır; toplam süre zaman budget'i içinde kalmalı.
        let policy = RetryPolicy::new(3, 1, 50);
        let mut calls = 0u32;
        let mut delays = 0u64;
        let start = 10_000u64;
        let now = Cell::new(start);
        let ok = poll_with_retries(
            || {
                let t = now.get();
                now.set(t + 5);
                t
            },
            &policy,
            100,
            || {
                calls += 1;
                calls >= 3
            },
            |d| {
                delays += d;
            },
        );
        assert!(ok);
        assert_eq!(calls, 3);
        assert!(delays > 0, "yeniden denemeler arasında gecikme uygulanmalı");
        assert!(
            now.get() - start < 100,
            "toplam süre zaman budget'i içinde kalmalı"
        );
    }
}

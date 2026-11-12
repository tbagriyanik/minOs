//! Gerçek zaman bütçesi ve iptal mekanizması.
//!
//! Render ve update işlemleri için süre sınırı sağlar. `TimeBudget`, başlatıldığı
//! andan itibaren geçen süreyi izler ve `remaining_ms()` ile kalan süreyi döndürür.
//! `CancellationToken`, uzun süren döngülerin (raycasting, düşman AI, vb.) erken
//! çıkışını sağlayarak ana döngünün bloke olmasını engeller.
//!
//! no_std + alloc ortamında çalışır. `monotonic_ms()` üzerinden zaman ölçer.

/// Zaman sağlayıcı işareti. Varsayılan olarak `crate::monotonic_ms` kullanılır;
/// testlerde geçersiz kılınabilir.
pub type ClockFn = fn() -> u64;

#[cfg(not(test))]
static mut CLOCK_FN: ClockFn = crate::monotonic_ms;
#[cfg(test)]
static mut CLOCK_FN: ClockFn = || 0;

/// Geçerli zamanı döndürür (yapılandırılmış saat işlevi üzerinden).
#[inline(always)]
fn now_ms() -> u64 {
    unsafe { CLOCK_FN() }
}

/// Testlerde saati geçersiz kılar. `#[cfg(test)]` dışında etkisizdir.
#[cfg(test)]
pub fn set_clock_for_test(f: ClockFn) {
    unsafe {
        CLOCK_FN = f;
    }
}

/// Testlerde saati varsayılana döndürür. `#[cfg(test)]` dışında etkisizdir.
#[cfg(test)]
pub fn reset_clock_for_test() {
    unsafe {
        CLOCK_FN = crate::monotonic_ms;
    }
}

/// Bir işlem için zaman bütçesi. `new(budget_ms)` ile oluşturulur ve
/// `check()` ile bütçe aşımı tespit edilir.
#[derive(Clone, Copy, Debug)]
pub struct TimeBudget {
    budget_ms: u64,
    start_ms: u64,
}

impl TimeBudget {
    /// Belirtilen milisaniye bütçesiyle yeni bir bütçe oluşturur.
    pub fn new(budget_ms: u64) -> Self {
        Self {
            budget_ms,
            start_ms: now_ms(),
        }
    }

    /// Bütçenin başlangıç zamanını sıfırlar (yeniden başlatma).
    pub fn reset(&mut self) {
        self.start_ms = now_ms();
    }

    /// Geçen süreyi milisaniye cinsinden döndürür.
    pub fn elapsed_ms(&self) -> u64 {
        now_ms().saturating_sub(self.start_ms)
    }

    /// Kalan süreyi milisaniye cinsinden döndürür (sıfırın altında olamaz).
    pub fn remaining_ms(&self) -> u64 {
        self.budget_ms.saturating_sub(self.elapsed_ms())
    }

    /// Bütçe aşılmışsa `true` döndürür.
    pub fn is_expired(&self) -> bool {
        self.elapsed_ms() >= self.budget_ms
    }

    /// Bütçe hala devam ediyorsa `true` döndürür.
    pub fn is_active(&self) -> bool {
        !self.is_expired()
    }

    /// Bütçenin milisaniye cinsinden sınırını döndürür.
    pub fn budget_ms(&self) -> u64 {
        self.budget_ms
    }
}

/// İptal sinyali. `cancel()` çağrıldığında `is_cancelled()` true döner.
/// Uzun süren işlemler bu bayrağı düzenli olarak kontrol ederek erken çıkabilir.
#[derive(Clone, Copy, Debug)]
pub struct CancellationToken {
    cancelled: bool,
}

impl CancellationToken {
    /// İptal edilmemiş bir token oluşturur.
    pub fn new() -> Self {
        Self { cancelled: false }
    }

    /// İşlemi iptal eder.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// İptal edilip edilmediğini döndürür.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// İptal edildiyse `true`, aksi halde `false` döndürür (kısa yol).
    pub fn should_stop(&self) -> bool {
        self.cancelled
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Render ve update işlemleri için birleşik koruma yapısı.
/// Hem zaman bütçesi hem de iptal token'ını içerir.
#[derive(Clone, Copy, Debug)]
pub struct OperationGuard {
    budget: TimeBudget,
    token: CancellationToken,
}

impl OperationGuard {
    /// Belirtilen bütçe ile yeni bir işlem koruması oluşturur.
    pub fn new(budget_ms: u64) -> Self {
        Self {
            budget: TimeBudget::new(budget_ms),
            token: CancellationToken::new(),
        }
    }

    /// Kalan süreyi döndürür.
    pub fn remaining_ms(&self) -> u64 {
        self.budget.remaining_ms()
    }

    /// Bütçe aşılmışsa veya iptal edilmişse `true` döndürür.
    pub fn should_stop(&self) -> bool {
        self.budget.is_expired() || self.token.is_cancelled()
    }

    /// Bütçe aşımını kontrol eder ve aşılmışsa `Err` döndürür.
    pub fn check_budget(&self) -> Result<(), BudgetExceeded> {
        if self.budget.is_expired() {
            Err(BudgetExceeded)
        } else {
            Ok(())
        }
    }

    /// İptal sinyalini gönderir.
    pub fn cancel(&mut self) {
        self.token.cancel();
    }

    /// İptal edildi mi?
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Bütçeyi sıfırlar.
    pub fn reset_budget(&mut self) {
        self.budget.reset();
    }
}

/// Bütçe aşımı hatası.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetExceeded;

impl core::fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("time budget exceeded")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_not_expired_immediately() {
        let b = TimeBudget::new(1000);
        assert!(!b.is_expired());
        assert!(b.is_active());
        assert_eq!(b.remaining_ms(), 1000);
    }

    #[test]
    fn test_token_cancel() {
        let mut t = CancellationToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
        assert!(t.should_stop());
    }

    #[test]
    fn test_guard_should_stop_on_cancel() {
        let mut g = OperationGuard::new(1000);
        assert!(!g.should_stop());
        g.cancel();
        assert!(g.should_stop());
    }
}

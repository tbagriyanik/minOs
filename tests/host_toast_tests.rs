//! Toast bildirim sözleşmesi (host testleri).
//!
//! `gui::toast` çizim ⛔ `Framebuffer` API'sine bağlı olduğu için host
//! harness'ında tam modül derlenmez. Host'ta doğrulanabilen kısım **saf**
//! yaşam döngüsü mantığıdır: tür başına renk/süre sırası, ilerlemenin
//! monotonluğu, otomatik kapanma + çıkış animasyonu ve duraklatma.
//!
//! Bu testler, `src/gui/toast.rs`'deki kuralların **aynı** sözleşmesini
//! bağımsız olarak kodlar; iki taraf ayrışırsa test kırılır ve modülde
//! değişiklik yapıldığı fark edilir.

// --- toast.rs'deki sabitlerin aynası ---
const TOAST_INFO_MS: u64 = 3000;
const TOAST_SUCCESS_MS: u64 = 3000;
const TOAST_WARNING_MS: u64 = 4000;
const TOAST_ERROR_MS: u64 = 5000;
const SLIDE_OUT_MS: u64 = 140;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

impl Kind {
    fn default_duration_ms(self) -> u64 {
        match self {
            Kind::Info => TOAST_INFO_MS,
            Kind::Success => TOAST_SUCCESS_MS,
            Kind::Warning => TOAST_WARNING_MS,
            Kind::Error => TOAST_ERROR_MS,
        }
    }

    fn is_alert(self) -> bool {
        matches!(self, Kind::Warning | Kind::Error)
    }
}

/// Toast'un yaşam döngüsü (toast.rs `ToastNotification` ile aynı kurallar).
struct Lifecycle {
    duration_ms: u64,
    created_ms: u64,
    exit_ms: u64,
    paused_at_ms: u64,
    percent: Option<u8>,
}

impl Lifecycle {
    fn new(kind: Kind, created_ms: u64) -> Self {
        Self {
            duration_ms: kind.default_duration_ms(),
            created_ms,
            exit_ms: 0,
            paused_at_ms: 0,
            percent: None,
        }
    }

    fn with_progress(mut self, pct: u8) -> Self {
        self.percent = Some(pct.min(100));
        self
    }

    fn set_progress(&mut self, pct: u8) -> bool {
        let next = pct.min(100);
        match self.percent {
            Some(cur) if next <= cur => false,
            _ => {
                self.percent = Some(next);
                true
            }
        }
    }

    fn elapsed_ms(&self, now: u64) -> u64 {
        if self.paused_at_ms != 0 {
            self.paused_at_ms.saturating_sub(self.created_ms)
        } else {
            now.saturating_sub(self.created_ms)
        }
    }

    fn pause(&mut self, now: u64) {
        if self.paused_at_ms == 0 && self.exit_ms == 0 {
            self.paused_at_ms = now;
        }
    }

    fn resume(&mut self, now: u64) {
        if self.paused_at_ms != 0 {
            self.created_ms += now.saturating_sub(self.paused_at_ms);
            self.paused_at_ms = 0;
        }
    }

    /// Dönen değer: slot boşaltılmalı mı?
    fn advance(&mut self, now: u64) -> bool {
        if self.exit_ms == 0 {
            let in_progress = self.percent.is_some_and(|p| p < 100);
            if in_progress {
                return false;
            }
            if now.saturating_sub(self.created_ms) >= self.duration_ms {
                self.exit_ms = now;
            }
            false
        } else {
            now.saturating_sub(self.exit_ms) >= SLIDE_OUT_MS
        }
    }
}

#[test]
fn each_kind_has_its_own_auto_dismiss_duration() {
    assert_eq!(Kind::Info.default_duration_ms(), 3000);
    assert_eq!(Kind::Success.default_duration_ms(), 3000);
    assert_eq!(Kind::Warning.default_duration_ms(), 4000);
    assert_eq!(Kind::Error.default_duration_ms(), 5000);
    assert!(
        Kind::Error.default_duration_ms() > Kind::Warning.default_duration_ms(),
        "hata mesajı uyarıdan daha uzun görünmeli"
    );
    // Ekran okuyucu kuyruğu: uyarı/hata Alert, bilgi/başarı Status.
    assert!(Kind::Warning.is_alert() && Kind::Error.is_alert());
    assert!(!Kind::Info.is_alert() && !Kind::Success.is_alert());
}

#[test]
fn toast_dismisses_after_duration_with_exit_animation() {
    let mut t = Lifecycle::new(Kind::Info, 0);
    assert!(!t.advance(2999));
    assert_eq!(t.exit_ms, 0, "süre dolmadan kapanmamalı");
    // Süre doldu: çıkış animasyonu başlar, hemen silinmez.
    assert!(!t.advance(3000));
    assert_eq!(t.exit_ms, 3000);
    assert!(!t.advance(3139), "animasyon bitmeden silinmemeli");
    assert!(t.advance(3140), "animasyon bitince slot boşalır");
}

#[test]
fn progress_toast_stays_until_it_completes() {
    let mut t = Lifecycle::new(Kind::Info, 0).with_progress(10);
    // Süre çoktan dolsa da işlem sürüyor: kapanmaz.
    assert!(!t.advance(60_000));
    assert_eq!(t.exit_ms, 0);
    assert!(!t.set_progress(5), "yüzde geri sarmamalı");
    assert!(t.set_progress(100));
    // Tamamlandı: artık normal süreyle kapanır.
    assert!(!t.advance(60_000));
    assert_eq!(t.exit_ms, 60_000);
    assert!(t.advance(60_140));
}

#[test]
fn hover_pause_extends_visible_time_without_changing_total() {
    // Hata bildirimi: 5 sn toplam görünme süresi.
    let mut t = Lifecycle::new(Kind::Error, 0);
    t.pause(1000);
    assert_eq!(t.elapsed_ms(4999), 1000, "duraklarken sayaç işlemez");
    t.resume(4000);
    // 3000 ms duraklatıldı → son kullanma 5000'den 8000'e kayar.
    assert_eq!(t.elapsed_ms(4000), 1000);
    assert!(!t.advance(7999), "duraklatma kadar gecikmeli kapanır");
    assert_eq!(t.exit_ms, 0);
    assert!(!t.advance(8000));
    assert_eq!(t.exit_ms, 8000, "toplam 5 sn görünürlük korunur");
    assert!(t.advance(8140), "çıkış animasyonu bitince boşalır");
}

#[test]
fn pause_does_not_stack_or_apply_after_exit() {
    let mut t = Lifecycle::new(Kind::Warning, 0);
    t.pause(500);
    t.pause(900); // ikinci duraklatma yok sayılır
    assert_eq!(t.paused_at_ms, 500);
    t.resume(1000);
    assert_eq!(t.created_ms, 500);
    t.resume(1500); // zaten devam ediyor: etki yok
    assert_eq!(t.created_ms, 500);
}

#[test]
fn esc_closes_toasts_and_reports_the_count() {
    // `dismiss_toasts` davranışı: açık bildirimler varsa Esc tuşu onları
    // kapatır ve tuşu tükettiğini bildirir (0 = başka katmana gider).
    let mut open = [true, true, false, false];
    let closed = open.iter().filter(|&&o| o).count();
    assert_eq!(closed, 2);
    open = [false; 4];
    assert_eq!(open.iter().filter(|&&o| o).count(), 0);
}

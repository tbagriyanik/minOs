//! Bağımsız watchdog mekanizması.
//!
//! Ana render döngüsünden bağımsız olarak çalışır. Her izlenen uygulama için
//! bir "son etkinlik zamanı" tutar ve bu zaman `WATCHDOG_TIMEOUT_MS`'ı aştığında
//! uygulamayı donmuş olarak işaretler ve sonlandırır.
//!
//! Watchdog, APIC zamanlayıcı kesmesinde `timer_tick()` ile ilerler; yalnızca
//! atomik/kısa kritik bölümlerde durum günceller ve donmuş adayları kuyruğa
//! alır. Sonlandırma ve GUI mutasyonu ana döngüde `drain_frozen()` ile yapılır.
//!
//! no_std + alloc ortamında çalışır.

/// Zaman sağlayıcı işareti. Varsayılan olarak `crate::monotonic_ms` kullanılır;
/// testlerde geçersiz kılınabilir.
pub type ClockFn = fn() -> u64;

#[cfg(not(test))]
static mut CLOCK_FN: ClockFn = crate::boot::time::monotonic_ms;
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
        CLOCK_FN = || 0;
    }
}

use crate::arch::x86_64::sync::SpinMutex;

/// Varsayılan watchdog zaman aşımı: 5 saniye.
pub const WATCHDOG_TIMEOUT_MS: u64 = 5_000;

/// Maksimum izlenen pencere sayısı.
const MAX_WATCHED_WINDOWS: usize = 16;

/// Donmuş bir pencerenin kaydı.
#[derive(Clone, Copy, Debug)]
pub struct FrozenWindow {
    pub idx: usize,
    pub app_id: usize,
    pub idle_ms: u64,
}

/// İzlenen bir pencerenin durumu.
#[derive(Clone, Copy, Debug)]
struct WatchedWindow {
    /// Pencere indeksi.
    idx: usize,
    /// Uygulama kimliği.
    app_id: usize,
    /// Son etkinlik zamanı (ms).
    last_activity_ms: u64,
    /// İzleme etkin mi?
    active: bool,
    /// Donmuş olarak işaretlendi mi?
    flagged_frozen: bool,
}

/// Watchdog durumu. Tüm izlenen pencereleri ve yapılandırmayı tutar.
#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    timeout_ms: u64,
    windows: [WatchedWindow; MAX_WATCHED_WINDOWS],
    watch_count: usize,
    total_terminations: u64,
    last_tick_ms: u64,
}

impl Watchdog {
    /// Varsayılan zaman aşımıyla yeni bir watchdog oluşturur.
    pub const fn new() -> Self {
        Self::with_timeout(WATCHDOG_TIMEOUT_MS)
    }

    /// Özel zaman aşımıyla yeni bir watchdog oluşturur.
    pub const fn with_timeout(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            windows: [WatchedWindow {
                idx: 0,
                app_id: 0,
                last_activity_ms: 0,
                active: false,
                flagged_frozen: false,
            }; MAX_WATCHED_WINDOWS],
            watch_count: 0,
            total_terminations: 0,
            last_tick_ms: 0,
        }
    }

    /// Zaman aşımını ayarlar.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Bir pencereyi izleme listesine ekler (veya günceller).
    pub fn watch(&mut self, idx: usize, app_id: usize, now_ms: u64) {
        // Zaten izleniyorsa güncelle.
        for w in self.windows.iter_mut().take(self.watch_count) {
            if w.idx == idx && w.app_id == app_id {
                w.last_activity_ms = now_ms;
                w.active = true;
                w.flagged_frozen = false;
                return;
            }
        }

        // Yeni pencere ekle.
        if self.watch_count < MAX_WATCHED_WINDOWS {
            let w = &mut self.windows[self.watch_count];
            w.idx = idx;
            w.app_id = app_id;
            w.last_activity_ms = now_ms;
            w.active = true;
            w.flagged_frozen = false;
            self.watch_count += 1;
        }
    }

    /// Bir pencerenin etkinlik zamanını günceller.
    pub fn heartbeat(&mut self, idx: usize, app_id: usize, now_ms: u64) {
        for w in self.windows.iter_mut().take(self.watch_count) {
            if w.idx == idx && w.app_id == app_id {
                w.last_activity_ms = now_ms;
                w.active = true;
                w.flagged_frozen = false;
                return;
            }
        }
        // Henüz izlenmiyorsa ekle.
        self.watch(idx, app_id, now_ms);
    }

    /// Bir penceyi izleme listesinden çıkar.
    pub fn unwatch(&mut self, idx: usize) {
        let mut found = None;
        for (i, window) in self.windows.iter().enumerate().take(self.watch_count) {
            if window.idx == idx {
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found {
            // Son elemanla değiştir ve sayacı azalt.
            let last = self.watch_count - 1;
            self.windows[i] = self.windows[last];
            self.windows[last] = WatchedWindow {
                idx: 0,
                app_id: 0,
                last_activity_ms: 0,
                active: false,
                flagged_frozen: false,
            };
            self.watch_count = last;
        }
    }

    /// Tüm izlemeyi temizler.
    pub fn clear(&mut self) {
        for w in self.windows.iter_mut() {
            *w = WatchedWindow {
                idx: 0,
                app_id: 0,
                last_activity_ms: 0,
                active: false,
                flagged_frozen: false,
            };
        }
        self.watch_count = 0;
    }

    /// Watchdog tick'i çalıştırır. Donmuş pencereleri tespit eder,
    /// `on_freeze` callback'i ile bildirir ve donmuş pencerelerin listesini döndürür.
    ///
    /// `on_freeze(idx, app_id, idle_ms)`: bir pencere donmuş olduğunda çağrılır.
    /// Geriye `true` döndürürse pencere sonlandırılmaya aday olarak işaretlenir.
    ///
    /// Geriye donmuş pencere kayıtları döner; çağrıcı bunları sonlandırabilir.
    pub fn tick<F>(&mut self, now_ms: u64, mut on_freeze: F) -> heapless::Vec<FrozenWindow, 16>
    where
        F: FnMut(usize, usize, u64) -> bool,
    {
        self.last_tick_ms = now_ms;
        let mut frozen: heapless::Vec<FrozenWindow, 16> = heapless::Vec::new();

        for w in self.windows.iter_mut().take(self.watch_count) {
            if !w.active {
                continue;
            }

            let idle_time = now_ms.saturating_sub(w.last_activity_ms);

            if idle_time > self.timeout_ms && !w.flagged_frozen {
                let should_terminate = on_freeze(w.idx, w.app_id, idle_time);
                if should_terminate {
                    w.flagged_frozen = true;
                    self.total_terminations = self.total_terminations.saturating_add(1);
                    let _ = frozen.push(FrozenWindow {
                        idx: w.idx,
                        app_id: w.app_id,
                        idle_ms: idle_time,
                    });
                }
            }
        }

        frozen
    }

    /// Donmuş olarak işaretlenmiş bir pencerenin izlemesini temizler
    /// (sonlandırıldıktan sonra çağrılmalı).
    pub fn clear_frozen(&mut self, idx: usize) {
        for w in self.windows.iter_mut().take(self.watch_count) {
            if w.idx == idx {
                w.active = false;
                w.flagged_frozen = false;
                w.last_activity_ms = 0;
            }
        }
    }

    /// Toplam sonlandırma sayısını döndürür.
    pub fn total_terminations(&self) -> u64 {
        self.total_terminations
    }

    /// İzlenen pencere sayısını döndürür.
    pub fn watch_count(&self) -> usize {
        self.watch_count
    }

    /// Zaman aşımını döndürür.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Son tick zamanını döndürür.
    pub fn last_tick_ms(&self) -> u64 {
        self.last_tick_ms
    }
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new()
    }
}

/// APIC zamanlayıcı bağlamında çalışan watchdog ve ertelenmiş dondurma kuyruğu.
/// Kesme işleyicisi yalnızca kısa kritik bölümlerde `try_lock` kullanır; ana
/// döngü kilidi tutuyorsa o tik atlanır, deadlock oluşmaz.
struct SharedWatchdog {
    watchdog: Watchdog,
    pending: heapless::Vec<FrozenWindow, 16>,
}

impl SharedWatchdog {
    const fn new() -> Self {
        Self {
            watchdog: Watchdog::with_timeout(WATCHDOG_TIMEOUT_MS),
            pending: heapless::Vec::new(),
        }
    }
}

static SHARED: SpinMutex<SharedWatchdog> = SpinMutex::new(SharedWatchdog::new());

/// Açık bir pencere için etkinlik bildirir (ana döngü bağlamı).
pub fn heartbeat(idx: usize, app_id: usize) {
    let now_ms = now_ms();
    let mut shared = SHARED.lock();
    shared.watchdog.heartbeat(idx, app_id, now_ms);
}

/// Pencereyi izleme dışı bırakır (kapatma/sonlandırma yollarında çağrılır).
pub fn unwatch(idx: usize) {
    let mut shared = SHARED.lock();
    shared.watchdog.unwatch(idx);
}

/// Dondurma olayını kuyruktan temizler (terminate_app sonrası çağrılır).
pub fn clear_frozen(idx: usize) {
    let mut shared = SHARED.lock();
    shared.watchdog.clear_frozen(idx);
}

/// APIC timer interrupt'ından çağrılır. Donanım kesmesi bağlamında çalışır;
/// burada yalnızca durum güncellenir ve donmuş pencereler kuyruğa alınır.
/// Sonlandırma kararı ve GUI durumu değişikliği ana döngüye aittir.
pub fn timer_tick() {
    let Some(mut shared) = SHARED.try_lock() else {
        return;
    };
    let now_ms = now_ms();
    let (cur_app, cur_win, _) = crate::apps::guard::current_context();
    let frozen = shared.watchdog.tick(now_ms, |idx, app_id, _idle_ms| {
        // Yalnızca o anda korunan uygulama bağlamı dondurma adayı olabilir.
        // Bu, ana döngü bir uygulamanın çiziminde takıldığında diğer sağlıklı
        // pencerelerin de yanlışlıkla sonlandırılmasını engeller.
        cur_app != crate::apps::guard::NO_APP
            && app_id == cur_app
            && (cur_win == crate::apps::guard::NO_WINDOW || cur_win == idx)
    });
    for fw in frozen {
        let _ = shared.pending.push(fw);
    }
}

/// Ana döngünün işlemesi gereken bekleyen dondurma olaylarını boşaltır.
pub fn drain_frozen() -> heapless::Vec<FrozenWindow, 16> {
    let mut shared = SHARED.lock();
    core::mem::replace(&mut shared.pending, heapless::Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_heartbeat_and_timeout() {
        let mut wd = Watchdog::with_timeout(100);
        let now = 1000;
        wd.watch(0, 1, now);
        assert_eq!(wd.watch_count(), 1);

        // 50 ms sonra heartbeat: henüz zaman aşımı yok.
        wd.tick(1050, |_, _, _| true);
        assert_eq!(wd.total_terminations(), 0);

        // 200 ms sonra (toplam 250 ms idle > 100 ms timeout): donmuş.
        wd.tick(1250, |_, _, _| true);
        assert_eq!(wd.total_terminations(), 1);
    }

    #[test]
    fn test_unwatch_removes() {
        let mut wd = Watchdog::with_timeout(100);
        wd.watch(0, 1, 1000);
        wd.unwatch(0);
        assert_eq!(wd.watch_count(), 0);
    }

    #[test]
    fn test_clear_frozen() {
        let mut wd = Watchdog::with_timeout(100);
        wd.watch(0, 1, 1000);
        wd.tick(1200, |_, _, _| true);
        assert_eq!(wd.total_terminations(), 1);
        wd.clear_frozen(0);
        // Yeniden izlenebilir olmalı.
        wd.watch(0, 1, 2000);
        wd.tick(2150, |_, _, _| true);
        assert_eq!(wd.total_terminations(), 2);
    }
}

//! Basit animasyon yardımcıları.
//!
//! Tüm GUI animasyonları milisaniye tabanlı ilerleme (0..=1) ve easing
//! fonksiyonları üzerinden çalışır; kare hızından bağımsız deterministik
//! davranış sağlanır. Durum süreleri milisaniye cinsindendir.

/// Azaltılmış hareket (reduced motion) tercihi. Açıkken dekoratif
/// animasyonlar (kayma, açılış) anında tamamlanır; işlevsel durum
/// geçişleri (ör. toast yaşam döngüsü) normal çalışmaya devam eder.
static REDUCED_MOTION: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[inline(always)]
pub fn reduced_motion() -> bool {
    REDUCED_MOTION.load(core::sync::atomic::Ordering::Relaxed)
}

#[inline(always)]
pub fn set_reduced_motion(value: bool) {
    REDUCED_MOTION.store(value, core::sync::atomic::Ordering::Relaxed);
}

/// `elapsed_ms`/`duration_ms` oranından 0..=1 ilerleme üretir.
#[inline(always)]
pub fn progress(elapsed_ms: u64, duration_ms: u64) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    (elapsed_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
}

/// ease-out quad: hızlı başlar, yumuşak durur. Kaydırma ve giriş
/// animasyonlarının varsayılan eğrisidir.
#[inline(always)]
pub fn ease_out_quad(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u
}

/// ease-in-out quad: hem başlangıçta hem bitişte yumuşaktır.
#[inline(always)]
pub fn ease_in_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        let u = -2.0 * t + 2.0;
        1.0 - u * u / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress(0, 100), 0.0);
        assert_eq!(progress(50, 100), 0.5);
        assert_eq!(progress(200, 100), 1.0);
        assert_eq!(progress(10, 0), 1.0);
    }

    #[test]
    fn easing_endpoints_and_midpoint() {
        assert_eq!(ease_out_quad(0.0), 0.0);
        assert_eq!(ease_out_quad(1.0), 1.0);
        assert!(ease_out_quad(0.5) > 0.5);
        assert_eq!(ease_in_out_quad(0.0), 0.0);
        assert_eq!(ease_in_out_quad(1.0), 1.0);
        assert!((ease_in_out_quad(0.5) - 0.5).abs() < 0.001);
    }
}

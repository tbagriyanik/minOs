//! Aero Snap bölge algılama ve hedef geometrisi.
//!
//! Bu modül **saf** mantıktır: çizim, pencere yöneticisi veya olay döngüsüne
//! bağımlı değildir. Böylece hem önizleme kutusu hem de bırakma işlemi aynı
//! kaynaktan beslenir ve "önizleme ile sonuç aynı" garantisi korunur.
//!
//! Davranış (Windows 11 Aero Snap ile uyumlu):
//!
//! - Kenara sürükle (sol/sağ/üst/alt) -> **yarım ekran**
//! - Köşeye sürükle -> **çeyrek ekran**
//! - Üst kenara sürükle -> **tam ekran**
//!
//! Host testlerinde `tests/host_tests.rs` üzerinden doğrudan sınanır.

/// Aero Snap hedefi.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SnapPosition {
    /// Sol yarı ekran.
    Left,
    /// Sağ yarı ekran.
    Right,
    /// Tam ekran (üst kenar).
    Top,
    /// Alt yarı ekran (alt kenar).
    Bottom,
    /// Sol üst çeyrek.
    TopLeft,
    /// Sağ üst çeyrek.
    TopRight,
    /// Sol alt çeyrek.
    BottomLeft,
    /// Sağ alt çeyrek.
    BottomRight,
}

/// Sol/sağ/üst kenar toleransı (piksel).
pub const EDGE_THRESHOLD: usize = 4;
/// Alt kenar toleransı (piksel).
pub const BOTTOM_THRESHOLD: usize = 5;

/// Fare konumundan Aero Snap hedefini belirler.
///
/// `usable_h`, görev çubuğu hariç kullanılabilir ekran yüksekliğidir.
/// Kenar bölgelerinde kenar → yarım, köşe → çeyrek ekran döner.
///
/// # Örnek
///
/// ```
/// ignore
/// use crate::gui::snap::{snap_zone, SnapPosition};
/// // Sol kenarın ortası -> sol yarı ekran
/// assert_eq!(snap_zone(1, 300, 1024, 740), Some(SnapPosition::Left));
/// ```
pub fn snap_zone(mx: usize, my: usize, screen_w: usize, usable_h: usize) -> Option<SnapPosition> {
    if screen_w == 0 || usable_h == 0 {
        return None;
    }

    let left = mx <= EDGE_THRESHOLD;
    let right = mx >= screen_w.saturating_sub(EDGE_THRESHOLD + 1);
    let top = my <= EDGE_THRESHOLD;
    let bottom = my >= usable_h.saturating_sub(BOTTOM_THRESHOLD);

    // Köşeler önce değerlendirilir: köşe -> çeyrek ekran.
    if top && left {
        return Some(SnapPosition::TopLeft);
    }
    if top && right {
        return Some(SnapPosition::TopRight);
    }
    if bottom && left {
        return Some(SnapPosition::BottomLeft);
    }
    if bottom && right {
        return Some(SnapPosition::BottomRight);
    }

    // Düz kenarlar -> yarım (üst kenar tam ekran).
    if top {
        return Some(SnapPosition::Top);
    }
    if bottom {
        return Some(SnapPosition::Bottom);
    }
    if left {
        return Some(SnapPosition::Left);
    }
    if right {
        return Some(SnapPosition::Right);
    }

    None
}

/// Verilen hedef için pencere dikdörtgenini döndürür.
///
/// Sonuç `(x, y, genişlik, yükseklik)` biçimindedir ve `snap_window`
/// uygulamasıyla birebir aynı geometriyi üretir; böylece sürükleme
/// önizlemesi bırakma sonucuyla çakışmaz.
///
/// `usable_h`, görev çubuğu hariç kullanılabilir ekran yüksekliğidir.
pub fn snap_rect(
    pos: SnapPosition,
    screen_w: usize,
    usable_h: usize,
) -> (usize, usize, usize, usize) {
    let half_w = screen_w / 2;
    let half_h = usable_h / 2;
    match pos {
        SnapPosition::Top => (0, 0, screen_w, usable_h),
        SnapPosition::Bottom => (0, half_h, screen_w, usable_h.saturating_sub(half_h)),
        SnapPosition::Left => (0, 0, half_w, usable_h),
        SnapPosition::Right => (half_w, 0, screen_w.saturating_sub(half_w), usable_h),
        SnapPosition::TopLeft => (0, 0, half_w, half_h),
        SnapPosition::TopRight => (half_w, 0, screen_w.saturating_sub(half_w), half_h),
        SnapPosition::BottomLeft => (0, half_h, half_w, usable_h.saturating_sub(half_h)),
        SnapPosition::BottomRight => (
            half_w,
            half_h,
            screen_w.saturating_sub(half_w),
            usable_h.saturating_sub(half_h),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 1024;
    const H: usize = 740; // 768 - 28 (görev çubuğu)

    #[test]
    fn edge_gives_half_screen_and_corner_gives_quarter() {
        // Sol kenar (köşe değil) -> sol yarı ekran
        assert_eq!(snap_zone(1, 300, W, H), Some(SnapPosition::Left));
        // Sağ kenar (köşe değil) -> sağ yarı ekran
        assert_eq!(snap_zone(W - 2, 300, W, H), Some(SnapPosition::Right));
        // Üst kenar (köşe değil) -> tam ekran
        assert_eq!(snap_zone(500, 1, W, H), Some(SnapPosition::Top));
        // Alt kenar (köşe değil) -> alt yarı ekran
        assert_eq!(snap_zone(500, H - 1, W, H), Some(SnapPosition::Bottom));

        // Köşeler -> çeyrek ekran
        assert_eq!(snap_zone(1, 1, W, H), Some(SnapPosition::TopLeft));
        assert_eq!(snap_zone(W - 2, 1, W, H), Some(SnapPosition::TopRight));
        assert_eq!(snap_zone(1, H - 1, W, H), Some(SnapPosition::BottomLeft));
        assert_eq!(
            snap_zone(W - 2, H - 1, W, H),
            Some(SnapPosition::BottomRight)
        );
    }

    #[test]
    fn interior_has_no_snap_zone() {
        assert_eq!(snap_zone(500, 300, W, H), None);
        assert_eq!(snap_zone(200, 100, W, H), None);
    }

    #[test]
    fn degenerate_screen_is_safe() {
        assert_eq!(snap_zone(0, 0, 0, 0), None);
        assert_eq!(snap_zone(0, 0, 1024, 0), None);
    }

    #[test]
    fn snap_rect_matches_expected_geometry() {
        assert_eq!(snap_rect(SnapPosition::Top, W, H), (0, 0, W, H));
        assert_eq!(
            snap_rect(SnapPosition::Bottom, W, H),
            (0, H / 2, W, H - H / 2)
        );
        assert_eq!(snap_rect(SnapPosition::Left, W, H), (0, 0, W / 2, H));
        assert_eq!(
            snap_rect(SnapPosition::Right, W, H),
            (W / 2, 0, W - W / 2, H)
        );
        assert_eq!(snap_rect(SnapPosition::TopLeft, W, H), (0, 0, W / 2, H / 2));
        assert_eq!(
            snap_rect(SnapPosition::BottomRight, W, H),
            (W / 2, H / 2, W - W / 2, H - H / 2)
        );
    }

    #[test]
    fn every_zone_has_a_rect_inside_the_usable_area() {
        let zones = [
            SnapPosition::Top,
            SnapPosition::Bottom,
            SnapPosition::Left,
            SnapPosition::Right,
            SnapPosition::TopLeft,
            SnapPosition::TopRight,
            SnapPosition::BottomLeft,
            SnapPosition::BottomRight,
        ];
        for z in zones {
            let (x, y, w, h) = snap_rect(z, W, H);
            assert!(w > 0 && h > 0, "{z:?} boş dikdörtgen verdi");
            assert!(x + w <= W, "{z:?} ekranın sağını taşıyor");
            assert!(y + h <= H, "{z:?} görev çubuğunun altına taşıyor");
        }
    }
}

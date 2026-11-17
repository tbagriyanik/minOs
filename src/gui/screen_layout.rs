//! Ekran geometrisi ve açılış/kapanış/hata ekranı metinleri.
//!
//! Çizim kodu (`gui::screens`) ile giriş işleme aynı dikdörtgenleri
//! kullanır; bu modül saf hesaplama içerdiğinden host testlerinde de
//! derlenir. `self_test()`, desteklenen çözünürlüklerde tüm dikdörtgenlerin
//! ekran sınırları içinde kaldığını doğrular (GUI smoke testi).

use super::theme;

/// Dikdörtgen: (x, y, genişlik, yükseklik).
pub type Rect = (usize, usize, usize, usize);

/// GUI'nin desteklediği en küçük çözünürlük (pencere/taskbar yerleşimi bu
/// eşiğin altında anlamlı değildir; smoke testleri buradan başlar).
pub const MIN_SCREEN_W: usize = 640;
pub const MIN_SCREEN_H: usize = 480;

fn clamp_to(value: usize, limit: usize) -> usize {
    value.min(limit.max(1))
}

/// Hoş geldin panelinin ekrana ortalı dikdörtgeni.
pub fn welcome_panel(width: usize, height: usize) -> Rect {
    let w = clamp_to(
        520.min(width.saturating_sub(24)).max(320),
        width.max(MIN_SCREEN_W),
    );
    let cols = theme_columns(w);
    let rows = theme::ThemeId::ALL.len().div_ceil(cols);
    let card_h = 46;
    let theme_h = rows * card_h + (rows.saturating_sub(1)) * 10;
    let reserve_h = 190; // dil, başlık, alt bilgi ve düğme alanı.
    let h = clamp_to(
        (reserve_h + theme_h)
            .max(310)
            .min(height.saturating_sub(24))
            .max(260),
        height.max(MIN_SCREEN_H),
    );
    (
        width.saturating_sub(w) / 2,
        height.saturating_sub(h) / 2,
        w,
        h,
    )
}

/// Tema kartlarının satır sayısını belirleyen sütun sayısı.
pub fn theme_columns(panel_width: usize) -> usize {
    if panel_width >= 420 {
        4
    } else if panel_width >= 350 {
        3
    } else {
        2
    }
}

/// Dil seçeneklerinden birinin dikdörtgeni (0 = Türkçe, 1 = English).
pub fn welcome_language_rect(width: usize, height: usize, index: usize) -> Rect {
    let (x, y, panel_w, _) = welcome_panel(width, height);
    // Dar ekranlarda iki dil düğmesi panel içinde kalacak şekilde küçülür;
    // normal çözünürlüklerde özgün 126×32 geometri korunur.
    let (button_w, step) = if panel_w >= 350 {
        (126, 140)
    } else {
        let w = 126.min(panel_w.saturating_sub(66) / 2).max(1);
        (w, w + 10)
    };
    (x + 28 + index * step, y + 84, button_w, 32)
}

/// Tema kartının dikdörtgeni.
pub fn welcome_theme_rect(width: usize, height: usize, index: usize) -> Rect {
    let (x, y, w, h) = welcome_panel(width, height);
    let cols = theme_columns(w);
    let gap_x = 10;
    let gap_y = 10;
    let card_h = 46;
    let available_w = w.saturating_sub(56).max(1);
    let card_w = available_w.saturating_sub(gap_x * (cols - 1)) / cols;
    let rows = theme::ThemeId::ALL.len().div_ceil(cols);
    let theme_total_h = rows * card_h + (rows.saturating_sub(1)) * gap_y;
    let theme_top = y + 148;
    let bottom_limit = y + h.saturating_sub(76);
    let theme_y = theme_top.min(bottom_limit.saturating_sub(theme_total_h));
    let col = index % cols;
    let row = index / cols;
    (
        x + 28 + col * (card_w + gap_x),
        theme_y + row * (card_h + gap_y),
        card_w,
        card_h,
    )
}

/// "Başla" düğmesinin dikdörtgeni.
pub fn welcome_continue_rect(width: usize, height: usize) -> Rect {
    let (x, y, w, h) = welcome_panel(width, height);
    (x + w.saturating_sub(168), y + h.saturating_sub(50), 140, 36)
}

/// Kısayol ipucunun çizim konumu (panel sol üstüne göre).
pub fn welcome_hint_pos(width: usize, height: usize) -> (usize, usize) {
    let (x, y, _, h) = welcome_panel(width, height);
    (x + 28, y + h.saturating_sub(40))
}

/// Önyükleme hatası bilgi kutusu (başlık şeridi 52 px).
pub fn boot_error_box(width: usize, height: usize) -> Rect {
    let w = clamp_to(640.min(width.saturating_sub(48)).max(320), width.max(1));
    let h = clamp_to(
        300.min(height.saturating_sub(96)).max(180),
        height.saturating_sub(52).max(1),
    );
    let x = width.saturating_sub(w) / 2;
    let y = 52 + height.saturating_sub(52 + h) / 2;
    (x, y, w, h)
}

/// Kapanış/yeniden başlatma bilgi kutusu.
pub fn shutdown_box(width: usize, height: usize) -> Rect {
    let w = clamp_to(560.min(width.saturating_sub(48)).max(320), width.max(1));
    let h = clamp_to(
        220.min(height.saturating_sub(96)).max(160),
        height.saturating_sub(52).max(1),
    );
    let x = width.saturating_sub(w) / 2;
    let y = 52 + height.saturating_sub(52 + h) / 2;
    (x, y, w, h)
}

/// Yükleme çubuğu: iz (track) ve hareketli parça (sweep) dikdörtgenleri.
pub struct LoadingBar {
    pub track: Rect,
    pub sweep: Rect,
}

/// Yükleme çubuğunu çözünürlük ve animasyon fazına göre yerleştirir.
pub fn loading_bar(width: usize, height: usize, phase: usize) -> LoadingBar {
    let cx = width / 2;
    let cy = height / 2;
    let bar_w = 220.min(width.saturating_sub(48)).max(1);
    let bar_x = cx.saturating_sub(bar_w / 2);
    let bar_y = cy + 34;
    let sweep_w = (bar_w / 3).max(2).min(bar_w);
    let travel = bar_w.saturating_sub(sweep_w);
    let sweep_x = bar_x + (phase * 19) % (travel + 1);
    LoadingBar {
        track: (bar_x, bar_y, bar_w, 8),
        sweep: (sweep_x, bar_y + 1, sweep_w, 6),
    }
}

/// Yükleme aşamasının kullanıcıya gösterilen adı.
pub fn loading_stage(phase: usize, english: bool) -> &'static str {
    match (phase, english) {
        (0, false) => "Duvar kağıtları hazırlanıyor",
        (0, true) => "Preparing wallpapers",
        (1, false) => "Tema ve simgeler yükleniyor",
        (1, true) => "Loading theme and icons",
        (2, false) => "Uygulamalar başlatılıyor",
        (2, true) => "Starting applications",
        (_, false) => "Masaüstü hazırlanıyor",
        (_, true) => "Preparing desktop",
    }
}

/// Yükleme yüzdesi (4 faz × %25).
pub fn loading_percent(phase: usize) -> usize {
    phase.saturating_add(1).saturating_mul(25).min(100)
}

/// Yükleme ekranı alt ipucu.
pub fn loading_hint(english: bool) -> &'static str {
    if english {
        "First boot may take a few seconds."
    } else {
        "İlk açılış birkaç saniye sürebilir."
    }
}

/// Hoş geldin ekranındaki klavye kısayolu ipucu.
pub fn welcome_hint(english: bool) -> &'static str {
    if english {
        "Tab: select  Enter: confirm  Esc: cancel  E: boot log"
    } else {
        "Tab: sec  Enter: onayla  Esc: vazgec  E: boot logu"
    }
}

/// Önyükleme hatasının alt bilgisi (dar ekranlar için kısa varyant).
pub fn boot_error_footer() -> &'static str {
    "Yeniden başlatmak için herhangi bir tuşa basın.  /  Press any key to restart."
}

/// Önyükleme hatası kodu (destek tanılaması için).
pub fn boot_error_code() -> &'static str {
    "BOOT-001"
}

/// Kapanış ekranı başlığı.
pub fn shutdown_title(restart: bool, english: bool) -> &'static str {
    match (restart, english) {
        (false, false) => "Sistem Kapatılıyor",
        (false, true) => "Shutting down...",
        (true, false) => "Yeniden Başlatılıyor",
        (true, true) => "Restarting...",
    }
}

/// Kapanışın geçerli adımı.
pub fn shutdown_step_text(phase: usize, restart: bool, english: bool) -> &'static str {
    match (phase, restart, english) {
        (0, false, false) => "Değişiklikler kaydediliyor...",
        (0, false, true) => "Saving changes...",
        (0, true, false) => "Değişiklikler kaydediliyor...",
        (0, true, true) => "Saving changes...",
        (1, _, false) => "Disk önbelleği boşaltılıyor...",
        (1, _, true) => "Flushing disk cache...",
        (2, _, false) => "Sürücüler kapatılıyor...",
        (2, _, true) => "Stopping drivers...",
        (_, false, false) => "Güç kesiliyor...",
        (_, false, true) => "Powering off...",
        (_, true, false) => "Yeniden başlatılıyor...",
        (_, true, true) => "Restarting...",
    }
}

/// Kapanış ekranındaki sabip ipucu.
pub fn shutdown_hint(english: bool) -> &'static str {
    if english {
        "Please wait. Closing open files and devices."
    } else {
        "Lütfen bekleyiniz. Açık dosyalar ve aygıtlar kapatılıyor."
    }
}

fn inside(rect: Rect, width: usize, height: usize) -> bool {
    rect.0 <= width
        && rect.1 <= height
        && rect.0.saturating_add(rect.2) <= width
        && rect.1.saturating_add(rect.3) <= height
}

/// GUI smoke testi: yaygın çözünürlüklerde tüm ekran geometrisini doğrular.
pub fn self_test() -> bool {
    const RESOLUTIONS: [(usize, usize); 7] = [
        (MIN_SCREEN_W, MIN_SCREEN_H),
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 720),
        (1920, 1080),
        (2560, 1440),
    ];
    for (width, height) in RESOLUTIONS {
        let panel = welcome_panel(width, height);
        if !inside(panel, width, height) {
            return false;
        }
        for index in 0..2 {
            if !inside(welcome_language_rect(width, height, index), width, height) {
                return false;
            }
        }
        for index in 0..theme::ThemeId::ALL.len() {
            if !inside(welcome_theme_rect(width, height, index), width, height) {
                return false;
            }
        }
        if !inside(welcome_continue_rect(width, height), width, height) {
            return false;
        }
        if !inside(boot_error_box(width, height), width, height) {
            return false;
        }
        if !inside(shutdown_box(width, height), width, height) {
            return false;
        }
        for phase in 0..8 {
            let bar = loading_bar(width, height, phase);
            if !inside(bar.track, width, height) || !inside(bar.sweep, width, height) {
                return false;
            }
            if bar.sweep.0 < bar.track.0 || bar.sweep.0 + bar.sweep.2 > bar.track.0 + bar.track.2 {
                return false;
            }
            if loading_stage(phase, false).is_empty()
                || loading_stage(phase, true).is_empty()
                || shutdown_step_text(phase, false, false).is_empty()
                || shutdown_step_text(phase, true, true).is_empty()
            {
                return false;
            }
        }
        if loading_percent(7) != 100 {
            return false;
        }
    }
    boot_error_code() == "BOOT-001"
        && !boot_error_footer().is_empty()
        && !shutdown_hint(false).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_self_test_passes_for_common_resolutions() {
        assert!(self_test(), "GUI yerleşim denetimi başarısız");
    }

    #[test]
    fn welcome_controls_stay_inside_panel() {
        for (w, h) in [(640usize, 480usize), (1024, 768), (1920, 1080)] {
            let panel = welcome_panel(w, h);
            for i in 0..2 {
                let r = welcome_language_rect(w, h, i);
                assert!(r.0 >= panel.0 && r.0 + r.2 <= panel.0 + panel.2);
                assert!(r.1 >= panel.1 && r.1 + r.3 <= panel.1 + panel.3);
            }
            let button = welcome_continue_rect(w, h);
            assert!(button.0 >= panel.0 && button.0 + button.2 <= panel.0 + panel.2);
            assert!(button.1 >= panel.1 && button.1 + button.3 <= panel.1 + panel.3);
            let (hint_x, hint_y) = welcome_hint_pos(w, h);
            assert!(hint_x >= panel.0 && hint_x < panel.0 + panel.2);
            assert!(hint_y >= panel.1 && hint_y < panel.1 + panel.3);
        }
    }

    #[test]
    fn theme_grid_is_centered_and_nonoverlapping() {
        let (w, h) = (1280usize, 720usize);
        let panel = welcome_panel(w, h);
        let cols = theme_columns(panel.2);
        assert_eq!(cols, 4);
        for i in 0..theme::ThemeId::ALL.len() {
            let a = welcome_theme_rect(w, h, i);
            assert!(a.2 > 0 && a.3 > 0);
            if (i + 1) % cols != 0 && i + 1 < theme::ThemeId::ALL.len() {
                let b = welcome_theme_rect(w, h, i + 1);
                assert!(a.0 + a.2 <= b.0, "yanyana kartlar çakışıyor");
            }
        }
    }

    #[test]
    fn copy_is_localized_and_progress_is_bounded() {
        assert_eq!(loading_percent(0), 25);
        assert_eq!(loading_percent(3), 100);
        assert_eq!(loading_percent(99), 100);
        assert_ne!(loading_stage(0, false), loading_stage(0, true));
        assert_ne!(shutdown_title(false, false), shutdown_title(false, true));
        assert_ne!(shutdown_title(true, false), shutdown_title(false, false));
        assert_ne!(
            shutdown_step_text(3, false, false),
            shutdown_step_text(3, true, false)
        );
    }
}

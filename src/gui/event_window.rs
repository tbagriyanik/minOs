//! Pencere olaylarını işler (surükleme, yeniden boyutlandırma, pencere tıklamaları).

use crate::gui::{in_rect, MinGuiCompositor, Window};

/// Pencere kontrol düğmesi (kapat/maksimize/küçült) tespiti.
///
/// Fare konumu ve pencereye göre hangi kontrol düğmesinin üzerinde
/// olduğunu döndürür. Dönüş: `Some(0)` kapat, `Some(1)` maksimize,
/// `Some(2)` küçült, `None` hiçbiri.
pub fn window_control_at(mx: usize, my: usize, win: &Window) -> Option<u8> {
    if in_rect(mx, my, win.close_hit_rect()) {
        Some(0)
    } else if in_rect(mx, my, win.max_hit_rect()) {
        Some(1)
    } else if in_rect(mx, my, win.min_hit_rect()) {
        Some(2)
    } else {
        None
    }
}

/// Pencere kontrol düğmesi bırakıldığında işlem yapar (kapat hariç).
///
/// `control`: 1 = maksimize/restore, 2 = küçült.
/// Dönüş: `true` ise işlem gerçekleşti, `false` ise yok.
pub fn handle_window_control_release(
    compositor: &mut MinGuiCompositor,
    idx: usize,
    control: u8,
    mx: usize,
    my: usize,
    width: usize,
    height: usize,
) -> bool {
    let Some(win) = compositor.windows.get(idx).and_then(|slot| *slot) else {
        return false;
    };
    let release_rect = match control {
        1 => win.max_hit_rect(),
        _ => win.min_hit_rect(),
    };
    if !win.is_open || win.is_minimized || !in_rect(mx, my, release_rect) {
        return false;
    }
    match control {
        1 => {
            compositor.focus_window(idx);
            compositor.toggle_maximize(idx, width, height);
            true
        }
        _ => {
            if let Some(w) = compositor.windows[idx].as_mut() {
                w.is_minimized = true;
            }
            compositor.focus_top_window();
            true
        }
    }
}

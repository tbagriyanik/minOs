//! Ortak UI ölçü sabitleri ve ölçek yardımcıları.
//!
//! Tüm pencere/buton/çubuk/boşluk boyutları tek yerden gelir.
//! `sp()` (scaled pixels) ile font ölçeğine duyarlı hesaplanmalar yapılır.

use super::*;

/// Temel (100% ölçekte) piksel değerleri.
pub struct Base;

impl Base {
    // === Çubuk / Şerit Yükseklikleri ===
    pub const TITLEBAR: usize = 24;
    pub const TASKBAR: usize = 36;
    pub const TOOLBAR: usize = 24;
    pub const STATUSBAR: usize = 22;
    pub const MENU_BAR: usize = 22;

    // === Buton / Kontrol Boyutları ===
    pub const BUTTON_HEIGHT: usize = 24;
    pub const BUTTON_MIN_WIDTH: usize = 48;
    pub const ICON_BUTTON_SIZE: usize = 22;
    pub const CLOSE_BUTTON: usize = 18;
    pub const MAX_BUTTON: usize = 18;
    pub const MIN_BUTTON: usize = 18;

    // === Giriş Alanları ===
    pub const INPUT_HEIGHT: usize = 20;
    pub const INPUT_PADDING_X: usize = 4;
    pub const INPUT_PADDING_Y: usize = 2;

    // === Metin / Yazı Tipi ===
    pub const LINE_HEIGHT: usize = 16;
    pub const CHAR_WIDTH_EST: usize = 8; // 'M' karakteri için tahmini genişlik
    pub const GUTTER_WIDTH: usize = 44;

    // === Boşluklar ===
    pub const PADDING_SMALL: usize = 4;
    pub const PADDING_MEDIUM: usize = 8;
    pub const PADDING_LARGE: usize = 12;
    pub const MARGIN_SMALL: usize = 4;
    pub const MARGIN_MEDIUM: usize = 8;
    pub const MARGIN_LARGE: usize = 12;
    pub const GAP_SMALL: usize = 4;
    pub const GAP_MEDIUM: usize = 8;
    pub const GAP_LARGE: usize = 12;

    // === Kenarlık / Yuvarlaklık ===
    pub const BORDER_RADIUS_SMALL: usize = 4;
    pub const BORDER_RADIUS_MEDIUM: usize = 6;
    pub const BORDER_RADIUS_LARGE: usize = 8;
    pub const BORDER_RADIUS_XL: usize = 10;

    // === Yeniden Boyutlandırma ===
    pub const RESIZE_GRIP: usize = 12;
    pub const TOP_RESIZE_GRIP: usize = 4;
    pub const OUTSIDE_MARGIN: usize = 4;

    // === Pencere Minimum Boyutları ===
    pub const MIN_WINDOW_WIDTH: usize = 200;
    pub const MIN_WINDOW_HEIGHT: usize = 120;
    pub const MAX_WINDOW_WIDTH: usize = 4096;
    pub const MAX_WINDOW_HEIGHT: usize = 4096;

    // === Menü ===
    pub const MENU_ITEM_HEIGHT: usize = 22;
    pub const MENU_PADDING: usize = 4;
    pub const MENU_WIDTH: usize = 180;
    pub const MENU_MAX_HEIGHT: usize = 300;

    // === Görev Çubuğu ===
    pub const TASKBAR_TAB_START: usize = 118;
    pub const TASKBAR_TAB_WIDTH: usize = 84;
    pub const TASKBAR_TAB_STEP: usize = 88;
    pub const TASKBAR_MIN_SLOT: usize = 30;

    // === Toast / Bildirim ===
    pub const TOAST_WIDTH: usize = 280;
    pub const TOAST_HEIGHT: usize = 60;
    pub const TOAST_HEIGHT_WITH_PROGRESS: usize = 76;
    pub const TOAST_MARGIN: usize = 10;
    pub const TOAST_SLIDE_IN_MS: u64 = 300;

    // === Kaydırma Çubuğu ===
    pub const SCROLLBAR_THICKNESS: usize = 10;
    pub const SCROLLBAR_MIN_THUMB: usize = 20;

    // === Başlangıç Menüsü ===
    pub const START_MENU_WIDTH: usize = 320;
    pub const START_MENU_HEIGHT: usize = 230;
    pub const START_MENU_ITEM_HEIGHT: usize = 20;
    pub const START_MENU_GROUP_HEIGHT: usize = 18;
    pub const START_MENU_CONTENT_TOP: usize = 82;

    // === Bağlam Menüsü ===
    pub const CONTEXT_MENU_WIDTH: usize = 180;
    pub const CONTEXT_MENU_ITEM_HEIGHT: usize = 22;
    pub const CONTEXT_MENU_PADDING: usize = 4;

    // === Diğer ===
    pub const SPLASH_WIDTH: usize = 400;
    pub const SPLASH_HEIGHT: usize = 240;
    pub const DIALOG_WIDTH: usize = 300;
    pub const DIALOG_HEIGHT: usize = 120;
    pub const DIALOG_BUTTON_WIDTH: usize = 80;
    pub const DIALOG_BUTTON_HEIGHT: usize = 28;
}

/// Ölçeklenmiş piksel değerini döndürür (font zoomuna göre).
#[inline(always)]
pub fn sp(base: usize) -> usize {
    let z = ui_scale_percent();
    if z == 100 {
        base
    } else {
        (base * z + 50) / 100
    }
}

/// Ölçeklenmiş çubuk yüksekliği.
#[inline(always)]
pub fn scaled_titlebar_h() -> usize {
    sp(Base::TITLEBAR)
}

#[inline(always)]
pub fn scaled_taskbar_h() -> usize {
    sp(Base::TASKBAR)
}

#[inline(always)]
pub fn scaled_toolbar_h() -> usize {
    sp(Base::TOOLBAR)
}

#[inline(always)]
pub fn scaled_statusbar_h() -> usize {
    sp(Base::STATUSBAR)
}

#[inline(always)]
pub fn scaled_button_h() -> usize {
    sp(Base::BUTTON_HEIGHT)
}

#[inline(always)]
pub fn scaled_input_h() -> usize {
    sp(Base::INPUT_HEIGHT)
}

#[inline(always)]
pub fn scaled_line_h() -> usize {
    sp(Base::LINE_HEIGHT)
}

#[inline(always)]
pub fn scaled_gutter_w() -> usize {
    sp(Base::GUTTER_WIDTH)
}

#[inline(always)]
pub fn scaled_char_w() -> usize {
    sp(Base::CHAR_WIDTH_EST)
}

#[inline(always)]
pub fn scaled_menu_item_h() -> usize {
    sp(Base::MENU_ITEM_HEIGHT)
}

#[inline(always)]
pub fn scaled_scrollbar_thickness() -> usize {
    sp(Base::SCROLLBAR_THICKNESS)
}

/// Minimum pencere boyutları (ölçeklenmiş).
#[inline(always)]
pub fn min_window_w() -> usize {
    sp(Base::MIN_WINDOW_WIDTH)
}

#[inline(always)]
pub fn min_window_h() -> usize {
    sp(Base::MIN_WINDOW_HEIGHT)
}

/// Menü boyutları (ölçeklenmiş).
#[inline(always)]
pub fn menu_w() -> usize {
    sp(Base::MENU_WIDTH)
}

#[inline(always)]
pub fn menu_pad() -> usize {
    sp(Base::MENU_PADDING)
}

#[inline(always)]
pub fn menu_max_h() -> usize {
    sp(Base::MENU_MAX_HEIGHT)
}

/// Görev çubuğu sekmeleri düzeni.
#[inline(always)]
pub fn taskbar_tab_start() -> usize {
    sp(Base::TASKBAR_TAB_START)
}

#[inline(always)]
pub fn taskbar_tab_width() -> usize {
    sp(Base::TASKBAR_TAB_WIDTH)
}

#[inline(always)]
pub fn taskbar_tab_step() -> usize {
    sp(Base::TASKBAR_TAB_STEP)
}

#[inline(always)]
pub fn taskbar_min_slot() -> usize {
    sp(Base::TASKBAR_MIN_SLOT)
}

/// Toast bildirimi boyutları.
#[inline(always)]
pub fn toast_w(fb_w: usize) -> usize {
    sp(Base::TOAST_WIDTH).min(fb_w.saturating_sub(sp(24)))
}

#[inline(always)]
pub fn toast_h(has_progress: bool) -> usize {
    if has_progress {
        sp(Base::TOAST_HEIGHT_WITH_PROGRESS)
    } else {
        sp(Base::TOAST_HEIGHT)
    }
}

#[inline(always)]
pub fn toast_margin() -> usize {
    sp(Base::TOAST_MARGIN)
}

/// Kaydırma çubuğu kalınlığı.
#[inline(always)]
pub fn scrollbar_thickness() -> usize {
    scaled_scrollbar_thickness()
}

/// Bağlam menüsü boyutları.
#[inline(always)]
pub fn context_menu_w() -> usize {
    sp(Base::CONTEXT_MENU_WIDTH)
}

#[inline(always)]
pub fn context_menu_item_h() -> usize {
    sp(Base::CONTEXT_MENU_ITEM_HEIGHT)
}

#[inline(always)]
pub fn context_menu_pad() -> usize {
    sp(Base::CONTEXT_MENU_PADDING)
}

/// Diyalog buton boyutları.
#[inline(always)]
pub fn dialog_button_w() -> usize {
    sp(Base::DIALOG_BUTTON_WIDTH)
}

#[inline(always)]
pub fn dialog_button_h() -> usize {
    sp(Base::DIALOG_BUTTON_HEIGHT)
}

/// Yeniden boyutlandırma tutamaç boyutu.
#[inline(always)]
pub fn resize_grip() -> usize {
    sp(Base::RESIZE_GRIP)
}

#[inline(always)]
pub fn top_resize_grip() -> usize {
    sp(Base::TOP_RESIZE_GRIP)
}

#[inline(always)]
pub fn outside_margin() -> usize {
    sp(Base::OUTSIDE_MARGIN)
}

/// Kenarlık yuvarlaklığı.
#[inline(always)]
pub fn border_radius_small() -> usize {
    sp(Base::BORDER_RADIUS_SMALL)
}

#[inline(always)]
pub fn border_radius_medium() -> usize {
    sp(Base::BORDER_RADIUS_MEDIUM)
}

#[inline(always)]
pub fn border_radius_large() -> usize {
    sp(Base::BORDER_RADIUS_LARGE)
}

#[inline(always)]
pub fn border_radius_xl() -> usize {
    sp(Base::BORDER_RADIUS_XL)
}

/// Boşluk yardımcıları.
#[inline(always)]
pub fn pad_small() -> usize {
    sp(Base::PADDING_SMALL)
}

#[inline(always)]
pub fn pad_medium() -> usize {
    sp(Base::PADDING_MEDIUM)
}

#[inline(always)]
pub fn pad_large() -> usize {
    sp(Base::PADDING_LARGE)
}

#[inline(always)]
pub fn gap_small() -> usize {
    sp(Base::GAP_SMALL)
}

#[inline(always)]
pub fn gap_medium() -> usize {
    sp(Base::GAP_MEDIUM)
}

#[inline(always)]
pub fn gap_large() -> usize {
    sp(Base::GAP_LARGE)
}

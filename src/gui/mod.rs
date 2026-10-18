pub mod font_render;
pub mod theme;
pub mod wallpaper;
pub mod clipboard;
pub mod scrollbar;
pub mod widget;
pub mod screens;
pub mod textbuf;
pub mod window;
pub mod draw;
pub mod icons;
pub mod desktop;
pub mod taskbar;
pub mod tray;
pub mod toast;
pub mod context;
pub mod overlay;
pub mod startmenu;

pub use scrollbar::{ScrollState, ScrollbarGeom};
pub use textbuf::TextBuf;
pub use screens::*;
pub use window::*;
pub use context::*;
pub use toast::*;

#[allow(unused_imports)]
use crate::drivers::video::Framebuffer;
#[allow(unused_imports)]
use crate::drivers::mouse::MouseState;
#[allow(unused_imports)]
use crate::drivers::keyboard::KeyboardState;
#[allow(unused_imports)]
use crate::drivers::rtc::DateTime;

/// Arayüz renkleri artık *tema jetonu* taşır. Gerçek RGB değeri çizim anında
/// aktif temadan okunur (bkz. `theme::resolve`). Böylece tema değişimi tüm
/// arayüzü tek seferde günceller.
pub struct Color;

impl Color {
    pub const DARK_BG: u32 = theme::token(theme::IDX_DESKTOP_BG); // masaüstü zemin
    pub const WINDOW_BG: u32 = theme::token(theme::IDX_WINDOW_BG); // pencere gövdesi
    pub const TITLE_BAR: u32 = theme::token(theme::IDX_TITLEBAR); // aktif başlık
    pub const TITLE_BAR_INACTIVE: u32 = theme::token(theme::IDX_TITLEBAR_INACTIVE);
    pub const TASKBAR_BG: u32 = theme::token(theme::IDX_TASKBAR_BG);
    pub const ACCENT_BLUE: u32 = theme::token(theme::IDX_ACCENT_BLUE);
    pub const ACCENT_GREEN: u32 = theme::token(theme::IDX_ACCENT_GREEN);
    pub const ACCENT_RED: u32 = theme::token(theme::IDX_ACCENT_RED);
    pub const ACCENT_YELLOW: u32 = theme::token(theme::IDX_ACCENT_YELLOW);
    pub const ACCENT_PINK: u32 = theme::token(theme::IDX_ACCENT_PINK);
    pub const ACCENT_CAT_EARS: u32 = theme::token(theme::IDX_ACCENT_CAT);
    pub const TEXT_MAIN: u32 = theme::token(theme::IDX_TEXT_MAIN);
    pub const TEXT_MUTED: u32 = theme::token(theme::IDX_TEXT_MUTED);
    pub const CURSOR_FILL: u32 = theme::token(theme::IDX_CURSOR_FILL);
    pub const CURSOR_OUTLINE: u32 = theme::token(theme::IDX_CURSOR_OUTLINE);
    pub const BORDER: u32 = theme::token(theme::IDX_BORDER);
    pub const SHADOW: u32 = theme::token(theme::IDX_SHADOW);
    pub const WHITE: u32 = 0xFFFFFF;
    pub const BLACK: u32 = 0x000000;
}

static FONT_ZOOM: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(100); // TEST 100%

#[inline(always)]
pub fn set_font_zoom(percent: u8) {
    // Clamp to valid range: 100 (default), 125, or 150
    let p = match percent { 125 => 125, 150 => 150, _ => 100 };
    FONT_ZOOM.store(p, core::sync::atomic::Ordering::Relaxed);
}

#[inline(always)]
pub fn font_zoom() -> u8 { FONT_ZOOM.load(core::sync::atomic::Ordering::Relaxed) }

/// Aktif UI ölçek yüzdesi (100 = temel 12px font hücresi). Menü, liste ve
/// düğme boyutları bu değerle otomatik ölçeklenir.
#[inline(always)]
pub fn ui_scale_percent() -> usize { font_zoom() as usize }

/// Temel bir piksel ölçüsünü aktif font ölçeğine göre büyütür.
/// Font büyüdükçe menü, liste satırı ve düğme boyutları da orantılı büyür.
#[inline(always)]
pub fn sp(base: usize) -> usize {
    let z = ui_scale_percent();
    if z == 100 { base } else { (base * z + 50) / 100 }
}

/// Aktif font ölçeğine göre satır yüksekliği/öğe adımı (px).
#[inline(always)]
pub fn item_step(base: usize) -> usize { sp(base) }

static UI_LANGUAGE_EN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
pub fn set_ui_language_english(value: bool) { UI_LANGUAGE_EN.store(value, core::sync::atomic::Ordering::Relaxed); }
pub fn ui_english() -> bool { UI_LANGUAGE_EN.load(core::sync::atomic::Ordering::Relaxed) }
pub fn ui_text<'a>(tr: &'a str, en: &'a str) -> &'a str { if ui_english() { en } else { tr } }

const BASE_FONT_SIZE: f32 = 14.0;

pub fn in_rect(mx: usize, my: usize, r: (usize, usize, usize, usize)) -> bool {
    mx >= r.0 && mx < r.0 + r.2 && my >= r.1 && my < r.1 + r.3
}

// ---------------------------------------------------------------------------
// Pencere Yöneticisi / Kompozitör
// ---------------------------------------------------------------------------
pub struct MinGuiCompositor {
    pub windows: [Option<Window>; MAX_WINDOWS],
    pub active_window_idx: Option<usize>,
    pub is_start_menu_open: bool,
    pub active_start_group: usize,
    pub selected_start_item: usize,
    pub start_search: [u8; 32],
    pub start_search_len: usize,
    pub is_context_menu_open: bool,
    pub is_context_submenu_open: bool,
    pub is_alt_tab_open: bool,
    pub is_calendar_open: bool,
    pub is_volume_popup_open: bool,
    pub context_menu_x: usize,
    pub context_menu_y: usize,
    pub context_file_idx: Option<usize>,
    /// Sağ tık menüsünün türü (tıklanan yere göre).
    pub context_kind: ContextKind,
    /// Pencere menüsü için hedef pencere indeksi.
    pub context_window: Option<usize>,
    /// Takvimde gösterilen ay/yıl (saatten bağımsız gezinilebilir).
    pub cal_year: u16,
    pub cal_month: u8,
    /// Sistem kapatma onayı bekleniyor mu? (Yanlış tetiklenmeyi önler.)
    pub confirm_shutdown: bool,
    pub confirm_restart: bool,
    pub toasts: [Option<ToastNotification>; MAX_TOASTS],
    alt_tab_order: [usize; MAX_WINDOWS],
    alt_tab_count: usize,
    alt_tab_selection: usize,
    z_counter: u8,
}

impl MinGuiCompositor {
    pub const fn new() -> Self {
        Self {
            windows: [None; MAX_WINDOWS],
            active_window_idx: None,
            is_start_menu_open: true,
            active_start_group: 0,
            selected_start_item: 0,
            start_search: [0; 32],
            start_search_len: 0,
            is_context_menu_open: false,
            is_context_submenu_open: false,
            is_alt_tab_open: false,
            is_calendar_open: false,
            is_volume_popup_open: false,
            context_menu_x: 0,
            context_menu_y: 0,
            context_file_idx: None,
            context_kind: ContextKind::Desktop,
            context_window: None,
            cal_year: 2026,
            cal_month: 1,
            confirm_shutdown: false,
            confirm_restart: false,
            toasts: [None, None, None, None],
            alt_tab_order: [0; MAX_WINDOWS],
            alt_tab_count: 0,
            alt_tab_selection: 0,
            z_counter: 0,
        }
    }

    pub fn add_window(&mut self, mut win: Window) -> bool {
        for idx in 0..self.windows.len() {
            if self.windows[idx].is_none() {
                // Sabit uygulama pencereleri 0..=13 arası slotlara eklenir (14 uygulama);
                if win.app_id == u8::MAX && idx < 14 { win.app_id = idx as u8; }
                self.z_counter = self.z_counter.wrapping_add(1);
                win.z = self.z_counter;
                self.windows[idx] = Some(win);
                return true;
            }
        }
        false
    }

    /// Çizim Tahtası (Paint) dışındaki uygulamalar tek örnektir; var olan pencereyi öne getirir.
    pub fn launch_app_window(&mut self, app_id: usize) -> Option<usize> {
        if app_id >= 14 { return None; }
        let existing = self.windows.iter().enumerate().find_map(|(idx, slot)| {
            slot.filter(|w| w.app_id as usize == app_id && w.is_open).map(|_| idx)
        });
        if app_id != 9 {
            if let Some(idx) = existing {
                if let Some(win) = self.windows[idx].as_mut() { win.is_minimized = false; }
                self.focus_window(idx);
                return Some(idx);
            }
        }

        let source_idx = existing.or_else(|| self.windows.iter().position(|slot| {
            slot.map_or(false, |w| w.app_id as usize == app_id)
        }))?;
        if existing.is_none() {
            if let Some(win) = self.windows[source_idx].as_mut() {
                win.is_open = true;
                win.is_minimized = false;
                win.is_maximized = false;
            }
            self.focus_window(source_idx);
            return Some(source_idx);
        }
        let mut win = self.windows[source_idx]?;
        let instances = self.windows.iter().filter(|slot| slot.map_or(false, |w| w.app_id as usize == app_id)).count();
        let offset = (instances % 8) * 24;
        win.x = win.x.saturating_add(offset);
        win.y = win.y.saturating_add(offset);
        win.is_open = true;
        win.is_minimized = false;
        win.is_focused = false;
        win.is_maximized = false;
        win.restore = None;
        win.scroll.x = 0;
        win.scroll.y = 0;
        let idx = self.windows.iter().position(Option::is_none)?;
        if !self.add_window(win) { return None; }
        self.focus_window(idx);
        Some(idx)
    }

    #[inline(always)]
    pub fn app_id_for_window(&self, idx: usize) -> usize {
        self.windows.get(idx).and_then(|slot| *slot).map_or(idx, |win| win.app_id as usize)
    }

    /// Belirtilen pencereyi odaklar ve z-sırasında en öne taşır.
    pub fn focus_window(&mut self, idx: usize) {
        if idx >= self.windows.len() || self.windows[idx].is_none() {
            return;
        }
        if self.z_counter >= 240 {
            let (order, count) = self.draw_order();
            for (new_z, &w_idx) in order[..count].iter().enumerate() {
                if let Some(w) = self.windows[w_idx].as_mut() {
                    w.z = new_z as u8;
                }
            }
            self.z_counter = count as u8;
        }
        self.z_counter = self.z_counter.saturating_add(1);
        let z = self.z_counter;
        self.active_window_idx = Some(idx);
        for (i, slot) in self.windows.iter_mut().enumerate() {
            if let Some(w) = slot {
                w.is_focused = i == idx;
                if i == idx {
                    w.z = z;
                }
            }
        }
    }

    /// En üstte duran, açık ve küçültülmemiş pencereye odaklanır.
    pub fn focus_top_window(&mut self) {
        let mut best: Option<(usize, u8)> = None;
        for (i, slot) in self.windows.iter().enumerate() {
            if let Some(w) = slot {
                if w.is_open && !w.is_minimized {
                    match best {
                        Some((_, bz)) if bz >= w.z => {}
                        _ => best = Some((i, w.z)),
                    }
                }
            }
        }
        if let Some((i, _)) = best {
            self.focus_window(i);
        } else {
            self.active_window_idx = None;
            for slot in self.windows.iter_mut() {
                if let Some(w) = slot {
                    w.is_focused = false;
                }
            }
        }
    }

    /// Alt+Tab kart dizisini ilk basışta sabitler; Alt basılı kaldıkça seçim
    /// soldan sağa, satır sonunda bir alt satırın başına ilerler.
    pub fn cycle_focus(&mut self, reverse: bool) {
        if self.alt_tab_count == 0 {
            let (order, count) = self.switcher_order();
            if count == 0 { return; }
            self.alt_tab_order = order;
            self.alt_tab_count = count;
            let current = self.active_window_idx.unwrap_or(order[count - 1]);
            self.alt_tab_selection = order[..count].iter().position(|&idx| idx == current).unwrap_or(count - 1);
        }
        let count = self.alt_tab_count;
        self.alt_tab_selection = if reverse {
            (self.alt_tab_selection + count - 1) % count
        } else {
            (self.alt_tab_selection + 1) % count
        };
        let next = self.alt_tab_order[self.alt_tab_selection];
        if let Some(win) = self.windows[next].as_mut() { win.is_minimized = false; }
        self.focus_window(next);
    }

    pub fn finish_alt_tab(&mut self) {
        self.is_alt_tab_open = false;
        self.alt_tab_count = 0;
        self.alt_tab_selection = 0;
    }

    /// Açık pencereleri z-sırasına göre (arkadan öne) indeks listesi olarak döndürür.
    pub fn draw_order(&self) -> ([usize; MAX_WINDOWS], usize) {
        let mut order: [usize; MAX_WINDOWS] = [0; MAX_WINDOWS];
        let mut count = 0usize;
        for (i, slot) in self.windows.iter().enumerate() {
            if let Some(w) = slot {
                if w.is_open && !w.is_minimized {
                    order[count] = i;
                    count += 1;
                }
            }
        }
        for i in 0..count {
            for j in i + 1..count {
                let zi = self.windows[order[i]].map(|w| w.z).unwrap_or(0);
                let zj = self.windows[order[j]].map(|w| w.z).unwrap_or(0);
                if zj < zi {
                    order.swap(i, j);
                }
            }
        }
        (order, count)
    }

    /// Alt+Tab için açık tüm pencereleri, küçültülmüş olanlar dahil, z-sırasıyla döndürür.
    pub fn switcher_order(&self) -> ([usize; MAX_WINDOWS], usize) {
        let mut order = [0; MAX_WINDOWS];
        let mut count = 0;
        for (i, slot) in self.windows.iter().enumerate() {
            if slot.map_or(false, |w| w.is_open) { order[count] = i; count += 1; }
        }
        for i in 0..count { for j in i + 1..count {
            let zi = self.windows[order[i]].map(|w| w.z).unwrap_or(0);
            let zj = self.windows[order[j]].map(|w| w.z).unwrap_or(0);
            if zj < zi { order.swap(i, j); }
        }}
        (order, count)
    }

    /// Verilen `(x, y)` noktasının açık ve küçültülmemiş herhangi bir pencere alanında olup olmadığını kontrol eder.
    pub fn is_point_covered_by_window(&self, x: usize, y: usize) -> bool {
        for slot in self.windows.iter() {
            if let Some(w) = slot {
                if w.is_open && !w.is_minimized {
                    if in_rect(x, y, (w.x, w.y, w.width, w.height)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn toggle_maximize(&mut self, idx: usize, screen_w: usize, screen_h: usize) {
        if let Some(ref mut win) = self.windows[idx] {
            if win.is_maximized {
                if let Some((x, y, w, h)) = win.restore {
                    win.x = x;
                    win.y = y;
                    win.width = w;
                    win.height = h;
                }
                win.is_maximized = false;
            } else {
                win.restore = Some((win.x, win.y, win.width, win.height));
                win.x = 0;
                win.y = 0;
                win.width = screen_w;
                win.height = screen_h.saturating_sub(taskbar_h());
                win.is_maximized = true;
            }
        }
    }

    /// Pencereyi ekranın sol yarısına, sağ yarısına veya tam ekrana yaslar (Aero Snap).
    pub fn snap_window(&mut self, idx: usize, snap: SnapPosition, screen_w: usize, screen_h: usize) {
        if let Some(ref mut win) = self.windows[idx] {
            let usable_h = screen_h.saturating_sub(taskbar_h());
            if !win.is_maximized && win.restore.is_none() {
                win.restore = Some((win.x, win.y, win.width, win.height));
            }
            match snap {
                SnapPosition::Top => {
                    win.x = 0;
                    win.y = 0;
                    win.width = screen_w;
                    win.height = usable_h;
                    win.is_maximized = true;
                }
                SnapPosition::Left => {
                    win.x = 0;
                    win.y = 0;
                    win.width = screen_w / 2;
                    win.height = usable_h;
                    win.is_maximized = false;
                }
                SnapPosition::Right => {
                    let half_w = screen_w / 2;
                    win.x = half_w;
                    win.y = 0;
                    win.width = screen_w.saturating_sub(half_w);
                    win.height = usable_h;
                    win.is_maximized = false;
                }
            }
        }
    }

    /// En üstteki (görünür) pencerenin verilen noktadaki kenar tutamacını döndürür.
    /// İmleç şeklini ve yeniden boyutlandırma başlangıcını belirlemek için kullanılır.
    pub fn topmost_resize_edge(&self, mx: usize, my: usize) -> Option<(usize, ResizeEdge)> {
        let (order, count) = self.draw_order();
        for k in (0..count).rev() {
            let idx = order[k];
            if let Some(win) = self.windows[idx] {
                if let Some(edge) = win.resize_edge_at(mx, my) {
                    return Some((idx, edge));
                }
            }
        }
        None
    }

    /// En üstteki pencerenin dikey kaydırma çubuğu (varsa) `(idx, track, content_h, view_h)`.
    pub fn topmost_v_scrollbar(&self, content_h_of: impl Fn(usize) -> usize) -> Option<(usize, (usize, usize, usize, usize), usize, usize)> {
        let (order, count) = self.draw_order();
        for k in (0..count).rev() {
            let idx = order[k];
            if let Some(win) = self.windows[idx] {
                let (cx, cy, cw, ch) = win.content_rect();
                let content_h = content_h_of(idx);
                if content_h > ch {
                    let track = (cx + cw - scrollbar::SCROLLBAR_THICKNESS, cy,
                                 scrollbar::SCROLLBAR_THICKNESS, ch - scrollbar::SCROLLBAR_THICKNESS.min(ch));
                    return Some((idx, track, content_h, ch));
                }
            }
        }
        None
    }
}

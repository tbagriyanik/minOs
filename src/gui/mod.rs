pub mod anim;
pub mod clipboard;
pub mod context;
pub mod desktop;
pub mod dirty_rect;
pub mod draw;
pub mod event_loop;
pub mod font_render;
pub mod frame;
pub mod icons;
pub mod overlay;
pub mod screen_layout;
pub mod screens;
pub mod scrollbar;
pub mod setup;
pub mod startmenu;
pub mod taskbar;
pub mod textbuf;
pub mod textedit;
pub mod theme;
pub mod timefmt;
pub mod toast;
pub mod tooltip;
pub mod tray;
pub mod ui_helpers;
pub mod wallpaper;
pub mod widget;
pub mod window;

pub use context::*;
pub use event_loop::*;
pub use frame::*;
pub use screens::*;
pub use scrollbar::{ScrollState, ScrollbarGeom};
pub use setup::*;
pub use textbuf::TextBuf;
pub use toast::*;
pub use ui_helpers::*;
pub use window::*;

#[allow(unused_imports)]
use crate::drivers::keyboard::KeyboardState;
#[allow(unused_imports)]
use crate::drivers::mouse::MouseState;
#[allow(unused_imports)]
use crate::drivers::rtc::DateTime;
#[allow(unused_imports)]
use crate::drivers::video::Framebuffer;

/// `topmost_v_scrollbar` sonucu: (pencere idx, dikey track dikdörtgeni,
/// içerik yüksekliği, görünüm yüksekliği).
pub type VScrollHit = (usize, (usize, usize, usize, usize), usize, usize);

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
    let p = match percent {
        125 => 125,
        150 => 150,
        _ => 100,
    };
    FONT_ZOOM.store(p, core::sync::atomic::Ordering::Relaxed);
}

#[inline(always)]
pub fn font_zoom() -> u8 {
    FONT_ZOOM.load(core::sync::atomic::Ordering::Relaxed)
}

/// Aktif UI ölçek yüzdesi (100 = temel 12px font hücresi). Menü, liste ve
/// düğme boyutları bu değerle otomatik ölçeklenir.
#[inline(always)]
pub fn ui_scale_percent() -> usize {
    font_zoom() as usize
}

/// Temel bir piksel ölçüsünü aktif font ölçeğine göre büyütür.
/// Font büyüdükçe menü, liste satırı ve düğme boyutları da orantılı büyür.
#[inline(always)]
pub fn sp(base: usize) -> usize {
    let z = ui_scale_percent();
    if z == 100 {
        base
    } else {
        (base * z + 50) / 100
    }
}

/// Aktif font ölçeğine göre satır yüksekliği/öğe adımı (px).
#[inline(always)]
pub fn item_step(base: usize) -> usize {
    sp(base)
}

static UI_LANGUAGE_EN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
pub fn set_ui_language_english(value: bool) {
    UI_LANGUAGE_EN.store(value, core::sync::atomic::Ordering::Relaxed);
}
pub fn ui_english() -> bool {
    UI_LANGUAGE_EN.load(core::sync::atomic::Ordering::Relaxed)
}

/// Saydam pencere tercihi: açıkken pencere gövdesi yarı saydam çizilir
/// ve altındaki masaüstü/duvar kağıdı görünür.
static TRANSPARENT_WINDOWS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
pub fn set_transparent_windows(value: bool) {
    TRANSPARENT_WINDOWS.store(value, core::sync::atomic::Ordering::Relaxed);
}
pub fn transparent_windows() -> bool {
    TRANSPARENT_WINDOWS.load(core::sync::atomic::Ordering::Relaxed)
}
pub fn ui_text<'a>(tr: &'a str, en: &'a str) -> &'a str {
    if ui_english() {
        en
    } else {
        tr
    }
}

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
    /// Başlat menüsünün son açılış anı (monotonic ms); açılış
    /// animasyonunun ilerlemesi buradan hesaplanır. 0 = animasyonsuz.
    pub start_menu_open_ms: u64,
    /// Kirli rectangle takibi: sadece değişen bölgeler yeniden çizilir.
    pub dirty_tracker: crate::gui::dirty_rect::GlobalDirtyTracker,
    /// Şu anda etkin sanal masaüstü (0..MAX_WORKSPACES).
    pub active_workspace: usize,
    pub active_start_group: usize,
    pub selected_start_item: usize,
    pub start_menu_focus_submenu: bool,
    pub start_search: [u8; 32],
    pub start_search_len: usize,
    pub start_search_cursor: usize,
    pub start_search_selection: Option<(usize, usize)>,
    start_search_anchor: Option<usize>,
    pub start_search_focused: bool,
    start_search_last_click: (u64, usize, usize),
    start_search_click_streak: u8,
    pub is_context_menu_open: bool,
    pub is_context_submenu_open: bool,
    pub is_alt_tab_open: bool,
    /// Bağlam menüsü ve Alt+Tab katmanının son açılış anı (monotonic ms);
    /// açılış kayma animasyonunun ilerlemesi buradan hesaplanır.
    pub context_menu_open_ms: u64,
    pub alt_tab_open_ms: u64,
    /// Tepsi açılır pencerelerinin (takvim, ses) son açılış anları.
    pub calendar_open_ms: u64,
    pub volume_popup_open_ms: u64,
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
    /// Hover ipuçları (tooltip) durumu; `Cell` ile iç değişkenlik
    /// sağlanır çünkü render katmanı `&self` ile çalışır.
    pub tooltip: core::cell::Cell<tooltip::TooltipState>,
    /// Sistem kapatma onayı bekleniyor mu? (Yanlış tetiklenmeyi önler.)
    pub confirm_shutdown: bool,
    pub confirm_restart: bool,
    pub toasts: [Option<ToastNotification>; MAX_TOASTS],
    pub installed_apps: [[u8; 24]; 8],
    pub installed_apps_len: [u8; 8],
    pub installed_apps_count: usize,
    /// Yerleşik ve `/bin` üzerinden keşfedilen uygulamaların kayıt defteri.
    pub app_registry: crate::apps::registry::AppRegistry,
    alt_tab_order: [usize; MAX_WINDOWS],
    alt_tab_count: usize,
    alt_tab_selection: usize,
    z_counter: usize,
}

impl MinGuiCompositor {
    /// Sanal masaüstü (workspace) sayısı.
    pub const MAX_WORKSPACES: usize = 4;

    pub const fn new() -> Self {
        Self {
            windows: [None; MAX_WINDOWS],
            active_window_idx: None,
            is_start_menu_open: true,
            start_menu_open_ms: 0,
            active_start_group: 0,
            selected_start_item: 0,
            start_menu_focus_submenu: false,
            start_search: [0; 32],
            start_search_len: 0,
            start_search_cursor: 0,
            start_search_selection: None,
            start_search_anchor: None,
            start_search_focused: false,
            start_search_last_click: (0, 0, 0),
            start_search_click_streak: 0,
            is_context_menu_open: false,
            is_context_submenu_open: false,
            is_alt_tab_open: false,
            context_menu_open_ms: 0,
            alt_tab_open_ms: 0,
            calendar_open_ms: 0,
            volume_popup_open_ms: 0,
            is_calendar_open: false,
            is_volume_popup_open: false,
            context_menu_x: 0,
            context_menu_y: 0,
            context_file_idx: None,
            context_kind: ContextKind::Desktop,
            context_window: None,
            cal_year: 2026,
            cal_month: 1,
            tooltip: core::cell::Cell::new(tooltip::TooltipState::empty()),
            confirm_shutdown: false,
            confirm_restart: false,
            toasts: [None, None, None, None],
            dirty_tracker: crate::gui::dirty_rect::GlobalDirtyTracker::new(),
            active_workspace: 0,
            installed_apps: [[0; 24]; 8],
            installed_apps_len: [0; 8],
            installed_apps_count: 0,
            app_registry: crate::apps::registry::AppRegistry::new(),
            alt_tab_order: [0; MAX_WINDOWS],
            alt_tab_count: 0,
            alt_tab_selection: 0,
            z_counter: 0,
        }
    }

    pub fn add_window(&mut self, mut win: Window) -> bool {
        for idx in 0..self.windows.len() {
            if self.windows[idx].is_none() {
                // Sabit uygulama pencereleri 0..=17 arası slotlara eklenir (18 uygulama);
                if win.app_id == u8::MAX && idx <= 17 {
                    win.app_id = idx as u8;
                }
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
        if app_id >= 18 {
            return None;
        }
        let existing = self.windows.iter().enumerate().find_map(|(idx, slot)| {
            slot.filter(|w| w.app_id as usize == app_id && w.is_open)
                .map(|_| idx)
        });
        if app_id != 9 {
            if let Some(idx) = existing {
                // Pencere başka bir sanal masaüstündeyse önce o masaüstüne geç.
                if let Some(win) = self.windows[idx].as_mut() {
                    win.is_minimized = false;
                    self.active_workspace = win.workspace;
                }
                self.focus_window(idx);
                return Some(idx);
            }
        }

        let source_idx = existing.or_else(|| {
            self.windows
                .iter()
                .position(|slot| slot.is_some_and(|w| w.app_id as usize == app_id))
        })?;
        if existing.is_none() {
            if let Some(win) = self.windows[source_idx].as_mut() {
                win.is_open = true;
                win.is_minimized = false;
                win.is_maximized = false;
                // Yeniden açılan pencere her zaman etkin sanal masaüstünde
                // belirir; kullanıcının içinde bulunduğu masaüstünden başka
                // bir masaüstüne ışınlanmaz.
                // A reopened window always appears on the active workspace;
                // it must not teleport the user to a different desktop.
                win.workspace = self.active_workspace;
            }
            self.focus_window(source_idx);
            return Some(source_idx);
        }
        let mut win = self.windows[source_idx]?;
        let instances = self
            .windows
            .iter()
            .filter(|slot| slot.is_some_and(|w| w.app_id as usize == app_id))
            .count();
        let offset = (instances % 8) * 24;
        win.x = win.x.saturating_add(offset);
        win.y = win.y.saturating_add(offset);
        win.is_open = true;
        win.is_minimized = false;
        win.is_focused = false;
        win.is_maximized = false;
        win.restore = None;
        // Yeni örnek her zaman geçerli sanal masaüstünde açılır.
        win.workspace = self.active_workspace;
        win.scroll.x = 0;
        win.scroll.y = 0;
        let idx = self.windows.iter().position(Option::is_none)?;
        if !self.add_window(win) {
            return None;
        }
        self.focus_window(idx);
        Some(idx)
    }

    /// Bu turun kirli bölge özetini toplar ve takipçiyi sıfırlar.
    ///
    /// `render_full_frame`'in `dirty_rect` parametresine beslenen birleşik
    /// bölgeyi döndürür: `Some((x, y, w, h))` → yalnızca bu bölge çizilip
    /// blit edilir; `None` → tam kare (taşma, izlenmeyen bölge veya güvenli
    /// varsayılan). `GlobalDirtyTracker` kapasitesi aşıldığında birleşim
    /// eksik kalacağı için `needs_full_redraw` durumunda `None` döner.
    pub fn take_dirty_region(&mut self) -> Option<(usize, usize, usize, usize)> {
        let region = if self.dirty_tracker.needs_full_redraw() {
            None
        } else {
            self.dirty_tracker
                .bounding_rect()
                .map(|r| (r.x, r.y, r.width, r.height))
        };
        self.dirty_tracker.clear();
        region
    }

    #[inline(always)]
    pub fn app_id_for_window(&self, idx: usize) -> usize {
        self.windows
            .get(idx)
            .and_then(|slot| *slot)
            .map_or(idx, |win| win.app_id as usize)
    }

    /// Verilen pencerenin etkin temasi: pencere bazlı override varsa onu,
    /// yoksa global temayı miras alir (per-app tema mirasi).
    pub fn theme_for_window(&self, idx: usize) -> theme::ThemeId {
        match self.windows.get(idx).and_then(|slot| *slot) {
            Some(win) => theme::resolve_theme(theme::current(), win.theme_override),
            None => theme::current(),
        }
    }

    /// Bir pencereye uygulama bazli tema override'u uygular.
    ///
    /// Override uygulandıktan sonra ilgili pencereyi kirli bölge listesine
    /// işler (tema değişikliği yeniden çizimi gerektirir).
    pub fn set_window_theme(&mut self, idx: usize, theme_id: theme::ThemeId) {
        if let Some(ref mut win) = self.windows.get_mut(idx).and_then(|slot| *slot) {
            win.theme_override = Some(theme_id);
        }
        let (x, y, w, h) = self
            .windows
            .get(idx)
            .and_then(|slot| *slot)
            .map(|win| (win.x, win.y, win.width, win.height))
            .unwrap_or((0, 0, 0, 0));
        self.dirty_tracker
            .mark_window(idx, crate::gui::dirty_rect::DirtyRect::new(x, y, w, h));
    }

    /// Pencere bazlı tema override'unu temizler; etkin tema global temaya doner.
    pub fn clear_window_theme(&mut self, idx: usize) {
        if let Some(ref mut win) = self.windows.get_mut(idx).and_then(|slot| *slot) {
            win.theme_override = None;
        }
        let (x, y, w, h) = self
            .windows
            .get(idx)
            .and_then(|slot| *slot)
            .map(|win| (win.x, win.y, win.width, win.height))
            .unwrap_or((0, 0, 0, 0));
        self.dirty_tracker
            .mark_window(idx, crate::gui::dirty_rect::DirtyRect::new(x, y, w, h));
    }

    /// Verilen pencere slotu icin kirli bölge render listesini döndürür.
    ///
    /// `None` → bu pencere bu frame'de yeniden cizilmeye gerek yoktur.
    /// Tam kare yeniden cizimler (`dirty_rect == None`) bu sorgulamayi bypass eder.
    pub fn window_dirty_rect(&self, idx: usize) -> Option<(usize, usize, usize, usize)> {
        self.dirty_tracker
            .window_dirty(idx)
            .map(|r| (r.x, r.y, r.width, r.height))
    }

    /// Belirtilen pencereyi odaklar ve z-sırasında en öne taşır.
    /// Kapalı (`is_open == false`) veya boş slotlar reddedilir; Alt+Tab
    /// sıralaması bayatladıktan sonra ölü bir pencere öne gelemez.
    pub fn focus_window(&mut self, idx: usize) {
        match self.windows.get(idx).and_then(|slot| slot.as_ref()) {
            Some(win) if win.is_open && win.workspace == self.active_workspace => {}
            _ => return,
        }
        if self.z_counter >= 240 {
            let (order, count) = self.draw_order();
            for (new_z, &w_idx) in order[..count].iter().enumerate() {
                if let Some(w) = self.windows[w_idx].as_mut() {
                    w.z = new_z;
                }
            }
            self.z_counter = count;
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
        let mut best: Option<(usize, usize)> = None;
        for (i, slot) in self.windows.iter().enumerate() {
            if let Some(w) = slot {
                if w.is_open && !w.is_minimized && w.workspace == self.active_workspace {
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
            for w in self.windows.iter_mut().flatten() {
                w.is_focused = false;
            }
        }
    }

    /// Alt+Tab kart dizisini ilk basışta sabitler; Alt basılı kaldıkça seçim
    /// soldan sağa, satır sonunda bir alt satırın başına ilerler.
    pub fn cycle_focus(&mut self, reverse: bool) {
        if self.alt_tab_count == 0 {
            let (order, count) = self.switcher_order();
            if count == 0 {
                return;
            }
            self.alt_tab_order = order;
            self.alt_tab_count = count;
            let current = self.active_window_idx.unwrap_or(order[count - 1]);
            self.alt_tab_selection = order[..count]
                .iter()
                .position(|&idx| idx == current)
                .unwrap_or(count - 1);
        }
        let count = self.alt_tab_count;
        self.alt_tab_selection = if reverse {
            (self.alt_tab_selection + count - 1) % count
        } else {
            (self.alt_tab_selection + 1) % count
        };
        let next = self.alt_tab_order[self.alt_tab_selection];
        if let Some(win) = self.windows[next].as_mut() {
            // Oturum sırasında kapanan bir pencere seçildiyse diriltme;
            // `focus_window` da zaten kapalı slotları reddeder.
            if win.is_open {
                win.is_minimized = false;
            }
        }
        self.focus_window(next);
    }

    pub fn finish_alt_tab(&mut self) {
        self.is_alt_tab_open = false;
        self.alt_tab_count = 0;
        self.alt_tab_selection = 0;
    }

    /// Aktif sanal masaüstünü `delta` kadar kaydırır (sarma davranışı).
    /// Alt+Tab durumu sıfırlanır; yeni masaüstünün en üst penceresine odaklanılır.
    pub fn switch_workspace(&mut self, delta: isize) {
        let n = Self::MAX_WORKSPACES as isize;
        self.active_workspace = (self.active_workspace as isize + delta).rem_euclid(n) as usize;
        self.finish_alt_tab();
        self.focus_top_window();
    }

    /// Etkin pencereyi `delta` kadar ileri/geri sanal masaüstüne taşır.
    /// Pencere aktif masaüstünden ayrılırsa kalan en üst pencereye odaklanılır.
    pub fn move_active_window_workspace(&mut self, delta: isize) {
        let n = Self::MAX_WORKSPACES as isize;
        let Some(idx) = self.active_window_idx else {
            return;
        };
        let new_ws = match self.windows.get_mut(idx).and_then(|slot| slot.as_mut()) {
            Some(win) if win.is_open => {
                win.workspace = (win.workspace as isize + delta).rem_euclid(n) as usize;
                win.workspace
            }
            _ => return,
        };
        if new_ws != self.active_workspace {
            self.focus_top_window();
        }
    }

    /// Açık pencereleri z-sırasına göre (arkadan öne) indeks listesi olarak döndürür.
    pub fn draw_order(&self) -> ([usize; MAX_WINDOWS], usize) {
        self.sorted_open_windows(false)
    }

    /// Alt+Tab için açık tüm pencereleri, küçültülmüş olanlar dahil, z-sırasıyla döndürür.
    pub fn switcher_order(&self) -> ([usize; MAX_WINDOWS], usize) {
        self.sorted_open_windows(true)
    }

    /// Açık pencereleri z değerine göre artan sırada (arkadan öne) sıralar.
    /// `include_minimized`: true ise küçültülmüş pencereler de dahildir
    /// (Alt+Tab hepsine dönebilir); false ise yalnız görünür olanlar.
    ///
    /// Ekleme sıralaması kullanılır: `z` değerleri çoğunlukla pencere sırasıyla
    /// arttığı için tek geçişte O(n) maliyet oluşur; `z` değerleri yerel diziye
    /// kopyalanır, her karşılaştırmada `windows[...]` erişimi yapılmaz.
    fn sorted_open_windows(&self, include_minimized: bool) -> ([usize; MAX_WINDOWS], usize) {
        let mut order = [0; MAX_WINDOWS];
        let mut z_values = [0usize; MAX_WINDOWS];
        let mut count = 0;
        for (index, slot) in self.windows.iter().enumerate() {
            let Some(window) = slot else {
                continue;
            };
            if !window.is_open
                || window.workspace != self.active_workspace
                || (!include_minimized && window.is_minimized)
            {
                continue;
            }
            let z = window.z;
            let mut position = count;
            while position > 0 && z_values[position - 1] > z {
                z_values[position] = z_values[position - 1];
                order[position] = order[position - 1];
                position -= 1;
            }
            z_values[position] = z;
            order[position] = index;
            count += 1;
        }
        (order, count)
    }

    /// Verilen `(x, y)` noktasının açık ve küçültülmemiş herhangi bir pencere alanında olup olmadığını kontrol eder.
    pub fn is_point_covered_by_window(&self, x: usize, y: usize) -> bool {
        for w in self.windows.iter().flatten() {
            if w.is_open
                && w.workspace == self.active_workspace
                && !w.is_minimized
                && in_rect(x, y, (w.x, w.y, w.width, w.height))
            {
                return true;
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

    /// Pencereyi ekranın sol yarısına, sağ yarısına, tam ekrana veya çeyreklere yaslar (Aero Snap).
    pub fn snap_window(
        &mut self,
        idx: usize,
        snap: SnapPosition,
        screen_w: usize,
        screen_h: usize,
    ) {
        if let Some(ref mut win) = self.windows[idx] {
            let usable_h = screen_h.saturating_sub(taskbar_h());
            if !win.is_maximized && win.restore.is_none() {
                win.restore = Some((win.x, win.y, win.width, win.height));
            }
            let half_w = screen_w / 2;
            let half_h = usable_h / 2;
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
                    win.width = half_w;
                    win.height = usable_h;
                    win.is_maximized = false;
                }
                SnapPosition::Right => {
                    win.x = half_w;
                    win.y = 0;
                    win.width = screen_w.saturating_sub(half_w);
                    win.height = usable_h;
                    win.is_maximized = false;
                }
                SnapPosition::TopLeft => {
                    win.x = 0;
                    win.y = 0;
                    win.width = half_w;
                    win.height = half_h;
                    win.is_maximized = false;
                }
                SnapPosition::TopRight => {
                    win.x = half_w;
                    win.y = 0;
                    win.width = screen_w.saturating_sub(half_w);
                    win.height = half_h;
                    win.is_maximized = false;
                }
                SnapPosition::BottomLeft => {
                    win.x = 0;
                    win.y = half_h;
                    win.width = half_w;
                    win.height = usable_h.saturating_sub(half_h);
                    win.is_maximized = false;
                }
                SnapPosition::BottomRight => {
                    win.x = half_w;
                    win.y = half_h;
                    win.width = screen_w.saturating_sub(half_w);
                    win.height = usable_h.saturating_sub(half_h);
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
    pub fn topmost_v_scrollbar(&self, content_h_of: impl Fn(usize) -> usize) -> Option<VScrollHit> {
        let (order, count) = self.draw_order();
        for k in (0..count).rev() {
            let idx = order[k];
            if let Some(win) = self.windows[idx] {
                let (cx, cy, cw, ch) = win.content_rect();
                let content_h = content_h_of(idx);
                if content_h > ch {
                    let track = (
                        cx + cw - scrollbar::SCROLLBAR_THICKNESS,
                        cy,
                        scrollbar::SCROLLBAR_THICKNESS,
                        ch - scrollbar::SCROLLBAR_THICKNESS.min(ch),
                    );
                    return Some((idx, track, content_h, ch));
                }
            }
        }
        None
    }

    pub fn update_installed_app(&mut self, idx: usize, name: &str) {
        if idx >= 8 {
            return;
        }
        let bytes = name.as_bytes();
        let len = bytes.len().min(24);
        self.installed_apps[idx][..len].copy_from_slice(&bytes[..len]);
        self.installed_apps_len[idx] = len as u8;
    }

    pub fn installed_app_name(&self, idx: usize) -> &str {
        if idx >= self.installed_apps_count {
            return "";
        }
        let len = self.installed_apps_len[idx] as usize;
        core::str::from_utf8(&self.installed_apps[idx][..len]).unwrap_or("")
    }

    pub fn sync_installed_apps(&mut self, vfs: &crate::fs::vfs::VirtualFileSystem) {
        // `/bin` içeriğini uygulama kayıt defterine de yansıt (hot-load keşfi).
        self.app_registry.scan_bin(vfs);
        let items = vfs.list_dir("/bin");
        let mut count = 0;
        for it in items {
            if it.node_type == crate::fs::vfs::NodeType::File {
                let name = it.name.as_str();
                let clean_name = name.strip_suffix(".elf").unwrap_or(name);
                self.update_installed_app(count, clean_name);
                count += 1;
                if count >= 8 {
                    break;
                }
            }
        }
        self.installed_apps_count = count;
    }
}

impl Default for MinGuiCompositor {
    fn default() -> Self {
        Self::new()
    }
}

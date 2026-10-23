//! Window model, layout types and frame rendering.

use super::*;

// ---------------------------------------------------------------------------
// Pencere
// ---------------------------------------------------------------------------
#[derive(Copy, Clone, Debug)]
pub struct WindowState {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub is_open: bool,
}

/// Temel pencere başlık çubuğu yüksekliği (100% ölçekte).
pub const TITLEBAR_BASE: usize = 24;
/// Temel görev çubuğu yüksekliği (100% ölçekte).
pub const TASKBAR_BASE: usize = 36;
pub const MAX_WINDOWS: usize = 32;

/// Aktif font ölçeğine göre başlık çubuğu yüksekliği (px).
#[inline(always)]
pub fn titlebar_h() -> usize { sp(TITLEBAR_BASE) }

/// Aktif font ölçeğine göre görev çubuğu yüksekliği (px).
#[inline(always)]
pub fn taskbar_h() -> usize { sp(TASKBAR_BASE) }

#[derive(Copy, Clone, Debug)]
pub struct Window {
    /// Uygulama türü; aynı uygulamanın birden çok pencere örneğini tanımlar.
    pub app_id: u8,
    pub title: &'static str,
    pub tab_title: &'static str,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub min_width: usize,
    pub min_height: usize,
    pub max_width: usize,
    pub max_height: usize,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub is_open: bool,
    pub is_maximized: bool,
    /// Maksimize öncesi konum/boyut (geri yükleme için).
    pub restore: Option<(usize, usize, usize, usize)>,
    /// Z-sırası: büyük olan üstte çizilir.
    pub z: u8,
    /// İçerik kaydırma durumu (pencere içeriği taşarsa kaydırılır).
    pub scroll: scrollbar::ScrollState,
    /// Uygulamanın bir önceki karede bildirdiği içerik boyutu (px).
    /// Kaydırma çubuğunun gerekip gerekmediğini belirlemek için kullanılır.
    pub content_w: usize,
    pub content_h: usize,
}

impl Window {
    /// Pencerenin içerik (çerçeve dışı, başlık çubuğu altı) alanı.
    /// Sağ altta durum çubuğu için ayrılan alan çağıran tarafından düşülür.
    pub fn content_rect(&self) -> (usize, usize, usize, usize) {
        let x = self.x + 1;
        let y = self.y + titlebar_h();
        let w = self.width.saturating_sub(2);
        let h = self.height.saturating_sub(titlebar_h() + 1);
        (x, y, w, h)
    }

    pub fn new(title: &'static str, tab_title: &'static str, x: usize, y: usize, width: usize, height: usize, is_open: bool) -> Self {
        Self {
            app_id: u8::MAX,
            title,
            tab_title,
            x,
            y,
            width,
            height,
            min_width: 200,
            min_height: 120,
            // Sürükleme ile yeniden boyutlandırma ekran sınırlarına kadar serbest
            // olmalı; tam ekran (1280x720, 1920x1080, 2560x1600) ile tutarlılık için.
            max_width: 4096,
            max_height: 4096,
            is_focused: false,
            is_minimized: false,
            is_open,
            is_maximized: false,
            restore: None,
            z: 0,
            scroll: scrollbar::ScrollState::new(),
            content_w: width,
            content_h: height.saturating_sub(titlebar_h()),
        }
    }

    pub fn get_state(&self) -> WindowState {
        WindowState {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            is_open: self.is_open,
        }
    }

    pub fn apply_state(&mut self, state: WindowState) {
        self.x = state.x;
        self.y = state.y;
        self.width = state.width;
        self.height = state.height;
        self.is_open = state.is_open;
        if self.is_open {
            self.is_minimized = false;
        }
    }

    /// Başlık çubuğu kontrol düğmesi dikdörtgenleri.
    /// Boyutlar aktif font ölçeğine göre büyür; başlık çubuğunda dikey ortalanır.
    pub fn close_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (self.x + self.width.saturating_sub(sp(26)), self.y + (titlebar_h().saturating_sub(b)) / 2, b, b)
    }
    pub fn max_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (self.x + self.width.saturating_sub(sp(46)), self.y + (titlebar_h().saturating_sub(b)) / 2, b, b)
    }
    pub fn min_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (self.x + self.width.saturating_sub(sp(66)), self.y + (titlebar_h().saturating_sub(b)) / 2, b, b)
    }
    /// Pencere kenarlarını yeniden boyutlandırmak için hit alanı.
    pub fn resize_grip_rect(&self) -> (usize, usize, usize, usize) {
        let g = RESIZE_GRIP;
        (
            self.x + self.width.saturating_sub(g),
            self.y + self.height.saturating_sub(g),
            g,
            g,
        )
    }
    /// Kontrol düğmelerinin bulunduğu başlık bölgesini resize hit-testinden çıkarır.
    fn title_controls_rect(&self) -> (usize, usize, usize, usize) {
        let left = self.min_rect().0;
        let right = self.x + self.width;
        (left, self.y, right.saturating_sub(left), titlebar_h())
    }
    pub fn title_drag_rect(&self) -> (usize, usize, usize, usize) {
        (self.x, self.y, self.width.saturating_sub(sp(66)), titlebar_h())
    }
}

/// Yeniden boyutlandırma yönü.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Yan/alt resize alanı 12px; başlık çakışmasını önlemek için üst alan 4px.
pub const RESIZE_GRIP: usize = 12;
const TOP_RESIZE_GRIP: usize = 4;

impl Window {
    /// Fare konumundaki kenar/köşe resize yönünü bulur. Başlık düğmeleri,
    /// pencere gövdesi ve maximize edilmiş pencereler resize edilmez.
    pub fn resize_edge_at(&self, mx: usize, my: usize) -> Option<ResizeEdge> {
        if !self.is_open || self.is_minimized || self.is_maximized {
            return None;
        }
        // Başlık düğmeleri sağ üst köşeye yakın olsa da tıklama her zaman
        // düğmelere ait olsun; köşe resize alanı bu bölgeye taşmasın.
        if in_rect(mx, my, self.title_controls_rect()) {
            return None;
        }

        let g = RESIZE_GRIP;
        const OUTSIDE_MARGIN: usize = 4;
        let left_bound = self.x.saturating_sub(OUTSIDE_MARGIN);
        let right_bound = self.x + self.width + OUTSIDE_MARGIN;
        let top_bound = self.y.saturating_sub(OUTSIDE_MARGIN);
        let bottom_bound = self.y + self.height + OUTSIDE_MARGIN;

        if mx < left_bound || mx >= right_bound || my < top_bound || my >= bottom_bound {
            return None;
        }

        let left = mx <= self.x + g;
        let edge_right = mx + g >= self.x + self.width;
        let top = my <= self.y + TOP_RESIZE_GRIP;
        let edge_bottom = my + g >= self.y + self.height;

        if top && left { Some(ResizeEdge::TopLeft) }
        else if top && edge_right { Some(ResizeEdge::TopRight) }
        else if edge_bottom && left { Some(ResizeEdge::BottomLeft) }
        else if edge_bottom && edge_right { Some(ResizeEdge::BottomRight) }
        else if left { Some(ResizeEdge::Left) }
        else if edge_right { Some(ResizeEdge::Right) }
        else if top { Some(ResizeEdge::Top) }
        else if edge_bottom { Some(ResizeEdge::Bottom) }
        else { None }
    }
}

/// Aero Snap yaslama türleri: Sol yarı, Sağ yarı veya Tam ekran.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SnapPosition {
    Left,
    Right,
    Top, // Maximize
}

// ---------------------------------------------------------------------------
// Görev çubuğu yerleşimi (hem çizim hem tıklama testi için ortak)
// ---------------------------------------------------------------------------
#[derive(Copy, Clone)]
pub struct TrayLayout {
    pub tb_y: usize,
    pub start: (usize, usize, usize, usize),
    pub clock: (usize, usize, usize, usize),
    pub volume: (usize, usize, usize, usize),
    pub network: (usize, usize, usize, usize),
    pub theme: (usize, usize, usize, usize),
    pub kbd: (usize, usize, usize, usize),
}

/// Başlat menüsü yerleşimi — TEK kanonik kaynak.
///
/// Grup başlıkları ve öğeler aynı sıralı düzende yerleştirilir; hem çizim hem
/// tıklama testi bu fonksiyonları kullanır, böylece görsel öğe ile tıklanan öğe
/// matematiksel olarak birebir örtüşür (yanlış öğe tetiklenmez).
///
/// Düzen: profil şeridi (40px) → her grup için bir başlık (GRP_H) + öğeler (ITEM_H).

/// Başlat menüsünün toplam yüksekliği.
pub const MENU_H: usize = 230;
/// Menü içerik başlangıcı (profil ve arama alanından sonra).
pub const MENU_CONTENT_TOP: usize = 82;
/// Grup başlığı yüksekliği.
pub const MENU_GROUP_H: usize = 18;
/// Menü öğesi yüksekliği (satır aralığı).
pub const MENU_ITEM_H: usize = 20;
/// Menü öğesi yüksekliği (tıklanabilir alan) — eski adlarla uyumluluk için.
pub const MENU_ITEM_STEP: usize = 20;

/// Grup tanımı: (görünen başlık, ilk öğe indeksi, öğe sayısı).
/// İndeksler menü öğesi sırasına göre ardışıktır (bkz. `menu_items`).
pub const MENU_GROUPS: [(&str, usize, usize); 5] = [
    ("Sistem", 0, 7),
    ("Üretkenlik", 7, 3),
    ("Medya", 10, 5),
    ("Kurulu", 15, 8),
    ("Güç", 23, 2),
];

impl MinGuiCompositor {
    // -----------------------------------------------------------------------
    // 2. Pencere Çerçevesi ve Başlık Çubuğu
    // -----------------------------------------------------------------------
    pub fn window_icon_index(win_idx: usize) -> usize {
        match win_idx {
            0 => 0, // Dosya Yöneticisi -> Bilgisayarım
            1 => 4, // Ayarlar -> Dişli
            2 => 1, // Not Defteri -> Notlar
            3 => 2, // Web Tarayıcısı -> SarmanNet
            4 => 1, // Resim Gösterici -> Görsel
            5 => 3, // Müzik Çalar -> Çalma Listesi
            6 => 3, // Video Oynatıcı -> Medya
            7 => 0, // Görev Yöneticisi -> Bilgisayarım
            8 => 4, // Hesap Makinesi -> Dişli
            9 => 1, // Çizim Tahtası -> Çizim
            10 => 0,// Terminal -> Komut İstemi
            11 => 0,// Kurulum Sihirbazı -> Bilgisayarım / Disk
            12 => 1,// 3D Gösterici -> Görsel/Çizim
            13 => 5,// Paket Yöneticisi -> Paket / Kutu
            14 => 4,// Kontrol Paneli -> Dişli
            _ => 0,
        }
    }

    pub(crate) fn window_title(win_idx: usize, english: bool) -> &'static str {
        if english {
            match win_idx {
                0 => "File Manager",
                1 => "System Settings",
                2 => "Notepad",
                3 => "Web Browser",
                4 => "Image Viewer",
                5 => "Music Player",
                6 => "Video Player",
                7 => "Task Manager",
                8 => "Calculator",
                9 => "Paint",
                10 => "Command Prompt",
                11 => "One-Click Install",
                12 => "3D Viewer",
                13 => "Package Manager",
                14 => "Control Panel",
                _ => "Window",
            }
        } else {
            match win_idx {
                0 => "Dosya Yöneticisi",
                1 => "Sistem Ayarları",
                2 => "Not Defteri",
                3 => "Web Gezgini",
                4 => "Resim Gösterici",
                5 => "Müzik Çalar",
                6 => "Video Oynatıcı",
                7 => "Görev Yöneticisi",
                8 => "Hesap Makinesi",
                9 => "Çizim Tahtası",
                10 => "Komut İstemi",
                11 => "Tek Tıkla Kur",
                12 => "3D Gösterici",
                13 => "Paket Yöneticisi",
                14 => "Kontrol Paneli",
                _ => "Pencere",
            }
        }
    }

    pub(crate) fn window_tab_title(win_idx: usize, english: bool) -> &'static str {
        if english {
            match win_idx {
                0 => "Files", 1 => "Settings", 2 => "Notes",
                3 => "Web", 4 => "Image", 5 => "Music",
                6 => "Video", 7 => "Tasks", 8 => "Calc",
                9 => "Paint", 10 => "CMD", 11 => "Install", 12 => "3D", 13 => "Packages", 14 => "Control", _ => "Win",
            }
        } else {
            match win_idx {
                0 => "Dosyalar", 1 => "Ayarlar", 2 => "Notlar",
                3 => "Web", 4 => "Resim", 5 => "Müzik",
                6 => "Video", 7 => "Görevler", 8 => "Hesap",
                9 => "Çizim", 10 => "Komut", 11 => "Kurulum", 12 => "3D", 13 => "Paketler", 14 => "Kontrol", _ => "Pencere",
            }
        }
    }

    pub fn render_window_frame(&self, fb: &mut Framebuffer, win: &Window, mouse: &MouseState, _win_idx: usize) {
        if !win.is_open || win.is_minimized {
            return;
        }
        let mx = mouse.x as usize;
        let my = mouse.y as usize;

        // Gölge (pencere arkasına derinlik)
        fb.draw_shadow(win.x, win.y, win.width, win.height, Color::SHADOW);

        // Çerçeve (border) ve Pencere Gövdesi (Yuvarlatılmış 6px)
        let border = if win.is_focused { Color::ACCENT_BLUE } else { Color::BORDER };
        fb.draw_rounded_rect(win.x.saturating_sub(1), win.y.saturating_sub(1), win.width + 2, win.height + 2, 6, border);
        fb.draw_rounded_rect(win.x, win.y, win.width, win.height, 6, Color::WINDOW_BG);

        // Başlık Çubuğu (dikey gradyan — yuvarlatılmış)
        let (t_top, t_bottom) = if win.is_focused {
            (Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -14))
        } else {
            (Color::TITLE_BAR_INACTIVE, Self::shade(Color::TITLE_BAR_INACTIVE, -10))
        };
        fb.draw_rounded_vgradient(win.x, win.y, win.width, titlebar_h(), 6, t_top, t_bottom);
        fb.draw_rect(win.x, win.y + titlebar_h() - 1, win.width, 1, border);

        // Pencereye özel ilgili program simgesi ve başlık (aktif ölçekte).
        let icon_idx = Self::window_icon_index(win.app_id as usize);
        let icon_sz = Self::icon_size();
        let icon_y = win.y + titlebar_h().saturating_sub(icon_sz) / 2;
        Self::draw_icon_by_index(fb, win.x + sp(8), icon_y, icon_idx);
        let title_col = if win.is_focused { Color::TEXT_MAIN } else { Color::TEXT_MUTED };
        let localized_title = Self::window_title(win.app_id as usize, ui_english());
        let title = if !localized_title.is_empty() { localized_title } else { win.title };
        // Başlık metnini başlık çubuğunda dikey ortala (font ölçeğine duyarlı).
        // Başlık alanı, simge ile kontrol düğmeleri arasına kırpılır; uzun
        // başlıklar pencere dışına veya düğmelerin üzerine taşmaz.
        let text_h = Self::text_height();
        let title_y = win.y + titlebar_h().saturating_sub(text_h) / 2;
        let title_x = win.x + sp(8) + icon_sz + sp(8);
        let title_area_w = win.width.saturating_sub((title_x - win.x) + sp(76));
        if title_area_w > 0 {
            let old_clip = fb.clip();
            fb.set_clip(title_x, win.y, title_area_w, titlebar_h());
            Self::draw_string(fb, title_x, title_y, title, title_col);
            fb.set_clip(old_clip.x0, old_clip.y0,
                old_clip.x1.saturating_sub(old_clip.x0), old_clip.y1.saturating_sub(old_clip.y0));
        }

        // Kontrol Butonları (hover'da parlama — yuvarlatılmış)
        // Boyut, scale'li rect'ten türetilir; bu sayede font büyüdükçe düğmeler de büyür.
        let (cx, cy, base_cb, _) = win.close_rect();
        let hover_close = in_rect(mx, my, win.close_rect());
        let pressed_close = mouse.left_button && hover_close;
        let press_inset = if pressed_close { 2 } else { 0 };
        let (cx, cy, cb) = (cx + press_inset, cy + press_inset, base_cb.saturating_sub(press_inset * 2));
        let close_bg = if pressed_close { Self::shade(Color::ACCENT_RED, -48) }
            else if hover_close { Self::shade(Color::ACCENT_RED, 42) } else { Color::ACCENT_RED };
        fb.draw_rounded_rect(cx, cy, cb, cb, 4, close_bg);
        if pressed_close { fb.draw_rounded_rect_outline(cx, cy, cb, cb, 4, Color::WHITE); }
        let close_mark_color = Self::text_on(close_bg);
        let mark_inset = cb as f32 * 0.29;
        let mark_left = cx as f32 + mark_inset;
        let mark_right = cx as f32 + cb as f32 - mark_inset;
        let mark_top = cy as f32 + mark_inset;
        let mark_bottom = cy as f32 + cb as f32 - mark_inset;
        let mark_width = (sp(16) as f32 / 10.0).max(1.5);
        Self::draw_antialiased_line(fb, mark_left, mark_top, mark_right, mark_bottom, close_mark_color, mark_width);
        Self::draw_antialiased_line(fb, mark_right, mark_top, mark_left, mark_bottom, close_mark_color, mark_width);

        let (mxr, myr, base_mb, _) = win.max_rect();
        let hover_max = in_rect(mx, my, win.max_rect());
        let pressed_max = mouse.left_button && hover_max;
        let max_inset = if pressed_max { 2 } else { 0 };
        let (mxr, myr, mb) = (mxr + max_inset, myr + max_inset, base_mb.saturating_sub(max_inset * 2));
        let max_bg = if pressed_max { Self::shade(Color::ACCENT_GREEN, -48) }
            else if hover_max { Self::shade(Color::ACCENT_GREEN, 42) } else { Color::ACCENT_GREEN };
        fb.draw_rounded_rect(mxr, myr, mb, mb, 4, max_bg);
        if pressed_max { fb.draw_rounded_rect_outline(mxr, myr, mb, mb, 4, Color::WHITE); }
        let max_mark_color = Self::text_on(max_bg);
        let inset = mb as f32 * 0.29;
        let left = mxr as f32 + inset;
        let right = mxr as f32 + mb as f32 - inset;
        let top = myr as f32 + inset;
        let bottom = myr as f32 + mb as f32 - inset;
        let stroke = (sp(14) as f32 / 10.0).max(1.4);
        if win.is_maximized {
            // Restore (Önceki Boyut) simgesi: Arkada sağ-üstte bir pencere karesi ve önde sol-altta bir pencere karesi.
            let size = (right - left) * 0.72;
            let offset = (right - left) * 0.28;

            // Arka pencere (sağ-üst): üst kenar, sağ kenar ve öne kadar uzanan parçalar
            let back_l = left + offset;
            let back_t = top;
            let back_r = back_l + size;
            let back_b = back_t + size;

            // Arka pencere üst kenarı
            Self::draw_antialiased_line(fb, back_l, back_t, back_r, back_t, max_mark_color, stroke);
            // Arka pencere sağ kenarı
            Self::draw_antialiased_line(fb, back_r, back_t, back_r, back_b, max_mark_color, stroke);
            // Arka pencere sol kenarının açıkta kalan üst parçası
            Self::draw_antialiased_line(fb, back_l, back_t, back_l, top + offset, max_mark_color, stroke);
            // Arka pencere alt kenarının açıkta kalan sağ parçası
            Self::draw_antialiased_line(fb, left + size, back_b, back_r, back_b, max_mark_color, stroke);

            // Ön pencere (sol-alt): tam dikdörtgen
            let front_l = left;
            let front_t = top + offset;
            let front_r = front_l + size;
            let front_b = front_t + size;

            Self::draw_antialiased_line(fb, front_l, front_t, front_r, front_t, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, front_r, front_t, front_r, front_b, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, front_r, front_b, front_l, front_b, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, front_l, front_b, front_l, front_t, max_mark_color, stroke);
        } else {
            Self::draw_antialiased_line(fb, left, top, right, top, max_mark_color, (sp(14) as f32 / 10.0).max(1.4));
            Self::draw_antialiased_line(fb, right, top, right, bottom, max_mark_color, (sp(14) as f32 / 10.0).max(1.4));
            Self::draw_antialiased_line(fb, right, bottom, left, bottom, max_mark_color, (sp(14) as f32 / 10.0).max(1.4));
            Self::draw_antialiased_line(fb, left, bottom, left, top, max_mark_color, (sp(14) as f32 / 10.0).max(1.4));
        }

        let (nx, ny, base_nb, _) = win.min_rect();
        let hover_min = in_rect(mx, my, win.min_rect());
        let pressed_min = mouse.left_button && hover_min;
        let min_inset = if pressed_min { 2 } else { 0 };
        let (nx, ny, nb) = (nx + min_inset, ny + min_inset, base_nb.saturating_sub(min_inset * 2));
        let min_bg = if pressed_min { Self::shade(Color::ACCENT_YELLOW, -48) }
            else if hover_min { Self::shade(Color::ACCENT_YELLOW, 42) } else { Color::ACCENT_YELLOW };
        fb.draw_rounded_rect(nx, ny, nb, nb, 4, min_bg);
        if pressed_min { fb.draw_rounded_rect_outline(nx, ny, nb, nb, 4, Color::WHITE); }
        let bar_inset = nb as f32 * 0.24;
        let bar_y = ny as f32 + nb as f32 * 0.67;
        Self::draw_antialiased_line(
            fb,
            nx as f32 + bar_inset,
            bar_y,
            nx as f32 + nb as f32 - bar_inset,
            bar_y,
            Self::text_on(min_bg),
            (sp(14) as f32 / 10.0).max(1.4),
        );
    }
}

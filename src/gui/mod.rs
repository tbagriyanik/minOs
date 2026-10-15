pub mod font_render;
pub mod theme;
pub mod clipboard;
pub mod scrollbar;

pub use scrollbar::{ScrollState, ScrollbarGeom};

use crate::drivers::video::Framebuffer;
use crate::drivers::mouse::MouseState;
use crate::drivers::keyboard::KeyboardState;
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

/// Draw one frame of the centered minOs loading transition.
pub fn render_loading_transition(fb: &mut Framebuffer, width: usize, height: usize, phase: usize, message: &str) {
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    let cx = width / 2;
    let cy = height / 2;
    let icon_size = 48 + (phase % 7) * 4;
    MinGuiCompositor::draw_cat_icon_at_size(fb, cx.saturating_sub(icon_size / 2), cy.saturating_sub(78), icon_size);

    let brand = "minOs";
    let brand_w = MinGuiCompositor::text_width(brand, 3);
    MinGuiCompositor::draw_string_scaled(fb, cx.saturating_sub(brand_w / 2), cy.saturating_sub(10), brand, Color::TEXT_MAIN, 3);

    let bar_w = 220.min(width.saturating_sub(48));
    let bar_x = cx.saturating_sub(bar_w / 2);
    let bar_y = cy + 34;
    fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::TASKBAR_BG);
    let sweep_w = (bar_w / 3).max(12);
    let travel = bar_w.saturating_sub(sweep_w);
    let sweep_x = bar_x + (phase * 19 % (travel + 1));
    fb.draw_rounded_rect(sweep_x, bar_y + 1, sweep_w, 6, 3, Color::ACCENT_BLUE);

    let dots = match phase % 4 { 0 => ".  ", 1 => ".. ", 2 => "...", _ => "   " };
    let text_w = MinGuiCompositor::text_width(message, 1);
    let dots_w = MinGuiCompositor::text_width(dots, 1);
    let group_w = text_w + 4 + dots_w;
    let text_x = cx.saturating_sub(group_w / 2);
    MinGuiCompositor::draw_string(fb, text_x, bar_y + 20, message, Color::TEXT_MUTED);
    let dots_x = text_x + text_w + 4;
    MinGuiCompositor::draw_string(fb, dots_x, bar_y + 20, dots, Color::ACCENT_BLUE);
    fb.flush();
}

/// Draw a short-lived busy panel before a synchronous operation begins.
/// The static indicator remains visible until the current disk call returns.
pub fn draw_busy_overlay(fb: &mut Framebuffer, _mx: usize, _my: usize, message: &str, progress: usize) {
    let w = 340.min(fb.width.saturating_sub(24));
    let h = 104;
    let x = fb.width.saturating_sub(w) / 2;
    let y = fb.height.saturating_sub(h) / 2;
    fb.draw_rounded_rect(x, y, w, h, 8, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(x, y, w, h, 8, Color::ACCENT_BLUE);
    MinGuiCompositor::draw_string(fb, x + 18, y + 16, message, Color::TEXT_MAIN);
    let bx = x + 18;
    let by = y + 52;
    let bw = w.saturating_sub(36);
    let pct = progress.min(100);
    fb.draw_rounded_rect(bx, by, bw, 14, 4, Color::DARK_BG);
    fb.draw_rounded_rect(bx + 1, by + 1, bw.saturating_sub(2) * pct / 100, 12, 3, Color::ACCENT_BLUE);
    // Yüzde etiketi: çubukla tutarlı tek bir değer gösterir.
    let mut label = TextBuf::new();
    label.push_usize(pct);
    label.push_str("%");
    MinGuiCompositor::draw_string(fb, bx, by + 22, label.as_str(), Color::TEXT_MUTED);
    fb.flush_rect((x, y, w, h));
}

/// Dolgulu kum saati işlem imleci. İmlecin altındaki arka planı saklar;
/// böylece bir sonraki karede `restore_cursor_background` ile temizlenebilir.
pub fn draw_busy_hourglass(fb: &mut Framebuffer, mx: usize, my: usize) {
    fb.save_cursor_background(mx, my);
    // 8x9 kum saati deseni (bit 7 = sol sütun).
    const HOURGLASS: [u8; 8] = [
        0b11111110,
        0b01111100,
        0b00111000,
        0b00010000,
        0b00111000,
        0b01111100,
        0b11111110,
        0b00000000,
    ];
    for (row, bits) in HOURGLASS.iter().enumerate() {
        for col in 0..8 {
            if bits & (1 << (7 - col)) != 0 {
                fb.put_pixel(mx + col, my + row, Color::ACCENT_YELLOW);
            }
        }
    }
    fb.flush_rect((mx, my, 10, 10));
}

/// Önyükleme hatası ekranı. Kernel `installed` modunda başlatılmak istendiğinde
/// ancak diskte geçerli bir minOs kurulumu (MINOS.SYS) bulunamazsa gösterilir.
/// Metin TR ve EN olarak birlikte çizilir; bu noktada ayarlar yüklenmemiş
/// olduğundan dil tercihi bilinemez.
pub fn render_boot_error(fb: &mut Framebuffer, width: usize, height: usize) {
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);

    // Üst hata şeridi
    fb.draw_rect(0, 0, width, 52, Color::ACCENT_RED);
    let title = "Önyükleme Hatası  /  Boot Error";
    let title_w = MinGuiCompositor::text_width(title, 2);
    MinGuiCompositor::draw_string_scaled(
        fb,
        width.saturating_sub(title_w) / 2,
        14,
        title,
        Color::WHITE,
        2,
    );

    // Bilgi kutusu
    let box_w = 640.min(width.saturating_sub(48)).max(320);
    let box_h = 300.min(height.saturating_sub(96)).max(180);
    let box_x = width.saturating_sub(box_w) / 2;
    let box_y = 52 + (height.saturating_sub(52 + box_h)) / 2;
    fb.draw_rounded_rect(box_x, box_y, box_w, box_h, 10, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(box_x, box_y, box_w, box_h, 10, Color::ACCENT_RED);

    let lines: [(&str, u32); 8] = [
        ("Diskte geçerli bir minOs kurulumu bulunamadı.", Color::TEXT_MAIN),
        ("No valid minOs installation was found on disk.", Color::TEXT_MUTED),
        ("", Color::TEXT_MUTED),
        ("Bu önyükleme seçeneği yalnızca kurulu sistem içindir.", Color::TEXT_MAIN),
        ("This boot option is only for the installed system.", Color::TEXT_MUTED),
        ("", Color::TEXT_MUTED),
        ("Kurulum için 'minOs Live Environment' ile açıp sihirbazı tamamlayın.", Color::ACCENT_YELLOW),
        ("Boot 'minOs Live Environment' and complete the setup wizard.", Color::ACCENT_YELLOW),
    ];

    let mut y = box_y + 24;
    for (text, color) in lines.iter() {
        if !text.is_empty() {
            MinGuiCompositor::draw_string(fb, box_x + 24, y, text, *color);
        }
        y += 26;
    }

    let footer = "Sistem bu ekranda bekleyecek. Çıkmak için sistemi kapatın.  /  System will stay here. Power off to exit.";
    let footer_w = MinGuiCompositor::text_width(footer, 1);
    if footer_w <= width.saturating_sub(24) {
        MinGuiCompositor::draw_string(
            fb,
            width.saturating_sub(footer_w) / 2,
            box_y + box_h + 18,
            footer,
            Color::TEXT_MUTED,
        );
    } else {
        MinGuiCompositor::draw_string(fb, 24, box_y + box_h + 18, "Çıkmak için sistemi kapatın. / Power off to exit.", Color::TEXT_MUTED);
    }

    fb.flush();
}

/// Hoş geldin ekranı seçim alanları; çizim ve giriş işleme aynı geometriyi kullanır.
fn welcome_panel(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let w = 480.min(width.saturating_sub(24)).max(320);
    let h = 290.min(height.saturating_sub(24)).max(240);
    ((width.saturating_sub(w)) / 2, (height.saturating_sub(h)) / 2, w, h)
}

pub fn welcome_language_rect(width: usize, height: usize, index: usize) -> (usize, usize, usize, usize) {
    let (x, y, _, _) = welcome_panel(width, height);
    (x + 28 + index * 140, y + 94, 126, 32)
}

pub fn welcome_theme_rect(width: usize, height: usize, index: usize) -> (usize, usize, usize, usize) {
    let (x, y, _, _) = welcome_panel(width, height);
    (x + 28 + index * 82, y + 164, 74, 50)
}

pub fn welcome_continue_rect(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let (x, y, w, h) = welcome_panel(width, height);
    (x + w.saturating_sub(168), y + h.saturating_sub(50), 140, 36)
}

pub fn render_welcome_screen(fb: &mut Framebuffer, width: usize, height: usize, english: bool, selected: theme::ThemeId, mouse: &MouseState) {
    let (x, y, w, h) = welcome_panel(width, height);
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    fb.draw_shadow(x, y, w, h, Color::SHADOW);
    fb.draw_rounded_rect(x, y, w, h, 10, Color::WINDOW_BG);
    fb.draw_rounded_vgradient(x, y, w, 48, 10, Color::TITLE_BAR, Color::TITLE_BAR_INACTIVE);
    MinGuiCompositor::draw_cat_icon_scaled(fb, x + 18, y + 12);
    MinGuiCompositor::draw_string(fb, x + 50, y + 16, if english { "Welcome to minOs" } else { "minOs'a hoş geldiniz" }, Color::TEXT_MAIN);

    // 1. Dil Seçimi Bölümü
    MinGuiCompositor::draw_string(fb, x + 28, y + 70,
        if english { "Interface Language" } else { "Arayüz Dili" }, Color::ACCENT_BLUE);

    for i in 0..2 {
        let rect = welcome_language_rect(width, height, i);
        let active = (i == 0 && !english) || (i == 1 && english);
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 6,
            if active { Color::ACCENT_BLUE } else { Color::TASKBAR_BG });
        fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3,
            if active { Color::ACCENT_BLUE } else { Color::BORDER });
        MinGuiCompositor::draw_string(fb, rect.0 + 26, rect.1 + 8,
            if i == 0 { "Türkçe" } else { "English" },
            if active { MinGuiCompositor::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN });
    }

    // 2. Masaüstü Teması Bölümü
    MinGuiCompositor::draw_string(fb, x + 28, y + 140,
        if english { "Desktop Theme" } else { "Masaüstü Teması" }, Color::ACCENT_BLUE);
    for (i, id) in theme::ThemeId::ALL.iter().enumerate() {
        let rect = welcome_theme_rect(width, height, i);
        let (background, accent) = theme::preview_colors(*id);
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 6, background);
        fb.draw_rounded_rect(rect.0 + 6, rect.1 + 6, rect.2.saturating_sub(12), 6, 2, accent);
        let name = if english {
            match id {
                theme::ThemeId::VelvetDark => "Velvet",
                theme::ThemeId::Daylight => "Daylight",
                theme::ThemeId::Nord => "Nord",
                theme::ThemeId::TerminalGreen => "Terminal",
                theme::ThemeId::Sunset => "Sunset",
            }
        } else { id.short_name() };
        MinGuiCompositor::draw_string(fb, rect.0 + 8, rect.1 + 25, name, MinGuiCompositor::text_on(background));
        if *id == selected {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::ACCENT_BLUE);
            fb.draw_rect_outline(rect.0.saturating_sub(1), rect.1.saturating_sub(1), rect.2 + 2, rect.3 + 2, Color::ACCENT_BLUE);
        } else {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::BORDER);
        }
    }

    // 3. Alt Kısım: Bilgi İpucu & Başla Düğmesi
    MinGuiCompositor::draw_string(fb, x + 28, y + h.saturating_sub(40),
        if english { "Press Enter to start" } else { "Başlamak için Enter'a basın" }, Color::TEXT_MUTED);

    let button = welcome_continue_rect(width, height);
    fb.draw_rounded_rect(button.0, button.1, button.2, button.3, 6, Color::ACCENT_GREEN);
    let btn_text = if english { "Get Started" } else { "Başla" };
    let text_col = MinGuiCompositor::text_on(Color::ACCENT_GREEN);
    MinGuiCompositor::draw_string(fb, button.0 + 20, button.1 + 10, btn_text, text_col);
    // Başlat menüsündeki gibi zarif antialiased sağ ok (chevron) simgesi
    let arrow_x = (button.0 + button.2 - 20) as f32;
    let arrow_y = (button.1 + button.3 / 2) as f32;
    MinGuiCompositor::draw_antialiased_line(fb, arrow_x - 3.0, arrow_y - 4.5, arrow_x + 1.5, arrow_y, text_col, 2.0);
    MinGuiCompositor::draw_antialiased_line(fb, arrow_x + 1.5, arrow_y, arrow_x - 3.0, arrow_y + 4.5, text_col, 2.0);

    let pointer_x = mouse.x.max(0) as usize;
    let pointer_y = mouse.y.max(0) as usize;
    fb.draw_rect(pointer_x, pointer_y, 5, 5, Color::WHITE);
    fb.draw_rect_outline(pointer_x, pointer_y, 5, 5, Color::BLACK);
    fb.flush();
}

const BASE_FONT_SIZE: f32 = 14.0;

// ---------------------------------------------------------------------------
// Metin tamponu (no_std, sabit boyutlu) — sayı/tarih biçimlendirme için.
// ---------------------------------------------------------------------------
#[derive(Clone, Copy)]
pub struct TextBuf {
    buf: [u8; 64],
    len: usize,
}

impl TextBuf {
    pub const fn new() -> Self {
        Self { buf: [0; 64], len: 0 }
    }

    /// Sabit metinden hücreler arası (const) tampon oluşturur.
    /// Metin `buf`'a sığmazsa 64 baytta kırpılır.
    pub const fn from_str(s: &str) -> Self {
        let mut buf = [0u8; 64];
        let bytes = s.as_bytes();
        let n = if bytes.len() < 64 { bytes.len() } else { 64 };
        let mut i = 0;
        while i < n {
            buf[i] = bytes[i];
            i += 1;
        }
        Self { buf, len: n }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push_byte(&mut self, b: u8) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    pub fn push_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.push_byte(b);
        }
    }

    /// İki haneli sayı (baştaki sıfır ile).
    pub fn push_u2(&mut self, v: u32) {
        self.push_byte(b'0' + ((v / 10) % 10) as u8);
        self.push_byte(b'0' + (v % 10) as u8);
    }

    /// Dört haneli yıl.
    pub fn push_u4(&mut self, v: u32) {
        self.push_byte(b'0' + ((v / 1000) % 10) as u8);
        self.push_byte(b'0' + ((v / 100) % 10) as u8);
        self.push_byte(b'0' + ((v / 10) % 10) as u8);
        self.push_byte(b'0' + (v % 10) as u8);
    }

    pub fn push_usize(&mut self, mut v: usize) {
        let mut tmp = [0u8; 20]; let mut n = 0;
        if v == 0 { self.push_byte(b'0'); return; }
        while v > 0 && n < tmp.len() { tmp[n] = b'0' + (v % 10) as u8; n += 1; v /= 10; }
        while n > 0 { n -= 1; self.push_byte(tmp[n]); }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

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
            max_width: 800,
            max_height: 600,
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
        let b = sp(14);
        (self.x + self.width.saturating_sub(sp(26)), self.y + (titlebar_h().saturating_sub(b)) / 2, b, b)
    }
    pub fn max_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(14);
        (self.x + self.width.saturating_sub(sp(46)), self.y + (titlebar_h().saturating_sub(b)) / 2, b, b)
    }
    pub fn min_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(14);
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
pub const MENU_H: usize = 200;
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
pub const MENU_GROUPS: [(&str, usize, usize); 4] =
    [("Sistem", 0, 5), ("Üretkenlik", 5, 3), ("Medya", 8, 5), ("Güç", 13, 2)];

impl MinGuiCompositor {
    /// Siyah veya beyaz metinden, verilen zemin üzerinde daha okunaklı olanı seçer.
    pub fn text_on(background: u32) -> u32 {
        let c = theme::resolve(background);
        let r = ((c >> 16) & 0xFF) as u32;
        let g = ((c >> 8) & 0xFF) as u32;
        let b = (c & 0xFF) as u32;
        if r * 299 + g * 587 + b * 114 >= 140_000 { Color::BLACK } else { Color::WHITE }
    }

    /// Bir grubun başlığının Y konumu.
    pub fn start_menu_group_y(menu_y: usize, group_index: usize) -> usize {
        menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32)
    }

    /// Bir menü ÖĞESİNİN Y konumu.
    pub fn start_menu_item_y(menu_y: usize, item_index: usize) -> usize {
        let (g, k) = Self::item_group_and_index(item_index);
        let (_, sub_y, _, _) = Self::start_menu_submenu_rect(menu_y, g, menu_y + MENU_H + 4);
        sub_y + sp(28) + k * sp(24)
    }

    pub fn item_group_and_index(index: usize) -> (usize, usize) {
        let mut remaining = index;
        for (g, (_, _, count)) in MENU_GROUPS.iter().enumerate() {
            if remaining < *count {
                return (g, remaining);
            }
            remaining -= *count;
        }
        (3, 0)
    }

    pub fn start_menu_category_rect(menu_y: usize, group_index: usize) -> (usize, usize, usize, usize) {
        let x = 6 + 6;
        let y = menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32) + sp(4);
        let w = sp(190) - 12;
        let h = sp(28);
        (x, y, w, h)
    }

    pub fn start_search_results_rect(menu_y: usize, group_index: usize, tb_y: usize, count: usize) -> (usize, usize, usize, usize) {
        let sub_w = sp(230);
        let sub_h = sp(28) + count * sp(24) + sp(6);
        let sub_x = 6 + sp(190) + 4;
        let cat_y = menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32) + sp(4);
        let sub_y = cat_y.min(tb_y.saturating_sub(sub_h + 4));
        (sub_x, sub_y, sub_w, sub_h)
    }

    pub fn start_menu_submenu_rect(menu_y: usize, group_index: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let sub_w = sp(230);
        let count = MENU_GROUPS[group_index.min(3)].2;
        let sub_h = sp(28) + count * sp(24) + sp(6);
        let sub_x = 6 + sp(190) + 4;
        let cat_y = menu_y + sp(74) + group_index * sp(32) + sp(4);
        let sub_y = cat_y.min(tb_y.saturating_sub(sub_h + 4));
        (sub_x, sub_y, sub_w, sub_h)
    }

    pub fn start_menu_subitem_rect(menu_y: usize, group_index: usize, item_in_group: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let (sub_x, sub_y, sub_w, _) = Self::start_menu_submenu_rect(menu_y, group_index, tb_y);
        let x = sub_x + 6;
        let y = sub_y + sp(28) + item_in_group * sp(24);
        let w = sub_w - 12;
        let h = sp(22);
        (x, y, w, h)
    }

    /// Başlat menüsü öğesinin tıklanabilir dikdörtgeni.
    pub fn start_menu_item_rect(menu_y: usize, index: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let (g, k) = Self::item_group_and_index(index);
        Self::start_menu_subitem_rect(menu_y, g, k, tb_y)
    }

    /// Menü öğesinin hedef uygulama indeksini döndürür.
    pub fn start_menu_app_idx(index: usize) -> Option<usize> {
        const APP_IDX: [usize; 13] = [0, 1, 7, 10, 11, 2, 8, 9, 3, 4, 5, 6, 12];
        APP_IDX.get(index).copied()
    }
    /// Sorguyu bir ad içinde **parça (substring)** olarak arar ve bulunduğu
    /// konumun puanını döner. Küçük puan daha iyi eşleşmedir:
    /// `0` = ad başında, `1` = kelime başında, `2` = ad içinde herhangi bir yerde.
    /// Eşleşme yoksa `None`.
    fn start_search_score(name: &[u8], query: &[u8]) -> Option<u8> {
        if query.is_empty() || name.len() < query.len() {
            return None;
        }
        let first = query[0];
        let mut i = 0usize;
        let limit = name.len() - query.len();
        while i <= limit {
            // Ad içinde sorgunun ilk harfini ara.
            if !name[i].eq_ignore_ascii_case(&first) {
                i += 1;
                continue;
            }
            if name[i..i + query.len()].eq_ignore_ascii_case(query) {
                // Sıralama: ad başı (0), kelime başı (1), diğer (2).
                let score = if i == 0 {
                    0
                } else if name[i - 1] == b' ' {
                    1
                } else {
                    2
                };
                return Some(score);
            }
            i += 1;
        }
        None
    }

    pub fn start_search_matches(&self, english: bool, results: &mut [usize; 13]) -> usize {
        let query = &self.start_search[..self.start_search_len];
        if query.is_empty() { return 0; }
        const NAMES: [[&str; 2]; 13] = [
            ["Dosya Yöneticisi", "File Manager"], ["Sistem Ayarları", "Settings"],
            ["Not Defteri", "Notepad"], ["Web Tarayıcısı", "Web Browser"],
            ["Resim Gösterici", "Image Viewer"], ["Müzik Çalar", "Music Player"],
            ["Video Oynatıcı", "Video Player"], ["Görev Yöneticisi", "Task Manager"],
            ["Hesap Makinesi", "Calculator"], ["Çizim Tahtası", "Paint"],
            ["Komut İstemi", "Terminal"], ["Tek Tıkla Kur", "One-Click Install"],
            ["3D Gösterici", "3D Viewer"],
        ];
        // Önce tüm eşleşmeleri (app_id, puan) olarak topla.
        let mut scored: [(usize, u8); 13] = [(0, 0); 13];
        let mut count = 0;
        for (app_id, names) in NAMES.iter().enumerate() {
            // Kurulu sistemde kurulum uygulaması arama sonuçlarında çıkmaz.
            if app_id == 11 && crate::fs::diskfs::is_installed() { continue; }
            let localized = Self::start_search_score(names[english as usize].as_bytes(), query);
            let english_hit = Self::start_search_score(names[1].as_bytes(), query);
            let score = match (localized, english_hit) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            if let Some(s) = score {
                scored[count] = (app_id, s);
                count += 1;
            }
        }
        // Puanı küçük olan (daha iyi eşleşen) önce gelecek şekilde sırala;
        // eşit puanda orijinal uygulama sırası korunur (kararlı sıralama).
        for i in 1..count {
            let mut j = i;
            while j > 0 && scored[j - 1].1 > scored[j].1 {
                scored.swap(j - 1, j);
                j -= 1;
            }
        }
        for i in 0..count {
            results[i] = scored[i].0;
        }
        count
    }

    pub fn push_start_search(&mut self, ch: char) {
        if ch.is_ascii_alphanumeric() && self.start_search_len < self.start_search.len() {
            self.start_search[self.start_search_len] = ch.to_ascii_lowercase() as u8;
            self.start_search_len += 1;
        }
    }

    pub fn clear_start_search(&mut self) {
        self.start_search_len = 0;
        self.start_search.fill(0);
        self.selected_start_item = 0;
    }

    /// Başlat menüsünü kapatır ve arama durumunu sıfırlar. Menü her kapandığında
    /// kullanılmalı; böylece bir sonraki açılış temiz başlar.
    pub fn close_start_menu(&mut self) {
        self.is_start_menu_open = false;
        self.confirm_shutdown = false;
        self.confirm_restart = false;
        self.clear_start_search();
    }

    pub fn move_start_search_selection(&mut self, english: bool, down: bool) {
        let mut results = [0; 13];
        let count = self.start_search_matches(english, &mut results);
        if down {
            if count > 0 { self.selected_start_item = (self.selected_start_item + 1).min(count - 1); }
        } else {
            self.selected_start_item = self.selected_start_item.saturating_sub(1);
        }
    }

    pub fn selected_start_search_result(&self, english: bool) -> Option<usize> {
        let mut results = [0; 13];
        let count = self.start_search_matches(english, &mut results);
        (self.selected_start_item < count).then_some(results[self.selected_start_item])
    }
    /// Görev çubuğu ilk uygulama sekmesinin X konumu (font ölçeğine duyarlı).
    pub fn taskbar_tab_start() -> usize { sp(118) }
    /// Görev çubuğu sekmesinin genişliği (font ölçeğine duyarlı).
    pub fn taskbar_tab_width() -> usize { sp(84) }
    /// Ardışık sekmeler arası adım (font ölçeğine duyarlı).
    pub fn taskbar_tab_step() -> usize { sp(88) }
    /// Görev çubuğunda kaç pencere sekmesinin sığacağını ve taşma sayacını hesaplar.
    /// Dönen değerler: başlangıç X, sekme genişliği, adım, normal sekme sayısı, toplam yuva.
    pub fn taskbar_tabs_layout(width: usize, height: usize, open_count: usize) -> (usize, usize, usize, usize, usize) {
        let start = Self::taskbar_tab_start();
        let right = Self::tray_layout(width, height).kbd.0.saturating_sub(sp(4));
        let available = right.saturating_sub(start);
        let min_slot = sp(30).max(1);
        let capacity = (available / min_slot).max(1);
        let slots = open_count.min(capacity);
        if slots == 0 { return (start, Self::taskbar_tab_width(), Self::taskbar_tab_step(), 0, 0); }
        let overflow = open_count > capacity;
        let step = (available / slots).max(1).min(Self::taskbar_tab_step());
        let tab_width = Self::taskbar_tab_width().min(step.saturating_sub(sp(4)).max(1));
        let shown = if overflow { slots.saturating_sub(1) } else { slots };
        (start, tab_width, step, shown, slots)
    }
    /// Verilen X konumundaki görev çubuğu sekmesinin dikdörtgeni.
    pub fn taskbar_tab_rect_at(tab_x: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let h = sp(26).min(taskbar_h().saturating_sub(8));
        (tab_x, tb_y + 5, Self::taskbar_tab_width(), h)
    }

    pub fn tray_layout(width: usize, height: usize) -> TrayLayout {
        let tb_y = height.saturating_sub(taskbar_h());
        let by = tb_y + 5;
        // Tray düğmeleri aktif font ölçeğine göre büyür.
        let h = sp(26).min(taskbar_h().saturating_sub(8));
        let pad = 8;
        // Genişlikler, çizilen ikon + metnin gerçek piksel genişliğine göre
        // seçilir; hiçbir tepsi öğesi metni yanındaki öğeye taşmaz veya
        // yapışmaz. (Metin hücresi artık 10px olduğu için ölçüler buna göre.)
        //   saat:  "HH:MM:SS" = 8 hücre = 80px + sol boşluk -> 92px
        //   ses:   ikon ~26px -> 46px
        //   ağ:    ikon 24px + "eth0" 40px -> 72px
        //   tema:  örnek 16px + aktif tema adının ölçülen genişliği
        //   klavye:ikon 30px + "TR"/"EN" 20px -> 54px
        let clock_w = sp(92);
        let vol_w = sp(44);
        let net_w = sp(68);
        let theme_w = (sp(22) + MinGuiCompositor::text_width(theme::current().short_name(), 1) + sp(4)).max(sp(38));
        let kbd_w = sp(50);
        let gap = sp(8);
        let clock_x = width.saturating_sub(pad + clock_w);
        let vol_x = clock_x.saturating_sub(gap + vol_w);
        let net_x = vol_x.saturating_sub(gap + net_w);
        let theme_x = net_x.saturating_sub(gap + theme_w);
        let kbd_x = theme_x.saturating_sub(gap + kbd_w);
        TrayLayout {
            tb_y,
            start: (6, by, sp(112), h),
            clock: (clock_x, by, clock_w, h),
            volume: (vol_x, by, vol_w, h),
            network: (net_x, by, net_w, h),
            theme: (theme_x, by, theme_w, h),
            kbd: (kbd_x, by, kbd_w, h),
        }
    }

    // -------------------------------------------------------------------
    // Masaüstü sol şerit hızlı simgeleri — TEK kanonik kaynak.
    // Hem çizim (`render_desktop_bg`) hem tıklama testi bu fonksiyonları
    // kullanır; /desktop dosya ızgarası da şeridin sağından başlar, böylece
    // klasör simgeleri hızlı simgelerin üzerine binmez.
    // -------------------------------------------------------------------
    /// Hızlı simge etiketleri (TR + EN). Şerit genişliği dile göre değişmesin
    /// diye her iki dilin en uzun etiketi ölçülür.
    const DESKTOP_QUICK_LABELS: [&str; 12] = [
        "Bilgisayarım", "Notlar", "SarmanNet", "Çalma Listesi", "Sistem Ayarları", "minOs'u Kur",
        "Computer", "Notes", "SarmanNet", "Playlist", "Settings", "Install minOs",
    ];

    /// Kurulu sistemde "minOs'u Kur" kısayolu gösterilmez.
    pub fn desktop_quick_count() -> usize {
        if crate::fs::diskfs::is_installed() { 5 } else { Self::DESKTOP_QUICK_LABELS.len() / 2 }
    }

    /// Hızlı simge etiketlerinin ölçülen en uzun genişliği (aktif ölçekte).
    fn desktop_quick_label_w() -> usize {
        let mut m = 0;
        for l in Self::DESKTOP_QUICK_LABELS {
            m = m.max(Self::text_width(l, 1));
        }
        m
    }

    /// Hızlı simgenin tıklanabilir alanı (kart + etiket).
    pub fn desktop_quick_rect(i: usize) -> (usize, usize, usize, usize) {
        let card_w = sp(34);
        let card_h = sp(30);
        let w = card_w + sp(10) + Self::desktop_quick_label_w() + sp(12);
        (sp(20), sp(30) + i * sp(54), w, card_h)
    }

    /// Hızlı simge şeridinin sağ kenarı. `/desktop` dosya ızgarası bu
    /// konumun sağından başlar; iki simge grubu asla çakışmaz.
    pub fn desktop_strip_right() -> usize {
        sp(20) + sp(34) + sp(10) + Self::desktop_quick_label_w() + sp(16)
    }

    /// Sağ üst filigran (watermark) alanı. Dosya ızgarası bu köşeye taşmaz.
    pub fn desktop_watermark_rect(width: usize) -> (usize, usize, usize, usize) {
        let title = if crate::fs::diskfs::is_installed() {
            "minOs Sarman Edition *"
        } else {
            "minOs (Live)"
        };
        let block_w = Self::icon_size() + sp(8) + Self::text_width(title, 1);
        let x = width.saturating_sub(sp(16) + block_w);
        (x, 0, block_w + sp(16), sp(70))
    }
}

pub fn in_rect(mx: usize, my: usize, r: (usize, usize, usize, usize)) -> bool {
    mx >= r.0 && mx < r.0 + r.2 && my >= r.1 && my < r.1 + r.3
}

// ---------------------------------------------------------------------------
// Bağlam Menüsü (sağ tık) — tıklanan yere göre değişir
// ---------------------------------------------------------------------------
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ContextKind {
    /// Masaüstü boş alanı
    Desktop,
    /// Dosya Yöneticisi içindeki bir dosya
    FileManager,
    /// Not Defteri metin alanı (Kes/Kopyala/Yapıştır...)
    Notepad,
    /// Görev Yöneticisi süreç satırı (Sonlandır...)
    TaskManager,
    /// Pencere başlık çubuğu / gövdesi
    Window,
}

/// Masaüstü sağ tık menüsü.
pub const CTX_DESKTOP: [(&str, u32); 9] = [
    ("Dosya Yöneticisini Aç", Color::ACCENT_BLUE),
    ("Yeni Not Oluştur", Color::TEXT_MAIN),
    ("Yeni Dosya Oluştur", Color::ACCENT_BLUE),
    ("Yeni Klasör Oluştur", Color::ACCENT_YELLOW),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Sistem Ayarları", Color::ACCENT_PINK),
    ("Görev Yöneticisi", Color::ACCENT_YELLOW),
    ("Tema Değiştir", Color::ACCENT_GREEN),
    ("Masaüstünü Yenile", Color::TEXT_MUTED),
];

/// Dosya Yöneticisi'nde bir dosyaya veya klasöre sağ tık menüsü.
pub const CTX_FILE: [(&str, u32); 10] = [
    ("Aç / Klasöre Gir", Color::ACCENT_GREEN),
    ("Not Defterinde Düzenle", Color::TEXT_MAIN),
    ("Kes", Color::ACCENT_RED),
    ("Kopyala", Color::ACCENT_BLUE),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Yeni Dosya Oluştur", Color::ACCENT_BLUE),
    ("Yeni Klasör Oluştur", Color::ACCENT_YELLOW),
    ("Adını Değiştir", Color::ACCENT_YELLOW),
    ("Dosyayı / Klasörü Sil", Color::ACCENT_RED),
    ("Özellikler", Color::TEXT_MUTED),
];

/// Not Defteri metin alanına sağ tık menüsü.
pub const CTX_NOTEPAD: [(&str, u32); 6] = [
    ("Kes", Color::ACCENT_RED),
    ("Kopyala", Color::ACCENT_BLUE),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Sil", Color::ACCENT_YELLOW),
    ("Tümünü Seç", Color::ACCENT_PINK),
    ("Tümünü Temizle", Color::TEXT_MUTED),
];

/// Görev Yöneticisi süreç satırına sağ tık menüsü.
pub const CTX_TASKMANAGER: [(&str, u32); 1] = [
    ("Sonlandır", Color::ACCENT_RED),
];

/// Pencere başlık çubuğuna sağ tık menüsü.
pub const CTX_WINDOW: [(&str, u32); 4] = [
    ("Küçült", Color::ACCENT_YELLOW),
    ("Maksimize Et / Geri Yükle", Color::ACCENT_GREEN),
    ("Pencereyi Ortala", Color::ACCENT_BLUE),
    ("Kapat", Color::ACCENT_RED),
];

const CTX_DESKTOP_EN: [(&str, u32); 9] = [
    ("Open File Manager", Color::ACCENT_BLUE), ("New Note", Color::TEXT_MAIN),
    ("New File", Color::ACCENT_BLUE), ("New Folder", Color::ACCENT_YELLOW), ("Paste", Color::ACCENT_GREEN),
    ("System Settings", Color::ACCENT_PINK), ("Task Manager", Color::ACCENT_YELLOW),
    ("Change Theme", Color::ACCENT_GREEN), ("Refresh Desktop", Color::TEXT_MUTED),
];
const CTX_FILE_EN: [(&str, u32); 10] = [
    ("Open / Enter Folder", Color::ACCENT_GREEN), ("Edit in Notepad", Color::TEXT_MAIN),
    ("Cut", Color::ACCENT_RED), ("Copy", Color::ACCENT_BLUE), ("Paste", Color::ACCENT_GREEN),
    ("New File", Color::ACCENT_BLUE), ("New Folder", Color::ACCENT_YELLOW),
    ("Rename", Color::ACCENT_YELLOW), ("Delete", Color::ACCENT_RED), ("Properties", Color::TEXT_MUTED),
];
const CTX_NOTEPAD_EN: [(&str, u32); 6] = [
    ("Cut", Color::ACCENT_RED), ("Copy", Color::ACCENT_BLUE), ("Paste", Color::ACCENT_GREEN),
    ("Delete", Color::ACCENT_YELLOW), ("Select All", Color::ACCENT_PINK), ("Clear All", Color::TEXT_MUTED),
];
const CTX_TASKMANAGER_EN: [(&str, u32); 1] = [
    ("End Task", Color::ACCENT_RED),
];
const CTX_WINDOW_EN: [(&str, u32); 4] = [
    ("Minimize", Color::ACCENT_YELLOW), ("Maximize / Restore", Color::ACCENT_GREEN),
    ("Center Window", Color::ACCENT_BLUE), ("Close", Color::ACCENT_RED),
];

pub const CONTEXT_MENU_W: usize = 226;
pub const CONTEXT_MENU_ITEM_H: usize = 26;

/// Bağlam menüsü başlık (header) yüksekliği — aktif font ölçeğine göre.
#[inline(always)]
pub fn context_header_h() -> usize { crate::gui::sp(CONTEXT_MENU_ITEM_H - 4) }

/// Bağlam menüsü öğe yüksekliği — aktif font ölçeğine göre.
#[inline(always)]
pub fn context_item_h() -> usize { crate::gui::sp(CONTEXT_MENU_ITEM_H) }


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
            alt_tab_order: [0; MAX_WINDOWS],
            alt_tab_count: 0,
            alt_tab_selection: 0,
            z_counter: 0,
        }
    }

    pub fn add_window(&mut self, mut win: Window) -> bool {
        for idx in 0..self.windows.len() {
            if self.windows[idx].is_none() {
                // Sabit uygulama pencereleri 0..=12 arası slotlara eklenir (13 uygulama);
                // 3D Gösterici (idx 12) dahil edilmezse app_id MAX kalır ve açılamaz.
                if win.app_id == u8::MAX && idx < 13 { win.app_id = idx as u8; }
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
        if app_id >= 13 { return None; }
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

// ---------------------------------------------------------------------------
// Çizim yardımcıları
// ---------------------------------------------------------------------------
impl MinGuiCompositor {
    /// Aktif temadan okunan RGB ile bir rengi aydınlatır/koyulaştırır.
    pub fn shade(color: u32, delta: i32) -> u32 {
        let c = theme::resolve(color);
        let r = (((c >> 16) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        let g = (((c >> 8) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        let b = ((c & 0xFF) as i32 + delta).clamp(0, 255) as u32;
        (r << 16) | (g << 8) | b
    }

    /// Inria Sans ile genel arayüz metni.
    pub fn draw_string(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
        let size = BASE_FONT_SIZE * font_zoom() as f32 / 100.0;
        font_render::draw(fb, font_render::Face::Inria, x, y, text, color, size);
    }

    /// Monospace metni CMD ekranında Geist Mono ile çizer.
    pub fn draw_terminal_string(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
        let size = BASE_FONT_SIZE * font_zoom() as f32 / 100.0;
        font_render::draw(fb, font_render::Face::Geist, x, y, text, color, size);
    }

    pub fn terminal_text_width(text: &str) -> usize {
        let size = BASE_FONT_SIZE * font_zoom() as f32 / 100.0;
        font_render::text_width(font_render::Face::Geist, text, size)
    }

    /// Aktif ölçekte bir metin satırının görsel yüksekliği (px).
    /// `blit_glyph` hedef yüksekliğiyle ve dikey ortalama hesaplarıyla tutarlıdır.
    #[inline(always)]
    pub fn text_height() -> usize {
        let zoom = font_zoom() as usize;
        (font_render::text_height(font_render::Face::Inria, BASE_FONT_SIZE) * zoom + 50) / 100
    }

    /// Ölçeklenebilir Inria Sans metni.
    pub fn draw_string_scaled(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32, scale: usize) {
        let size = BASE_FONT_SIZE * font_zoom() as f32 / 100.0 * scale.max(1) as f32;
        font_render::draw(fb, font_render::Face::Inria, x, y, text, color, size);
    }

    /// Gölgeli/vurgulu metin — tek geçişli temiz çizim yaparak font bulanıklığını ve gölge çakışmasını önler.
    pub fn draw_string_shadow(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        _shadow: u32,
        scale: usize,
    ) {
        Self::draw_string_scaled(fb, x, y, text, color, scale);
    }

    /// Inria Sans kullanarak metin genişliğini hesaplar.
    pub fn text_width(text: &str, scale: usize) -> usize {
        let zoomed_size = BASE_FONT_SIZE * font_zoom() as f32 / 100.0 * scale.max(1) as f32;
        font_render::text_width(font_render::Face::Inria, text, zoomed_size)
    }

    /// Etiketli, hover durumuna duyarlı düğme (Yuvarlatılmış köşeli).
    pub fn draw_button(
        fb: &mut Framebuffer,
        r: (usize, usize, usize, usize),
        bg: u32,
        label: &str,
        _label_color: u32,
        hovered: bool,
    ) {
        let bg = if hovered { Self::shade(bg, 26) } else { bg };
        fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 5, bg);
        fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 5, Color::BORDER);
        let tw = Self::text_width(label, 1).saturating_sub(1);
        let tx = r.0 + r.2.saturating_sub(tw) / 2;
        let text_h = Self::text_height();
        let ty = r.1 + r.3.saturating_sub(text_h) / 2;
        let readable_label_color = Self::text_on(bg);
        Self::draw_string_clipped(fb, tx, ty, label, readable_label_color,
            (r.0 + 2, r.1 + 1, r.2.saturating_sub(4), r.3.saturating_sub(2)));
    }

    /// Metni belirtilen dikdörtgenin içinde kırparak çizer.
    pub fn draw_string_clipped(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
        rect: (usize, usize, usize, usize),
    ) {
        let old_clip = fb.clip();
        let clip_x = old_clip.x0.max(rect.0);
        let clip_y = old_clip.y0.max(rect.1);
        let clip_right = old_clip.x1.min(rect.0.saturating_add(rect.2));
        let clip_bottom = old_clip.y1.min(rect.1.saturating_add(rect.3));
        fb.set_clip(clip_x, clip_y, clip_right.saturating_sub(clip_x), clip_bottom.saturating_sub(clip_y));
        Self::draw_string(fb, x, y, text, color);
        fb.set_clip(old_clip.x0, old_clip.y0, old_clip.x1.saturating_sub(old_clip.x0), old_clip.y1.saturating_sub(old_clip.y0));
    }

    /// minOs kedi logosunu alan örneklemeli yumuşatma ile hedef boyuta taşır.
    fn draw_icon_bitmap(fb: &mut Framebuffer, x: usize, y: usize, size: usize, source: &[u32; 256]) {
        let mut resolved = [0u32; 256];
        for (output, input) in resolved.iter_mut().zip(source.iter()) {
            if *input != 0 { *output = theme::resolve(*input); }
        }
        for dy in 0..size {
            let sy0 = dy * 16;
            let sy1 = (dy + 1) * 16;
            let row_start = sy0 / size;
            let row_end = ((sy1 - 1) / size).min(15);
            for dx in 0..size {
                let sx0 = dx * 16;
                let sx1 = (dx + 1) * 16;
                let col_start = sx0 / size;
                let col_end = ((sx1 - 1) / size).min(15);
                let mut covered = 0u32;
                let mut red = 0u32;
                let mut green = 0u32;
                let mut blue = 0u32;
                for row in row_start..=row_end {
                    let oy = sy1.min((row + 1) * size).saturating_sub(sy0.max(row * size));
                    for col in col_start..=col_end {
                        let color = resolved[row * 16 + col];
                        if color == 0 { continue; }
                        let ox = sx1.min((col + 1) * size).saturating_sub(sx0.max(col * size));
                        let weight = (ox * oy) as u32;
                        covered += weight;
                        red += ((color >> 16) & 0xFF) * weight;
                        green += ((color >> 8) & 0xFF) * weight;
                        blue += (color & 0xFF) * weight;
                    }
                }
                if covered == 0 { continue; }
                let color = ((red / covered) << 16) | ((green / covered) << 8) | (blue / covered);
                let alpha = ((covered * 255 + 128) / 256) as u8;
                if alpha == 255 {
                    fb.put_pixel(x + dx, y + dy, color);
                } else {
                    fb.put_subpixel_text(x + dx, y + dy, color, [alpha; 3]);
                }
            }
        }
    }

    fn draw_antialiased_line(
        fb: &mut Framebuffer,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: u32,
        width: f32,
    ) {
        let radius = width / 2.0;
        let radius_sq = radius * radius;
        let feather = 0.5f32;
        let max_r = radius + feather;
        let max_r_sq = max_r * max_r;
        let min_x = (x0.min(x1) - max_r).max(0.0) as usize;
        let min_y = (y0.min(y1) - max_r).max(0.0) as usize;
        let max_x = (x0.max(x1) + max_r) as usize;
        let max_y = (y0.max(y1) + max_r) as usize;
        let vx = x1 - x0;
        let vy = y1 - y0;
        let length_sq = (vx * vx + vy * vy).max(0.0001);
        let inv_len_sq = 1.0 / length_sq;

        for py in min_y..=max_y {
            let sy = py as f32 + 0.5;
            let dy0 = sy - y0;
            for px in min_x..=max_x {
                let sx = px as f32 + 0.5;
                let dx0 = sx - x0;
                let t = ((dx0 * vx + dy0 * vy) * inv_len_sq).clamp(0.0, 1.0);
                let dx = sx - (x0 + t * vx);
                let dy = sy - (y0 + t * vy);
                let d_sq = dx * dx + dy * dy;
                if d_sq <= max_r_sq {
                    if d_sq <= radius_sq {
                        fb.put_pixel(px, py, color);
                    } else {
                        // Fast inverse square root to avoid libm dependency in no_std
                        let inv_d = {
                            let i = d_sq.to_bits();
                            let i = 0x5f3759df - (i >> 1);
                            let y = f32::from_bits(i);
                            let y = y * (1.5 - 0.5 * d_sq * y * y);
                            y * (1.5 - 0.5 * d_sq * y * y)
                        };
                        let d = d_sq * inv_d;
                        let alpha = (((max_r - d) / feather).clamp(0.0, 1.0) * 255.0) as u8;
                        if alpha > 0 {
                            fb.put_subpixel_text(px, py, color, [alpha; 3]);
                        }
                    }
                }
            }
        }
    }

    /// Aktif UI ölçeğinde bir ikonun kenar uzunluğu (px). 20px temel alınır ve
    /// `sp()` ile ölçeklenir; böylece ikon konumu ile boyutu tutarlıdır.
    #[inline(always)]
    fn icon_size() -> usize {
        sp(20)
    }

    pub fn draw_cat_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        Self::draw_cat_icon_scaled(fb, x, y)
    }

    pub fn draw_cat_icon_scaled(fb: &mut Framebuffer, x: usize, y: usize) {
        Self::draw_cat_icon_at_size(fb, x, y, Self::icon_size());
    }

    fn draw_cat_icon_at_size(fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let mut bitmap = [0u32; 256];
        const CAT_ICON_MAP: [u16; 16] = [
            0b0110000000000110,
            0b1111000000001111,
            0b1111111111111111,
            0b1111111111111111,
            0b1100111111110011,
            0b1100111111110011,
            0b1111111111111111,
            0b1001111001111001,
            0b1111110000111111,
            0b0111111111111110,
            0b0011111111111100,
            0b0011111111111100,
            0b0111001111001110,
            0b0111001111001110,
            0b0011111111111100,
            0b0000000000000000,
        ];

        for (row, &mask) in CAT_ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let pixel_color = if (row == 4 || row == 5) && (col == 2 || col == 3 || col == 12 || col == 13) {
                        Color::ACCENT_BLUE
                    } else if row == 8 && (col == 7 || col == 8) {
                        Color::ACCENT_PINK
                    } else if theme::current() == theme::ThemeId::Daylight {
                        0xE67E22
                    } else if row <= 1 {
                        Color::ACCENT_CAT_EARS
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = pixel_color;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, size, &bitmap);
    }

    /// Bilgisayarım / Dosya Yöneticisi Simgesi (16x16)
    pub fn draw_computer_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0111111111111100,
            0b1000000000000010,
            0b1011111111111010,
            0b1010000000001010,
            0b1010111011101010,
            0b1010000000001010,
            0b1011111111111010,
            0b1000000000000010,
            0b0111111111111100,
            0b0000011110000000,
            0b0000001100000000,
            0b0000111111000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 2 && row <= 6 && col >= 2 && col <= 13 {
                        Color::ACCENT_BLUE
                    } else if row >= 9 {
                        Color::TEXT_MUTED
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Notlar / Not Defteri Simgesi (16x16)
    pub fn draw_notepad_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0011111111000000,
            0b0010000001100000,
            0b0010000001010000,
            0b0010111111110000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0010111110010000,
            0b0010000000010000,
            0b0010111111010000,
            0b0010000000010000,
            0b0011111111110000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if (row == 5 || row == 7 || row == 9 || row == 11) && col >= 4 && col <= 10 {
                        Color::ACCENT_BLUE
                    } else if row <= 2 && col >= 9 {
                        Color::ACCENT_YELLOW
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Web Tarayıcısı / Ağ Simgesi (16x16)
    pub fn draw_browser_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011110000000,
            0b0001111111100000,
            0b0011001100110000,
            0b0110001100011000,
            0b0111111111111000,
            0b1100001100001100,
            0b1100001100001100,
            0b1111111111111100,
            0b1100001100001100,
            0b1100001100001100,
            0b0111111111111000,
            0b0110001100011000,
            0b0011001100110000,
            0b0001111111100000,
            0b0000011110000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row == 4 || row == 7 || row == 10 {
                        Color::ACCENT_GREEN
                    } else {
                        Color::ACCENT_BLUE
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Müzik Çalar / Çalma Listesi Simgesi (16x16)
    pub fn draw_music_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011111110000,
            0b0000011111110000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0000010000010000,
            0b0001110001110000,
            0b0011111001111100,
            0b0011111001111100,
            0b0001110001110000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
            0b0000000000000000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 8 {
                        Color::ACCENT_PINK
                    } else {
                        Color::ACCENT_YELLOW
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// Sistem Ayarları / Dişli Çark Simgesi (16x16)
    pub fn draw_gear_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        let s = Self::icon_size();
        let mut bitmap = [0u32; 256];
        const ICON_MAP: [u16; 16] = [
            0b0000011001100000,
            0b0000011001100000,
            0b0011001111001100,
            0b0011011111101100,
            0b0001110000111000,
            0b1111100000011111,
            0b1111000110001111,
            0b0011001111001100,
            0b0011001111001100,
            0b1111000110001111,
            0b1111100000011111,
            0b0001110000111000,
            0b0011011111101100,
            0b0011001111001100,
            0b0000011001100000,
            0b0000011001100000,
        ];
        for (row, &mask) in ICON_MAP.iter().enumerate() {
            for col in 0..16 {
                if (mask & (1 << (15 - col))) != 0 {
                    let c = if row >= 6 && row <= 9 && col >= 6 && col <= 9 {
                        Color::ACCENT_BLUE
                    } else {
                        Color::TEXT_MAIN
                    };
                    bitmap[row * 16 + col] = c;
                }
            }
        }
        Self::draw_icon_bitmap(fb, x, y, s, &bitmap);
    }

    /// İndekse göre ilgili simgeyi çizer (aktif UI ölçeğinde).
    pub fn draw_icon_by_index(fb: &mut Framebuffer, x: usize, y: usize, index: usize) {
        Self::draw_icon_by_index_scaled(fb, x, y, index)
    }

    /// `draw_icon_by_index` ile aynıdır; ikonlar içeride `icon_scale()` ile
    /// ölçeklendiği için ölçek uygulaması ikon fonksiyonlarının içindedir.
    pub fn draw_icon_by_index_scaled(fb: &mut Framebuffer, x: usize, y: usize, index: usize) {
        match index {
            0 => Self::draw_computer_icon(fb, x, y),
            1 => Self::draw_notepad_icon(fb, x, y),
            2 => Self::draw_browser_icon(fb, x, y),
            3 => Self::draw_music_icon(fb, x, y),
            4 => Self::draw_gear_icon(fb, x, y),
            _ => Self::draw_cat_icon(fb, x, y),
        }
    }

    // -----------------------------------------------------------------------
    // 1. Masaüstü Arka Planı
    // -----------------------------------------------------------------------
    pub fn render_desktop_bg(&self, fb: &mut Framebuffer) {
        // Hafif dikey gradyan masaüstüne derinlik katar.
        let top = Color::DARK_BG;
        let bottom = Self::shade(Color::DARK_BG, -12);
        fb.draw_vgradient(0, 0, fb.width, fb.height, top, bottom);

        // Masaüstü Sağ Üst minOs Kedi Logosu ve Başlığı (Watermark)
        // Metin genişliğine göre sağa hizalanır; sağ kenardan taşmaz.
        let title = if crate::fs::diskfs::is_installed() {
            "minOs Sarman Edition *"
        } else {
            "minOs (Live)"
        };
        let title_w = Self::text_width(title, 1);
        let icon_w = Self::icon_size();
        let gap = sp(8);
        let margin_right = sp(16);
        let block_w = icon_w + gap + title_w;
        let block_x = fb.width.saturating_sub(margin_right + block_w);
        let title_y = sp(35);
        Self::draw_cat_icon(fb, block_x, sp(30));
        Self::draw_string_shadow(
            fb,
            block_x + icon_w + gap,
            title_y,
            title,
            Color::ACCENT_PINK,
            Color::SHADOW,
            1,
        );

        // Masaüstü Hızlı İkonları (Sol Kenar)
        let desk_icons = if ui_english() { [
            ("Computer", 0), ("Notes", 1),
            ("SarmanNet", 2), ("Playlist", 3),
            ("Settings", 4), ("Install minOs", 0),
        ] } else { [
            ("Bilgisayarım", 0), ("Notlar", 1),
            ("SarmanNet", 2), ("Çalma Listesi", 3),
            ("Sistem Ayarları", 4), ("minOs'u Kur", 0),
        ] };

        let card_w = sp(34);
        // Kurulu sistemde "minOs'u Kur" masaüstü kısayolu gösterilmez.
        let desk_icon_count = Self::desktop_quick_count();
        for (i, (lbl, icon_idx)) in desk_icons.iter().enumerate().take(desk_icon_count) {
            // Tıklama testiyle aynı kanonik konum (kayma/çakışma olmaz).
            let (card_x, dy, _hit_w, card_h) = Self::desktop_quick_rect(i);
            fb.draw_rounded_rect(card_x, dy, card_w, card_h, 6, Self::shade(Color::WINDOW_BG, 8));
            fb.draw_rounded_rect_outline(card_x, dy, card_w, card_h, 6, Color::BORDER);
            let isz = Self::icon_size();
            let inner_x = card_x + (card_w.saturating_sub(isz)) / 2;
            let inner_y = dy + (card_h.saturating_sub(isz)) / 2;
            Self::draw_icon_by_index(fb, inner_x, inner_y, *icon_idx);
            // Etiket, ikon kartına dikey ortalı ve net boşlukla çizilir (temiz, tekdüze yazı).
            let label_x = card_x + card_w + sp(10);
            let label_y = dy + card_h.saturating_sub(Self::text_height()) / 2;
            Self::draw_string(fb, label_x, label_y, lbl, Color::TEXT_MAIN);
        }
    }

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
            _ => 0,
        }
    }

    fn window_title(win_idx: usize, english: bool) -> &'static str {
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
                _ => "Pencere",
            }
        }
    }

    fn window_tab_title(win_idx: usize, english: bool) -> &'static str {
        if english {
            match win_idx {
                0 => "Files", 1 => "Settings", 2 => "Notes",
                3 => "Web", 4 => "Image", 5 => "Music",
                6 => "Video", 7 => "Tasks", 8 => "Calc",
                9 => "Paint", 10 => "CMD", 11 => "Install", 12 => "3D", _ => "Win",
            }
        } else {
            match win_idx {
                0 => "Dosyalar", 1 => "Ayarlar", 2 => "Notlar",
                3 => "Web", 4 => "Resim", 5 => "Müzik",
                6 => "Video", 7 => "Görevler", 8 => "Hesap",
                9 => "Çizim", 10 => "Komut", 11 => "Kurulum", 12 => "3D", _ => "Pencere",
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
        let (cx, cy, cb, _) = win.close_rect();
        let hover_close = in_rect(mx, my, win.close_rect());
        let close_bg = if hover_close { Self::shade(Color::ACCENT_RED, 30) } else { Color::ACCENT_RED };
        fb.draw_rounded_rect(cx, cy, cb, cb, 4, close_bg);
        let close_mark_color = Self::text_on(close_bg);
        let mark_inset = cb as f32 * 0.29;
        let mark_left = cx as f32 + mark_inset;
        let mark_right = cx as f32 + cb as f32 - mark_inset;
        let mark_top = cy as f32 + mark_inset;
        let mark_bottom = cy as f32 + cb as f32 - mark_inset;
        let mark_width = (sp(16) as f32 / 10.0).max(1.5);
        Self::draw_antialiased_line(fb, mark_left, mark_top, mark_right, mark_bottom, close_mark_color, mark_width);
        Self::draw_antialiased_line(fb, mark_right, mark_top, mark_left, mark_bottom, close_mark_color, mark_width);

        let (mxr, myr, mb, _) = win.max_rect();
        let hover_max = in_rect(mx, my, win.max_rect());
        let max_bg = if hover_max { Self::shade(Color::ACCENT_GREEN, 30) } else { Color::ACCENT_GREEN };
        fb.draw_rounded_rect(mxr, myr, mb, mb, 4, max_bg);
        let max_mark_color = Self::text_on(max_bg);
        let inset = mb as f32 * 0.29;
        let left = mxr as f32 + inset;
        let right = mxr as f32 + mb as f32 - inset;
        let top = myr as f32 + inset;
        let bottom = myr as f32 + mb as f32 - inset;
        let stroke = (sp(14) as f32 / 10.0).max(1.4);
        if win.is_maximized {
            let offset = mb as f32 * 0.16;
            Self::draw_antialiased_line(fb, left + offset, top + offset, right, top + offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right, top + offset, right, bottom - offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right, bottom - offset, left + offset, bottom - offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, left + offset, bottom - offset, left + offset, top + offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, left, top, right - offset, top, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right - offset, top, right - offset, bottom - offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right - offset, bottom - offset, left, bottom - offset, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, left, bottom - offset, left, top, max_mark_color, stroke);
        } else {
            Self::draw_antialiased_line(fb, left, top, right, top, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right, top, right, bottom, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, right, bottom, left, bottom, max_mark_color, stroke);
            Self::draw_antialiased_line(fb, left, bottom, left, top, max_mark_color, stroke);
        }

        let (nx, ny, nb, _) = win.min_rect();
        let hover_min = in_rect(mx, my, win.min_rect());
        let min_bg = if hover_min { Self::shade(Color::ACCENT_YELLOW, 30) } else { Color::ACCENT_YELLOW };
        fb.draw_rounded_rect(nx, ny, nb, nb, 4, min_bg);
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

    // -----------------------------------------------------------------------
    // 3. Görev Çubuğu, Sistem Tepsisi (saat/ses/tema) ve Kaplamalar
    // -----------------------------------------------------------------------
    pub fn render_taskbar_and_overlays(
        &self,
        fb: &mut Framebuffer,
        mouse: &MouseState,
        kbd: &KeyboardState,
        now: &DateTime,
        volume: u8,
        is_muted: bool,
        english: bool,
        ethernet_available: bool,
    ) {
        let l = Self::tray_layout(fb.width, fb.height);
        let tb_y = l.tb_y;
        let mx = mouse.x as usize;
        let my = mouse.y as usize;

        // Görev Çubuğu Zemin (gradyan)
        fb.draw_vgradient(0, tb_y, fb.width, taskbar_h(), Color::TASKBAR_BG, Self::shade(Color::TASKBAR_BG, 8));
        fb.draw_rect(0, tb_y, fb.width, 1, Color::BORDER);

        // Başlat Menüsü Butonu
        let start_hover = in_rect(mx, my, l.start) || self.is_start_menu_open;
        let start_bg = if self.is_start_menu_open { Color::ACCENT_BLUE } else { Color::ACCENT_BLUE };
        Self::draw_button(fb, l.start, start_bg, "", Color::WHITE, start_hover);
        // Kedi ikonu + "minOs" metni buton içinde yatay olarak ortalanır;
        // metin buton kenarlarına yapışmaz veya taşmaz.
        {
            let label = "minOs";
            let text_w = Self::text_width(label, 1);
            let icon_w = Self::icon_size();
            let inner_gap = sp(8);
            let block_w = icon_w + inner_gap + text_w;
            let block_x = l.start.0 + l.start.2.saturating_sub(block_w) / 2;
            let icon_y = tb_y + taskbar_h().saturating_sub(icon_w) / 2;
            let text_h = Self::text_height();
            let text_y = tb_y + taskbar_h().saturating_sub(text_h) / 2;
            Self::draw_cat_icon(fb, block_x, icon_y);
            Self::draw_string_shadow(fb, block_x + icon_w + inner_gap, text_y, label, Self::text_on(start_bg), Color::SHADOW, 1);
        }

        // Uygulama Sekmeleri (Yuvarlatılmış 5px) — font ölçeğine göre büyür.
        let open_count = self.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open)).count();
        let (tab_start, tab_width, tab_step, shown_tabs, tab_slots) = Self::taskbar_tabs_layout(fb.width, fb.height, open_count);
        let mut tab_slot = 0usize;
        for opt_win in self.windows.iter() {
            if let Some(win) = opt_win {
                if win.is_open {
                    if tab_slot >= shown_tabs { break; }
                    let tab_x = tab_start + tab_slot * tab_step;
                    let (_, _, tw, th) = MinGuiCompositor::taskbar_tab_rect_at(tab_x, tb_y);
                    let tw = tw.min(tab_width);
                    let rect = (tab_x, tb_y + 5, tw, th);
                    let active = win.is_focused && !win.is_minimized;
                    let base = if active { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE };
                    let hovered = in_rect(mx, my, rect);
                    let bg = if hovered { Self::shade(base, 20) } else { base };
                    fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 5, bg);
                    fb.draw_rounded_rect_outline(rect.0, rect.1, rect.2, rect.3, 5, Color::BORDER);
                    if active {
                        fb.draw_rounded_rect(rect.0 + 2, rect.1 + 2, 3, rect.3 - 4, 1, Color::ACCENT_BLUE);
                    }
                    let tab_text_color = if win.is_minimized { Color::TEXT_MUTED } else { Color::TEXT_MAIN };
                    let loc_tab = Self::window_tab_title(win.app_id as usize, ui_english());
                    let tab_title = if !loc_tab.is_empty() { loc_tab } else { win.tab_title };
                    let text_h = Self::text_height();
                    let text_y = rect.1 + rect.3.saturating_sub(text_h) / 2;
                    if tw >= sp(58) {
                        Self::draw_icon_by_index(fb, tab_x + 5, text_y, Self::window_icon_index(win.app_id as usize));
                        Self::draw_string_clipped(fb, tab_x + sp(30), text_y, tab_title, tab_text_color,
                            (tab_x + sp(28), rect.1, rect.2.saturating_sub(sp(32)), rect.3));
                    } else {
                        Self::draw_icon_by_index(fb, tab_x + tw.saturating_sub(Self::icon_size()) / 2, text_y,
                            Self::window_icon_index(win.app_id as usize));
                    }
                    tab_slot += 1;
                }
            }
        }
        if tab_slots > shown_tabs {
            let badge_x = tab_start + shown_tabs * tab_step;
            let (_, _, _, tab_h) = MinGuiCompositor::taskbar_tab_rect_at(badge_x, tb_y);
            let badge_w = tab_width.min(l.kbd.0.saturating_sub(sp(4)).saturating_sub(badge_x));
            if badge_w > 0 {
                let badge = (badge_x, tb_y + 5, badge_w, tab_h);
                fb.draw_rounded_rect(badge.0, badge.1, badge.2, badge.3, 5, Color::ACCENT_BLUE);
                let mut hidden = TextBuf::new();
                hidden.push_byte(b'+');
                hidden.push_usize(open_count.saturating_sub(shown_tabs));
                let text_w = Self::text_width(hidden.as_str(), 1);
                let text_h = Self::text_height();
                Self::draw_string_clipped(fb, badge.0 + badge.2.saturating_sub(text_w) / 2,
                    badge.1 + badge.3.saturating_sub(text_h) / 2, hidden.as_str(), Self::text_on(Color::ACCENT_BLUE), badge);
            }
        }

        // Sistem Tepsisi: saat, ses, Ethernet, tema ve klavye düzeni.
        // 1. Klavye / Dil Düğmesi (l.kbd)
        let kbd_tag = kbd.layout_tag();
        let kbd_col = match kbd.layout {
            crate::drivers::keyboard::KeyboardLayout::TurkishQ => Color::ACCENT_RED,
            crate::drivers::keyboard::KeyboardLayout::EnglishUS => Color::ACCENT_BLUE,
        };
        let kbd_hover = in_rect(mx, my, l.kbd);
        // Klavye resmi: kısa etiket yerine sade bir klavye simgesi.
        let keyboard_icon = if kbd_hover { Color::TEXT_MAIN } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(l.kbd.0 + sp(6), l.kbd.1 + sp(6), sp(18), sp(14), 2, keyboard_icon);
        for row in 0..2 {
            for col in 0..5 {
                fb.draw_rect(l.kbd.0 + sp(9) + col * sp(3), l.kbd.1 + sp(9) + row * sp(3), sp(2), sp(1), Color::TASKBAR_BG);
            }
        }
        fb.draw_rect(l.kbd.0 + sp(11), l.kbd.1 + sp(17), sp(8), sp(1), Color::TASKBAR_BG);
        Self::draw_string(fb, l.kbd.0 + sp(30), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, kbd_tag, kbd_col);

        // 2. Tema Düğmesi (l.theme)
        let theme_hover = in_rect(mx, my, l.theme);
        fb.draw_rounded_rect(l.theme.0 + sp(6), l.theme.1 + sp(8), sp(10), sp(10), 3, Color::ACCENT_PINK);
        let t_name = theme::current().short_name();
        Self::draw_string(fb, l.theme.0 + sp(22), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, t_name, if theme_hover { Color::ACCENT_PINK } else { Color::TEXT_MAIN });

        // 3. Ses Düğmesi (l.volume)
        let vol_hover = in_rect(mx, my, l.volume) || self.is_volume_popup_open;
        let icon_color = if is_muted { Color::ACCENT_RED } else if vol_hover { Color::TEXT_MAIN } else { Color::ACCENT_GREEN };
        // Hoparlör gövdesi ve ses dalgaları.
        fb.draw_rect(l.volume.0 + sp(8), l.volume.1 + sp(11), sp(4), sp(6), icon_color);
        fb.draw_rect(l.volume.0 + sp(12), l.volume.1 + sp(8), sp(3), sp(12), icon_color);
        if !is_muted && volume > 0 {
            fb.draw_rect(l.volume.0 + sp(17), l.volume.1 + sp(10), sp(1), sp(8), icon_color);
            if volume > 35 { fb.draw_rect(l.volume.0 + sp(20), l.volume.1 + sp(8), sp(1), sp(12), icon_color); }
            if volume > 70 { fb.draw_rect(l.volume.0 + sp(23), l.volume.1 + sp(6), sp(1), sp(16), icon_color); }
        } else {
            fb.draw_rect(l.volume.0 + sp(17), l.volume.1 + sp(9), sp(2), sp(2), icon_color);
            fb.draw_rect(l.volume.0 + sp(20), l.volume.1 + sp(16), sp(2), sp(2), icon_color);
        }
        // Ethernet simgesi yalnızca e1000 aygıtı başarıyla başlatıldıysa görünür.
        if ethernet_available {
            let net_hover = in_rect(mx, my, l.network);
            let linked = crate::drivers::e1000::link_up();
            let c = if linked { if net_hover { Color::TEXT_MAIN } else { Color::ACCENT_GREEN } } else { Color::TEXT_MUTED };
            // Ethernet jakı: gövde, pinler, kablo ve bağlantı durumu ışığı.
            fb.draw_rounded_rect_outline(l.network.0 + sp(3), l.network.1 + sp(6), sp(14), sp(14), 2, c);
            fb.draw_rect(l.network.0 + sp(6), l.network.1 + sp(12), sp(8), sp(6), c);
            fb.draw_rect(l.network.0 + sp(7), l.network.1 + sp(9), sp(2), sp(4), c);
            fb.draw_rect(l.network.0 + sp(11), l.network.1 + sp(9), sp(2), sp(4), c);
            fb.draw_rect(l.network.0 + sp(9), l.network.1 + sp(19), sp(2), sp(2), c);
            fb.draw_rect(l.network.0 + sp(20), l.network.1 + sp(7), sp(3), sp(3), c);
            Self::draw_string(fb, l.network.0 + sp(28), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, "eth0", c);
        }

        // 4. Saat Düğmesi (l.clock)
        let clock_hover = in_rect(mx, my, l.clock) || self.is_calendar_open;
        let mut clock = TextBuf::new();
        clock.push_u2(now.hour as u32);
        clock.push_byte(b':');
        clock.push_u2(now.minute as u32);
        clock.push_byte(b':');
        clock.push_u2(now.second as u32);
        Self::draw_string(fb, l.clock.0 + sp(8), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, clock.as_str(), if clock_hover { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });

        // Akıllı tepsi ağı simgesi

        // Başlat Menüsü
        if self.is_start_menu_open {
            self.render_start_menu(fb, tb_y, english, mx, my);
        }
        // Bağlam Menüsü
        if self.is_context_menu_open {
            self.render_context_menu(fb);
        }
        // Alt+Tab
        if self.is_alt_tab_open {
            self.render_alt_tab_overlay(fb);
        }
        // Takvim
        if self.is_calendar_open {
            self.render_calendar(fb, now, &l);
        }
        // Ses paneli
        if self.is_volume_popup_open {
            self.render_volume_popup(fb, &l, volume, is_muted);
        }

        // Donanım Fare İmleci
        self.draw_mouse_cursor(fb, mx, my);
    }

    pub fn render_alt_tab_overlay(&self, fb: &mut Framebuffer) {
        let box_w = 470.min(fb.width.saturating_sub(24)).max(1);
        let open_count = if self.is_alt_tab_open && self.alt_tab_count > 0 { self.alt_tab_count }
            else { self.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open && !win.is_minimized)).count() };
        let columns = (box_w.saturating_sub(24) / 58).max(1);
        let rows = open_count.saturating_add(columns - 1) / columns;
        let grid_h = rows * 66;
        let box_h = (40 + grid_h + 58).min(fb.height.saturating_sub(24)).max(1);
        let box_x = fb.width.saturating_sub(box_w) / 2;
        let box_y = fb.height.saturating_sub(box_h) / 2;

        fb.draw_shadow(box_x, box_y, box_w, box_h, Color::SHADOW);
        fb.draw_rect(box_x, box_y, box_w, box_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(box_x, box_y, box_w, box_h, Color::BORDER);
        fb.draw_rect(box_x, box_y, box_w, 26, Color::TITLE_BAR);
        let english = ui_english();
        Self::draw_string_shadow(fb, box_x + 12, box_y + 9,
            ui_text("Uygulama Geçişi (Alt + Tab)", "Window Switcher (Alt + Tab)"), Color::TEXT_MAIN, Color::SHADOW, 1);

        let mut active_app_title = ui_text("Pencere Seçilmedi", "No Window Selected");

        let (draw_order, draw_count) = self.draw_order();
        let (switch_order, switch_count) = if self.is_alt_tab_open && self.alt_tab_count > 0 {
            (self.alt_tab_order, self.alt_tab_count)
        } else {
            (draw_order, draw_count)
        };
        for app_count in 0..switch_count {
            if let Some(win) = self.windows[switch_order[app_count]] {
                let col = app_count % columns;
                let row = app_count / columns;
                let grid_w = columns * 58;
                let card_x = box_x + box_w.saturating_sub(grid_w) / 2 + col * 58;
                let card_y = box_y + 36 + row * 66;
                let is_sel = win.is_focused;
                let card_bg = if is_sel { Color::ACCENT_BLUE } else { Color::WINDOW_BG };

                if is_sel {
                    active_app_title = if english { Self::window_title(win.app_id as usize, true) } else { win.title };
                }

                fb.draw_rounded_rect(card_x, card_y, 50, 58, 6, card_bg);
                if is_sel {
                    fb.draw_rounded_rect_outline(card_x.saturating_sub(1), card_y.saturating_sub(1), 52, 60, 6, Color::ACCENT_PINK);
                }
                let icon_idx = Self::window_icon_index(win.app_id as usize);
                Self::draw_icon_by_index(fb, card_x + 15, card_y + 9, icon_idx);
                let tab_col = if is_sel { Self::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN };
                let tab_title = if english { Self::window_tab_title(win.app_id as usize, true) } else { win.tab_title };
                Self::draw_string_clipped(fb, card_x + 2, card_y + 38, tab_title, tab_col, (card_x + 2, card_y + 36, 46, 20));
            }
        }

        let footer_y = box_y + box_h.saturating_sub(52);
        fb.draw_rect(box_x + 12, footer_y, box_w.saturating_sub(24), 44, Color::TITLE_BAR_INACTIVE);
        let active_label = ui_text("Aktif Pencere:", "Active Window:");
        let active_label_w = Self::text_width(active_label, 1) + 8;
        Self::draw_string_clipped(fb, box_x + 20, footer_y + 5, active_label, Color::TEXT_MUTED,
            (box_x + 18, footer_y + 2, box_w.saturating_sub(36), 20));
        Self::draw_string_clipped(fb, box_x + 20 + active_label_w, footer_y + 5, active_app_title, Color::ACCENT_GREEN,
            (box_x + 20 + active_label_w, footer_y + 2, box_w.saturating_sub(active_label_w + 38), 20));
        Self::draw_string_clipped(fb, box_x + 20, footer_y + 24,
            ui_text("Alt tuşunu bıraktığınızda pencere öne gelir.", "Release Alt to bring the window forward."), Color::TEXT_MUTED,
            (box_x + 18, footer_y + 21, box_w.saturating_sub(36), 20));
    }

    /// Aktif bağlam menüsünün öğe listesi.
    pub fn context_items(&self) -> &'static [(&'static str, u32)] {
        match (self.context_kind, ui_english()) {
            (ContextKind::Desktop, false) => &CTX_DESKTOP,
            (ContextKind::FileManager, false) => &CTX_FILE,
            (ContextKind::Notepad, false) => &CTX_NOTEPAD,
            (ContextKind::TaskManager, false) => &CTX_TASKMANAGER,
            (ContextKind::Window, false) => &CTX_WINDOW,
            (ContextKind::Desktop, true) => &CTX_DESKTOP_EN,
            (ContextKind::FileManager, true) => &CTX_FILE_EN,
            (ContextKind::Notepad, true) => &CTX_NOTEPAD_EN,
            (ContextKind::TaskManager, true) => &CTX_TASKMANAGER_EN,
            (ContextKind::Window, true) => &CTX_WINDOW_EN,
        }
    }

    /// Bağlam menüsünün ekran üzerindeki dikdörtgeni (çizim ve tıklama ortak).
    /// Öğe yüksekliği aktif font ölçeğine göre otomatik büyür.
    pub fn context_menu_geom(&self, fb_w: usize, fb_h: usize) -> (usize, usize, usize, usize) {
        let items = self.context_items();
        let widest_item = items.iter().map(|(label, _)| Self::text_width(label, 1)).max().unwrap_or(0);
        let icon_pad = match self.context_kind {
            ContextKind::Desktop | ContextKind::Notepad => sp(24),
            _ => 0,
        };
        let shortcut_pad = match self.context_kind {
            ContextKind::Notepad | ContextKind::TaskManager => sp(64),
            _ => 0,
        };
        let w = sp(CONTEXT_MENU_W + 16).max(widest_item + sp(24) + icon_pad + shortcut_pad).min(fb_w);
        // Başlık + öğeler (ölçekli) + alt boşluk
        let h = context_header_h() + items.len() * context_item_h();
        let x = self.context_menu_x.min(fb_w.saturating_sub(w));
        let y = self.context_menu_y.min(fb_h.saturating_sub(h));
        (x, y, w, h)
    }

    pub fn render_context_menu(&self, fb: &mut Framebuffer) {
        let (menu_x, menu_y, menu_w, menu_h) = self.context_menu_geom(fb.width, fb.height);

        fb.draw_shadow(menu_x, menu_y, menu_w, menu_h, Color::SHADOW);
        fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
        fb.draw_rect(menu_x, menu_y, menu_w, 2, Color::ACCENT_BLUE);

        // Menü başlığı (tür)
        let header = match self.context_kind {
            ContextKind::Desktop => ui_text("Masaüstü", "Desktop"),
            ContextKind::FileManager => ui_text("Dosya İşlemleri", "File Actions"),
            ContextKind::Notepad => ui_text("Düzenleme", "Edit"),
            ContextKind::TaskManager => ui_text("Görev", "Task"),
            ContextKind::Window => ui_text("Pencere", "Window"),
        };
        let hdr_h = context_header_h();
        let item_h = context_item_h();
        Self::draw_string_clipped(fb, menu_x + 8, menu_y + sp(6), header, Color::TEXT_MUTED,
            (menu_x + 6, menu_y + 2, menu_w.saturating_sub(12), hdr_h.saturating_sub(3)));
        fb.draw_rect(menu_x + 4, menu_y + hdr_h - sp(4), menu_w - 8, 1, Color::BORDER);

        for (i, (label, color)) in self.context_items().iter().enumerate() {
            let item_y = menu_y + hdr_h + i * item_h;
            fb.draw_rect(menu_x + 4, item_y, menu_w - 8, item_h.saturating_sub(sp(4)), Color::WINDOW_BG);

            let mut text_x = menu_x + sp(10);
            if self.context_kind == ContextKind::Desktop {
                // Masaüstü sağ tık menüsü simgeleri (12x12 piksel)
                let icon_x = menu_x + sp(10);
                let icon_y = item_y + (item_h.saturating_sub(sp(4)).saturating_sub(12)) / 2;
                Self::draw_desktop_context_icon(fb, icon_x, icon_y, i, *color);
                text_x += sp(20);
            } else if self.context_kind == ContextKind::Notepad {
                // Not Defteri / Textarea sağ tık menüsü simgeleri (12x12 piksel)
                let icon_x = menu_x + sp(10);
                let icon_y = item_y + (item_h.saturating_sub(sp(4)).saturating_sub(12)) / 2;
                Self::draw_notepad_context_icon(fb, icon_x, icon_y, i, *color);
                text_x += sp(20);
            }

            Self::draw_string_clipped(fb, text_x, item_y + sp(5), label, *color,
                (text_x, item_y + 2, menu_w.saturating_sub(text_x - menu_x + sp(60)), item_h.saturating_sub(4)));

            // Not Defteri ve Görev Yöneticisi için sağa hizalı kısayol etiketi (örn: Ctrl+X, Del)
            if self.context_kind == ContextKind::Notepad || self.context_kind == ContextKind::TaskManager {
                let shortcut = match (self.context_kind, i) {
                    (ContextKind::TaskManager, 0) => "Del",
                    (_, 0) => "Ctrl+X",
                    (_, 1) => "Ctrl+C",
                    (_, 2) => "Ctrl+V",
                    (_, 3) => "Del",
                    (_, 4) => "Ctrl+A",
                    _ => "",
                };
                if !shortcut.is_empty() {
                    let sc_w = Self::text_width(shortcut, 1);
                    let sc_x = menu_x + menu_w.saturating_sub(sc_w + sp(12));
                    Self::draw_string_clipped(fb, sc_x, item_y + sp(5), shortcut, Color::TEXT_MUTED,
                        (sc_x, item_y + 2, sc_w + sp(4), item_h.saturating_sub(4)));
                }
            }
        }
    }

    /// Not Defteri / Textarea sağ tık bağlam menüsü için 12x12 piksel simgeler çizer.
    fn draw_notepad_context_icon(fb: &mut Framebuffer, x: usize, y: usize, index: usize, color: u32) {
        match index {
            0 => {
                // Kes (Makas simgesi)
                let c = color;
                // Sol halka & sağ halka
                fb.draw_rect_outline(x + 1, y + 8, 3, 3, c);
                fb.draw_rect_outline(x + 7, y + 8, 3, 3, c);
                // Makas bıçakları
                fb.draw_rect(x + 3, y + 6, 2, 2, c);
                fb.draw_rect(x + 6, y + 6, 2, 2, c);
                fb.draw_rect(x + 4, y + 4, 3, 2, c);
                fb.draw_rect(x + 2, y + 1, 2, 3, c);
                fb.draw_rect(x + 7, y + 1, 2, 3, c);
            }
            1 => {
                // Kopyala (Üst üste iki sayfa simgesi)
                let c = color;
                // Arka sayfa
                fb.draw_rect_outline(x + 3, y, 7, 8, Color::TEXT_MUTED);
                // Ön sayfa
                fb.draw_rect(x, y + 3, 8, 9, Color::WINDOW_BG);
                fb.draw_rect_outline(x, y + 3, 8, 9, c);
                fb.draw_rect(x + 2, y + 6, 4, 1, c);
                fb.draw_rect(x + 2, y + 8, 4, 1, c);
            }
            2 => {
                // Yapıştır (Pano ve belge simgesi)
                let c = color;
                // Pano tablası
                fb.draw_rect(x + 1, y + 2, 10, 10, Color::ACCENT_YELLOW);
                // Pano klipsi
                fb.draw_rect(x + 4, y, 4, 3, Color::TEXT_MAIN);
                // Üstündeki kağıt
                fb.draw_rect(x + 3, y + 4, 6, 7, Color::WHITE);
                fb.draw_rect(x + 4, y + 6, 4, 1, c);
                fb.draw_rect(x + 4, y + 8, 3, 1, c);
            }
            3 => {
                // Sil (Çöp kutusu / silme çarpısı)
                let c = color;
                fb.draw_rect(x + 2, y + 2, 8, 2, c);
                fb.draw_rect(x + 4, y, 4, 2, c);
                fb.draw_rect(x + 3, y + 4, 6, 7, c);
                fb.draw_rect(x + 4, y + 5, 1, 5, Color::WINDOW_BG);
                fb.draw_rect(x + 6, y + 5, 1, 5, Color::WINDOW_BG);
            }
            4 => {
                // Tümünü Seç (Kesikli seçim çerçevesi ve dolu alan)
                let c = color;
                fb.draw_rect(x, y + 1, 11, 10, Color::DARK_BG);
                fb.draw_rect(x + 2, y + 3, 7, 6, c);
                // Köşe tutamaçları
                fb.draw_rect(x, y + 1, 2, 2, Color::WHITE);
                fb.draw_rect(x + 9, y + 1, 2, 2, Color::WHITE);
                fb.draw_rect(x, y + 9, 2, 2, Color::WHITE);
                fb.draw_rect(x + 9, y + 9, 2, 2, Color::WHITE);
            }
            _ => {
                // Tümünü Temizle (Boş sayfa veya süpürge)
                let c = color;
                fb.draw_rect_outline(x + 1, y + 1, 9, 10, c);
                fb.draw_rect(x + 3, y + 3, 5, 1, Color::ACCENT_RED);
                fb.draw_rect(x + 3, y + 5, 5, 1, Color::ACCENT_RED);
                fb.draw_rect(x + 3, y + 7, 5, 1, Color::ACCENT_RED);
            }
        }
    }

    /// Masaüstü sağ tık bağlam menüsü için 12x12 piksel simgeler çizer.
    fn draw_desktop_context_icon(fb: &mut Framebuffer, x: usize, y: usize, index: usize, color: u32) {
        match index {
            0 => {
                // Dosya Yöneticisi: Klasör simgesi
                fb.draw_rect(x + 1, y + 2, 4, 2, Color::ACCENT_YELLOW);
                fb.draw_rounded_rect(x, y + 3, 12, 8, 2, Color::ACCENT_BLUE);
                fb.draw_rect(x + 2, y + 5, 8, 4, Color::WHITE);
            }
            1 => {
                // Yeni Not / Not Defteri: Sayfa ve kalem
                fb.draw_rounded_rect(x + 1, y + 1, 9, 10, 2, Color::TEXT_MAIN);
                fb.draw_rect(x + 3, y + 3, 5, 1, Color::ACCENT_BLUE);
                fb.draw_rect(x + 3, y + 5, 5, 1, Color::ACCENT_BLUE);
                fb.draw_rect(x + 3, y + 7, 3, 1, Color::ACCENT_BLUE);
                // Kalem ucu
                fb.draw_rect(x + 8, y + 8, 3, 3, Color::ACCENT_YELLOW);
            }
            2 => {
                // Sistem Ayarları: Dişli simgesi
                fb.draw_rect(x + 4, y, 4, 12, Color::ACCENT_PINK);
                fb.draw_rect(x, y + 4, 12, 4, Color::ACCENT_PINK);
                fb.draw_rect(x + 2, y + 2, 8, 8, Color::ACCENT_PINK);
                fb.draw_rect(x + 4, y + 4, 4, 4, Color::WINDOW_BG);
            }
            3 => {
                // Görev Yöneticisi: Aktivite / Nabız grafiği
                fb.draw_rect_outline(x, y + 1, 12, 10, Color::BORDER);
                let pulse_col = Color::ACCENT_GREEN;
                fb.draw_rect(x + 1, y + 6, 2, 1, pulse_col);
                fb.draw_rect(x + 3, y + 4, 2, 4, pulse_col);
                fb.draw_rect(x + 5, y + 2, 2, 7, pulse_col);
                fb.draw_rect(x + 7, y + 7, 2, 3, pulse_col);
                fb.draw_rect(x + 9, y + 5, 2, 2, pulse_col);
            }
            4 => {
                // Tema Değiştir: Palet / Boya fırçası
                fb.draw_rounded_rect(x, y + 1, 11, 10, 3, Color::ACCENT_GREEN);
                fb.draw_rect(x + 2, y + 3, 2, 2, Color::ACCENT_RED);
                fb.draw_rect(x + 6, y + 3, 2, 2, Color::ACCENT_BLUE);
                fb.draw_rect(x + 3, y + 7, 2, 2, Color::ACCENT_YELLOW);
                fb.draw_rect(x + 7, y + 7, 2, 2, Color::WINDOW_BG);
            }
            _ => {
                // Masaüstünü Yenile: Dairesel yenileme okları (Refresh)
                let c = color;
                fb.draw_rect(x + 3, y + 1, 6, 1, c);
                fb.draw_rect(x + 1, y + 3, 1, 4, c);
                fb.draw_rect(x + 3, y + 9, 6, 1, c);
                fb.draw_rect(x + 10, y + 4, 1, 4, c);
                // Ok uçları
                fb.draw_rect(x + 8, y, 2, 3, c);
                fb.draw_rect(x + 2, y + 8, 2, 3, c);
            }
        }
    }

    /// Bağlam menüsünde tıklanan öğe indeksini döndürür (menü dışındaysa None).
    pub fn context_hit(&self, fb_w: usize, fb_h: usize, mx: usize, my: usize) -> Option<usize> {
        let (menu_x, menu_y, menu_w, menu_h) = self.context_menu_geom(fb_w, fb_h);
        if !in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) {
            return None;
        }
        let hdr_h = context_header_h();
        if my < menu_y + hdr_h {
            // Başlık satırı — öğe yok
            return None;
        }
        let items = self.context_items();
        let rel = my - (menu_y + hdr_h);
        let idx = rel / context_item_h();
        if idx < items.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Başlat menüsünün ekran üzerindeki dikdörtgeni (x, y, w, h).
    /// Genişlik ve yükseklik aktif font ölçeğine göre otomatik büyür.
    pub fn start_menu_rect(tb_y: usize) -> (usize, usize, usize, usize) {
        let menu_w = sp(190);
        let menu_h = sp(MENU_H + 34);
        let menu_x = 6;
        let menu_y = tb_y.saturating_sub(menu_h + 4);
        (menu_x, menu_y, menu_w, menu_h)
    }

    /// Başlat menüsündeki arama kutusunun dikdörtgeni (x, y, w, h).
    /// Profil şeridinin hemen altına yerleşir; çizim ve tıklama testi bunu kullanır.
    pub fn start_search_rect(menu_x: usize, menu_y: usize, menu_w: usize) -> (usize, usize, usize, usize) {
        let strip_h = sp(34) + Self::icon_size();
        let search_h = sp(22);
        // Arama alanını profil şeridinin altına ve kategori listesinin üstüne
        // koy; iki alan ölçek değişse de üst üste binmesin.
        let search_y = menu_y + strip_h + sp(4);
        (menu_x + sp(8), search_y, menu_w.saturating_sub(sp(16)), search_h)
    }

    pub fn render_start_menu(&self, fb: &mut Framebuffer, tb_y: usize, english: bool, mx: usize, my: usize) {
        let (menu_x, menu_y, menu_w, menu_h) = Self::start_menu_rect(tb_y);
        fb.draw_shadow(menu_x, menu_y, menu_w, menu_h, Color::SHADOW);
        fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);

        // Profil Şeridi (gradyan)
        let strip_h = sp(34) + Self::icon_size();
        fb.draw_vgradient(menu_x + 1, menu_y + 1, menu_w - 2, strip_h, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
        let icon_sz = Self::icon_size();
        Self::draw_cat_icon(fb, menu_x + sp(8), menu_y + sp(8));
        let title_x = menu_x + sp(8) + icon_sz + sp(6);
        let text_h2 = Self::text_height();
        let title_y = menu_y + (strip_h.saturating_sub(text_h2)) / 2;
        Self::draw_string_shadow(fb, title_x, title_y, "minOs", Color::TEXT_MAIN, Color::SHADOW, 1);
        let settings_hover = in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22));
        if settings_hover { fb.draw_rounded_rect(menu_x + menu_w - 34, menu_y + 6, 26, 22, 4, Color::ACCENT_BLUE); }
        // Ayarlar menü öğesinde kullanılan mevcut uygulama simgesini yeniden kullan.
        Self::draw_icon_by_index(fb, menu_x + menu_w - 28, menu_y + 9, Self::window_icon_index(1));
        let search_rect = Self::start_search_rect(menu_x, menu_y, menu_w);
        fb.draw_rounded_rect(search_rect.0, search_rect.1, search_rect.2, search_rect.3, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(search_rect.0, search_rect.1, search_rect.2, search_rect.3, 4, Color::BORDER);
        let search_text = if self.start_search_len == 0 { ui_text("Uygulama ara...", "Search apps...") }
            else { core::str::from_utf8(&self.start_search[..self.start_search_len]).unwrap_or("") };
        Self::draw_string_clipped(fb, search_rect.0 + 6, search_rect.1 + 4, search_text, Color::TEXT_MAIN,
            (search_rect.0 + 4, search_rect.1 + 2, search_rect.2.saturating_sub(8), search_rect.3.saturating_sub(4)));

        // Ana Menü Alt Başlıkları (Kategoriler)
        let category_titles = if english {
            ["System", "Productivity", "Media", "Power"]
        } else {
            ["Sistem", "Üretkenlik", "Medya", "Güç"]
        };

        let active_g = self.active_start_group.min(3);

        for (g, title) in category_titles.iter().enumerate() {
            let r = Self::start_menu_category_rect(menu_y, g);
            let is_hovered = in_rect(mx, my, r);
            let is_active = g == active_g;

            let bg_col = if is_active {
                Color::ACCENT_BLUE
            } else if is_hovered {
                Color::TITLE_BAR
            } else {
                Color::WINDOW_BG
            };

            let fg_col = if is_active { Self::text_on(bg_col) } else { Color::TEXT_MAIN };

            fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 5, bg_col);
            if is_active {
                fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 5, Color::WHITE);
            }

            Self::draw_string(fb, r.0 + 8, r.1 + 10, title, fg_col);
            let arrow_x = (r.0 + r.2 - 13) as f32;
            let arrow_y = (r.1 + r.3 / 2) as f32;
            let arrow_color = Self::text_on(bg_col);
            Self::draw_antialiased_line(fb, arrow_x - 2.5, arrow_y - 4.0, arrow_x + 1.5, arrow_y, arrow_color, 1.7);
            Self::draw_antialiased_line(fb, arrow_x + 1.5, arrow_y, arrow_x - 2.5, arrow_y + 4.0, arrow_color, 1.7);
        }

        // Yana Açılır Alt Menü
        if self.start_search_len == 0 {
            self.render_start_submenu(fb, menu_y, active_g, tb_y, english, mx, my);
        } else {
            let mut results = [0; 13];
            let count = self.start_search_matches(english, &mut results);
            let (sub_x, sub_y, sub_w, sub_h) = Self::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));
            fb.draw_shadow(sub_x, sub_y, sub_w, sub_h, Color::SHADOW);
            fb.draw_rounded_rect(sub_x, sub_y, sub_w, sub_h, 8, Color::TASKBAR_BG);
            fb.draw_rounded_rect_outline(sub_x, sub_y, sub_w, sub_h, 8, Color::BORDER);
            fb.draw_vgradient(sub_x + 1, sub_y + 1, sub_w - 2, 24, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
            Self::draw_string_shadow(fb, sub_x + 8, sub_y + 7, ui_text("Arama Sonuçları", "Search Results"), Color::TEXT_MAIN, Color::SHADOW, 1);

            let names = ["Dosya Yöneticisi", "Sistem Ayarları", "Not Defteri", "Web Tarayıcısı", "Resim Gösterici", "Müzik Çalar", "Video Oynatıcı", "Görev Yöneticisi", "Hesap Makinesi", "Çizim Tahtası", "Komut İstemi", "Tek Tıkla Kur", "3D Gösterici"];
            let en_names = ["File Manager", "Settings", "Notepad", "Web Browser", "Image Viewer", "Music Player", "Video Player", "Task Manager", "Calculator", "Paint", "Terminal", "One-Click Install", "3D Viewer"];
            if count == 0 {
                Self::draw_string(fb, sub_x + 12, sub_y + 38, ui_text("Sonuç bulunamadı", "No apps found"), Color::TEXT_MUTED);
            }
            for row in 0..count {
                let r = (sub_x + 6, sub_y + sp(30) + row * sp(24), sub_w - 12, sp(22));
                let is_selected = row == self.selected_start_item;
                let is_hovered = in_rect(mx, my, r);
                if is_selected { fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, Color::ACCENT_BLUE); }
                else if is_hovered { fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, Color::TITLE_BAR); }
                let app_id = results[row];
                let app_name = if english { en_names[app_id] } else { names[app_id] };
                Self::draw_string_clipped(fb, r.0 + 8, r.1 + 5, app_name,
                    if is_selected { Self::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN },
                    (r.0 + 4, r.1 + 2, r.2 - 8, r.3 - 4));
            }
        }
    }

    pub fn render_start_submenu(&self, fb: &mut Framebuffer, menu_y: usize, group_idx: usize, tb_y: usize, english: bool, mx: usize, my: usize) {
        let (sub_x, sub_y, sub_w, sub_h) = Self::start_menu_submenu_rect(menu_y, group_idx, tb_y);

        fb.draw_shadow(sub_x, sub_y, sub_w, sub_h, Color::SHADOW);
        fb.draw_rounded_rect(sub_x, sub_y, sub_w, sub_h, 8, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(sub_x, sub_y, sub_w, sub_h, 8, Color::BORDER);

        let sub_header_title = match group_idx {
            0 => crate::gui::ui_text("Sistem Uygulamaları", "System Apps"),
            1 => crate::gui::ui_text("Üretkenlik Araçları", "Productivity Tools"),
            2 => crate::gui::ui_text("Medya ve Eğlence", "Media & Entertainment"),
            _ => crate::gui::ui_text("Güç Seçenekleri", "Power Options"),
        };
        fb.draw_vgradient(sub_x + 1, sub_y + 1, sub_w - 2, 24, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
        Self::draw_string_shadow(fb, sub_x + 8, sub_y + 7, sub_header_title, Color::TEXT_MAIN, Color::SHADOW, 1);

        let menu_items: [(&str, u32, usize); 15] = if english { [
            ("File Manager", Color::ACCENT_YELLOW, 0),
            ("Settings", Color::ACCENT_BLUE, 1usize),
            ("Task Manager", Color::ACCENT_BLUE, 7),
            ("Terminal", Color::ACCENT_GREEN, 10),
            ("One-Click Install", Color::ACCENT_GREEN, 11),
            ("Notepad", Color::TEXT_MAIN, 2),
            ("Calculator", Color::ACCENT_GREEN, 8),
            ("Paint", Color::ACCENT_PINK, 9),
            ("Web Browser", Color::ACCENT_GREEN, 3),
            ("Image Viewer", Color::ACCENT_PINK, 4),
            ("Music Player", Color::ACCENT_YELLOW, 5),
            ("Video Player", Color::ACCENT_RED, 6),
            ("3D Viewer", Color::ACCENT_BLUE, 12),
            ("Shut Down", Color::ACCENT_RED, 255),
            ("Restart", Color::ACCENT_YELLOW, 254),
        ] } else { [
            ("Dosya Yöneticisi", Color::ACCENT_YELLOW, 0),
            ("Sistem Ayarları", Color::ACCENT_BLUE, 1usize),
            ("Görev Yöneticisi", Color::ACCENT_BLUE, 7),
            ("Komut İstemi", Color::ACCENT_GREEN, 10),
            ("Tek Tıkla Kur", Color::ACCENT_GREEN, 11),
            ("Not Defteri", Color::TEXT_MAIN, 2),
            ("Hesap Makinesi", Color::ACCENT_GREEN, 8),
            ("Çizim Tahtası", Color::ACCENT_PINK, 9),
            ("Web Tarayıcısı", Color::ACCENT_GREEN, 3),
            ("Resim Gösterici", Color::ACCENT_PINK, 4),
            ("Müzik Çalar", Color::ACCENT_YELLOW, 5),
            ("Video Oynatıcı", Color::ACCENT_RED, 6),
            ("3D Gösterici", Color::ACCENT_BLUE, 12),
            ("Sistemi Kapat", Color::ACCENT_RED, 255),
            ("Yeniden Başlat", Color::ACCENT_YELLOW, 254),
        ] };

        let (_, first, count) = MENU_GROUPS[group_idx];

        for k in 0..count {
            let i = first + k;
            let (name, _col, app_or_code) = menu_items[i];
            let r = Self::start_menu_subitem_rect(menu_y, group_idx, k, tb_y);

            let is_hovered = in_rect(mx, my, r) || (self.selected_start_item == k);
            let bg_col = if (i == 13 && self.confirm_shutdown) || (i == 14 && self.confirm_restart) {
                Color::ACCENT_RED
            } else if is_hovered {
                Color::ACCENT_BLUE
            } else if i >= 13 {
                Color::TITLE_BAR
            } else {
                Color::WINDOW_BG
            };

            fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, bg_col);
            if is_hovered {
                fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 4, Color::WHITE);
            }

            // Çizim: Simgeler (İkonlar) — öğe içinde dikey ortalanır.
            let icon_y = r.1 + r.3.saturating_sub(Self::icon_size()) / 2;
            if app_or_code < 13 {
                let icon_idx = Self::window_icon_index(app_or_code);
                Self::draw_icon_by_index_scaled(fb, r.0 + sp(6), icon_y, icon_idx);
            } else {
                // Güç simgeleri (kedi veya varsayılan simge)
                Self::draw_cat_icon_scaled(fb, r.0 + sp(6), icon_y);
            }

            let label = if i == 13 && self.confirm_shutdown {
                crate::gui::ui_text("KAPATMAYI ONAYLA (tekrar tıkla)", "CONFIRM SHUTDOWN (click again)")
            } else if i == 14 && self.confirm_restart {
                crate::gui::ui_text("YENİDEN BAŞLAT? TEKRAR TIKLA", "CONFIRM RESTART (click again)")
            } else {
                name
            };

            let text_col = if is_hovered { Color::WHITE } else { Color::TEXT_MAIN };
            // Metin konumu ikon boyutundan türetilir: sol boşluk + ikon + boşluk.
            // Böylece ikon büyüse de metin ikonun üzerine binmez (taşma/kayma yok).
            let text_off = sp(6) + Self::icon_size() + sp(8);
            let text_x = r.0 + text_off;
            let text_y = r.1 + r.3.saturating_sub(Self::text_height()) / 2;
            Self::draw_string_clipped(fb, text_x, text_y, label, text_col,
                (r.0 + text_off - sp(2), r.1, r.2.saturating_sub(text_off), r.3));
        }
    }

    // -----------------------------------------------------------------------
    // 4. Takvim Kaplaması (Clock -> Calendar)
    // -----------------------------------------------------------------------
    pub fn calendar_rect(width: usize, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let w = 252;
        let h = 238;
        let x = (layout.clock.0 + layout.clock.2).saturating_sub(w).min(width.saturating_sub(w));
        let y = layout.tb_y.saturating_sub(h + 6);
        (x, y, w, h)
    }

    pub fn render_calendar(&self, fb: &mut Framebuffer, now: &DateTime, layout: &TrayLayout) {
        let (x, y, w, h) = Self::calendar_rect(fb.width, layout);

        fb.draw_shadow(x, y, w, h, Color::SHADOW);
        fb.draw_rect(x, y, w, h, Color::TASKBAR_BG);
        fb.draw_rect_outline(x, y, w, h, Color::BORDER);

        // Başlık: < Ay Yıl >
        fb.draw_vgradient(x + 1, y + 1, w - 2, 30, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -14));
        Self::draw_string(fb, x + 10, y + 11, "<", Color::ACCENT_BLUE);
        let mut title = TextBuf::new();
        title.push_str(if ui_english() { DateTime::month_name_en(self.cal_month) } else { DateTime::month_name(self.cal_month) });
        title.push_byte(b' ');
        title.push_u4(self.cal_year as u32);
        let tw = Self::text_width(title.as_str(), 1);
        Self::draw_string(fb, x + (w - tw) / 2, y + 11, title.as_str(), Color::TEXT_MAIN);
        Self::draw_string(fb, x + w - 22, y + 11, ">", Color::ACCENT_BLUE);

        // Gün başlıkları (Pt..Pz)
        let grid_x = x + 10;
        let grid_y = y + 40;
        let cell_w = (w - 20) / 7;
        for d in 0..7usize {
            let label = if ui_english() { DateTime::weekday_short_en(d as u8) } else { DateTime::weekday_short(d as u8) };
            let tw = Self::text_width(label, 1);
            Self::draw_string(fb, grid_x + d * cell_w + cell_w.saturating_sub(tw) / 2, grid_y, label, Color::TEXT_MUTED);
        }

        // Ayın günleri
        let first_w = DateTime::weekday(self.cal_year, self.cal_month, 1) as usize;
        let dim = DateTime::days_in_month(self.cal_year, self.cal_month) as usize;
        let today_month = self.cal_year == now.year && self.cal_month == now.month;
        let row_h = 26;
        for day in 1..=dim {
            let idx = first_w + (day - 1);
            let row = idx / 7;
            let col = idx % 7;
            let cx = grid_x + col * cell_w;
            let cy = grid_y + 16 + row * row_h;
            if cy + 20 > y + h - 8 {
                break;
            }
            let is_today = today_month && day == now.day as usize;
            if is_today {
                fb.draw_rect(cx, cy, cell_w - 2, 20, Color::ACCENT_BLUE);
            }
            let mut db = TextBuf::new();
            db.push_u2(day as u32);
            let col_txt = if is_today { Self::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN };
            let tw = Self::text_width(db.as_str(), 1);
            Self::draw_string(fb, cx + cell_w.saturating_sub(tw) / 2, cy + 6, db.as_str(), col_txt);
        }

        // Alt bilgi: tam tarih ve saat
        fb.draw_rect(x + 1, y + h - 26, w - 2, 25, Color::TITLE_BAR_INACTIVE);
        let mut foot = TextBuf::new();
        foot.push_u2(now.hour as u32);
        foot.push_byte(b':');
        foot.push_u2(now.minute as u32);
        foot.push_byte(b':');
        foot.push_u2(now.second as u32);
        foot.push_str("  ");
        let weekday = DateTime::weekday(now.year, now.month, now.day);
        foot.push_str(if ui_english() { DateTime::weekday_name_en(weekday) } else { DateTime::weekday_name(weekday) });
        Self::draw_string(fb, x + 12, y + h - 20, foot.as_str(), Color::ACCENT_GREEN);
    }

    // -----------------------------------------------------------------------
    // 5. Ses Paneli (Volume Popup)
    // -----------------------------------------------------------------------
    pub fn volume_slider_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let w = 220;
        let x = layout.volume.0.saturating_sub(w).saturating_sub(10).max(10);
        let y = layout.tb_y.saturating_sub(96);
        (x, y, w, 90)
    }

    pub fn volume_track_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let panel = self.volume_slider_rect(layout);
        (panel.0 + 16, panel.1 + 46, panel.2 - 32, 12)
    }

    pub fn volume_mute_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let panel = self.volume_slider_rect(layout);
        (panel.0 + panel.2 - 78, panel.1 + 66, 62, 20)
    }

    pub fn render_volume_popup(&self, fb: &mut Framebuffer, layout: &TrayLayout, volume: u8, is_muted: bool) {
        let panel = self.volume_slider_rect(layout);
        let (x, y, w, h) = panel;

        fb.draw_shadow(x, y, w, h, Color::SHADOW);
        fb.draw_rect(x, y, w, h, Color::TASKBAR_BG);
        fb.draw_rect_outline(x, y, w, h, Color::BORDER);
        fb.draw_vgradient(x + 1, y + 1, w - 2, 26, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -14));
        Self::draw_string_shadow(fb, x + 12, y + 10, ui_text("Ses Düzeyi", "Volume"), Color::TEXT_MAIN, Color::SHADOW, 1);

        // Yüzde
        let mut pct = TextBuf::new();
        pct.push_byte(b'%');
        pct.push_u2(volume as u32);
        let pc = if is_muted { Color::TEXT_MUTED } else { Color::ACCENT_GREEN };
        Self::draw_string(fb, x + w - 42, y + 10, pct.as_str(), pc);

        // Kaydırıcı yolu
        let track = self.volume_track_rect(layout);
        fb.draw_rect(track.0, track.1, track.2, track.3, Color::TITLE_BAR_INACTIVE);
        let fill_w = (track.2 * volume as usize) / 100;
        if !is_muted {
            fb.draw_rect(track.0, track.1, fill_w, track.3, Color::ACCENT_BLUE);
        }
        // Tutamaç
        let knob_x = track.0 + fill_w.saturating_sub(4);
        fb.draw_rect(knob_x, track.1 - 4, 8, track.3 + 8, Color::TEXT_MAIN);

        // Sessiz düğmesi
        let mute = self.volume_mute_rect(layout);
        let bg = if is_muted { Color::ACCENT_RED } else { Color::TITLE_BAR };
        let label = match (is_muted, ui_english()) {
            (true, true) => "Unmute", (false, true) => "Mute",
            (true, false) => "Sesi Aç", (false, false) => "Sustur",
        };
        Self::draw_button(fb, mute, bg, label, Color::TEXT_MAIN, false);
    }

    pub fn draw_mouse_cursor(&self, fb: &mut Framebuffer, mx: usize, my: usize) {
        fb.save_cursor_background(mx, my);
        // Not: BGA/VGA grafik modda HW sprite yoktur; yazılım imleci her
        // zaman çizilir. HW bayrağı yalnızca CRTC konum takibi içindir.
        // Fare bir pencere kenarı üzerindeyse yönlü yeniden boyutlandırma imleci çiz.
        let over_control = self.windows.iter().flatten().any(|win| {
            win.is_open && !win.is_minimized && in_rect(mx, my, win.close_rect())
        });
        if !over_control {
        if let Some((_, edge)) = self.topmost_resize_edge(mx, my) {
            self.draw_resize_cursor(fb, mx, my, edge);
            return;
        }
        }

        const CURSOR_PATTERN: [u8; 12] = [
            0b10000000,
            0b11000000,
            0b11100000,
            0b11110000,
            0b11111000,
            0b11111100,
            0b11111110,
            0b11110000,
            0b10011000,
            0b00011000,
            0b00001100,
            0b00001100,
        ];

        // Önce dış hat (outline), sonra dolgu — her zeminde görünür kalır.
        for (row, &byte) in CURSOR_PATTERN.iter().enumerate() {
            for col in 0..8 {
                if (byte & (1 << (7 - col))) != 0 {
                    fb.put_pixel(mx + col + 1, my + row, Color::CURSOR_OUTLINE);
                    fb.put_pixel(mx + col, my + row + 1, Color::CURSOR_OUTLINE);
                    fb.put_pixel(mx + col + 1, my + row + 1, Color::CURSOR_OUTLINE);
                }
            }
        }
        for (row, &byte) in CURSOR_PATTERN.iter().enumerate() {
            for col in 0..8 {
                if (byte & (1 << (7 - col))) != 0 {
                    fb.put_pixel(mx + col, my + row, Color::CURSOR_FILL);
                }
            }
        }
    }

    /// Yönlü yeniden boyutlandırma imleci: kenarlar için çift ok, köşeler için çapraz.
    fn draw_resize_cursor(&self, fb: &mut Framebuffer, mx: usize, my: usize, edge: ResizeEdge) {
        use ResizeEdge::*;
        // Her yön için 7x7 ikon deseni (1 = dolgu).
        const H_ARROW: [u8; 7] = [
            0b0001000, 0b0001100, 0b1111111, 0b1111111, 0b1111111, 0b0001100, 0b0001000,
        ];
        const V_ARROW: [u8; 7] = [
            0b0001000, 0b0011100, 0b0101010, 0b0001000, 0b0101010, 0b0011100, 0b0001000,
        ];
        const DIAG_TLBR: [u8; 7] = [
            0b1100000, 0b1110000, 0b1111000, 0b0001000, 0b0001100, 0b0001110, 0b0000011,
        ];
        const DIAG_TRBL: [u8; 7] = [
            0b0000011, 0b0000111, 0b0001111, 0b0001000, 0b0011000, 0b0111000, 0b1100000,
        ];
        let pat: &[u8; 7] = match edge {
            Left | Right => &H_ARROW,
            Top | Bottom => &V_ARROW,
            TopLeft | BottomRight => &DIAG_TLBR,
            TopRight | BottomLeft => &DIAG_TRBL,
        };
        // İmleci tutamacın merkezine hizala.
        let ox = mx.saturating_sub(3);
        let oy = my.saturating_sub(3);
        for (row, &bits) in pat.iter().enumerate() {
            for col in 0..7 {
                if (bits & (1 << (6 - col))) != 0 {
                    fb.put_pixel(ox + col, oy + row, Color::CURSOR_FILL);
                }
            }
        }
    }
}

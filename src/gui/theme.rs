//! minOs Dinamik Tema Motoru (Dynamic Theme Engine)
//!
//! Renkler artık sabit değerler değil; `Color::TEXT_MAIN` gibi sabitler bir
//! *tema jetonu* (token) taşır. Gerçek RGB değeri, kare çizimi sırasında
//! `resolve()` ile aktif paletden okunur. Böylece tüm arayüz, tek bir çağrıyla
//! (`set_theme`) anında tema değiştirir ve tüm uygulamalar uyum sağlar.

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Tema jetonlarını normal 0xRRGGBB renklerinden ayıran üst bayt.
pub const THEME_MARKER: u32 = 0xFF00_0000;
/// Jeton işaretini (marker) maskeleyen bit deseni.
pub const THEME_MASK: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// Palet indeksleri (Color sabitleri bu indeksleri kullanır)
// ---------------------------------------------------------------------------
pub const IDX_DESKTOP_BG: u32 = 0;
pub const IDX_WINDOW_BG: u32 = 1;
pub const IDX_TITLEBAR: u32 = 2;
pub const IDX_TITLEBAR_INACTIVE: u32 = 3;
pub const IDX_TASKBAR_BG: u32 = 4;
pub const IDX_ACCENT_BLUE: u32 = 5;
pub const IDX_ACCENT_GREEN: u32 = 6;
pub const IDX_ACCENT_RED: u32 = 7;
pub const IDX_ACCENT_YELLOW: u32 = 8;
pub const IDX_ACCENT_PINK: u32 = 9;
pub const IDX_ACCENT_CAT: u32 = 10;
pub const IDX_TEXT_MAIN: u32 = 11;
pub const IDX_TEXT_MUTED: u32 = 12;
pub const IDX_CURSOR_FILL: u32 = 13;
pub const IDX_CURSOR_OUTLINE: u32 = 14;
pub const IDX_BORDER: u32 = 15;
pub const IDX_SHADOW: u32 = 16;

pub const PALETTE_LEN: usize = 17;

/// Bir renk jetonu üretir. `const` bağlamda kullanılabilir.
#[inline(always)]
pub const fn token(idx: u32) -> u32 {
    THEME_MARKER | (idx & 0xFF)
}

/// Verilen renk bir tema jetonu mu?
#[inline(always)]
pub const fn is_token(color: u32) -> bool {
    (color & THEME_MASK) == THEME_MARKER
}

/// Jetonu aktif paletdeki gerçek RGB değerine çevirir.
#[inline(always)]
pub fn resolve(color: u32) -> u32 {
    if (color & THEME_MASK) == THEME_MARKER {
        let idx = (color & 0xFF) as usize;
        if idx < PALETTE_LEN {
            PALETTE[idx].load(Ordering::Relaxed)
        } else {
            // Opaque framebuffer/icon colors can share the marker prefix.
            // Invalid token indices are therefore treated as literal RGB(A),
            // not used as unchecked palette offsets.
            color
        }
    } else {
        color
    }
}

// ---------------------------------------------------------------------------
// Hazır temalar
// ---------------------------------------------------------------------------
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ThemeId {
    VelvetDark,
    Daylight,
    Nord,
    TerminalGreen,
    Sunset,
}

impl ThemeId {
    pub const ALL: [ThemeId; 5] = [
        ThemeId::VelvetDark,
        ThemeId::Daylight,
        ThemeId::Nord,
        ThemeId::TerminalGreen,
        ThemeId::Sunset,
    ];

    pub fn next(self) -> Self {
        match self {
            ThemeId::VelvetDark => ThemeId::Daylight,
            ThemeId::Daylight => ThemeId::Nord,
            ThemeId::Nord => ThemeId::TerminalGreen,
            ThemeId::TerminalGreen => ThemeId::Sunset,
            ThemeId::Sunset => ThemeId::VelvetDark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Kadife Gece (Velvet Dark)",
            ThemeId::Daylight => "Gün Işığı (Daylight)",
            ThemeId::Nord => "Kutup Mavisi (Nord)",
            ThemeId::TerminalGreen => "Yeşil Terminal (Terminal)",
            ThemeId::Sunset => "Gün Batımı (Sunset)",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Gece",
            ThemeId::Daylight => "Gündüz",
            ThemeId::Nord => "Nord",
            ThemeId::TerminalGreen => "Yeşil",
            ThemeId::Sunset => "Günbatımı",
        }
    }

    pub(crate) fn index(self) -> u8 {
        match self {
            ThemeId::VelvetDark => 0,
            ThemeId::Daylight => 1,
            ThemeId::Nord => 2,
            ThemeId::TerminalGreen => 3,
            ThemeId::Sunset => 4,
        }
    }
}

// Palet sırası: yukarıdaki IDX_* sabitlerinin sırası ile birebir aynı olmalıdır.
const VELVET_DARK: [u32; PALETTE_LEN] = [
    0x181825, // desktop bg
    0x1E1E2E, // window bg
    0x313244, // titlebar (aktif)
    0x232336, // titlebar (pasif)
    0x11111B, // taskbar
    0x89B4FA, // accent blue
    0xA6E3A1, // accent green
    0xF38BA8, // accent red
    0xF9E2AF, // accent yellow
    0xF5C2E7, // accent pink
    0xF2CDCD, // accent cat
    0xE6E9F5, // text main (yüksek okunabilirlik)
    0xA6ADC8, // text muted (eski 0x6C7086 yerine daha okunaklı)
    0xFFFFFF, // cursor fill
    0x181825, // cursor outline
    0x45475A, // border
    0x0A0A11, // shadow
];

// Acik temada vurgu renkleri metin olarak da kullanilir (etiket, durum, surum
// bilgisi). Bu yuzden tonlar beyaz/pasif baslik zemininde en az 4.5:1 kontrast
// verecek kadar koyulastirildi (WCAG AA, normal metin).
const DAYLIGHT: [u32; PALETTE_LEN] = [
    0xDCE0E9, // desktop bg
    0xFFFFFF, // window bg
    0xE7EAF0, // titlebar (aktif)
    0xD3D7E0, // titlebar (pasif)
    0xEDEFF4, // taskbar
    0x1450C8, // accent blue  (en dusuk kontrast 4.84:1)
    0x23651A, // accent green (en dusuk kontrast 4.94:1)
    0xB31238, // accent red   (en dusuk kontrast 4.76:1)
    0x7A4E00, // accent yellow (en dusuk kontrast 4.99:1)
    0x93356F, // accent pink  (en dusuk kontrast 4.89:1)
    0xFFAA00, // accent cat
    0x1E2030, // text main (koyu)
    0x575A61, // text muted (pasif baslikta 4.79:1)
    0x2A2C3A, // cursor fill
    0xFFFFFF, // cursor outline
    0x9CA0B0, // border
    0xB8BDC9, // shadow
];

// Nord'da vurgu renkleri de metin olarak kullanilir; resmi Nord tonlari koyu
// baslik zemininde 2.1-4.3:1 kaliyordu. Tonlar ayni ton ailesinde kalinarak
// en az 4.5:1 kontrast verecek sekilde acildi.
const NORD: [u32; PALETTE_LEN] = [
    0x2E3440, // desktop bg
    0x3B4252, // window bg
    0x434C5E, // titlebar (aktif)
    0x353C4A, // titlebar (pasif)
    0x272C36, // taskbar
    0x93C9D8, // accent blue  (en dusuk kontrast 4.76:1)
    0xACCA96, // accent green (en dusuk kontrast 4.78:1)
    0xF2B0B6, // accent red   (en dusuk kontrast 4.79:1)
    0xEBCB8B, // accent yellow
    0xDAB6D5, // accent pink  (en dusuk kontrast 4.78:1)
    0xD08770, // accent cat
    0xECEFF4, // text main
    0xC8D0DC, // text muted (yüksek kontrast)
    0xECEFF4, // cursor fill
    0x2E3440, // cursor outline
    0x4C566A, // border
    0x1E222A, // shadow
];

// Modern terminal temasi: derin siyah zemin uzerinde yuksek kontrastli neon yesil.
const TERMINAL_GREEN: [u32; PALETTE_LEN] = [
    0x050805, // desktop bg
    0x0B120B, // window bg
    0x14260F, // titlebar (aktif)
    0x0E1A0C, // titlebar (pasif)
    0x030603, // taskbar
    0x39FF6A, // accent blue (neon mint)
    0x7CFF9B, // accent green
    0xFF5C6C, // accent red
    0xE8FF6B, // accent yellow
    0x63D2FF, // accent pink (cyan)
    0xB6FF7A, // accent cat
    0xDFFFE2, // text main
    0x8FCE9A, // text muted
    0xDFFFE2, // cursor fill
    0x050805, // cursor outline
    0x1F3A1C, // border
    0x000000, // shadow
];

// Modern gun batimi temasi: sicak mor-gul zemin, turuncu vurgular.
const SUNSET: [u32; PALETTE_LEN] = [
    0x241726, // desktop bg
    0x2F1E33, // window bg
    0x4A2B46, // titlebar (aktif)
    0x3A233C, // titlebar (pasif)
    0x1A1020, // taskbar
    0xFFB86B, // accent blue (amber)
    0x8BE9A5, // accent green
    0xFF7E96, // accent red (coral; en dusuk kontrast 5.03:1)
    0xFFD479, // accent yellow
    0xE58BFF, // accent pink (orchid)
    0xFFA36B, // accent cat
    0xFFF1E6, // text main
    0xD3B8D8, // text muted
    0xFFF1E6, // cursor fill
    0x241726, // cursor outline
    0x5E4064, // border
    0x140B18, // shadow
];

const fn palette_for(id: ThemeId) -> [u32; PALETTE_LEN] {
    match id {
        ThemeId::VelvetDark => VELVET_DARK,
        ThemeId::Daylight => DAYLIGHT,
        ThemeId::Nord => NORD,
        ThemeId::TerminalGreen => TERMINAL_GREEN,
        ThemeId::Sunset => SUNSET,
    }
}

/// Bir temanin RGB paletini aktif paleti degistirmeden dondurur.
///
/// Arayuz onizlemeleri ve host testlerindeki kontrast denetimleri icin.
pub const fn palette(id: ThemeId) -> [u32; PALETTE_LEN] {
    palette_for(id)
}

/// Bir rengin `IDX_*` indeksindeki degerini verilen paletten okur.
pub const fn palette_color(id: ThemeId, index: u32) -> u32 {
    let p = palette_for(id);
    p[index as usize % PALETTE_LEN]
}

/// sRGB kanal degerini (0..=1) lineer isik degerine cevirir.
///
/// `core` hedefinde `powf` bulunmadigi icin 2.4 ussu, karekok gerektirmeyen
/// bir Newton besinci-kok yaklasimiyla hesaplanir.
fn srgb_to_linear(channel: u8) -> f32 {
    let c = channel as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        let x = (c + 0.055) / 1.055;
        // x^2.4 = x^2 * x^0.4, x^0.4 = (x^2)^(1/5)
        x * x * fifth_root(x * x)
    }
}

/// `y` degerinin besinci koku (0 <= y <= 1 icin Newton yinelemesi).
fn fifth_root(y: f32) -> f32 {
    if y <= 0.0 {
        return 0.0;
    }
    // y 0..=1 araliginda oldugu icin 1.0 iyi bir baslangic tahminidir;
    // kucuk y degerlerinde `root = y` Newton yinelemesini bozardi.
    let mut root = 1.0;
    let mut i = 0;
    while i < 24 {
        let square = root * root;
        root = (4.0 * root + y / (square * square)) / 5.0;
        i += 1;
    }
    root
}

/// WCAG 2.x bagil parlaklik (0.0 = siyah, 1.0 = beyaz).
pub fn relative_luminance(rgb: u32) -> f32 {
    let r = srgb_to_linear(((rgb >> 16) & 0xFF) as u8);
    let g = srgb_to_linear(((rgb >> 8) & 0xFF) as u8);
    let b = srgb_to_linear((rgb & 0xFF) as u8);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Iki renk arasindaki WCAG kontrast orani (1.0 .. 21.0).
pub fn contrast_ratio(a: u32, b: u32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Bir temanın önizleme renklerini (zemin + vurgu) aktif paleti değiştirmeden döndürür.
/// Ayar ekranındaki renk örnekleri (swatch) için kullanılır.
pub const fn preview_colors(id: ThemeId) -> (u32, u32) {
    let p = palette_for(id);
    // (masaüstü zemin, vurgu mavi)
    (p[IDX_DESKTOP_BG as usize], p[IDX_ACCENT_BLUE as usize])
}

// ---------------------------------------------------------------------------
// Global durum
// ---------------------------------------------------------------------------
static PALETTE: [AtomicU32; PALETTE_LEN] = [const { AtomicU32::new(0) }; PALETTE_LEN];
static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);

/// Aktif paleti başlangıç teması ile doldurur. `_start` içinde çağrılır.
pub fn init() {
    apply(ThemeId::VelvetDark);
}

/// Temayı değiştirir; sonraki karede tüm arayüz yeni renklerle çizilir.
pub fn set_theme(id: ThemeId) {
    apply(id);
}

/// Aktif tema kimliği.
pub fn current() -> ThemeId {
    ThemeId::ALL[(CURRENT_THEME.load(Ordering::Relaxed) as usize).min(ThemeId::ALL.len() - 1)]
}

/// Sıradaki temaya geçer ve yeni temayı döndürür.
pub fn cycle() -> ThemeId {
    let next = current().next();
    apply(next);
    next
}

fn apply(id: ThemeId) {
    let p = palette_for(id);
    for (i, &c) in p.iter().enumerate() {
        PALETTE[i].store(c, Ordering::Relaxed);
    }
    CURRENT_THEME.store(id.index(), Ordering::Relaxed);
}

/// Aktif paletten tek bir renk indeksini okur (UI kodu doğrudan RGB isteyebilir).
#[inline(always)]
pub fn get(idx: u32) -> u32 {
    let index = idx as usize;
    if index < PALETTE_LEN {
        PALETTE[index].load(Ordering::Relaxed)
    } else {
        0
    }
}

//! MinOS Dinamik Tema Motoru (Dynamic Theme Engine)
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
        PALETTE[(color & 0xFF) as usize].load(Ordering::Relaxed)
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
}

impl ThemeId {
    pub const ALL: [ThemeId; 3] = [ThemeId::VelvetDark, ThemeId::Daylight, ThemeId::Nord];

    pub fn next(self) -> Self {
        match self {
            ThemeId::VelvetDark => ThemeId::Daylight,
            ThemeId::Daylight => ThemeId::Nord,
            ThemeId::Nord => ThemeId::VelvetDark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Kadife Gece (Velvet Dark)",
            ThemeId::Daylight => "Gün Işığı (Daylight)",
            ThemeId::Nord => "Kutup Mavisi (Nord)",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Gece",
            ThemeId::Daylight => "Gündüz",
            ThemeId::Nord => "Nord",
        }
    }

    fn index(self) -> u8 {
        match self {
            ThemeId::VelvetDark => 0,
            ThemeId::Daylight => 1,
            ThemeId::Nord => 2,
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

const DAYLIGHT: [u32; PALETTE_LEN] = [
    0xDCE0E9, // desktop bg
    0xFFFFFF, // window bg
    0xE7EAF0, // titlebar (aktif)
    0xD3D7E0, // titlebar (pasif)
    0xEDEFF4, // taskbar
    0x1E66F5, // accent blue
    0x40A02B, // accent green
    0xD20F39, // accent red
    0xDF8E1D, // accent yellow
    0xEA76CB, // accent pink
    0xFFAA00, // accent cat
    0x1E2030, // text main (koyu)
    0x5C5F66, // text muted
    0x2A2C3A, // cursor fill
    0xFFFFFF, // cursor outline
    0x9CA0B0, // border
    0xB8BDC9, // shadow
];

const NORD: [u32; PALETTE_LEN] = [
    0x2E3440, // desktop bg
    0x3B4252, // window bg
    0x434C5E, // titlebar (aktif)
    0x353C4A, // titlebar (pasif)
    0x272C36, // taskbar
    0x88C0D0, // accent blue
    0xA3BE8C, // accent green
    0xBF616A, // accent red
    0xEBCB8B, // accent yellow
    0xB48EAD, // accent pink
    0xD08770, // accent cat
    0xECEFF4, // text main
    0xB8C0CE, // text muted
    0xECEFF4, // cursor fill
    0x2E3440, // cursor outline
    0x4C566A, // border
    0x1E222A, // shadow
];

const fn palette_for(id: ThemeId) -> [u32; PALETTE_LEN] {
    match id {
        ThemeId::VelvetDark => VELVET_DARK,
        ThemeId::Daylight => DAYLIGHT,
        ThemeId::Nord => NORD,
    }
}

// ---------------------------------------------------------------------------
// Global durum
// ---------------------------------------------------------------------------
static PALETTE: [AtomicU32; PALETTE_LEN] =
    [const { AtomicU32::new(0) }; PALETTE_LEN];
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
    PALETTE[(idx & 0xFF) as usize].load(Ordering::Relaxed)
}

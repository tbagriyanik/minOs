/// # MinOs Dynamic Theme Engine
///
/// This module implements a flexible theming system for the MinOs GUI.
/// Instead of hardcoded color values, all UI colors reference **theme tokens**.
/// These tokens are resolved to actual RGB values at render time based on the
/// currently active palette.
///
/// ## Design
///
/// - **Tokens**: Colors like `Color::TEXT_MAIN` are actually token values
///   with a special marker bit (0xFF00_0000) in the upper byte.
/// - **Resolution**: At draw time, `theme::resolve()` maps tokens to the
///   active palette's RGB values.
/// - **Instant switching**: Calling `set_theme()` swaps the entire palette,
///   and all subsequent draws use the new colors immediately.
///
/// ## Usage
///
/// ```ignore
/// // Set theme at startup or runtime
/// gui::theme::set_theme(gui::theme::ThemeId::Nord);
///
/// // In render code, use Color constants (they're tokens)
/// fb.draw_text(10, 10, "Hello", gui::Color::TEXT_MAIN);
/// // At draw time, resolves to Nord's text color
/// ```
///
/// ## Palette Indices
///
/// Each token corresponds to a palette index:
/// - 0: Desktop background
/// - 1: Window background
/// - 2: Active title bar
/// - 3: Inactive title bar
/// - 4: Taskbar background
/// - 5: Accent blue
/// - 6: Accent green
/// - 7: Accent red
/// - 8: Accent yellow
/// - 9: Accent pink
/// - 10: Accent cat (special)
/// - 11: Primary text
/// - 12: Muted text
/// - 13: Cursor fill
/// - 14: Cursor outline
/// - 15: Border
/// - 16: Shadow
///
/// ## Available Themes
///
/// - [`ThemeId::VelvetDark`] - Default dark theme
/// - [`ThemeId::Daylight`] - Light theme
/// - [`ThemeId::Nord`] - Nord color scheme
/// - [`ThemeId::TerminalGreen`] - Retro terminal green
/// - [`ThemeId::Sunset`] - Warm sunset tones
/// - [`ThemeId::Dracula`] - Dracula color scheme
/// - [`ThemeId::Monokai`] - Monokai editor theme
/// - [`ThemeId::SolarizedDark`] - Solarized dark
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Tema jetonlarını normal 0xRRGGBB renklerinden ayıran üst bayt.
pub const THEME_MARKER: u32 = 0xFF00_0000;
/// Jeton işaretini (marker) maskeleyen bit deseni.
pub const THEME_MASK: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// Palet indeksleri (Color sabitleri bu indeksleri kullanır)
// ---------------------------------------------------------------------------
/// Palette index constants for theme tokens.
///
/// These indices correspond to the positions in the active palette array.
/// Each index maps to a specific UI element color.
pub const IDX_DESKTOP_BG: u32 = 0;
/// Window/panel background color index.
pub const IDX_WINDOW_BG: u32 = 1;
/// Active window title bar index.
pub const IDX_TITLEBAR: u32 = 2;
/// Inactive window title bar index.
pub const IDX_TITLEBAR_INACTIVE: u32 = 3;
/// Taskbar background index.
pub const IDX_TASKBAR_BG: u32 = 4;
/// Accent blue index (primary action color).
pub const IDX_ACCENT_BLUE: u32 = 5;
/// Accent green index (success/active state).
pub const IDX_ACCENT_GREEN: u32 = 6;
/// Accent red index (error/danger state).
pub const IDX_ACCENT_RED: u32 = 7;
/// Accent yellow index (warning/caution).
pub const IDX_ACCENT_YELLOW: u32 = 8;
/// Accent pink index (special highlight).
pub const IDX_ACCENT_PINK: u32 = 9;
/// Accent cat index (easter egg/special).
pub const IDX_ACCENT_CAT: u32 = 10;
/// Primary text color index.
pub const IDX_TEXT_MAIN: u32 = 11;
/// Secondary/muted text color index.
pub const IDX_TEXT_MUTED: u32 = 12;
/// Mouse cursor fill color index.
pub const IDX_CURSOR_FILL: u32 = 13;
/// Mouse cursor outline color index.
pub const IDX_CURSOR_OUTLINE: u32 = 14;
/// Window border color index.
pub const IDX_BORDER: u32 = 15;
/// Drop shadow color index.
pub const IDX_SHADOW: u32 = 16;

/// Total number of palette entries.
pub const PALETTE_LEN: usize = 17;

/// Creates a theme token from a palette index.
///
/// Tokens are marked with `THEME_MARKER` in the upper byte to distinguish
/// them from literal RGB values. Can be used in `const` contexts.
///
/// # Arguments
/// * `idx` - Palette index (0-16)
///
/// # Returns
/// A token value for use in [`Color`] constants.
#[inline(always)]
pub const fn token(idx: u32) -> u32 {
    THEME_MARKER | (idx & 0xFF)
}

/// Checks if a color value is a theme token.
///
/// # Arguments
/// * `color` - Color value to check
///
/// # Returns
/// `true` if the value is a token, `false` if it's a literal RGB.
#[inline(always)]
pub const fn is_token(color: u32) -> bool {
    (color & THEME_MASK) == THEME_MARKER
}

/// Resolves a theme token to its current RGB value.
///
/// If the input is a token, looks up the active palette entry.
/// If the input is a literal RGB value (no marker bit), returns it unchanged.
/// If the token index is out of bounds, returns the raw value as fallback.
///
/// # Arguments
/// * `color` - Token or literal RGB value
///
/// # Returns
/// Resolved 0xRRGGBB color value.
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

/// Pre-defined theme identifiers.
///
/// Each theme provides a complete 17-color palette covering all UI elements.
/// Themes can be switched at runtime via [`set_theme()`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ThemeId {
    /// Default dark theme with deep blues and warm accents.
    VelvetDark,
    /// Light theme with soft whites and subtle blues.
    Daylight,
    /// Nord color scheme - arctic, nordic tones.
    Nord,
    /// Retro terminal green-on-black.
    TerminalGreen,
    /// Warm sunset oranges and reds.
    Sunset,
    /// Popular Dracula editor theme.
    Dracula,
    /// Monokai editor theme.
    Monokai,
    /// Solarized dark variant.
    SolarizedDark,
}

impl ThemeId {
    /// All available themes in order.
    pub const ALL: [ThemeId; 8] = [
        ThemeId::VelvetDark,
        ThemeId::Daylight,
        ThemeId::Nord,
        ThemeId::TerminalGreen,
        ThemeId::Sunset,
        ThemeId::Dracula,
        ThemeId::Monokai,
        ThemeId::SolarizedDark,
    ];

    /// Returns the next theme in the cycle.
    ///
    /// Useful for cycling through themes with a keybind.
    ///
    /// # Returns
    /// The next [`ThemeId`] in the sequence.
    pub fn next(self) -> Self {
        match self {
            ThemeId::VelvetDark => ThemeId::Daylight,
            ThemeId::Daylight => ThemeId::Nord,
            ThemeId::Nord => ThemeId::TerminalGreen,
            ThemeId::TerminalGreen => ThemeId::Sunset,
            ThemeId::Sunset => ThemeId::Dracula,
            ThemeId::Dracula => ThemeId::Monokai,
            ThemeId::Monokai => ThemeId::SolarizedDark,
            ThemeId::SolarizedDark => ThemeId::VelvetDark,
        }
    }

    /// Returns the full localized display name.
    ///
    /// # Returns
    /// Human-readable theme name (Turkish/English mixed for display).
    pub fn name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Kadife Gece (Velvet Dark)",
            ThemeId::Daylight => "Gün Işığı (Daylight)",
            ThemeId::Nord => "Kutup Mavisi (Nord)",
            ThemeId::TerminalGreen => "Yeşil Terminal (Terminal)",
            ThemeId::Sunset => "Gün Batımı (Sunset)",
            ThemeId::Dracula => "Dracula",
            ThemeId::Monokai => "Monokai",
            ThemeId::SolarizedDark => "Gün Işığı Koyu (Solarized Dark)",
        }
    }

    /// Returns a short name for UI selectors.
    ///
    /// # Returns
    /// Abbreviated theme name.
    pub fn short_name(self) -> &'static str {
        match self {
            ThemeId::VelvetDark => "Gece",
            ThemeId::Daylight => "Gündüz",
            ThemeId::Nord => "Nord",
            ThemeId::TerminalGreen => "Yeşil",
            ThemeId::Sunset => "Günbatımı",
            ThemeId::Dracula => "Dracula",
            ThemeId::Monokai => "Monokai",
            ThemeId::SolarizedDark => "Solarized",
        }
    }

    /// Returns the internal palette index.
    ///
    /// # Returns
    /// Zero-based index into the theme table.
    pub(crate) fn index(self) -> u8 {
        match self {
            ThemeId::VelvetDark => 0,
            ThemeId::Daylight => 1,
            ThemeId::Nord => 2,
            ThemeId::TerminalGreen => 3,
            ThemeId::Sunset => 4,
            ThemeId::Dracula => 5,
            ThemeId::Monokai => 6,
            ThemeId::SolarizedDark => 7,
        }
    }
}

// Palet sırası: yukarıdaki IDX_* sabitlerinin sırası ile birebir aynı olmalıdır.
const VELVET_DARK: [u32; PALETTE_LEN] = [
    0x1A1D26, // desktop bg (mantle - biraz daha parlak)
    0x24273A, // window bg (base; koyu, metin ile yuksek kontrast)
    0x1E1E2E, // titlebar (aktif) (gri değil, zemin ile uyumlu)
    0x2A2E3E, // titlebar (pasif)
    0x16161E, // taskbar (crust)
    0x89B4FA, // accent blue
    0xA6E3A1, // accent green (daha parlak, koyu zeminde okunabilir)
    0xF38BA8, // accent red
    0xF9E2AF, // accent yellow
    0xF5C2E7, // accent pink
    0xF2CDCD, // accent cat
    0xEDF2FA, // text main (daha parlak, yüksek okunabilirlik)
    0xB4BEFE, // text muted (daha okunaklı)
    0xFFFFFF, // cursor fill
    0x1A1D26, // cursor outline
    0x4A5066, // border (daha belirgin)
    0x0C0E14, // shadow
];

// Acik temada vurgu renkleri metin olarak da kullanilir (etiket, durum, surum
// bilgisi). Bu yuzden tonlar beyaz/pasif baslik zemininde en az 4.5:1 kontrast
// verecek kadar koyulastirildi (WCAG AA, normal metin).
const DAYLIGHT: [u32; PALETTE_LEN] = [
    0xD6DAE3, // desktop bg (biraz daha sıcak)
    0xFFFFFF, // window bg
    0xE2E6EB, // titlebar (aktif)
    0xD0D4DB, // titlebar (pasif)
    0xE8ECF2, // taskbar
    0x1046B8, // accent blue  (daha belirgin)
    0x1F5A16, // accent green (daha belirgin)
    0xA31030, // accent red   (daha belirgin)
    0x6B4400, // accent yellow (daha belirgin)
    0x832C5F, // accent pink  (daha belirgin)
    0xFF9900, // accent cat
    0x1A1C26, // text main (daha koyu)
    0x4D5058, // text muted (daha belirgin)
    0x242630, // cursor fill
    0xFFFFFF, // cursor outline
    0x8A90A0, // border (daha belirgin)
    0xB0B5C0, // shadow
];

// Nord'da vurgu renkleri de metin olarak kullanilir; resmi Nord tonlari koyu
// baslik zemininde 2.1-4.3:1 kaliyordu. Tonlar ayni ton ailesinde kalinarak
// en az 4.5:1 kontrast verecek sekilde acildi.
const NORD: [u32; PALETTE_LEN] = [
    0x2E3440, // desktop bg
    0x3B4252, // window bg
    0x4C566A, // titlebar (aktif) - daha parlak
    0x3E4656, // titlebar (pasif)
    0x272C36, // taskbar
    0xB8E8F5, // accent blue  (daha parlak, metin olarak okunabilir)
    0xD0E8B8, // accent green (daha parlak, metin olarak okunabilir)
    0xFFC8CC, // accent red   (daha parlak, metin olarak okunabilir)
    0xEBCB8B, // accent yellow
    0xF0C8E8, // accent pink  (daha parlak, metin olarak okunabilir)
    0xD08770, // accent cat
    0xECEFF4, // text main
    0xD8DEE9, // text muted (daha parlak)
    0xECEFF4, // cursor fill
    0x2E3440, // cursor outline
    0x5E81AC, // border (daha belirgin)
    0x1E222A, // shadow
];

// Modern terminal temasi: derin siyah zemin uzerinde yuksek kontrastli neon yesil.
const TERMINAL_GREEN: [u32; PALETTE_LEN] = [
    0x0A0F0A, // desktop bg (biraz daha parlak)
    0x121A12, // window bg
    0x1A2E18, // titlebar (aktif)
    0x122414, // titlebar (pasif)
    0x080E08, // taskbar
    0x00FF6A, // accent blue (neon mint - daha parlak)
    0x00FF9B, // accent green (daha parlak)
    0xFF5C6C, // accent red
    0xE8FF6B, // accent yellow
    0x00D2FF, // accent pink (cyan - daha parlak)
    0x00FF7A, // accent cat
    0xE6FFE6, // text main (daha parlak)
    0x9AFF9A, // text muted (daha parlak)
    0xE6FFE6, // cursor fill
    0x0A0F0A, // cursor outline
    0x2A4A2C, // border (daha belirgin)
    0x000000, // shadow
];

// Modern gun batimi temasi: sicak mor-gul zemin, turuncu vurgular.
const SUNSET: [u32; PALETTE_LEN] = [
    0x2A1B2E, // desktop bg (biraz daha parlak)
    0x38263E, // window bg
    0x553252, // titlebar (aktif) - daha parlak
    0x452B44, // titlebar (pasif)
    0x1F1428, // taskbar
    0xFFC86B, // accent blue (amber - daha parlak)
    0x9BF3B5, // accent green (daha parlak)
    0xFF8E9E, // accent red (coral - daha parlak)
    0xFFE089, // accent yellow (daha parlak)
    0xE99BFF, // accent pink (orchid - daha parlak)
    0xFFB37B, // accent cat
    0xFFF6EC, // text main (daha parlak)
    0xE0C8D6, // text muted (daha parlak)
    0xFFF6EC, // cursor fill
    0x2A1B2E, // cursor outline
    0x6E4C78, // border (daha belirgin)
    0x1A0E20, // shadow
];

// Dracula temasi: populer karanlik tema, yuksek kontrastli canli renkler
const DRACULA: [u32; PALETTE_LEN] = [
    0x282A36, // desktop bg
    0x21222C, // window bg
    0x4E5268, // titlebar (aktif) - daha parlak
    0x3E4256, // titlebar (pasif)
    0x191A21, // taskbar
    0x8BE9FD, // accent cyan
    0x50FA7B, // accent green
    0xFFB8B8, // accent red (metin olarak okunabilir)
    0xF1FA8C, // accent yellow
    0xFFC0E0, // accent pink (metin olarak okunabilir)
    0xBD93F9, // accent purple/cat
    0xF8F8F2, // text main
    0xD6DEFF, // text muted (daha parlak, WCAG AA)
    0xF8F8F2, // cursor fill
    0x282A36, // cursor outline
    0x6B7DB0, // border (daha belirgin)
    0x15161D, // shadow
];

// Monokai temasi: klasik kod editörü teması
const MONOKAI: [u32; PALETTE_LEN] = [
    0x272822, // desktop bg
    0x2D2E27, // window bg
    0x49483E, // titlebar (aktif) - daha parlak
    0x3E3D32, // titlebar (pasif)
    0x1E1F1C, // taskbar
    0x66D9EF, // accent blue/cyan
    0xA6E22E, // accent green
    0xFF9FB8, // accent red/magenta (metin olarak okunabilir)
    0xE6DB74, // accent yellow
    0xD7B8FF, // accent purple (metin olarak okunabilir)
    0xFD971F, // accent orange/cat
    0xF8F8F2, // text main
    0xBCBAAE, // text muted (daha parlak, WCAG AA)
    0xF8F8F0, // cursor fill
    0x272822, // cursor outline
    0x5A5A4E, // border (daha belirgin)
    0x141510, // shadow
];

// Solarized Dark teması: okunaklılığı yüksek bilimsel tema
const SOLARIZED_DARK: [u32; PALETTE_LEN] = [
    0x002B36, // desktop bg (base03)
    0x073642, // window bg (base02)
    0x5E747C, // titlebar (aktif) - daha parlak
    0x4A5A66, // titlebar (pasif)
    0x001F27, // taskbar
    0xE8FFFF, // accent blue (metin olarak okunabilir)
    0xF0FFC0, // accent green (metin olarak okunabilir)
    0xFFF8F0, // accent red (metin olarak okunabilir)
    0xFFFFC0, // accent yellow (metin olarak okunabilir)
    0xFFFFFF, // accent magenta (metin olarak okunabilir)
    0x2AA198, // accent cyan/cat
    0x93A1A1, // text main (base1)
    0xFFFFFF, // text muted (base01 - daha parlak, WCAG AA)
    0x93A1A1, // cursor fill
    0x002B36, // cursor outline
    0x657B83, // border (daha belirgin)
    0x00141A, // shadow
];

const fn palette_for(id: ThemeId) -> [u32; PALETTE_LEN] {
    match id {
        ThemeId::VelvetDark => VELVET_DARK,
        ThemeId::Daylight => DAYLIGHT,
        ThemeId::Nord => NORD,
        ThemeId::TerminalGreen => TERMINAL_GREEN,
        ThemeId::Sunset => SUNSET,
        ThemeId::Dracula => DRACULA,
        ThemeId::Monokai => MONOKAI,
        ThemeId::SolarizedDark => SOLARIZED_DARK,
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

/// Returns preview colors for a theme (desktop bg + accent blue).
///
/// Used for theme swatches in the settings UI. Does not modify the active palette.
///
/// # Arguments
/// * `id` - Theme to preview
///
/// # Returns
/// Tuple of (desktop_bg, accent_blue) RGB values.
pub const fn preview_colors(id: ThemeId) -> (u32, u32) {
    let p = palette_for(id);
    (p[IDX_DESKTOP_BG as usize], p[IDX_ACCENT_BLUE as usize])
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------
static PALETTE: [AtomicU32; PALETTE_LEN] = [const { AtomicU32::new(0) }; PALETTE_LEN];
static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);

/// Initializes the active palette with the default theme.
///
/// Called during kernel startup before GUI initialization.
pub fn init() {
    apply(ThemeId::VelvetDark);
}

/// Switches the active theme.
///
/// All subsequently rendered frames will use the new palette.
/// Does not affect already-rendered pixels.
///
/// # Arguments
/// * `id` - Theme to activate
pub fn set_theme(id: ThemeId) {
    apply(id);
}

/// Returns the currently active theme.
///
/// # Returns
/// The active [`ThemeId`].
pub fn current() -> ThemeId {
    ThemeId::ALL[(CURRENT_THEME.load(Ordering::Relaxed) as usize).min(ThemeId::ALL.len() - 1)]
}

/// Cycles to the next theme and returns it.
///
/// Convenience function for keybind/theme rotation.
///
/// # Returns
/// The newly active [`ThemeId`].
pub fn cycle() -> ThemeId {
    let next = current().next();
    apply(next);
    next
}

/// Applies a theme by writing its palette to the global atomic palette.
fn apply(id: ThemeId) {
    let p = palette_for(id);
    for (i, &c) in p.iter().enumerate() {
        PALETTE[i].store(c, Ordering::Relaxed);
    }
    CURRENT_THEME.store(id.index(), Ordering::Relaxed);
}

/// Reads a single color index from the active palette.
///
/// Bypasses token resolution; used when literal RGB is needed.
///
/// # Arguments
/// * `idx` - Palette index (0-16)
///
/// # Returns
/// RGB color value, or 0 if index out of bounds.
#[inline(always)]
pub fn get(idx: u32) -> u32 {
    let index = idx as usize;
    if index < PALETTE_LEN {
        PALETTE[index].load(Ordering::Relaxed)
    } else {
        0
    }
}

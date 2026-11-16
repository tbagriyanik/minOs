//! Early framebuffer text output for boot progress and fatal errors.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::x86_64::sync::SpinMutex;
use crate::drivers::keyboard::KeyAction;
use crate::drivers::video::Framebuffer;

/// `write_color` paylaşılan imleç çerçevesi (`CURSOR_*`), framebuffer ve
/// konsol transkriptini (`LOG_LINES`) günceller; paralel sürücü hatları birden çok
/// CPU'dan aynı anda yazdırabildiği için tüm çıktı bu kilitle serileştirilir.
static TEXT_LOCK: SpinMutex<()> = SpinMutex::new(());

static ACTIVE: AtomicBool = AtomicBool::new(false);
static IN_ERROR_PROMPT: AtomicBool = AtomicBool::new(false);
static mut FB: Option<Framebuffer> = None;
static mut CURSOR_X: usize = TEXT_LEFT;
static mut CURSOR_Y: usize = TEXT_TOP;
static mut SAVED_SCREEN_BUF: [u32; crate::drivers::video::BACKBUFFER_LEN] =
    [0; crate::drivers::video::BACKBUFFER_LEN];
static mut SAVED_SCREEN_DIM: Option<(usize, usize)> = None;

/// Konsol transkripti: her satır (karakter, renk) hücrelerinden oluşur.
/// Ekran kaydırılsa da metinler bu tamponda saklanmaya devam eder; böylece
/// PageUp/PageDown ile geçmişe bakılabilir ve log metinleri kaybolmaz.
#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    color: u32,
}

static mut LOG_LINES: alloc::vec::Vec<alloc::vec::Vec<Cell>> = alloc::vec::Vec::new();
/// İmlecin bulunduğu (son) satırdaki sütun.
static mut LOG_COL: usize = 0;
/// Transkriptte saklanan toplam hücre sayısı (bellek sınırı için).
static mut LOG_CELLS: usize = 0;
/// Görünen en alt satırdan kaç satır yukarıda olunduğu (0 = en altta).
static mut VIEW_OFFSET: usize = 0;

const LINE_H: usize = 14;
const CHAR_W: usize = 8; // 8x8 font * 1x scale
/// Font yüksekliği (satır yüksekliğinden kısa; aradaki boşluk satırları ayırır).
const GLYPH_H: usize = 8;
const FONT_SCALE: usize = 1;
const LINE_PAUSE_MS: u64 = 15;
/// Metin alanının sol/sağ kenar boşluğu ve başlık satırının altı.
const TEXT_LEFT: usize = 16;
const TEXT_TOP: usize = 40;
/// Konsol arka planı; karakter hücreleri de bu renkle temizlenir (komşuya taşmaz).
const CONSOLE_BG: u32 = 0x080D16;
/// Transkriptte saklanacak en fazla satır ve hücre sayısı (aşarsa en eskiler atılır).
const LOG_MAX_LINES: usize = 4096;
const LOG_MAX_CELLS: usize = 262_144;

/// Basit 8x8 font bitmap (ASCII 32-126 için)
const FONT_8X8: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // Space
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // !
    [0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // "
    [0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x36], // #
    [0x0C, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x0C, 0x00], // $
    [0x00, 0x63, 0x33, 0x18, 0x0C, 0x26, 0x63, 0x00], // %
    [0x1C, 0x36, 0x1C, 0x0E, 0x3B, 0x33, 0x6E, 0x00], // &
    [0x06, 0x06, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00], // '
    [0x18, 0x0C, 0x06, 0x06, 0x06, 0x0C, 0x18, 0x00], // (
    [0x06, 0x0C, 0x18, 0x18, 0x18, 0x0C, 0x06, 0x00], // )
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // *
    [0x00, 0x0C, 0x0C, 0x3F, 0x0C, 0x0C, 0x00, 0x00], // +
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x06, 0x03], // ,
    [0x00, 0x00, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x00], // -
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00], // .
    [0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x01, 0x00], // /
    [0x3E, 0x63, 0x73, 0x7B, 0x6F, 0x67, 0x3E, 0x00], // 0
    [0x0C, 0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x3F, 0x00], // 1
    [0x1E, 0x33, 0x30, 0x1C, 0x06, 0x33, 0x3F, 0x00], // 2
    [0x1E, 0x33, 0x30, 0x1C, 0x30, 0x33, 0x1E, 0x00], // 3
    [0x38, 0x3C, 0x36, 0x33, 0x3F, 0x30, 0x78, 0x00], // 4
    [0x3F, 0x03, 0x1F, 0x30, 0x30, 0x33, 0x1E, 0x00], // 5
    [0x1C, 0x06, 0x03, 0x1F, 0x33, 0x33, 0x1E, 0x00], // 6
    [0x3F, 0x33, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x00], // 7
    [0x1E, 0x33, 0x33, 0x1E, 0x33, 0x33, 0x1E, 0x00], // 8
    [0x1E, 0x33, 0x33, 0x3E, 0x30, 0x18, 0x0E, 0x00], // 9
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x00], // :
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x06, 0x03], // ;
    [0x18, 0x0C, 0x06, 0x03, 0x06, 0x0C, 0x18, 0x00], // <
    [0x00, 0x00, 0x3F, 0x00, 0x00, 0x3F, 0x00, 0x00], // =
    [0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00], // >
    [0x1E, 0x33, 0x30, 0x18, 0x0C, 0x00, 0x0C, 0x00], // ?
    [0x3E, 0x63, 0x7B, 0x7B, 0x7B, 0x03, 0x1E, 0x00], // @
    [0x0C, 0x1E, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x00], // A
    [0x3F, 0x66, 0x66, 0x3E, 0x66, 0x66, 0x3F, 0x00], // B
    [0x3C, 0x66, 0x03, 0x03, 0x03, 0x66, 0x3C, 0x00], // C
    [0x1F, 0x36, 0x66, 0x66, 0x66, 0x36, 0x1F, 0x00], // D
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x46, 0x7F, 0x00], // E
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x06, 0x0F, 0x00], // F
    [0x3C, 0x66, 0x03, 0x03, 0x73, 0x66, 0x7C, 0x00], // G
    [0x33, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x33, 0x00], // H
    [0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // I
    [0x78, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E, 0x00], // J
    [0x67, 0x66, 0x36, 0x1E, 0x36, 0x66, 0x67, 0x00], // K
    [0x0F, 0x06, 0x06, 0x06, 0x46, 0x66, 0x7F, 0x00], // L
    [0x63, 0x77, 0x7F, 0x7F, 0x6B, 0x63, 0x63, 0x00], // M
    [0x63, 0x67, 0x6F, 0x7B, 0x73, 0x63, 0x63, 0x00], // N
    [0x1C, 0x36, 0x63, 0x63, 0x63, 0x36, 0x1C, 0x00], // O
    [0x3F, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x0F, 0x00], // P
    [0x1E, 0x33, 0x33, 0x33, 0x3B, 0x1E, 0x38, 0x00], // Q
    [0x3F, 0x66, 0x66, 0x3E, 0x36, 0x66, 0x67, 0x00], // R
    [0x1E, 0x33, 0x07, 0x0E, 0x38, 0x33, 0x1E, 0x00], // S
    [0x3F, 0x2D, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // T
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00], // U
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // V
    [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00], // W
    [0x63, 0x63, 0x36, 0x1C, 0x1C, 0x36, 0x63, 0x00], // X
    [0x33, 0x33, 0x33, 0x1E, 0x0C, 0x0C, 0x1E, 0x00], // Y
    [0x7F, 0x63, 0x31, 0x18, 0x4C, 0x66, 0x7F, 0x00], // Z
    [0x3E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x3E, 0x00], // [
    [0x00, 0x00, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x00], // backslash
    [0x3E, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3E, 0x00], // ]
    [0x08, 0x1C, 0x36, 0x63, 0x00, 0x00, 0x00, 0x00], // ^
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // _
    [0x0E, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00], // `
    [0x00, 0x00, 0x1E, 0x30, 0x3E, 0x33, 0x6E, 0x00], // a
    [0x07, 0x06, 0x06, 0x3E, 0x66, 0x66, 0x3B, 0x00], // b
    [0x00, 0x00, 0x1E, 0x33, 0x03, 0x33, 0x1E, 0x00], // c
    [0x38, 0x30, 0x30, 0x3e, 0x33, 0x33, 0x6E, 0x00], // d
    [0x00, 0x00, 0x1E, 0x33, 0x3F, 0x03, 0x1E, 0x00], // e
    [0x1C, 0x36, 0x06, 0x0F, 0x06, 0x06, 0x0F, 0x00], // f
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x1F], // g
    [0x07, 0x06, 0x36, 0x6E, 0x66, 0x66, 0x67, 0x00], // h
    [0x0C, 0x00, 0x0E, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // i
    [0x30, 0x00, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E], // j
    [0x07, 0x06, 0x66, 0x36, 0x1E, 0x36, 0x67, 0x00], // k
    [0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // l
    [0x00, 0x00, 0x33, 0x7F, 0x7F, 0x6B, 0x63, 0x00], // m
    [0x00, 0x00, 0x1F, 0x33, 0x33, 0x33, 0x33, 0x00], // n
    [0x00, 0x00, 0x1E, 0x33, 0x33, 0x33, 0x1E, 0x00], // o
    [0x00, 0x00, 0x3B, 0x66, 0x66, 0x3E, 0x06, 0x0F], // p
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x78], // q
    [0x00, 0x00, 0x3B, 0x6E, 0x66, 0x06, 0x0F, 0x00], // r
    [0x00, 0x00, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x00], // s
    [0x08, 0x0C, 0x3E, 0x0C, 0x0C, 0x2C, 0x18, 0x00], // t
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00], // u
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // v
    [0x00, 0x00, 0x63, 0x6B, 0x7F, 0x7F, 0x36, 0x00], // w
    [0x00, 0x00, 0x63, 0x36, 0x1C, 0x36, 0x63, 0x00], // x
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x3E, 0x30, 0x1F], // y
    [0x00, 0x00, 0x3F, 0x19, 0x0C, 0x26, 0x3F, 0x00], // z
    [0x38, 0x0C, 0x0C, 0x07, 0x0C, 0x0C, 0x38, 0x00], // {
    [0x00, 0x00, 0x0C, 0x0C, 0x3F, 0x0C, 0x0C, 0x00], // |
    [0x07, 0x0C, 0x0C, 0x38, 0x0C, 0x0C, 0x07, 0x00], // }
    [0x6C, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // ~
];

// Turkish letters used by the boot transcript (same 8x8, row-major format).
fn turkish_glyph(ch: char) -> Option<[u8; 8]> {
    Some(match ch {
        'Ç' => [0x1C, 0x36, 0x03, 0x03, 0x03, 0x36, 0x1C, 0x0C],
        'ç' => [0x00, 0x1E, 0x33, 0x03, 0x33, 0x1E, 0x0C, 0x18],
        'Ğ' => [0x3C, 0x66, 0x03, 0x03, 0x73, 0x66, 0x7C, 0x18],
        'ğ' => [0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x1F, 0x18],
        'İ' => [0x0C, 0x00, 0x1E, 0x0C, 0x0C, 0x0C, 0x1E, 0x00],
        'ı' => [0x00, 0x00, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x00],
        'Ö' => [0x36, 0x00, 0x1C, 0x36, 0x63, 0x63, 0x36, 0x1C],
        'ö' => [0x36, 0x00, 0x1E, 0x33, 0x33, 0x33, 0x1E, 0x00],
        'Ş' => [0x1E, 0x33, 0x07, 0x0E, 0x38, 0x33, 0x1E, 0x0C],
        'ş' => [0x00, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x0C, 0x18],
        'Ü' => [0x36, 0x00, 0x33, 0x33, 0x33, 0x33, 0x33, 0x3F],
        'ü' => [0x36, 0x00, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00],
        _ => return None,
    })
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Metin alanına sığan sütun sayısı.
fn text_cols(width: usize) -> usize {
    let usable = width.saturating_sub(TEXT_LEFT * 2);
    (usable / CHAR_W).max(8)
}

/// Metin alanına sığan satır sayısı.
fn text_rows(height: usize) -> usize {
    let usable = height.saturating_sub(TEXT_TOP);
    (usable / LINE_H).max(1)
}

/// Transkriptteki satırların ekranda kaç satır görüneceğini hesaplar (sarma dahil).
fn visual_line_count(cols: usize) -> usize {
    let lines = unsafe { &*core::ptr::addr_of!(LOG_LINES) };
    let mut total = 0usize;
    for line in lines.iter() {
        total += line.len().div_ceil(cols).max(1);
    }
    total
}

/// Transkripte yeni hücre yazar ve imleç sütununu ilerletir.
fn model_push_cell(ch: char, color: u32) {
    let lines = unsafe { &mut *core::ptr::addr_of_mut!(LOG_LINES) };
    if lines.is_empty() {
        lines.push(alloc::vec::Vec::new());
    }
    let col = unsafe { LOG_COL };
    let line = lines.last_mut().expect("konsol satiri yok");
    let grew = if col < line.len() {
        line[col] = Cell { ch, color };
        0
    } else {
        // Aradaki boşluklar boş hücre ile doldurulur (imleç atlama durumu).
        let fillers = col - line.len();
        for _ in 0..fillers {
            line.push(Cell { ch: ' ', color });
        }
        line.push(Cell { ch, color });
        fillers + 1
    };
    if grew > 0 {
        unsafe {
            LOG_CELLS += grew;
        }
    }
    unsafe {
        LOG_COL = col + 1;
    }
    model_trim();
}

/// Transkriptte yeni satır açar.
fn model_newline() {
    let lines = unsafe { &mut *core::ptr::addr_of_mut!(LOG_LINES) };
    lines.push(alloc::vec::Vec::new());
    unsafe {
        LOG_COL = 0;
    }
    model_trim();
}

/// İmleci bir sütun geri alır ve o hücreyi siler (backspace).
fn model_backspace() {
    let col = unsafe { LOG_COL };
    if col == 0 {
        return;
    }
    let new_col = col - 1;
    let lines = unsafe { &mut *core::ptr::addr_of_mut!(LOG_LINES) };
    if let Some(line) = lines.last_mut() {
        if line.len() > new_col {
            let removed = line.len() - new_col;
            line.truncate(new_col);
            unsafe {
                LOG_CELLS -= removed;
            }
        }
    }
    unsafe {
        LOG_COL = new_col;
    }
}

/// Sekme genişliğini doldurarak sütun ilerletir; ilerlenen sütun sayısını
/// döndürür. Sekme, ekrandaki görünür satırda 4 sütunluk adımlarla ilerler.
fn model_tab(cols: usize) -> usize {
    let col = unsafe { LOG_COL };
    let visual = col % cols.max(1);
    let mut advance = 4 - (visual % 4);
    if visual + advance > cols {
        // Satır sonuna kadar doldur; sonraki karakter yeni satırda başlar.
        advance = cols.saturating_sub(visual);
    }
    for _ in 0..advance {
        model_push_cell(' ', 0xD8E1EE);
    }
    advance
}

/// İmleç sütununu doğrudan atar (satır sonu / carriage return).
fn model_set_col(col: usize) {
    unsafe {
        LOG_COL = col;
    }
}

/// Transkriptin satır/hücre sınırını uygular; en eski satırlar atılır.
fn model_trim() {
    let lines = unsafe { &mut *core::ptr::addr_of_mut!(LOG_LINES) };
    while lines.len() > LOG_MAX_LINES && !lines.is_empty() {
        let removed = lines.remove(0);
        unsafe {
            LOG_CELLS -= removed.len();
        }
    }
    while unsafe { LOG_CELLS } > LOG_MAX_CELLS && !lines.is_empty() {
        let removed = lines.remove(0);
        unsafe {
            LOG_CELLS -= removed.len();
        }
    }
}

/// Mevcut ekran durumunu kaydet (GUI'den boot mesajlarına geçiş için)
///
/// # Safety
/// `FB` geçerli bir framebuffer içermeli veya `None` olmalı; fonksiyon yalnızca
/// framebuffer pointer'ı üzerinden volatile okuma yapar.
pub unsafe fn save_screen() {
    if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
        let len = screen.width.saturating_mul(screen.height);
        if len <= crate::drivers::video::BACKBUFFER_LEN {
            let pitch_words = screen.pitch / 4;
            for y in 0..screen.height {
                for x in 0..screen.width {
                    let packed = y * screen.width + x;
                    let visible = y * pitch_words + x;
                    SAVED_SCREEN_BUF[packed] =
                        core::ptr::read_volatile(screen.address.add(visible));
                }
            }
            SAVED_SCREEN_DIM = Some((screen.width, screen.height));
        }
    }
}

/// Kaydedilen ekran durumunu geri yükle (boot mesajlarından GUI'ye dönüş için)
///
/// # Safety
/// `FB` geçerli bir framebuffer içermeli veya `None` olmalı; fonksiyon yalnızca
/// framebuffer pointer'ı üzerinden volatile yazma yapar.
pub unsafe fn restore_screen() {
    if let Some((saved_w, saved_h)) = SAVED_SCREEN_DIM {
        if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
            let len = screen.width.saturating_mul(screen.height);
            if len <= crate::drivers::video::BACKBUFFER_LEN
                && screen.width == saved_w
                && screen.height == saved_h
            {
                let pitch_words = screen.pitch / 4;
                for y in 0..screen.height {
                    for x in 0..screen.width {
                        let packed = y * screen.width + x;
                        let visible = y * pitch_words + x;
                        core::ptr::write_volatile(
                            screen.address.add(visible),
                            SAVED_SCREEN_BUF[packed],
                        );
                    }
                }
                // Arka tamponu da geri yükle: sonraki `flush` boot logunu öne getirmesin.
                let saved = core::ptr::addr_of!(SAVED_SCREEN_BUF) as *const u32;
                screen.copy_backbuffer_from_raw(saved, len);
            }
        }
    }
}

/// Keep the boot console framebuffer in sync when the main GUI changes video mode.
///
/// # Safety
/// `fb` geçerli ve ömrü boyunca mapped kalacak bir framebuffer olmalıdır.
pub unsafe fn update_framebuffer(fb: Framebuffer) {
    FB = Some(fb);
    SAVED_SCREEN_DIM = None;
    CURSOR_X = TEXT_LEFT;
    CURSOR_Y = TEXT_TOP;
}

/// Text console'u tekrar aktif hale getir (welcome ekranından boot mesajlarına dönüş için)
///
/// # Safety
/// `FB` geçerli bir framebuffer içermeli veya `None` olmalı.
pub unsafe fn toggle_active() {
    if ACTIVE.load(Ordering::Acquire) {
        ACTIVE.store(false, Ordering::Release);
    } else {
        show_boot_log();
    }
}

/// Konsol başlığını çizer (metin alanının üstündeki sabit başlık satırı).
fn draw_title(screen: &mut Framebuffer) {
    let title = "minOs UEFI BOOT";
    let mut tx = TEXT_LEFT;
    for ch in title.chars() {
        draw_char(screen, tx, 12, ch, 0x62D6A8);
        tx += CHAR_W;
    }
}

/// Tek bir karakter hücresini çizer. Hücre tam olarak kendi kutusuna
/// sığdırılır; komşu hücrelerin üzerine taşma olmaz.
fn draw_cell(screen: &mut Framebuffer, x: usize, y: usize, cell: Cell) {
    screen.draw_rect(x, y, CHAR_W, GLYPH_H, CONSOLE_BG);
    draw_char(screen, x, y, cell.ch, cell.color);
}

/// Kaydırma durumunu gösteren başlık satırı ipucu (görünüm yukarıdayken).
fn draw_scroll_hint(screen: &mut Framebuffer, offset: usize) {
    if offset == 0 {
        return;
    }
    let hint = alloc::format!("[{offset} satir yukarida - PageUp/PageDown ile kaydirin]");
    let width = hint.chars().count() * CHAR_W;
    let mut tx = screen.width.saturating_sub(width + TEXT_LEFT);
    let y = 12;
    // Önce eski ipucunun kaldığı alanı temizle (başlık solda kalır).
    let clear_x = screen.width / 2;
    screen.draw_rect(
        clear_x,
        y - 1,
        screen.width - clear_x,
        GLYPH_H + 2,
        CONSOLE_BG,
    );
    for ch in hint.chars() {
        draw_char(screen, tx, y, ch, 0x4DABF7);
        tx += CHAR_W;
    }
}

/// Transkriptten görünüm alanını yeniden çizer. Ekranda görünecek satırlar
/// transkriptin son `text_rows` satırıdır; `VIEW_OFFSET` kadar satır yukarıda
/// bakılır. Bu sayede kaydırılan log metinleri kaybolmaz.
///
/// # Safety
/// `screen` geçerli bir framebuffer olmalıdır; konsol durumu (`VIEW_OFFSET`,
/// `CURSOR_*`) bu çağrı sırasında yalnızca konsol kilidi altında değiştirilmelidir.
unsafe fn repaint_view(screen: &mut Framebuffer) {
    let cols = text_cols(screen.width);
    let rows = text_rows(screen.height);
    let total = visual_line_count(cols);
    let max_offset = total.saturating_sub(1);
    if VIEW_OFFSET > max_offset {
        VIEW_OFFSET = max_offset;
    }
    let start = total.saturating_sub(rows.saturating_add(VIEW_OFFSET));

    // Metin alanını temizle.
    screen.draw_rect(
        0,
        TEXT_TOP,
        screen.width,
        screen.height.saturating_sub(TEXT_TOP),
        CONSOLE_BG,
    );

    let lines = unsafe { &*core::ptr::addr_of!(LOG_LINES) };
    let mut vidx = 0usize;
    for line in lines.iter() {
        let chunks = line.len().div_ceil(cols).max(1);
        for chunk in 0..chunks {
            if vidx >= start && vidx < start + rows {
                let row = vidx - start;
                let from = chunk * cols;
                let to = (from + cols).min(line.len());
                let y = TEXT_TOP + row * LINE_H;
                for (i, cell) in line[from..to].iter().enumerate() {
                    draw_cell(screen, TEXT_LEFT + i * CHAR_W, y, *cell);
                }
            }
            vidx += 1;
        }
    }

    // İmleci transkriptteki konumuna taşı.
    let last_len = lines.last().map_or(0, |l| l.len());
    let last_chunks = last_len.div_ceil(cols).max(1);
    let before_last = total.saturating_sub(last_chunks);
    let col = unsafe { LOG_COL };
    let cursor_vidx = before_last + (col / cols);
    CURSOR_X = TEXT_LEFT + (col % cols) * CHAR_W;
    CURSOR_Y = TEXT_TOP + cursor_vidx.saturating_sub(start) * LINE_H;

    draw_scroll_hint(screen, VIEW_OFFSET);
    screen.flush();
}

/// Konsol alanını transkriptten yeniden çizer (kaydırma, ekran değişimi veya
/// üstüne çizilen katman kaldırıldığında kullanılır).
pub fn repaint() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let _guard = TEXT_LOCK.lock();
    unsafe {
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
            return;
        };
        repaint_view(screen);
    }
}

/// Görünümü `delta` satır kaydırır (negatif: yukarı, pozitif: aşağı).
pub fn scroll_lines(delta: isize) {
    let _guard = TEXT_LOCK.lock();
    unsafe {
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
            return;
        };
        let cols = text_cols(screen.width);
        let total = visual_line_count(cols);
        let max_offset = total.saturating_sub(1);
        let current = VIEW_OFFSET;
        let target = if delta < 0 {
            current.saturating_add(delta.unsigned_abs())
        } else {
            current.saturating_sub(delta as usize)
        };
        let target = target.min(max_offset);
        if target == current {
            return;
        }
        VIEW_OFFSET = target;
        repaint_view(screen);
    }
}

/// Görünümü bir sayfa (görünür satır sayısı kadar) yukarı kaydırır.
pub fn page_up() {
    let page = unsafe {
        let Some(screen) = (&*core::ptr::addr_of!(FB)).as_ref() else {
            return;
        };
        text_rows(screen.height)
    };
    scroll_lines(-(page.max(1) as isize));
}

/// Görünümü bir sayfa aşağı kaydırır.
pub fn page_down() {
    let page = unsafe {
        let Some(screen) = (&*core::ptr::addr_of!(FB)).as_ref() else {
            return;
        };
        text_rows(screen.height)
    };
    scroll_lines(page.max(1) as isize);
}

/// Görünümü en eski satıra (transkriptin başına) kaydırır.
pub fn scroll_to_top() {
    let _guard = TEXT_LOCK.lock();
    unsafe {
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
            return;
        };
        let cols = text_cols(screen.width);
        let total = visual_line_count(cols);
        let target = total.saturating_sub(1);
        if VIEW_OFFSET == target {
            return;
        }
        VIEW_OFFSET = target;
        repaint_view(screen);
    }
}

/// Görünümü en alt satıra (canlı akış) döndürür.
pub fn scroll_to_bottom() {
    let _guard = TEXT_LOCK.lock();
    unsafe {
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
            return;
        };
        if VIEW_OFFSET == 0 {
            return;
        }
        VIEW_OFFSET = 0;
        repaint_view(screen);
    }
}

/// Klavye eylemlerini konsol kaydırmasına yönlendirir; konsol kaydırmasını
/// ele alıyorsa `true` döner.
pub fn handle_key_action(action: KeyAction) -> bool {
    match action {
        KeyAction::PageUpKey => {
            page_up();
            true
        }
        KeyAction::PageDownKey => {
            page_down();
            true
        }
        KeyAction::ArrowUp => {
            scroll_lines(-1);
            true
        }
        KeyAction::ArrowDown => {
            scroll_lines(1);
            true
        }
        KeyAction::HomeKey => {
            scroll_to_top();
            true
        }
        KeyAction::EndKey => {
            scroll_to_bottom();
            true
        }
        _ => false,
    }
}

/// Klavye eylemini global klavye kuyruğuna ekler (doğrudan port yoklaması için).
fn push_scroll_action(action: KeyAction) {
    unsafe {
        let kbd = &mut *core::ptr::addr_of_mut!(crate::GLOBAL_KEYBOARD);
        kbd.push_action(action);
    }
}

/// Klavye kuyruğundaki kaydırma tuşlarını konsola uygular; bir kaydırma
/// yapıldıysa `true` döner.
fn poll_scroll_action() -> bool {
    let mut handled = false;
    unsafe {
        let kbd = &mut *core::ptr::addr_of_mut!(crate::GLOBAL_KEYBOARD);
        while let Some(action) = kbd.take_last_action() {
            if handle_key_action(action) {
                handled = true;
            }
        }
    }
    handled
}

/// Force the buffered boot transcript onto the framebuffer, including after GUI output.
///
/// # Safety
/// `FB` geçerli bir framebuffer içermeli veya `None` olmalı; `None` ise fonksiyon
/// erken döner.
pub unsafe fn show_boot_log() {
    let _guard = TEXT_LOCK.lock();
    let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
        return;
    };
    screen.clear(CONSOLE_BG);
    draw_title(screen);
    VIEW_OFFSET = 0;
    repaint_view(screen);
    ACTIVE.store(true, Ordering::Release);
}

/// Basit bitmap font ile karakter çiz (2x ölçekli)
fn draw_char(screen: &mut Framebuffer, x: usize, y: usize, ch: char, color: u32) {
    let font_data = if let Some(glyph) = turkish_glyph(ch) {
        glyph
    } else if (ch as u32) >= 32 && (ch as u32) < 127 {
        FONT_8X8[(ch as u8 - 32) as usize]
    } else {
        FONT_8X8[(b'?' - 32) as usize]
    };

    for (row, &row_data) in font_data.iter().enumerate() {
        for col in 0..8 {
            if (row_data >> (7 - col)) & 1 == 1 {
                // Yatay ayna (horizontal mirror): x ekseninde sütunları tersine çevir
                let mirror_col = 7 - col;
                for sy in 0..FONT_SCALE {
                    for sx in 0..FONT_SCALE {
                        screen.draw_rect(
                            x + mirror_col * FONT_SCALE + sx,
                            y + row * FONT_SCALE + sy,
                            1,
                            1,
                            color,
                        );
                    }
                }
            }
        }
    }
}

/// Install a validated framebuffer as a text console before lengthy init work.
///
/// # Safety
/// `fb` geçerli ve ömrü boyunca mapped kalacak bir framebuffer olmalıdır.
pub unsafe fn init(fb: Framebuffer) {
    FB = Some(fb);
    CURSOR_X = TEXT_LEFT;
    CURSOR_Y = TEXT_TOP;
    if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
        screen.clear(CONSOLE_BG);
        // Basit bitmap font ile başlık çiz
        draw_title(screen);
        CURSOR_Y = TEXT_TOP;
        screen.flush();
        ACTIVE.store(true, Ordering::Release);
    }
}

/// Renkli metin çıktısı verir (hata, uyarı ve komut satırı için).
///
/// Metin önce konsol transkriptine yazılır (ekrandan kaybolsa da saklanır),
/// ardından etkin konsol varsa ekrana çizilir.
pub fn write_color(text: &str, color: u32) {
    let _guard = TEXT_LOCK.lock();
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else {
            return;
        };
        write_color_locked(screen, text, color);
    }
}

/// Konsol ekranına yazma (kilit zaten alınmış durumda).
unsafe fn write_color_locked(screen: &mut Framebuffer, text: &str, color: u32) {
    let cols = text_cols(screen.width);
    let right_edge = TEXT_LEFT + cols * CHAR_W;
    // Görünüm yukarı kaydırılmışsa ekrana dokunulmaz; metin yalnızca
    // transkripte yazılır, kullanıcı PageDown ile tekrar aşağı gelir.
    let follow = VIEW_OFFSET == 0;

    // Yeniden çizimden sonra imleç görünüm alanının altında kalmış olabilir;
    // bu durumda içeriği bir satır yukarı kaydır.
    if follow && CURSOR_Y + LINE_H > screen.height {
        scroll_one_line(screen);
    }

    for ch in text.chars() {
        match ch {
            '\r' => {
                model_set_col(0);
                if follow {
                    CURSOR_X = TEXT_LEFT;
                }
            }
            '\n' => {
                model_newline();
                if follow {
                    CURSOR_X = TEXT_LEFT;
                    CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
                    if CURSOR_Y + LINE_H > screen.height {
                        // Alt satır doldu: metin alanını bir satır yukarı kaydır.
                        scroll_one_line(screen);
                    }
                    screen.flush();
                    if !IN_ERROR_PROMPT.load(Ordering::Relaxed) && crate::boot::time::is_ready() {
                        let started = crate::boot::time::monotonic_ms();
                        while crate::boot::time::monotonic_ms().wrapping_sub(started)
                            < LINE_PAUSE_MS
                        {
                            core::hint::spin_loop();
                        }
                    }
                }
            }
            '\t' => {
                let advance = model_tab(cols);
                if follow {
                    CURSOR_X = CURSOR_X.saturating_add(advance * CHAR_W);
                    if CURSOR_X + CHAR_W > right_edge {
                        CURSOR_X = TEXT_LEFT;
                        CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
                        if CURSOR_Y + LINE_H > screen.height {
                            scroll_one_line(screen);
                        }
                    }
                }
            }
            '\x08' => {
                // Backspace karakteri: önceki karakteri ekranda ve transkriptte temizle
                model_backspace();
                if follow && CURSOR_X >= TEXT_LEFT + CHAR_W {
                    CURSOR_X -= CHAR_W;
                    screen.draw_rect(CURSOR_X, CURSOR_Y, CHAR_W, GLYPH_H, CONSOLE_BG);
                }
            }
            c if !c.is_control() => {
                model_push_cell(c, color);
                if follow {
                    draw_cell(screen, CURSOR_X, CURSOR_Y, Cell { ch: c, color });
                    CURSOR_X = CURSOR_X.saturating_add(CHAR_W);
                    if CURSOR_X + CHAR_W > right_edge {
                        CURSOR_X = TEXT_LEFT;
                        CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
                        if CURSOR_Y + LINE_H > screen.height {
                            scroll_one_line(screen);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if follow {
        screen.flush();
    }
}

pub fn write(text: &str) {
    write_color(text, 0xD8E1EE);
}

/// Metin alanını bir satır yukarı kaydırır.
///
/// Kaydırma **arka tampon** üzerinde yapılır: ham framebuffer'ı kaydırmak
/// sonraki `flush()` çağrısında üstüne yazılır ve log kaybolurdu.
unsafe fn scroll_one_line(screen: &mut Framebuffer) {
    let area_h = screen.height.saturating_sub(TEXT_TOP);
    if area_h <= LINE_H {
        return;
    }
    screen.scroll_region_up(0, TEXT_TOP, screen.width, area_h, LINE_H, CONSOLE_BG);
    // İmleç en alt satırda kalır (kaydırma sonrası da aynı satır).
    CURSOR_Y = TEXT_TOP + (area_h / LINE_H - 1) * LINE_H;
}

/// Polls for a single character from serial (COM1), GLOBAL_KEYBOARD or PS/2 port 0x60.
pub fn poll_input_char() -> Option<char> {
    // 1. Check COM1 serial line status (port 0x3FD bit 0 = Data Ready)
    unsafe {
        let mut lsr: u8;
        core::arch::asm!("in al, dx", in("dx") 0x3FDu16, out("al") lsr, options(nomem, nostack, preserves_flags));
        if lsr & 1 != 0 {
            let mut byte: u8;
            core::arch::asm!("in al, dx", in("dx") 0x3F8u16, out("al") byte, options(nomem, nostack, preserves_flags));
            if byte == b'\r' || byte == b'\n' {
                return Some('\n');
            }
            if byte == 0x7F || byte == 0x08 {
                return Some('\x08');
            }
            if (32..127).contains(&byte) {
                return Some(byte as char);
            }
        }
    }

    // 2. Check GLOBAL_KEYBOARD if initialized
    unsafe {
        let kbd = &mut *core::ptr::addr_of_mut!(crate::GLOBAL_KEYBOARD);
        if let Some(ch) = kbd.take_last_char() {
            return Some(ch);
        }
    }

    // 3. Direct PS/2 Keyboard polling (port 0x64 and 0x60)
    // Kesme modu etkinken baytlar INPUT_QUEUE'da birikir; önce kuyruğu
    // GLOBAL_KEYBOARD'a boşalt ki tuşlar kaybolmasın. Enter gibi eylem
    // üreten tuşlar da karakter akışına çevrilir.
    if crate::drivers::ps2::irq_mode() {
        unsafe {
            let kbd = &mut *core::ptr::addr_of_mut!(crate::GLOBAL_KEYBOARD);
            crate::drivers::ps2::drain_keyboard_events(kbd);
            if let Some(ch) = kbd.take_last_char() {
                return Some(ch);
            }
            while let Some(action) = kbd.take_last_action() {
                if matches!(action, crate::drivers::keyboard::KeyAction::EnterKey) {
                    return Some('\n');
                }
            }
        }
        return None;
    }
    unsafe {
        let mut status: u8;
        core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") status, options(nomem, nostack, preserves_flags));
        if status & 0x01 != 0 {
            let mut scancode: u8;
            core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") scancode, options(nomem, nostack, preserves_flags));
            // If bit 5 is 1, it's mouse data, skip it
            if status & 0x20 == 0 && (scancode & 0x80 == 0) {
                if scancode == 0xE0 {
                    // Genişletilmiş (E0) önek: sonraki bayt gezinme tuşudur.
                    let mut status2: u8;
                    core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") status2, options(nomem, nostack, preserves_flags));
                    if status2 & 0x01 != 0 {
                        let mut ext: u8;
                        core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") ext, options(nomem, nostack, preserves_flags));
                        if ext & 0x80 == 0 {
                            match ext {
                                0x48 => push_scroll_action(KeyAction::ArrowUp),
                                0x50 => push_scroll_action(KeyAction::ArrowDown),
                                0x49 => push_scroll_action(KeyAction::PageUpKey),
                                0x51 => push_scroll_action(KeyAction::PageDownKey),
                                _ => {}
                            }
                        }
                    }
                    return None;
                }
                // Key press make code
                return match scancode {
                    0x13 => Some('r'),
                    0x1C => Some('\n'),
                    0x1F => Some('s'),
                    0x2E => Some('c'),
                    0x23 => Some('h'),
                    0x0E => Some('\x08'), // Backspace
                    0x01 => Some('\x1b'), // Escape
                    0x39 => Some(' '),
                    0x10 => Some('q'),
                    0x11 => Some('w'),
                    0x12 => Some('e'),
                    0x14 => Some('t'),
                    0x15 => Some('y'),
                    0x16 => Some('u'),
                    0x17 => Some('i'),
                    0x18 => Some('o'),
                    0x19 => Some('p'),
                    0x1E => Some('a'),
                    0x20 => Some('d'),
                    0x21 => Some('f'),
                    0x22 => Some('g'),
                    0x24 => Some('j'),
                    0x25 => Some('k'),
                    0x26 => Some('l'),
                    0x2C => Some('z'),
                    0x2D => Some('x'),
                    0x2F => Some('v'),
                    0x30 => Some('b'),
                    0x31 => Some('n'),
                    0x32 => Some('m'),
                    0x02 => Some('1'),
                    0x03 => Some('2'),
                    0x04 => Some('3'),
                    0x05 => Some('4'),
                    0x06 => Some('5'),
                    0x07 => Some('6'),
                    0x08 => Some('7'),
                    0x09 => Some('8'),
                    0x0A => Some('9'),
                    0x0B => Some('0'),
                    _ => None,
                };
            }
        }
    }

    None
}

/// Shows a boot error or critical driver problem in text mode,
/// and presents an interactive command recovery prompt before GUI/Welcome screen.
pub fn interactive_error_prompt(message: &str) -> ! {
    IN_ERROR_PROMPT.store(true, Ordering::SeqCst);
    crate::serial::print("\n==================== [ BOOT ERROR ] ====================\n");
    crate::serial::print("[BOOT] HATA: ");
    crate::serial::print(message);
    crate::serial::print("\n=========================================================\n");

    unsafe {
        show_boot_log();
    }

    write("\n\n");
    write_color(
        "================================================================================\n",
        0xFF6B6B,
    );
    write_color("[minOs - BOOT ERROR / KRİTİK SÜRÜCÜ HATASI]\n", 0xFF6B6B);
    write_color(
        "================================================================================\n",
        0xFF6B6B,
    );
    write("Hata / Error: ");
    write_color(message, 0xFFD43B);
    write("\n\n");
    write_color(
        "Kritik başlatma veya sürücü hatası tespit edildi.\n",
        0xD8E1EE,
    );
    write_color(
        "Grafik arayüzü ve Hoş Geldin ekranı başlatılmadı (Halted before Welcome Screen).\n\n",
        0xFA5252,
    );
    write_color("Kullanilabilir Komutlar / Available Commands:\n", 0x62D6A8);
    write("  [Enter] / restart / r : Sistemi yeniden başlat (Restart system)\n");
    write("  halt / shutdown / h   : Sistemi kapat (Halt CPU)\n");
    write("  help                  : Komut listesini göster\n\n");
    write_color("Komut girin veya [Enter] tuşuna basın:\n", 0x4DABF7);
    write_color("recovery# ", 0x62D6A8);

    let mut cmd_buf = [0u8; 32];
    let mut cmd_len = 0usize;

    loop {
        if let Some(c) = poll_input_char() {
            if c == '\n' {
                write("\n");
                let cmd_str = core::str::from_utf8(&cmd_buf[..cmd_len])
                    .unwrap_or("")
                    .trim();
                if cmd_str.is_empty()
                    || cmd_str.eq_ignore_ascii_case("restart")
                    || cmd_str.eq_ignore_ascii_case("reboot")
                    || cmd_str.eq_ignore_ascii_case("r")
                {
                    write_color(
                        "[RESTART] Sistem yeniden başlatılıyor (Restarting)...\n",
                        0x62D6A8,
                    );
                    crate::serial::print(
                        "[BOOT] Komut alındı: restart. Sistem yeniden başlatılıyor...\n",
                    );
                    for _ in 0..200_000 {
                        core::hint::spin_loop();
                    }
                    crate::boot::console::restart_system();
                } else if cmd_str.eq_ignore_ascii_case("halt")
                    || cmd_str.eq_ignore_ascii_case("shutdown")
                    || cmd_str.eq_ignore_ascii_case("h")
                {
                    write_color(
                        "[HALT] Sistem güvenli şekilde durduruldu (Halted).\n",
                        0xFF6B6B,
                    );
                    crate::serial::print("[BOOT] Komut alındı: halt. Sistem durduruldu.\n");
                    unsafe {
                        core::arch::asm!("cli");
                        loop {
                            core::arch::asm!("hlt");
                        }
                    }
                } else if cmd_str.eq_ignore_ascii_case("help") || cmd_str == "?" {
                    write("Komutlar / Commands:\n");
                    write("  [Enter] / restart / r : Sistemi hemen yeniden başlatır\n");
                    write("  halt / shutdown / h   : Sistemi durdurur (CPU hlt)\n");
                    write("  help                  : Bu yardım mesajını gösterir\n");
                    write_color("recovery# ", 0x62D6A8);
                    cmd_len = 0;
                } else {
                    write("Bilinmeyen komut: '");
                    write(cmd_str);
                    write("'. Yeniden başlatmak için [Enter], yardım için 'help' yazın.\n");
                    write_color("recovery# ", 0x62D6A8);
                    cmd_len = 0;
                }
            } else if c == '\x08' {
                // Backspace
                if cmd_len > 0 {
                    cmd_len -= 1;
                    write("\x08");
                }
            } else if !c.is_control() && cmd_len < cmd_buf.len() {
                cmd_buf[cmd_len] = c as u8;
                cmd_len += 1;
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                write(s);
            }
        }
        // PageUp/PageDown ile boot logunun eski satırlarına geri bakılabilir.
        poll_scroll_action();
        core::hint::spin_loop();
    }
}

/// Show a fatal boot message using the text console, then run the interactive recovery prompt.
pub fn fatal(message: &str) -> ! {
    crate::serial::print("\n[FATAL] ");
    crate::serial::print(message);
    crate::serial::print("\n[BOOT] Kritik hata: grafik arayüzü başlatılmadı.\n");
    interactive_error_prompt(message);
}

/// Clear graphics output after every visible boot check has passed.
pub fn disable() {
    ACTIVE.store(false, Ordering::Release);
}

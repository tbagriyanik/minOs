//! Early framebuffer text output for boot progress and fatal errors.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::drivers::video::Framebuffer;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static IN_ERROR_PROMPT: AtomicBool = AtomicBool::new(false);
static mut FB: Option<Framebuffer> = None;
static mut CURSOR_X: usize = 16;
static mut CURSOR_Y: usize = 16;
static mut SAVED_SCREEN_BUF: [u32; crate::drivers::video::BACKBUFFER_LEN] = [0; crate::drivers::video::BACKBUFFER_LEN];
static mut SAVED_SCREEN_DIM: Option<(usize, usize)> = None;
static mut BOOT_LOG_BUFFER: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
const LINE_H: usize = 14;
const CHAR_W: usize = 8; // 8x8 font * 1x scale
const FONT_SCALE: usize = 1;
const LOG_LINE_CAPACITY: usize = 4096;
const MAX_VISIBLE_LINES: usize = 80;
const LINE_PAUSE_MS: u64 = 15;

/// Basit 8x8 font bitmap (ASCII 32-126 için)
const FONT_8X8: [[u8; 8]; 95] = [
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00], // Space
    [0x18,0x3C,0x3C,0x18,0x18,0x00,0x18,0x00], // !
    [0x36,0x36,0x00,0x00,0x00,0x00,0x00,0x00], // "
    [0x36,0x36,0x7F,0x36,0x7F,0x36,0x36,0x36], // #
    [0x0C,0x3E,0x03,0x1E,0x30,0x1F,0x0C,0x00], // $
    [0x00,0x63,0x33,0x18,0x0C,0x26,0x63,0x00], // %
    [0x1C,0x36,0x1C,0x0E,0x3B,0x33,0x6E,0x00], // &
    [0x06,0x06,0x03,0x00,0x00,0x00,0x00,0x00], // '
    [0x18,0x0C,0x06,0x06,0x06,0x0C,0x18,0x00], // (
    [0x06,0x0C,0x18,0x18,0x18,0x0C,0x06,0x00], // )
    [0x00,0x66,0x3C,0xFF,0x3C,0x66,0x00,0x00], // *
    [0x00,0x0C,0x0C,0x3F,0x0C,0x0C,0x00,0x00], // +
    [0x00,0x00,0x00,0x00,0x00,0x0C,0x06,0x03], // ,
    [0x00,0x00,0x00,0x3F,0x00,0x00,0x00,0x00], // -
    [0x00,0x00,0x00,0x00,0x00,0x0C,0x0C,0x00], // .
    [0x60,0x30,0x18,0x0C,0x06,0x03,0x01,0x00], // /
    [0x3E,0x63,0x73,0x7B,0x6F,0x67,0x3E,0x00], // 0
    [0x0C,0x0E,0x0C,0x0C,0x0C,0x0C,0x3F,0x00], // 1
    [0x1E,0x33,0x30,0x1C,0x06,0x33,0x3F,0x00], // 2
    [0x1E,0x33,0x30,0x1C,0x30,0x33,0x1E,0x00], // 3
    [0x38,0x3C,0x36,0x33,0x3F,0x30,0x78,0x00], // 4
    [0x3F,0x03,0x1F,0x30,0x30,0x33,0x1E,0x00], // 5
    [0x1C,0x06,0x03,0x1F,0x33,0x33,0x1E,0x00], // 6
    [0x3F,0x33,0x30,0x18,0x0C,0x0C,0x0C,0x00], // 7
    [0x1E,0x33,0x33,0x1E,0x33,0x33,0x1E,0x00], // 8
    [0x1E,0x33,0x33,0x3E,0x30,0x18,0x0E,0x00], // 9
    [0x00,0x0C,0x0C,0x00,0x00,0x0C,0x0C,0x00], // :
    [0x00,0x0C,0x0C,0x00,0x00,0x0C,0x06,0x03], // ;
    [0x18,0x0C,0x06,0x03,0x06,0x0C,0x18,0x00], // <
    [0x00,0x00,0x3F,0x00,0x00,0x3F,0x00,0x00], // =
    [0x06,0x0C,0x18,0x30,0x18,0x0C,0x06,0x00], // >
    [0x1E,0x33,0x30,0x18,0x0C,0x00,0x0C,0x00], // ?
    [0x3E,0x63,0x7B,0x7B,0x7B,0x03,0x1E,0x00], // @
    [0x0C,0x1E,0x33,0x33,0x3F,0x33,0x33,0x00], // A
    [0x3F,0x66,0x66,0x3E,0x66,0x66,0x3F,0x00], // B
    [0x3C,0x66,0x03,0x03,0x03,0x66,0x3C,0x00], // C
    [0x1F,0x36,0x66,0x66,0x66,0x36,0x1F,0x00], // D
    [0x7F,0x46,0x16,0x1E,0x16,0x46,0x7F,0x00], // E
    [0x7F,0x46,0x16,0x1E,0x16,0x06,0x0F,0x00], // F
    [0x3C,0x66,0x03,0x03,0x73,0x66,0x7C,0x00], // G
    [0x33,0x33,0x33,0x3F,0x33,0x33,0x33,0x00], // H
    [0x1E,0x0C,0x0C,0x0C,0x0C,0x0C,0x1E,0x00], // I
    [0x78,0x30,0x30,0x30,0x33,0x33,0x1E,0x00], // J
    [0x67,0x66,0x36,0x1E,0x36,0x66,0x67,0x00], // K
    [0x0F,0x06,0x06,0x06,0x46,0x66,0x7F,0x00], // L
    [0x63,0x77,0x7F,0x7F,0x6B,0x63,0x63,0x00], // M
    [0x63,0x67,0x6F,0x7B,0x73,0x63,0x63,0x00], // N
    [0x1C,0x36,0x63,0x63,0x63,0x36,0x1C,0x00], // O
    [0x3F,0x66,0x66,0x3E,0x06,0x06,0x0F,0x00], // P
    [0x1E,0x33,0x33,0x33,0x3B,0x1E,0x38,0x00], // Q
    [0x3F,0x66,0x66,0x3E,0x36,0x66,0x67,0x00], // R
    [0x1E,0x33,0x07,0x0E,0x38,0x33,0x1E,0x00], // S
    [0x3F,0x2D,0x0C,0x0C,0x0C,0x0C,0x1E,0x00], // T
    [0x33,0x33,0x33,0x33,0x33,0x33,0x3F,0x00], // U
    [0x33,0x33,0x33,0x33,0x33,0x1E,0x0C,0x00], // V
    [0x63,0x63,0x63,0x6B,0x7F,0x77,0x63,0x00], // W
    [0x63,0x63,0x36,0x1C,0x1C,0x36,0x63,0x00], // X
    [0x33,0x33,0x33,0x1E,0x0C,0x0C,0x1E,0x00], // Y
    [0x7F,0x63,0x31,0x18,0x4C,0x66,0x7F,0x00], // Z
    [0x3E,0x06,0x06,0x06,0x06,0x06,0x3E,0x00], // [
    [0x00,0x00,0x00,0x3F,0x00,0x00,0x00,0x00], // backslash
    [0x3E,0x30,0x30,0x30,0x30,0x30,0x3E,0x00], // ]
    [0x08,0x1C,0x36,0x63,0x00,0x00,0x00,0x00], // ^
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xFF], // _
    [0x0E,0x18,0x18,0x30,0x00,0x00,0x00,0x00], // `
    [0x00,0x00,0x1E,0x30,0x3E,0x33,0x6E,0x00], // a
    [0x07,0x06,0x06,0x3E,0x66,0x66,0x3B,0x00], // b
    [0x00,0x00,0x1E,0x33,0x03,0x33,0x1E,0x00], // c
    [0x38,0x30,0x30,0x3e,0x33,0x33,0x6E,0x00], // d
    [0x00,0x00,0x1E,0x33,0x3F,0x03,0x1E,0x00], // e
    [0x1C,0x36,0x06,0x0F,0x06,0x06,0x0F,0x00], // f
    [0x00,0x00,0x6E,0x33,0x33,0x3E,0x30,0x1F], // g
    [0x07,0x06,0x36,0x6E,0x66,0x66,0x67,0x00], // h
    [0x0C,0x00,0x0E,0x0C,0x0C,0x0C,0x1E,0x00], // i
    [0x30,0x00,0x30,0x30,0x30,0x33,0x33,0x1E], // j
    [0x07,0x06,0x66,0x36,0x1E,0x36,0x67,0x00], // k
    [0x0E,0x0C,0x0C,0x0C,0x0C,0x0C,0x1E,0x00], // l
    [0x00,0x00,0x33,0x7F,0x7F,0x6B,0x63,0x00], // m
    [0x00,0x00,0x1F,0x33,0x33,0x33,0x33,0x00], // n
    [0x00,0x00,0x1E,0x33,0x33,0x33,0x1E,0x00], // o
    [0x00,0x00,0x3B,0x66,0x66,0x3E,0x06,0x0F], // p
    [0x00,0x00,0x6E,0x33,0x33,0x3E,0x30,0x78], // q
    [0x00,0x00,0x3B,0x6E,0x66,0x06,0x0F,0x00], // r
    [0x00,0x00,0x3E,0x03,0x1E,0x30,0x1F,0x00], // s
    [0x08,0x0C,0x3E,0x0C,0x0C,0x2C,0x18,0x00], // t
    [0x00,0x00,0x33,0x33,0x33,0x33,0x3F,0x00], // u
    [0x00,0x00,0x33,0x33,0x33,0x1E,0x0C,0x00], // v
    [0x00,0x00,0x63,0x6B,0x7F,0x7F,0x36,0x00], // w
    [0x00,0x00,0x63,0x36,0x1C,0x36,0x63,0x00], // x
    [0x00,0x00,0x33,0x33,0x33,0x3E,0x30,0x1F], // y
    [0x00,0x00,0x3F,0x19,0x0C,0x26,0x3F,0x00], // z
    [0x38,0x0C,0x0C,0x07,0x0C,0x0C,0x38,0x00], // {
    [0x00,0x00,0x0C,0x0C,0x3F,0x0C,0x0C,0x00], // |
    [0x07,0x0C,0x0C,0x38,0x0C,0x0C,0x07,0x00], // }
    [0x6C,0x36,0x00,0x00,0x00,0x00,0x00,0x00], // ~
];

// Turkish letters used by the boot transcript (same 8x8, row-major format).
fn turkish_glyph(ch: char) -> Option<[u8; 8]> {
    Some(match ch {
        'Ç' => [0x1C,0x36,0x03,0x03,0x03,0x36,0x1C,0x0C],
        'ç' => [0x00,0x1E,0x33,0x03,0x33,0x1E,0x0C,0x18],
        'Ğ' => [0x3C,0x66,0x03,0x03,0x73,0x66,0x7C,0x18],
        'ğ' => [0x00,0x6E,0x33,0x33,0x3E,0x30,0x1F,0x18],
        'İ' => [0x0C,0x00,0x1E,0x0C,0x0C,0x0C,0x1E,0x00],
        'ı' => [0x00,0x00,0x0C,0x0C,0x0C,0x0C,0x0C,0x00],
        'Ö' => [0x36,0x00,0x1C,0x36,0x63,0x63,0x36,0x1C],
        'ö' => [0x36,0x00,0x1E,0x33,0x33,0x33,0x1E,0x00],
        'Ş' => [0x1E,0x33,0x07,0x0E,0x38,0x33,0x1E,0x0C],
        'ş' => [0x00,0x3E,0x03,0x1E,0x30,0x1F,0x0C,0x18],
        'Ü' => [0x36,0x00,0x33,0x33,0x33,0x33,0x33,0x3F],
        'ü' => [0x36,0x00,0x33,0x33,0x33,0x33,0x3F,0x00],
        _ => return None,
    })
}

pub fn is_active() -> bool { ACTIVE.load(Ordering::Acquire) }

/// Mevcut ekran durumunu kaydet (GUI'den boot mesajlarına geçiş için)
pub unsafe fn save_screen() {
    if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
        let len = screen.width.checked_mul(screen.height).unwrap_or(usize::MAX);
        if len <= crate::drivers::video::BACKBUFFER_LEN {
            let pitch_words = screen.pitch / 4;
            for y in 0..screen.height {
                for x in 0..screen.width {
                    let packed = y * screen.width + x;
                    let visible = y * pitch_words + x;
                    SAVED_SCREEN_BUF[packed] = core::ptr::read_volatile(screen.address.add(visible));
                }
            }
            SAVED_SCREEN_DIM = Some((screen.width, screen.height));
        }
    }
}

/// Kaydedilen ekran durumunu geri yükle (boot mesajlarından GUI'ye dönüş için)
pub unsafe fn restore_screen() {
    if let Some((saved_w, saved_h)) = SAVED_SCREEN_DIM {
        if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
            let len = screen.width.checked_mul(screen.height).unwrap_or(usize::MAX);
            if len <= crate::drivers::video::BACKBUFFER_LEN && screen.width == saved_w && screen.height == saved_h {
                let pitch_words = screen.pitch / 4;
                for y in 0..screen.height {
                    for x in 0..screen.width {
                        let packed = y * screen.width + x;
                        let visible = y * pitch_words + x;
                        core::ptr::write_volatile(screen.address.add(visible), SAVED_SCREEN_BUF[packed]);
                    }
                }
            }
        }
    }
}

/// Keep the boot console framebuffer in sync when the main GUI changes video mode.
pub unsafe fn update_framebuffer(fb: Framebuffer) {
    FB = Some(fb);
    SAVED_SCREEN_DIM = None;
    CURSOR_X = 16;
    CURSOR_Y = 16;
}

/// Text console'u tekrar aktif hale getir (welcome ekranından boot mesajlarına dönüş için)
pub unsafe fn toggle_active() {
    if ACTIVE.load(Ordering::Acquire) {
        ACTIVE.store(false, Ordering::Release);
    } else {
        show_boot_log();
    }
}

/// Force the buffered boot transcript onto the framebuffer, including after GUI output.
pub unsafe fn show_boot_log() {
    let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else { return };
    screen.clear(0x080D16);
    CURSOR_X = 16;
    CURSOR_Y = 16;
    screen.flush();

    let log_slice = &*core::ptr::addr_of!(BOOT_LOG_BUFFER);
    let log_text = core::str::from_utf8(log_slice).unwrap_or("");
    for ch in log_text.chars() {
        match ch {
            '\r' => CURSOR_X = 16,
            '\n' => {
                CURSOR_X = 16;
                CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
            }
            '\t' => CURSOR_X = CURSOR_X.saturating_add(CHAR_W * 4),
            c if !c.is_control() => {
                screen.draw_rect(CURSOR_X.saturating_sub(1), CURSOR_Y.saturating_sub(1), CHAR_W + 2, LINE_H, 0x000000);
                draw_char(screen, CURSOR_X, CURSOR_Y, c, 0xD8E1EE);
                CURSOR_X = CURSOR_X.saturating_add(CHAR_W);
            }
            _ => {}
        }
        if CURSOR_X + CHAR_W >= screen.width {
            CURSOR_X = 16;
            CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
        }
        if CURSOR_Y + LINE_H >= screen.height {
            screen.draw_rect(0, 36, screen.width, screen.height.saturating_sub(36), 0x080D16);
            CURSOR_Y = 40;
        }
    }
    screen.flush();
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
        
        for row in 0..8 {
            let row_data = font_data[row];
            for col in 0..8 {
                if (row_data >> (7 - col)) & 1 == 1 {
                    // Yatay ayna (horizontal mirror): x ekseninde sütunları tersine çevir
                    let mirror_col = 7 - col;
                    for sy in 0..FONT_SCALE {
                        for sx in 0..FONT_SCALE {
                            screen.draw_rect(
                                x + mirror_col * FONT_SCALE + sx,
                                y + row * FONT_SCALE + sy,
                                1, 1, color
                            );
                        }
                    }
                }
            }
        }
}

/// Install a validated framebuffer as a text console before lengthy init work.
pub unsafe fn init(fb: Framebuffer) {
    FB = Some(fb);
    CURSOR_X = 16;
    CURSOR_Y = 16;
    if let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() {
        screen.clear(0x080D16);
        // Basit bitmap font ile başlık çiz
        let title = "minOs UEFI BOOT";
        let title_bytes = title.as_bytes();
        let mut tx = 16;
        for &b in title_bytes {
            draw_char(screen, tx, 12, b as char, 0x62D6A8);
            tx += CHAR_W;
        }
        CURSOR_Y = 40;
        screen.flush();
        ACTIVE.store(true, Ordering::Release);
    }
}

/// Buffer'a kaydet ama ekrana çizme (sonsuz döngü önleme)
fn write_to_buffer(_text: &str) {
    // Buffer growth and truncation are performed by write_color below.
}

/// Renkli metin çıktısı verir (hata, uyarı ve komut satırı için).
pub fn write_color(text: &str, color: u32) {
    write_to_buffer(text);
    if !ACTIVE.load(Ordering::Acquire) { return; }
    unsafe {
        let log = &mut *core::ptr::addr_of_mut!(BOOT_LOG_BUFFER);
        if log.len().saturating_add(text.len()) > LOG_LINE_CAPACITY {
            let drop_len = log.len().saturating_add(text.len()) - LOG_LINE_CAPACITY;
            let drop_len = log.iter().enumerate().skip(drop_len.min(log.len()))
                .find(|(_, b)| **b == b'\n').map(|(i, _)| i + 1)
                .unwrap_or(drop_len.min(log.len()));
            log.drain(..drop_len);
        }
        log.extend_from_slice(text.as_bytes());
        let Some(screen) = (&mut *core::ptr::addr_of_mut!(FB)).as_mut() else { return };
        for raw_ch in text.chars() {
            let ch = raw_ch;
            match ch {
                '\r' => CURSOR_X = 16,
                '\n' => {
                    CURSOR_X = 16;
                    CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
                    screen.flush();
                    if !IN_ERROR_PROMPT.load(Ordering::Relaxed) && crate::boot::time::is_ready() {
                        let started = crate::boot::time::monotonic_ms();
                        while crate::boot::time::monotonic_ms().wrapping_sub(started) < LINE_PAUSE_MS {
                            core::hint::spin_loop();
                        }
                    }
                }
                '\t' => CURSOR_X = CURSOR_X.saturating_add(CHAR_W * 4),
                '\x08' => {
                    // Backspace karakteri: önceki karakteri ekranda temizle
                    if CURSOR_X >= 16 + CHAR_W {
                        CURSOR_X -= CHAR_W;
                        screen.draw_rect(CURSOR_X.saturating_sub(1), CURSOR_Y.saturating_sub(1), CHAR_W + 2, LINE_H, 0x080D16);
                    }
                }
                c if !c.is_control() => {
                    screen.draw_rect(CURSOR_X.saturating_sub(1), CURSOR_Y.saturating_sub(1), CHAR_W + 2, LINE_H, 0x000000);
                    draw_char(screen, CURSOR_X, CURSOR_Y, c, color);
                    CURSOR_X = CURSOR_X.saturating_add(CHAR_W);
                }
                _ => {}
            }
            if CURSOR_X + CHAR_W >= screen.width {
                CURSOR_X = 16;
                CURSOR_Y = CURSOR_Y.saturating_add(LINE_H);
            }
            if CURSOR_Y + LINE_H >= screen.height { scroll_one_line(screen); }
        }
        screen.flush();
    }
}

pub fn write(text: &str) {
    write_color(text, 0xD8E1EE);
}

unsafe fn scroll_one_line(screen: &mut Framebuffer) {
    let row_words = screen.pitch / 4;
    let max_lines = (screen.height.saturating_sub(40) / LINE_H).min(MAX_VISIBLE_LINES);
    if max_lines == 0 { return; }
    let shift = LINE_H.min(screen.height.saturating_sub(40));
    for y in 40..screen.height.saturating_sub(shift) {
        for x in 0..row_words {
            let dst = y * row_words + x;
            let src = (y + shift) * row_words + x;
            let value = core::ptr::read_volatile(screen.address.add(src));
            core::ptr::write_volatile(screen.address.add(dst), value);
        }
    }
    let clear_y = screen.height.saturating_sub(shift);
    screen.draw_rect(0, clear_y, screen.width, shift, 0x080D16);
    CURSOR_Y = CURSOR_Y.saturating_sub(shift);
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
            if byte >= 32 && byte < 127 {
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
    unsafe {
        let mut status: u8;
        core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") status, options(nomem, nostack, preserves_flags));
        if status & 0x01 != 0 {
            let mut scancode: u8;
            core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") scancode, options(nomem, nostack, preserves_flags));
            // If bit 5 is 1, it's mouse data, skip it
            if status & 0x20 == 0 && (scancode & 0x80 == 0) {
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
    write_color("================================================================================\n", 0xFF6B6B);
    write_color("[minOs - BOOT ERROR / KRİTİK SÜRÜCÜ HATASI]\n", 0xFF6B6B);
    write_color("================================================================================\n", 0xFF6B6B);
    write("Hata / Error: ");
    write_color(message, 0xFFD43B);
    write("\n\n");
    write_color("Kritik başlatma veya sürücü hatası tespit edildi.\n", 0xD8E1EE);
    write_color("Grafik arayüzü ve Hoş Geldin ekranı başlatılmadı (Halted before Welcome Screen).\n\n", 0xFA5252);
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
                let cmd_str = core::str::from_utf8(&cmd_buf[..cmd_len]).unwrap_or("").trim();
                if cmd_str.is_empty() || cmd_str.eq_ignore_ascii_case("restart") || cmd_str.eq_ignore_ascii_case("reboot") || cmd_str.eq_ignore_ascii_case("r") {
                    write_color("[RESTART] Sistem yeniden başlatılıyor (Restarting)...\n", 0x62D6A8);
                    crate::serial::print("[BOOT] Komut alındı: restart. Sistem yeniden başlatılıyor...\n");
                    for _ in 0..200_000 { core::hint::spin_loop(); }
                    crate::boot::console::restart_system();
                } else if cmd_str.eq_ignore_ascii_case("halt") || cmd_str.eq_ignore_ascii_case("shutdown") || cmd_str.eq_ignore_ascii_case("h") {
                    write_color("[HALT] Sistem güvenli şekilde durduruldu (Halted).\n", 0xFF6B6B);
                    crate::serial::print("[BOOT] Komut alındı: halt. Sistem durduruldu.\n");
                    unsafe {
                        core::arch::asm!("cli");
                        loop { core::arch::asm!("hlt"); }
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

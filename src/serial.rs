use core::arch::asm;
extern crate alloc;

const COM1_DATA: u16 = 0x3F8;
const COM1_INT_EN: u16 = 0x3F9;
const COM1_FIFO: u16 = 0x3FA;
const COM1_LINE_CTL: u16 = 0x3FB;
const COM1_MODEM_CTL: u16 = 0x3FC;
const COM1_LINE_STATUS: u16 = 0x3FD;

static mut LOG_BYTES: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

const RED: &[u8] = b"\x1b[31m";
const YELLOW: &[u8] = b"\x1b[33m";
const RESET: &[u8] = b"\x1b[0m";

pub fn print(message: &str) {
    append_log(message.as_bytes());
    let color = if is_warning(message) {
        Some(YELLOW)
    } else if is_error(message) {
        Some(RED)
    } else {
        None
    };

    if let Some(color) = color {
        write_bytes(color);
    }
    write_bytes(message.as_bytes());
    if color.is_some() {
        write_bytes(RESET);
    }
}

#[allow(static_mut_refs)]
fn append_log(bytes: &[u8]) {
    unsafe {
        // Log storage grows with the transcript; no fixed 4 KiB/16 KiB cap.
        // try_reserve keeps logging failure from recursively panicking.
        if LOG_BYTES.try_reserve(bytes.len()).is_ok() {
            LOG_BYTES.extend_from_slice(bytes);
        }
    }
}

#[allow(static_mut_refs)]
pub fn log_snapshot() -> alloc::vec::Vec<u8> {
    unsafe { LOG_BYTES.clone() }
}

/// Copies the newest portion of the transcript without allocating. The caller
/// can render this tail while the boot screen is active.
#[allow(static_mut_refs)]
pub fn copy_log_tail(out: &mut [u8]) -> usize {
    unsafe {
        let len = LOG_BYTES.len().min(out.len());
        if len == 0 { return 0; }
        let start = LOG_BYTES.len() - len;
        out[..len].copy_from_slice(&LOG_BYTES[start..]);
        len
    }
}

pub fn print_dec(mut v: u64) {
    if v == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        print(s);
    }
}

pub fn print_hex(mut v: u64) {
    print("0x");
    if v == 0 {
        print("0");
        return;
    }
    let hex_chars = b"0123456789ABCDEF";
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = hex_chars[(v & 0xF) as usize];
        v >>= 4;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        print(s);
    }
}


fn is_warning(message: &str) -> bool {
    message.contains("UYARI") || message.contains("WARNING")
}

fn is_error(message: &str) -> bool {
    [
        "ERROR", "HATA", "PANIC", "failed", "Failed", "not found", "bulunamadi",
        "bulunamadı", "taninamadi", "tanınamadı", "basarisiz", "başarısız",
        "Olusturulamadi", "Silinemedi", "Yazilamadi",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

fn write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        write_byte(byte);
    }
}

/// COM1 (0x3F8) UART'ını hazırlar: 115200 baud, 8N1, FIFO açık.
///
/// Firmware (SeaBIOS/VBox BIOS) varsayılanlarına güvenilmez; özellikle
/// VirtualBox'ta bölücü farklı kalırsa tüm seri log bozuk okunur.
pub fn init() {
    unsafe {
        // Tüm UART kesmelerini kapat.
        asm!("out dx, al", in("dx") COM1_INT_EN, in("al") 0u8, options(nomem, nostack, preserves_flags));
        // DLAB = 1: bölücü yazmacı erişimi.
        asm!("out dx, al", in("dx") COM1_LINE_CTL, in("al") 0x80u8, options(nomem, nostack, preserves_flags));
        // Bölücü = 1 → 115200 baud (115200 / 1).
        asm!("out dx, al", in("dx") COM1_DATA, in("al") 1u8, options(nomem, nostack, preserves_flags));
        asm!("out dx, al", in("dx") COM1_INT_EN, in("al") 0u8, options(nomem, nostack, preserves_flags));
        // 8 veri biti, parity yok, 1 stop biti (8N1); DLAB = 0.
        asm!("out dx, al", in("dx") COM1_LINE_CTL, in("al") 0x03u8, options(nomem, nostack, preserves_flags));
        // FIFO aç + temizle, 14 bayt eşik.
        asm!("out dx, al", in("dx") COM1_FIFO, in("al") 0xC7u8, options(nomem, nostack, preserves_flags));
        // DTR + RTS + OUT2.
        asm!("out dx, al", in("dx") COM1_MODEM_CTL, in("al") 0x0Bu8, options(nomem, nostack, preserves_flags));
    }
}

/// Verici (THR) boşalana kadar bekleyip tek bayt yazar.
/// Beklemeden art arda `out` yapmak özellikle VirtualBox'ta bayt düşürür
/// (kesik/bozuk log). UART yanıt vermezse zaman aşımıyla vazgeçer.
pub fn write_byte(b: u8) {
    let mut spins = 0u32;
    loop {
        let status: u8;
        unsafe {
            asm!("in al, dx", in("dx") COM1_LINE_STATUS, out("al") status, options(nomem, nostack, preserves_flags));
        }
        if status & 0x20 != 0 {
            break;
        }
        spins += 1;
        if spins > 10_000_000 {
            break;
        }
        core::hint::spin_loop();
    }
    unsafe {
        asm!("out dx, al", in("dx") COM1_DATA, in("al") b, options(nomem, nostack, preserves_flags));
    }
}

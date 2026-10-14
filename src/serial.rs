use core::arch::asm;
extern crate alloc;

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
        unsafe {
            asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte, options(nomem, nostack, preserves_flags));
        }
    }
}

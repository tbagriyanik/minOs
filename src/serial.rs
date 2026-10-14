use core::arch::asm;

const RED: &[u8] = b"\x1b[31m";
const YELLOW: &[u8] = b"\x1b[33m";
const RESET: &[u8] = b"\x1b[0m";

pub fn print(message: &str) {
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
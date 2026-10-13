//! Uygulamalar arasında paylaşılan sabit boyutlu metin panosu.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub const CAPACITY: usize = 512;

static CLIPBOARD: [AtomicU32; CAPACITY] = [const { AtomicU32::new(' ' as u32) }; CAPACITY];
static CLIPBOARD_LEN: AtomicUsize = AtomicUsize::new(0);

pub fn set_chars(text: &[char]) {
    let len = text.len().min(CAPACITY);
    for (slot, ch) in CLIPBOARD.iter().zip(text.iter()).take(len) {
        slot.store(*ch as u32, Ordering::Relaxed);
    }
    CLIPBOARD_LEN.store(len, Ordering::Release);
}

pub fn set_str(text: &str) {
    let mut len = 0;
    for (slot, ch) in CLIPBOARD.iter().zip(text.chars()).take(CAPACITY) {
        slot.store(ch as u32, Ordering::Relaxed);
        len += 1;
    }
    CLIPBOARD_LEN.store(len, Ordering::Release);
}

pub fn copy_chars(out: &mut [char]) -> usize {
    let len = CLIPBOARD_LEN.load(Ordering::Acquire).min(out.len());
    for (dst, src) in out.iter_mut().zip(CLIPBOARD.iter()).take(len) {
        let value = src.load(Ordering::Relaxed);
        *dst = char::from_u32(value).unwrap_or('\u{FFFD}');
    }
    len
}

pub fn len() -> usize {
    CLIPBOARD_LEN.load(Ordering::Acquire)
}

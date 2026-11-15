//! Protection Keys for User pages (PKU/PKRU) desteği.
//!
//! PKU, kullanıcı sayfalarını 16 koruma anahtarına ayırıp `PKRU` yazmacı ile
//! anahtar başına okuma/yazma izni vermeyi sağlar (Intel SKX+, AMD Zen 3+).
//! Bu modül CPUID algılamasını, `CR4.PKE` etkinleştirmesini ve `PKRU`
//! okuma/yazma yardımcılarını sağlar.
//!
//! Not: Sayfa tablosu girdilerine anahtar ataması (`PTE` bit 59:62) ve
//! süreç başına anahtar politikası henüz uygulanmadı; başlangıç değeri
//! `PKRU = 0` tüm anahtarlara okuma/yazma izni verir, yani mevcut davranış
//! değişmez. Altyapı, ileride W^X / hassas veri izolasyonu için hazırdır.

use core::sync::atomic::{AtomicBool, Ordering};

/// `CR4.PKE` (bit 22): PKU etkin.
const CR4_PKE: u64 = 1 << 22;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// CPUID.07H:ECX.PKU (bit 3): koruma anahtarı donanım desteği.
pub fn supported() -> bool {
    let leaf = core::arch::x86_64::__cpuid_count(7, 0);
    (leaf.ecx & (1 << 3)) != 0
}

/// Çekirdek PKU'yu etkinleştirdi mi? (`CR4.PKE` kurulu ve `PKRU` yazılabilir)
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// `PKRU` yazmacını okur. PKU etkin değilse `0` döner.
pub fn read_pkru() -> u32 {
    if !enabled() {
        return 0;
    }
    let value: u32;
    unsafe {
        core::arch::asm!(
            "rdpkru",
            out("eax") value,
            in("ecx") 0u32,
            in("edx") 0u32,
            options(nostack, preserves_flags),
        );
    }
    value
}

/// `PKRU` yazmacını yazar. `value == 0` tüm anahtarlara tam erişim verir.
///
/// Her anahtar için iki bit ayrılır: `bit 2k` = yazma yasağı (WD),
/// `bit 2k+1` = okuma yasağı (AD). Çekirdek yalnızca PKU etkinken yazar.
pub fn write_pkru(value: u32) {
    if !enabled() {
        return;
    }
    unsafe {
        core::arch::asm!(
            "wrpkru",
            in("eax") value,
            in("ecx") 0u32,
            in("edx") 0u32,
            options(nostack, preserves_flags),
        );
    }
}

/// PKU'yu algılar, `CR4.PKE` bitini kurar ve `PKRU = 0` ile başlatır.
///
/// Dönüş: `true` ise PKU etkin. Destek yoksa hiçbir yazmaç değiştirilmez.
pub fn init() -> bool {
    if !supported() {
        return false;
    }
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= CR4_PKE;
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    ENABLED.store(true, Ordering::Release);
    write_pkru(0);
    // Etkinleştirmeyi doğrula: CR4.PKE gerçekten kurulu mu?
    let mut cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    let active = (cr4 & CR4_PKE) != 0;
    ENABLED.store(active, Ordering::Release);
    active
}

/// Koruma anahtarı (`0..16`) için `PKRU` değeri üretir.
///
/// `read`/`write` false ise ilgili erişim yasaklanır. `key >= 16` için `None`.
pub fn key_bits(key: u8, read: bool, write: bool) -> Option<u32> {
    if key >= 16 {
        return None;
    }
    let mut bits = 0u32;
    if !write {
        bits |= 1 << (key as u32 * 2);
    }
    if !read {
        bits |= 1 << (key as u32 * 2 + 1);
    }
    Some(bits)
}

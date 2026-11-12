//! Erken önyükleme: Limine framebuffer doğrulaması, metin konsolu, zaman tabanı
//! ve HHDM ofseti. `main.rs` içinden `src/boot/` katmanına taşındı.

use crate::boot::console::{serial_print, serial_print_dec};
use crate::boot::limine::{
    is_canonical_ptr, is_mapped_limine_ptr, valid_limine_response, FRAMEBUFFER_REQUEST,
    HHDM_REQUEST,
};
use crate::boot::time::init_timebase;
use crate::drivers;
use crate::drivers::video::{
    Framebuffer, PANIC_FB_ADDR, PANIC_FB_BPP, PANIC_FB_H, PANIC_FB_PITCH, PANIC_FB_W,
};

/// Erken önyüklemeden sonra ana çekirdeğe devredilen ekran bilgisi.
pub struct BootDisplay {
    pub fb: Framebuffer,
    pub fb_ptr: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
}

/// Erken çekirdek başlatması: CPU/SSE, seri konsol, yığın ayırıcı, PRNG ve
/// erken ekran/zaman altyapısını hazırlar.
pub fn init() -> BootDisplay {
    crate::arch::x86_64::enable_sse();
    crate::arch::x86_64::disable_pic();

    // Seri konsolu ilk iş olarak hazırla: sonraki tüm loglar (PMM/VMM/SMP…)
    // 115200 8N1 ile çerçeveye oturur.
    crate::serial::init();

    crate::kernel::heap::init();
    crate::random::init();

    early_init()
}

/// Framebuffer'ı doğrular, metin konsolunu kurar, PIT/TSC zaman tabanını
/// başlatır ve HHDM ofsetini sürücü katmanına bildirir.
pub fn early_init() -> BootDisplay {
    let (fb_ptr, width, height, pitch, bpp) = unsafe {
        let resp = valid_limine_response(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))
            .filter(|resp| !resp.is_null())
            .unwrap_or_else(|| crate::boot::text_console::fatal("Limine framebuffer yaniti yok"));

        let count = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffer_count));
        let entries = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffers));

        if count == 0 || !is_mapped_limine_ptr(entries) {
            crate::boot::text_console::fatal("Limine framebuffer listesi gecersiz; GUI acilmadi");
        }

        let entry = core::ptr::read_volatile(entries);
        if !is_mapped_limine_ptr(entry) {
            crate::boot::text_console::fatal("Limine framebuffer girdisi gecersiz; GUI acilmadi");
        }

        let info = core::ptr::read_volatile(entry);
        if info.address.is_null() || !is_canonical_ptr(info.address as *const u8) {
            crate::boot::text_console::fatal("Limine framebuffer adresi gecersiz; GUI acilmadi");
        }

        (
            info.address as *mut u32,
            info.width as usize,
            info.height as usize,
            info.pitch as usize,
            info.bpp as usize,
        )
    };

    if bpp != 32
        || width < 320
        || height < 200
        || pitch < width.saturating_mul(4)
        || pitch % 4 != 0
        || width
            .checked_mul(height)
            .is_none_or(|n| n > drivers::video::BACKBUFFER_LEN)
    {
        crate::boot::text_console::fatal("Framebuffer dogrulamasi basarisiz; GUI acilmadi");
    }

    let mut fb = unsafe { Framebuffer::new(fb_ptr, width, height, pitch, bpp) };
    if !fb.verify_bounds() {
        crate::boot::text_console::fatal("Framebuffer cizim sinirlari dogrulanamadi; GUI acilmadi");
    }

    unsafe {
        PANIC_FB_ADDR = fb_ptr;
        PANIC_FB_W = width;
        PANIC_FB_H = height;
        PANIC_FB_PITCH = pitch;
        PANIC_FB_BPP = bpp;

        crate::boot::text_console::init(Framebuffer::new(fb_ptr, width, height, pitch, bpp));
    }

    serial_print("[BOOT] UEFI metin konsolu etkin; donanım ve bellek hazırlanıyor.\n");

    serial_print("[BOOT] PIT/TSC zaman tabanı başlatılıyor...\n");
    if init_timebase() {
        serial_print("[BOOT] PIT/TSC zaman tabanı hazır.\n");
    } else {
        serial_print("[BOOT] UYARI: PIT yanıt vermedi; CPUID TSC tahmini kullanılıyor.\n");
    }

    serial_print("[VIDEO] ");
    serial_print_dec(width as u64);
    serial_print("x");
    serial_print_dec(height as u64);
    serial_print(" @ ");
    serial_print_dec(bpp as u64);
    serial_print(" bpp.\n");

    #[cfg(minos_boot_log)]
    serial_print("[minOs] Çekirdek _start noktasına ulaştı.\n");

    #[cfg(minos_boot_log)]
    serial_print("[minOs] SSE etkinleştirildi, PIC kesmeleri maskelendi.\n");

    // HHDM ofsetini al (fiziksel MMIO ve sayfa tablosu sanal erisimi icin).
    let hhdm_offset = unsafe {
        // SAFETY: the response pointer is validated before being dereferenced.
        let response_offset = valid_limine_response(core::ptr::addr_of!(HHDM_REQUEST.response))
            .map(|resp| core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset)));
        match response_offset {
            Some(offset) if is_mapped_limine_ptr(offset as *const u8) => offset,
            Some(_) => {
                serial_print(
                    "[BOOT] Invalid HHDM offset / gecersiz HHDM ofseti; using fallback.\n",
                );
                0xFFFF_8000_0000_0000
            }
            None => {
                serial_print("[BOOT] HHDM response missing / HHDM yaniti yok; using fallback.\n");
                0xFFFF_8000_0000_0000
            }
        }
    };
    serial_print("[BOOT] HHDM offset / ofset: ");
    crate::boot::console::serial_print_hex(hhdm_offset);
    serial_print("\n");
    drivers::set_hhdm_offset(hhdm_offset);

    BootDisplay {
        fb,
        fb_ptr,
        width,
        height,
        pitch,
        bpp,
    }
}

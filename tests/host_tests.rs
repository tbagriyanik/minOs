//! Host tarafinda calisan birim testleri icin kok modul.
//!
//! Cekirdek `x86_64-minos.json` hedefine `panic = "abort"` + `-Z build-std`
//! ile derlenir. `cargo test` ise `--test` modu yuzunden `panic = "unwind"`
//! ister; Cargo bu durumda `core`'u ikinci kez derler ve rustc
//! "duplicate lang item in crate `core`" (E0152) hatasi verir.
//!
//! Bu yuzden cekirdekten bagimsiz saf mantik modulleri burada, host
//! hedefinde test edilir (`test_host.ps1`). Yeni bir test eklerken ilgili
//! modulu bu dosyaya baglamaniz yeterlidir.

#![no_std]

#[macro_use]
extern crate alloc;
extern crate heapless;

#[path = "../src/drivers/crypto/mod.rs"]
pub mod crypto;

// HTTP `Content-Encoding` çözücüsü (miniz_oxide + brotli-decompressor).
#[path = "../src/drivers/brotli.rs"]
pub mod brotli;

#[path = "../src/drivers/compression.rs"]
pub mod compression;

#[path = "../src/apps/js.rs"]
pub mod js;

#[path = "../src/apps/browser/types.rs"]
pub mod browser_types;

/// `browser_css`, `super::types` yolunu bekler; host testlerinde bu adı
/// `browser_types` modülüne yönlendir.
pub mod types {
    pub use crate::browser_types::*;
}

#[path = "../src/apps/browser/css.rs"]
pub mod browser_css;

#[path = "../src/apps/browser/svg.rs"]
pub mod browser_svg;

// Canvas 2D: dolgu/fillRect, gradyan, save/restore, transform,
// path (quad/cubic/arc), getImageData/putImageData birim testleri.
#[path = "../src/apps/browser/canvas.rs"]
pub mod browser_canvas;

// PNG/JPEG/GIF/WebP/BMP/SVG kod çözücüleri (saf alloc, host'ta test edilebilir).
#[path = "../src/apps/imgdecode.rs"]
pub mod imgdecode;

#[path = "../src/apps/calc_core.rs"]
pub mod calc_core;

#[path = "../src/apps/game_core.rs"]
pub mod game_core;

// Uygulama hata siniflari/kurtarma yollari ve uygulama bazli bellek olcumu.
#[path = "../src/apps/errors.rs"]
pub mod app_errors;

#[path = "../src/apps/memory.rs"]
pub mod app_memory;

#[path = "../src/gui/textbuf.rs"]
pub mod textbuf;

#[path = "../src/gui/clipboard.rs"]
pub mod clipboard;

#[path = "../src/gui/dirty_rect.rs"]
pub mod dirty_rect;

#[path = "../src/gui/textedit.rs"]
pub mod textedit;

#[path = "../src/gui/timefmt.rs"]
pub mod timefmt;

#[path = "../src/gui/theme.rs"]
pub mod theme;

// Ekran geometrisi: `super::theme` yolu crate kokundeki theme modulunu bulur.
#[path = "../src/gui/screen_layout.rs"]
pub mod screen_layout;

#[path = "../src/gui/font_render.rs"]
pub mod font_render;

#[path = "../src/drivers/rtc.rs"]
pub mod rtc;

// TLS protokol katmani: `crate::boot::time` ve `crate::monotonic_ms`
// cekirdekte var; host testlerinde sabit birer yedek (stub) saglanir.
pub fn monotonic_ms() -> u64 {
    // 2024-01-01 00:00:00 UTC in milliseconds
    // This date is after nearly all TLS certificates issued (2016+),
    // and before most certificate expiry dates (typically 1-39 months validity).
    // Works across different timezones and avoids "6 hours ahead" issues
    // that can occur with QEMU when using specific calendar dates.
    1704067200000
}

#[path = "../src/drivers/bluetooth.rs"]
pub mod bluetooth_mod;

#[path = "../src/drivers/port_io.rs"]
pub mod port_io;

#[path = "../src/drivers/virtio_gpu.rs"]
pub mod virtio_gpu_mod;

#[path = "../src/drivers/acpi_battery.rs"]
pub mod acpi_battery_mod;

#[path = "../src/drivers/usb_transfer.rs"]
pub mod usb_transfer_mod;

#[path = "../src/drivers/usb_touchscreen.rs"]
pub mod usb_touchscreen_mod;

#[path = "../src/drivers/netstack.rs"]
pub mod netstack_mod;

pub fn serial_print(_s: &str) {}

pub mod serial {
    pub fn print_dec(_v: u64) {}
}

pub mod boot {
    pub mod time {
        pub fn main_monotonic_ms() -> u64 {
            // 2024-01-01 00:00:00 UTC in milliseconds
            // This date is after nearly all TLS certificates issued (2016+),
            // and before most certificate expiry dates (typically 1-39 months validity).
            // Works across different timezones and avoids "6 hours ahead" issues
            // that can occur with QEMU when using specific calendar dates.
            1704067200000
        }

        pub fn monotonic_ms() -> u64 {
            main_monotonic_ms()
        }
    }
    pub mod limine {
        pub fn is_canonical_ptr<T>(ptr: *const T) -> bool {
            !ptr.is_null()
        }
        pub fn is_mapped_limine_ptr<T>(ptr: *const T) -> bool {
            !ptr.is_null()
        }
        pub unsafe fn valid_limine_response<T>(slot: *const *const T) -> Option<*const T> {
            if slot.is_null() {
                None
            } else {
                let p = *slot;
                if p.is_null() {
                    None
                } else {
                    Some(p)
                }
            }
        }
        pub fn limine_rsdp_address() -> Option<*const u8> {
            None
        }
    }
    pub mod console {
        pub fn serial_print(_s: &str) {}
        pub fn serial_print_dec(_v: u64) {}
    }
}

// Watchdog kesme-bağımsız denetimde uygulama bağlamını okur; host testinde
// her zaman masaüstü/uygulama-dışı bağlam döndürülür.
pub mod apps {
    pub mod guard {
        pub const NO_APP: usize = usize::MAX;
        pub const NO_WINDOW: usize = usize::MAX;
        pub fn current_context() -> (usize, usize, bool) {
            (NO_APP, NO_WINDOW, false)
        }
    }
    // imgdecode.rs SVG için `crate::apps::browser::svg::render_svg` çağırır;
    // host testlerinde SVG çözümü devre dışı bırakılır.
    pub mod browser {
        pub mod svg {
            pub fn render_svg(_src: &[u8]) -> Option<(usize, usize, alloc::vec::Vec<u8>)> {
                None
            }
        }
    }

    // canvas.rs üçgen/arc için viewer3d'nin saf matematik yardımcılarını
    // kullanır; host testinde aynı Taylor yaklaşımlarını sağlar.
    pub mod viewer3d {
        const PI: f32 = core::f32::consts::PI;
        const TWO_PI: f32 = core::f32::consts::TAU;
        const HALF_PI: f32 = core::f32::consts::FRAC_PI_2;

        pub fn sin(mut x: f32) -> f32 {
            while x > PI {
                x -= TWO_PI;
            }
            while x < -PI {
                x += TWO_PI;
            }
            let x2 = x * x;
            let x3 = x * x2;
            let x5 = x3 * x2;
            let x7 = x5 * x2;
            x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
        }

        pub fn cos(x: f32) -> f32 {
            sin(x + HALF_PI)
        }
    }
}

#[path = "../src/drivers/tls.rs"]
pub mod tls;

// RFC 6265 çerez jarı: Set-Cookie ayrıştırma, Expires/Max-Age, Secure,
// SameSite ve domain/path eşleştirme birim testleri.
#[path = "../src/drivers/cookie.rs"]
pub mod cookie_jar;

// QUIC v1 paket koruma katmanı: HKDF, header protection, AEAD,
// varint ve frame kodlama birim testleri (RFC 9001 vektörleri).
#[path = "../src/drivers/quic.rs"]
pub mod quic;

// HPACK/QPACK ortak Huffman tablosu (RFC 7541 Ek B).
#[path = "../src/drivers/huffman.rs"]
pub mod huffman;

// HTTP istemcilerinin ortak User-Agent dizesi.
#[path = "../src/user_agent.rs"]
pub mod user_agent;

// QPACK (RFC 9204) statik tablo kodlayıcı/çözücü.
#[path = "../src/drivers/qpack.rs"]
pub mod qpack;

// QUIC akış/CRYPTO yeniden birleştirme tamponları (offset'li,
// tekrar-birleştirmeli, sürekli-kapsam çıkarımı).
#[path = "../src/drivers/streambuf.rs"]
pub mod streambuf;

#[path = "../src/time_budget.rs"]
pub mod time_budget;

#[path = "../src/watchdog.rs"]
pub mod watchdog;

// Host test icin rastgelelik stub'i: Gercek donanim RDRAND/RDSEED yok,
// bu yuzde basit bir xorshift PRNG kullanilir.
pub mod random {
    static mut STATE: u64 = 0x6d69_6e4f_532d_524e;

    pub fn init() {
        unsafe {
            STATE = 0x6d69_6e4f_532d_524e;
        }
    }

    pub fn seed(seed: [u8; 32]) {
        let mut value = 0u64;
        for chunk in seed.chunks_exact(8) {
            value ^= u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
        }
        unsafe {
            STATE = value | 1;
        }
    }

    pub fn hw_available() -> bool {
        false
    }

    pub fn fill_bytes(buf: &mut [u8]) {
        let mut offset = 0;
        while offset < buf.len() {
            let remaining = buf.len() - offset;
            let chunk = if remaining >= 8 { 8 } else { remaining };
            unsafe {
                let mut x = STATE;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                STATE = x;
                let val = x.wrapping_mul(0x2545_f491_4f6c_dd1d).to_le_bytes();
                buf[offset..offset + chunk].copy_from_slice(&val[..chunk]);
            }
            offset += chunk;
        }
    }

    // Host harness only: deterministic stub for crypto tests. Real kernel
    // builds use random::secure_fill_bytes(), which requires hardware entropy.
    pub fn secure_fill_bytes(buf: &mut [u8]) -> bool {
        fill_bytes(buf);
        true
    }
}

// Kaynak moduller `crate::drivers::crypto::...` yoluyla birbirine bagli
// oldugu icin ayni yolu host harness'inda da saglariz.
pub mod drivers {
    pub use crate::crypto;
    pub use crate::huffman;
    pub use crate::qpack;
    pub use crate::tls;
    pub fn phys_to_virt(p: u64) -> u64 {
        p
    }
    pub mod video {
        pub static mut PANIC_FB_ADDR: usize = 0;

        /// `font_render.rs`'deki `draw`/`blit_glyph` imzalarında ve
        /// `canvas.rs`'in `render_to_fb`'sinde kullanılan minimal
        /// `Framebuffer` yurdu; font_render testleri `put_subpixel_text`'i,
        /// canvas testleri `put_pixel`'i çağırır.
        pub struct Framebuffer {
            pub width: usize,
            pub height: usize,
            pub pixels: alloc::vec::Vec<u32>,
        }

        impl Framebuffer {
            pub fn new(width: usize, height: usize) -> Self {
                Framebuffer {
                    width,
                    height,
                    pixels: alloc::vec![0; width * height],
                }
            }

            pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
                if x < self.width && y < self.height {
                    self.pixels[y * self.width + x] = color;
                }
            }

            pub fn put_subpixel_text(
                &mut self,
                _x: usize,
                _y: usize,
                _color: u32,
                _alpha: [u8; 3],
            ) {
            }
        }
    }
    pub mod bluetooth {
        pub use crate::bluetooth_mod::*;
    }
    pub mod port_io {
        pub use crate::port_io::*;
    }
    pub mod virtio_gpu {
        pub use crate::virtio_gpu_mod::*;
    }
    pub mod acpi_battery {
        pub use crate::acpi_battery_mod::*;
    }
    pub mod usb {
        pub use crate::usb_transfer_mod::*;
    }
    pub mod usb_hid {
        pub fn touchscreen_present() -> bool {
            false
        }
        pub fn touchscreen_info() -> Option<(u32, u32, bool)> {
            None
        }
    }
    pub mod usb_touchscreen {
        pub use crate::usb_touchscreen_mod::*;
    }
    pub mod e1000 {
        pub static mut MAC_ADDR: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        pub fn transmit(_data: &[u8]) -> bool {
            true
        }
        pub fn is_ready() -> bool {
            true
        }
        pub fn link_up() -> bool {
            true
        }
        pub fn poll() {}
    }
    pub mod socket {
        use super::netstack::{IpAddress, Ipv4Address};
        use crate::netstack_mod::Ipv6Address;
        pub fn on_udp_datagram(_src: IpAddress, _sport: u16, _dport: u16, _data: &[u8]) -> bool {
            false
        }
        pub fn on_tcp_segment(_src: Ipv4Address, _data: &[u8]) -> bool {
            false
        }
        pub fn on_tcp_segment_ipv6(_src: Ipv6Address, _data: &[u8]) -> bool {
            false
        }
        pub fn self_test() -> bool {
            true
        }
    }
    pub mod http {
        pub fn self_test() -> bool {
            true
        }
    }
    pub mod netstack {
        pub use crate::netstack_mod::*;
    }
}

pub mod arch {
    pub mod x86_64 {
        pub fn mmio_map_2mb(_p: u64) {}

        /// Host testlerinde sayfa eşlemesi yok; ACPI tablo okumaları
        /// çalıştırılmadığı için yedek (no-op) uygulama yeterlidir.
        pub fn map_phys_readable(_phys: u64) {}

        /// Host testlerinde sayfa eşlemesi yok; yedek (no-op) uygulama.
        pub fn map_legacy_acpi_region() {}

        pub mod sync {
            use core::cell::UnsafeCell;
            use core::ops::{Deref, DerefMut};

            pub struct SpinMutex<T> {
                inner: UnsafeCell<T>,
            }

            unsafe impl<T: Send> Sync for SpinMutex<T> {}
            unsafe impl<T: Send> Send for SpinMutex<T> {}

            pub struct SpinGuard<'a, T> {
                lock: &'a SpinMutex<T>,
            }

            impl<T> SpinMutex<T> {
                pub const fn new(value: T) -> Self {
                    Self {
                        inner: UnsafeCell::new(value),
                    }
                }
                pub fn lock(&self) -> SpinGuard<'_, T> {
                    SpinGuard { lock: self }
                }
                pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
                    Some(SpinGuard { lock: self })
                }
            }

            impl<'a, T> Deref for SpinGuard<'a, T> {
                type Target = T;
                fn deref(&self) -> &T {
                    unsafe { &*self.lock.inner.get() }
                }
            }

            impl<'a, T> DerefMut for SpinGuard<'a, T> {
                fn deref_mut(&mut self) -> &mut T {
                    unsafe { &mut *self.lock.inner.get() }
                }
            }
        }
    }
}

#[path = "../src/mm/allocator/mod.rs"]
pub mod allocator;

// Cekirdekten bagimsiz saf mantik modulleri: paket bagimlilik cozumleyici
// (dagitim oncesi surum/cevre analizi) ve sertifika iptal kontrolu (CRL/OCSP).
#[path = "../src/apps/pkg_deps.rs"]
pub mod pkg_deps;

// Dosya sistemleri: `block` (blok aygiti + bellek ici aygit), `exfat_core`
// (VBR yapisi, checksum ve upcase tablosu) ile `exfat_rw` / `ext4_rw`
// suruculeri. Cekirdek ATA yerine `BlockDevice` kullandiklari icin host'ta
// uctan uca test edilebilir. Suruculer `crate::fs::...` yoluyla birbirine
// bagli oldugundan ayni yol host harness'inda da saglanir.
#[path = "../src/fs/block.rs"]
pub mod block;

pub mod fs {
    pub mod block {
        pub use crate::block::*;
    }
    pub mod exfat {
        pub use crate::exfat_core::*;
    }
    pub mod ext4 {
        pub use crate::ext4_core::*;
    }
}

#[path = "../src/fs/exfat.rs"]
pub mod exfat_core;

#[path = "../src/fs/exfat_rw.rs"]
pub mod exfat_rw;

/// Cekirdekte `crate::fs::exfat` altindaki kisa yol; host'ta `exfat_core`.
pub mod exfat {
    pub const EXFAT_OEM_NAME: &[u8; 8] = b"EXFAT   ";
    pub const EXFAT_SIGNATURE: u16 = 0xAA55;
    pub fn to_upper_exfat(c: u16) -> u16 {
        crate::exfat_core::to_upper_exfat(c)
    }
}

#[path = "../src/fs/ext4_rw.rs"]
pub mod ext4_rw;

#[path = "../src/fs/ext4.rs"]
pub mod ext4_core;

#[cfg(test)]
#[path = "host_crypto_tests.rs"]
mod crypto_tests;

#[cfg(test)]
#[path = "host_allocator_tests.rs"]
mod host_allocator_tests;

#[cfg(test)]
#[path = "host_feature_tests.rs"]
mod host_feature_tests;

#[cfg(test)]
#[path = "host_gui_tests.rs"]
mod host_gui_tests;

#[cfg(test)]
#[path = "host_fs_tests.rs"]
mod host_fs_tests;

#[cfg(test)]
#[path = "host_driver_tests.rs"]
mod host_driver_tests;

#[cfg(test)]
#[path = "host_compression_tests.rs"]
mod host_compression_tests;

#[path = "../src/mm/memory/compress.rs"]
pub mod mzram_compress;

#[path = "../src/drivers/input_queue.rs"]
pub mod input_queue;

#[cfg(test)]
#[path = "host_mzram_tests.rs"]
mod host_mzram_tests;

#[cfg(test)]
#[path = "host_perf_tests.rs"]
mod host_perf_tests;

#[cfg(test)]
#[path = "host_input_tests.rs"]
mod host_input_tests;

#[cfg(test)]
#[path = "host_imgdecode_tests.rs"]
mod host_imgdecode_tests;

#[path = "host_canvas_tests.rs"]
mod host_canvas_tests;

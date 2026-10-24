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

#[path = "../src/drivers/crypto/mod.rs"]
pub mod crypto;

#[path = "../src/apps/js.rs"]
pub mod js;

#[path = "../src/apps/browser/types.rs"]
pub mod browser_types;

#[path = "../src/apps/calc_core.rs"]
pub mod calc_core;

#[path = "../src/gui/textedit.rs"]
pub mod textedit;

#[path = "../src/gui/timefmt.rs"]
pub mod timefmt;

#[path = "../src/gui/theme.rs"]
pub mod theme;

// TLS protokol katmani: `crate::boot::time` ve `crate::monotonic_ms`
// cekirdekte var; host testlerinde sabit birer yedek (stub) saglanir.
pub fn monotonic_ms() -> u64 {
    0
}

pub mod boot {
    pub mod time {
        pub fn main_monotonic_ms() -> u64 {
            0
        }
    }
}

#[path = "../src/drivers/tls.rs"]
pub mod tls;

// Kaynak moduller `crate::drivers::crypto::...` yoluyla birbirine bagli
// oldugu icin ayni yolu host harness'inda da saglariz.
pub mod drivers {
    pub use crate::crypto;
    pub use crate::tls;
}

pub mod arch {
    pub mod x86_64 {
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
                    Self { inner: UnsafeCell::new(value) }
                }
                pub fn lock(&self) -> SpinGuard<'_, T> {
                    SpinGuard { lock: self }
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
    pub fn to_upper_exfat(c: u16) -> u16 { crate::exfat_core::to_upper_exfat(c) }
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
#[path = "host_fs_tests.rs"]
mod host_fs_tests;


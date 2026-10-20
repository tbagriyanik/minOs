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

// Kaynak moduller `crate::drivers::crypto::...` yoluyla birbirine bagli
// oldugu icin ayni yolu host harness'inda da saglariz.
pub mod drivers {
    pub use crate::crypto;
}

#[cfg(test)]
#[path = "host_crypto_tests.rs"]
mod crypto_tests;


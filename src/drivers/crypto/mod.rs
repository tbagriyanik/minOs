//! Cryptographic primitives for TLS — no_std.

pub mod sha256;
pub mod sha384;
pub mod hmac;
pub mod aes;
pub mod gcm;
pub mod rsa;
pub mod ecc;
pub mod x509;
// Iptal kontrolu: ayrıştırma/imza çekirdekten bağımsız (`revocation_core`),
// durum tutan politika katmanı çekirdekte (`revocation`).
#[path = "revocation_core.rs"]
pub mod revocation_core;
#[cfg_attr(not(test), path = "revocation.rs")]
#[cfg_attr(test, path = "revocation_core.rs")]
pub mod revocation;

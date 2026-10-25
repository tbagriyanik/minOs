//! Cryptographic primitives for TLS — no_std.

pub mod aes;
pub mod ecc;
pub mod gcm;
pub mod hmac;
pub mod rsa;
pub mod sha256;
pub mod sha384;
pub mod x509;
// Iptal kontrolu: ayrıştırma/imza çekirdekten bağımsız (`revocation_core`),
// durum tutan politika katmanı çekirdekte (`revocation`).
#[cfg_attr(not(test), path = "revocation.rs")]
#[cfg_attr(test, path = "revocation_core.rs")]
pub mod revocation;
#[path = "revocation_core.rs"]
pub mod revocation_core;

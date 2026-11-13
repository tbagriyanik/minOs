//! Cryptographic primitives for TLS — no_std.

pub mod aes;
pub mod ecc;
pub mod gcm;
pub mod hmac;
pub mod rsa;
pub mod sha1;
pub mod sha256;
pub mod sha384;
pub mod x25519;
// Ağ indirme katmanı yalnızca çekirdek derlemesinde (host test mock'ları dışında).
#[cfg(not(test))]
pub mod revocation_fetch;
pub mod x509;
// Iptal kontrolu: ayrıştırma/imza çekirdekten bağımsız (`revocation_core`),
// durum tutan politika katmanı çekirdekte (`revocation`).
pub mod revocation;
pub mod revocation_core;

//! Sertifika iptal kontrolü (CRL / OCSP) — çekirdek tarafı depo ve politika.
//!
//! Ayrıştırma ve imza/içerik kuralları `revocation_core` içindedir (host
//! testleriyle doğrulanır). Bu modül, TLS el sıkışmasının sorgulayacağı son
//! iptal sonucunu tutar ve tek noktadan (`check_certificate`) fail-closed
//! politika uygular:
//!
//! - Açıkça `revoked` işaretlenmiş sertifika → reddet.
//! - Kaynak yok / sonuç `unknown` → reddet (fail-closed).
//! - Kaynak `good` → kabul.
//!
//! `set_status` / `clear` ağ katmanı veya kullanıcı komutu tarafından
//! doldurulur; gerçek OCSP isteği üretimi (imzalı request) henüz yoktur.

#![allow(dead_code, static_mut_refs)]

use alloc::vec::Vec;

pub use super::revocation_core::{
    check_crl, check_crl_verified, check_ocsp, parse_crl, parse_ocsp_response, CrlInfo,
    OcspResponseInfo, RevocationResult, RevocationStatus,
};

/// İptal kaynağı etiketi (tanılama logları için).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevocationSource {
    None,
    Crl,
    Ocsp,
    Manual,
}

impl RevocationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RevocationSource::None => "none",
            RevocationSource::Crl => "crl",
            RevocationSource::Ocsp => "ocsp",
            RevocationSource::Manual => "manual",
        }
    }
}

static mut SOURCE: RevocationSource = RevocationSource::None;
static mut RESULT: RevocationResult = RevocationResult {
    status: RevocationStatus::Unknown,
    revoked_at: None,
    reason: None,
};

pub fn source() -> RevocationSource {
    unsafe { SOURCE }
}

/// Son iptal sonucu.
pub fn last_result() -> RevocationResult {
    unsafe { RESULT }
}

/// İptal sonucunu güncelle.
pub fn set_status(source: RevocationSource, result: RevocationResult) {
    unsafe {
        SOURCE = source;
        RESULT = result;
    }
}

/// Sertifikayı elle iptal edilmiş say (tanılama / zararlı anahtar müdahalesi).
pub fn set_revoked(source: RevocationSource) {
    set_status(source, RevocationResult::revoked(None, None))
}

/// İptal kaydını sıfırla (yeni bağlantı öncesi).
pub fn clear() {
    unsafe {
        SOURCE = RevocationSource::None;
        RESULT = RevocationResult::unknown();
    }
}

/// TLS politika kararı: yalnızca açıkça `good` sonucu kabul edilir.
///
/// Dönen `Err(&'static str)` metni açılış/bağlantı günlüğüne yazılır.
pub fn check_certificate() -> Result<(), &'static str> {
    match last_result() {
        RevocationResult {
            status: RevocationStatus::Good,
            ..
        } => Ok(()),
        RevocationResult {
            status: RevocationStatus::Revoked,
            ..
        } => Err("Sertifika iptal edilmis (revoked)"),
        _ => Err("Sertifika iptal durumu bilinmiyor (OCSP/CRL yaniti yok)"),
    }
}

/// Bir CRL bayt dizisini doğrulayıp depoya yazar. CRL'in imzası ve tazeliği
/// `revocation_core::check_crl_verified` tarafından denetlenir.
pub fn apply_crl(
    crl_der: &[u8],
    issuer_cert_der: &[u8],
    leaf_serial: &[u8],
    now: &super::x509::X509Time,
) -> RevocationResult {
    let r = check_crl_verified(crl_der, issuer_cert_der, leaf_serial, now)
        .unwrap_or_else(|_| RevocationResult::unknown());
    set_status(RevocationSource::Crl, r);
    r
}

/// Bir OCSP yanıtını ayrıştırıp depoya yazar (imza doğrulaması çağıranın
/// sorumluluğundadır; `OcspResponseInfo::verify_signature`).
pub fn apply_ocsp(
    ocsp_der: &[u8],
    leaf_serial: &[u8],
    now: &super::x509::X509Time,
) -> RevocationResult {
    let r = check_ocsp(ocsp_der, leaf_serial, now).unwrap_or_else(|_| RevocationResult::unknown());
    set_status(RevocationSource::Ocsp, r);
    r
}

/// Sertifikanın seri numarası (tanılama).
pub fn serial_of(cert_der: &[u8]) -> Option<Vec<u8>> {
    super::revocation_core::certificate_serial(cert_der).ok()
}

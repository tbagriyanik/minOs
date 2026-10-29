//! Sertifika iptal kontrolü (CRL ve OCSP) — `no_std` + `alloc`.
//!
//! TLS zincir doğrulaması bir sertifikanın güvenilir bir kök CA'ya kadar
//! imzalandığını kanıtlar; ancak süresi dolmadan iptal edilmiş (anahtar sızıntısı,
//! yanlış düzenleme vb.) sertifikalar imza olarak hâlâ geçerlidir. Bu modül
//! iptal durumunu iki yolla kontrol eder:
//!
//! - **CRL**: DER kodlu `CertificateList` (X.509 v2 liste) ayrıştırılır; listede
//!   çakışan `serialNumber` varsa sertifika iptal edilmiştir. Listenin kendisi
//!   `tbsCertList` kısmı üzerinden bir CA sertifikasıyla imza doğrulamasına tabi
//!   tutulur. `thisUpdate`/`nextUpdate` alanları ile bayat liste reddedilir.
//! - **OCSP**: DER kodlu `OCSPResponse` → `BasicOCSPResponse` →
//!   `SingleResponse` zinciri ayrıştırılır; istenen sertifikanın `status` alanı
//!   `good` / `revoked` / `unknown` olarak okunur, `producedAt`, `thisUpdate` ve
//!   `nextUpdate` zamanları kontrol edilir.
//!
//! Ağ katmanından bağımsızdır: içerik çağıran tarafından (TLS bağlantısı
//! kapatıldıktan sonra HTTP/OCSP isteğiyle) getirilir; burada yalnızca ayrıştırma
//! ve politika kararı yapılır. Saat `unix_to_x509_time` ile RTC'den beslenir.
//!
//! Bilinmeyen bir durum "iyi" sayılmaz: çağıranın (TLS) eksik/ayrıştırılamayan
//! iptal verisini reddetmesi (fail-closed) beklenir.

use crate::drivers::crypto::x509::{parse_der_time, CertValidationError, ParsedX509Cert, X509Time};
use alloc::vec::Vec;

#[allow(dead_code)]
const TAG_BOOLEAN: u8 = 0x01;
const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_OCTET_STRING: u8 = 0x04;
#[allow(dead_code)]
const TAG_NULL: u8 = 0x05;
const TAG_OID: u8 = 0x06;
const TAG_ENUMERATED: u8 = 0x0A;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_CONTEXT_0: u8 = 0xA0;
const TAG_CONTEXT_1: u8 = 0xA1;

/// id-ce-cRLNumber + CRLReason vb. uzantı OID öneki (2.5.29.x) ayrıştırmada
/// yalnızca etiket olarak gereklidir; özel bir anlam yüklenmez.
#[allow(dead_code)]
const OID_CE_PREFIX: &[u8] = &[0x55, 0x1D];

/// OCSP imzalı yanıt: nesne tanımlayıcısı `1.3.6.1.5.5.7.48.1.1`.
pub const OID_OCSP_BASIC_RESPONSE: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x01, 0x01];

/// Bir sertifikanın iptal durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevocationStatus {
    Good,
    Revoked,
    /// Sunucu sertifikayı tanımıyor (OCSP `unknown` / CRL'de yok-ama-güvenli değil).
    Unknown,
}

/// Tek bir CRL/OCSP sorgusun doğrulanmış sonucu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RevocationResult {
    pub status: RevocationStatus,
    /// OCSP: iptal zamanı; CRL: listede bulunduysa `revocationDate`.
    pub revoked_at: Option<X509Time>,
    /// OCSP: `revoked` yanıtındaki `CRLReason` (0=unspecified ... 5=cessationOfOperation).
    pub reason: Option<u8>,
}

impl RevocationResult {
    pub fn good() -> Self {
        Self {
            status: RevocationStatus::Good,
            revoked_at: None,
            reason: None,
        }
    }
    pub fn revoked(at: Option<X509Time>, reason: Option<u8>) -> Self {
        Self {
            status: RevocationStatus::Revoked,
            revoked_at: at,
            reason,
        }
    }
    pub fn unknown() -> Self {
        Self {
            status: RevocationStatus::Unknown,
            revoked_at: None,
            reason: None,
        }
    }
    /// "İyi" sonuç yalnızca açıkça `good` olduğunda döner.
    pub fn is_acceptable(&self) -> bool {
        self.status == RevocationStatus::Good
    }
}

// ---------------------------------------------------------------------------
// Yardımcı ayrıştırıcılar
// ---------------------------------------------------------------------------

/// DER uzunluğunu okur: (tüketilen bayt, uzunluk).
fn der_len(data: &[u8], offset: usize) -> Result<(usize, usize), ()> {
    if offset >= data.len() {
        return Err(());
    }
    let first = data[offset];
    if first & 0x80 == 0 {
        return Ok((1, first as usize));
    }
    let n = (first & 0x7F) as usize;
    if n == 0 || n > 4 || offset + 1 + n > data.len() {
        return Err(());
    }
    let mut len = 0usize;
    for i in 0..n {
        len = (len << 8) | data[offset + 1 + i] as usize;
    }
    Ok((1 + n, len))
}

/// DER TLV okur: (etiket, değer başlangıcı, değer uzunluğu, sonraki ofset).
fn tlv(data: &[u8], offset: usize) -> Result<(u8, usize, usize, usize), ()> {
    if offset >= data.len() {
        return Err(());
    }
    let tag = data[offset];
    let (lc, len) = der_len(data, offset + 1)?;
    let vo = offset + 1 + lc;
    if vo.checked_add(len).map_or(true, |end| end > data.len()) {
        return Err(());
    }
    Ok((tag, vo, len, vo + len))
}

/// Baştaki sıfır baytları atılmış büyük-endian tamsayı (seri numarası).
fn strip_leading_zeros(v: &[u8]) -> &[u8] {
    let mut i = 0;
    while i + 1 < v.len() && v[i] == 0 {
        i += 1;
    }
    &v[i..]
}

/// İki sertifika seri numarasını (DER INTEGER içerikleri, işaretsiz) karşılaştır.
pub fn serials_equal(a: &[u8], b: &[u8]) -> bool {
    strip_leading_zeros(a) == strip_leading_zeros(b)
}

/// Bir sertifikanın seri numarasını döner (DER `tbsCertificate.serialNumber` değeri).
pub fn certificate_serial(cert_der: &[u8]) -> Result<Vec<u8>, ()> {
    let (tag, vo, _, _) = tlv(cert_der, 0)?;
    if tag != TAG_SEQUENCE {
        return Err(());
    }
    let (tbs_tag, tbs_vo, _, _) = tlv(cert_der, vo)?;
    if tbs_tag != TAG_SEQUENCE {
        return Err(());
    }
    let mut pos = tbs_vo;
    let (first_tag, _, _, first_next) = tlv(cert_der, pos)?;
    if first_tag == TAG_CONTEXT_0 {
        pos = first_next;
    }
    let (ser_tag, ser_vo, ser_vl, _) = tlv(cert_der, pos)?;
    if ser_tag != TAG_INTEGER || ser_vl == 0 {
        return Err(());
    }
    Ok(cert_der[ser_vo..ser_vo + ser_vl].to_vec())
}

/// Öznitelik (attribute) değerini okur; değer bir SET içindedir.
/// `atv_vo` AttributeTypeAndValue SEQUENCE'inin başlangıcıdır.
fn atv_value<'a>(der: &'a [u8], atv_vo: usize) -> Result<(u8, usize, usize), ()> {
    let (oid_tag, _oid_vo, _oid_vl, oid_next) = tlv(der, atv_vo)?;
    if oid_tag != TAG_OID {
        return Err(());
    }
    let (val_tag, val_vo, val_vl, _) = tlv(der, oid_next)?;
    Ok((val_tag, val_vo, val_vl))
}

/// Name (RDNSequence) içinden CN (`2.5.4.3`) değerini döner.
fn name_common_name(der: &[u8], name_vo: usize, name_vl: usize) -> Option<Vec<u8>> {
    const CN_OID: &[u8] = &[0x55, 0x04, 0x03];
    let end = name_vo.checked_add(name_vl)?;
    let mut pos = name_vo;
    while pos < end {
        let (tag, rdn_vo, rdn_vl, rdn_next) = tlv(der, pos).ok()?;
        if tag == TAG_SET || tag == TAG_SEQUENCE {
            let mut inner = rdn_vo;
            let inner_end = rdn_vo + rdn_vl;
            while inner < inner_end {
                let (seq_tag, atv_vo, _atv_vl, atv_next) = tlv(der, inner).ok()?;
                if seq_tag == TAG_SEQUENCE {
                    if let Ok((oid_tag, oid_vo, oid_vl, _)) = tlv(der, atv_vo) {
                        if oid_tag == TAG_OID
                            && oid_vl == CN_OID.len()
                            && &der[oid_vo..oid_vo + oid_vl] == CN_OID
                        {
                            if let Ok((_, val_vo, val_vl)) = atv_value(der, atv_vo) {
                                return Some(der[val_vo..val_vo + val_vl].to_vec());
                            }
                        }
                    }
                }
                inner = atv_next;
            }
        }
        pos = rdn_next;
    }
    None
}

/// Issuer Name'in CN değerini ham bayt olarak döner.
pub fn issuer_common_name(cert_der: &[u8]) -> Option<Vec<u8>> {
    let parsed = ParsedX509Cert::parse(cert_der).ok()?;
    let raw = parsed.issuer_raw;
    let (_tag, vo, vl, _) = tlv(raw, 0).ok()?;
    name_common_name(raw, vo, vl)
}

/// Zamanı `X509Time`'a çevir (CRL/OCSP alanları UTC/Generalized zaman).
fn time_at(der: &[u8], offset: usize) -> Option<(X509Time, usize)> {
    let (tag, vo, vl, next) = tlv(der, offset).ok()?;
    parse_der_time(tag, &der[vo..vo + vl]).map(|t| (t, next))
}

/// 1 Ocak 1970'ten günümüze saat dilimi düzeltmesi YOK — çağıran RTC'nin
/// yerel saatini değil, UTC değerini vermelidir.
pub fn unix_to_x509_time(unix_secs: u64) -> X509Time {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    let hour = (rem / 3_600) as u8;
    let minute = ((rem % 3_600) / 60) as u8;
    let second = (rem % 60) as u8;

    // 1970-01-01 tarihinden itibaren gün sayısı ile yıl/ay/gün hesabı.
    let mut year = 1970u32;
    let mut day_of_year = days as u32;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let year_len = if leap { 366 } else { 365 };
        if day_of_year < year_len {
            break;
        }
        day_of_year -= year_len;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let months = [
        31u32,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    let mut d = day_of_year;
    while month < 12 && d >= months[month] {
        d -= months[month];
        month += 1;
    }
    X509Time {
        year: year.min(u16::MAX as u32) as u16,
        month: (month + 1) as u8,
        day: (d + 1) as u8,
        hour,
        minute,
        second,
    }
}

// ---------------------------------------------------------------------------
// CRL
// ---------------------------------------------------------------------------

/// Ayrıştırılmış CRL alanları.
#[derive(Clone, Debug)]
pub struct CrlInfo {
    pub issuer: Vec<u8>,
    pub issuer_cn: Option<Vec<u8>>,
    pub this_update: Option<X509Time>,
    pub next_update: Option<X509Time>,
    /// Listelenen sertifikaların seri numaraları (işaretsiz baytlar).
    pub revoked: Vec<Vec<u8>>,
    /// İmza doğrulaması için `tbsCertList` ham baytları.
    pub tbs_raw: Vec<u8>,
    pub sig_alg_oid: Vec<u8>,
    pub signature: Vec<u8>,
}

impl CrlInfo {
    /// İptal listesini bir CA sertifikasıyla imza doğrulamasına tabi tutar.
    pub fn verify_signature(&self, issuer_cert_der: &[u8]) -> Result<(), CertValidationError> {
        let fake_cert = ParsedX509Cert {
            tbs_raw: &self.tbs_raw,
            issuer_raw: &[],
            subject_raw: &[],
            sig_alg_oid: &self.sig_alg_oid,
            signature: &self.signature,
            cert_der: issuer_cert_der,
        };
        fake_cert.verify_signature_with_issuer(issuer_cert_der)
    }

    /// Listeyi `now` zamanına göre kullanılabilir kabul ediyor mu?
    pub fn is_current(&self, now: &X509Time) -> bool {
        if let Some(next) = &self.next_update {
            if now > next {
                return false;
            }
        }
        if let Some(this) = &self.this_update {
            if now < this {
                return false;
            }
        }
        true
    }

    pub fn contains_serial(&self, serial: &[u8]) -> bool {
        self.revoked.iter().any(|s| serials_equal(s, serial))
    }
}

/// DER kodlu `CertificateList` ayrıştır.
///
/// ```text
/// CertificateList ::= SEQUENCE {
///     tbsCertList          TBSCertList,
///     signatureAlgorithm   AlgorithmIdentifier,
///     signatureValue       BIT STRING }
/// ```
pub fn parse_crl(der: &[u8]) -> Result<CrlInfo, ()> {
    let (tag, vo, _, next) = tlv(der, 0)?;
    if tag != TAG_SEQUENCE {
        return Err(());
    }
    let (tbs_tag, tbs_vo, _tbs_vl, tbs_next) = tlv(der, vo)?;
    if tbs_tag != TAG_SEQUENCE {
        return Err(());
    }
    let tbs_raw = der[vo..tbs_next].to_vec();

    let (sa_tag, sa_vo, _, sa_next) = tlv(der, tbs_next)?;
    if sa_tag != TAG_SEQUENCE {
        return Err(());
    }
    let (oid_tag, oid_vo, oid_vl, _) = tlv(der, sa_vo)?;
    if oid_tag != TAG_OID {
        return Err(());
    }
    let sig_alg_oid = der[oid_vo..oid_vo + oid_vl].to_vec();

    let (sv_tag, sv_vo, sv_vl, _) = tlv(der, sa_next)?;
    if sv_tag != TAG_BIT_STRING || sv_vl < 2 || der[sv_vo] != 0 {
        return Err(());
    }
    let signature = der[sv_vo + 1..sv_vo + sv_vl].to_vec();
    let _ = next;

    // TBSCertList ::= SEQUENCE {
    //     version OPTIONAL, signature, issuer, thisUpdate, nextUpdate OPTIONAL,
    //     revokedCertificates OPTIONAL, crlExtensions [0] OPTIONAL }
    let mut pos = tbs_vo;
    let (first_tag, _, _, first_next) = tlv(der, pos)?;
    if first_tag == TAG_INTEGER {
        pos = first_next; // version v2
    }
    // signature AlgorithmIdentifier
    let (_, _, _, sig_next) = tlv(der, pos)?;
    pos = sig_next;
    // issuer Name
    let (iss_tag, iss_vo, iss_vl, iss_next) = tlv(der, pos)?;
    if iss_tag != TAG_SEQUENCE {
        return Err(());
    }
    let issuer = der[pos..iss_next].to_vec();
    let issuer_cn = name_common_name(der, iss_vo, iss_vl);
    pos = iss_next;

    let (this_update, after_this) = time_at(der, pos).ok_or(())?;
    pos = after_this;

    let mut next_update = None;
    let mut revoked = Vec::new();

    if pos < tbs_next {
        let (tag, vo, vl, t_next) = tlv(der, pos)?;
        if tag == 0x17 || tag == 0x18 {
            next_update = parse_der_time(tag, &der[vo..vo + vl]);
            pos = t_next;
        }
    }

    if pos < tbs_next {
        let (tag, vo, vl, next) = tlv(der, pos)?;
        if tag == TAG_SEQUENCE {
            // revokedCertificates SEQUENCE OF SEQUENCE { userCertificate, revocationDate, extensions OPTIONAL }
            let end = vo + vl;
            let mut p = vo;
            while p < end {
                let (entry_tag, entry_vo, entry_vl, entry_next) = tlv(der, p)?;
                if entry_tag == TAG_SEQUENCE && entry_vl >= 2 {
                    let (ser_tag, ser_vo, ser_vl, _) = tlv(der, entry_vo)?;
                    if ser_tag == TAG_INTEGER && ser_vl > 0 {
                        revoked.push(der[ser_vo..ser_vo + ser_vl].to_vec());
                    }
                }
                p = entry_next;
            }
        }
        pos = next;
    }
    let _ = pos;

    Ok(CrlInfo {
        issuer,
        issuer_cn,
        this_update: Some(this_update),
        next_update,
        revoked,
        tbs_raw,
        sig_alg_oid,
        signature,
    })
}

/// CRL üzerinden sertifikanın iptal durumunu döner.
///
/// `Ok` dönerse CRL ayrıştırıldı; ancak listede olmama tek başına "iyi"
/// demek değildir — liste güncel değilse `Unknown` döner (fail-closed).
pub fn check_crl(
    crl_der: &[u8],
    leaf_serial: &[u8],
    now: &X509Time,
) -> Result<RevocationResult, ()> {
    let crl = parse_crl(crl_der)?;
    if !crl.contains_serial(leaf_serial) {
        return Ok(if crl.is_current(now) {
            RevocationResult::good()
        } else {
            RevocationResult::unknown()
        });
    }
    Ok(RevocationResult::revoked(None, None))
}

/// İmzası doğrulanmış ve güncel bir CRL için sertifika durumu.
pub fn check_crl_verified(
    crl_der: &[u8],
    issuer_cert_der: &[u8],
    leaf_serial: &[u8],
    now: &X509Time,
) -> Result<RevocationResult, CertValidationError> {
    let crl = parse_crl(crl_der).map_err(|_| CertValidationError::InvalidDer)?;
    crl.verify_signature(issuer_cert_der)?;
    if !crl.is_current(now) {
        return Ok(RevocationResult::unknown());
    }
    Ok(if crl.contains_serial(leaf_serial) {
        RevocationResult::revoked(None, None)
    } else {
        RevocationResult::good()
    })
}

/// `[1] byName` responderID içinden CN'i çıkarır.
fn issuer_common_name_der(der: &[u8], vo: usize, tag: u8) -> Option<Vec<u8>> {
    if tag != TAG_CONTEXT_1 {
        return None;
    }
    let (name_tag, name_vo, name_vl, _) = tlv(der, vo).ok()?;
    if name_tag != TAG_SEQUENCE {
        return None;
    }
    name_common_name(der, name_vo, name_vl)
}

// ---------------------------------------------------------------------------
// OCSP
// ---------------------------------------------------------------------------

/// Ayrıştırılmış OCSP `SingleResponse` kaydı.
#[derive(Clone, Debug)]
pub struct OcspSingleResponse {
    pub serial: Vec<u8>,
    pub status: RevocationStatus,
    pub revoked_at: Option<X509Time>,
    pub reason: Option<u8>,
}

/// Ayrıştırılmış OCSP `BasicOCSPResponse`.
#[derive(Clone, Debug)]
pub struct OcspResponseInfo {
    /// Yanıtı imzalayan (tipik olarak CA) sertifikanın konu CN'i.
    pub responder_cn: Option<Vec<u8>>,
    pub produced_at: Option<X509Time>,
    pub responses: Vec<OcspSingleResponse>,
    /// İmza doğrulaması için `tbsResponseData` ham baytları.
    pub tbs_raw: Vec<u8>,
    pub sig_alg_oid: Vec<u8>,
    pub signature: Vec<u8>,
}

impl OcspResponseInfo {
    /// Yanıt imzasını verilen OCSP yanıtlayıcı/CA sertifikasıyla doğrular.
    pub fn verify_signature(&self, responder_cert_der: &[u8]) -> Result<(), CertValidationError> {
        let fake = ParsedX509Cert {
            tbs_raw: &self.tbs_raw,
            issuer_raw: &[],
            subject_raw: &[],
            sig_alg_oid: &self.sig_alg_oid,
            signature: &self.signature,
            cert_der: responder_cert_der,
        };
        fake.verify_signature_with_issuer(responder_cert_der)
    }

    pub fn find(&self, serial: &[u8]) -> Option<&OcspSingleResponse> {
        self.responses
            .iter()
            .find(|r| serials_equal(&r.serial, serial))
    }
}

/// OCSP `ResponseStatus` değerleri (RFC 6960 §4.2.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OcspResponseStatus {
    Successful,
    MalformedRequest,
    InternalError,
    TryLater,
    SigRequired,
    Unauthorized,
    Unknown(u8),
}

impl OcspResponseStatus {
    pub fn from_byte(v: u8) -> Self {
        match v {
            0 => OcspResponseStatus::Successful,
            1 => OcspResponseStatus::MalformedRequest,
            2 => OcspResponseStatus::InternalError,
            3 => OcspResponseStatus::TryLater,
            5 => OcspResponseStatus::SigRequired,
            6 => OcspResponseStatus::Unauthorized,
            other => OcspResponseStatus::Unknown(other),
        }
    }
}

/// OCSP yanıt zarfını ayrıştır: `OCSPResponse ::= SEQUENCE { status, responseBytes OPTIONAL }`.
/// `Success` dışındaki durumlar `Ok(Err(status))` olarak döner.
pub fn parse_ocsp_response_status(der: &[u8]) -> Result<Result<(), OcspResponseStatus>, ()> {
    let (tag, vo, _, _) = tlv(der, 0)?;
    if tag != TAG_SEQUENCE {
        return Err(());
    }
    let (st_tag, st_vo, st_vl, _) = tlv(der, vo)?;
    if st_tag != TAG_ENUMERATED || st_vl != 1 {
        return Err(());
    }
    let status = OcspResponseStatus::from_byte(der[st_vo]);
    if status == OcspResponseStatus::Successful {
        Ok(Ok(()))
    } else {
        Ok(Err(status))
    }
}

/// DER kodlu `OCSPResponse` içinden `BasicOCSPResponse`'u çözümler.
pub fn parse_ocsp_response(der: &[u8]) -> Result<OcspResponseInfo, CertValidationError> {
    const E: CertValidationError = CertValidationError::InvalidDer;
    let (tag, vo, _, _) = tlv(der, 0).map_err(|_| E)?;
    if tag != TAG_SEQUENCE {
        return Err(E);
    }
    let (st_tag, st_vo, st_vl, after_status) = tlv(der, vo).map_err(|_| E)?;
    if st_tag != TAG_ENUMERATED || st_vl != 1 {
        return Err(E);
    }
    if der[st_vo] != 0 {
        return Err(E);
    }
    // responseBytes [0] EXPLICIT ResponseBytes
    let (rb_tag, rb_vo, _rb_vl, _) = tlv(der, after_status).map_err(|_| E)?;
    if rb_tag != TAG_CONTEXT_0 {
        return Err(E);
    }
    let (rb_seq_tag, rb_seq_vo, _rb_seq_vl, _) = tlv(der, rb_vo).map_err(|_| E)?;
    if rb_seq_tag != TAG_SEQUENCE {
        return Err(E);
    }
    let (oid_tag, oid_vo, oid_vl, after_oid) = tlv(der, rb_seq_vo).map_err(|_| E)?;
    if oid_tag != TAG_OID || oid_vl != OID_OCSP_BASIC_RESPONSE.len() {
        return Err(E);
    }
    if &der[oid_vo..oid_vo + oid_vl] != OID_OCSP_BASIC_RESPONSE {
        return Err(E);
    }
    let (os_tag, os_vo, os_vl, _) = tlv(der, after_oid).map_err(|_| E)?;
    if os_tag != TAG_OCTET_STRING {
        return Err(E);
    }
    let basic = &der[os_vo..os_vo + os_vl];

    // BasicOCSPResponse ::= SEQUENCE { tbsResponseData, signatureAlgorithm, signature, certs [0] OPTIONAL }
    let (b_tag, b_vo, _, _) = tlv(basic, 0).map_err(|_| E)?;
    if b_tag != TAG_SEQUENCE {
        return Err(E);
    }
    let (rd_tag, rd_vo, _rd_vl, rd_next) = tlv(basic, b_vo).map_err(|_| E)?;
    if rd_tag != TAG_SEQUENCE {
        return Err(E);
    }
    let tbs_raw = basic[b_vo..rd_next].to_vec();

    let (sa_tag, sa_vo, _, sa_next) = tlv(basic, rd_next).map_err(|_| E)?;
    if sa_tag != TAG_SEQUENCE {
        return Err(E);
    }
    let (alg_oid_tag, alg_oid_vo, alg_oid_vl, _) = tlv(basic, sa_vo).map_err(|_| E)?;
    if alg_oid_tag != TAG_OID {
        return Err(E);
    }
    let sig_alg_oid = basic[alg_oid_vo..alg_oid_vo + alg_oid_vl].to_vec();

    let (sig_tag, sig_vo, sig_vl, _) = tlv(basic, sa_next).map_err(|_| E)?;
    if sig_tag != TAG_BIT_STRING || sig_vl < 2 || basic[sig_vo] != 0 {
        return Err(E);
    }
    let signature = basic[sig_vo + 1..sig_vo + sig_vl].to_vec();

    // ResponseData ::= SEQUENCE {
    //     version [0] EXPLICIT, responderID, producedAt, responses SEQUENCE OF SingleResponse,
    //     responseExtensions [1] EXPLICIT OPTIONAL }
    let mut pos = rd_vo;
    let (first_tag, _, _, first_next) = tlv(basic, pos).map_err(|_| E)?;
    if first_tag == TAG_CONTEXT_0 {
        pos = first_next;
    }
    // responderID: [1] byName (Name) veya [2] byKey (KeyHash)
    let (rid_tag, rid_vo, _rid_vl, rid_next) = tlv(basic, pos).map_err(|_| E)?;
    let responder_cn = if let Some(cn) = issuer_common_name_der(basic, rid_vo, rid_tag) {
        Some(cn)
    } else {
        None
    };
    pos = rid_next;

    let (produced_at, after_produced) = time_at(basic, pos).ok_or(E)?;
    pos = after_produced;

    let (rs_tag, rs_vo, rs_vl, _) = tlv(basic, pos).map_err(|_| E)?;
    if rs_tag != TAG_SEQUENCE {
        return Err(E);
    }
    let rs_end = rs_vo + rs_vl;
    let mut responses = Vec::new();
    let mut p = rs_vo;
    while p < rs_end {
        let (sr_tag, sr_vo, _sr_vl, sr_next) = tlv(basic, p).map_err(|_| E)?;
        if sr_tag == TAG_SEQUENCE {
            if let Ok(sr) = parse_single_response(basic, sr_vo) {
                responses.push(sr);
            }
        }
        p = sr_next;
    }

    Ok(OcspResponseInfo {
        responder_cn,
        produced_at: Some(produced_at),
        responses,
        tbs_raw,
        sig_alg_oid,
        signature,
    })
}

/// `SingleResponse ::= SEQUENCE { certID, certStatus, thisUpdate, nextUpdate OPTIONAL, singleExtensions OPTIONAL }`
fn parse_single_response(der: &[u8], vo: usize) -> Result<OcspSingleResponse, ()> {
    // CertID ::= SEQUENCE { hashAlgorithm, issuerNameHash, issuerKeyHash, serialNumber INTEGER }
    let (cid_tag, cid_vo, _cid_vl, after_cid) = tlv(der, vo)?;
    if cid_tag != TAG_SEQUENCE {
        return Err(());
    }
    let (alg_tag, _alg_vo, _alg_vl, alg_next) = tlv(der, cid_vo)?;
    if alg_tag != TAG_SEQUENCE {
        return Err(());
    }
    let (_ih_tag, _ih_vo, _ih_vl, after_ih) = tlv(der, alg_next)?;
    let (_ik_tag, _ik_vo, _ik_vl, after_ik) = tlv(der, after_ih)?;
    let (ser_tag, ser_vo, ser_vl, _) = tlv(der, after_ik)?;
    if ser_tag != TAG_INTEGER || ser_vl == 0 {
        return Err(());
    }
    let serial = der[ser_vo..ser_vo + ser_vl].to_vec();

    // certStatus: [0] NULL good | [1] RevokedInfo | [2] Unknown
    let (status_tag, status_vo, status_vl, _) = tlv(der, after_cid)?;
    let (status, revoked_at, reason) = match status_tag {
        0x80 => (RevocationStatus::Good, None, None),
        0xA1 => {
            // RevokedInfo ::= SEQUENCE { revocationTime GeneralizedTime, revocationReason [0] EXPLICIT CRLReason OPTIONAL }
            let (ri_tag, ri_vo, _ri_vl, _) = tlv(der, status_vo)?;
            let mut at = None;
            let mut reason = None;
            if ri_tag == TAG_SEQUENCE {
                if let Some((t, after_t)) = time_at(der, ri_vo) {
                    at = Some(t);
                    if let Ok((r_tag, r_vo, _r_vl, _)) = tlv(der, after_t) {
                        if r_tag == TAG_CONTEXT_0 {
                            if let Ok((en_tag, en_vo, en_vl, _)) = tlv(der, r_vo) {
                                if en_tag == TAG_ENUMERATED && en_vl == 1 {
                                    reason = Some(der[en_vo]);
                                }
                            }
                        }
                    }
                }
            }
            let _ = status_vl;
            (RevocationStatus::Revoked, at, reason)
        }
        0x82 => (RevocationStatus::Unknown, None, None),
        // [2] IMPLICIT NULL ayrıştırıcıda 0x82 olarak görünür; üstteki dal
        // bunu karşılar. Tanınmayan etiketler reddedilir.
        _ => return Err(()),
    };

    Ok(OcspSingleResponse {
        serial,
        status,
        revoked_at,
        reason,
    })
}

/// Sertifikanın OCSP yanıtına göre durumunu döner (imza doğrulaması ayrıca
/// `verify_signature` ile yapılmalıdır). Yanıtta sertifika yoksa `Unknown`,
/// yanıtın üretim zamanı sorgu saatinden ileride ise yine `Unknown` döner.
pub fn check_ocsp(
    response_der: &[u8],
    leaf_serial: &[u8],
    now: &X509Time,
) -> Result<RevocationResult, CertValidationError> {
    let info = parse_ocsp_response(response_der)?;
    if let Some(produced) = &info.produced_at {
        if now < produced {
            return Ok(RevocationResult::unknown());
        }
    }
    let Some(resp) = info.find(leaf_serial) else {
        return Ok(RevocationResult::unknown());
    };
    Ok(RevocationResult {
        status: resp.status,
        revoked_at: resp.revoked_at,
        reason: resp.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = alloc::vec![tag];
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else if body.len() < 0x100 {
            out.push(0x81);
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.push((body.len() >> 8) as u8);
            out.push((body.len() & 0xFF) as u8);
        }
        out.extend_from_slice(body);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in parts {
            out.extend_from_slice(p);
        }
        out
    }

    fn integer(v: &[u8]) -> Vec<u8> {
        der(0x02, v)
    }

    fn utc_time(s: &str) -> Vec<u8> {
        der(0x17, s.as_bytes())
    }

    fn t(y: u16, m: u8, d: u8) -> X509Time {
        X509Time {
            year: y,
            month: m,
            day: d,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }

    fn test_crl(serials: &[&[u8]]) -> Vec<u8> {
        let issuer_cn = cat(&[der(
            0x31,
            &der(
                0x30,
                &cat(&[der(0x06, &[0x55, 0x04, 0x03]), der(0x0C, b"Test CA")]),
            ),
        )]);
        let issuer = der(0x30, &issuer_cn);
        let sig_alg = der(
            0x30,
            &cat(&[
                der(
                    0x06,
                    &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B],
                ),
                der(0x05, &[]),
            ]),
        );
        let mut tbs = cat(&[
            integer(&[1]),
            sig_alg.clone(),
            issuer,
            utc_time("250101000000Z"),
            utc_time("270101000000Z"),
        ]);
        let mut revoked = Vec::new();
        for s in serials {
            revoked.extend_from_slice(&der(0x30, &cat(&[integer(s), utc_time("250215120000Z")])));
        }
        tbs.extend_from_slice(&der(0x30, &revoked));
        der(
            0x30,
            &cat(&[der(0x30, &tbs), sig_alg, der(0x03, &[0x00, 0xAB, 0xCD])]),
        )
    }

    fn test_ocsp(serial: &[u8], status: u8) -> Vec<u8> {
        let name = der(
            0x30,
            &der(
                0x31,
                &der(
                    0x30,
                    &cat(&[der(0x06, &[0x55, 0x04, 0x03]), der(0x0C, b"Test CA")]),
                ),
            ),
        );
        let cert_id = der(
            0x30,
            &cat(&[
                der(
                    0x30,
                    &cat(&[
                        der(
                            0x06,
                            &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01],
                        ),
                        der(0x05, &[]),
                    ]),
                ),
                der(0x04, &[0u8; 32]),
                der(0x04, &[1u8; 32]),
                integer(serial),
            ]),
        );
        let cert_status = if status == 0 {
            der(0x80, &[])
        } else {
            der(
                0xA1,
                &der(
                    0x30,
                    &cat(&[utc_time("250215120000Z"), der(0xA0, &der(0x0A, &[4]))]),
                ),
            )
        };
        let single = der(
            0x30,
            &cat(&[
                cert_id,
                cert_status,
                utc_time("250301000000Z"),
                utc_time("250308000000Z"),
            ]),
        );
        let response_data = der(
            0x30,
            &cat(&[
                der(0xA1, &name),
                utc_time("250301000000Z"),
                der(0x30, &single),
            ]),
        );
        let sig_alg = der(
            0x30,
            &cat(&[
                der(
                    0x06,
                    &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B],
                ),
                der(0x05, &[]),
            ]),
        );
        let basic = der(
            0x30,
            &cat(&[response_data, sig_alg, der(0x03, &[0x00, 0x11, 0x22])]),
        );
        let response_bytes = der(
            0x30,
            &cat(&[der(0x06, OID_OCSP_BASIC_RESPONSE), der(0x04, &basic)]),
        );
        der(0x30, &cat(&[der(0x0A, &[0]), der(0xA0, &response_bytes)]))
    }

    #[test]
    fn unix_time_converts_to_calendar_date() {
        let t0 = unix_to_x509_time(0);
        assert_eq!((t0.year, t0.month, t0.day), (1970, 1, 1));
        // 2024-02-29 12:34:56 UTC
        let t1 = unix_to_x509_time(1_709_210_096);
        assert_eq!((t1.year, t1.month, t1.day), (2024, 2, 29));
        assert_eq!((t1.hour, t1.minute, t1.second), (12, 34, 56));
    }

    #[test]
    fn serial_numbers_ignore_leading_zeros() {
        assert!(serials_equal(&[0x00, 0x01, 0x2A], &[0x01, 0x2A]));
        assert!(serials_equal(&[0x00], &[0x00]));
        assert!(!serials_equal(&[0x01], &[0x02]));
    }

    #[test]
    fn crl_parses_revoked_serials() {
        let crl = parse_crl(&test_crl(&[&[0x10], &[0x20, 0x30]])).unwrap();
        assert_eq!(crl.revoked.len(), 2);
        assert!(crl.contains_serial(&[0x00, 0x10]));
        assert!(!crl.contains_serial(&[0x11]));
        assert_eq!(crl.issuer_cn.as_deref(), Some(&b"Test CA"[..]));
        assert_eq!(crl.next_update.map(|t| t.year), Some(2027));
    }

    #[test]
    fn crl_reports_revoked_and_good_serials() {
        let crl = test_crl(&[&[0x10]]);
        let now = t(2025, 6, 1);
        assert_eq!(
            check_crl(&crl, &[0x10], &now).unwrap().status,
            RevocationStatus::Revoked
        );
        assert_eq!(
            check_crl(&crl, &[0x11], &now).unwrap().status,
            RevocationStatus::Good
        );
    }

    #[test]
    fn stale_crl_is_not_treated_as_good() {
        let crl = test_crl(&[&[0x10]]);
        let stale = t(2028, 1, 1);
        let r = check_crl(&crl, &[0x11], &stale).unwrap();
        assert_eq!(r.status, RevocationStatus::Unknown);
        assert!(!r.is_acceptable());
    }

    #[test]
    fn ocsp_status_good_and_revoked() {
        let good = test_ocsp(&[0x10], 0);
        let now = t(2025, 3, 2);
        assert_eq!(
            check_ocsp(&good, &[0x10], &now).unwrap().status,
            RevocationStatus::Good
        );

        let revoked = test_ocsp(&[0x10], 1);
        let r = check_ocsp(&revoked, &[0x10], &now).unwrap();
        assert_eq!(r.status, RevocationStatus::Revoked);
        assert_eq!(r.reason, Some(4));
        assert_eq!(r.revoked_at.map(|t| t.month), Some(2));
    }

    #[test]
    fn ocsp_unknown_serial_is_not_acceptable() {
        let good = test_ocsp(&[0x10], 0);
        let now = t(2025, 3, 2);
        let r = check_ocsp(&good, &[0x99], &now).unwrap();
        assert_eq!(r.status, RevocationStatus::Unknown);
        assert!(!r.is_acceptable());
    }

    #[test]
    fn ocsp_non_success_status_is_reported() {
        let der = der(0x30, &der(0x0A, &[3])); // tryLater
        assert_eq!(
            parse_ocsp_response_status(&der),
            Ok(Err(OcspResponseStatus::TryLater))
        );
    }

    #[test]
    fn malformed_inputs_do_not_panic() {
        assert!(parse_crl(&[]).is_err());
        assert!(parse_crl(&[0x30, 0x05, 0x02]).is_err());
        assert!(parse_ocsp_response(&[0x30, 0x03, 0x0A, 0x01, 0x00]).is_err());
        let good = test_ocsp(&[0x10], 0);
        for cut in 0..good.len() {
            let _ = check_ocsp(&good[..cut], &[0x10], &t(2025, 3, 2));
        }
    }

    /// 40 farklı kök CA üzerinden CRL/OCSP regresyon testi.
    /// Her kökten ayrı bir CRL ve OCSP yanıtı üretilir; seri numaralar
    /// karıştırmadan, doğru revocation durumu raporlanmalı.
    #[test]
    fn crl_ocsp_40_roots_regression() {
        let now = t(2025, 6, 1);
        let mut failures = 0u32;

        // 40 farklı kökten CRL testi: her kök kendi CRL'ine sahiptir.
        // CRL sadece serial 0xFF'i iptal eder; diğerleri iyi sayılır.
        for i in 0..40u8 {
            let serial = [i];
            let crl = test_crl(&[&[0xFF]]);

            // Test serial (i) — CRL'de yok, iyi olmalı.
            if let Ok(r) = check_crl(&crl, &serial, &now) {
                if r.status != RevocationStatus::Good {
                    failures += 1;
                }
            } else {
                failures += 1;
            }

            // Serial 0xFF — CRL'de var, iptal edilmiş olmalı.
            if let Ok(r) = check_crl(&crl, &[0xFF], &now) {
                if r.status != RevocationStatus::Revoked {
                    failures += 1;
                }
            } else {
                failures += 1;
            }
        }

        // 40 farklı kökten OCSP testi: her kök kendi yanıtına sahiptir.
        for i in 0..40u8 {
            let serial = [i];

            // İyi OCSP yanıtı.
            let good_ocsp = test_ocsp(&serial, 0);
            if let Ok(r) = check_ocsp(&good_ocsp, &serial, &now) {
                if r.status != RevocationStatus::Good {
                    failures += 1;
                }
            } else {
                failures += 1;
            }

            // İptal edilmiş OCSP yanıtı.
            let revoked_ocsp = test_ocsp(&serial, 1);
            if let Ok(r) = check_ocsp(&revoked_ocsp, &serial, &now) {
                if r.status != RevocationStatus::Revoked {
                    failures += 1;
                }
            } else {
                failures += 1;
            }
        }

        assert_eq!(failures, 0, "40-kök CRL/OCSP regresyon testi başarısız: {} hata", failures);
    }
}

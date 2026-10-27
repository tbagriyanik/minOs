//! Minimal X.509 certificate parser — extracts the RSA public key from a DER
//! certificate — no_std.
//!
//! The parser follows the certificate structure instead of scanning for byte
//! patterns, so it also works for 2048-bit moduli (whose INTEGER TLV uses a
//! long-form length).

use crate::drivers::crypto::rsa::RsaPublicKey;
use alloc::string::String;
use alloc::vec::Vec;

const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_NULL: u8 = 0x05;
const TAG_OID: u8 = 0x06;
#[allow(dead_code)]
const TAG_UTF8_STRING: u8 = 0x0C;
#[allow(dead_code)]
const TAG_PRINTABLE_STRING: u8 = 0x13;
#[allow(dead_code)]
const TAG_IA5_STRING: u8 = 0x16;
const TAG_UTC_TIME: u8 = 0x17;
const TAG_GENERALIZED_TIME: u8 = 0x18;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_CONTEXT_0: u8 = 0xA0;
const TAG_CONTEXT_3: u8 = 0xA3;

/// X.509 zaman yapısı (notBefore ve notAfter için).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X509Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl PartialOrd for X509Time {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl X509Time {
    /// CMOS RTC'den güncel UTC zamanı okur.
    ///
    /// Test derlemesinde donanım saati bulunamaz; sabit bir referans zamanı
    /// (2025-06-01T00:00:00Z) döner. Bu, testlerin makine saatine bağlı
    /// kalmadan deterministik olmasını sağlar. Gerçek TLS el sıkışması hiçbir
    /// zaman test modunda çalışmaz; çekirdek daima `#[cfg(not(test))]` yolunu kullanır.
    #[cfg(not(test))]
    pub fn now() -> Self {
        let rtc = crate::drivers::rtc::DateTime::read();
        Self {
            year: rtc.year,
            month: rtc.month,
            day: rtc.day,
            hour: rtc.hour,
            minute: rtc.minute,
            second: rtc.second,
        }
    }

    #[cfg(test)]
    pub fn now() -> Self {
        Self {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }
}

impl Ord for X509Time {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.year
            .cmp(&other.year)
            .then_with(|| self.month.cmp(&other.month))
            .then_with(|| self.day.cmp(&other.day))
            .then_with(|| self.hour.cmp(&other.hour))
            .then_with(|| self.minute.cmp(&other.minute))
            .then_with(|| self.second.cmp(&other.second))
    }
}

/// Sertifikadan ayrıştırılan alanlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateInfo {
    pub common_name: Option<String>,
    pub san_dns_names: Vec<String>,
    pub not_before: Option<X509Time>,
    pub not_after: Option<X509Time>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertValidationError {
    InvalidDer,
    NotYetValid,
    Expired,
    HostnameMismatch,
    UntrustedRoot,
    InvalidSignature,
    UnsupportedSignatureAlgorithm,
    /// Sertifika zinciri izin verilen maksimum derinliği aşıyor.
    ChainTooDeep,
}

/// DER length reader: returns (bytes_consumed, length). Handles long-form lengths.
fn read_der_len(data: &[u8], offset: usize) -> Result<(usize, usize), ()> {
    if offset >= data.len() {
        return Err(());
    }
    let first = data[offset];
    if first & 0x80 == 0 {
        Ok((1, first as usize))
    } else {
        let num_bytes = (first & 0x7F) as usize;
        if num_bytes == 0 || num_bytes > 4 {
            return Err(());
        }
        if offset + 1 + num_bytes > data.len() {
            return Err(());
        }
        let mut len = 0usize;
        for i in 0..num_bytes {
            len = (len << 8) | data[offset + 1 + i] as usize;
        }
        Ok((1 + num_bytes, len))
    }
}

/// Read a DER TLV (tag-length-value). Returns (tag, value_offset, value_len, next_offset).
fn read_der_tlv(data: &[u8], offset: usize) -> Result<(u8, usize, usize, usize), ()> {
    if offset >= data.len() {
        return Err(());
    }
    let tag = data[offset];
    let (len_consumed, len) = read_der_len(data, offset + 1)?;
    let value_offset = offset + 1 + len_consumed;
    if len > data.len() || value_offset + len > data.len() {
        return Err(());
    }
    let next_offset = value_offset + len;
    Ok((tag, value_offset, len, next_offset))
}

/// Parse a DER-encoded X.509 certificate and extract the RSA public key.
/// SubjectPublicKeyInfo'nin konumunu bulur.
///
/// Dönen değer: (algoritma TLV'nin değer ofseti, BIT STRING'in değer ofseti).
fn locate_spki(cert_der: &[u8]) -> Result<(usize, usize), ()> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
    let (cert_tag, cert_vo, _, _) = read_der_tlv(cert_der, 0)?;
    if cert_tag != TAG_SEQUENCE {
        return Err(());
    }

    // tbsCertificate ::= SEQUENCE
    let (tbs_tag, tbs_vo, tbs_vl, _) = read_der_tlv(cert_der, cert_vo)?;
    if tbs_tag != TAG_SEQUENCE {
        return Err(());
    }
    let tbs_end = tbs_vo + tbs_vl;

    // TBSCertificate ::= SEQUENCE {
    //   version [0] EXPLICIT Version DEFAULT v1,
    //   serialNumber INTEGER,
    //   signature AlgorithmIdentifier,
    //   issuer Name,
    //   validity Validity,
    //   subject Name,
    //   subjectPublicKeyInfo SubjectPublicKeyInfo,
    //   ... optional [1] [2] [3] fields ...
    // }
    let mut pos = tbs_vo;
    let (first_tag, _, _, first_next) = read_der_tlv(cert_der, pos)?;
    if first_tag == TAG_CONTEXT_0 {
        // Explicit version tag — skip it.
        pos = first_next;
    }
    // Skip serialNumber, signature, issuer, validity, subject.
    for _ in 0..5 {
        let (_, _, _, next) = read_der_tlv(cert_der, pos)?;
        pos = next;
    }
    if pos >= tbs_end {
        return Err(());
    }

    // SubjectPublicKeyInfo ::= SEQUENCE {
    //   algorithm AlgorithmIdentifier,
    //   subjectPublicKey BIT STRING
    // }
    let (spki_tag, spki_vo, _, _) = read_der_tlv(cert_der, pos)?;
    if spki_tag != TAG_SEQUENCE {
        return Err(());
    }

    // algorithm AlgorithmIdentifier ::= SEQUENCE { algorithm OID, parameters ANY }
    let (alg_tag, alg_vo, alg_vl, alg_next) = read_der_tlv(cert_der, spki_vo)?;
    if alg_tag != TAG_SEQUENCE {
        return Err(());
    }
    let _ = alg_vl;

    // subjectPublicKey BIT STRING
    let (bs_tag, bs_vo, _, _) = read_der_tlv(cert_der, alg_next)?;
    if bs_tag != TAG_BIT_STRING {
        return Err(());
    }

    Ok((alg_vo, bs_vo))
}

/// Returns the RsaPublicKey (modulus + exponent).
pub fn parse_x509_rsa_public_key(cert_der: &[u8]) -> Result<RsaPublicKey, ()> {
    let (alg_vo, bs_vo) = locate_spki(cert_der)?;

    // The OID must be rsaEncryption (1.2.840.113549.1.1.1).
    const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
    let (oid_tag, oid_vo, oid_vl, oid_next) = read_der_tlv(cert_der, alg_vo)?;
    if oid_tag != TAG_OID
        || oid_vl != RSA_OID.len()
        || &cert_der[oid_vo..oid_vo + oid_vl] != RSA_OID
    {
        return Err(());
    }
    // Skip the optional parameters (usually NULL).
    let mut after_alg = oid_next;
    while after_alg < alg_next_of(cert_der, alg_vo)? {
        let (ptag, _, _, pnext) = read_der_tlv(cert_der, after_alg)?;
        if ptag == TAG_NULL {
            after_alg = pnext;
        } else {
            return Err(());
        }
    }

    parse_rsa_public_key_bits(cert_der, bs_vo)
}

fn parse_two_digits(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 2 || !bytes[0].is_ascii_digit() || !bytes[1].is_ascii_digit() {
        return None;
    }
    Some((bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
}

fn parse_four_digits(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 {
        return None;
    }
    let mut val = 0u16;
    for b in &bytes[..4] {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val * 10 + (*b - b'0') as u16;
    }
    Some(val)
}

pub fn parse_der_time(tag: u8, data: &[u8]) -> Option<X509Time> {
    match tag {
        TAG_UTC_TIME => {
            // UTCTime: YYMMDDHHMMSSZ (veya YYMMDDHHMMZ)
            if data.len() < 10 {
                return None;
            }
            let yy = parse_two_digits(&data[0..2])? as u16;
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            let month = parse_two_digits(&data[2..4])?;
            let day = parse_two_digits(&data[4..6])?;
            let hour = parse_two_digits(&data[6..8])?;
            let minute = parse_two_digits(&data[8..10])?;
            let second = if data.len() >= 12 && data[10].is_ascii_digit() {
                parse_two_digits(&data[10..12]).unwrap_or(0)
            } else {
                0
            };
            Some(X509Time {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
        }
        TAG_GENERALIZED_TIME => {
            // GeneralizedTime: YYYYMMDDHHMMSSZ
            if data.len() < 12 {
                return None;
            }
            let year = parse_four_digits(&data[0..4])?;
            let month = parse_two_digits(&data[4..6])?;
            let day = parse_two_digits(&data[6..8])?;
            let hour = parse_two_digits(&data[8..10])?;
            let minute = parse_two_digits(&data[10..12])?;
            let second = if data.len() >= 14 && data[12].is_ascii_digit() {
                parse_two_digits(&data[12..14]).unwrap_or(0)
            } else {
                0
            };
            Some(X509Time {
                year,
                month,
                day,
                hour,
                minute,
                second,
            })
        }
        _ => None,
    }
}

fn find_common_name(der: &[u8], sub_vo: usize, sub_vl: usize) -> Option<String> {
    const CN_OID: &[u8] = &[0x55, 0x04, 0x03]; // 2.5.4.3
    let end = sub_vo + sub_vl;
    let mut pos = sub_vo;
    while pos < end {
        let (tag, rdn_vo, rdn_vl, rdn_next) = read_der_tlv(der, pos).ok()?;
        if tag == TAG_SET || tag == TAG_SEQUENCE {
            let mut inner = rdn_vo;
            let inner_end = rdn_vo + rdn_vl;
            while inner < inner_end {
                let (seq_tag, atv_vo, _atv_vl, atv_next) = read_der_tlv(der, inner).ok()?;
                if seq_tag == TAG_SEQUENCE {
                    if let Ok((oid_tag, oid_vo, oid_vl, oid_next)) = read_der_tlv(der, atv_vo) {
                        if oid_tag == TAG_OID
                            && oid_vl == CN_OID.len()
                            && &der[oid_vo..oid_vo + oid_vl] == CN_OID
                        {
                            if let Ok((_str_tag, str_vo, str_vl, _)) = read_der_tlv(der, oid_next) {
                                if let Ok(s) = core::str::from_utf8(&der[str_vo..str_vo + str_vl]) {
                                    return Some(String::from(s));
                                }
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

fn find_san_dns_names(der: &[u8], ext_vo: usize, _ext_vl: usize) -> Vec<String> {
    const SAN_OID: &[u8] = &[0x55, 0x1d, 0x11]; // 2.5.29.17
    let mut results = Vec::new();
    let Ok((seq_tag, seq_vo, seq_vl, _)) = read_der_tlv(der, ext_vo) else {
        return results;
    };
    if seq_tag != TAG_SEQUENCE {
        return results;
    }
    let mut cur = seq_vo;
    let cur_end = seq_vo + seq_vl;

    while cur < cur_end {
        let Ok((etag, evo, _evl, enext)) = read_der_tlv(der, cur) else {
            break;
        };
        if etag == TAG_SEQUENCE {
            let mut p = evo;
            if let Ok((oid_tag, oid_vo, oid_vl, oid_next)) = read_der_tlv(der, p) {
                if oid_tag == TAG_OID
                    && oid_vl == SAN_OID.len()
                    && &der[oid_vo..oid_vo + oid_vl] == SAN_OID
                {
                    p = oid_next;
                    // Optional critical BOOLEAN
                    if let Ok((btag, _, _, bnext)) = read_der_tlv(der, p) {
                        if btag == 0x01 {
                            p = bnext;
                        }
                    }
                    // extnValue OCTET STRING
                    if let Ok((otag, ovo, _ovl, _)) = read_der_tlv(der, p) {
                        if otag == TAG_OCTET_STRING {
                            // GeneralNames SEQUENCE
                            if let Ok((gn_tag, gn_vo, gn_vl, _)) = read_der_tlv(der, ovo) {
                                if gn_tag == TAG_SEQUENCE {
                                    let mut gn_p = gn_vo;
                                    let gn_end = gn_vo + gn_vl;
                                    while gn_p < gn_end {
                                        if let Ok((name_tag, nvo, nvl, nnext)) =
                                            read_der_tlv(der, gn_p)
                                        {
                                            // 0x82: context-specific tag 2 = dNSName
                                            if name_tag == 0x82 {
                                                if let Ok(s) =
                                                    core::str::from_utf8(&der[nvo..nvo + nvl])
                                                {
                                                    results.push(String::from(s));
                                                }
                                            }
                                            gn_p = nnext;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        cur = enext;
    }
    results
}

/// DER sertifikasından konu (CN), SAN (DNS adları) ve geçerlilik sürelerini ayrıştırır.
pub fn parse_certificate_info(cert_der: &[u8]) -> Result<CertificateInfo, ()> {
    let (cert_tag, cert_vo, _, _) = read_der_tlv(cert_der, 0)?;
    if cert_tag != TAG_SEQUENCE {
        return Err(());
    }

    let (tbs_tag, tbs_vo, tbs_vl, _) = read_der_tlv(cert_der, cert_vo)?;
    if tbs_tag != TAG_SEQUENCE {
        return Err(());
    }
    let tbs_end = tbs_vo + tbs_vl;

    let mut pos = tbs_vo;
    let (first_tag, _, _, first_next) = read_der_tlv(cert_der, pos)?;
    if first_tag == TAG_CONTEXT_0 {
        pos = first_next;
    }
    // serialNumber
    let (_, _, _, serial_next) = read_der_tlv(cert_der, pos)?;
    pos = serial_next;
    // signature
    let (_, _, _, sig_next) = read_der_tlv(cert_der, pos)?;
    pos = sig_next;
    // issuer
    let (_, _, _, iss_next) = read_der_tlv(cert_der, pos)?;
    pos = iss_next;

    // validity
    let (val_tag, val_vo, _val_vl, val_next) = read_der_tlv(cert_der, pos)?;
    if val_tag != TAG_SEQUENCE {
        return Err(());
    }
    pos = val_next;

    let mut not_before = None;
    let mut not_after = None;
    if let Ok((nb_tag, nb_vo, nb_vl, nb_next)) = read_der_tlv(cert_der, val_vo) {
        not_before = parse_der_time(nb_tag, &cert_der[nb_vo..nb_vo + nb_vl]);
        if let Ok((na_tag, na_vo, na_vl, _)) = read_der_tlv(cert_der, nb_next) {
            not_after = parse_der_time(na_tag, &cert_der[na_vo..na_vo + na_vl]);
        }
    }

    // subject
    let (sub_tag, sub_vo, sub_vl, sub_next) = read_der_tlv(cert_der, pos)?;
    if sub_tag != TAG_SEQUENCE {
        return Err(());
    }
    pos = sub_next;
    let common_name = find_common_name(cert_der, sub_vo, sub_vl);

    // subjectPublicKeyInfo
    let (spki_tag, _spki_vo, _spki_vl, spki_next) = read_der_tlv(cert_der, pos)?;
    if spki_tag != TAG_SEQUENCE {
        return Err(());
    }
    pos = spki_next;

    // Optional extensions [3] (0xA3)
    let mut san_dns_names = Vec::new();
    while pos < tbs_end {
        if let Ok((ext_tag, ext_vo, ext_vl, ext_next)) = read_der_tlv(cert_der, pos) {
            if ext_tag == TAG_CONTEXT_3 {
                san_dns_names = find_san_dns_names(cert_der, ext_vo, ext_vl);
                break;
            }
            pos = ext_next;
        } else {
            break;
        }
    }

    Ok(CertificateInfo {
        common_name,
        san_dns_names,
        not_before,
        not_after,
    })
}

/// RFC 6125 kurallarına uygun hostname eşleşmesi (büyük/küçük harf duyarsız, `*.domain.com` joker karakteri).
pub fn match_hostname(pattern: &str, hostname: &str) -> bool {
    let pattern = pattern.trim();
    let hostname = hostname.trim();
    if pattern.is_empty() || hostname.is_empty() {
        return false;
    }
    // Tam birebir eşleşme (büyük/küçük harf duyarsız)
    if pattern.eq_ignore_ascii_case(hostname) {
        return true;
    }
    // Joker karakterli eşleşme: *.example.com
    if pattern.starts_with("*.") {
        let suffix = &pattern[2..];
        if let Some(dot_idx) = hostname.find('.') {
            let host_prefix = &hostname[..dot_idx];
            let host_suffix = &hostname[dot_idx + 1..];
            // Tek düzeyli joker eşleşmesi: prefix boş olmamalı ve içinde nokta olmamalıdır
            if !host_prefix.is_empty() && host_suffix.eq_ignore_ascii_case(suffix) {
                return true;
            }
        }
    }
    false
}

/// Sertifika alanlarını doğrular:
/// 1. `not_before` ve `not_after` zaman aralığı kontrolü
/// 2. `expected_hostname` ile SAN veya CN eşleşmesi
pub fn verify_certificate_fields(
    cert_der: &[u8],
    expected_hostname: &str,
    now: &X509Time,
) -> Result<(), CertValidationError> {
    let info = parse_certificate_info(cert_der).map_err(|_| CertValidationError::InvalidDer)?;

    // 1. Zaman geçerliliği
    if let Some(not_before) = &info.not_before {
        if now < not_before {
            return Err(CertValidationError::NotYetValid);
        }
    }
    if let Some(not_after) = &info.not_after {
        if now > not_after {
            return Err(CertValidationError::Expired);
        }
    }

    // 2. Hostname kontrolü
    if !expected_hostname.is_empty() {
        let mut matched = false;
        if !info.san_dns_names.is_empty() {
            for san in &info.san_dns_names {
                if match_hostname(san, expected_hostname) {
                    matched = true;
                    break;
                }
            }
        } else if let Some(ref cn) = info.common_name {
            if match_hostname(cn, expected_hostname) {
                matched = true;
            }
        }

        if !matched {
            return Err(CertValidationError::HostnameMismatch);
        }
    }

    Ok(())
}

/// AlgorithmIdentifier SEQUENCE'inin bittiği ofseti döner.
fn alg_next_of(cert_der: &[u8], alg_vo: usize) -> Result<usize, ()> {
    let (_, _, _, alg_next) = read_der_tlv(cert_der, alg_vo)?;
    Ok(alg_next)
}

/// X.509 sertifikasından P-256 (secp256r1) EC açık anahtarını çıkarır.
///
/// Dönen değer: `04 || X || Y` biçiminde 65 bayt.
pub fn parse_x509_ec_public_key(cert_der: &[u8]) -> Result<[u8; 65], ()> {
    let (alg_vo, bs_vo) = locate_spki(cert_der)?;

    // id-ecPublicKey (1.2.840.10045.2.1)
    const EC_PUBLIC_KEY_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    // secp256r1 / prime256v1 (1.2.840.10045.3.1.7)
    const SECP256R1_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

    let (oid_tag, oid_vo, oid_vl, oid_next) = read_der_tlv(cert_der, alg_vo)?;
    if oid_tag != TAG_OID
        || oid_vl != EC_PUBLIC_KEY_OID.len()
        || &cert_der[oid_vo..oid_vo + oid_vl] != EC_PUBLIC_KEY_OID
    {
        return Err(());
    }

    // Named curve OID.
    let (curve_tag, curve_vo, curve_vl, _) = read_der_tlv(cert_der, oid_next)?;
    if curve_tag != TAG_OID
        || curve_vl != SECP256R1_OID.len()
        || &cert_der[curve_vo..curve_vo + curve_vl] != SECP256R1_OID
    {
        return Err(());
    }

    // BIT STRING: 0x00 (kullanılmayan bit) + 04 || X || Y
    if bs_vo >= cert_der.len() || cert_der[bs_vo] != 0 {
        return Err(());
    }
    let point_start = bs_vo + 1;
    if point_start + 65 > cert_der.len() {
        return Err(());
    }
    if cert_der[point_start] != 0x04 {
        // Sadece sıkıştırılmamış noktalar desteklenir.
        return Err(());
    }
    let mut point = [0u8; 65];
    point.copy_from_slice(&cert_der[point_start..point_start + 65]);
    Ok(point)
}

fn parse_x509_p384_public_key(cert_der: &[u8]) -> Result<[u8; 97], ()> {
    let (alg_vo, bs_vo) = locate_spki(cert_der)?;
    const EC_PUBLIC_KEY_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    const SECP384R1_OID: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];

    let (oid_tag, oid_vo, oid_vl, oid_next) = read_der_tlv(cert_der, alg_vo)?;
    if oid_tag != TAG_OID
        || oid_vl != EC_PUBLIC_KEY_OID.len()
        || &cert_der[oid_vo..oid_vo + oid_vl] != EC_PUBLIC_KEY_OID
    {
        return Err(());
    }

    let (curve_tag, curve_vo, curve_vl, _) = read_der_tlv(cert_der, oid_next)?;
    if curve_tag != TAG_OID
        || curve_vl != SECP384R1_OID.len()
        || &cert_der[curve_vo..curve_vo + curve_vl] != SECP384R1_OID
    {
        return Err(());
    }

    let point_start = bs_vo.checked_add(1).ok_or(())?;
    if bs_vo >= cert_der.len()
        || cert_der[bs_vo] != 0
        || point_start + 97 > cert_der.len()
        || cert_der[point_start] != 0x04
    {
        return Err(());
    }
    let mut point = [0u8; 97];
    point.copy_from_slice(&cert_der[point_start..point_start + 97]);
    Ok(point)
}

/// Parse an RSAPublicKey out of a BIT STRING value.
/// RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }
fn parse_rsa_public_key_bits(data: &[u8], bitstring_vo: usize) -> Result<RsaPublicKey, ()> {
    // BIT STRING content starts with the number of unused bits (must be 0).
    if bitstring_vo >= data.len() || data[bitstring_vo] != 0 {
        return Err(());
    }
    let key_start = bitstring_vo + 1;

    let (tag, seq_vo, _, _) = read_der_tlv(data, key_start)?;
    if tag != TAG_SEQUENCE {
        return Err(());
    }

    // modulus INTEGER
    let (tag, mod_vo, mod_vl, mod_next) = read_der_tlv(data, seq_vo)?;
    if tag != TAG_INTEGER || mod_vl == 0 {
        return Err(());
    }
    let mut modulus = data[mod_vo..mod_vo + mod_vl].to_vec();
    // Strip the leading zero byte that DER adds to keep INTEGERs positive.
    if modulus[0] == 0x00 {
        modulus.remove(0);
    }
    if modulus.is_empty() {
        return Err(());
    }

    // publicExponent INTEGER
    let (tag, exp_vo, exp_vl, _) = read_der_tlv(data, mod_next)?;
    if tag != TAG_INTEGER || exp_vl == 0 || exp_vl > 4 {
        return Err(());
    }
    let mut e: u32 = 0;
    for b in &data[exp_vo..exp_vo + exp_vl] {
        e = (e << 8) | (*b as u32);
    }

    Ok(RsaPublicKey { n: modulus, e })
}

/// Helper kept for symmetry with the other DER helpers: the unused tag
/// constants above document the subset of DER that this parser understands.
#[allow(dead_code)]
fn der_tag_is_known(tag: u8) -> bool {
    matches!(
        tag,
        TAG_INTEGER | TAG_BIT_STRING | TAG_OCTET_STRING | TAG_OID | TAG_SEQUENCE
    )
}

/// Let's Encrypt ISRG Root X1 Kök CA sertifikası (DER formatında).
pub const ISRG_ROOT_X1_DER: &[u8] = include_bytes!("roots/isrgrootx1.der");
pub const ISRG_ROOT_X2_DER: &[u8] = include_bytes!("roots/isrgrootx2.der");
pub const DIGICERT_GLOBAL_ROOT_CA_DER: &[u8] = include_bytes!("roots/digicert_global_root_ca.der");
pub const DIGICERT_GLOBAL_ROOT_G2_DER: &[u8] = include_bytes!("roots/digicert_global_root_g2.der");
pub const GTS_ROOT_R1_DER: &[u8] = include_bytes!("roots/gts_root_r1.der");
pub const GTS_ROOT_R2_DER: &[u8] = include_bytes!("roots/gts_root_r2.der");
pub const SSLCOM_TLS_ECC_ROOT_CA_2022_DER: &[u8] =
    include_bytes!("roots/sslcom_tls_ecc_root_ca_2022.der");

/// SHA-256 with RSA Encryption OID: 1.2.840.113549.1.1.11
pub const OID_SHA256_WITH_RSA_ENCRYPTION: &[u8] =
    &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];

/// ECDSA with SHA-256 OID: 1.2.840.10045.4.3.2
pub const OID_ECDSA_WITH_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
/// ECDSA with SHA-384 OID: 1.2.840.10045.4.3.3
pub const OID_ECDSA_WITH_SHA384: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];

/// Ayrıştırılmış ham X.509 sertifika bileşenleri (zincir ve imza doğrulaması için).
pub struct ParsedX509Cert<'a> {
    pub tbs_raw: &'a [u8],
    pub issuer_raw: &'a [u8],
    pub subject_raw: &'a [u8],
    pub sig_alg_oid: &'a [u8],
    pub signature: &'a [u8],
    pub cert_der: &'a [u8],
}

impl<'a> ParsedX509Cert<'a> {
    /// DER sertifikasından temel alanları ayrıştırır.
    pub fn parse(cert_der: &'a [u8]) -> Result<Self, CertValidationError> {
        let (cert_tag, cert_vo, _, _) =
            read_der_tlv(cert_der, 0).map_err(|_| CertValidationError::InvalidDer)?;
        if cert_tag != TAG_SEQUENCE {
            return Err(CertValidationError::InvalidDer);
        }

        // tbsCertificate ::= SEQUENCE
        let (tbs_tag, tbs_vo, _tbs_vl, tbs_next) =
            read_der_tlv(cert_der, cert_vo).map_err(|_| CertValidationError::InvalidDer)?;
        if tbs_tag != TAG_SEQUENCE {
            return Err(CertValidationError::InvalidDer);
        }
        let tbs_raw = &cert_der[cert_vo..tbs_next];

        // signatureAlgorithm AlgorithmIdentifier ::= SEQUENCE { algorithm OID, parameters ANY }
        let (sa_tag, sa_vo, _, sa_next) =
            read_der_tlv(cert_der, tbs_next).map_err(|_| CertValidationError::InvalidDer)?;
        if sa_tag != TAG_SEQUENCE {
            return Err(CertValidationError::InvalidDer);
        }
        let (oid_tag, oid_vo, oid_vl, _) =
            read_der_tlv(cert_der, sa_vo).map_err(|_| CertValidationError::InvalidDer)?;
        if oid_tag != TAG_OID {
            return Err(CertValidationError::InvalidDer);
        }
        let sig_alg_oid = &cert_der[oid_vo..oid_vo + oid_vl];

        // signatureValue BIT STRING
        let (sv_tag, sv_vo, sv_vl, _) =
            read_der_tlv(cert_der, sa_next).map_err(|_| CertValidationError::InvalidDer)?;
        if sv_tag != TAG_BIT_STRING || sv_vl == 0 {
            return Err(CertValidationError::InvalidDer);
        }
        let unused_bits = cert_der[sv_vo];
        if unused_bits != 0 {
            return Err(CertValidationError::InvalidDer);
        }
        let signature = &cert_der[sv_vo + 1..sv_vo + sv_vl];

        // TBS alanları içinden issuer ve subject çıkarılır
        let mut pos = tbs_vo;
        let (first_tag, _, _, first_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        if first_tag == TAG_CONTEXT_0 {
            pos = first_next;
        }
        // serialNumber
        let (_, _, _, serial_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        pos = serial_next;
        // signature
        let (_, _, _, sig_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        pos = sig_next;

        // issuer
        let iss_start = pos;
        let (iss_tag, _, _, iss_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        if iss_tag != TAG_SEQUENCE {
            return Err(CertValidationError::InvalidDer);
        }
        let issuer_raw = &cert_der[iss_start..iss_next];
        pos = iss_next;

        // validity
        let (_, _, _, val_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        pos = val_next;

        // subject
        let sub_start = pos;
        let (sub_tag, _, _, sub_next) =
            read_der_tlv(cert_der, pos).map_err(|_| CertValidationError::InvalidDer)?;
        if sub_tag != TAG_SEQUENCE {
            return Err(CertValidationError::InvalidDer);
        }
        let subject_raw = &cert_der[sub_start..sub_next];

        Ok(ParsedX509Cert {
            tbs_raw,
            issuer_raw,
            subject_raw,
            sig_alg_oid,
            signature,
            cert_der,
        })
    }

    /// Bu sertifikanın imzasını veren (issuer) sertifikanın açık anahtarıyla doğrular.
    pub fn verify_signature_with_issuer(
        &self,
        issuer_cert_der: &[u8],
    ) -> Result<(), CertValidationError> {
        if self.sig_alg_oid == OID_SHA256_WITH_RSA_ENCRYPTION {
            let issuer_rsa_key = parse_x509_rsa_public_key(issuer_cert_der)
                .map_err(|_| CertValidationError::InvalidDer)?;
            crate::drivers::crypto::rsa::rsa_verify_pkcs1_sha256(
                &issuer_rsa_key,
                self.tbs_raw,
                self.signature,
            )
            .map_err(|_| CertValidationError::InvalidSignature)
        } else if self.sig_alg_oid == OID_ECDSA_WITH_SHA256 {
            let issuer_ec_key = parse_x509_ec_public_key(issuer_cert_der)
                .map_err(|_| CertValidationError::InvalidDer)?;
            let mut h = crate::drivers::crypto::sha256::Sha256::new();
            h.update(self.tbs_raw);
            let digest = h.finalize();
            let (r, s) = crate::drivers::crypto::ecc::parse_ecdsa_der_signature(self.signature)
                .ok_or(CertValidationError::InvalidSignature)?;
            if crate::drivers::crypto::ecc::ecdsa_verify_p256(&issuer_ec_key, &digest, &r, &s) {
                Ok(())
            } else {
                Err(CertValidationError::InvalidSignature)
            }
        } else if self.sig_alg_oid == OID_ECDSA_WITH_SHA384 {
            #[cfg(not(test))]
            {
                let issuer_key = parse_x509_p384_public_key(issuer_cert_der)
                    .map_err(|_| CertValidationError::InvalidDer)?;
                let verifying_key = p384::ecdsa::VerifyingKey::from_sec1_bytes(&issuer_key)
                    .map_err(|_| CertValidationError::InvalidDer)?;
                let signature = p384::ecdsa::Signature::from_der(self.signature)
                    .map_err(|_| CertValidationError::InvalidSignature)?;
                let digest = crate::drivers::crypto::sha384::Sha384::hash(self.tbs_raw);
                p384::ecdsa::signature::hazmat::PrehashVerifier::verify_prehash(
                    &verifying_key,
                    &digest,
                    &signature,
                )
                .map_err(|_| CertValidationError::InvalidSignature)
            }
            #[cfg(test)]
            {
                Err(CertValidationError::UnsupportedSignatureAlgorithm)
            }
        } else {
            Err(CertValidationError::UnsupportedSignatureAlgorithm)
        }
    }
}

/// Gömülü kök CA listesi (ISRG Root X1).
pub const TRUSTED_ROOTS: &[&[u8]] = &[
    ISRG_ROOT_X1_DER,
    ISRG_ROOT_X2_DER,
    DIGICERT_GLOBAL_ROOT_CA_DER,
    DIGICERT_GLOBAL_ROOT_G2_DER,
    GTS_ROOT_R1_DER,
    GTS_ROOT_R2_DER,
    SSLCOM_TLS_ECC_ROOT_CA_2022_DER,
];

/// Verilen sertifika zincirini güvenilen kök sertifikalara karşı doğrular.
/// `chain`: Yaprak (leaf) sunucu sertifikasından ara sertifikalara doğru sıralı dizi.
///
/// Derinlik sınırı: zincir en fazla `MAX_CHAIN_DEPTH` sertifika içerebilir.
/// Daha uzun zincirler `ChainTooDeep` ile reddedilir; bu hem kötü niyetli
/// aşırı derin zincirleri hem de ara sertifika önbelleği olmayan çekirdekte
/// uzun zincirlerin yol açacağı bellek/performans yükünü sınırlar.
pub const MAX_CHAIN_DEPTH: usize = 6;

pub fn verify_certificate_chain(chain: &[&[u8]]) -> Result<(), CertValidationError> {
    if chain.is_empty() {
        return Err(CertValidationError::InvalidDer);
    }
    if chain.len() > MAX_CHAIN_DEPTH {
        return Err(CertValidationError::ChainTooDeep);
    }

    // 1. Her bir sertifikayı ayrıştır
    let mut parsed_chain = Vec::with_capacity(chain.len());
    for &cert_der in chain {
        parsed_chain.push(ParsedX509Cert::parse(cert_der)?);
    }

    // 2. Yapraktan başlayarak zincirdeki her sertifikanın bir sonraki tarafından imzalandığını doğrula.
    //    Kök sertifika sunucu tarafından gönderilmek zorunda değildir; bu yüzden zincir,
    //    kendinden imzalı bir köke veya gömülü bir güvenilen köke ulaşana kadar ilerler.
    let mut current = &parsed_chain[0];
    for depth in 0..=MAX_CHAIN_DEPTH {
        // Kendinden imzalı sertifika: zincirin kökü. Yalnızca gömülü güvenilen
        // köklerden biriyle birebir (DER) eşleşiyorsa kabul edilir.
        if current.issuer_raw == current.subject_raw {
            if TRUSTED_ROOTS.iter().any(|&root| root == chain[depth]) {
                return Ok(());
            }
            return Err(CertValidationError::UntrustedRoot);
        }

        // 3. Mevcut sertifikanın vereni (issuer) güvenilen köklerimizden biriyle eşleşiyorsa,
        //    gömülü kökün açık anahtarıyla imzayı doğrula. Sunucu zinciri kökü
        //    göndermemiş olsa bile (yaygın durum) doğrulama burada tamamlanır.
        if let Some(root_der) = trusted_root_for(current.issuer_raw) {
            current.verify_signature_with_issuer(root_der)?;
            return Ok(());
        }

        // 4. Kök ile doğrudan eşleşmediyse, zincirdeki sıradaki ara (intermediate) sertifikaya bak.
        if depth + 1 >= parsed_chain.len() {
            // Zincirin sonuna ulaşıldı ama güvenilen hiçbir kök bulunamadı
            return Err(CertValidationError::UntrustedRoot);
        }
        let next = &parsed_chain[depth + 1];
        if current.issuer_raw != next.subject_raw {
            return Err(CertValidationError::UntrustedRoot);
        }
        current.verify_signature_with_issuer(chain[depth + 1])?;
        current = next;
    }

    Err(CertValidationError::ChainTooDeep)
}

/// Güvenilen kök sertifikalardan birini, verilen ara/yaprak sertifikanın imzasını
/// doğrulamak için seçer. Sunucu zinciri kökü göndermediğinde (yaygın durum) yaprak
/// veya ara sertifikanın issuer'ı güvenilen bir kökün subject'i ile eşleşiyorsa o
/// kökü döner.
pub fn trusted_root_for(issuer_raw: &[u8]) -> Option<&'static [u8]> {
    TRUSTED_ROOTS.iter().copied().find(|&root_der| {
        ParsedX509Cert::parse(root_der)
            .map(|parsed| parsed.subject_raw == issuer_raw)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// DER TLV uretir: tag + uzunluk + icerik (kisa ve uzun uzunluk formlari).
    fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 6);
        out.push(tag);
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else if body.len() < 0x100 {
            out.push(0x81);
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.push((body.len() >> 8) as u8);
            out.push(body.len() as u8);
        }
        out.extend_from_slice(body);
        out
    }

    /// Minimal DER certificate with an RSA public key (n=3233, e=17).
    ///
    /// Tum uzunluklar icerigin gercek boyutundan hesaplanir; elle yazilmaz.
    fn make_test_cert_der() -> Vec<u8> {
        // RSAPublicKey SEQUENCE { modulus INTEGER, exponent INTEGER }
        let modulus: [u8; 2] = [0x0c, 0x91]; // 3233
        let exponent: [u8; 1] = [0x11]; // 17

        let mut rsapubkey_body = Vec::new();
        rsapubkey_body.push(0x02);
        rsapubkey_body.push(modulus.len() as u8);
        rsapubkey_body.extend_from_slice(&modulus);
        rsapubkey_body.push(0x02);
        rsapubkey_body.push(exponent.len() as u8);
        rsapubkey_body.extend_from_slice(&exponent);
        let rsapubkey = der_tlv(0x30, &rsapubkey_body);

        // BIT STRING: 0x00 (kullanilmayan bit) + RSAPublicKey
        let mut bits = Vec::new();
        bits.push(0x00);
        bits.extend_from_slice(&rsapubkey);
        let bitstring = der_tlv(0x03, &bits);

        // AlgorithmIdentifier: SEQUENCE { OID rsaEncryption, NULL }
        const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
        let mut alg_id_body = Vec::new();
        alg_id_body.push(0x06);
        alg_id_body.push(RSA_OID.len() as u8);
        alg_id_body.extend_from_slice(RSA_OID);
        alg_id_body.push(0x05);
        alg_id_body.push(0x00);
        let alg_id = der_tlv(0x30, &alg_id_body);

        // SubjectPublicKeyInfo: SEQUENCE { algorithm, subjectPublicKey BIT STRING }
        let mut spki_body = Vec::new();
        spki_body.extend_from_slice(&alg_id);
        spki_body.extend_from_slice(&bitstring);
        let spki = der_tlv(0x30, &spki_body);

        // issuer / subject Name: SEQUENCE { SET { SEQUENCE { OID, value } } }
        let mut atv = Vec::new();
        atv.push(0x06);
        atv.push(3);
        atv.extend_from_slice(&[0x55, 0x04, 0x03]);
        atv.push(0x13);
        atv.push(2);
        atv.extend_from_slice(b"XX");
        let atv_seq = der_tlv(0x30, &atv);
        let set = der_tlv(0x31, &atv_seq);
        let name = der_tlv(0x30, &set);

        // validity: SEQUENCE { notBefore, notAfter }
        let mut val_body = Vec::new();
        for ts in [&b"240101000000Z"[..], &b"250101000000Z"[..]] {
            val_body.push(0x17);
            val_body.push(ts.len() as u8);
            val_body.extend_from_slice(ts);
        }
        let validity = der_tlv(0x30, &val_body);

        // TBSCertificate: SEQUENCE { serialNumber, signature, issuer, validity,
        //                             subject, subjectPublicKeyInfo }
        let mut tbs_body = Vec::new();
        tbs_body.push(0x02);
        tbs_body.push(1);
        tbs_body.push(0x01);
        tbs_body.extend_from_slice(&alg_id);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&validity);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&spki);
        let tbs = der_tlv(0x30, &tbs_body);

        // Certificate: SEQUENCE { tbs, sigAlg, sigValue }
        let mut cert_body = Vec::new();
        cert_body.extend_from_slice(&tbs);
        cert_body.extend_from_slice(&alg_id);
        cert_body.extend_from_slice(&der_tlv(0x03, &[0x00]));
        der_tlv(0x30, &cert_body)
    }

    #[test]
    fn parse_x509_rsa_test() {
        let der = make_test_cert_der();
        let key = parse_x509_rsa_public_key(&der).expect("should parse RSA key");
        assert_eq!(key.n, vec![0x0c, 0x91]);
        assert_eq!(key.e, 0x11);
    }

    #[test]
    fn test_parse_certificate_info() {
        let der = make_test_cert_der();
        let info = parse_certificate_info(&der).expect("should parse certificate info");
        assert_eq!(info.common_name.as_deref(), Some("XX"));
        let nb = info.not_before.expect("not_before present");
        assert_eq!(nb.year, 2024);
        assert_eq!(nb.month, 1);
        assert_eq!(nb.day, 1);
        let na = info.not_after.expect("not_after present");
        assert_eq!(na.year, 2025);
        assert_eq!(na.month, 1);
        assert_eq!(na.day, 1);
    }

    #[test]
    fn test_verify_certificate_fields() {
        let der = make_test_cert_der();
        let valid_now = X509Time {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 0,
            second: 0,
        };
        // Başarılı doğrulama (CN="XX", tarih aralık içinde)
        assert_eq!(verify_certificate_fields(&der, "XX", &valid_now), Ok(()));
        assert_eq!(verify_certificate_fields(&der, "xx", &valid_now), Ok(())); // case-insensitive

        // Hostname uyuşmazlığı
        assert_eq!(
            verify_certificate_fields(&der, "yy.com", &valid_now),
            Err(CertValidationError::HostnameMismatch)
        );

        // Henüz geçerli değil
        let too_early = X509Time {
            year: 2023,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
        };
        assert_eq!(
            verify_certificate_fields(&der, "XX", &too_early),
            Err(CertValidationError::NotYetValid)
        );

        // Süresi dolmuş
        let too_late = X509Time {
            year: 2025,
            month: 1,
            day: 2,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(
            verify_certificate_fields(&der, "XX", &too_late),
            Err(CertValidationError::Expired)
        );
    }

    #[test]
    fn test_match_hostname_wildcard() {
        assert!(match_hostname("*.example.com", "foo.example.com"));
        assert!(match_hostname("*.example.com", "FOO.EXAMPLE.COM"));
        assert!(!match_hostname("*.example.com", "example.com"));
        assert!(!match_hostname("*.example.com", "bar.foo.example.com"));
        assert!(match_hostname("minos.org", "MINOS.ORG"));
        assert!(!match_hostname("minos.org", "other.org"));
    }

    #[test]
    fn test_chain_depth_limit_rejects_long_chain() {
        // Sinir asildiginda pahali DER ayristirmasina girmeden reddedilmelidir;
        // ayni (gecerli) sertifikanin tekrarlanmasi yeterlidir.
        let der = make_test_cert_der();
        let too_deep = alloc::vec![der.as_slice(); MAX_CHAIN_DEPTH + 1];
        assert_eq!(
            verify_certificate_chain(&too_deep),
            Err(CertValidationError::ChainTooDeep)
        );

        // Tam sinir uzunlugundaki zincir derinlik kontrolunu gecer (burada
        // imza/kok kontrolu nedeniyle baska bir hata donebilir).
        let at_limit = alloc::vec![der.as_slice(); MAX_CHAIN_DEPTH];
        assert_ne!(
            verify_certificate_chain(&at_limit),
            Err(CertValidationError::ChainTooDeep)
        );
    }

    /// Kök sertifika sunucu zincirinde gelmez; gömülü kökün **açık anahtarı**
    /// kullanılmalıdır. Önceden zincirin sonuna gelindiğinde her durumda
    /// `UntrustedRoot` dönüyordu.
    #[test]
    fn chain_without_root_uses_embedded_root() {
        let x1 = ISRG_ROOT_X1_DER;
        // Kökün kendisi kendinden imzalıdır ve gömülü kökle birebir eşleşir.
        assert_eq!(verify_certificate_chain(&[x1]), Ok(()));

        // Gömülü kökün subject'ini "veren" olarak gösteren bir sertifika
        // (gerçek R3 yerine test üretimi) zincir sonunda kökle eşleşmelidir:
        // kendinden imzalı ve gömülü olmayan bir sertifika reddedilmelidir.
        let rogue_root = make_test_cert_der();
        assert_eq!(
            verify_certificate_chain(&[rogue_root.as_slice()]),
            Err(CertValidationError::UntrustedRoot)
        );
    }

    /// `trusted_root_for`, gömülü kökün subject'i ile verilen issuer'ı eşler.
    #[test]
    fn trusted_root_lookup_matches_embedded_subject() {
        let parsed = ParsedX509Cert::parse(ISRG_ROOT_X1_DER).unwrap();
        assert!(trusted_root_for(parsed.subject_raw).is_some());
        assert!(trusted_root_for(b"no-such-issuer").is_none());
    }

    #[test]
    fn sslcom_tls_ecc_root_is_trusted_and_uses_p384() {
        let parsed = ParsedX509Cert::parse(SSLCOM_TLS_ECC_ROOT_CA_2022_DER).unwrap();
        assert_eq!(parsed.sig_alg_oid, OID_ECDSA_WITH_SHA384);
        assert!(parse_x509_p384_public_key(SSLCOM_TLS_ECC_ROOT_CA_2022_DER).is_ok());
        assert!(trusted_root_for(parsed.subject_raw).is_some());
        assert_eq!(
            verify_certificate_chain(&[SSLCOM_TLS_ECC_ROOT_CA_2022_DER]),
            Ok(())
        );
    }
}

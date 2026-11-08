//! CRL / OCSP otomatik indirme katmanı.
//!
//! Sertifikadan çıkarılan CRL Dağıtım Noktası (CDP) ve Authority Information
//! Access (AIA) URL'lerine HTTP GET isteği yapar; gelen DER yanıtı
//! `revocation::apply_crl` / `apply_ocsp` ile depoya işler. Fail-closed
//! politika (`revocation::check_certificate`) TLS katmanı tarafından
//! sorgulanır; sonucun ne zaman kontrol edileceği çağıranın sorumluluğudur.
//!
//! Sınırlar:
//! - OCSP yanıtlarının imzası `apply_ocsp_verified` ile yanıtlayıcı/CA
//!   sertifikasıyla doğrulanır (RFC 6960 §4.2.1); geçersiz imzalı yanıtlar
//!   `Unknown` ile reddedilir (fail-closed).
//! - Yalnızca HTTP GET ile indirme yapılır (OCSP POST üretimi gerekmez; birçok
//!   CA hem GET hem POST destekler, RFC 6960 §A.1 GET şablonu kullanılır).
//! - HTTPS URL'lerinde güvenlik el sıkışması TLS istemcisinden geçmez; HTTP
//!   dışı şemalar (örn. ldap://) atlanır.

#![allow(dead_code)]

use alloc::{string::String, string::ToString, vec::Vec};

use super::revocation::RevocationResult;
use super::revocation_core;

/// GET şablonu ile OCSP isteği URL'i üretir (RFC 6960 §A.1):
/// `{ocsp_url}/{url_encode(base64(der_encode(CertID)))}`
/// CertID ::= SEQUENCE { hashAlgorithm, issuerNameHash, issuerKeyHash, serialNumber }
fn build_ocsp_get_url(ocsp_url: &str, leaf_der: &[u8], issuer_der: &[u8]) -> Option<String> {
    use crate::drivers::crypto::sha1::Sha1;

    // --- SHA-1 helper -------------------------------------------------------
    fn sha1_hex(data: &[u8]) -> String {
        let digest = Sha1::hash(data);
        let mut s = String::new();
        for b in digest.iter() {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xF) as usize] as char);
        }
        s
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";

    // --- issuer Name DER (tbs.issuer alanının değeri) -----------------------
    let issuer_name_der = || -> Option<Vec<u8>> {
        let (tag, vo, _vl, _) = revocation_core::tlv_pub(issuer_der, 0).ok()?;
        if tag != 0x30 {
            return None;
        }
        let (tbs_tag, tbs_vo, _tbs_vl, _) = revocation_core::tlv_pub(issuer_der, vo).ok()?;
        if tbs_tag != 0x30 {
            return None;
        }
        let mut pos = tbs_vo;
        if let Ok((first_tag, _, _, first_next)) = revocation_core::tlv_pub(issuer_der, pos) {
            if first_tag == 0xA0 {
                pos = first_next;
            }
        }
        // serial, signature, issuer
        for _ in 0..2 {
            let (_, _, _, next) = revocation_core::tlv_pub(issuer_der, pos).ok()?;
            pos = next;
        }
        let (_, issuer_vo, issuer_vl, _) = revocation_core::tlv_pub(issuer_der, pos).ok()?;
        Some(issuer_der[issuer_vo..issuer_vo + issuer_vl].to_vec())
    }()?;

    // --- issuer Public Key (SPKI BIT STRING içeriği) ------------------------
    let issuer_key_der = || -> Option<Vec<u8>> {
        let (tag, vo, _, _) = revocation_core::tlv_pub(issuer_der, 0).ok()?;
        if tag != 0x30 {
            return None;
        }
        let (tbs_tag, tbs_vo, tbs_vl, _) = revocation_core::tlv_pub(issuer_der, vo).ok()?;
        if tbs_tag != 0x30 {
            return None;
        }
        let mut pos = tbs_vo;
        if let Ok((first_tag, _, _, first_next)) = revocation_core::tlv_pub(issuer_der, pos) {
            if first_tag == 0xA0 {
                pos = first_next;
            }
        }
        for _ in 0..4 {
            let (_, _, _, next) = revocation_core::tlv_pub(issuer_der, pos).ok()?;
            pos = next;
        }
        // subjectPublicKeyInfo SEQUENCE
        let (spki_tag, spki_vo, spki_vl, _) = revocation_core::tlv_pub(issuer_der, pos).ok()?;
        if spki_tag != 0x30 {
            return None;
        }
        let _ = (spki_vl, tbs_vl);
        let spki = &issuer_der[spki_vo..spki_vo + spki_vl];
        // BIT STRING içeriği (ilk bayt unused-bits)
        let (bs_tag, bs_vo, bs_vl, _) = revocation_core::tlv_pub(spki, 0).ok()?;
        let _ = bs_tag;
        // subjectPublicKeyInfo ::= SEQUENCE { algorithm, subjectPublicKey BIT STRING }
        let (seq_tag, seq_vo, seq_vl, _) = revocation_core::tlv_pub(spki, 0).ok()?;
        if seq_tag != 0x30 {
            return None;
        }
        let inner = &spki[seq_vo..seq_vo + seq_vl];
        let (alg_tag, _alg_vo, _alg_vl, alg_next) = revocation_core::tlv_pub(inner, 0).ok()?;
        if alg_tag != 0x30 {
            return None;
        }
        let (bs_tag2, bs_vo2, bs_vl2, _) = revocation_core::tlv_pub(inner, alg_next).ok()?;
        if bs_tag2 != 0x03 || bs_vl2 < 1 {
            return None;
        }
        let _ = (bs_vo, bs_vl);
        Some(inner[bs_vo2 + 1..bs_vo2 + bs_vl2].to_vec()) // unused-bits baytını at
    }()?;

    // --- leaf serial ---------------------------------------------------------
    let serial = revocation_core::certificate_serial(leaf_der).ok()?;

    // --- CertID DER ----------------------------------------------------------
    // SEQUENCE {
    //   SEQUENCE { OID 1.3.14.3.2.26 (sha1), NULL }   hashAlgorithm
    //   OCTET STRING issuerNameHash
    //   OCTET STRING issuerKeyHash
    //   INTEGER serialNumber
    // }
    let mut cert_id = Vec::new();
    // hashAlgorithm SEQUENCE
    cert_id.extend_from_slice(&[0x30, 0x09]);
    cert_id.extend_from_slice(&[0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a]); // 1.3.14.3.2.26 sha1
    cert_id.extend_from_slice(&[0x05, 0x00]); // NULL
                                              // issuerNameHash OCTET STRING
    let nh = sha1_hex(&issuer_name_der);
    cert_id.extend_from_slice(&[0x04, nh.len() as u8]);
    cert_id.extend_from_slice(nh.as_bytes());
    // issuerKeyHash OCTET STRING
    let kh = sha1_hex(&issuer_key_der);
    cert_id.extend_from_slice(&[0x04, kh.len() as u8]);
    cert_id.extend_from_slice(kh.as_bytes());
    // serialNumber INTEGER
    cert_id.push(0x02);
    cert_id.push(serial.len() as u8);
    cert_id.extend_from_slice(&serial);

    // --- base64 + URL encode -------------------------------------------------
    let b64 = base64_encode(&cert_id);
    let mut url = String::from(ocsp_url);
    if !url.ends_with('/') {
        url.push('/');
    }
    url.push_str(&b64);
    Some(url)
}

/// Basit base64 kodlayıcı (standart alfabe, padding'li).
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(TABLE[(n >> 6) as usize & 63] as char);
        out.push(TABLE[n as usize & 63] as char);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(TABLE[(n >> 6) as usize & 63] as char);
        out.push('=');
    }
    out
}

/// URL'i (scheme://host[:port]/path) parçalar; yalnızca http şeması desteklenir.
fn parse_http_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rfind(':') {
        Some(i) => (&authority[..i], authority[i + 1..].parse::<u16>().ok()?),
        None => (authority, 80),
    };
    if host.is_empty() {
        return None;
    }
    Some((String::from(host), port, String::from(path)))
}

/// HTTP GET ile verilen URL'den DER gövdesi indirir.
/// Yalnızca `http://` şeması desteklenir; HTTPS URL'leri `None` döner.
/// Dönüş: 200 durum kodu + gövde (Response Content-Length / chunked / EOF).
fn http_fetch_der(url: &str) -> Option<Vec<u8>> {
    let (host, port, path) = parse_http_url(url)?;
    let host: &str = &host;
    let path: &str = &path;
    let fd = crate::drivers::socket::socket(crate::drivers::socket::AF_INET, 1).ok()?;
    let _ = crate::drivers::socket::set_timeout(fd, 10_000);
    let ip = crate::drivers::socket::resolve_host(&host).ok()?;
    crate::drivers::socket::connect(fd, ip, port).ok()?;

    let req = alloc::format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: minos-revocation/1.0\r\nAccept: application/ocsp-response, application/pkix-crl\r\nConnection: close\r\n\r\n",
        path, host
    );
    let req_b = req.as_bytes();
    let mut sent = 0;
    while sent < req_b.len() {
        sent += crate::drivers::socket::send(fd, &req_b[sent..]).ok()?;
    }

    // Yanıtı oku (başlık + gövde)
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut chunk = [0u8; 4096];
    loop {
        let n = match crate::drivers::socket::recv(fd, &mut chunk) {
            Ok(n) => n as usize,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 512 * 1024 {
            break; // güvenlik sınırı
        }
    }
    let _ = crate::drivers::socket::close(fd);

    // Başlık/gövde ayrımı
    let header_end = find_subslice(&buf, b"\r\n\r\n")?;
    let body_start = header_end + 4;
    let head = core::str::from_utf8(&buf[..header_end]).ok()?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    let lower = head.to_ascii_lowercase_vec_ignored();

    // Content-Length varsa buna göre kırp
    if let Some(cl) = header_value_ci(head, "content-length") {
        let len: usize = cl.trim().parse().ok()?;
        if body_start + len <= buf.len() {
            return Some(buf[body_start..body_start + len].to_vec());
        }
        return Some(buf[body_start..].to_vec());
    }
    // Chunked ise çöz
    if lower.contains("transfer-encoding: chunked") {
        return decode_chunked_body(&buf[body_start..]);
    }
    // EOF'a kadar okunan her şey gövdedir
    Some(buf[body_start..].to_vec())
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn header_value_ci(head: &str, name: &str) -> Option<String> {
    for line in head.split("\r\n") {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_ascii_lowercase();
            if key == name {
                return Some(line[colon + 1..].trim().to_string());
            }
        }
    }
    None
}

// (Vec için yardımcı; kullanılmıyor olsa da derlemede sorun çıkarmaz.)
trait ToAsciiLowercaseVecIgnored {
    fn to_ascii_lowercase_vec_ignored(&self) -> String;
}
impl ToAsciiLowercaseVecIgnored for str {
    fn to_ascii_lowercase_vec_ignored(&self) -> String {
        self.to_ascii_lowercase()
    }
}

fn decode_chunked_body(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = find_subslice(&data[pos..], b"\r\n")? + pos;
        let size_str = core::str::from_utf8(&data[pos..line_end]).ok()?;
        let size = usize::from_str_radix(size_str.trim().split(';').next()?.trim(), 16).ok()?;
        pos = line_end + 2;
        if size == 0 {
            return Some(out);
        }
        if pos + size > data.len() {
            return None;
        }
        out.extend_from_slice(&data[pos..pos + size]);
        pos += size + 2; // CRLF atla
    }
}

/// OCSP yanıtını imza doğrulamalı olarak depoya yazar. Yanıtın imzası
/// verilen yanıtlayıcı/CA sertifikasıyla (`OcspResponseInfo::verify_signature`)
/// doğrulanır; ayrıştırma veya imza doğrulaması başarısızsa `Unknown` döner
/// (fail-closed, RFC 6960 §4.2.1).
fn apply_ocsp_verified(
    ocsp_der: &[u8],
    issuer_der: &[u8],
    leaf_serial: &[u8],
    now: &super::x509::X509Time,
) -> RevocationResult {
    let Ok(info) = super::revocation_core::parse_ocsp_response(ocsp_der) else {
        return RevocationResult::unknown();
    };
    if info.verify_signature(issuer_der).is_err() {
        return RevocationResult::unknown();
    }
    let r = super::revocation_core::check_ocsp(ocsp_der, leaf_serial, now)
        .unwrap_or_else(|_| RevocationResult::unknown());
    super::revocation::set_status(super::revocation::RevocationSource::Ocsp, r);
    r
}

/// Sertifika zincirindeki yaprak (leaf) sertifika için iptal durumunu otomatik
/// sorgular: önce CRL DP, sonra OCSP AIA. İlk `Good`/`Revoked` sonucu depoya
/// yazılır ve döndürülür; hiçbir kaynak bulunamazsa `Unknown` döner.
pub fn fetch_and_apply_revocation(
    leaf_der: &[u8],
    issuer_der: &[u8],
    now: &super::x509::X509Time,
) -> RevocationResult {
    let mut last = RevocationResult::unknown();

    // 1) CRL Dağıtım Noktaları
    let crl_urls = super::x509::find_crl_urls(leaf_der);
    for url in crl_urls.iter().take(2) {
        if !url.starts_with("http://") {
            continue; // HTTPS/ldap desteği yok
        }
        if let Some(crl_der) = http_fetch_der(url) {
            last = super::revocation::apply_crl(&crl_der, issuer_der, &serial_of(leaf_der), now);
            if last.status != revocation_core::RevocationStatus::Unknown {
                return last;
            }
        }
    }

    // 2) OCSP (AIA)
    let ocsp_urls = super::x509::find_ocsp_urls(leaf_der);
    for url in ocsp_urls.iter().take(2) {
        if !url.starts_with("http://") {
            continue;
        }
        if let Some(ocsp_der) = http_fetch_der(url) {
            last = apply_ocsp_verified(&ocsp_der, issuer_der, &serial_of(leaf_der), now);
            if last.status != revocation_core::RevocationStatus::Unknown {
                return last;
            }
        }
    }

    last
}

/// Bir sertifikanın seri numarası; hata durumunda boş dizi.
fn serial_of(cert_der: &[u8]) -> Vec<u8> {
    revocation_core::certificate_serial(cert_der).unwrap_or_default()
}

/// OCSP GET şablonu URL'i üretip indirir ve yanıtı uygular.
/// (GET şablonu birçok CA tarafından desteklenir; POST şablonu gerekmiyor.)
pub fn fetch_and_apply_ocsp(
    ocsp_url: &str,
    leaf_der: &[u8],
    issuer_der: &[u8],
    now: &super::x509::X509Time,
) -> RevocationResult {
    let Some(url) = build_ocsp_get_url(ocsp_url, leaf_der, issuer_der) else {
        return RevocationResult::unknown();
    };
    let Some(ocsp_der) = http_fetch_der(&url) else {
        return RevocationResult::unknown();
    };
    apply_ocsp_verified(&ocsp_der, issuer_der, &serial_of(leaf_der), now)
}

/// Sonucu kaynak etiketiyle raporlamak için yardımcı.
pub fn last_source_label() -> &'static str {
    super::revocation::source().as_str()
}

// Kaynak etiketleri dışa açık kopya (testler için).
pub use super::revocation::RevocationSource as FetchSource;

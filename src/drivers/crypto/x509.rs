//! Minimal X.509 certificate parser — extracts the RSA public key from a DER
//! certificate — no_std.
//!
//! The parser follows the certificate structure instead of scanning for byte
//! patterns, so it also works for 2048-bit moduli (whose INTEGER TLV uses a
//! long-form length).

use crate::drivers::crypto::rsa::RsaPublicKey;

const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_NULL: u8 = 0x05;
const TAG_OID: u8 = 0x06;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_CONTEXT_0: u8 = 0xA0;

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
    if oid_tag != TAG_OID || oid_vl != RSA_OID.len() || &cert_der[oid_vo..oid_vo + oid_vl] != RSA_OID {
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
}

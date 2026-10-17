//! Minimal X.509 certificate parser — extracts RSA public key from DER cert — no_std.

use crate::drivers::crypto::x25519::RsaPublicKey;

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
    if value_offset + len > data.len() {
        return Err(());
    }
    let next_offset = value_offset + len;
    Ok((tag, value_offset, len, next_offset))
}

/// Expect a specific tag at offset.
fn expect_tag(data: &[u8], offset: usize, tag: u8) -> Result<(usize, usize, usize), ()> {
    let (t, vo, vl, no) = read_der_tlv(data, offset)?;
    if t != tag {
        return Err(());
    }
    Ok((vo, vl, no))
}

/// Parse a DER-encoded X.509 certificate and extract the RSA public key.
/// Returns the RsaPublicKey (modulus + exponent).
pub fn parse_x509_rsa_public_key(cert_der: &[u8]) -> Result<RsaPublicKey, ()> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
    let (_, _, cert_end) = expect_tag(cert_der, 0, 0x30)?; // SEQUENCE
    let _ = cert_end;

    // tbsCertificate ::= SEQUENCE
    let (tbs_start, _, tbs_end) = expect_tag(cert_der, 0, 0x30)?;

    // Inside TBSCertificate, we need SubjectPublicKeyInfo.
    // SubjectPublicKeyInfo ::= SEQUENCE { algorithm, subjectPublicKey }
    // It's near the end of TBSCertificate, before extensions (if any).
    // We'll scan for the SEQUENCE that contains the OID 1.2.840.113549.1.1.1 (rsaEncryption).

    // RSA encryption OID: 1.2.840.113549.1.1.1
    const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

    // Search within TBSCertificate for SubjectPublicKeyInfo
    let mut pos = tbs_start;
    while pos < tbs_end {
        // Look for SEQUENCE tag
        if pos < cert_der.len() && cert_der[pos] == 0x30 {
            let (seq_start, _seq_len, seq_end) = match read_der_tlv(cert_der, pos) {
                Ok((t, vo, vl, no)) if t == 0x30 => (vo, vl, no),
                _ => break,
            };
            // Check if this sequence contains the RSA OID followed by BIT STRING
            // AlgorithmIdentifier ::= SEQUENCE { algorithm, parameters }
            // Look for OID tag (0x06)
            let mut inner = seq_start;
            while inner < seq_end {
                if cert_der[inner] == 0x06 {
                    let (oid_vo, oid_len, _) = match read_der_tlv(cert_der, inner) {
                        Ok((t, vo, vl, no)) if t == 0x06 => (vo, vl, no),
                        _ => break,
                    };
                    if oid_len == RSA_OID.len()
                        && &cert_der[oid_vo..oid_vo + oid_len] == RSA_OID
                    {
                        // Found rsaEncryption OID. Next should be BIT STRING with the key.
                        let bitstring_pos = find_bitstring_after_oid(cert_der, inner, seq_end)?;
                        let key_bytes = parse_rsa_public_key_bits(cert_der, bitstring_pos)?;
                        return Ok(key_bytes);
                    }
                }
                inner += 1;
            }
        }
        pos += 1;
    }

    Err(())
}

/// Find BIT STRING tag (0x03) after an OID within a sequence boundary.
fn find_bitstring_after_oid(
    data: &[u8],
    oid_pos: usize,
    seq_end: usize,
) -> Result<usize, ()> {
    let mut pos = oid_pos;
    // Skip OID tag + length + value
    if pos >= data.len() {
        return Err(());
    }
    pos += 1; // skip tag
    let (len_consumed, oid_len) = read_der_len(data, pos)?;
    pos += 1 + len_consumed + oid_len;

    // Skip optional parameters (usually NULL 0x05 0x00)
    while pos < seq_end {
        if data[pos] == 0x03 {
            // BIT STRING
            return Ok(pos);
        }
        // Skip this TLV
        let (_, _, _, next) = read_der_tlv(data, pos)?;
        pos = next;
    }
    Err(())
}

/// Parse RSAPublicKey from BIT STRING contents.
/// RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }
fn parse_rsa_public_key_bits(
    data: &[u8],
    bitstring_pos: usize,
) -> Result<RsaPublicKey, ()> {
    // BIT STRING: tag 0x03, length, then [0 unused-bytes bit-string]
    let (_tag, value_offset, value_len, _next) = read_der_tlv(data, bitstring_pos)?;
    if value_len < 1 {
        return Err(());
    }
    let unused_bits = data[value_offset];
    let _ = unused_bits;
    let key_start = value_offset + 1;
    let _key_len = value_len - 1;

    // RSAPublicKey ::= SEQUENCE
    let (_tag, seq_vo, _seq_vl, seq_end) = read_der_tlv(data, key_start)?;
    if _tag != 0x30 {
        return Err(());
    }
    let _ = seq_end;

    // modulus INTEGER (tag 0x02)
    let (_tag, mod_vo, mod_vl, _mod_next) = read_der_tlv(data, seq_vo)?;
    if _tag != 0x02 {
        return Err(());
    }
    let mut modulus = data[mod_vo..mod_vo + mod_vl].to_vec();
    // Strip leading zero byte if present (INTEGER padding)
    if modulus.first() == Some(&0x00) {
        modulus.remove(0);
    }

    // publicExponent INTEGER
    let (_tag, exp_vo, exp_vl, _exp_next) = read_der_tlv(data, seq_vo + 2 + mod_vl)?;
    if _tag != 0x02 {
        return Err(());
    }
    let exp_bytes = &data[exp_vo..exp_vo + exp_vl];
    let mut e: u32 = 0;
    for b in exp_bytes {
        e = (e << 8) | (*b as u32);
    }

    Ok(RsaPublicKey { n: modulus, e })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal DER certificate with RSA public key (n=3233, e=17), self-signed, not valid
    /// but structurally correct enough for parsing test.
    fn make_test_cert_der() -> Vec<u8> {
        // Build: SEQUENCE { tbs SEQUENCE {...}, sigAlg SEQUENCE {rsaEncryption, NULL}, sigValue BIT STRING }
        // For testing, we just need SubjectPublicKeyInfo inside tbs with rsaEncryption OID and BIT STRING key.
        let mut der = Vec::new();

        // RSAPublicKey SEQUENCE { modulus INTEGER, exponent INTEGER }
        let modulus: [u8; 2] = [0x0c, 0x91]; // 3233
        let exponent: [u8; 1] = [0x11]; // 17

        let mut rsapubkey = Vec::new();
        rsapubkey.push(0x30); // SEQUENCE
        // length: 2 (modulus TLV) + 2 + 1 (exponent TLV) + ...
        // modulus TLV: 0x02 0x02 0x0c 0x91 = 4 bytes
        // exponent TLV: 0x02 0x01 0x11 = 3 bytes
        // total content = 7 bytes
        rsapubkey.push(7u8);
        rsapubkey.push(0x02); rsapubkey.push(2u8);
        rsapubkey.extend_from_slice(&modulus);
        rsapubkey.push(0x02); rsapubkey.push(1u8);
        rsapubkey.extend_from_slice(&exponent);

        // BIT STRING wrapping: 0x03, length, 0x00 (unused bits), content
        let mut bitstring = Vec::new();
        bitstring.push(0x03);
        bitstring.push((1 + rsapubkey.len()) as u8);
        bitstring.push(0x00);
        bitstring.extend_from_slice(&rsapubkey);

        // AlgorithmIdentifier: SEQUENCE { OID rsaEncryption, NULL }
        const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
        let mut alg_id = Vec::new();
        alg_id.push(0x30);
        let oid_tlv_len = 1 + 1 + RSA_OID.len();
        let null_tlv_len = 2; // 0x05 0x00
        let alg_content_len = oid_tlv_len + null_tlv_len;
        alg_id.push(alg_content_len as u8);
        alg_id.push(0x06);
        alg_id.push(RSA_OID.len() as u8);
        alg_id.extend_from_slice(RSA_OID);
        alg_id.push(0x05);
        alg_id.push(0x00);

        // SubjectPublicKeyInfo: SEQUENCE { algorithm, subjectPublicKey BIT STRING }
        let mut spki = Vec::new();
        spki.push(0x30);
        let spki_content = alg_id.len() + bitstring.len();
        spki.push(spki_content as u8);
        spki.extend_from_slice(&alg_id);
        spki.extend_from_slice(&bitstring);

        // TBSCertificate: SEQUENCE { ..., subjectPublicKeyInfo }
        let mut tbs = Vec::new();
        tbs.push(0x30);
        tbs.push(spki.len() as u8);
        tbs.extend_from_slice(&spki);

        // Certificate: SEQUENCE { tbs, sigAlg, sigValue }
        der.push(0x30);
        let cert_content = tbs.len() + alg_id.len() + (1 + 1 + 1); // sigValue placeholder
        der.push(cert_content as u8);
        der.extend_from_slice(&tbs);
        der.extend_from_slice(&alg_id);
        der.push(0x03); der.push(1); der.push(0); // sigValue BIT STRING

        der
    }

    #[test]
    fn parse_x509_rsa_test() {
        let der = make_test_cert_der();
        let key = parse_x509_rsa_public_key(&der).expect("should parse RSA key");
        assert_eq!(key.n, vec![0x0c, 0x91]);
        assert_eq!(key.e, 0x11);
    }
}

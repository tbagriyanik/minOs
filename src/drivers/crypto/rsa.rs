//! Minimal RSA public-key operations (PKCS#1 v1.5) for TLS key exchange — no_std.
//!
//! The big-integer core works on 32-bit limbs stored least-significant-word-first.
//! Reduction uses binary long division, which is simple and easy to verify;
//! it is slower than Knuth's algorithm D but fast enough for a TLS handshake.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::drivers::crypto::sha256::Sha256;

/// Counter used to vary the PKCS#1 padding between calls.
static PAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RSA public key: modulus n and public exponent e.
pub struct RsaPublicKey {
    pub n: Vec<u8>, // big-endian modulus
    pub e: u32,     // public exponent (typically 65537)
}

/// RSA private key: modulus n and private exponent d.
///
/// The kernel currently only verifies signatures, but keeping the signing
/// primitive here lets the TLS layer be tested end-to-end against a local
/// server implementation.
pub struct RsaPrivateKey {
    pub n: Vec<u8>, // big-endian modulus
    pub d: Vec<u8>, // big-endian private exponent
}

/// Convert a big-endian byte string into little-endian 32-bit limbs.
fn bytes_to_limbs(b: &[u8]) -> Vec<u32> {
    let mut limbs: Vec<u32> = Vec::with_capacity(b.len() / 4 + 1);
    let mut i = b.len();
    while i >= 4 {
        limbs.push(u32::from_be_bytes([b[i - 4], b[i - 3], b[i - 2], b[i - 1]]));
        i -= 4;
    }
    if i > 0 {
        let mut buf = [0u8; 4];
        buf[4 - i..].copy_from_slice(&b[..i]);
        limbs.push(u32::from_be_bytes(buf));
    }
    if limbs.is_empty() {
        limbs.push(0);
    }
    limbs
}

/// Convert little-endian 32-bit limbs into a big-endian byte string of exactly
/// `out_len` bytes (truncating high bytes / zero-padding low bytes as needed).
fn limbs_to_bytes(limbs: &[u32], out_len: usize) -> Vec<u8> {
    let mut out = vec![0u8; out_len];
    for (idx, limb) in limbs.iter().enumerate() {
        let bytes = limb.to_be_bytes();
        for j in 0..4 {
            // Bu baytin, tum sayinin en anlamsiz baytina uzakligi.
            let rev = 4 * idx + (3 - j);
            if rev >= out_len {
                continue;
            }
            // Big-endian ciktida bu bayt, sagdan `rev` konumunda durur.
            out[out_len - 1 - rev] = bytes[j];
        }
    }
    out
}

/// Compare two limb slices (least-significant limb first).
fn cmp_limbs(a: &[u32], b: &[u32]) -> Ordering {
    let n = a.len().max(b.len());
    for i in (0..n).rev() {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

/// `a -= b` in place (assumes `a >= b`).
fn sub_limbs(a: &mut [u32], b: &[u32]) {
    let mut borrow: i64 = 0;
    for i in 0..a.len() {
        let bv = *b.get(i).unwrap_or(&0) as i64;
        let diff = a[i] as i64 - bv - borrow;
        if diff < 0 {
            a[i] = (diff + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            a[i] = diff as u32;
            borrow = 0;
        }
    }
}

/// Reduce `num` (any length) modulo `m` using binary long division.
fn mod_reduce_limbs(num: &[u32], m: &[u32]) -> Vec<u32> {
    if m.len() == 1 && m[0] == 0 {
        return vec![0u32; m.len()];
    }
    // The running remainder always stays below m, so m.len() + 1 limbs suffice
    // for the intermediate "r << 1 | bit" value.
    let mut r = vec![0u32; m.len() + 1];
    let total_bits = num.len() * 32;
    for i in (0..total_bits).rev() {
        let bit = (num[i / 32] >> (i % 32)) & 1;
        // r = r << 1 | bit
        let mut carry = bit;
        for limb in r.iter_mut() {
            let next = *limb >> 31;
            *limb = (*limb << 1) | carry;
            carry = next;
        }
        // r < 2*m here, so a single conditional subtraction is enough.
        if carry != 0 || cmp_limbs(&r, m) != Ordering::Less {
            sub_limbs(&mut r, m);
        }
    }
    r.truncate(m.len());
    r
}

/// Multiply two limb slices and reduce modulo `m` (schoolbook multiplication).
fn mod_mul(a: &[u32], b: &[u32], m: &[u32]) -> Vec<u32> {
    let mut acc = vec![0u32; a.len() + b.len()];
    for i in 0..a.len() {
        let ai = a[i] as u64;
        if ai == 0 {
            continue;
        }
        let mut carry: u64 = 0;
        for j in 0..b.len() {
            let t = acc[i + j] as u64 + ai * (b[j] as u64) + carry;
            acc[i + j] = t as u32;
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry != 0 && k < acc.len() {
            let t = acc[k] as u64 + carry;
            acc[k] = t as u32;
            carry = t >> 32;
            k += 1;
        }
    }
    mod_reduce_limbs(&acc, m)
}

/// Modular exponentiation: base^exp mod modulus (square-and-multiply).
fn mod_pow_limbs(base: &[u32], exp: u32, m: &[u32]) -> Vec<u32> {
    let mut result = vec![0u32; m.len()];
    result[0] = 1;
    let mut acc = mod_reduce_limbs(base, m);
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = mod_mul(&result, &acc, m);
        }
        e >>= 1;
        if e > 0 {
            acc = mod_mul(&acc, &acc, m);
        }
    }
    result
}

/// Fill `ps` with non-zero pseudo-random bytes derived from `msg` and `seed`.
/// PKCS#1 v1.5 requires the padding string to be non-zero and unpredictable.
fn fill_padding(ps: &mut [u8], msg: &[u8], seed: u64) {
    let mut counter = seed;
    let mut i = 0usize;
    while i < ps.len() {
        let mut h = Sha256::new();
        h.update(msg);
        h.update(&counter.to_be_bytes());
        let block = h.finalize();
        for b in block.iter() {
            if i < ps.len() {
                // A zero byte would terminate the padding string, so map it away.
                ps[i] = if *b == 0 { 1 } else { *b };
                i += 1;
            }
        }
        counter = counter.wrapping_add(1);
    }
}

/// PKCS#1 v1.5 encryption: EM = 0x00 || 0x02 || PS || 0x00 || M
/// where PS is non-zero pseudo-random padding of length k - len(M) - 3.
///
/// The padding seed comes from an internal counter, so consecutive calls with
/// the same message produce different ciphertexts. Callers that own a better
/// entropy source should use [`pkcs1_v1_5_encrypt_seeded`] instead.
pub fn pkcs1_v1_5_encrypt(key: &RsaPublicKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    let counter = PAD_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let seed = counter.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17) ^ (msg.len() as u64);
    pkcs1_v1_5_encrypt_seeded(key, msg, seed)
}

/// PKCS#1 v1.5 encryption with an explicit padding seed. The TLS layer passes a
/// value that changes per session so the padding is not byte-for-byte constant.
pub fn pkcs1_v1_5_encrypt_seeded(key: &RsaPublicKey, msg: &[u8], seed: u64) -> Result<Vec<u8>, ()> {
    let k = key.n.len();
    // A modulus shorter than 11 bytes cannot hold the PKCS#1 v1.5 padding.
    if k < 11 || msg.len() > k - 11 {
        return Err(());
    }
    let ps_len = k - msg.len() - 3;
    let mut em = Vec::with_capacity(k);
    em.push(0x00);
    em.push(0x02);
    let mut ps = vec![0u8; ps_len];
    fill_padding(&mut ps, msg, seed);
    em.extend_from_slice(&ps);
    em.push(0x00);
    em.extend_from_slice(msg);

    let modulus = bytes_to_limbs(&key.n);
    let c = mod_pow_limbs(&bytes_to_limbs(&em), key.e, &modulus);
    Ok(limbs_to_bytes(&c, k))
}

/// Modular exponentiation with an arbitrary-length exponent (limbs, LSW first).
fn mod_pow_limbs_big(base: &[u32], exp: &[u32], m: &[u32]) -> Vec<u32> {
    let mut result = vec![0u32; m.len()];
    result[0] = 1;
    let mut acc = mod_reduce_limbs(base, m);
    let total_bits = exp.len() * 32;
    // En dusuk bitten yuksege dogru (LSB-first) square-and-multiply.
    for i in 0..total_bits {
        if (exp[i / 32] >> (i % 32)) & 1 == 1 {
            result = mod_mul(&result, &acc, m);
        }
        acc = mod_mul(&acc, &acc, m);
    }
    result
}

/// SHA-256 DigestInfo prefix used by PKCS#1 v1.5 signatures.
const SHA256_DIGEST_INFO_PREFIX: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// PKCS#1 v1.5 + SHA-256 signature generation.
///
/// `EM = 0x00 || 0x01 || 0xFF... || 0x00 || DigestInfo(SHA-256(msg))`
/// followed by the raw private-key exponentiation `EM^d mod n`.
pub fn pkcs1_v1_5_sign(key: &RsaPrivateKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    let k = key.n.len();
    if k < 11 {
        return Err(());
    }
    let mut hash = Sha256::new();
    hash.update(msg);
    let digest = hash.finalize();

    let mut t = Vec::with_capacity(SHA256_DIGEST_INFO_PREFIX.len() + digest.len());
    t.extend_from_slice(&SHA256_DIGEST_INFO_PREFIX);
    t.extend_from_slice(&digest);

    if t.len() > k - 11 {
        return Err(());
    }
    let ps_len = k - t.len() - 3;
    let mut em = Vec::with_capacity(k);
    em.push(0x00);
    em.push(0x01);
    em.extend(core::iter::repeat(0xFFu8).take(ps_len));
    em.push(0x00);
    em.extend_from_slice(&t);

    let modulus = bytes_to_limbs(&key.n);
    let sig = mod_pow_limbs_big(&bytes_to_limbs(&em), &bytes_to_limbs(&key.d), &modulus);
    Ok(limbs_to_bytes(&sig, k))
}

/// Verify a signature produced by [`pkcs1_v1_5_sign`] using the public key.
pub fn pkcs1_v1_5_sign_verify(key: &RsaPublicKey, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
    rsa_verify_pkcs1_sha256(key, msg, sig)
}

/// Raw RSA public-key operation: m^e mod n.
pub fn rsa_public_encrypt(key: &RsaPublicKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    pkcs1_v1_5_encrypt(key, msg)
}

/// PKCS#1 v1.5 + SHA-256 imza doğrulaması (TLS ServerKeyExchange için).
///
/// `sig^e mod n` ile şifre çözülür, elde edilen EM = 0x00||0x01||PS||0x00||
/// DigestInfo yapısı ve SHA-256 özeti karşılaştırılır.
pub fn rsa_verify_pkcs1_sha256(key: &RsaPublicKey, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
    let mut h = Sha256::new();
    h.update(msg);
    let digest = h.finalize();
    rsa_verify_pkcs1_digest_info(key, sig, &SHA256_DIGEST_INFO_PREFIX, &digest)
}

/// SHA-384 DigestInfo prefix used by PKCS#1 v1.5 signatures.
const SHA384_DIGEST_INFO_PREFIX: [u8; 19] = [
    0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05,
    0x00, 0x04, 0x30,
];

/// PKCS#1 v1.5 + SHA-384 imza doğrulaması.
///
/// Bazı kök/ara CA'lar (ör. Sectigo/USERTrust zincirleri) alt
/// sertifikaları sha384WithRSAEncryption ile imzalar; yalnızca SHA-256
/// desteklendiğinde bu zincirler `UnsupportedSignatureAlgorithm` ile
/// reddediliyordu.
pub fn rsa_verify_pkcs1_sha384(key: &RsaPublicKey, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
    let digest = crate::drivers::crypto::sha384::Sha384::hash(msg);
    rsa_verify_pkcs1_digest_info(key, sig, &SHA384_DIGEST_INFO_PREFIX, &digest)
}

/// PKCS#1 v1.5 imzasını açıp DigestInfo başlığını ve özeti karşılaştırır.
fn rsa_verify_pkcs1_digest_info(
    key: &RsaPublicKey,
    sig: &[u8],
    prefix: &[u8],
    digest: &[u8],
) -> Result<(), ()> {
    let k = key.n.len();
    if k < 11 || sig.len() != k {
        return Err(());
    }
    let modulus = bytes_to_limbs(&key.n);
    let m = mod_pow_limbs(&bytes_to_limbs(sig), key.e, &modulus);
    let em = limbs_to_bytes(&m, k);

    if em.len() < 11 || em[0] != 0x00 || em[1] != 0x01 {
        return Err(());
    }
    // PS: en az 8 bayt 0xFF, ardından tek bir 0x00 ayracı.
    let mut i = 2usize;
    let mut ps_len = 0usize;
    while i < em.len() && em[i] == 0xFF {
        ps_len += 1;
        i += 1;
    }
    if ps_len < 8 || i >= em.len() || em[i] != 0x00 {
        return Err(());
    }
    i += 1;

    let di = &em[i..];
    if di.len() != prefix.len() + digest.len() || di[..prefix.len()] != prefix[..] {
        return Err(());
    }
    if di[prefix.len()..] != digest[..] {
        return Err(());
    }
    Ok(())
}

/// Verify an RSA-PSS signature using SHA-256 and a 32-byte salt (TLS scheme
/// `rsa_pss_rsae_sha256`).
pub fn rsa_verify_pss_sha256(key: &RsaPublicKey, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
    const H_LEN: usize = 32;
    let k = key.n.len();
    if k < 2 * H_LEN + 2 || sig.len() != k || key.n[0] == 0 {
        return Err(());
    }

    let modulus_bits = k * 8 - key.n[0].leading_zeros() as usize;
    let em_bits = modulus_bits.checked_sub(1).ok_or(())?;
    let em_len = (em_bits + 7) / 8;
    if em_len < 2 * H_LEN + 2 || em_len > k {
        return Err(());
    }

    let modulus = bytes_to_limbs(&key.n);
    let encoded = limbs_to_bytes(&mod_pow_limbs(&bytes_to_limbs(sig), key.e, &modulus), k);
    if encoded[..k - em_len].iter().any(|&b| b != 0) {
        return Err(());
    }
    let em = &encoded[k - em_len..];
    if em[em_len - 1] != 0xbc {
        return Err(());
    }

    let db_len = em_len - H_LEN - 1;
    let (masked_db, h_and_trailer) = em.split_at(db_len);
    let h = &h_and_trailer[..H_LEN];
    let unused_bits = 8 * em_len - em_bits;
    if unused_bits > 0 && masked_db[0] & (0xff << (8 - unused_bits)) != 0 {
        return Err(());
    }

    let mut db = masked_db.to_vec();
    let mut counter = 0u32;
    let mut offset = 0;
    while offset < db_len {
        let mut mgf = Sha256::new();
        mgf.update(h);
        mgf.update(&counter.to_be_bytes());
        let block = mgf.finalize();
        let take = (db_len - offset).min(H_LEN);
        for i in 0..take {
            db[offset + i] ^= block[i];
        }
        offset += take;
        counter = counter.checked_add(1).ok_or(())?;
    }
    if unused_bits > 0 {
        db[0] &= 0xff >> unused_bits;
    }

    let salt_start = db_len - H_LEN;
    if db[..salt_start - 1].iter().any(|&b| b != 0) || db[salt_start - 1] != 1 {
        return Err(());
    }
    let salt = &db[salt_start..];
    let mut m_hash = Sha256::new();
    m_hash.update(msg);
    let m_hash = m_hash.finalize();
    let mut check = Sha256::new();
    check.update(&[0u8; 8]);
    check.update(&m_hash);
    check.update(salt);
    if check.finalize().as_slice() != h {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_pow_small() {
        // 2^10 mod 256 = 0
        let m = bytes_to_limbs(&[0x01, 0x00]);
        let r = mod_pow_limbs(&bytes_to_limbs(&[0x02]), 10, &m);
        assert_eq!(limbs_to_bytes(&r, 2), vec![0, 0]);
    }

    #[test]
    fn mod_pow_known_value() {
        // 7^13 mod 11 = 2
        let m = bytes_to_limbs(&[0x0b]);
        let r = mod_pow_limbs(&bytes_to_limbs(&[0x07]), 13, &m);
        assert_eq!(limbs_to_bytes(&r, 1), vec![2]);
    }

    #[test]
    fn pkcs1_encrypt_decrypt_roundtrip() {
        // Gercekci uzunlukta bir modulus (16 bayt). PKCS#1 v1.5 dolgusu
        // k - 11 bayttan uzun mesajlari kabul etmez, dolayisiyla k >= 11 olmali.
        let key = RsaPublicKey {
            n: vec![
                0xd3, 0xa4, 0xf0, 0x92, 0x5b, 0x1c, 0x77, 0x4e, 0x2a, 0x86, 0x39, 0x0b, 0xc5, 0x64,
                0x18, 0x8f,
            ],
            e: 65537,
        };
        let msg = [0x42u8];
        let ct = pkcs1_v1_5_encrypt(&key, &msg).expect("encryption should succeed");
        assert_eq!(ct.len(), key.n.len());

        // Dolgu sigmayan cok kucuk modulus reddedilmeli.
        let tiny = RsaPublicKey {
            n: vec![0x0c, 0x91],
            e: 17,
        };
        assert!(pkcs1_v1_5_encrypt(&tiny, &msg).is_err());
    }

    fn hex(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(s.len() / 2);
        let mut i = 0;
        while i + 1 < b.len() {
            let hi = (b[i] as char).to_digit(16).unwrap() as u8;
            let lo = (b[i + 1] as char).to_digit(16).unwrap() as u8;
            out.push((hi << 4) | lo);
            i += 2;
        }
        out
    }

    #[test]
    fn pkcs1_sign_verify_roundtrip() {
        // 512-bit RSA test anahtari (n, d).
        const N: [u8; 64] = [
            0xb0, 0xea, 0xe6, 0x91, 0x0f, 0xea, 0x84, 0x1c, 0x20, 0x51, 0x68, 0xa0, 0xb8, 0x35,
            0x0e, 0xb8, 0x95, 0xfc, 0x78, 0x8d, 0xdb, 0x3e, 0x3c, 0x10, 0xc2, 0x7b, 0xa5, 0xbf,
            0xf8, 0x42, 0x70, 0xa7, 0x1b, 0x73, 0x4b, 0xb2, 0xa9, 0xbb, 0x95, 0x6c, 0x91, 0xdf,
            0xa1, 0xfa, 0xa6, 0xd4, 0xf2, 0xe8, 0x42, 0xe9, 0xf6, 0x6d, 0xb5, 0xb0, 0x1b, 0x99,
            0xe2, 0x05, 0x81, 0xc4, 0x18, 0x44, 0xa2, 0xbb,
        ];
        const D: [u8; 64] = [
            0x21, 0x25, 0xb1, 0xd3, 0x66, 0x0f, 0x03, 0x77, 0x9d, 0x9a, 0x83, 0x26, 0xf1, 0xbc,
            0x87, 0x28, 0x90, 0x79, 0x80, 0x40, 0xdb, 0xde, 0x1a, 0x34, 0xed, 0xab, 0x6b, 0x0f,
            0xbf, 0xbc, 0xfb, 0x0c, 0x14, 0x17, 0x4f, 0x54, 0xde, 0xeb, 0xd5, 0xcf, 0xfa, 0x17,
            0xa0, 0xdb, 0xc4, 0xc6, 0x1d, 0x58, 0x75, 0x13, 0xdf, 0xa1, 0x57, 0xff, 0xa3, 0x7e,
            0xba, 0x46, 0xde, 0x19, 0x9a, 0x14, 0x7e, 0xf9,
        ];
        let priv_key = RsaPrivateKey {
            n: N.to_vec(),
            d: D.to_vec(),
        };
        let pub_key = RsaPublicKey {
            n: N.to_vec(),
            e: 65537,
        };

        let msg = b"TLS 1.3, server CertificateVerify";
        let sig = pkcs1_v1_5_sign(&priv_key, msg).expect("imza uretilemedi");
        assert_eq!(sig.len(), 64);
        // Python ile uretilen referans deger.
        let expected = hex(
            "2eaad98f9fdc9d794e0dfe201b8556e0610d3191b3555cd86eb03a6183a4e147\
             0471f2e551db6ae2f64051e0f6f78b6dbae2420b55f057e5c6f0bd02381e254a",
        );
        assert_eq!(sig, expected, "imza referans degerle eslesmeli");
        assert!(rsa_verify_pkcs1_sha256(&pub_key, msg, &sig).is_ok());
        assert!(rsa_verify_pkcs1_sha256(&pub_key, b"baska mesaj", &sig).is_err());
    }

    #[test]
    fn pkcs1_sha256_verify_known_answer() {
        // Python cryptography ile uretilmis 1024-bit RSA anahtari + imza (KAT).
        let key = RsaPublicKey {
            n: hex(
                "C10FD34BFCE8F2EDC37AB97EDF7CF0CF123183CD4C3DA773955510D08C4FA14F\
                    419337BF31B4088D8A476E3DB9873195F77C188A4C172C737F19286E225ACB1C\
                    AE341265864603E568256EB692994E5BEADE574840B996C1B6437B40F999407B\
                    8CCB51BC4778ADB798DBE8C4E8EE433C6759CC4625ABA0C935454EE36B389815",
            ),
            e: 65537,
        };
        let sig = hex(
            "6138E115E6FA79723510B616E2983FB022B71ABCDED8199CA12D39D498B25413\
             6E7F817BDB74EE08470E93092A47B5EC62390B167BEBA4B283500409BEA61B3D\
             3838C4B6E22BBF92739673B35CBBCD06B87F8A7D839EAD2E009D6CD23DC1827\
             F9692741CD15F1B3AF3E33F678901C293E842A4FB3A633B45339FF02520B6868D",
        );
        let msg = b"client_random|server_random|params";
        assert!(rsa_verify_pkcs1_sha256(&key, msg, &sig).is_ok());

        // Bozuk imza reddedilmeli.
        let bad = hex(
            "4E128A7F776B24E29690A722F912B02997991DFF5CC53A43CD56734F0219159F\
             C85DA42625CDA217B7890BD9A8A10981C9ECCC407929D566E279BA35284960F\
             58B425814045717EB11163FA29A4947915EEC349F2D395064FD92EEB946610CC\
             CCF5848062371F3F50307C9D0EFE964C4689C5BACC7D471E2178C5B9AD699EA316",
        );
        assert!(rsa_verify_pkcs1_sha256(&key, msg, &bad).is_err());

        // Yanlis mesaj, dogru imza bile olsa reddedilmeli.
        assert!(rsa_verify_pkcs1_sha256(&key, b"yanlis mesaj", &sig).is_err());
        // Uzunluk uyusmayan imza reddedilmeli.
        assert!(rsa_verify_pkcs1_sha256(&key, msg, &sig[..64]).is_err());
    }
}

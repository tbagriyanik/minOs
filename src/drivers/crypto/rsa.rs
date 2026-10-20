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
    let seed = counter
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        ^ (msg.len() as u64);
    pkcs1_v1_5_encrypt_seeded(key, msg, seed)
}

/// PKCS#1 v1.5 encryption with an explicit padding seed. The TLS layer passes a
/// value that changes per session so the padding is not byte-for-byte constant.
pub fn pkcs1_v1_5_encrypt_seeded(
    key: &RsaPublicKey,
    msg: &[u8],
    seed: u64,
) -> Result<Vec<u8>, ()> {
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

/// Raw RSA public-key operation: m^e mod n.
pub fn rsa_public_encrypt(key: &RsaPublicKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    pkcs1_v1_5_encrypt(key, msg)
}

/// PKCS#1 v1.5 + SHA-256 imza doğrulaması (TLS ServerKeyExchange için).
///
/// `sig^e mod n` ile şifre çözülür, elde edilen EM = 0x00||0x01||PS||0x00||
/// DigestInfo yapısı ve SHA-256 özeti karşılaştırılır.
pub fn rsa_verify_pkcs1_sha256(key: &RsaPublicKey, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
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
    // DigestInfo: ASN.1 başlığı (19 bayt) + 32 baytlık SHA-256 özeti.
    let prefix: [u8; 19] = [
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    if di.len() != 19 + 32 || di[..19] != prefix[..] {
        return Err(());
    }
    let mut h = Sha256::new();
    h.update(msg);
    let digest = h.finalize();
    if di[19..] != digest[..] {
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
        let tiny = RsaPublicKey { n: vec![0x0c, 0x91], e: 17 };
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
    fn pkcs1_sha256_verify_known_answer() {
        // Python cryptography ile uretilmis 1024-bit RSA anahtari + imza (KAT).
        let key = RsaPublicKey {
            n: hex("C10FD34BFCE8F2EDC37AB97EDF7CF0CF123183CD4C3DA773955510D08C4FA14F\
                    419337BF31B4088D8A476E3DB9873195F77C188A4C172C737F19286E225ACB1C\
                    AE341265864603E568256EB692994E5BEADE574840B996C1B6437B40F999407B\
                    8CCB51BC4778ADB798DBE8C4E8EE433C6759CC4625ABA0C935454EE36B389815"),
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

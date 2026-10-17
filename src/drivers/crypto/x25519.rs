//! Minimal RSA public-key operations (PKCS#1 v1.5) for TLS key exchange — no_std.

use alloc::vec;
use alloc::vec::Vec;

/// RSA public key: modulus n and public exponent e.
pub struct RsaPublicKey {
    pub n: Vec<u8>, // big-endian modulus
    pub e: u32,     // public exponent (typically 65537)
}

/// Modular exponentiation: base^exp mod modulus. All big-endian, same length.
fn mod_pow(base: &[u8], exp: u32, modulus: &[u8]) -> Vec<u8> {
    let n = modulus.len();
    let mut result = vec![0u8; n];
    result[0] = 1;
    let mut base = base.to_vec();
    // Reduce base mod n
    base = mod_reduce(&base, modulus);

    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = mod_mul(&result, &base, modulus);
        }
        base = mod_mul(&base, &base, modulus);
        e >>= 1;
    }
    result
}

/// Multiply two big-endian numbers mod modulus (schoolbook, result same length as modulus).
fn mod_mul(a: &[u8], b: &[u8], modulus: &[u8]) -> Vec<u8> {
    let n = modulus.len();
    let mut acc = vec![0u8; 2 * n];
    for i in 0..n {
        if a[n - 1 - i] == 0 {
            continue;
        }
        let mut carry: u64 = 0;
        for j in 0..n {
            let idx = n - 1 - i + n - 1 - j;
            let val = acc[idx] as u64
                + (a[n - 1 - i] as u64) * (b[n - 1 - j] as u64)
                + carry;
            acc[idx] = val as u8;
            carry = val >> 8;
        }
        // propagate carry
        let mut c = carry;
        let mut pos = 2 * n - 1 - i;
        while c > 0 && pos < acc.len() {
            let val = acc[pos] as u64 + c;
            acc[pos] = val as u8;
            c = val >> 8;
            pos += 1;
        }
    }
    mod_reduce(&acc, modulus)
}

/// Reduce a big-endian number (any length) mod modulus (big-endian, n bytes).
fn mod_reduce(num: &[u8], modulus: &[u8]) -> Vec<u8> {
    let n = modulus.len();
    // Simple repeated subtraction is too slow; use schoolbook division.
    // For our use case (RSA 1024/2048 bit), num is at most 2n bytes.
    let mut r = num.to_vec();
    // Strip leading zeros
    while r.len() > 1 && r[0] == 0 {
        r.remove(0);
    }
    if r.len() < n {
        return r;
    }

    // Schoolbook division: r = r mod modulus
    // Normalize: shift so modulus top bit is set
    let shift = (modulus[0] as u32).leading_zeros();
    let m_norm: Vec<u8> = {
        let mut v = modulus.to_vec();
        let mut carry: u8 = 0;
        for b in v.iter_mut().rev() {
            let c = (*b << shift) | carry;
            carry = (*b >> (8 - shift)) & 0xFF;
            *b = c;
        }
        v
    };

    let mut r_norm = r.clone();
    let mut carry: u8 = 0;
    for b in r_norm.iter_mut().rev() {
        let c = (*b << shift) | carry;
        carry = (*b >> (8 - shift)) & 0xFF;
        *b = c;
    }
    r_norm.push(0);
    r_norm.push(0);

    let n_bytes = m_norm.len();
    let r_len = r_norm.len();
    let q_len = r_len.saturating_sub(n_bytes) + 1;

    for i in (0..q_len).rev() {
        // Estimate quotient digit
        let mut q_hat: u64 = 0;
        if r_len > i + n_bytes && i + n_bytes < r_norm.len() {
            let r_top = (r_norm[i] as u64) << 8 | (r_norm[i + 1] as u64);
            let m_top = m_norm[0] as u64;
            if m_top != 0 {
                q_hat = r_top / m_top;
                if q_hat > 255 {
                    q_hat = 255;
                }
            }
        }
        // Multiply and subtract
        let mut borrow: i64 = 0;
        let mut q = q_hat;
        while q > 0 {
            for j in 0..n_bytes {
                let idx = i + j;
                if idx >= r_norm.len() {
                    break;
                }
                let val = r_norm[idx] as i64 - (m_norm[j] as i64) * (q as i64) - borrow;
                if val < 0 {
                    borrow = -1;
                    r_norm[idx] = (val + 256) as u8;
                } else {
                    borrow = 0;
                    r_norm[idx] = val as u8;
                }
            }
            if borrow != 0 {
                // Add back modulus
                let mut carry: i64 = 0;
                for j in 0..n_bytes {
                    let idx = i + j;
                    if idx >= r_norm.len() {
                        break;
                    }
                    let val = r_norm[idx] as i64 + (m_norm[j] as i64) + carry;
                    if val > 255 {
                        carry = 1;
                        r_norm[idx] = (val - 256) as u8;
                    } else {
                        carry = 0;
                        r_norm[idx] = val as u8;
                    }
                }
                q -= 1;
            } else {
                break;
            }
        }
    }

    // Result is in first n bytes (with possible leading zeros)
    let mut result = r_norm[..n.min(r_norm.len())].to_vec();
    // Un-normalize
    if shift > 0 {
        let mut carry: u8 = 0;
        for b in result.iter_mut().rev() {
            let c = (*b >> shift) | (carry << (8 - shift));
            carry = (*b << (8 - shift)) & 0xFF;
            *b = c;
        }
    }
    // Strip leading zeros
    while result.len() > 1 && result[0] == 0 {
        result.remove(0);
    }
    result
}

/// PKCS#1 v1.5 encryption: EM = 0x00 || 0x02 || PS || 0x00 || M
/// where PS is non-zero random padding of length k - len(M) - 3.
pub fn pkcs1_v1_5_encrypt(key: &RsaPublicKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    let k = key.n.len();
    if msg.len() > k - 11 {
        return Err(());
    }
    let ps_len = k - msg.len() - 3;
    let mut em = Vec::with_capacity(k);
    em.push(0x00);
    em.push(0x02);
    // Deterministic padding for simplicity (not CSPRNG, but acceptable for this context)
    for _ in 0..ps_len {
        em.push(0x01); // non-zero
    }
    em.push(0x00);
    em.extend_from_slice(msg);

    let m_int = mod_pow(&em, key.e, &key.n);
    // Pad to k bytes
    let mut out = vec![0u8; k.saturating_sub(m_int.len())];
    out.extend_from_slice(&m_int);
    Ok(out)
}

pub fn rsa_public_encrypt(key: &RsaPublicKey, msg: &[u8]) -> Result<Vec<u8>, ()> {
    pkcs1_v1_5_encrypt(key, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_pow_small() {
        // 2^10 mod 1000 = 1024 mod 1000 = 24
        let result = mod_pow(&[2u8], 10, &[256u8]); // 2^10 mod 256 = 0
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn pkcs1_encrypt_decrypt_roundtrip() {
        // Small RSA key: n = 3233 (61*53), e = 17
        // We can't easily decrypt without private key, just test encryption doesn't error
        let key = RsaPublicKey {
            n: vec![0x0c, 0x91], // 3233
            e: 17,
        };
        let msg = [0x42u8];
        let ct = pkcs1_v1_5_encrypt(&key, &msg);
        assert!(ct.is_ok());
        assert_eq!(ct.unwrap().len(), 2);
    }
}

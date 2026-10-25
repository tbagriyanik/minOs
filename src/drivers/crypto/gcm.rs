//! GCM mode (NIST SP 800-38D) built on AES — no_std.

use crate::drivers::crypto::aes::Aes;
use alloc::vec;
use alloc::vec::Vec;

#[allow(dead_code)]
const BLOCK_SIZE: usize = 16;
#[allow(dead_code)]
const TAG_SIZE: usize = 16;

/// Multiply block by x in GF(2^128) with reduction polynomial x^128 + x^7 + x^2 + x + 1.
/// The GCM bit string is big-endian (bit 0 = MSB of byte 0), so "multiply by x"
/// is a right shift of the 128-bit integer.
#[inline]
#[allow(dead_code)]
fn gf_mul_x(block: &mut [u8; 16]) {
    let carry = (block[15] & 1) != 0;
    for i in (1..16).rev() {
        block[i] = (block[i] >> 1) | (block[i - 1] << 7);
    }
    block[0] >>= 1;
    if carry {
        block[0] ^= 0xe1;
    }
}

/// Multiply two GF(2^128) elements: out = x * y (NIST SP 800-38D, Algorithm 1).
/// The bits of `x` are consumed from the most significant (MSB of byte 0, i.e.
/// GCM bit 0) towards the least significant (LSB of byte 15, GCM bit 127).
/// `v` starts at `y` and is multiplied by x (a right shift of the big-endian
/// 128-bit integer) after every bit.
fn gf_mul(x: &mut [u8; 16], y: &[u8; 16]) {
    let mut z = [0u8; 16];
    let mut v = *y;
    let xb = *x;
    for byte_idx in 0..16 {
        let mut byte = xb[byte_idx];
        for _ in 0..8 {
            if byte & 0x80 != 0 {
                for i in 0..16 {
                    z[i] ^= v[i];
                }
            }
            byte <<= 1;
            // v = v * x  (right shift of the big-endian 128-bit integer)
            let carry = (v[15] & 1) != 0;
            for i in (1..16).rev() {
                v[i] = (v[i] >> 1) | (v[i - 1] << 7);
            }
            v[0] >>= 1;
            if carry {
                v[0] ^= 0xe1;
            }
        }
    }
    *x = z;
}

/// GHASH: process `blocks` (each up to 16 bytes, padded with zeros) accumulating y = y * H.
fn ghash(y: &mut [u8; 16], h: &[u8; 16], blocks: &[&[u8]]) {
    for block in blocks {
        let mut b = [0u8; 16];
        let len = block.len().min(16);
        b[..len].copy_from_slice(&block[..len]);
        for i in 0..16 {
            y[i] ^= b[i];
        }
        gf_mul(y, h);
    }
}

/// Increment a 16-byte counter block (big-endian, 32-bit counter at the end).
fn inc32(counter: &mut [u8; 16]) {
    let mut i = 15;
    while i > 11 {
        counter[i] = counter[i].wrapping_add(1);
        if counter[i] != 0 {
            break;
        }
        i -= 1;
    }
}

/// AES-GCM context.
pub struct Gcm {
    aes: Aes,
    h: [u8; 16],
}

impl Gcm {
    pub fn new(key: &[u8]) -> Result<Self, ()> {
        let aes = Aes::new(key)?;
        let zero = [0u8; 16];
        let h = aes.encrypt(&zero);
        Ok(Gcm { aes, h })
    }

    /// Derive J0 from nonce.
    fn derive_j0(&self, nonce: &[u8]) -> [u8; 16] {
        if nonce.len() == 12 {
            let mut j0 = [0u8; 16];
            j0[..12].copy_from_slice(nonce);
            j0[15] = 1;
            j0
        } else {
            // GHASH(H, nonce || 0^(16*ceil(len/16)-len) || 0^64 || len(nonce)*8)
            let neg_len = (16 - (nonce.len() % 16)) % 16;
            let mut padded = nonce.to_vec();
            padded.resize(nonce.len() + neg_len + 8, 0);
            let nonce_bits = (nonce.len() as u64).wrapping_mul(8);
            let end = padded.len();
            padded[end - 8..].copy_from_slice(&nonce_bits.to_be_bytes());

            let mut blocks: Vec<&[u8]> = Vec::new();
            for chunk in padded.chunks(16) {
                blocks.push(chunk);
            }
            let mut y = [0u8; 16];
            ghash(&mut y, &self.h, &blocks);
            y
        }
    }

    /// Encrypt plaintext with AES-GCM. Returns (ciphertext, tag).
    pub fn encrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<(Vec<u8>, [u8; 16]), ()> {
        let j0 = self.derive_j0(nonce);

        // CTR encryption: counter starts at J0 + 1
        let mut ctr = j0;
        inc32(&mut ctr);

        let mut ciphertext = vec![0u8; plaintext.len()];
        let mut pt_blocks: Vec<&[u8]> = plaintext.chunks(16).collect();
        let mut ct_blocks: Vec<&mut [u8]> = ciphertext.chunks_mut(16).collect();

        for (pt, ct) in pt_blocks.iter_mut().zip(ct_blocks.iter_mut()) {
            let enc_ctr = self.aes.encrypt(&ctr);
            for i in 0..pt.len() {
                ct[i] = pt[i] ^ enc_ctr[i];
            }
            inc32(&mut ctr);
        }

        // Tag = AES_encrypt(J0, GHASH(H, AAD || ciphertext || len(AAD)*8 || len(ct)*8))
        let aad_len_bits = (aad.len() as u64).wrapping_mul(8);
        let ct_len_bits = (ciphertext.len() as u64).wrapping_mul(8);

        let mut aad_blocks: Vec<&[u8]> = Vec::new();
        for chunk in aad.chunks(16) {
            aad_blocks.push(chunk);
        }
        let mut ct_padded: Vec<&[u8]> = Vec::new();
        for chunk in ciphertext.chunks(16) {
            ct_padded.push(chunk);
        }

        // Length block: len(AAD) || len(CT) as 64-bit big-endian each
        let mut len_block = [0u8; 16];
        len_block[..8].copy_from_slice(&aad_len_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_len_bits.to_be_bytes());

        let mut all_blocks: Vec<&[u8]> = Vec::new();
        for b in &aad_blocks {
            all_blocks.push(*b);
        }
        for b in &ct_padded {
            all_blocks.push(*b);
        }
        all_blocks.push(&len_block);

        let mut y = [0u8; 16];
        ghash(&mut y, &self.h, &all_blocks);

        // NIST SP 800-38D §7.2: Tag = MSB_128(GCTR_K(J0, S)) = AES_encrypt(K, J0) XOR S
        let tag = self.aes.encrypt(&j0);
        let mut out_tag = tag;
        for i in 0..16 {
            out_tag[i] ^= y[i];
        }

        Ok((ciphertext, out_tag))
    }

    /// Decrypt ciphertext with AES-GCM. Returns plaintext if tag verifies.
    pub fn decrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        tag: &[u8; 16],
    ) -> Result<Vec<u8>, ()> {
        // Verify tag first by recomputing
        let aad_len_bits = (aad.len() as u64).wrapping_mul(8);
        let ct_len_bits = (ciphertext.len() as u64).wrapping_mul(8);

        let mut aad_blocks: Vec<&[u8]> = Vec::new();
        for chunk in aad.chunks(16) {
            aad_blocks.push(chunk);
        }
        let mut ct_blocks: Vec<&[u8]> = Vec::new();
        for chunk in ciphertext.chunks(16) {
            ct_blocks.push(chunk);
        }

        let mut len_block = [0u8; 16];
        len_block[..8].copy_from_slice(&aad_len_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_len_bits.to_be_bytes());

        let mut all_blocks: Vec<&[u8]> = Vec::new();
        for b in &aad_blocks {
            all_blocks.push(*b);
        }
        for b in &ct_blocks {
            all_blocks.push(*b);
        }
        all_blocks.push(&len_block);

        let mut y = [0u8; 16];
        ghash(&mut y, &self.h, &all_blocks);

        // NIST SP 800-38D 7.2: Tag = MSB_128(GCTR_K(J0, S)) = AES_encrypt(K, J0) XOR S
        let j0 = self.derive_j0(nonce);
        let mut computed_tag = self.aes.encrypt(&j0);
        for i in 0..16 {
            computed_tag[i] ^= y[i];
        }

        if computed_tag.as_slice() != tag {
            return Err(());
        }

        // Decrypt: counter starts at J0 + 1
        let mut ctr = j0;
        inc32(&mut ctr);

        let mut plaintext = vec![0u8; ciphertext.len()];
        let mut ct_blocks = ciphertext.chunks(16);
        let mut pt_blocks = plaintext.chunks_mut(16);

        for (ct, pt) in ct_blocks.by_ref().zip(pt_blocks.by_ref()) {
            let enc_ctr = self.aes.encrypt(&ctr);
            for i in 0..ct.len() {
                pt[i] = ct[i] ^ enc_ctr[i];
            }
            inc32(&mut ctr);
        }

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcm_rfc5116_case1() {
        // NIST SP 800-38D / RFC 5116 Test Case 1: AES-128 GCM
        let key = <[u8; 16]>::default();
        let nonce = <[u8; 12]>::default();
        let aad = <[u8; 16]>::default();
        let plaintext = <[u8; 16]>::default();

        let gcm = Gcm::new(&key).unwrap();
        let (ct, tag) = gcm.encrypt(&nonce, &aad, &plaintext).unwrap();

        let expected_ct = [
            0x03, 0x88, 0xda, 0xce, 0x60, 0xb6, 0xa3, 0x92, 0xf3, 0x28, 0xc2, 0xb9, 0x71, 0xb2,
            0xfe, 0x78,
        ];
        let expected_tag = [
            0xd2, 0x4e, 0x50, 0x3a, 0x1b, 0xb0, 0x37, 0x07, 0x1c, 0x71, 0xb3, 0x5d, 0x98, 0x7b,
            0x86, 0x57,
        ];
        assert_eq!(ct, expected_ct);
        assert_eq!(tag, expected_tag);

        // Decrypt
        let pt = gcm.decrypt(&nonce, &aad, &ct, &tag).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn gcm_wrong_tag_fails() {
        let key = <[u8; 16]>::default();
        let nonce = <[u8; 12]>::default();
        let gcm = Gcm::new(&key).unwrap();
        let (ct, tag) = gcm.encrypt(&nonce, &[], &<[u8; 16]>::default()).unwrap();
        let mut bad_tag = tag;
        bad_tag[0] ^= 0xff;
        assert!(gcm.decrypt(&nonce, &[], &ct, &bad_tag).is_err());
    }
}

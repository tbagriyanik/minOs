//! HMAC-SHA256 / HMAC-SHA384 (RFC 2104 / FIPS 198-1) — no_std.

use crate::drivers::crypto::sha256::Sha256;
use crate::drivers::crypto::sha384::Sha384;

const BLOCK_SIZE: usize = 64;

/// SHA-384 blok boyutu 128 bayt, çıktı uzunluğu 48 bayttır (FIPS 180-4).
const BLOCK_SIZE_384: usize = 128;
const SHA384_OUTPUT_LEN: usize = 48;

pub struct HmacSha256 {
    ipad: [u8; BLOCK_SIZE],
    opad: [u8; BLOCK_SIZE],
}

impl HmacSha256 {
    pub fn new(key: &[u8]) -> Self {
        let mut k = [0u8; BLOCK_SIZE];
        if key.len() > BLOCK_SIZE {
            let hk = Sha256::hash(key);
            k[..32].copy_from_slice(&hk);
        } else {
            k[..key.len()].copy_from_slice(key);
        }

        let mut ipad = [0u8; BLOCK_SIZE];
        let mut opad = [0u8; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            ipad[i] = k[i] ^ 0x36;
            opad[i] = k[i] ^ 0x5c;
        }
        HmacSha256 { ipad, opad }
    }

    pub fn sign(key: &[u8], data: &[u8]) -> [u8; 32] {
        let h = HmacSha256::new(key);
        let mut inner = Sha256::new();
        inner.update(&h.ipad);
        inner.update(data);
        let inner_hash = inner.finalize();

        let mut outer = Sha256::new();
        outer.update(&h.opad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    pub fn verify(key: &[u8], data: &[u8], mac: &[u8; 32]) -> bool {
        let expected = Self::sign(key, data);
        expected.as_slice() == mac
    }
}

/// HMAC-SHA384 — TLS 1.3 `TLS_AES_256_GCM_SHA384` paketinin HKDF/Finished
/// hesapları için kullanılır (RFC 8446 §7.1).
pub struct HmacSha384 {
    ipad: [u8; BLOCK_SIZE_384],
    opad: [u8; BLOCK_SIZE_384],
}

impl HmacSha384 {
    pub fn new(key: &[u8]) -> Self {
        let mut k = [0u8; BLOCK_SIZE_384];
        if key.len() > BLOCK_SIZE_384 {
            let hk = Sha384::hash(key);
            k[..SHA384_OUTPUT_LEN].copy_from_slice(&hk);
        } else {
            k[..key.len()].copy_from_slice(key);
        }

        let mut ipad = [0u8; BLOCK_SIZE_384];
        let mut opad = [0u8; BLOCK_SIZE_384];
        for i in 0..BLOCK_SIZE_384 {
            ipad[i] = k[i] ^ 0x36;
            opad[i] = k[i] ^ 0x5c;
        }
        HmacSha384 { ipad, opad }
    }

    pub fn sign(key: &[u8], data: &[u8]) -> [u8; SHA384_OUTPUT_LEN] {
        let h = HmacSha384::new(key);
        let mut inner = Sha384::new();
        inner.update(&h.ipad);
        inner.update(data);
        let inner_hash = inner.finalize();

        let mut outer = Sha384::new();
        outer.update(&h.opad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    pub fn verify(key: &[u8], data: &[u8], mac: &[u8; SHA384_OUTPUT_LEN]) -> bool {
        let expected = Self::sign(key, data);
        expected.as_slice() == mac
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn hmac_rfc4231_case1() {
        // RFC 4231 Test Case 1: key = 0x0b repeated 20, data = "Hi There"
        let key: Vec<u8> = vec![0x0b; 20];
        let data = b"Hi There";
        let mac = HmacSha256::sign(&key, data);
        let expected = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(mac, expected);
        assert!(HmacSha256::verify(&key, data, &mac));
    }

    #[test]
    fn hmac_sha384_rfc4231_case1() {
        // RFC 4231 Test Case 1: key = 0x0b repeated 20, data = "Hi There"
        let key: Vec<u8> = vec![0x0b; 20];
        let data = b"Hi There";
        let mac = HmacSha384::sign(&key, data);
        let expected = [
            0xaf, 0xd0, 0x39, 0x44, 0xd8, 0x48, 0x95, 0x62, 0x6b, 0x08, 0x25, 0xf4, 0xab, 0x46,
            0x90, 0x7f, 0x15, 0xf9, 0xda, 0xdb, 0xe4, 0x10, 0x1e, 0xc6, 0x82, 0xaa, 0x03, 0x4c,
            0x7c, 0xeb, 0xc5, 0x9c, 0xfa, 0xea, 0x9e, 0xa9, 0x07, 0x6e, 0xde, 0x7f, 0x4a, 0xf1,
            0x52, 0xe8, 0xb2, 0xfa, 0x9c, 0xb6,
        ];
        assert_eq!(mac, expected);
        assert!(HmacSha384::verify(&key, data, &mac));
    }

    #[test]
    fn hmac_key_longer_than_block() {
        // Key longer than block size should be hashed first.
        let key: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let data = b"Test";
        let mac = HmacSha256::sign(&key, data);
        // Verify determinism: same call yields same result.
        let mac2 = HmacSha256::sign(&key, data);
        assert_eq!(mac, mac2);
    }
}

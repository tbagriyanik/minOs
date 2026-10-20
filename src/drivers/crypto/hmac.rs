//! HMAC-SHA256 (RFC 2104 / FIPS 198-1) — no_std.

use crate::drivers::crypto::sha256::Sha256;

const BLOCK_SIZE: usize = 64;

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
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b, 0xf1,
            0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c, 0x2e, 0x32,
            0xcf, 0xf7,
        ];
        assert_eq!(mac, expected);
        assert!(HmacSha256::verify(&key, data, &mac));
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

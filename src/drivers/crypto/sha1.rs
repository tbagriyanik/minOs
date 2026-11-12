//! SHA-1 (FIPS 180-1 / RFC 3174) — no_std, no external dependencies.
//!
//! WPA2-PSK 4-way el sıkışması için zorunludur: PBKDF2-HMAC-SHA1 ile
//! 256-bit PMK türetimi (4096 tur) ve HMAC-SHA1 ile EAPOL-Key MIC
//! (anahtar onayı). SHA-1 çıktısı 20 bayt (160 bit), blok boyutu 64 bayttır.

const K0: u32 = 0x5A82_7999; // 0 <= t <= 19
const K1: u32 = 0x6ED9_EBA1; // 20 <= t <= 39
const K2: u32 = 0x8F1B_BCDC; // 40 <= t <= 59
const K3: u32 = 0xCA62_C1D6; // 60 <= t <= 79

fn rotl(x: u32, n: u32) -> u32 {
    x.rotate_left(n)
}

#[allow(non_snake_case)]
fn f0(b: u32, c: u32, d: u32) -> u32 {
    (b & c) | ((!b) & d)
}

#[allow(non_snake_case)]
fn f1(b: u32, c: u32, d: u32) -> u32 {
    b ^ c ^ d
}

#[allow(non_snake_case)]
fn f2(b: u32, c: u32, d: u32) -> u32 {
    (b & c) | (b & d) | (c & d)
}

#[derive(Clone, Copy, Debug)]
pub struct Sha1 {
    h: [u32; 5],
    buffer: [u8; 64],
    buffer_len: usize,
    total_bits: u64,
}

impl Sha1 {
    pub fn new() -> Self {
        Sha1 {
            h: [
                0x6745_2301,
                0xEFCD_AB89,
                0x98BA_DCFE,
                0x1032_5476,
                0xC3D2_E1F0,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    fn transform(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = ((block[i * 4] as u32) << 24)
                | ((block[i * 4 + 1] as u32) << 16)
                | ((block[i * 4 + 2] as u32) << 8)
                | (block[i * 4 + 3] as u32);
        }
        for i in 16..80 {
            w[i] = rotl(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }

        let mut a = self.h[0];
        let mut b = self.h[1];
        let mut c = self.h[2];
        let mut d = self.h[3];
        let mut e = self.h[4];

        for (i, &wv) in w.iter().enumerate() {
            let (f, k) = if i < 20 {
                (f0(b, c, d), K0)
            } else if i < 40 {
                (f1(b, c, d), K1)
            } else if i < 60 {
                (f2(b, c, d), K2)
            } else {
                (f1(b, c, d), K3)
            };
            let temp = rotl(a, 5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wv);
            e = d;
            d = c;
            c = rotl(b, 30);
            b = a;
            a = temp;
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut idx = 0;
        while idx < data.len() {
            let take = (64 - self.buffer_len).min(data.len() - idx);
            self.buffer[self.buffer_len..self.buffer_len + take]
                .copy_from_slice(&data[idx..idx + take]);
            self.buffer_len += take;
            idx += take;
            self.total_bits += take as u64 * 8;
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.transform(&block);
                self.buffer_len = 0;
            }
        }
    }

    pub fn finalize(mut self) -> [u8; 20] {
        // `update` zaten tampona kopyalanan her baytı total_bits'e ekledi,
        // burada tekrar eklenmemelidir.
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            while self.buffer_len < 64 {
                self.buffer[self.buffer_len] = 0;
                self.buffer_len += 1;
            }
            let block = self.buffer;
            self.transform(&block);
            self.buffer_len = 0;
        }
        while self.buffer_len < 56 {
            self.buffer[self.buffer_len] = 0;
            self.buffer_len += 1;
        }
        self.buffer[56..64].copy_from_slice(&self.total_bits.to_be_bytes());
        let block = self.buffer;
        self.transform(&block);

        let mut out = [0u8; 20];
        for i in 0..5 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_be_bytes());
        }
        out
    }

    pub fn hash(data: &[u8]) -> [u8; 20] {
        let mut s = Self::new();
        s.update(data);
        s.finalize()
    }
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

/// HMAC-SHA1 (RFC 2104 / FIPS 198-1).
pub struct HmacSha1 {
    ipad: [u8; 64],
    opad: [u8; 64],
}

impl HmacSha1 {
    pub fn new(key: &[u8]) -> Self {
        let mut k = [0u8; 64];
        if key.len() > 64 {
            let hk = Sha1::hash(key);
            k[..20].copy_from_slice(&hk);
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0u8; 64];
        let mut opad = [0u8; 64];
        for i in 0..64 {
            ipad[i] = k[i] ^ 0x36;
            opad[i] = k[i] ^ 0x5c;
        }
        HmacSha1 { ipad, opad }
    }

    pub fn sign(key: &[u8], data: &[u8]) -> [u8; 20] {
        let h = HmacSha1::new(key);
        let mut inner = Sha1::new();
        inner.update(&h.ipad);
        inner.update(data);
        let inner_hash = inner.finalize();

        let mut outer = Sha1::new();
        outer.update(&h.opad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    pub fn verify(key: &[u8], data: &[u8], mac: &[u8; 20]) -> bool {
        Self::sign(key, data).as_slice() == mac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_empty() {
        let h = Sha1::hash(&[]);
        let expected = [
            0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
            0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha1_abc() {
        let h = Sha1::hash(b"abc");
        let expected = [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha1_rfc3174_second_vector() {
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let h = Sha1::hash(data);
        let expected = [
            0x84, 0x98, 0x3e, 0x44, 0x1c, 0x3b, 0xd2, 0x6e, 0xba, 0xae, 0x4a, 0xa1, 0xf9, 0x51,
            0x29, 0xe5, 0xe5, 0x46, 0x70, 0xf1,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha1_million_a() {
        let data = vec![b'a'; 1_000_000];
        let h = Sha1::hash(&data);
        let expected = [
            0x34, 0xaa, 0x97, 0x3c, 0xd4, 0xc4, 0xda, 0xa4, 0xf6, 0x1e, 0xeb, 0x2b, 0xdb, 0xad,
            0x27, 0x31, 0x65, 0x34, 0x01, 0x6f,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn hmac_sha1_rfc2202_case1() {
        // Anahtar = 0x0b x 20, veri = "Hi There"
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac = HmacSha1::sign(&key, data);
        let expected = [
            0xb6, 0x17, 0x31, 0x86, 0x55, 0x05, 0x72, 0x64, 0xe2, 0x8b, 0xc0, 0xb6, 0xfb, 0x37,
            0x8c, 0x8e, 0xf1, 0x46, 0xbe, 0x00,
        ];
        assert_eq!(mac, expected);
        assert!(HmacSha1::verify(&key, data, &mac));
    }

    #[test]
    fn hmac_sha1_rfc2202_case2() {
        // Anahtar = "Jefe", veri = "what do ya want for nothing?"
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let mac = HmacSha1::sign(key, data);
        let expected = [
            0xef, 0xfc, 0xdf, 0x6a, 0xe5, 0xeb, 0x2f, 0xa2, 0xd2, 0x74, 0x16, 0xd5, 0xf1, 0x84,
            0xdf, 0x9c, 0x25, 0x9a, 0x7c, 0x79,
        ];
        assert_eq!(mac, expected);
    }
}

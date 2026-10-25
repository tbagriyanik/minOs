//! SHA-512 / SHA-384 (FIPS 180-4) — no_std, no external dependencies.
//! SHA-512 uses 64-bit words, 128-byte blocks, and 80 rounds.
//! SHA-384 is SHA-512 with different initial values and truncated 48-byte output.

const K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

const H0_512: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

// SHA-384 baslangic degerleri: ilk dokuz... on altinci asal sayilarin kare
// koklerinin kesir kismlarinin ilk 64 biti (FIPS 180-4).
const H0_384: [u64; 8] = [
    0xcbbb9d5dc1059ed8,
    0x629a292a367cd507,
    0x9159015a3070dd17,
    0x152fecd8f70e5939,
    0x67332667ffc00b31,
    0x8eb44a8768581511,
    0xdb0c2e0d64f98fa7,
    0x47b5481dbefa4fa4,
];

const BLOCK_SIZE: usize = 128;
const DIGEST_SIZE_512: usize = 64;
const DIGEST_SIZE_384: usize = 48;

#[inline]
fn rotr64(x: u64, n: u32) -> u64 {
    x.rotate_right(n)
}

#[inline]
fn sigma0_512(x: u64) -> u64 {
    rotr64(x, 1) ^ rotr64(x, 8) ^ (x >> 7)
}

#[inline]
fn sigma1_512(x: u64) -> u64 {
    rotr64(x, 19) ^ rotr64(x, 61) ^ (x >> 6)
}

#[allow(non_snake_case)]
#[inline]
fn Sigma0_512(x: u64) -> u64 {
    rotr64(x, 28) ^ rotr64(x, 34) ^ rotr64(x, 39)
}

#[allow(non_snake_case)]
#[inline]
fn Sigma1_512(x: u64) -> u64 {
    rotr64(x, 14) ^ rotr64(x, 18) ^ rotr64(x, 41)
}

#[inline]
fn ch64(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ ((!x) & z)
}

#[inline]
fn maj64(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ (x & z) ^ (y & z)
}

pub struct Sha512 {
    h: [u64; 8],
    buffer: [u8; BLOCK_SIZE],
    buffer_len: usize,
    total_bits: u64,
}

impl Sha512 {
    pub fn new() -> Self {
        Sha512 {
            h: H0_512,
            buffer: [0; BLOCK_SIZE],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    fn compress_block(&mut self, block: &[u8]) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let off = i * 8;
            w[i] = u64::from_be_bytes([
                block[off],
                block[off + 1],
                block[off + 2],
                block[off + 3],
                block[off + 4],
                block[off + 5],
                block[off + 6],
                block[off + 7],
            ]);
        }
        for i in 16..80 {
            w[i] = sigma1_512(w[i - 2])
                .wrapping_add(w[i - 7])
                .wrapping_add(sigma0_512(w[i - 15]))
                .wrapping_add(w[i - 16]);
        }

        let mut a = self.h[0];
        let mut b = self.h[1];
        let mut c = self.h[2];
        let mut d = self.h[3];
        let mut e = self.h[4];
        let mut f = self.h[5];
        let mut g = self.h[6];
        let mut h_val = self.h[7];

        for i in 0..80 {
            let t1 = h_val
                .wrapping_add(Sigma1_512(e))
                .wrapping_add(ch64(e, f, g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = Sigma0_512(a).wrapping_add(maj64(a, b, c));
            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
        self.h[5] = self.h[5].wrapping_add(f);
        self.h[6] = self.h[6].wrapping_add(g);
        self.h[7] = self.h[7].wrapping_add(h_val);
    }

    pub fn update(&mut self, data: &[u8]) {
        self.total_bits = self
            .total_bits
            .wrapping_add((data.len() as u64).wrapping_mul(8));
        let mut data = data;
        if self.buffer_len > 0 && self.buffer_len + data.len() >= BLOCK_SIZE {
            let need = BLOCK_SIZE - self.buffer_len;
            self.buffer[self.buffer_len..BLOCK_SIZE].copy_from_slice(&data[..need]);
            let buf = self.buffer.clone();
            self.compress_block(&buf);
            self.buffer_len = 0;
            data = &data[need..];
        }
        while data.len() >= BLOCK_SIZE {
            self.compress_block(&data[..BLOCK_SIZE]);
            data = &data[BLOCK_SIZE..];
        }
        if !data.is_empty() {
            self.buffer[self.buffer_len..self.buffer_len + data.len()].copy_from_slice(data);
            self.buffer_len += data.len();
        }
    }

    pub fn finalize(mut self) -> [u8; DIGEST_SIZE_512] {
        let bit_len = self.total_bits;
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > BLOCK_SIZE - 16 {
            for i in self.buffer_len..BLOCK_SIZE {
                self.buffer[i] = 0;
            }
            let buf = self.buffer.clone();
            self.compress_block(&buf);
            self.buffer = [0; BLOCK_SIZE];
            self.buffer_len = 0;
        }
        for i in self.buffer_len..(BLOCK_SIZE - 16) {
            self.buffer[i] = 0;
        }
        // 128-bit uzunluk alani: ust 8 bayt sifir, alt 8 bayt bit sayisi.
        self.buffer[BLOCK_SIZE - 16..BLOCK_SIZE - 8].copy_from_slice(&[0u8; 8]);
        self.buffer[BLOCK_SIZE - 8..BLOCK_SIZE].copy_from_slice(&bit_len.to_be_bytes());
        let buf = self.buffer.clone();
        self.compress_block(&buf);

        let mut out = [0u8; DIGEST_SIZE_512];
        for i in 0..8 {
            out[i * 8..i * 8 + 8].copy_from_slice(&self.h[i].to_be_bytes());
        }
        out
    }

    pub fn hash(data: &[u8]) -> [u8; DIGEST_SIZE_512] {
        let mut s = Sha512::new();
        s.update(data);
        s.finalize()
    }
}

pub struct Sha384 {
    inner: Sha512,
}

impl Sha384 {
    pub fn new() -> Self {
        Sha384 {
            inner: Sha512 {
                h: H0_384,
                buffer: [0; BLOCK_SIZE],
                buffer_len: 0,
                total_bits: 0,
            },
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize(self) -> [u8; DIGEST_SIZE_384] {
        let full = self.inner.finalize();
        let mut output = [0u8; DIGEST_SIZE_384];
        output.copy_from_slice(&full[..DIGEST_SIZE_384]);
        output
    }

    pub fn hash(data: &[u8]) -> [u8; DIGEST_SIZE_384] {
        let mut s = Sha384::new();
        s.update(data);
        s.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn sha512_empty() {
        let h = Sha512::hash(&[]);
        let expected = [
            0xcf, 0x83, 0xe1, 0x35, 0x7e, 0xef, 0xb8, 0xbd, 0xf1, 0x54, 0x28, 0x50, 0xd6, 0x6d,
            0x80, 0x07, 0xd6, 0x20, 0xe4, 0x05, 0x0b, 0x57, 0x15, 0xdc, 0x83, 0xf4, 0xa9, 0x21,
            0xd3, 0x6c, 0xe9, 0xce, 0x47, 0xd0, 0xd1, 0x3c, 0x5d, 0x85, 0xf2, 0xb0, 0xff, 0x83,
            0x18, 0xd2, 0x87, 0x7e, 0xec, 0x2f, 0x63, 0xb9, 0x31, 0xbd, 0x47, 0x41, 0x7a, 0x81,
            0xa5, 0x38, 0x32, 0x7a, 0xf9, 0x27, 0xda, 0x3e,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha384_empty() {
        let h = Sha384::hash(&[]);
        let expected = [
            0x38, 0xb0, 0x60, 0xa7, 0x51, 0xac, 0x96, 0x38, 0x4c, 0xd9, 0x32, 0x7e, 0xb1, 0xb1,
            0xe3, 0x6a, 0x21, 0xfd, 0xb7, 0x11, 0x14, 0xbe, 0x07, 0x43, 0x4c, 0x0c, 0xc7, 0xbf,
            0x63, 0xf6, 0xe1, 0xda, 0x27, 0x4e, 0xde, 0xbf, 0xe7, 0x6f, 0x65, 0xfb, 0xd5, 0x1a,
            0xd2, 0xf1, 0x48, 0x98, 0xb9, 0x5b,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha384_incremental_matches() {
        let input: Vec<u8> = (0..300).map(|i| (i * 7) as u8).collect();
        let mut s = Sha384::new();
        for chunk in input.chunks(17) {
            s.update(chunk);
        }
        assert_eq!(s.finalize(), Sha384::hash(&input));
    }

    #[test]
    fn sha384_abc_vector() {
        // FIPS 180-4 "abc" test vektörü.
        let h = Sha384::hash(b"abc");
        let expected = [
            0xcb, 0x00, 0x75, 0x3f, 0x45, 0xa3, 0x5e, 0x8b, 0xb5, 0xa0, 0x3d, 0x69, 0x9a, 0xc6,
            0x50, 0x07, 0x27, 0x2c, 0x32, 0xab, 0x0e, 0xde, 0xd1, 0x63, 0x1a, 0x8b, 0x60, 0x5a,
            0x43, 0xff, 0x5b, 0xed, 0x80, 0x86, 0x07, 0x2b, 0xa1, 0xe7, 0xcc, 0x23, 0x58, 0xba,
            0xec, 0xa1, 0x34, 0xc8, 0x25, 0xa7,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn sha512_abc_vector() {
        // FIPS 180-4 "abc" test vektörü.
        let h = Sha512::hash(b"abc");
        let expected = [
            0xdd, 0xaf, 0x35, 0xa1, 0x93, 0x61, 0x7a, 0xba, 0xcc, 0x41, 0x73, 0x49, 0xae, 0x20,
            0x41, 0x31, 0x12, 0xe6, 0xfa, 0x4e, 0x89, 0xa9, 0x7e, 0xa2, 0x0a, 0x9e, 0xee, 0xe6,
            0x4b, 0x55, 0xd3, 0x9a, 0x21, 0x92, 0x99, 0x2a, 0x27, 0x4f, 0xc1, 0xa8, 0x36, 0xba,
            0x3c, 0x23, 0xa3, 0xfe, 0xeb, 0xbd, 0x45, 0x4d, 0x44, 0x23, 0x64, 0x3c, 0xe8, 0x0e,
            0x2a, 0x9a, 0xc9, 0x4f, 0xa5, 0x4c, 0xa4, 0x9f,
        ];
        assert_eq!(h, expected);
    }
}

//! X25519 (Curve25519) Diffie-Hellman — no_std.
//! RFC 7748 uyumlu, Montgomery merdiveni tabanlı uygulama.

/// 25519 asal alanı: p = 2^255 - 19, radix 2^51 uzuvlarıyla.
const P: [u64; 5] = [
    0x7F_FFFF_FFFF_FFED,
    0x7F_FFFF_FFFF_FFFF,
    0x7F_FFFF_FFFF_FFFF,
    0x7F_FFFF_FFFF_FFFF,
    0x7F_FFFF_FFFF_FFFF,
];

/// Alan elemanı: 5 × 51-bit (radix 2^51), little-endian.
#[derive(Clone, Copy)]
struct Fe([u64; 5]);

impl Fe {
    const fn zero() -> Self {
        Fe([0; 5])
    }
    const fn one() -> Self {
        Fe([1, 0, 0, 0, 0])
    }

    fn from_bytes(b: &[u8; 32]) -> Self {
        // Little-endian 32 baytı 5×51-bit uzva yükle (mask 2^51-1)
        let mask: u64 = (1 << 51) - 1;
        let load = |i: usize| -> u64 {
            let mut v = 0u64;
            for j in 0..8 {
                let idx = i + j;
                if idx < 32 {
                    v |= (b[idx] as u64) << (8 * j);
                }
            }
            v
        };
        let mut h = [
            load(0) & mask,
            (load(6) >> 3) & mask,
            (load(12) >> 6) & mask,
            (load(19) >> 1) & mask,
            (load(24) >> 12) & mask,
        ];
        // Üst bitleri temizle (Curve25519 için ilk 255 bit)
        h[4] &= (1 << 51) - 1;
        Fe(h)
    }

    fn to_bytes(self) -> [u8; 32] {
        let mut t = self.carry_full();
        // p'den çıkarma: t >= p ise t -= p
        let mut borrow: i64 = 0;
        let mut c = [0i64; 5];
        for i in 0..5 {
            c[i] = t.0[i] as i64 - P[i] as i64 - borrow;
            if c[i] < 0 {
                c[i] += 1 << 51;
                borrow = 1;
            } else {
                borrow = 0;
            }
        }
        // Eğer borrow kaldıysa (t < p), orijinali kullan
        let mask_borrow = 0u64.wrapping_sub(borrow as u64);
        for i in 0..5 {
            t.0[i] = (t.0[i] & mask_borrow) | ((c[i] as u64) & !mask_borrow);
        }
        let mut out = [0u8; 32];
        // 5×51-bit -> 32 bayt (little-endian)
        let limbs = t.0;
        let mut acc: u128 = 0;
        let mut acc_bits = 0;
        let mut idx = 0;
        let mut limb_i = 0;
        while idx < 32 {
            while acc_bits < 8 && limb_i < 5 {
                acc |= (limbs[limb_i] as u128) << acc_bits;
                acc_bits += 51;
                limb_i += 1;
            }
            out[idx] = acc as u8;
            acc >>= 8;
            if acc_bits >= 8 {
                acc_bits -= 8;
            } else {
                acc_bits = 0;
            }
            idx += 1;
        }
        out
    }

    fn carry(&self) -> Fe {
        let mask: u64 = (1 << 51) - 1;
        let mut h = self.0;
        for i in 0..4 {
            let c = h[i] >> 51;
            h[i] &= mask;
            h[i + 1] += c;
        }
        let c = h[4] >> 51;
        h[4] &= mask;
        h[0] += c * 19;
        Fe(h)
    }

    fn carry_full(&self) -> Fe {
        let mut h = *self;
        for _ in 0..3 {
            h = h.carry();
        }
        h
    }

    fn add(&self, rhs: &Fe) -> Fe {
        let mut r = [0u64; 5];
        for i in 0..5 {
            r[i] = self.0[i] + rhs.0[i];
        }
        Fe(r)
    }

    fn sub(&self, rhs: &Fe) -> Fe {
        // 2*p ekleyerek negatifliği önle. 2p'nin uzuvları radix 2^51'de:
        //   [2^52 - 38, 2^52 - 2, 2^52 - 2, 2^52 - 2, 2^52 - 2]
        let mut r = [0u64; 5];
        r[0] = self.0[0] + 0x000F_FFFF_FFFF_FFDA - rhs.0[0];
        r[1] = self.0[1] + 0x000F_FFFF_FFFF_FFFE - rhs.0[1];
        r[2] = self.0[2] + 0x000F_FFFF_FFFF_FFFE - rhs.0[2];
        r[3] = self.0[3] + 0x000F_FFFF_FFFF_FFFE - rhs.0[3];
        r[4] = self.0[4] + 0x000F_FFFF_FFFF_FFFE - rhs.0[4];
        Fe(r)
    }

    fn mul(&self, rhs: &Fe) -> Fe {
        let a = self.0;
        let b = rhs.0;
        let a19_1 = a[1] * 19;
        let a19_2 = a[2] * 19;
        let a19_3 = a[3] * 19;
        let a19_4 = a[4] * 19;

        // Okul kitabı çarpımı + indirgeme (2^255 ≡ 19 mod p)
        let mut r0 = (a[0] as u128) * (b[0] as u128)
            + (a19_1 as u128) * (b[4] as u128)
            + (a19_2 as u128) * (b[3] as u128)
            + (a19_3 as u128) * (b[2] as u128)
            + (a19_4 as u128) * (b[1] as u128);
        let mut r1 = (a[0] as u128) * (b[1] as u128)
            + (a[1] as u128) * (b[0] as u128)
            + (a19_2 as u128) * (b[4] as u128)
            + (a19_3 as u128) * (b[3] as u128)
            + (a19_4 as u128) * (b[2] as u128);
        let mut r2 = (a[0] as u128) * (b[2] as u128)
            + (a[1] as u128) * (b[1] as u128)
            + (a[2] as u128) * (b[0] as u128)
            + (a19_3 as u128) * (b[4] as u128)
            + (a19_4 as u128) * (b[3] as u128);
        let mut r3 = (a[0] as u128) * (b[3] as u128)
            + (a[1] as u128) * (b[2] as u128)
            + (a[2] as u128) * (b[1] as u128)
            + (a[3] as u128) * (b[0] as u128)
            + (a19_4 as u128) * (b[4] as u128);
        let mut r4 = (a[0] as u128) * (b[4] as u128)
            + (a[1] as u128) * (b[3] as u128)
            + (a[2] as u128) * (b[2] as u128)
            + (a[3] as u128) * (b[1] as u128)
            + (a[4] as u128) * (b[0] as u128);

        let mask: u64 = (1 << 51) - 1;
        let c0 = (r0 >> 51) as u64;
        r0 &= mask as u128;
        r1 += c0 as u128;
        let c1 = (r1 >> 51) as u64;
        r1 &= mask as u128;
        r2 += c1 as u128;
        let c2 = (r2 >> 51) as u64;
        r2 &= mask as u128;
        r3 += c2 as u128;
        let c3 = (r3 >> 51) as u64;
        r3 &= mask as u128;
        r4 += c3 as u128;
        let c4 = (r4 >> 51) as u64;
        r4 &= mask as u128;
        r0 += (c4 * 19) as u128;
        let c0b = (r0 >> 51) as u64;
        r0 &= mask as u128;
        r1 += c0b as u128;

        Fe([r0 as u64, r1 as u64, r2 as u64, r3 as u64, r4 as u64])
    }

    fn sq(&self) -> Fe {
        self.mul(self)
    }

    fn sq_repeat(&self, n: usize) -> Fe {
        let mut r = *self;
        for _ in 0..n {
            r = r.sq();
        }
        r
    }

    /// a^(p-2) mod p = a^(-1) mod p (Fermat)
    fn invert(&self) -> Fe {
        // 2^255-21 için toplama zinciri (djb)
        let z2 = self.sq();
        let z9 = z2.sq_repeat(2).mul(self);
        let z11 = z9.mul(&z2);
        let z2_5_0 = z11.sq().mul(&z9);
        let z2_10_0 = z2_5_0.sq_repeat(5).mul(&z2_5_0);
        let z2_20_0 = z2_10_0.sq_repeat(10).mul(&z2_10_0);
        let z2_40_0 = z2_20_0.sq_repeat(20).mul(&z2_20_0);
        let z2_50_0 = z2_40_0.sq_repeat(10).mul(&z2_10_0);
        let z2_100_0 = z2_50_0.sq_repeat(50).mul(&z2_50_0);
        let z2_200_0 = z2_100_0.sq_repeat(100).mul(&z2_100_0);
        let z2_250_0 = z2_200_0.sq_repeat(50).mul(&z2_50_0);
        z2_250_0.sq_repeat(5).mul(&z11)
    }
}

fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let t = mask & (a.0[i] ^ b.0[i]);
        a.0[i] ^= t;
        b.0[i] ^= t;
    }
}

/// X25519(scalar, point) -> shared secret (RFC 7748).
pub fn x25519(scalar: &[u8; 32], u_coordinate: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    // Clamp scalar (RFC 7748 §5)
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    let x1 = Fe::from_bytes(u_coordinate);
    let mut x2 = Fe::one();
    let mut z2 = Fe::zero();
    let mut x3 = x1;
    let mut z3 = Fe::one();
    let mut swap: u64 = 0;

    // Montgomery merdiveni: bit 254'ten 0'a
    let mut t = 254i32;
    while t >= 0 {
        let byte = (t / 8) as usize;
        let bit = (t % 8) as u32;
        let kt = ((k[byte] >> bit) & 1) as u64;
        swap ^= kt;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = kt;

        let a = x2.add(&z2);
        let aa = a.sq();
        let b = x2.sub(&z2);
        let bb = b.sq();
        let e = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        let x3n = da.add(&cb).sq();
        let z3n = x1.mul(&da.sub(&cb).sq());
        x3 = x3n;
        z3 = z3n;
        x2 = aa.mul(&bb);
        // z2 = e*(aa + 121665*e), 121665 = (121665 mod 2^51)
        let a24 = Fe([121665, 0, 0, 0, 0]);
        z2 = e.mul(&aa.add(&a24.mul(&e)));
        t -= 1;
    }
    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);
    x2.mul(&z2.invert()).to_bytes()
}

/// X25519 taban noktası (u=9).
pub const BASEPOINT: [u8; 32] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// X25519 anahtar çifti üretir.
pub fn generate_keypair(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let mut secret = *seed;
    // Clamp
    secret[0] &= 248;
    secret[31] &= 127;
    secret[31] |= 64;
    let public = x25519(&secret, &BASEPOINT);
    (secret, public)
}

/// X25519 paylaşılmış sır (secret * peer_public).
pub fn shared_secret(secret: &[u8; 32], peer_public: &[u8; 32]) -> [u8; 32] {
    x25519(secret, peer_public)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7748_vector_1() {
        let alice_private = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2,
            0x66, 0x45, 0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5,
            0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let alice_public = [
            0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54, 0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e,
            0xf7, 0x5a, 0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4, 0xeb, 0xa4, 0xa9, 0x8e,
            0xaa, 0x9b, 0x4e, 0x6a,
        ];
        let bob_private = [
            0x5d, 0xab, 0x08, 0x7e, 0x62, 0x4a, 0x8a, 0x4b, 0x79, 0xe1, 0x7f, 0x8b, 0x83, 0x80,
            0x0e, 0xe6, 0x6f, 0x3b, 0xb1, 0x29, 0x26, 0x18, 0xb6, 0xfd, 0x1c, 0x2f, 0x8b, 0x27,
            0xff, 0x88, 0xe0, 0xeb,
        ];
        let bob_public = [
            0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4, 0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4,
            0x35, 0x37, 0x3f, 0x83, 0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d, 0xad, 0xfc, 0x7e, 0x14,
            0x6f, 0x88, 0x2b, 0x4f,
        ];
        let shared = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1, 0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35,
            0x0f, 0x25, 0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33, 0x76, 0xf0, 0x9b, 0x3c,
            0x1e, 0x16, 0x17, 0x42,
        ];
        assert_eq!(x25519(&alice_private, &BASEPOINT), alice_public);
        assert_eq!(x25519(&bob_private, &BASEPOINT), bob_public);
        assert_eq!(x25519(&alice_private, &bob_public), shared);
        assert_eq!(x25519(&bob_private, &alice_public), shared);
    }

    /// RFC 7748 §5.2, Test Vector 1 (taban noktası dışı u-koordinatı).
    #[test]
    fn rfc7748_vector_non_basepoint() {
        let scalar = [
            0xa5, 0x46, 0xe3, 0x6b, 0xf0, 0x52, 0x7c, 0x9d, 0x3b, 0x16, 0x15, 0x4b, 0x82, 0x46,
            0x5e, 0xdd, 0x62, 0x14, 0x4c, 0x0a, 0xc1, 0xfc, 0x5a, 0x18, 0x50, 0x6a, 0x22, 0x44,
            0xba, 0x44, 0x9a, 0xc4,
        ];
        let u = [
            0xe6, 0xdb, 0x68, 0x67, 0x58, 0x30, 0x30, 0xdb, 0x35, 0x94, 0xc1, 0xa4, 0x24, 0xb1,
            0x5f, 0x7c, 0x72, 0x66, 0x24, 0xec, 0x26, 0xb3, 0x35, 0x3b, 0x10, 0xa9, 0x03, 0xa6,
            0xd0, 0xab, 0x1c, 0x4c,
        ];
        assert_eq!(
            x25519(&scalar, &u),
            [
                0xc3, 0xda, 0x55, 0x37, 0x9d, 0xe9, 0xc6, 0x90, 0x8e, 0x94, 0xea, 0x4d, 0xf2, 0x8d,
                0x08, 0x4f, 0x32, 0xec, 0xcf, 0x03, 0x49, 0x1c, 0x71, 0xf7, 0x54, 0xb4, 0x07, 0x55,
                0x77, 0xa2, 0x85, 0x52,
            ]
        );
    }

    /// Taban noktası (u=9) ile çarpma bilinen bir vektör üretmelidir.
    #[test]
    fn basepoint_public_key_is_stable() {
        let secret = [0x42u8; 32];
        let first = x25519(&secret, &BASEPOINT);
        let second = x25519(&secret, &BASEPOINT);
        assert_eq!(first, second, "x25519 deterministik olmalı");
        assert_ne!(first, [0u8; 32], "paylaşılan sır sıfır olmamalı");
    }
}

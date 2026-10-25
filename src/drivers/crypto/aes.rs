//! AES (FIPS 197) — ECB encrypt/decrypt, 128 and 256 bit keys — no_std.

use alloc::vec::Vec;

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const fn build_inv_sbox() -> [u8; 256] {
    let mut s = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        s[SBOX[i] as usize] = i as u8;
        i += 1;
    }
    s
}

const INV_SBOX: [u8; 256] = build_inv_sbox();

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn xtime(x: u8) -> u8 {
    let mut b = (x << 1) as u16;
    if x & 0x80 != 0 {
        b ^= 0x011b;
    }
    b as u8
}

fn sub_word(w: u32) -> u32 {
    let b0 = SBOX[(w >> 24) as usize];
    let b1 = SBOX[((w >> 16) & 0xFF) as usize];
    let b2 = SBOX[((w >> 8) & 0xFF) as usize];
    let b3 = SBOX[(w & 0xFF) as usize];
    ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32)
}

fn rot_word(w: u32) -> u32 {
    (w << 8) | (w >> 24)
}

pub struct Aes {
    rounds: usize,
    encrypt_keys: Vec<u32>,
    decrypt_keys: Vec<u32>,
}

impl Aes {
    pub fn new(key: &[u8]) -> Result<Self, ()> {
        let nk = match key.len() {
            16 => 4,
            24 => 6,
            32 => 8,
            _ => return Err(()),
        };
        let nr = nk + 6;
        let nb = 4;

        let mut w: Vec<u32> = Vec::with_capacity(nb * (nr + 1));
        for i in 0..nk {
            w.push(u32::from_be_bytes([
                key[i * 4],
                key[i * 4 + 1],
                key[i * 4 + 2],
                key[i * 4 + 3],
            ]));
        }
        let mut i = nk;
        while i < nb * (nr + 1) {
            let mut temp = w[i - 1];
            if i % nk == 0 {
                // RCON occupies the most significant byte of the round constant word.
                temp = sub_word(rot_word(temp)) ^ ((RCON[(i / nk - 1) as usize] as u32) << 24);
            } else if nk > 6 && i % nk == 4 {
                temp = sub_word(temp);
            }
            w.push(w[i - nk] ^ temp);
            i += 1;
        }

        let encrypt_keys = w.clone();

        // Straightforward inverse cipher (FIPS-197 Fig. 15): the encryption round
        // keys are consumed in descending order, w[Nr] .. w[0]. InvMixColumns is
        // applied to the state after AddRoundKey, so the keys themselves stay
        // unchanged.
        let mut decrypt_keys: Vec<u32> = Vec::with_capacity(w.len());
        for round in (0..=nr).rev() {
            decrypt_keys.extend_from_slice(&w[round * nb..(round + 1) * nb]);
        }

        Ok(Aes {
            rounds: nr,
            encrypt_keys,
            decrypt_keys,
        })
    }

    fn add_round_key(state: &mut [u8; 16], rk: &[u32]) {
        for i in 0..4 {
            let col = rk[i];
            state[i * 4] ^= (col >> 24) as u8;
            state[i * 4 + 1] ^= (col >> 16) as u8;
            state[i * 4 + 2] ^= (col >> 8) as u8;
            state[i * 4 + 3] ^= col as u8;
        }
    }

    fn sub_bytes(state: &mut [u8; 16], sbox: &[u8; 256]) {
        for b in state.iter_mut() {
            *b = sbox[*b as usize];
        }
    }

    fn shift_rows(state: &mut [u8; 16]) {
        let mut t = [0u8; 16];
        t[0] = state[0];
        t[1] = state[5];
        t[2] = state[10];
        t[3] = state[15];
        t[4] = state[4];
        t[5] = state[9];
        t[6] = state[14];
        t[7] = state[3];
        t[8] = state[8];
        t[9] = state[13];
        t[10] = state[2];
        t[11] = state[7];
        t[12] = state[12];
        t[13] = state[1];
        t[14] = state[6];
        t[15] = state[11];
        state.copy_from_slice(&t);
    }

    fn inv_shift_rows(state: &mut [u8; 16]) {
        let mut t = [0u8; 16];
        t[0] = state[0];
        t[1] = state[13];
        t[2] = state[10];
        t[3] = state[7];
        t[4] = state[4];
        t[5] = state[1];
        t[6] = state[14];
        t[7] = state[11];
        t[8] = state[8];
        t[9] = state[5];
        t[10] = state[2];
        t[11] = state[15];
        t[12] = state[12];
        t[13] = state[9];
        t[14] = state[6];
        t[15] = state[3];
        state.copy_from_slice(&t);
    }

    fn mix_columns(state: &mut [u8; 16]) {
        for c in 0..4 {
            let s0 = state[c * 4];
            let s1 = state[c * 4 + 1];
            let s2 = state[c * 4 + 2];
            let s3 = state[c * 4 + 3];
            state[c * 4] = gmul_02(s0) ^ gmul_03(s1) ^ s2 ^ s3;
            state[c * 4 + 1] = s0 ^ gmul_02(s1) ^ gmul_03(s2) ^ s3;
            state[c * 4 + 2] = s0 ^ s1 ^ gmul_02(s2) ^ gmul_03(s3);
            state[c * 4 + 3] = gmul_03(s0) ^ s1 ^ s2 ^ gmul_02(s3);
        }
    }

    fn inv_mix_columns(state: &mut [u8; 16]) {
        for c in 0..4 {
            let s0 = state[c * 4];
            let s1 = state[c * 4 + 1];
            let s2 = state[c * 4 + 2];
            let s3 = state[c * 4 + 3];
            state[c * 4] = gmul_0e(s0) ^ gmul_0b(s1) ^ gmul_0d(s2) ^ gmul_09(s3);
            state[c * 4 + 1] = gmul_09(s0) ^ gmul_0e(s1) ^ gmul_0b(s2) ^ gmul_0d(s3);
            state[c * 4 + 2] = gmul_0d(s0) ^ gmul_09(s1) ^ gmul_0e(s2) ^ gmul_0b(s3);
            state[c * 4 + 3] = gmul_0b(s0) ^ gmul_0d(s1) ^ gmul_09(s2) ^ gmul_0e(s3);
        }
    }

    pub fn encrypt(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut state = *input;
        let nb = 4;
        Self::add_round_key(&mut state, &self.encrypt_keys[0..nb]);
        for round in 1..self.rounds {
            Self::sub_bytes(&mut state, &SBOX);
            Self::shift_rows(&mut state);
            Self::mix_columns(&mut state);
            Self::add_round_key(&mut state, &self.encrypt_keys[round * nb..(round + 1) * nb]);
        }
        Self::sub_bytes(&mut state, &SBOX);
        Self::shift_rows(&mut state);
        Self::add_round_key(
            &mut state,
            &self.encrypt_keys[self.rounds * nb..(self.rounds + 1) * nb],
        );
        state
    }

    pub fn decrypt(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut state = *input;
        let nb = 4;
        Self::add_round_key(&mut state, &self.decrypt_keys[0..nb]);
        for round in 1..self.rounds {
            Self::inv_shift_rows(&mut state);
            Self::sub_bytes(&mut state, &INV_SBOX);
            Self::add_round_key(&mut state, &self.decrypt_keys[round * nb..(round + 1) * nb]);
            Self::inv_mix_columns(&mut state);
        }
        Self::inv_shift_rows(&mut state);
        Self::sub_bytes(&mut state, &INV_SBOX);
        Self::add_round_key(
            &mut state,
            &self.decrypt_keys[self.rounds * nb..(self.rounds + 1) * nb],
        );
        state
    }
}

#[inline(always)]
fn gmul_02(x: u8) -> u8 {
    xtime(x)
}

#[inline(always)]
fn gmul_03(x: u8) -> u8 {
    xtime(x) ^ x
}

#[inline(always)]
fn gmul_09(x: u8) -> u8 {
    xtime(xtime(xtime(x))) ^ x
}

#[inline(always)]
fn gmul_0b(x: u8) -> u8 {
    xtime(xtime(xtime(x))) ^ xtime(x) ^ x
}

#[inline(always)]
fn gmul_0d(x: u8) -> u8 {
    xtime(xtime(xtime(x))) ^ xtime(xtime(x)) ^ x
}

#[inline(always)]
fn gmul_0e(x: u8) -> u8 {
    xtime(xtime(xtime(x))) ^ xtime(xtime(x)) ^ xtime(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_128_ecb_rfc3602_vector1() {
        // RFC 3602 Appendix A.1: AES-128 ECB
        let key = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let input = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let aes = Aes::new(&key).unwrap();
        let ct = aes.encrypt(&input);
        // Sifir anahtar + sifir blok icin beklenen AES-128 ECB ciktisi
        // (.NET Aes / ECB / NoPadding referansi ile dogrulandi).
        let expected = [
            0x66, 0xe9, 0x4b, 0xd4, 0xef, 0x8a, 0x2c, 0x3b, 0x88, 0x4c, 0xfa, 0x59, 0xca, 0x34,
            0x2b, 0x2e,
        ];
        assert_eq!(ct, expected);
        let pt = aes.decrypt(&ct);
        assert_eq!(pt, input);
    }

    #[test]
    fn aes_256_ecb() {
        let key: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let input: [u8; 16] = [0; 16];
        let aes = Aes::new(&key).unwrap();
        let ct = aes.encrypt(&input);
        let pt = aes.decrypt(&ct);
        assert_eq!(pt, input);
    }

    #[test]
    fn aes_128_known_vector() {
        // FIPS 197 Example: key 0001...0f, input 0011...ff
        let key: [u8; 16] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let input: [u8; 16] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let aes = Aes::new(&key).unwrap();
        let ct = aes.encrypt(&input);
        let expected = [
            0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
            0xc5, 0x5a,
        ];
        assert_eq!(ct, expected);
        let pt = aes.decrypt(&ct);
        assert_eq!(pt, input);
    }
}

//! Kernel randomness backed by HC-128 (`rand_hc`).
//! The seed is mixed with the boot TSC; callers must not use this as a
//! cryptographic entropy source until a hardware entropy source is available.
use core::sync::atomic::{AtomicU64, Ordering};
use rand_core::{Rng, SeedableRng};

static STATE: AtomicU64 = AtomicU64::new(0x6d69_6e4f_532d_524e);

pub fn seed(seed: [u8; 32]) {
    let mut value = 0u64;
    for chunk in seed.chunks_exact(8) {
        value ^= u64::from_le_bytes(chunk.try_into().unwrap());
    }
    STATE.store(value | 1, Ordering::Release);
}

pub fn random_u32() -> u32 {
    let n = STATE.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    let mut s = [0u8; 32];
    for (i, b) in s.iter_mut().enumerate() {
        *b = (n.rotate_left((i as u32) & 31) as u8) ^ (i as u8).wrapping_mul(17);
    }
    let mut rng = rand_hc::Hc128Rng::from_seed(s);
    rng.next_u32()
}

pub fn random_u64() -> u64 {
    ((random_u32() as u64) << 32) | random_u32() as u64
}

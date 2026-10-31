//! Kernel randomness.
//!
//! `rand_hc`/`rand_core` bağımlılığı kaldırıldı: üretici artık içeride,
//! sayaç karıştırma (bölme tabanlı değil, kaydırma + FNV çarpımı) ile
//! çalışır. Tohum TSC/boot entropy ile karıştırılır; donanım entropi
//! kaynağı gelene kadar kriptografik amaçla kullanılmamalıdır.
use core::sync::atomic::{AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0x6d69_6e4f_532d_524e);

pub fn seed(seed: [u8; 32]) {
    let mut value = 0u64;
    for chunk in seed.chunks_exact(8) {
        value ^= u64::from_le_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
    }
    STATE.store(value | 1, Ordering::Release);
}

/// xorshift64* adımı.
fn next_u64() -> u64 {
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    STATE.store(x, Ordering::Relaxed);
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

pub fn random_u32() -> u32 {
    (next_u64() >> 32) as u32
}

pub fn random_u64() -> u64 {
    next_u64()
}

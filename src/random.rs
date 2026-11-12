//! Kernel randomness.
//!
//! `rand_hc`/`rand_core` bağımlılığı kaldırıldı: üretici artık içeride,
//! sayaç karıştırma (bölme tabanlı değil, kaydırma + FNV çarpımı) ile
//! çalışır. Tohum TSC/boot entropy ile karıştırılır; donanım entropi
//! kaynağı gelene kadar kriptografik amaçla kullanılmamalıdır.
//!
//! Donanım rastgelelik: CPUID ile RDRAND (leaf 0x01 ECX bit 30) ve
//! RDSEED (leaf 0x07 EBX bit 18) desteği tespit edilir; mevcutsa
//! donanım entropisi doğrudan kullanılır, yoksa mevcut xorshift64*
//! PRNG'e düşülür.

use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0x6d69_6e4f_532d_524e);
static HW_RNG: AtomicBool = AtomicBool::new(false);

/// CPUID ile RDRAND desteği var mı?
fn has_rdrand() -> bool {
    let info = __cpuid(0x1);
    (info.ecx >> 30) & 1 == 1
}

/// CPUID ile RDSEED desteği var mı?
fn has_rdseed() -> bool {
    let info = __cpuid(0x7);
    (info.ebx >> 18) & 1 == 1
}

/// RDRAND 64-bit. Başarılıysa CF=1 ve rastgele değer döner.
fn rdrand64() -> Option<u64> {
    let mut val: u64 = 0;
    let ok: u8;
    unsafe {
        core::arch::asm!("rdrand {0}; setc {1}", out(reg) val, out(reg_byte) ok, options(nomem, nostack, preserves_flags));
    }
    if ok != 0 {
        Some(val)
    } else {
        None
    }
}

/// RDSEED 64-bit. Başarılıysa CF=1 ve rastgele değer döner.
fn rdseed64() -> Option<u64> {
    let mut val: u64 = 0;
    let ok: u8;
    unsafe {
        core::arch::asm!("rdseed {0}; setc {1}", out(reg) val, out(reg_byte) ok, options(nomem, nostack, preserves_flags));
    }
    if ok != 0 {
        Some(val)
    } else {
        None
    }
}

/// Donanım entropisi ile bir doldurma denemesi. `true` dönerse tüm baytlar
/// donanım Rastgelelik kaynağından gelmiştir.
fn fill_hw(buf: &mut [u8]) -> bool {
    if !HW_RNG.load(Ordering::Acquire) {
        return false;
    }
    let mut offset = 0;
    while offset < buf.len() {
        let remaining = buf.len() - offset;
        let chunk = if remaining >= 8 { 8 } else { remaining };
        let val = if has_rdseed() { rdseed64() } else { rdrand64() };
        let Some(v) = val else {
            // Donanım geçici olarak uygun değil; PRNG'e düş.
            return false;
        };
        let bytes = v.to_le_bytes();
        buf[offset..offset + chunk].copy_from_slice(&bytes[..chunk]);
        offset += chunk;
    }
    true
}

/// Donanım entropisi mevcutsa `true` döner; aksi halde mevcut PRNG kullanılır.
pub fn hw_available() -> bool {
    HW_RNG.load(Ordering::Acquire)
}

/// Donanım Rastgelelik kaynağını tespit eder ve PRNG'i tohumlar.
///
/// Çekirdek başlangıcında (`_start`) çağrılmalıdır. RDRAND/RDSEED mevcutsa
/// donanım entropisi kullanılır; yoksa mevcut xorshift64* PRNG TSC/boot
/// entropy ile tohumlanmaya devam eder.
pub fn init() {
    let hw = has_rdseed() || has_rdrand();
    HW_RNG.store(hw, Ordering::Release);

    if hw {
        // Donanım entropisiyle PRNG durumunu karıştır.
        let mut seed_val = [0u8; 32];
        if fill_hw(&mut seed_val) {
            seed(seed_val);
            return;
        }
    }

    // Yedek: TSC ile tohumla.
    let tsc = crate::boot::time::read_cpu_cycles();
    let mut seed_val = [0u8; 32];
    seed_val[..8].copy_from_slice(&tsc.to_le_bytes());
    seed_val[8..16].copy_from_slice(&(tsc.wrapping_mul(0x9e37_79b9_7f4a_7c15)).to_le_bytes());
    seed(seed_val);
}

/// PRNG'i belirli bir tohumla başlatır (testler için).
pub fn seed(seed: [u8; 32]) {
    let mut value = 0u64;
    for chunk in seed.as_chunks::<8>().0 {
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

/// `buf`'yi kriptografik olarak yeterli rastgelelikle doldurur.
///
/// Donanım entropisi (RDRAND/RDSEED) mevcutsa doğrudan kullanılır; yoksa
/// mevcut xorshift64* PRNG'den baytlar üretilir.
pub fn fill_bytes(buf: &mut [u8]) {
    if fill_hw(buf) {
        return;
    }
    // PRNG'den 8 baytlık bloklar üret.
    let mut offset = 0;
    while offset < buf.len() {
        let remaining = buf.len() - offset;
        let chunk = if remaining >= 8 { 8 } else { remaining };
        let val = random_u64().to_le_bytes();
        buf[offset..offset + chunk].copy_from_slice(&val[..chunk]);
        offset += chunk;
    }
}

//! Zaman tabani: TSC okuma, PIT kanal 2 ile kalibrasyon ve monotonik milisaniye.

use core::sync::atomic::{AtomicBool, Ordering};

static TIMEBASE_READY: AtomicBool = AtomicBool::new(false);

pub fn read_cpu_cycles() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        // `nomem` bilerek KULLANILMAZ: rdtsc bir bellek engeli (barrier)
        // olarak davranmalıdır. `nomem` ile derleyici, zaman damgası okumasını
        // belleğe yazan iş yüklerinin *önüne* taşıyabiliyor; boylece kısa
        // ölçümler (benchmark, kare süresi) tamamen yanlış değer veriyordu.
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags),
        );
    }
    ((high as u64) << 32) | low as u64
}

pub static mut TSC_CYCLES_PER_US: u64 = 1;

fn estimate_tsc_cycles_per_us() -> u64 {
    let max_leaf = core::arch::x86_64::__cpuid(0).eax;
    if max_leaf >= 0x15 {
        let ratio = core::arch::x86_64::__cpuid(0x15);
        if ratio.eax != 0 && ratio.ebx != 0 && ratio.ecx != 0 {
            let hz = (ratio.ecx as u64).saturating_mul(ratio.ebx as u64) / ratio.eax as u64;
            if hz != 0 {
                return (hz / 1_000_000).max(1);
            }
        }
    }
    if max_leaf >= 0x16 {
        let mhz = core::arch::x86_64::__cpuid(0x16).eax as u64;
        if mhz != 0 {
            return mhz;
        }
    }
    1_000
}

/// TSC frekansını kalibre eder.
///
/// CPUID leaf 0x15/0x16'den TSC frekans tahminini alır. PIT kanal 2 ile
/// port-polling kalibrasyonu QEMU'da takılma riski taşıdığı (PIT çıkışı
/// port 0x61 bit 5'ini set etmeyebilir) çıkarılmıştır; CPUID estime
/// `monotonic_ms()` için yeterince doğrudur.
pub fn init_timebase() -> bool {
    let cycles_per_us = estimate_tsc_cycles_per_us();
    unsafe {
        TSC_CYCLES_PER_US = cycles_per_us;
    }
    TIMEBASE_READY.store(true, Ordering::Release);
    true
}

pub fn is_ready() -> bool {
    TIMEBASE_READY.load(Ordering::Acquire)
}

pub fn monotonic_ms() -> u64 {
    // Zaman tabanı kurulmadan önce TSC_CYCLES_PER_US=1 olduğundan
    // okuma astronomik değer üretir; hazır olana dek 0 döndür.
    if !is_ready() {
        return 0;
    }
    let cycles_per_us = unsafe { TSC_CYCLES_PER_US }.max(1);
    read_cpu_cycles() / cycles_per_us / 1_000
}

pub fn main_monotonic_ms() -> usize {
    monotonic_ms() as usize
}

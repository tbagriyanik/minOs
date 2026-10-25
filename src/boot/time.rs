//! Zaman tabani: TSC okuma, PIT kanal 2 ile kalibrasyon ve monotonik milisaniye.

use core::sync::atomic::{AtomicBool, Ordering};

static TIMEBASE_READY: AtomicBool = AtomicBool::new(false);

pub fn read_cpu_cycles() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
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

/// TSC frekansını PIT kanal 2 ile kalibre eder.
pub fn init_timebase() -> bool {
    let fallback_cycles_per_us = estimate_tsc_cycles_per_us();
    unsafe {
        TSC_CYCLES_PER_US = fallback_cycles_per_us;
    }
    unsafe {
        let mut port61: u8;
        core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
        let saved = port61;
        port61 = (port61 & !0x03) | 0x01;
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
        // PIT kanal 2 mode 0, 65536 sayım ≈ 54.9 ms.
        core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") 0u8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") 0u8, options(nomem, nostack, preserves_flags));
        let start = read_cpu_cycles();
        let timeout_cycles = fallback_cycles_per_us.saturating_mul(1_000_000);
        let mut ready = false;
        for _ in 0..10_000_000 {
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
            if port61 & 0x20 != 0 {
                ready = true;
                break;
            }
            if read_cpu_cycles().wrapping_sub(start) >= timeout_cycles {
                break;
            }
            core::hint::spin_loop();
        }
        if ready {
            let elapsed = read_cpu_cycles().wrapping_sub(start).max(1);
            TSC_CYCLES_PER_US = (elapsed / 54_925).max(1);
        }
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") saved, options(nomem, nostack, preserves_flags));
        TIMEBASE_READY.store(true, Ordering::Release);
        ready
    }
}

pub fn is_ready() -> bool {
    TIMEBASE_READY.load(Ordering::Acquire)
}

pub fn monotonic_ms() -> u64 {
    let cycles_per_us = unsafe { TSC_CYCLES_PER_US }.max(1);
    read_cpu_cycles() / cycles_per_us / 1_000
}

pub fn main_monotonic_ms() -> usize {
    monotonic_ms() as usize
}

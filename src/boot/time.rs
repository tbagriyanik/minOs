//! Zaman tabani: TSC okuma, PIT kanal 2 ile kalibrasyon ve monotonik milisaniye.

fn read_cpu_cycles() -> u64 {
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

static mut TSC_CYCLES_PER_US: u64 = 1;

/// TSC frekansını PIT kanal 2 ile kalibre eder.
fn init_timebase() {
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
        loop {
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
            if port61 & 0x20 != 0 { break; }
            core::hint::spin_loop();
        }
        let elapsed = read_cpu_cycles().wrapping_sub(start).max(1);
        TSC_CYCLES_PER_US = (elapsed / 54_925).max(1);
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") saved, options(nomem, nostack, preserves_flags));
    }
}

pub fn monotonic_ms() -> u64 {
    let cycles_per_us = unsafe { TSC_CYCLES_PER_US }.max(1);
    read_cpu_cycles() / cycles_per_us / 1_000
}

pub(crate) fn main_monotonic_ms() -> usize { monotonic_ms() as usize }

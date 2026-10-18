//! CPU ureticisi ve mantiksal cekirdek sayisi.

use crate::arch;

pub fn cpu_vendor() -> &'static str {
    let result = core::arch::x86_64::__cpuid(0);
    let ebx = result.ebx;
    let edx = result.edx;
    let ecx = result.ecx;
    if ebx == u32::from_le_bytes(*b"Genu") && edx == u32::from_le_bytes(*b"ineI") && ecx == u32::from_le_bytes(*b"ntel") {
        "Intel"
    } else if ebx == u32::from_le_bytes(*b"Auth") && edx == u32::from_le_bytes(*b"enti") && ecx == u32::from_le_bytes(*b"cAMD") {
        "AMD"
    } else {
        "x86_64 uyumlu"
    }
}

pub fn cpu_logical_count() -> u32 {
    let result = core::arch::x86_64::__cpuid(1);
    let from_cpuid = ((result.ebx >> 16) & 0xFF).max(1);
    // SMP açılışı tamamlandıysa gerçek çevrimiçi CPU sayısını yansıt.
    let from_smp = arch::x86_64::smp::cpu_count() as u32;
    from_cpuid.max(from_smp).max(1)
}

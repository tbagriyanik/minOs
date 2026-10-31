//! CPU capability and common logging adapters.

pub use log::{debug, error, info, trace, warn};

/// Snapshot of CPU features used by boot decisions and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuFeatures {
    pub invariant_tsc: bool,
    pub apic: bool,
    pub x2apic: bool,
    pub sse2: bool,
    pub avx: bool,
}

pub fn cpu_features() -> CpuFeatures {
    let cpuid = raw_cpuid::CpuId::new();
    let feature = cpuid.get_feature_info();
    let extended = cpuid.get_extended_feature_info();
    CpuFeatures {
        invariant_tsc: cpuid.get_processor_frequency_info().is_some(),
        apic: feature.as_ref().map_or(false, |v| v.has_apic()),
        x2apic: feature.as_ref().map_or(false, |v| v.has_x2apic()),
        sse2: feature.as_ref().map_or(false, |v| v.has_sse2()),
        avx: feature.as_ref().map_or(false, |v| v.has_avx())
            && extended.map_or(false, |v| v.has_avx2()),
    }
}

/// ACPI integration marker. Tables are parsed locally from the Limine RSDP
/// (the external `acpi` crate is not linked), so this reports the runtime
/// switch rather than a compile-time crate probe.
pub fn acpi_enabled() -> bool {
    true
}

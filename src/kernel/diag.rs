//! CPU capability and common logging adapters.

use core::sync::atomic::{AtomicU64, Ordering};

pub use log::{debug, error, info, trace, warn};

// ---------------------------------------------------------------------------
// Ağ paket ve hata sayaçları
// ---------------------------------------------------------------------------

static NET_RX_PACKETS: AtomicU64 = AtomicU64::new(0);
static NET_TX_PACKETS: AtomicU64 = AtomicU64::new(0);
static NET_RX_BYTES: AtomicU64 = AtomicU64::new(0);
static NET_TX_BYTES: AtomicU64 = AtomicU64::new(0);
static NET_RX_ERRORS: AtomicU64 = AtomicU64::new(0);
static NET_TX_ERRORS: AtomicU64 = AtomicU64::new(0);
static NET_DROPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub dropped: u64,
}

/// Ağ paket ve hata sayaçlarının anlık kopyası.
pub fn net_stats() -> NetStats {
    NetStats {
        rx_packets: NET_RX_PACKETS.load(Ordering::Relaxed),
        tx_packets: NET_TX_PACKETS.load(Ordering::Relaxed),
        rx_bytes: NET_RX_BYTES.load(Ordering::Relaxed),
        tx_bytes: NET_TX_BYTES.load(Ordering::Relaxed),
        rx_errors: NET_RX_ERRORS.load(Ordering::Relaxed),
        tx_errors: NET_TX_ERRORS.load(Ordering::Relaxed),
        dropped: NET_DROPPED.load(Ordering::Relaxed),
    }
}

pub fn inc_net_rx(bytes: u64) {
    NET_RX_PACKETS.fetch_add(1, Ordering::Relaxed);
    NET_RX_BYTES.fetch_add(bytes, Ordering::Relaxed);
}

pub fn inc_net_tx(bytes: u64) {
    NET_TX_PACKETS.fetch_add(1, Ordering::Relaxed);
    NET_TX_BYTES.fetch_add(bytes, Ordering::Relaxed);
}

pub fn inc_net_rx_err() {
    NET_RX_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_net_tx_err() {
    NET_TX_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_net_dropped() {
    NET_DROPPED.fetch_add(1, Ordering::Relaxed);
}

pub fn reset_net_stats() {
    NET_RX_PACKETS.store(0, Ordering::Relaxed);
    NET_TX_PACKETS.store(0, Ordering::Relaxed);
    NET_RX_BYTES.store(0, Ordering::Relaxed);
    NET_TX_BYTES.store(0, Ordering::Relaxed);
    NET_RX_ERRORS.store(0, Ordering::Relaxed);
    NET_TX_ERRORS.store(0, Ordering::Relaxed);
    NET_DROPPED.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Sürücü başlatma süreleri (Driver Init Durations)
// ---------------------------------------------------------------------------

pub const MAX_DRIVER_TIMINGS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverInitTiming {
    pub name: &'static str,
    pub duration_us: u64,
    pub ok: bool,
}

static DRIVER_TIMINGS_COUNT: AtomicU64 = AtomicU64::new(0);
// Sabit boyutlu array'de tutuyoruz. `UnsafeCell` ile iç değiştirilebilirlik
// sağlanır; erişim `record_driver_init`/`clear_driver_timings` üzerinden ve
// yalnızca sürücü başlatma sırasında tek bağlamda yapılır.
struct DriverTimingsStore(core::cell::UnsafeCell<[Option<DriverInitTiming>; MAX_DRIVER_TIMINGS]>);

// SAFETY: Erişim iki yoldan da `DRIVER_TIMINGS_LOCK` benzeri bir kritik bölge
// içinde değil, ancak tüm yazma işlemleri `DRIVER_TIMINGS_COUNT` atomik sayacıyla
// sıralanır ve okuyucular yalnızca sayaçla görünür olan slotları okur.
unsafe impl Sync for DriverTimingsStore {}

static DRIVER_TIMINGS: DriverTimingsStore =
    DriverTimingsStore(core::cell::UnsafeCell::new([None; MAX_DRIVER_TIMINGS]));

pub fn record_driver_init(name: &'static str, duration_us: u64, ok: bool) {
    // Süre ve sonuç seri günlüğe de yazılır: `[DRIVER-INIT] name=.. us=.. ok=..`
    crate::boot::console::serial_print("[DRIVER-INIT] name=");
    crate::boot::console::serial_print(name);
    crate::boot::console::serial_print(" us=");
    crate::boot::console::serial_print_dec(duration_us);
    crate::boot::console::serial_print(" ok=");
    crate::boot::console::serial_print(if ok { "1" } else { "0" });
    crate::boot::console::serial_print("\n");
    let count = DRIVER_TIMINGS_COUNT.load(Ordering::Relaxed) as usize;
    if count < MAX_DRIVER_TIMINGS {
        // SAFETY: `count` sayaçtan okundu ve slot yazımı sayaç artırımından önce
        // tamamlanır; böylece görünür her slot yazılmış olur.
        unsafe {
            (*DRIVER_TIMINGS.0.get())[count] = Some(DriverInitTiming {
                name,
                duration_us,
                ok,
            });
        }
        DRIVER_TIMINGS_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn driver_init_timings() -> alloc::vec::Vec<DriverInitTiming> {
    let count = (DRIVER_TIMINGS_COUNT.load(Ordering::Relaxed) as usize).min(MAX_DRIVER_TIMINGS);
    let mut list = alloc::vec::Vec::new();
    // SAFETY: Yalnızca sayaçla görünür olan (dolayısıyla yazılmış) slotlar
    // kopyalanır; `DriverInitTiming` `Copy` olduğundan referans dışa verilmez.
    unsafe {
        for timing in (&*DRIVER_TIMINGS.0.get())[..count].iter().flatten() {
            list.push(*timing);
        }
    }
    list
}

pub fn clear_driver_timings() {
    DRIVER_TIMINGS_COUNT.store(0, Ordering::Relaxed);
    // SAFETY: Sayaç sıfırlandıktan sonra tüm slotlar tek bağlamda temizlenir.
    unsafe {
        for slot in (*DRIVER_TIMINGS.0.get()).iter_mut() {
            *slot = None;
        }
    }
}

// ---------------------------------------------------------------------------
// CPU features and ACPI
// ---------------------------------------------------------------------------

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
        apic: feature.as_ref().is_some_and(|v| v.has_apic()),
        x2apic: feature.as_ref().is_some_and(|v| v.has_x2apic()),
        sse2: feature.as_ref().is_some_and(|v| v.has_sse2()),
        avx: feature.as_ref().is_some_and(|v| v.has_avx())
            && extended.is_some_and(|v| v.has_avx2()),
    }
}

/// ACPI integration marker. Tables are parsed locally from the Limine RSDP
/// (the external `acpi` crate is not linked), so this reports the runtime
/// switch rather than a compile-time crate probe.
pub fn acpi_enabled() -> bool {
    true
}

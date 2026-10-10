pub mod video;
pub mod mouse;
pub mod net;
pub mod audio;
pub mod hda;
pub mod e1000;
pub mod netstack;
pub mod keyboard;
pub mod ps2;
pub mod rtc;

// --- HHDM ofset yönetimi: fiziksel adresleri sanal adrese çevirir ---
static mut HHDM_OFFSET: u64 = 0;

pub fn set_hhdm_offset(v: u64) {
    unsafe { HHDM_OFFSET = v; }
}

/// Fiziksel adresi HHDM üzerinden sanal adrese çevirir.
pub fn phys_to_virt(p: u64) -> u64 {
    unsafe { p + HHDM_OFFSET }
}

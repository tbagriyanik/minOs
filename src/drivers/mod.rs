pub mod video;
pub mod gpu;
pub mod mouse;
pub mod net;
pub mod audio;
pub mod hda;
pub mod e1000;
pub mod netstack;
pub mod socket;
pub mod tls;
pub mod http;
pub mod crypto;
pub mod keyboard;
pub mod ps2;
pub mod rtc;
pub mod ata;
pub mod usb;
pub mod usb_hid;
pub mod usb_keyboard;
pub mod usb_mouse;
pub mod camera;
pub mod wifi;
pub mod virtio_net;
pub mod i219;
pub mod atheros;
pub mod marvell;
pub mod bluetooth;
pub mod ahci;
pub mod nvme;
pub mod virtio_blk;
pub mod sdmmc;
pub mod usb_mass_storage;
pub mod usb_printer;
pub mod usb_audio;
pub mod usb_touchscreen;
pub mod virtio_gpu;
pub mod amdgpu;
pub mod nvidia;
pub mod acpi_battery;
pub mod acpi_power;
pub mod fan_temp;
pub mod pcie;
pub mod pci;
pub mod timer;

// --- HHDM ofset yönetimi: fiziksel adresleri sanal adrese çevirir ---
static mut HHDM_OFFSET: u64 = 0;

pub fn set_hhdm_offset(v: u64) {
    unsafe { HHDM_OFFSET = v; }
}

/// Fiziksel adresi HHDM üzerinden sanal adrese çevirir.
pub fn phys_to_virt(p: u64) -> u64 {
    unsafe { p + HHDM_OFFSET }
}

#![allow(dead_code, unused_variables, static_mut_refs)]

//! PCIe (PCI Express) Driver - PCIe aygıt yönetimi
//!
//! Bu driver, PCIe veri yolunu yönetir ve PCIe aygıtlarını tespit eder.
//! PCIe konfigürasyon uzayı, MMIO adresleme ve özellikleri destekler.
//!
//! TR: Bu sürücü, PCIe veri yolunu yönetir ve PCIe aygıtlarını tespit eder.
//! PCIe konfigürasyon uzayı, MMIO adresleme ve özellikleri destekler.

use core::arch::asm;

// ---------------------------------------------------------------------------
// I/O port yardımcıları (PCI için)
// ---------------------------------------------------------------------------

#[inline]
fn io_out32(port: u16, val: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn io_in32(port: u16) -> u32 {
    let ret: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

// ---------------------------------------------------------------------------
// PCI Konfigürasyon Erişimi
// ---------------------------------------------------------------------------

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[inline]
fn pci_read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in32(PCI_CONFIG_DATA)
}

#[inline]
fn pci_write32(bus: u8, dev: u8, func: u8, off: u8, val: u32) {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out32(PCI_CONFIG_DATA, val);
}

// ---------------------------------------------------------------------------
// PCIe Aygıt Bilgileri
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PcieDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub irq: u8,
}

// ---------------------------------------------------------------------------
// PCIe Konfigürasyon Registerları
// ---------------------------------------------------------------------------

const PCI_COMMAND: u8 = 0x04;
const PCI_STATUS: u8 = 0x08;
const PCI_CLASS: u8 = 0x0B;
const PCI_HEADER_TYPE: u8 = 0x0E;
const PCI_BAR0: u8 = 0x10;
const PCI_BAR1: u8 = 0x14;
const PCI_BAR2: u8 = 0x18;
const PCI_BAR3: u8 = 0x1C;
const PCI_BAR4: u8 = 0x20;
const PCI_BAR5: u8 = 0x24;
const PCI_INTERRUPT_LINE: u8 = 0x3C;

// Command register bitleri
const PCI_CMD_IO_SPACE: u16 = 1 << 0;
const PCI_CMD_MEMORY_SPACE: u16 = 1 << 1;
const PCI_CMD_BUS_MASTER: u16 = 1 << 2;
const PCI_CMD_SPECIAL_CYCLES: u16 = 1 << 3;
const PCI_CMD_MEM_WR_INVALID: u16 = 1 << 4;
const PCI_CMD_VGA_PALETTE_SNOOP: u16 = 1 << 5;
const PCI_CMD_PARITY_ERROR: u16 = 1 << 6;
const PCI_CMD_SERR_ENABLE: u16 = 1 << 8;

// Status register bitleri
const PCI_STATUS_CAP_LIST: u16 = 1 << 4;
const PCI_STATUS_66MHZ: u16 = 1 << 5;
const PCI_STATUS_FAST_BACK2BACK: u16 = 1 << 7;
const PCI_STATUS_DEVSEL: u16 = 0x0600;

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut INITIALIZED: bool = false;
static mut DEVICES: [PcieDevice; 256] = [PcieDevice {
    bus: 0,
    device: 0,
    function: 0,
    vendor_id: 0,
    device_id: 0,
    class: 0,
    subclass: 0,
    prog_if: 0,
    revision: 0,
    irq: 0,
}; 256];
static mut DEVICE_COUNT: usize = 0;

// ---------------------------------------------------------------------------
// PCIe Başlatma
// ---------------------------------------------------------------------------

pub fn init() -> bool {
    unsafe {
        if INITIALIZED {
            return true;
        }

        scan_pci_bus();
        INITIALIZED = true;

        true
    }
}

// ---------------------------------------------------------------------------
// PCI Bus Taraması
// ---------------------------------------------------------------------------

fn scan_pci_bus() {
    unsafe {
        DEVICE_COUNT = 0;

        for bus in 0..=255u8 {
            for dev in 0..=31u8 {
                for func in 0..=7u8 {
                    let vendor_device = pci_read32(bus, dev, func, 0);
                    let vendor_id = (vendor_device & 0xFFFF) as u16;
                    let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;

                    if vendor_id == 0xFFFF {
                        continue; // Aygıt yok
                    }

                    let class_rev = pci_read32(bus, dev, func, 0x08);
                    let class = ((class_rev >> 24) & 0xFF) as u8;
                    let subclass = ((class_rev >> 16) & 0xFF) as u8;
                    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
                    let revision = (class_rev & 0xFF) as u8;

                    let irq = pci_read32(bus, dev, func, 0x3C) as u8;

                    if DEVICE_COUNT < DEVICES.len() {
                        DEVICES[DEVICE_COUNT] = PcieDevice {
                            bus,
                            device: dev,
                            function: func,
                            vendor_id,
                            device_id,
                            class,
                            subclass,
                            prog_if,
                            revision,
                            irq,
                        };
                        DEVICE_COUNT += 1;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_initialized() -> bool {
    unsafe { INITIALIZED }
}

pub fn name() -> &'static str {
    "PCI Express"
}

pub fn name_tr() -> &'static str {
    "PCI Express"
}

pub fn device_count() -> usize {
    unsafe { DEVICE_COUNT }
}

pub fn get_device(index: usize) -> Option<PcieDevice> {
    unsafe {
        if index < DEVICE_COUNT {
            Some(DEVICES[index])
        } else {
            None
        }
    }
}

/// Belirli bir class/subclass ile aygıt ara
pub fn find_device(class: u8, subclass: u8) -> Option<PcieDevice> {
    unsafe {
        for i in 0..DEVICE_COUNT {
            if DEVICES[i].class == class && DEVICES[i].subclass == subclass {
                return Some(DEVICES[i]);
            }
        }
        None
    }
}

/// Belirli bir vendor/device ile aygıt ara
pub fn find_device_by_ids(vendor_id: u16, device_id: u16) -> Option<PcieDevice> {
    unsafe {
        for i in 0..DEVICE_COUNT {
            if DEVICES[i].vendor_id == vendor_id && DEVICES[i].device_id == device_id {
                return Some(DEVICES[i]);
            }
        }
        None
    }
}

/// BAR adresini oku
pub fn read_bar(device: &PcieDevice, bar_index: u8) -> Option<u64> {
    let bar_offset = PCI_BAR0 + (bar_index * 4);
    let bar = pci_read32(device.bus, device.device, device.function, bar_offset);

    if bar & 0x1 == 0 {
        // Memory-mapped I/O
        let bar64 = if (bar & 0x6) == 0x4 {
            // 64-bit address
            let bar_high = pci_read32(device.bus, device.device, device.function, bar_offset + 4);
            ((bar_high as u64) << 32) | ((bar & 0xFFFF_FFF0) as u64)
        } else {
            (bar & 0xFFFF_FFF0) as u64
        };
        Some(bar64)
    } else {
        // I/O port
        None
    }
}

/// Command register'ı ayarla
pub fn set_command(device: &PcieDevice, commands: u16) {
    let current = pci_read32(device.bus, device.device, device.function, PCI_COMMAND) as u16;
    pci_write32(
        device.bus,
        device.device,
        device.function,
        PCI_COMMAND,
        (current | commands) as u32,
    );
}

/// Bus master'ı etkinleştir
pub fn enable_bus_master(device: &PcieDevice) {
    set_command(device, PCI_CMD_BUS_MASTER);
}

/// Memory space'i etkinleştir
pub fn enable_memory_space(device: &PcieDevice) {
    set_command(device, PCI_CMD_MEMORY_SPACE);
}

/// I/O space'i etkinleştir
pub fn enable_io_space(device: &PcieDevice) {
    set_command(device, PCI_CMD_IO_SPACE);
}

/// PCIe capability listesini oku
pub fn read_capabilities(device: &PcieDevice) -> Option<u8> {
    let status = pci_read32(device.bus, device.device, device.function, PCI_STATUS) as u16;
    if status & PCI_STATUS_CAP_LIST != 0 {
        let header = pci_read32(device.bus, device.device, device.function, PCI_HEADER_TYPE);
        let cap_ptr = ((header >> 8) & 0xFF) as u8;
        Some(cap_ptr)
    } else {
        None
    }
}

#![allow(dead_code, unused_variables, static_mut_refs)]

//! PCI (Peripheral Component Interconnect) Driver - PCI aygıt yönetimi
//!
//! Bu driver, PCI veri yolunu yönetir ve PCI aygıtlarını tespit eder.
//! Legacy PCI ve PCI aygıtları için temel yönetim işlevleri sağlar.
//!
//! TR: Bu sürücü, PCI veri yolunu yönetir ve PCI aygıtlarını tespit eder.
//! Legacy PCI ve PCI aygıtları için temel yönetim işlevleri sağlar.

use core::arch::asm;

// ---------------------------------------------------------------------------
// I/O port yardımcıları
// ---------------------------------------------------------------------------

#[inline]
fn io_out8(port: u16, val: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn io_in8(port: u16) -> u8 {
    let ret: u8;
    unsafe { asm!("in al, dx", in("dx") port, out("al") ret, options(nomem, nostack, preserves_flags)); }
    ret
}

#[inline]
fn io_out16(port: u16, val: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags)); }
    ret
}

#[inline]
fn io_out32(port: u16, val: u32) {
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn io_in32(port: u16) -> u32 {
    let ret: u32;
    unsafe { asm!("in eax, dx", in("dx") port, out("eax") ret, options(nomem, nostack, preserves_flags)); }
    ret
}

// ---------------------------------------------------------------------------
// PCI Konfigürasyon Erişimi
// ---------------------------------------------------------------------------

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[inline]
fn pci_read8(bus: u8, dev: u8, func: u8, off: u8) -> u8 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in8(PCI_CONFIG_DATA + (off & 0x3) as u16)
}

#[inline]
fn pci_read16(bus: u8, dev: u8, func: u8, off: u8) -> u16 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in16(PCI_CONFIG_DATA + (off & 0x2) as u16)
}

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
fn pci_write8(bus: u8, dev: u8, func: u8, off: u8, val: u8) {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out8(PCI_CONFIG_DATA + (off & 0x3) as u16, val);
}

#[inline]
fn pci_write16(bus: u8, dev: u8, func: u8, off: u8, val: u16) {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out16(PCI_CONFIG_DATA + (off & 0x2) as u16, val);
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
// PCI Class Kodları
// ---------------------------------------------------------------------------

const PCI_CLASS_UNCLASSIFIED: u8 = 0x00;
const PCI_CLASS_STORAGE: u8 = 0x01;
const PCI_CLASS_NETWORK: u8 = 0x02;
const PCI_CLASS_DISPLAY: u8 = 0x03;
const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
const PCI_CLASS_MEMORY: u8 = 0x05;
const PCI_CLASS_BRIDGE: u8 = 0x06;
const PCI_CLASS_COMMUNICATION: u8 = 0x07;
const PCI_CLASS_SYSTEM: u8 = 0x08;
const PCI_CLASS_INPUT: u8 = 0x09;
const PCI_CLASS_DOCKING: u8 = 0x0A;
const PCI_CLASS_PROCESSOR: u8 = 0x0B;
const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;

// ---------------------------------------------------------------------------
// PCI Aygıt Bilgileri
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
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
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut INITIALIZED: bool = false;
static mut DEVICES: [PciDevice; 256] = [PciDevice {
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
// PCI Başlatma
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

                    let irq = pci_read8(bus, dev, func, 0x3C);

                    if DEVICE_COUNT < DEVICES.len() {
                        DEVICES[DEVICE_COUNT] = PciDevice {
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
    "PCI Bus"
}

pub fn name_tr() -> &'static str {
    "PCI Veri Yolu"
}

pub fn device_count() -> usize {
    unsafe { DEVICE_COUNT }
}

pub fn get_device(index: usize) -> Option<PciDevice> {
    unsafe {
        if index < DEVICE_COUNT {
            Some(DEVICES[index])
        } else {
            None
        }
    }
}

/// Belirli bir class ile aygıt ara
pub fn find_device(class: u8) -> Option<PciDevice> {
    unsafe {
        for i in 0..DEVICE_COUNT {
            if DEVICES[i].class == class {
                return Some(DEVICES[i]);
            }
        }
        None
    }
}

/// Belirli bir class/subclass ile aygıt ara
pub fn find_device_subclass(class: u8, subclass: u8) -> Option<PciDevice> {
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
pub fn find_device_by_ids(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
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
pub fn read_bar(device: &PciDevice, bar_index: u8) -> Option<u32> {
    let bar_offset = 0x10 + (bar_index * 4);
    let bar = pci_read32(device.bus, device.device, device.function, bar_offset);

    if bar & 0x1 == 0 {
        // Memory-mapped I/O
        Some(bar & 0xFFFF_FFF0)
    } else {
        // I/O port
        Some(bar & 0xFFFC)
    }
}

/// Konfigürasyon register'ı oku
pub fn read_config(device: &PciDevice, offset: u8) -> u32 {
    pci_read32(device.bus, device.device, device.function, offset)
}

/// Konfigürasyon register'ına yaz
pub fn write_config(device: &PciDevice, offset: u8, value: u32) {
    pci_write32(device.bus, device.device, device.function, offset, value)
}

/// Interrupt line'ı oku
pub fn read_interrupt_line(device: &PciDevice) -> u8 {
    pci_read8(device.bus, device.device, device.function, 0x3C)
}

/// Interrupt pin'ini oku
pub fn read_interrupt_pin(device: &PciDevice) -> u8 {
    pci_read8(device.bus, device.device, device.function, 0x3D)
}

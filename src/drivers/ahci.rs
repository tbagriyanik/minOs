#![allow(dead_code, unused_variables, static_mut_refs)]

//! AHCI (Advanced Host Controller Interface) SATA Driver
//!
//! AHCI, modern SATA denetleyicileri için standart bir arayüzdür.
//! ATA yerine daha gelişmiş özellikler (NCQ, hot-plug, port multipliers) sunar.
//!
//! TR: AHCI, modern SATA denetleyicileri için standart bir arayüzdür.
//! ATA yerine daha gelişmiş özellikler (NCQ, hot-plug, port multipliers) sunar.

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
// PCI Erişimi
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
// AHCI PCI Class Kodları
// ---------------------------------------------------------------------------

const PCI_CLASS_SATA: u8 = 0x01;
const PCI_SUBCLASS_AHCI: u8 = 0x06;

// ---------------------------------------------------------------------------
// AHCI HBA (Host Bus Adapter) Kayıt Ofsetleri
// ---------------------------------------------------------------------------

const HBA_CAP: u32 = 0x00; // Host Capabilities
const HBA_GHC: u32 = 0x04; // Global Host Control
const HBA_IS: u32 = 0x08; // Interrupt Status
const HBA_PI: u32 = 0x0C; // Ports Implemented
const HBA_VS: u32 = 0x10; // Version

// GHC bitleri
const GHC_AE: u32 = 1 << 31; // AHCI Enable
const GHC_MRSM: u32 = 1 << 0; // MSI Revert to Single Message

// ---------------------------------------------------------------------------
// AHCI Port Kayıt Ofsetleri
// ---------------------------------------------------------------------------

const PORT_CLB: u32 = 0x00; // Command List Base
const PORT_CLB_U: u32 = 0x04; // Command List Base Upper
const PORT_FB: u32 = 0x08; // FIS Base
const PORT_FB_U: u32 = 0x0C; // FIS Base Upper
const PORT_IS: u32 = 0x10; // Interrupt Status
const PORT_IE: u32 = 0x14; // Interrupt Enable
const PORT_CMD: u32 = 0x18; // Command and Status
const PORT_TFD: u32 = 0x20; // Task File Data
const PORT_SIG: u32 = 0x24; // Signature
const PORT_SSTS: u32 = 0x28; // SATA Status
const PORT_SCTL: u32 = 0x2C; // SATA Control
const PORT_SERR: u32 = 0x30; // SATA Error
const PORT_CI: u32 = 0x38; // Command Issue

// PORT_CMD bitleri
const PORT_CMD_ST: u32 = 1 << 0; // Start
const PORT_CMD_SUD: u32 = 1 << 1; // Spin Up Device
const PORT_CMD_POD: u32 = 1 << 2; // Power On Device
const PORT_CMD_CLO: u32 = 1 << 3; // Command List Override
const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
const PORT_CMD_CR: u32 = 1 << 15; // Command List Running
const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Running
const PORT_CMD_ICC_MASK: u32 = 0xF << 28; // Interface Communication Control

// PORT_TFD bitleri
const PORT_TFD_BSY: u8 = 1 << 7; // Busy
const PORT_TFD_DRQ: u8 = 1 << 3; // Data Request

// PORT_SSTS bitleri
const PORT_SSTS_DET_MASK: u8 = 0x3; // Device Detection
const PORT_SSTS_DET_PRESENT: u8 = 0x3; // Device present

// ---------------------------------------------------------------------------
// Command Header Yapısı
// ---------------------------------------------------------------------------

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct CommandHeader {
    prdtl: u16, // Physical Region Descriptor Table Length
    prdbc: u16, // Physical Region Descriptor Byte Count
    ctba: u32,  // Command Table Descriptor Base Address
    ctbau: u32, // Command Table Descriptor Base Address Upper
    reserved: [u32; 4],
}

// ---------------------------------------------------------------------------
// FIS (Frame Information Structure) Türleri
// ---------------------------------------------------------------------------

const FIS_TYPE_REG_H2D: u8 = 0x27; // Register Host to Device
const FIS_TYPE_REG_D2H: u8 = 0x34; // Register Device to Host
const FIS_TYPE_DMA_ACT: u8 = 0x39; // DMA Activate
const FIS_TYPE_DMA_SETUP: u8 = 0x41; // DMA Setup
const FIS_TYPE_DATA: u8 = 0x46; // Data
const FIS_TYPE_BIST: u8 = 0x58; // BIST
const FIS_TYPE_PIO_SETUP: u8 = 0x5F; // PIO Setup
const FIS_TYPE_DEV_BITS: u8 = 0xA1; // Device Bits

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct FisRegH2D {
    fis_type: u8,
    pm_port: u8,
    command: u8,
    feature: u8,
    lba0: u8,
    lba1: u8,
    lba2: u8,
    device: u8,
    lba3: u8,
    lba4: u8,
    lba5: u8,
    feature_low: u8,
    count: u16,
    icc: u8,
    control: u8,
    reserved: [u8; 4],
}

// ---------------------------------------------------------------------------
// PRDT (Physical Region Descriptor Table)
// ---------------------------------------------------------------------------

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct PrdtEntry {
    dba: u32,  // Data Base Address
    dbau: u32, // Data Base Address Upper
    reserved: u32,
    dbc: u32, // Byte Count (0-indexed)
    i: u32,   // Interrupt on Completion
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut ABAR: u64 = 0; // HHDM sanal adresindeki AHCI Base Memory Register
static mut DETECTED: bool = false;
static mut PORT_COUNT: u32 = 0;
static mut PORT_BASE: u64 = 0;

// ---------------------------------------------------------------------------
// MMIO Yardımcı Fonksiyonları
// ---------------------------------------------------------------------------

#[inline]
fn hba_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (ABAR + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn hba_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (ABAR + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
}

#[inline]
fn port_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (PORT_BASE + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn port_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (PORT_BASE + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
}

// ---------------------------------------------------------------------------
// Cihaz Algılama
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    unsafe {
        if DETECTED {
            return true;
        }

        // PCI bus taranması
        for bus in 0..=8u8 {
            for dev in 0..=31u8 {
                for func in 0..=7u8 {
                    let class = pci_read32(bus, dev, func, 0x08) >> 24;
                    let subclass = (pci_read32(bus, dev, func, 0x08) >> 16) & 0xFF;

                    if class as u8 == PCI_CLASS_SATA && subclass as u8 == PCI_SUBCLASS_AHCI {
                        // ABAR adresini al (BAR5)
                        let bar5 = pci_read32(bus, dev, func, 0x24);
                        if bar5 & 0x1 == 0 {
                            let abar_phys = (bar5 & 0xFFFF_FFF0) as u64;
                            crate::arch::x86_64::mmio_map_2mb(abar_phys & !0x1F_FFFF);
                            ABAR = crate::drivers::phys_to_virt(abar_phys);
                            DETECTED = true;
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Cihaz Başlatma
// ---------------------------------------------------------------------------

pub fn init() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        // AHCI'yi etkinleştir
        let mut ghc = hba_read32(HBA_GHC);
        ghc |= GHC_AE;
        hba_write32(HBA_GHC, ghc);

        // Portları oku
        let pi = hba_read32(HBA_PI);
        PORT_COUNT = pi.count_ones();

        // İlk aktif portu seç
        for i in 0..32 {
            if pi & (1 << i) != 0 {
                PORT_BASE = ABAR + 0x100 + (i as u64 * 0x80);
                break;
            }
        }

        // Portu başlat
        // Stop command engine
        let mut cmd = port_read32(PORT_CMD);
        cmd &= !(PORT_CMD_ST | PORT_CMD_FRE);
        port_write32(PORT_CMD, cmd);

        // Wait for stop
        for _ in 0..100000 {
            if port_read32(PORT_CMD) & (PORT_CMD_CR | PORT_CMD_FR) == 0 {
                break;
            }
        }

        // FIS ve Command List belleğini ayarla
        // TODO: Bellek tahsisi ve adres ayarlama

        // Start command engine
        cmd = port_read32(PORT_CMD);
        cmd |= PORT_CMD_ST | PORT_CMD_FRE;
        port_write32(PORT_CMD, cmd);

        true
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn name() -> &'static str {
    "AHCI SATA"
}

pub fn name_tr() -> &'static str {
    "AHCI SATA"
}

pub fn port_count() -> u32 {
    unsafe { PORT_COUNT }
}

/// Sektör okuma
pub fn read_sector(lba: u64, buffer: &mut [u8]) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        // TODO: AHCI üzerinden LBA okuma
        // Command Header, FIS ve PRDT hazırla
        // Command Issue register'a yaz

        true
    }
}

/// Sektör yazma
pub fn write_sector(lba: u64, buffer: &[u8]) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        // TODO: AHCI üzerinden LBA yazma

        true
    }
}

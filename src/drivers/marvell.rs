//! Marvell Ethernet Driver - Marvell chip setleri için ağ sürücüsü
//!
//! Marvell, çeşitli Gigabit Ethernet denetleyicileri üreten bir şirkettir.
//! Özellikle 88E8056, 88E8070, 88E8072 gibi denetleyiciler için destek sağlar.
//!
//! Maturity: Kısmi — PCI tespiti ve temel başlatma var; RX/TX geliştiriliyor.
//! Bkz. docs/driver-maturity.md

use crate::drivers::port_io::*;

// ---------------------------------------------------------------------------
// Marvell PCI Kimlikleri
// ---------------------------------------------------------------------------

const MARVELL_VENDOR: u32 = 0x11AB; // Marvell Technology Group

// Marvell 88E8056/88E8070/88E8072 aygıt kimlikleri
const MARVELL_DEVICES: [u32; 10] = [
    0x4360, // 88E8056
    0x4361, // 88E8053
    0x4362, // 88E8052
    0x4363, // 88E8055
    0x4364, // 88E8056
    0x4365, // 88E8058
    0x4366, // 88E8070
    0x4367, // 88E8071
    0x4368, // 88E8072
    0x4369, // 88E8059
];

// ---------------------------------------------------------------------------
// Marvell PCI Config Register Ofsetleri (PCI Configuration Space)
// ---------------------------------------------------------------------------

// Marvell 88E8070/88E8072 kontrol registerları PCI config space üzerinden
const MRV_REG_CMD: u8 = 0x04; // Command register (PCI config offset)
const MRV_REG_STATUS: u8 = 0x04; // Status register (PCI config offset)

// Marvell register bit definitions (PCI config space bit positions)
const MRV_STATUS_LINK_UP: u32 = 1 << 2; // Link up from status register

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut DETECTED: bool = false;
static mut MAC_ADDR: [u8; 6] = [0; 6];

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
                    let vendor_id = pci_read32(bus, dev, func, 0) & 0xFFFF;
                    let device_id = (pci_read32(bus, dev, func, 0) >> 16) & 0xFFFF;

                    if vendor_id == MARVELL_VENDOR && MARVELL_DEVICES.contains(&device_id) {
                        DETECTED = true;

                        // MAC adresini oku (PCI config space'den)
                        let mac_low = pci_read32(bus, dev, func, 0x14);
                        let mac_high = pci_read32(bus, dev, func, 0x18);
                        MAC_ADDR[0] = (mac_low & 0xFF) as u8;
                        MAC_ADDR[1] = ((mac_low >> 8) & 0xFF) as u8;
                        MAC_ADDR[2] = ((mac_low >> 16) & 0xFF) as u8;
                        MAC_ADDR[3] = ((mac_low >> 24) & 0xFF) as u8;
                        MAC_ADDR[4] = (mac_high & 0xFF) as u8;
                        MAC_ADDR[5] = ((mac_high >> 8) & 0xFF) as u8;

                        return true;
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

/// Marvell Ethernet cihazını temel olarak başlatır.
/// PCI tarama sonucunda cihaz tespetilmiş olmalıdır.
pub fn init() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        // PCI Konfigürasyon alanından BAR0 (MMIO base adresi) çıkar
        let bar0 = pci_read32(0, 0, 0, 0x24);
        let mmio_valid = (bar0 & 1) == 0; // Memory space mi?

        if !mmio_valid {
            return false;
        }

        // 32-byte aligned adresi al
        let _mmio_base_addr = bar0 & !0xF;

        // Command register'ı oku
        let _cmd = pci_read32(0, 0, 0, MRV_REG_CMD);

        // Status register'dan link kontrolü
        let status = pci_read32(0, 0, 0, MRV_REG_STATUS);
        let _link_up = (status & MRV_STATUS_LINK_UP) != 0;

        // MAC adresini tekrar oku (garanti olsun diye)
        let mac_low = pci_read32(0, 0, 0, 0x14);
        let mac_high = pci_read32(0, 0, 0, 0x18);
        MAC_ADDR[0] = (mac_low & 0xFF) as u8;
        MAC_ADDR[1] = ((mac_low >> 8) & 0xFF) as u8;
        MAC_ADDR[2] = ((mac_low >> 16) & 0xFF) as u8;
        MAC_ADDR[3] = ((mac_low >> 24) & 0xFF) as u8;
        MAC_ADDR[4] = (mac_high & 0xFF) as u8;
        MAC_ADDR[5] = ((mac_high >> 8) & 0xFF) as u8;

        true
    }
}

/// Bağlantı durumunu kontrol eder (link up mı?)
/// PCI config space Status registerından kontrol yapar.
pub fn link_up_check() -> bool {
    let status = pci_read32(0, 0, 0, MRV_REG_STATUS);
    (status & MRV_STATUS_LINK_UP) != 0
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn mac_address() -> [u8; 6] {
    unsafe { MAC_ADDR }
}

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn name() -> &'static str {
    "Marvell Ethernet"
}

pub fn name_tr() -> &'static str {
    "Marvell Ethernet"
}

pub fn link_up() -> bool {
    link_up_check()
}

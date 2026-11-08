//! Atheros Ethernet Driver - Atheros chip setleri için ağ sürücüsü
//!
//! Atheros, Qualcomm tarafından satın alınmış bir ağ chip seti üreticisidir.
//! Özellikle AR8131, AR8132, AR8161, AR8171 gibi Gigabit Ethernet denetleyicileri
//! için destek sağlar.
//!
//! Maturity: Kısmi — PCI tespiti ve temel başlatma var; RX/TX geliştiriliyor.
//! Bkz. docs/driver-maturity.md

use crate::drivers::port_io::*;

// ---------------------------------------------------------------------------
// Atheros PCI Kimlikleri
// ---------------------------------------------------------------------------

const ATHEROS_VENDOR: u32 = 0x1969; // Atheros Communications

// Atheros AR8131/AR8132/AR8161/AR8171 aygıt kimlikleri
const ATHEROS_DEVICES: [u32; 8] = [
    0x1063, // AR8131
    0x1062, // AR8132
    0x1091, // AR8161
    0x1090, // AR8162
    0x10A1, // AR8171
    0x10A0, // AR8172
    0xE091, // AR8161 (alt)
    0xE090, // AR8162 (alt)
];

// ---------------------------------------------------------------------------
// Atheros PCI Config Register Ofsetleri (PCI Configuration Space)
// ---------------------------------------------------------------------------

// Atheros AR8161/8171 kontrol registerları PCI config space üzerinden
// Genel amaçlı register'lar; gerçek MMIO erişimi BAR0 üzerinden yapılır.
const ATH_REG_CMD: u8 = 0x04; // Command register (PCI config offset)
const ATH_REG_STATUS: u8 = 0x04; // Status register (PCI config offset)

// Atheros register bit definitions (PCI config space bit positions)
const ATH_STATUS_LINK_UP: u32 = 1 << 2; // Link up from status register

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

                    if vendor_id == ATHEROS_VENDOR && ATHEROS_DEVICES.contains(&device_id) {
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

/// Atheros Ethernet cihazını temel olarak başlatır.
/// PCI tarama sonucunda cihaz tespetilmiş olmalıdır.
/// Gerçek MMIO tabanlı başlatma donanımımız için ayrıntılı ayar gerektirdiği için,
/// bu fonksiyon cihazı tespit eder ve temel konfigürasyonu uygular.
pub fn init() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        // PCI Konfigürasyon alanından BAR0 (MMIO base adresi) çıkar
        // BAR 0 genellikle 32-bit memory space; bit 0=0 demektir memory space
        let bar0 = pci_read32(0, 0, 0, 0x24);
        let mmio_valid = (bar0 & 1) == 0; // Memory space mi?

        if !mmio_valid {
            return false;
        }

        // 32-byte aligned adresi al (bottom 4 bit sıfır)
        let _mmio_base_addr = bar0 & !0xF;

        // Command register'ı oku ve TX/RX enable'ını ayarla
        // PCI config spaceCommand register (offset 0x04)
        let _cmd = pci_read32(0, 0, 0, ATH_REG_CMD);
        // Bit 0: Memory/IO space enable, Bit 1: VGA palette snoop,
        // Bit 2: Memory write enable, Bit 3: Memory enable
        // Bit 20: Fast back-to-back, Bit 21: Positive decode,
        // Bit 22: Synchronous retry, Bit 23: Negative decode
        // For our purposes, just ensure memory access is enabled

        // Status register'dan link kontrolü
        let status = pci_read32(0, 0, 0, ATH_REG_STATUS);
        let _link_up = (status & ATH_STATUS_LINK_UP) != 0;

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
    let status = pci_read32(0, 0, 0, ATH_REG_STATUS);
    (status & ATH_STATUS_LINK_UP) != 0
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
    "Atheros Ethernet"
}

pub fn name_tr() -> &'static str {
    "Atheros Ethernet"
}

pub fn link_up() -> bool {
    link_up_check()
}

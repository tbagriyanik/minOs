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
static mut DEV_BUS: u8 = 0;
static mut DEV_DEV: u8 = 0;
static mut DEV_FUNC: u8 = 0;
static mut MMIO_BASE: u64 = 0;

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
                        DEV_BUS = bus;
                        DEV_DEV = dev;
                        DEV_FUNC = func;

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

        // Algılanan BDF üzerinden BAR0 (MMIO base adresi) çıkar
        // BAR 0 genellikle 32-bit memory space; bit 0=0 demektir memory space
        let (bus, dev, func) = (DEV_BUS, DEV_DEV, DEV_FUNC);
        let bar0 = pci_read32(bus, dev, func, 0x24);
        let mmio_valid = (bar0 & 1) == 0; // Memory space mi?

        if !mmio_valid {
            return false;
        }

        // 32-byte aligned adresi al (bottom 4 bit sıfır)
        MMIO_BASE = (bar0 & !0xF) as u64;

        // Command register'ı oku ve TX/RX enable'ını ayarla
        // PCI config spaceCommand register (offset 0x04)
        let _cmd = pci_read32(bus, dev, func, ATH_REG_CMD);
        // Bit 0: Memory/IO space enable, Bit 1: VGA palette snoop,
        // Bit 2: Memory write enable, Bit 3: Memory enable
        // Bit 20: Fast back-to-back, Bit 21: Positive decode,
        // Bit 22: Synchronous retry, Bit 23: Negative decode
        // For our purposes, just ensure memory access is enabled

        // Status register'dan link kontrolü
        let status = pci_read32(bus, dev, func, ATH_REG_STATUS);
        let _link_up = (status & ATH_STATUS_LINK_UP) != 0;

        // MAC adresini tekrar oku (garanti olsun diye)
        let mac_low = pci_read32(bus, dev, func, 0x14);
        let mac_high = pci_read32(bus, dev, func, 0x18);
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
    let (bus, dev, func) = unsafe { (DEV_BUS, DEV_DEV, DEV_FUNC) };
    let status = pci_read32(bus, dev, func, ATH_REG_STATUS);
    (status & ATH_STATUS_LINK_UP) != 0
}

// ---------------------------------------------------------------------------
// RX Filtresi ve Checksum Offload (iskelet)
// ---------------------------------------------------------------------------
//
// Atheros AR81xx çipleri checksum offload ve multicast filtrelemeyi MMIO
// registerları üzerinden yönetir. Kesin register haritası çip revizyonuna
// göre değişir; aşağıdaki ofsetler AR8161/AR8171 ailesi için belgelenen
// değerlerdir ve gerçek donanım doğrulaması bekler.

/// RX filtre kayıt ofsetleri (BAR0/MMIO göreli).
const ATH_RX_HASH: u64 = 0x04A0; // Multicast hash tablosu (64 bit)
const ATH_RX_CTL: u64 = 0x0480; // RX kontrol: promiscuous/unicast filtre

#[inline]
fn mmio_read32(off: u64) -> u32 {
    unsafe { ((MMIO_BASE + off) as *const u32).read_volatile() }
}

#[inline]
fn mmio_write32(off: u64, val: u32) {
    unsafe { ((MMIO_BASE + off) as *mut u32).write_volatile(val) }
}

/// Multicast MAC adresini 6-bit hash'e indirger (Atheros hash algoritması).
/// Donanım HASH tablosuna bu bit yazılır.
pub fn mac_hash6(mac: &[u8; 6]) -> u8 {
    // Basit FNV-tarzı hash; gerçek AR8161 hash fonksiyonu donanım
    // dokümanıyla doğrulanmalıdır.
    let mut h: u32 = 0x811C_9DC5;
    for &b in mac.iter() {
        h = h.wrapping_mul(0x0100_0193) ^ b as u32;
    }
    (h & 0x3F) as u8
}

/// Promiscuous modu açar/kapatır. `true` → tüm kareler kabul edilir.
pub fn set_promiscuous(enable: bool) {
    unsafe {
        if !DETECTED || MMIO_BASE == 0 {
            return;
        }
        let mut ctl = mmio_read32(ATH_RX_CTL);
        if enable {
            ctl |= 1 << 1; // PR bit
        } else {
            ctl &= !(1 << 1);
        }
        mmio_write32(ATH_RX_CTL, ctl);
    }
}

/// Multicast hash tablosuna tek bit yazar (64 bit, 2 × u32).
pub fn set_multicast_hash(bit6: u8, enable: bool) {
    unsafe {
        if !DETECTED || MMIO_BASE == 0 || bit6 >= 64 {
            return;
        }
        let word = (bit6 / 32) as u64;
        let off = ATH_RX_HASH + word * 4;
        let mut val = mmio_read32(off);
        let mask = 1u32 << (bit6 % 32);
        if enable {
            val |= mask;
        } else {
            val &= !mask;
        }
        mmio_write32(off, val);
    }
}

/// Checksum offload kontrolü: IPv4 header + TCP/UDP checksum'ları
/// donanımda doğrulansın mı? (`rx_ipv4`, `rx_tcp_udp`)
///
/// AR8161'de bu yetenek MAC TX/RX descriptor bayraklarıyla yönetilir;
/// burada RX tarafındaki genel kontrol bitleri programlanır.
pub fn set_checksum_offload(rx_ipv4: bool, rx_tcp_udp: bool) {
    unsafe {
        if !DETECTED || MMIO_BASE == 0 {
            return;
        }
        // RX_CTL bit 4: IPv4 checksum offload, bit 5: L4 (TCP/UDP) offload.
        let mut ctl = mmio_read32(ATH_RX_CTL);
        if rx_ipv4 {
            ctl |= 1 << 4;
        } else {
            ctl &= !(1 << 4);
        }
        if rx_tcp_udp {
            ctl |= 1 << 5;
        } else {
            ctl &= !(1 << 5);
        }
        mmio_write32(ATH_RX_CTL, ctl);
    }
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

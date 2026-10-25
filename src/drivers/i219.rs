#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel I219-V Ethernet Driver - Modern Intel anakart Ethernet denetleyicileri
//!
//! Intel I219-V, modern Intel anakartlarda bulunan Gigabit Ethernet denetleyicisidir.
//! Özellikle yeni nesil CPU'ların (8th gen ve sonrası) PCH'lerinde (Platform Controller Hub)
//! bulunur.

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
// Intel I219 PCI Kimlikleri
// ---------------------------------------------------------------------------

const INTEL_VENDOR: u32 = 0x8086;

// I219-V aygıt kimlikleri (farklı revizyonlar)
const I219_DEVICES: [u32; 12] = [
    0x156F, // I219-V
    0x15B8, // I219-V
    0x15B9, // I219-V
    0x15D7, // I219-V
    0x15D8, // I219-V
    0x15E3, // I219-V
    0x15D6, // I219-V
    0x15BC, // I219-V
    0x15BD, // I219-V
    0x15BE, // I219-V
    0x15DF, // I219-V
    0x15E0, // I219-V
];

// ---------------------------------------------------------------------------
// Intel I219 MMIO Kayıt Ofsetleri
// ---------------------------------------------------------------------------

// Control Register
const REG_CTRL: u32 = 0x0000;
const CTRL_RST: u32 = 1 << 26; // Device reset
const CTRL_SLU: u32 = 1 << 6; // Set Link Up
const CTRL_LRST: u32 = 1 << 3; // Link reset

// Status Register
const REG_STATUS: u32 = 0x0008;
const STATUS_LU: u32 = 1 << 1; // Link Up

// EEPROM/Flash Control
const REG_EERD: u32 = 0x0014;
const EERD_START: u32 = 1 << 0;
const EERD_DONE: u32 = 1 << 4;

// Receive Control
const REG_RCTL: u32 = 0x0100;
const RCTL_EN: u32 = 1 << 1; // Receiver Enable
const RCTL_SBP: u32 = 1 << 2; // Store Bad Packets
const RCTL_UPE: u32 = 1 << 3; // Unicast Promiscuous
const RCTL_MPE: u32 = 1 << 4; // Multicast Promiscuous
const RCTL_BAM: u32 = 1 << 15; // Broadcast Accept
const RCTL_BSIZE_2048: u32 = 0; // Buffer size 2048
const RCTL_SECRC: u32 = 1 << 26; // Strip CRC

// Transmit Control
const REG_TCTL: u32 = 0x0400;
const TCTL_EN: u32 = 1 << 1; // Transmitter Enable
const TCTL_PSP: u32 = 1 << 3; // Pad Short Packets
const TCTL_CT_SHIFT: u32 = 4; // Collision Threshold
const TCTL_COLD_SHIFT: u32 = 12; // Collision Distance

// RX Descriptor Registers
const REG_RDBAL: u32 = 0x2800; // RX Descriptor Base Low
const REG_RDBAH: u32 = 0x2804; // RX Descriptor Base High
const REG_RDLEN: u32 = 0x2808; // RX Descriptor Length
const REG_RDH: u32 = 0x2810; // RX Descriptor Head
const REG_RDT: u32 = 0x2818; // RX Descriptor Tail

// TX Descriptor Registers
const REG_TDBAL: u32 = 0x3800; // TX Descriptor Base Low
const REG_TDBAH: u32 = 0x3804; // TX Descriptor Base High
const REG_TDLEN: u32 = 0x3808; // TX Descriptor Length
const REG_TDH: u32 = 0x3810; // TX Descriptor Head
const REG_TDT: u32 = 0x3818; // TX Descriptor Tail

// Interrupt Registers
const REG_ICR: u32 = 0x00C0; // Interrupt Cause Read
const REG_IMS: u32 = 0x00D0; // Interrupt Mask Set
const REG_IMC: u32 = 0x00D8; // Interrupt Mask Clear

// MAC Address
const REG_RAL0: u32 = 0x5400; // Receive Address Low 0
const REG_RAH0: u32 = 0x5404; // Receive Address High 0

// PHY Status
const REG_PHY_STATUS: u32 = 0xE24; // PHY Status for I219

// ---------------------------------------------------------------------------
// Descriptor Yapıları
// ---------------------------------------------------------------------------

const RX_RING_SIZE: usize = 32;
const TX_RING_SIZE: usize = 32;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct RxDescriptor {
    pub addr: u64,
    pub length: u16,
    pub csum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TxDescriptor {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

// TX descriptor command bits
const TX_CMD_EOP: u8 = 1 << 0; // End of Packet
const TX_CMD_IFCS: u8 = 1 << 1; // Insert FCS
const TX_CMD_RS: u8 = 1 << 3; // Report Status

// RX descriptor status bits
const RX_STAT_DD: u8 = 1 << 0; // Descriptor Done

#[repr(C, align(128))]
struct I219Rings {
    rx_ring: [RxDescriptor; RX_RING_SIZE],
    tx_ring: [TxDescriptor; TX_RING_SIZE],
    rx_bufs: [u8; RX_RING_SIZE * RX_BUF_SIZE],
    tx_bufs: [u8; TX_RING_SIZE * TX_BUF_SIZE],
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut MMIO_BASE: u64 = 0;
static mut DETECTED: bool = false;
static mut MAC_ADDR: [u8; 6] = [0; 6];
static mut RINGS_ADDR: u64 = 0;
static mut RINGS: Option<&'static mut I219Rings> = None;

static mut RX_HEAD: u16 = 0;
static mut RX_TAIL: u16 = 0;
static mut TX_HEAD: u16 = 0;
static mut TX_TAIL: u16 = 0;

// ---------------------------------------------------------------------------
// MMIO Yardımcı Fonksiyonları
// ---------------------------------------------------------------------------

#[inline]
fn mmio_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn mmio_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
}

#[inline]
fn mmio_write64(offset: u32, val: u64) {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *mut u64;
        ptr.write_volatile(val);
    }
}

// ---------------------------------------------------------------------------
// EEPROM Okuma (MAC adresi için)
// ---------------------------------------------------------------------------

fn read_eeprom_word(offset: u16) -> u16 {
    let addr = (offset as u32) << 8;
    mmio_write32(REG_EERD, EERD_START | addr);

    // Done bitini bekle
    for _ in 0..100000 {
        let val = mmio_read32(REG_EERD);
        if val & EERD_DONE != 0 {
            return (val >> 16) as u16;
        }
    }

    0xFFFF
}

fn read_mac_from_eeprom() -> [u8; 6] {
    let word0 = read_eeprom_word(0);
    let word1 = read_eeprom_word(1);
    let word2 = read_eeprom_word(2);

    [
        (word0 & 0xFF) as u8,
        ((word0 >> 8) & 0xFF) as u8,
        (word1 & 0xFF) as u8,
        ((word1 >> 8) & 0xFF) as u8,
        (word2 & 0xFF) as u8,
        ((word2 >> 8) & 0xFF) as u8,
    ]
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
                    let vendor_id = pci_read32(bus, dev, func, 0) & 0xFFFF;
                    let device_id = (pci_read32(bus, dev, func, 0) >> 16) & 0xFFFF;

                    if vendor_id == INTEL_VENDOR && I219_DEVICES.contains(&device_id) {
                        // BAR0 adresini al (MMIO)
                        let bar0 = pci_read32(bus, dev, func, 0x10);
                        if bar0 & 0x1 == 0 {
                            // Memory-mapped I/O
                            MMIO_BASE = (bar0 & 0xFFFF_FFF0) as u64;
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

        // Reset
        mmio_write32(REG_CTRL, CTRL_RST);
        for _ in 0..100000 {
            if mmio_read32(REG_CTRL) & CTRL_RST == 0 {
                break;
            }
        }

        // Bellek kontrolü devre dışı
        mmio_write32(REG_CTRL, 0);

        // MAC adresini oku
        MAC_ADDR = read_mac_from_eeprom();

        // MAC adresini kayda yaz
        let mac_low = ((MAC_ADDR[3] as u32) << 24)
            | ((MAC_ADDR[2] as u32) << 16)
            | ((MAC_ADDR[1] as u32) << 8)
            | (MAC_ADDR[0] as u32);
        let mac_high = ((MAC_ADDR[5] as u32) << 8) | (MAC_ADDR[4] as u32) | (1 << 31); // Address Valid

        mmio_write32(REG_RAL0, mac_low);
        mmio_write32(REG_RAH0, mac_high);

        // RX ring başlat
        if RINGS.is_none() || RINGS_ADDR == 0 {
            return false;
        }

        let rings = RINGS.as_mut().unwrap();

        // RX descriptor'ları hazırla
        for i in 0..RX_RING_SIZE {
            let buf_addr = RINGS_ADDR
                + core::mem::offset_of!(I219Rings, rx_bufs) as u64
                + (i * RX_BUF_SIZE) as u64;
            rings.rx_ring[i].addr = buf_addr;
            rings.rx_ring[i].status = 0;
        }

        // RX ring kayıtlarını ayarla
        let rx_ring_addr = RINGS_ADDR + core::mem::offset_of!(I219Rings, rx_ring) as u64;
        mmio_write32(REG_RDBAL, (rx_ring_addr & 0xFFFFFFFF) as u32);
        mmio_write32(REG_RDBAH, (rx_ring_addr >> 32) as u32);
        mmio_write32(
            REG_RDLEN,
            (RX_RING_SIZE * core::mem::size_of::<RxDescriptor>()) as u32,
        );
        mmio_write32(REG_RDH, 0);
        mmio_write32(REG_RDT, (RX_RING_SIZE - 1) as u32);

        // TX ring kayıtlarını ayarla
        let tx_ring_addr = RINGS_ADDR + core::mem::offset_of!(I219Rings, tx_ring) as u64;
        mmio_write32(REG_TDBAL, (tx_ring_addr & 0xFFFFFFFF) as u32);
        mmio_write32(REG_TDBAH, (tx_ring_addr >> 32) as u32);
        mmio_write32(
            REG_TDLEN,
            (TX_RING_SIZE * core::mem::size_of::<TxDescriptor>()) as u32,
        );
        mmio_write32(REG_TDH, 0);
        mmio_write32(REG_TDT, 0);

        // RX/TX kontrolü
        mmio_write32(
            REG_RCTL,
            RCTL_EN | RCTL_SBP | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_SECRC,
        );
        mmio_write32(
            REG_TCTL,
            TCTL_EN | TCTL_PSP | (0x0F << TCTL_CT_SHIFT) | (0x40 << TCTL_COLD_SHIFT),
        );

        // Link up
        mmio_write32(REG_CTRL, CTRL_SLU);

        // Kesmeleri devre dışı bırak
        mmio_write32(REG_IMC, 0xFFFFFFFF);

        RX_HEAD = 0;
        RX_TAIL = 0;
        TX_HEAD = 0;
        TX_TAIL = 0;

        true
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
    "Intel I219-V"
}

pub fn name_tr() -> &'static str {
    "Intel I219-V Ethernet"
}

/// Paket gönderme
pub fn send_packet(data: &[u8]) -> bool {
    unsafe {
        if !DETECTED || RINGS.is_none() {
            return false;
        }

        let rings = RINGS.as_mut().unwrap();
        let next_tail = (TX_TAIL + 1) % TX_RING_SIZE as u16;

        if next_tail == TX_HEAD {
            return false; // Ring dolu
        }

        let len = data.len().min(TX_BUF_SIZE);
        let buf_offset = TX_TAIL as usize * TX_BUF_SIZE;
        rings.tx_bufs[buf_offset..buf_offset + len].copy_from_slice(data);

        let tx_buf_addr =
            RINGS_ADDR + core::mem::offset_of!(I219Rings, tx_bufs) as u64 + buf_offset as u64;

        rings.tx_ring[TX_TAIL as usize].addr = tx_buf_addr;
        rings.tx_ring[TX_TAIL as usize].length = len as u16;
        rings.tx_ring[TX_TAIL as usize].cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
        rings.tx_ring[TX_TAIL as usize].status = 0;

        TX_TAIL = next_tail;
        mmio_write32(REG_TDT, TX_TAIL as u32);

        true
    }
}

/// Paket alma
pub fn receive_packet() -> Option<&'static [u8]> {
    unsafe {
        if !DETECTED || RINGS.is_none() {
            return None;
        }

        let rings = RINGS.as_mut().unwrap();

        if RX_HEAD == RX_TAIL {
            return None;
        }

        let desc = &rings.rx_ring[RX_HEAD as usize];
        if desc.status & RX_STAT_DD == 0 {
            return None;
        }

        let len = desc.length as usize;
        let buf_offset = RX_HEAD as usize * RX_BUF_SIZE;
        let data = &rings.rx_bufs[buf_offset..buf_offset + len];

        RX_HEAD = (RX_HEAD + 1) % RX_RING_SIZE as u16;
        mmio_write32(REG_RDH, RX_HEAD as u32);

        Some(data)
    }
}

/// Link durumu
pub fn link_up() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        let status = mmio_read32(REG_STATUS);
        status & STATUS_LU != 0
    }
}

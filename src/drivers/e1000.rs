#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel 82540EM (e1000) Ethernet sürücüsü — QEMU `-device e1000,netdev=n0`
//! ile uyumlu, MMIO tabanlı TX/RX ring'leri kullanır.
//!
//! Aynı giriş noktaları (`init`, `poll`, `transmit`, `receive`, `is_ready`,
//! `link_up`) Realtek RTL8139 (PCI, C+ modu) için de hizmet verir; otobüs
//! seçimi `net::set_bus` ile yapılır. NE2000 (ISA/PnP) sürücüsü ayrı modüldedir
//! (`drivers::ne2000`).

use crate::drivers::net::{self, BusKind};
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

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

/// Intel, VMware, Realtek ve Broadcom PCI kimlikleri
const E1000_VENDOR: u32 = 0x8086;
const VMWARE_VENDOR: u32 = 0x15AD;
const REALTEK_VENDOR: u32 = 0x10EC;
const BROADCOM_VENDOR: u32 = 0x14E4;

// Aynı descriptor/MMIO düzenini kullanan klasik 8254x ve 8257x denetleyicileri (Intel e1000 / e1000e).
const E1000_DEVICES: [u32; 10] = [
    0x100E, // 82540EM (QEMU default)
    0x100F, // 82545EM (VMware e1000 default)
    0x1010, // 82546EB
    0x1011, // 82545EM
    0x1012, // 82546EB
    0x1079, // 82546GB
    0x107C, // 82541PI
    0x10D3, // 82574L (VMware / QEMU e1000e)
    0x10F5, // 82567LM
    0x1539, // I211
];

// VMware sanal ağ aygıtları
const VMWARE_DEVICES: [u32; 2] = [
    0x07B0, // VMware VMXNET3 Ethernet Controller
    0x0720, // VMware VMXNET Virtual Ethernet Controller
];

// Realtek PCIe Gigabit & Fast Ethernet Aygıtları (RTL8168, RTL8169, RTL8101, RTL8125 vb.)
const REALTEK_DEVICES: [u32; 11] = [
    0x8139, // RTL8139 Fast Ethernet (klasik PCI NIC, C+ modu)
    0x8168, // RTL8111/8168/8411 PCI Express Gigabit Ethernet (En yaygın anakart PCIe NIC)
    0x8169, // RTL8169 Gigabit Ethernet
    0x8167, // RTL8169SC Gigabit Ethernet
    0x8136, // RTL8101E/RTL8102E PCI Express Fast Ethernet
    0x8125, // RTL8125 2.5GbE PCIe NIC
    0x8161, // RTL8111G/8168G PCIe NIC
    0x8162, // RTL8111EP PCIe NIC
    0x8401, // RTL8401E Fast Ethernet
    0x8402, // RTL8402 Fast Ethernet
    0x8411, // RTL8411 Gigabit Ethernet
];

// Broadcom NetXtreme / NetLink / NetXtreme II — PCI vendor 0x14E4.
// MAC çekirdeği B57: 32-bit Genel Kayıt'lar (ARL) + genişletilmiş kayıtlar.
const BROADCOM_DEVICES: [u32; 14] = [
    0x1646, // NetXtreme BCM5701
    0x1647, // NetXtreme BCM5701S
    0x16A6, // NetXtreme BCM5702
    0x16A7, // NetXtreme BCM5702S
    0x16A8, // NetXtreme BCM5702 / 5703
    0x164A, // NetXtreme BCM5703
    0x1648, // NetXtreme BCM5704
    0x1649, // NetXtreme BCM5704S
    0x1659, // NetXtreme BCM5721
    0x165A, // NetXtreme BCM5722
    0x1668, // NetXtreme BCM5714/5715
    0x1677, // NetXtreme BCM5751
    0x1678, // NetXtreme BCM5714S
    0x169A, // NetXtreme BCM5782
];

// ---------------------------------------------------------------------------
// Broadcom B57 MAC kayıt ofsetleri
// ---------------------------------------------------------------------------
// Her 4 KiB'lik kayıt bloğu kendi adresinin bir bitini taşır; ARL (Address
// Recognition Logic) bloğu tüm diğer bloklara ayna görevi görür. Bu nedenle
// ayni adlı register'lara erişim daima bu blok üzerinden yapılır.
const B57_BASE: u32 = 0xE000; // ARL: ortak kayıtlar
const B57_HOST_CTRL: u32 = B57_BASE | 0x00; // Host Control (FW boot / sıfırlama)
const B57_MAC_HASH0: u32 = B57_BASE | 0x08;
const B57_MAC_HASH1: u32 = B57_BASE | 0x0C;
/// Host Status Word: bit 0-15 TX indeksi, bit 16-31 RX indeksi.
const B57_HOST_STAT: u32 = B57_BASE | 0x0C + 0; // 0x0C ARL bloğunda host status
/// RX descriptor halkası tabanı (alma penceresi, 0xC000 + 0x40).
const B57_RX_LIST_ADDR: u32 = 0xC000 | 0x40;
/// TX descriptor halkası tabanı (gönderme penceresi, 0x1000 + 0x40).
const B57_TX_LIST_ADDR: u32 = 0x1000 | 0x40;
const B57_MAC_ADDR_HI: u32 = B57_BASE | 0x10;
const B57_MAC_ADDR_LO: u32 = B57_BASE | 0x14;
const B57_MISC_HOST_CTRL: u32 = B57_BASE | 0x2C; // bit 16: Register Access Enable
const B57_TX_MAC_ADDR_CTRL: u32 = B57_BASE | 0x40;
const B57_COMMAND: u32 = B57_BASE | 0x3C;
const B57_PHY_STATUS: u32 = B57_BASE | 0xD4;
const B57_MODE: u32 = B57_BASE | 0xF4; // MAC çalışma modu
const B57_MISC_HOST_CTRL_ENABLE: u32 = 1 << 16;
const B57_COMMAND_RX_ENABLE: u32 = 1 << 0;
const B57_COMMAND_TX_ENABLE: u32 = 1 << 1;
const B57_COMMAND_PURGE: u32 = 1 << 2;
const B57_MODE_PROMISC: u32 = 1 << 0;
const B57_MODE_NO_CRC: u32 = 1 << 2;
const B57_PHY_LINK_UP: u32 = 1 << 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NicType {
    IntelE1000,
    VmwareVmxnet3,
    RealtekR8169,
    RealtekRtl8139,
    BroadcomB57,
    /// NE2000 uyumlu PCI kart (RTL8029AS). ISA karşılığı `drivers::ne2000`
    /// modülündedir; sadece PCI adresi/BAR tespiti için burada tutulur.
    Ne2000Pci,
}

static mut DETECTED_NIC: NicType = NicType::IntelE1000;

pub fn nic_type() -> NicType {
    unsafe { DETECTED_NIC }
}

pub fn nic_name() -> &'static str {
    match unsafe { DETECTED_NIC } {
        NicType::IntelE1000 => "Intel e1000",
        NicType::VmwareVmxnet3 => "VMware VMXNET3",
        NicType::RealtekR8169 => "Realtek PCIe GbE (RTL8168/8169)",
        NicType::RealtekRtl8139 => "Realtek RTL8139",
        NicType::BroadcomB57 => "Broadcom NetXtreme (BCM57xx/577xx)",
        NicType::Ne2000Pci => "NE2000 compatible (RTL8029AS)",
    }
}

/// Arayüzlerde gösterilecek yerelleştirilmiş (TR/EN) açıklayıcı sürücü adı.
pub fn nic_label(english: bool) -> &'static str {
    match unsafe { DETECTED_NIC } {
        NicType::IntelE1000 => "Intel e1000",
        NicType::VmwareVmxnet3 => "VMware VMXNET3",
        NicType::RealtekR8169 => "Realtek PCIe GbE",
        NicType::RealtekRtl8139 => {
            if english {
                "Realtek RTL8139 (PCI Fast Ethernet)"
            } else {
                "Realtek RTL8139 (PCI Hızlı Ethernet)"
            }
        }
        NicType::BroadcomB57 => "Broadcom NetXtreme",
        NicType::Ne2000Pci => {
            if english {
                "NE2000 compatible (RTL8029AS)"
            } else {
                "NE2000 uyumlu (RTL8029AS)"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Realtek RTL8168 / RTL8169 PCIe MMIO Kayıt Ofsetleri ve Sabitleri
// ---------------------------------------------------------------------------
const RTL_REG_IDR0: u32 = 0x0000; // MAC adresi bayt 0..3
const RTL_REG_IDR4: u32 = 0x0004; // MAC adresi bayt 4..5
const RTL_REG_MAR0: u32 = 0x0008; // Multicast filter 0..3
const RTL_REG_MAR4: u32 = 0x000C; // Multicast filter 4..7
const RTL_REG_TXADDR: u32 = 0x0020; // TX Descriptor ring tabanı (64-bit: 0x20..0x27)
const RTL_REG_COMMAND: u32 = 0x0037; // Command Register (RST, RE, TE)
const RTL_REG_TXPOLL: u32 = 0x0038; // Transmit Priority Polling (NPQ = bit 6)
const RTL_REG_IMR: u32 = 0x003C; // Interrupt Mask Register (16-bit)
const RTL_REG_ISR: u32 = 0x003E; // Interrupt Status Register (16-bit)
const RTL_REG_TCR: u32 = 0x0040; // Transmit Configuration Register
const RTL_REG_RCR: u32 = 0x0044; // Receive Configuration Register
const RTL_REG_9346CR: u32 = 0x0050; // 93C46 Command Register (Config write enable)
const RTL_REG_CONFIG1: u32 = 0x0052; // Config1 Register
const RTL_REG_CONFIG3: u32 = 0x0054; // Config3 Register (ASF / WOL)
const RTL_REG_CONFIG4: u32 = 0x0056; // Config4 Register (MAC yedek)
const RTL_REG_PHYSTATUS: u32 = 0x006C; // PHY Status Register (Link status = bit 1)
const RTL_REG_RMS: u32 = 0x00DA; // Rx Max Packet Size (16-bit)
const RTL_REG_RXADDR: u32 = 0x00E4; // RX Descriptor ring tabanı (64-bit: 0xE4..0xEB)

const RTL_CMD_RESET: u8 = 0x10;
const RTL_CMD_RX_EN: u8 = 0x08;
const RTL_CMD_TX_EN: u8 = 0x04;

// ---------------------------------------------------------------------------
// Realtek RTL8139 (PCI, C+ modu) I/O kayıtları
// ---------------------------------------------------------------------------
// RTL8139 PCI BAR0 bir I/O BAR'dır; kayıtlara I/O portundan erişilir.
const RTL8139_IDR0: u16 = 0x00; // MAC adresi bayt 0..5
const RTL8139_TSD0: u16 = 0x10; // TX status (4 kanal: 0x10, 0x14, 0x18, 0x1C)
const RTL8139_TSAD0: u16 = 0x20; // TX start address (4 kanal: 0x20, 0x24, 0x28, 0x2C)
const RTL8139_RBSTART: u16 = 0x30; // RX buffer fiziksel adresi
const RTL8139_CR: u16 = 0x37; // Command
const RTL8139_CAPR: u16 = 0x38; // Current Address of Packet Read
const RTL8139_CBR: u16 = 0x3A; // Current Buffer Address
const RTL8139_IMR: u16 = 0x3C;
const RTL8139_ISR: u16 = 0x3E;
const RTL8139_TCR: u16 = 0x40;
const RTL8139_RCR: u16 = 0x44;
const RTL8139_CONFIG1: u16 = 0x52;
const RTL8139_CONFIG3: u16 = 0x54; // EEPROM adres bitleri (Config3)
const RTL8139_CONFIG4: u16 = 0x55; // EEPROM veri bitleri (Config4)
const RTL8139_9346CR: u16 = 0x50;
const RTL8139_MSR: u16 = 0x58;
const RTL8139_PHY_MII: u16 = 0x32; // MII serial interface (BMCR/BMEDIA)
const RTL8139_PHY_DATA: u16 = 0x34;
const RTL8139_CONFIG1_LWAKE: u8 = 1 << 0; // LWAKE: Config1 yazma kilidini açar
const RTL8139_CONFIG4_EEM0: u8 = 1 << 7; // EEM0 = 1: 93C46 yok, sürücü programlar

const RTL8139_RX_RING_BITS: u32 = 1 << 11; // RCR bit 11: 8K + 16B RX halkası
const RTL8139_RX_WRAP: u16 = 0x2000; // Halka girdisi WRAP işareti

const RTL8139_CR_BUFE: u8 = 0x01; // RX tamponu boş
const RTL8139_CR_TE: u8 = 0x04; // TX enable
const RTL8139_CR_RE: u8 = 0x08; // RX enable
const RTL8139_CR_RST: u8 = 0x10; // Sıfırlama (kendini temizler)

const RTL8139_TSD_OWN: u32 = 1 << 13; // Kanalın gönderimi tamamladı
const RTL8139_TSD_UNDERRUN: u32 = 1 << 14;

// RX halkası 8K + 16 bayt "wrap around" payı, 2K paket tavanı (tek segment).
const RTL8139_RX_RING_SIZE: usize = 8192;
const RTL8139_RX_BUF_TOTAL: usize = RTL8139_RX_RING_SIZE + 16;
const RTL8139_MAX_PACKET: usize = 2048;
const RTL8139_TX_CHANNELS: usize = 4;

// RTL8139 başlatılamadığında (ör. DMA belleği yok) diğer otobüslere devam
// edebilmek için uyarı iletisi bir kez yazılır.
static RTL8139_WARNED: AtomicBool = AtomicBool::new(false);

// Realtek Descriptor Bitleri
const RTL_DESC_OWN: u32 = 1 << 31;
const RTL_DESC_EOR: u32 = 1 << 30; // Ring Sonu (End of Ring)
const RTL_DESC_FS: u32 = 1 << 29; // First Segment
const RTL_DESC_LS: u32 = 1 << 28; // Last Segment

// ---------------------------------------------------------------------------
// MMIO kayıt ofsetleri (Intel e1000)
// ---------------------------------------------------------------------------

const REG_CTRL: u32 = 0x0000;
const REG_STATUS: u32 = 0x0008;
const REG_EERD: u32 = 0x0014;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_ICR: u32 = 0x00C0; // Interrupt Cause Read (okuma temizler)
const REG_IMC: u32 = 0x00D8; // Interrupt Mask Clear (tüm kesmeleri kapat)
const REG_RAL0: u32 = 0x5400; // Receive Address Low 0
const REG_RAH0: u32 = 0x5404; // Receive Address High 0

// ---------------------------------------------------------------------------
// VMware VMXNET3 MMIO kayıt ve komutları (BAR0)
// ---------------------------------------------------------------------------
const VMXNET3_REG_VRRS: u32 = 0x0000;
const VMXNET3_REG_UVRS: u32 = 0x0008;
const VMXNET3_REG_DSAL: u32 = 0x0010;
const VMXNET3_REG_DSAH: u32 = 0x0014;
const VMXNET3_REG_CMD: u32 = 0x0018;
const VMXNET3_REG_MACL: u32 = 0x0020;
const VMXNET3_REG_MACH: u32 = 0x0024;

const VMXNET3_CMD_ENABLE: u32 = 0x0000;
const VMXNET3_CMD_DISABLE: u32 = 0x0001;
const VMXNET3_CMD_RESET: u32 = 0x0002;
const VMXNET3_CMD_GET_LINK: u32 = 0x0005;
const VMXNET3_CMD_GET_MAC: u32 = 0x0006;

// Kontrol bitleri
const RCTL_EN: u32 = 1 << 1;
const RCTL_SBP: u32 = 1 << 2;
const RCTL_UPE: u32 = 1 << 3;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_LPE: u32 = 1 << 5;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_BSIZE_2048: u32 = 0; // 0 << 16
const RCTL_SECRC: u32 = 1 << 26; // Strip Ethernet CRC

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT_SHIFT: u32 = 4;
const TCTL_COLD_SHIFT: u32 = 12;

const CTRL_SLU: u32 = 1 << 6; // Set Link Up
const CTRL_LRST: u32 = 1 << 3; // Link Reset

// Ring boyutları
const RX_RING_SIZE: usize = 32;
const TX_RING_SIZE: usize = 32;
const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct RxDescriptor {
    pub addr: u64, // buffer fiziksel adresi
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

#[repr(C, align(128))]
struct E1000Rings {
    rx_ring: [RxDescriptor; RX_RING_SIZE],
    tx_ring: [TxDescriptor; TX_RING_SIZE],
    rx_bufs: [u8; RX_RING_SIZE * RX_BUF_SIZE],
    tx_bufs: [u8; TX_RING_SIZE * TX_BUF_SIZE],
}

/// Ring belleği PMM'den ayrılır; tahsis yoksa sürücü başlatılmaz.
static mut RINGS_ADDR: u64 = 0;
static mut MMIO_BASE: u64 = 0;

/// TX tamponlarının fiziksel başlangıcı (RX tamponlarını çakmasın diye
/// E1000Rings::tx_bufs ofsetinden hesaplanır).
pub fn tx_buffer_phys() -> u64 {
    unsafe { RINGS_ADDR + core::mem::offset_of!(E1000Rings, tx_bufs) as u64 }
}

/// Ring yapısına HHDM üzerinden erişim.
#[inline(always)]
unsafe fn rings() -> *mut E1000Rings {
    crate::drivers::phys_to_virt(RINGS_ADDR) as *mut E1000Rings
}
static E1000_READY: AtomicBool = AtomicBool::new(false);

// Ring index sayaçları
static mut RX_TAIL: usize = 0;
static mut TX_TAIL: usize = 0;

// MAC adresi karttan okunur
pub static mut MAC_ADDR: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

#[inline(always)]
unsafe fn mmio_read8(off: u32) -> u8 {
    core::ptr::read_volatile((MMIO_BASE + off as u64) as *const u8)
}

#[inline(always)]
unsafe fn mmio_write8(off: u32, val: u8) {
    core::ptr::write_volatile((MMIO_BASE + off as u64) as *mut u8, val);
}

#[inline(always)]
unsafe fn mmio_read16(off: u32) -> u16 {
    core::ptr::read_volatile((MMIO_BASE + off as u64) as *const u16)
}

#[inline(always)]
unsafe fn mmio_write16(off: u32, val: u16) {
    core::ptr::write_volatile((MMIO_BASE + off as u64) as *mut u16, val);
}

#[inline(always)]
unsafe fn mmio_read32(off: u32) -> u32 {
    core::ptr::read_volatile((MMIO_BASE + off as u64) as *const u32)
}

#[inline(always)]
unsafe fn mmio_write32(off: u32, val: u32) {
    core::ptr::write_volatile((MMIO_BASE + off as u64) as *mut u32, val);
}

pub fn is_ready() -> bool {
    E1000_READY.load(Ordering::Relaxed)
}

/// Reports physical Ethernet link state.
pub fn link_up() -> bool {
    if !is_ready() {
        return false;
    }
    // ISA/PnP kartlar için sorgu NE2000 sürücüsüne yönlendirilir.
    if net::active_bus() == BusKind::IsaPnp {
        return crate::drivers::ne2000::link_up();
    }
    unsafe {
        if DETECTED_NIC == NicType::RealtekRtl8139 {
            return rtl8139_link_up();
        }
        match DETECTED_NIC {
            NicType::IntelE1000 => mmio_read32(REG_STATUS) & (1 << 1) != 0,
            NicType::VmwareVmxnet3 => {
                mmio_write32(VMXNET3_REG_CMD, VMXNET3_CMD_GET_LINK);
                let v = mmio_read32(VMXNET3_REG_CMD);
                (v & 1) != 0
            }
            NicType::RealtekR8169 => {
                // PHY status bit 1: 1 = Link Up, 0 = Link Down
                (mmio_read8(RTL_REG_PHYSTATUS) & 0x02) != 0
            }
            NicType::BroadcomB57 => (mmio_read32(B57_PHY_STATUS) & B57_PHY_LINK_UP) != 0,
            NicType::RealtekRtl8139 => rtl8139_link_up(),
            NicType::Ne2000Pci => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Realtek RTL8139 (C+ modu) sürücüsü
// ---------------------------------------------------------------------------
// Kayıtlara I/O portundan erişilir. TX için 4 kanal doğrudan
// `tx_bufs` içine yazılan tamponlara yönlendirilir; RX tek bir fiziksel
// halkadır (`rtl8139_rx()`) ve paketler halkadan okunup `rx_bufs`
// içinde yeniden oluşturulur (wrap-around böylece tek segment olur).

static mut RTL8139_IOBASE: u16 = 0;
static mut RTL8139_READ_PTR: usize = 0;
static mut RTL8139_BUF_ADDR: usize = 0;

#[inline(always)]
unsafe fn rtl8139_out8(off: u16, val: u8) {
    crate::drivers::port_io::io_out8(RTL8139_IOBASE + off, val)
}

#[inline(always)]
unsafe fn rtl8139_in8(off: u16) -> u8 {
    crate::drivers::port_io::io_in8(RTL8139_IOBASE + off)
}

#[inline(always)]
unsafe fn rtl8139_out16(off: u16, val: u16) {
    crate::drivers::port_io::io_out16(RTL8139_IOBASE + off, val)
}

#[inline(always)]
unsafe fn rtl8139_in16(off: u16) -> u16 {
    crate::drivers::port_io::io_in16(RTL8139_IOBASE + off)
}

#[inline(always)]
unsafe fn rtl8139_out32(off: u16, val: u32) {
    crate::drivers::port_io::io_out32(RTL8139_IOBASE + off, val)
}

#[inline(always)]
unsafe fn rtl8139_in32(off: u16) -> u32 {
    crate::drivers::port_io::io_in32(RTL8139_IOBASE + off)
}

/// Halkadan küçük-endian 16-bit alan okur (CPU x86-64 olduğundan doğal u16
/// okuması doğrudan kullanılabilir).
#[inline(always)]
unsafe fn read_le16(base: *const u8, off: usize) -> u16 {
    core::ptr::read_volatile(base.add(off) as *const u16)
}

unsafe fn rtl8139_rx_ring_phys() -> u64 {
    RINGS_ADDR + core::mem::offset_of!(E1000Rings, rx_bufs) as u64
}

unsafe fn rtl8139_tx_buf_phys() -> u64 {
    RINGS_ADDR + core::mem::offset_of!(E1000Rings, tx_bufs) as u64
}

/// RTL8139 yazılım sıfırlaması; 0x10 biti temizlenince tamamlanır.
pub(crate) unsafe fn rtl8139_reset() {
    rtl8139_out8(RTL8139_CR, RTL8139_CR_RST);
    let mut spins = 0u32;
    while (rtl8139_in8(RTL8139_CR) & RTL8139_CR_RST) != 0 {
        spins += 1;
        if spins > 1_000_000 {
            break;
        }
        core::hint::spin_loop();
    }
}

/// C+ modu: Config1 LWAKE + Config4 EEM0; aygıt 0xFF baytına (kendi MAC'i)
/// yönlendirilir, böylece MAC config alanından doğru okunur.
unsafe fn rtl8139_enter_c_plus_mode() {
    let mut config1 = rtl8139_in8(RTL8139_CONFIG1);
    let config4 = rtl8139_in8(RTL8139_CONFIG4);
    rtl8139_out8(RTL8139_CONFIG1, config1 | RTL8139_CONFIG1_LWAKE);
    rtl8139_out8(RTL8139_CONFIG4, config4 | RTL8139_CONFIG4_EEM0);
    config1 = rtl8139_in8(RTL8139_CONFIG1);
    rtl8139_out8(RTL8139_CONFIG1, config1 & !RTL8139_CONFIG1_LWAKE);
}

unsafe fn rtl8139_read_mac() -> [u8; 6] {
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = rtl8139_in8(RTL8139_IDR0 + i as u16);
    }
    mac
}

/// RTL8139 EEPROM arayüzü: 9346CR (0x50) komut, Config3 (0x54) adres
/// bitleri, Config4 (0x55) veri bitleri.
unsafe fn rtl8139_read_mac_eeprom() -> Option<[u8; 6]> {
    let mut mac = [0u8; 6];
    for (word_idx, pair) in mac.chunks_mut(2).enumerate() {
        // 93C46 word adresi (0..2) 6 bitlik adres alanına yazılır.
        rtl8139_out8(RTL8139_CONFIG3, (word_idx as u8) << 3);
        // EE_ENB (0xC0) | READ (0x02)
        rtl8139_out8(RTL8139_9346CR, 0xC0 | 0x02);
        let mut spins = 0u32;
        loop {
            if rtl8139_in8(RTL8139_CONFIG4) & 0x01 != 0 {
                break;
            }
            spins += 1;
            if spins > 1_000_000 {
                return None;
            }
            core::hint::spin_loop();
        }
        let val = rtl8139_in16(0x50);
        pair[0] = (val & 0xFF) as u8;
        if pair.len() > 1 {
            pair[1] = (val >> 8) as u8;
        }
    }
    rtl8139_out8(RTL8139_9346CR, 0x00);
    Some(mac)
}

/// 2.5MHz MII serial bus üzerinden PHY register'ı (BMCR/BMSR) okur.
unsafe fn rtl8139_phy_read(reg: u8) -> u16 {
    // Yön biti (MDO) + PHY adresi 0 + register + MII_WRITE (bit 3).
    rtl8139_out8(RTL8139_PHY_MII, (reg << 2) | 0x01 | 0x08);
    let mut spins = 0u32;
    while rtl8139_in8(RTL8139_PHY_MII) & 0x80 == 0 {
        spins += 1;
        if spins > 100_000 {
            return 0;
        }
        core::hint::spin_loop();
    }
    rtl8139_in16(RTL8139_PHY_DATA)
}

unsafe fn rtl8139_link_up() -> bool {
    // MII taraması: register 0x01 (BMSR) bit 2 = bağlantı kuruldu.
    let bmsr = rtl8139_phy_read(0x01);
    if bmsr != 0 {
        return bmsr & 0x0004 != 0;
    }
    // MEDIA_STATUS (MSR) yedek: jumper / autonegotiate sonucu.
    let media = rtl8139_in8(RTL8139_MSR);
    (media & 0x0C) != 0 || (media & 0x03) == 0x03
}

pub(crate) unsafe fn rtl8139_transmit(packet: &[u8]) -> bool {
    let tail = TX_TAIL % RTL8139_TX_CHANNELS;
    let buf_phys = rtl8139_tx_buf_phys() + (tail as u64) * TX_BUF_SIZE as u64;
    let buf_ptr = crate::drivers::phys_to_virt(buf_phys) as *mut u8;
    core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_ptr, packet.len());
    core::sync::atomic::fence(Ordering::SeqCst);
    TX_TAIL = (tail + 1) % RTL8139_TX_CHANNELS;

    rtl8139_out32(RTL8139_TSAD0 + (tail as u16) * 4, buf_phys as u32);
    // Boyut (bit 0-12), OWN (bit 13), IFG ayarlanmadan önce temizlenir.
    rtl8139_out32(
        RTL8139_TSD0 + (tail as u16) * 4,
        packet.len() as u32 & 0x1FFF,
    );

    let mut spins = 0u32;
    loop {
        let status = rtl8139_in32(RTL8139_TSD0 + (tail as u16) * 4);
        if status & (RTL8139_TSD_OWN | RTL8139_TSD_UNDERRUN) != 0 {
            return status & RTL8139_TSD_UNDERRUN == 0;
        }
        spins += 1;
        if spins > 500_000 {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// RX halkasından ilk paketi okur ve `buf` içine kopyalar.
/// Halka girdisi: [durum u16][boyut u16][MAC 6][...].
pub(crate) unsafe fn rtl8139_receive(buf: &mut [u8]) -> usize {
    let ring_ptr = crate::drivers::phys_to_virt(rtl8139_rx_ring_phys()) as *const u8;

    let offset = RTL8139_READ_PTR;
    if offset >= RTL8139_RX_RING_SIZE {
        return 0;
    }
    let status = read_le16(ring_ptr, offset);
    let len = read_le16(ring_ptr, offset + 2);

    // Halka boş: CBR == CAPR; yine de bekleyen iş varsa (WRAP) okunmalıdır.
    if status & RTL8139_RX_WRAP == 0 && rtl8139_in16(RTL8139_CBR) == rtl8139_in16(RTL8139_CAPR) {
        return 0;
    }

    // ROK (bit 0) yoksa paket bozulmuştur; yine de tüketilir.
    let ok = status & 0x0001 != 0;
    let len = len as usize;
    let aligned = (len + 4 + 3) & !3;

    // Sonraki okuma noktası, 8K halka + 16 baytlık WRAP payı içinde döner.
    RTL8139_BUF_ADDR = RTL8139_BUF_ADDR.wrapping_add(aligned) & !3;
    if RTL8139_BUF_ADDR >= RTL8139_RX_RING_SIZE {
        RTL8139_BUF_ADDR -= RTL8139_RX_RING_SIZE;
    }
    RTL8139_READ_PTR = (offset + aligned) & (RTL8139_RX_BUF_TOTAL - 1);
    // CAPR bir büyük yazılır (WRAP biti için 16 bayt boşluk bırakılır).
    rtl8139_out16(RTL8139_CAPR, (offset + 16) as u16);

    if !ok || len <= 4 || len - 4 > RTL8139_MAX_PACKET || len - 4 > buf.len() {
        return 0;
    }

    let copied = len - 4;
    let src = offset + 4;
    for (i, b) in buf.iter_mut().take(copied).enumerate() {
        let mut idx = src + i;
        if idx >= RTL8139_RX_BUF_TOTAL {
            idx -= RTL8139_RX_BUF_TOTAL;
        }
        *b = core::ptr::read_volatile(ring_ptr.add(idx));
    }
    copied
}

/// RTL8139 başlatma. DMA ringi (E1000Rings) mevcut değilse `false` döner,
/// böylece çağrı sahibi diğer otobüslere geçebilir.
pub(crate) unsafe fn init_rtl8139(bus: u8, dev: u8, func: u8, io_base: u16) -> bool {
    let phys = RINGS_ADDR;
    if phys == 0 {
        // `init` DMA halkasını ayrımadan yalın RTL8139 başlatması çağrıldı.
        if !RTL8139_WARNED.swap(true, Ordering::Relaxed) {
            e1000_print("[NET] RTL8139 için DMA halkası ayrılamadı.\n");
        }
        return false;
    }
    RTL8139_IOBASE = io_base;

    // PCI Komut: I/O Space (bit0) + Bus Master (bit2) + Memory Write (bit4)
    let cmd = pci_read32(bus, dev, func, 0x04);
    pci_write32(bus, dev, func, 0x04, (cmd & 0xFFFF_0000) | 0x15);

    rtl8139_reset();
    rtl8139_enter_c_plus_mode();

    let mut mac = rtl8139_read_mac();
    if mac.iter().all(|&b| b == 0 || b == 0xFF) {
        if let Some(ee) = rtl8139_read_mac_eeprom() {
            if ee.iter().any(|&b| b != 0 && b != 0xFF) {
                mac = ee;
            }
        }
    }
    MAC_ADDR = mac;
    e1000_print("[NET] RTL8139 MAC: ");
    for (i, b) in mac.iter().enumerate() {
        if i > 0 {
            e1000_print(":");
        }
        e1000_print_hex(*b);
    }
    e1000_print("\n");

    // PHY'nin autonegotiation tamamlamasına fırsat ver.
    for _ in 0..10_000 {
        core::hint::spin_loop();
    }

    // RX halkası: 8 KiB + 16 bayt WRAP payı; kayıtlar temizlenir.
    let rx_phys = rtl8139_rx_ring_phys();
    let rx_ptr = crate::drivers::phys_to_virt(rx_phys) as *mut u8;
    core::ptr::write_bytes(rx_ptr, 0, RTL8139_RX_BUF_TOTAL);
    rtl8139_out32(RTL8139_RBSTART, rx_phys as u32);

    // Kesmeleri kapat; bekleyen durumları temizle (polling).
    rtl8139_out16(RTL8139_IMR, 0x0000);
    rtl8139_out16(RTL8139_ISR, 0xFFFF);

    // RCR: 8K RX halkası (bit 11), AB/AM/AP (bit 12-14), 2K tampon (0<<16),
    // yalnız ROK paketleri kabul et (bit 20).
    rtl8139_out32(RTL8139_RCR, RTL8139_RX_RING_BITS | (0x7 << 12) | (1 << 20));
    // TCR: standart interframe gap
    rtl8139_out32(RTL8139_TCR, 0x00_00_03_00);

    // TX kanallarını temizle
    for ch in 0..RTL8139_TX_CHANNELS {
        let off = (ch as u16) * 4;
        rtl8139_out32(RTL8139_TSD0 + off, 0);
        rtl8139_out32(RTL8139_TSAD0 + off, 0);
    }
    core::ptr::write_bytes(
        crate::drivers::phys_to_virt(rtl8139_tx_buf_phys()) as *mut u8,
        0,
        TX_RING_SIZE * TX_BUF_SIZE,
    );
    TX_TAIL = 0;
    RTL8139_READ_PTR = 0;
    RTL8139_BUF_ADDR = 0;

    // RX + TX etkinleştir. CAPR başlangıçta 0xFFF0'dır: CAPR + 16 == CBR
    // olduğundan "halka boş" kuralı sağlanır ve ilk paket okunur.
    rtl8139_out8(RTL8139_CR, RTL8139_CR_BUFE);
    rtl8139_out8(RTL8139_CR, RTL8139_CR_RE | RTL8139_CR_TE);
    rtl8139_out16(RTL8139_CAPR, 0xFFF0);

    e1000_print("[NET] Realtek RTL8139 bulundu. PCI ");
    e1000_print_hex(bus);
    e1000_print(":");
    e1000_print_hex(dev);
    e1000_print(" IO:0x");
    e1000_print_hex((io_base >> 8) as u8);
    e1000_print_hex((io_base & 0xFF) as u8);
    e1000_print("\n");
    true
}

// ---------------------------------------------------------------------------
// EEPROM / Register'dan MAC okuma
// ---------------------------------------------------------------------------

unsafe fn read_mac_realtek() -> [u8; 6] {
    let mut mac = [0u8; 6];
    let idr0 = mmio_read32(RTL_REG_IDR0);
    let idr4 = mmio_read32(RTL_REG_IDR4);
    mac[0] = (idr0 & 0xFF) as u8;
    mac[1] = ((idr0 >> 8) & 0xFF) as u8;
    mac[2] = ((idr0 >> 16) & 0xFF) as u8;
    mac[3] = ((idr0 >> 24) & 0xFF) as u8;
    mac[4] = (idr4 & 0xFF) as u8;
    mac[5] = ((idr4 >> 8) & 0xFF) as u8;
    mac
}

unsafe fn read_mac_vmxnet3() -> [u8; 6] {
    let mut mac = [0u8; 6];
    mmio_write32(VMXNET3_REG_CMD, VMXNET3_CMD_GET_MAC);
    let mac_l = mmio_read32(VMXNET3_REG_MACL);
    let mac_h = mmio_read32(VMXNET3_REG_MACH);
    mac[0] = (mac_l & 0xFF) as u8;
    mac[1] = ((mac_l >> 8) & 0xFF) as u8;
    mac[2] = ((mac_l >> 16) & 0xFF) as u8;
    mac[3] = ((mac_l >> 24) & 0xFF) as u8;
    mac[4] = (mac_h & 0xFF) as u8;
    mac[5] = ((mac_h >> 8) & 0xFF) as u8;
    mac
}

unsafe fn read_mac_from_eeprom() -> [u8; 6] {
    let mut mac = [0u8; 6];
    // EEPROM word 0,1,2 MAC'i içerir
    for i in 0..3u32 {
        // EERD register: bit 0 start, 8..15 adres, 16-31 data
        mmio_write32(REG_EERD, (1 << 0) | (i << 8));
        let mut spins = 0;
        loop {
            let v = mmio_read32(REG_EERD);
            if (v & 0x10) != 0 {
                // done bit
                let word = (v >> 16) & 0xFFFF;
                mac[(i as usize) * 2] = (word & 0xFF) as u8;
                mac[(i as usize) * 2 + 1] = ((word >> 8) & 0xFF) as u8;
                break;
            }
            spins += 1;
            if spins > 100_000 {
                break;
            }
            core::hint::spin_loop();
        }
    }
    mac
}

// ---------------------------------------------------------------------------
// Broadcom B57 yardimcilari
// ---------------------------------------------------------------------------

/// Broadcom MAC adresi: yüksek kayıt bayt 0-1, düşük kayıt bayt 2-5 (büyük-endian sıra).
unsafe fn read_mac_broadcom() -> [u8; 6] {
    let hi = mmio_read32(B57_MAC_ADDR_HI);
    let lo = mmio_read32(B57_MAC_ADDR_LO);
    [
        (hi >> 8) as u8,
        (hi & 0xFF) as u8,
        (lo >> 24) as u8,
        (lo >> 16) as u8,
        (lo >> 8) as u8,
        lo as u8,
    ]
}

/// Broadcom TX: Host Status Word (0x0C) sıradaki descriptor indeksini taşır.
unsafe fn broadcom_transmit(packet: &[u8]) -> bool {
    let r = rings();
    let tail = TX_TAIL;
    let buf_phys = RINGS_ADDR
        + core::mem::offset_of!(E1000Rings, tx_bufs) as u64
        + (tail as u64) * TX_BUF_SIZE as u64;
    let buf_ptr = crate::drivers::phys_to_virt(buf_phys) as *mut u8;
    core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_ptr, packet.len());

    let desc_u32 = &mut (*r).tx_ring[tail] as *mut TxDescriptor as *mut u32;
    core::ptr::write_volatile(desc_u32.add(0), buf_phys as u32);
    core::ptr::write_volatile(desc_u32.add(1), (buf_phys >> 32) as u32);
    // Opak alan: B57 bu 16-bit alanı "flags/length" olarak kullanır (bit 0-15 uzunluk).
    core::ptr::write_volatile(desc_u32.add(3), packet.len() as u32 & 0xFFFF);
    core::ptr::write_volatile(desc_u32.add(2), 0);
    core::sync::atomic::fence(Ordering::SeqCst);

    TX_TAIL = (tail + 1) % TX_RING_SIZE;
    // Gönderim kapısı: TX kayıt penceresindeki host status word'e indeks yaz.
    mmio_write32(B57_HOST_STAT, TX_TAIL as u32);

    let mut spins = 0;
    loop {
        let idx = mmio_read32(B57_HOST_STAT) & 0xFFFF;
        if idx != TX_TAIL as u32 {
            return true;
        }
        spins += 1;
        if spins > 200_000 {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// Broadcom RX: host status word alma tarafindaki descriptor indeksini verir.
unsafe fn broadcom_receive(buf: &mut [u8]) -> usize {
    let r = rings();
    let mut received = 0usize;
    loop {
        let idx = (mmio_read32(B57_HOST_STAT) >> 16) as usize;
        if idx == RX_TAIL {
            break;
        }
        let idx = idx % RX_RING_SIZE;
        let desc_u32 = &mut (*r).rx_ring[idx] as *mut RxDescriptor as *mut u32;
        let len = (core::ptr::read_volatile(desc_u32.add(3)) & 0xFFFF) as usize;
        let addr_lo = core::ptr::read_volatile(desc_u32.add(0)) as u64;
        let addr_hi = core::ptr::read_volatile(desc_u32.add(1)) as u64;
        let addr = addr_lo | (addr_hi << 32);
        if received == 0 && len > 0 && len <= buf.len() {
            let src = crate::drivers::phys_to_virt(addr) as *const u8;
            core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), len);
            received = len;
        }
        // Tamponu yeniden karta ver ve statüyü temizle.
        core::ptr::write_volatile(desc_u32.add(3), RX_BUF_SIZE as u32 & 0xFFFF);
        mmio_write32(B57_HOST_STAT, ((idx as u32) << 16) | (TX_TAIL as u32));
    }
    received
}

unsafe fn init_broadcom(rings_addr: u64) {
    // Register Access Enable: MISC_HOST_CTRL bit 16-17.
    let mhc = mmio_read32(B57_MISC_HOST_CTRL);
    mmio_write32(B57_MISC_HOST_CTRL, mhc | B57_MISC_HOST_CTRL_ENABLE);

    // Sıfırlama: TX/RX durdur ve purge et.
    mmio_write32(B57_COMMAND, 0);
    mmio_write32(B57_COMMAND, B57_COMMAND_PURGE);

    // MAC modu: promiscuous + (donanim CRC)
    let mode = mmio_read32(B57_MODE);
    mmio_write32(B57_MODE, mode | B57_MODE_PROMISC);
    // Donanimsal filtre: tüm hash bitleri açık (promiscuous destek).
    mmio_write32(B57_MAC_HASH0, 0xFFFF_FFFF);
    mmio_write32(B57_MAC_HASH1, 0xFFFF_FFFF);
    mmio_write32(B57_TX_MAC_ADDR_CTRL, 0xFFFF_FFFF);

    // RX/TX descriptor halkasi taban adresleri
    mmio_write32(
        B57_RX_LIST_ADDR,
        (rings_addr + core::mem::offset_of!(E1000Rings, rx_ring) as u64) as u32,
    );
    mmio_write32(
        B57_TX_LIST_ADDR,
        (rings_addr + core::mem::offset_of!(E1000Rings, tx_ring) as u64) as u32,
    );

    // RX descriptorlari hazirla
    let r = rings();
    for i in 0..RX_RING_SIZE {
        let buf_phys = rings_addr
            + core::mem::offset_of!(E1000Rings, rx_bufs) as u64
            + (i as u64) * RX_BUF_SIZE as u64;
        let desc_u32 = &mut (*r).rx_ring[i] as *mut RxDescriptor as *mut u32;
        core::ptr::write_volatile(desc_u32.add(0), buf_phys as u32);
        core::ptr::write_volatile(desc_u32.add(1), (buf_phys >> 32) as u32);
        core::ptr::write_volatile(desc_u32.add(3), RX_BUF_SIZE as u32 & 0xFFFF);
        core::ptr::write_volatile(desc_u32.add(2), 0);
    }

    // Host status word: RX indeksi = RX_TAIL, TX indeksi = TX_TAIL
    mmio_write32(B57_HOST_STAT, ((RX_RING_SIZE as u32 - 1) << 16) | 0);

    // TX/RX etkinleştir
    mmio_write32(B57_COMMAND, B57_COMMAND_RX_ENABLE | B57_COMMAND_TX_ENABLE);
}

unsafe fn init_rx_ring() {
    let r = rings();
    let rings_addr = RINGS_ADDR;
    for i in 0..RX_RING_SIZE {
        let buf_phys = rings_addr
            + core::mem::offset_of!(E1000Rings, rx_bufs) as u64
            + (i as u64) * RX_BUF_SIZE as u64;
        (*r).rx_ring[i].addr = buf_phys;
        (*r).rx_ring[i].status = 0;
    }
    mmio_write32(
        REG_RDBAL,
        (rings_addr + core::mem::offset_of!(E1000Rings, rx_ring) as u64) as u32,
    );
    mmio_write32(
        REG_RDBAH,
        ((rings_addr + core::mem::offset_of!(E1000Rings, rx_ring) as u64) >> 32) as u32,
    );
    mmio_write32(REG_RDLEN, (RX_RING_SIZE * 16) as u32);
    mmio_write32(REG_RDH, 0);
    mmio_write32(REG_RDT, RX_RING_SIZE as u32 - 1);
    // RCTL: enable, broadcast accept, unicast promiscuous, strip CRC, buffer 2048
    mmio_write32(
        REG_RCTL,
        RCTL_EN
            | RCTL_SBP
            | RCTL_UPE
            | RCTL_MPE
            | RCTL_BAM
            | RCTL_BSIZE_2048
            | RCTL_LPE
            | RCTL_SECRC,
    );
}

// ---------------------------------------------------------------------------
// TX ring hazırlama
// ---------------------------------------------------------------------------

unsafe fn init_tx_ring() {
    let rings_addr = RINGS_ADDR;
    mmio_write32(
        REG_TDBAL,
        (rings_addr + core::mem::offset_of!(E1000Rings, tx_ring) as u64) as u32,
    );
    mmio_write32(
        REG_TDBAH,
        ((rings_addr + core::mem::offset_of!(E1000Rings, tx_ring) as u64) >> 32) as u32,
    );
    mmio_write32(REG_TDLEN, (TX_RING_SIZE * 16) as u32);
    mmio_write32(REG_TDH, 0);
    mmio_write32(REG_TDT, 0);
    // TCTL: enable, pad short packets, collision threshold 15, collision distance 64
    mmio_write32(
        REG_TCTL,
        TCTL_EN | TCTL_PSP | (0x0F << TCTL_CT_SHIFT) | (0x40 << TCTL_COLD_SHIFT),
    );
}

// ---------------------------------------------------------------------------
// Paket TX / RX
// ---------------------------------------------------------------------------

/// Ham bir Ethernet çerçevesini etkin ağ kartı üzerinden iletir.
pub fn transmit(packet: &[u8]) -> bool {
    if !is_ready() || packet.len() > TX_BUF_SIZE {
        return false;
    }
    if net::active_bus() == BusKind::IsaPnp {
        return crate::drivers::ne2000::transmit(packet);
    }
    unsafe {
        if DETECTED_NIC == NicType::RealtekRtl8139 {
            return rtl8139_transmit(packet);
        }
        if DETECTED_NIC == NicType::BroadcomB57 {
            return broadcom_transmit(packet);
        }
        let r = rings();
        let tail = TX_TAIL;
        let desc = &mut (*r).tx_ring[tail];
        let buf_phys = RINGS_ADDR
            + core::mem::offset_of!(E1000Rings, tx_bufs) as u64
            + (tail as u64) * TX_BUF_SIZE as u64;

        let buf_ptr = crate::drivers::phys_to_virt(buf_phys) as *mut u8;
        core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_ptr, packet.len());

        if DETECTED_NIC == NicType::RealtekR8169 {
            let eor = if tail == TX_RING_SIZE - 1 {
                RTL_DESC_EOR
            } else {
                0
            };
            let desc_u32 = desc as *mut TxDescriptor as *mut u32;
            let opts1 =
                RTL_DESC_OWN | RTL_DESC_FS | RTL_DESC_LS | eor | (packet.len() as u32 & 0x3FFF);
            core::ptr::write_volatile(desc_u32.add(0), buf_phys as u32);
            core::ptr::write_volatile(desc_u32.add(1), (buf_phys >> 32) as u32);
            core::ptr::write_volatile(desc_u32.add(3), 0);
            core::ptr::write_volatile(desc_u32.add(2), opts1);

            TX_TAIL = (tail + 1) % TX_RING_SIZE;
            mmio_write8(RTL_REG_TXPOLL, 0x40);

            let mut spins = 0;
            loop {
                let status_opts1 = core::ptr::read_volatile(desc_u32.add(2));
                if (status_opts1 & RTL_DESC_OWN) == 0 {
                    return true;
                }
                spins += 1;
                if spins > 200_000 {
                    return false;
                }
                core::hint::spin_loop();
            }
        } else {
            desc.addr = buf_phys;
            desc.length = packet.len() as u16;
            // EOP (bit 0 = 0x01), IFCS (bit 1 = 0x02), RS (bit 3 = 0x08)
            desc.cmd = 0x01 | 0x02 | 0x08;
            desc.status = 0;
            desc.cso = 0;
            desc.css = 0;
            desc.special = 0;

            TX_TAIL = (tail + 1) % TX_RING_SIZE;
            mmio_write32(REG_TDT, TX_TAIL as u32);

            // DD (Descriptor Done - bit 0) kontrolü
            let mut spins = 0;
            let desc_ptr = &(*rings()).tx_ring[tail] as *const TxDescriptor;
            loop {
                let status = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).status));
                if status & 0x01 != 0 {
                    return true;
                }
                spins += 1;
                if spins > 200_000 {
                    return false;
                }
                core::hint::spin_loop();
            }
        }
    }
}

pub fn receive(buf: &mut [u8]) -> usize {
    if !is_ready() {
        return 0;
    }
    if net::active_bus() == BusKind::IsaPnp {
        return crate::drivers::ne2000::receive(buf);
    }
    unsafe {
        if DETECTED_NIC == NicType::RealtekRtl8139 {
            return rtl8139_receive(buf);
        }
        if DETECTED_NIC == NicType::BroadcomB57 {
            return broadcom_receive(buf);
        }
        let idx = RX_TAIL;
        let desc_ptr = &mut (*rings()).rx_ring[idx] as *mut RxDescriptor;

        if DETECTED_NIC == NicType::RealtekR8169 {
            let desc_u32 = desc_ptr as *mut u32;
            let opts1 = core::ptr::read_volatile(desc_u32.add(2));
            if (opts1 & RTL_DESC_OWN) != 0 {
                return 0;
            }
            let len = (opts1 & 0x3FFF) as usize;
            let eor = if idx == RX_RING_SIZE - 1 {
                RTL_DESC_EOR
            } else {
                0
            };

            if len == 0
                || len > buf.len()
                || (opts1 & (RTL_DESC_FS | RTL_DESC_LS)) != (RTL_DESC_FS | RTL_DESC_LS)
            {
                core::ptr::write_volatile(
                    desc_u32.add(2),
                    RTL_DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                );
                RX_TAIL = (idx + 1) % RX_RING_SIZE;
                return 0;
            }

            let addr_lo = core::ptr::read_volatile(desc_u32.add(0)) as u64;
            let addr_hi = core::ptr::read_volatile(desc_u32.add(1)) as u64;
            let addr = addr_lo | (addr_hi << 32);
            let src = crate::drivers::phys_to_virt(addr) as *const u8;
            core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), len);

            core::ptr::write_volatile(
                desc_u32.add(2),
                RTL_DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
            );
            RX_TAIL = (idx + 1) % RX_RING_SIZE;
            len
        } else {
            let status = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).status));
            if status & 0x01 == 0 {
                return 0; // DD biti ayarlanmadı
            }
            let len = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).length)) as usize;
            let errors = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).errors));
            let addr = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).addr));

            if len == 0 || len > buf.len() || errors != 0 || status & 0x02 == 0 {
                core::ptr::write_volatile(core::ptr::addr_of_mut!((*desc_ptr).status), 0);
                mmio_write32(REG_RDT, idx as u32);
                RX_TAIL = (idx + 1) % RX_RING_SIZE;
                return 0;
            }
            let src = crate::drivers::phys_to_virt(addr) as *const u8;
            core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), len);

            core::ptr::write_volatile(core::ptr::addr_of_mut!((*desc_ptr).status), 0);
            mmio_write32(REG_RDT, idx as u32);
            RX_TAIL = (idx + 1) % RX_RING_SIZE;
            len
        }
    }
}
pub fn poll() {
    let mut buf = [0u8; RX_BUF_SIZE];
    loop {
        let n = receive(&mut buf);
        if n == 0 {
            break;
        }
        crate::drivers::netstack::on_packet(&buf[..n]);
        // NE2000: döngü, tampon bitene kadar `receive` içinde sınırlanır.
        if net::active_bus() == BusKind::IsaPnp && !crate::drivers::ne2000::has_pending() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// e1000 Init
// ---------------------------------------------------------------------------

pub fn init(pmm_dma_phys: Option<u64>) {
    let Some(phys) = pmm_dma_phys else {
        e1000_print("[NET] DMA ring havuzu ayrılamadı; PCI ağ sürücüsü başlatılmadı.\n");
        return;
    };

    // PCI'da Intel e1000 / VMware VMXNET3 / Realtek (RTL8139, RTL816x) /
    // Broadcom NIC ara.
    let mut found: Option<(u8, u8, u8, u64, NicType)> = None;
    unsafe {
        for bus in 0..8u8 {
            for dev in 0..32u8 {
                let id0 = pci_read32(bus, dev, 0, 0x00);
                if id0 == 0xFFFF_FFFF || id0 == 0 {
                    continue;
                }
                let hdr = (pci_read32(bus, dev, 0, 0x0C) >> 16) & 0xFF;
                let max_func = if (hdr & 0x80) != 0 { 8u8 } else { 1u8 };

                for func in 0..max_func {
                    let id = if func == 0 {
                        id0
                    } else {
                        pci_read32(bus, dev, func, 0x00)
                    };
                    if id == 0xFFFF_FFFF || id == 0 {
                        continue;
                    }
                    let vid = (id & 0xFFFF) as u32;
                    let did = (id >> 16) & 0xFFFF;

                    if did == 0x8029 {
                        // NE2000 uyumlu PCI kartlar (RTL8029AS). Klasik ISA
                        // NE2000'ler PCI'da g\u{f6}r\u{fc}nmez; onlar PnP taramas\u{131}yla bulunur.
                        found = Some((bus, dev, func, 0, NicType::Ne2000Pci));
                        break;
                    }

                    if vid == REALTEK_VENDOR && did == 0x8139 {
                        // RTL8139 klasik PCI kart\u{131}: BAR0 bir I/O BAR'd\u{131}r.
                        let bar0 = pci_read32(bus, dev, func, 0x10);
                        if bar0 != 0 && bar0 & 1 == 1 {
                            found = Some((
                                bus,
                                dev,
                                func,
                                (bar0 & 0xFFFF_FFFC) as u64,
                                NicType::RealtekRtl8139,
                            ));
                            break;
                        }
                        continue;
                    }

                    if vid == E1000_VENDOR && E1000_DEVICES.contains(&did) {
                        let bar0_lo = pci_read32(bus, dev, func, 0x10);
                        let base: u64 = bar0_lo as u64 & 0xFFFF_FFF0;
                        found = Some((bus, dev, func, base, NicType::IntelE1000));
                        break;
                    } else if vid == VMWARE_VENDOR && VMWARE_DEVICES.contains(&did) {
                        let bar0_lo = pci_read32(bus, dev, func, 0x10);
                        let base: u64 = bar0_lo as u64 & 0xFFFF_FFF0;
                        found = Some((bus, dev, func, base, NicType::VmwareVmxnet3));
                        break;
                    } else if vid == REALTEK_VENDOR
                        && (REALTEK_DEVICES.contains(&did) || (did & 0xFF00) == 0x8100)
                    {
                        // Realtek PCIe NIC: BAR2 (0x18) veya BAR1 (0x14) MMIO olabilir, standart BAR2'dir (BAR0 genelde I/O porttur).
                        let mut base: u64 = 0;
                        for bar_off in [0x18u8, 0x14u8, 0x10u8] {
                            let bar = pci_read32(bus, dev, func, bar_off);
                            if bar != 0 && (bar & 1) == 0 {
                                // MMIO bar
                                let is_64 = (bar & 0x06) == 0x04;
                                let bar_hi = if is_64 {
                                    pci_read32(bus, dev, func, bar_off + 4) as u64
                                } else {
                                    0
                                };
                                base = ((bar as u64) & 0xFFFF_FFF0) | (bar_hi << 32);
                                break;
                            }
                        }
                        if base != 0 {
                            found = Some((bus, dev, func, base, NicType::RealtekR8169));
                            break;
                        }
                    } else if vid == BROADCOM_VENDOR && BROADCOM_DEVICES.contains(&did) {
                        // Broadcom NetXtreme: aygıtlar farklı sırada MMIO BAR taşıyabilir
                        // (genelde BAR0 64-bit), bu yüzden ilk MMIO BAR seçilir.
                        let mut base: u64 = 0;
                        for bar_off in [0x10u8, 0x14u8, 0x18u8, 0x1C, 0x20, 0x24] {
                            let bar = pci_read32(bus, dev, func, bar_off);
                            if bar != 0 && (bar & 1) == 0 {
                                let is_64 = (bar & 0x06) == 0x04;
                                let bar_hi = if is_64 {
                                    pci_read32(bus, dev, func, bar_off + 4) as u64
                                } else {
                                    0
                                };
                                base = ((bar as u64) & 0xFFFF_FFF0) | (bar_hi << 32);
                                break;
                            }
                        }
                        if base != 0 {
                            found = Some((bus, dev, func, base, NicType::BroadcomB57));
                            break;
                        }
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }

        let (bus, dev, func, base_phys, ntype) = match found {
            Some(v) => v,
            None => {
                e1000_print(
                    "[NET] Ag adaptoru (Intel / VMware / Realtek / Broadcom) PCI'da bulunamadi.\n",
                );
                return;
            }
        };
        DETECTED_NIC = ntype;

        // NE2000 uyumlu PCI kart (RTL8029AS): ISA/PnP sürücüsüne devredilir.
        if ntype == NicType::Ne2000Pci {
            e1000_print("[NET] NE2000 uyumlu PCI ag adaptoru bulundu. PCI ");
            e1000_print_hex(bus);
            e1000_print(":");
            e1000_print_hex(dev);
            e1000_print("\n");
            // BAR0 bir I/O BAR'dır.
            let bar0 = pci_read32(bus, dev, func, 0x10);
            let io_base = if bar0 & 1 == 1 {
                (bar0 & 0xFFFC) as u16
            } else {
                0
            };
            let _ = crate::drivers::ne2000::init_pci(io_base);
            return;
        }

        RINGS_ADDR = phys;
        let base = crate::drivers::phys_to_virt(base_phys as u64);
        crate::arch::x86_64::mmio_map_2mb(base_phys as u64 & !0x1F_FFFF);

        match ntype {
            NicType::IntelE1000 => {
                e1000_print("[NET] Intel e1000 bulundu. PCI ");
                e1000_print_hex(bus);
                e1000_print(":");
                e1000_print_hex(dev);
                e1000_print("\n");
            }
            NicType::RealtekRtl8139 => {}
            NicType::VmwareVmxnet3 => {
                e1000_print("[NET] VMware VMXNET3 ag adaptoru bulundu. PCI ");
                e1000_print_hex(bus);
                e1000_print(":");
                e1000_print_hex(dev);
                e1000_print("\n");
            }
            NicType::RealtekR8169 => {
                e1000_print(
                    "[NET] Realtek PCIe Gigabit ag adaptoru bulundu (Vendor: 0x10EC). PCI ",
                );
                e1000_print_hex(bus);
                e1000_print(":");
                e1000_print_hex(dev);
                e1000_print("\n");
            }
            NicType::BroadcomB57 => {
                e1000_print("[NET] Broadcom NetXtreme ag adaptoru bulundu (Vendor: 0x14E4). PCI ");
                e1000_print_hex(bus);
                e1000_print(":");
                e1000_print_hex(dev);
                e1000_print("\n");
            }
            NicType::Ne2000Pci => {}
        }

        // PCI Komut: Bus Master + MMIO enable + Memory Write and Invalidate.
        // RTL8139 için I/O Space (bit 0) de açık kalmalıdır.
        let cmd = pci_read32(bus, dev, func, 0x04);
        let cmd_bits = if ntype == NicType::RealtekRtl8139 {
            0x17
        } else {
            0x16
        };
        pci_write32(bus, dev, func, 0x04, (cmd & 0xFFFF_FFF0) | cmd_bits);

        MMIO_BASE = base;

        // Reset ve temel başlatma
        match ntype {
            NicType::IntelE1000 => {
                mmio_write32(REG_CTRL, CTRL_LRST);
                for _ in 0..100_000 {
                    core::hint::spin_loop();
                }
                mmio_write32(REG_CTRL, CTRL_SLU);
            }
            NicType::VmwareVmxnet3 => {
                mmio_write32(VMXNET3_REG_CMD, VMXNET3_CMD_RESET);
                for _ in 0..50_000 {
                    core::hint::spin_loop();
                }
                mmio_write32(VMXNET3_REG_CMD, VMXNET3_CMD_ENABLE);
            }
            NicType::RealtekR8169 => {
                // Realtek C+ modunda yazılım sıfırlaması
                mmio_write8(RTL_REG_COMMAND, RTL_CMD_RESET);
                for _ in 0..100_000 {
                    if (mmio_read8(RTL_REG_COMMAND) & RTL_CMD_RESET) == 0 {
                        break;
                    }
                    core::hint::spin_loop();
                }
                // EEPROM/Config kilitlerini aç (93C46 unlock: 0xC0)
                mmio_write8(RTL_REG_9346CR, 0xC0);
            }
            NicType::RealtekRtl8139 => {}
            NicType::BroadcomB57 => {
                // B57: register access + MAC modu/ring hazırlığı `init_broadcom`
                // içinde yapılır (ring belleği sıfırlandıktan sonra).
            }
            NicType::Ne2000Pci => {}
        }

        // Ring yapısını HHDM üzerinden sıfırla
        let r = rings();
        core::ptr::write_bytes(r as *mut u8, 0, core::mem::size_of::<E1000Rings>());

        // MAC oku
        let mac = match ntype {
            NicType::IntelE1000 => read_mac_from_eeprom(),
            NicType::VmwareVmxnet3 => read_mac_vmxnet3(),
            NicType::RealtekR8169 => read_mac_realtek(),
            NicType::BroadcomB57 => read_mac_broadcom(),
            NicType::RealtekRtl8139 | NicType::Ne2000Pci => [0u8; 6],
        };
        MAC_ADDR = mac;
        // RTL8139 MAC'ini `init_rtl8139` (C+ modu sonrası) kendisi yazar.
        if ntype != NicType::RealtekRtl8139 {
            e1000_print("[NET] MAC adresi: ");
            e1000_print_hex(mac[0]);
            e1000_print(":");
            e1000_print_hex(mac[1]);
            e1000_print(":");
            e1000_print_hex(mac[2]);
            e1000_print(":");
            e1000_print_hex(mac[3]);
            e1000_print(":");
            e1000_print_hex(mac[4]);
            e1000_print(":");
            e1000_print_hex(mac[5]);
            e1000_print("\n");
        }

        if ntype == NicType::IntelE1000 {
            let ral = (mac[0] as u32)
                | ((mac[1] as u32) << 8)
                | ((mac[2] as u32) << 16)
                | ((mac[3] as u32) << 24);
            let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | 0x8000_0000;
            mmio_write32(REG_RAL0, ral);
            mmio_write32(REG_RAH0, rah);

            for i in 0..128u32 {
                mmio_write32(0x5200 + i * 4, 0);
            }

            mmio_write32(REG_IMC, 0xFFFF_FFFF);
            mmio_write32(REG_CTRL, CTRL_SLU);
            let _ = mmio_read32(REG_ICR);
        } else if ntype == NicType::RealtekR8169 {
            // Kesmeleri kapat (polling kullanıyoruz)
            mmio_write16(RTL_REG_IMR, 0x0000);
            mmio_write16(RTL_REG_ISR, 0xFFFF); // W1C temizle

            // Maksimum RX paket boyutu
            mmio_write16(RTL_REG_RMS, (RX_BUF_SIZE as u16).min(16383));

            // RX ve TX konfigürasyonu
            // RCR: Accept Broadcast, Multicast, My Physical, Promiscuous, Unlimited DMA burst
            mmio_write32(RTL_REG_RCR, 0x0000_000F | (7 << 8));
            // TCR: Interframe gap (3 << 24), Unlimited DMA burst (7 << 8)
            mmio_write32(RTL_REG_TCR, (3 << 24) | (7 << 8));

            // Ring taban adreslerini bildir
            let rx_ring_phys = phys + core::mem::offset_of!(E1000Rings, rx_ring) as u64;
            let tx_ring_phys = phys + core::mem::offset_of!(E1000Rings, tx_ring) as u64;
            mmio_write32(RTL_REG_RXADDR, rx_ring_phys as u32);
            mmio_write32(RTL_REG_RXADDR + 4, (rx_ring_phys >> 32) as u32);
            mmio_write32(RTL_REG_TXADDR, tx_ring_phys as u32);
            mmio_write32(RTL_REG_TXADDR + 4, (tx_ring_phys >> 32) as u32);

            // Realtek RX ring descriptorlarını hazırla
            for i in 0..RX_RING_SIZE {
                let buf_phys = phys
                    + core::mem::offset_of!(E1000Rings, rx_bufs) as u64
                    + (i as u64) * RX_BUF_SIZE as u64;
                let desc_u32 = &mut (*r).rx_ring[i] as *mut RxDescriptor as *mut u32;
                let eor = if i == RX_RING_SIZE - 1 {
                    RTL_DESC_EOR
                } else {
                    0
                };
                core::ptr::write_volatile(desc_u32.add(0), buf_phys as u32);
                core::ptr::write_volatile(desc_u32.add(1), (buf_phys >> 32) as u32);
                core::ptr::write_volatile(desc_u32.add(3), 0);
                core::ptr::write_volatile(
                    desc_u32.add(2),
                    RTL_DESC_OWN | eor | (RX_BUF_SIZE as u32 & 0x3FFF),
                );
            }

            // Realtek TX ring descriptorlarını hazırla
            for i in 0..TX_RING_SIZE {
                let desc_u32 = &mut (*r).tx_ring[i] as *mut TxDescriptor as *mut u32;
                let eor = if i == TX_RING_SIZE - 1 {
                    RTL_DESC_EOR
                } else {
                    0
                };
                core::ptr::write_volatile(desc_u32.add(0), 0);
                core::ptr::write_volatile(desc_u32.add(1), 0);
                core::ptr::write_volatile(desc_u32.add(3), 0);
                core::ptr::write_volatile(desc_u32.add(2), eor);
            }

            // Config kilitlerini kapat (93C46 lock: 0x00)
            mmio_write8(RTL_REG_9346CR, 0x00);

            // RX ve TX'i aktifleştir
            mmio_write8(RTL_REG_COMMAND, RTL_CMD_RX_EN | RTL_CMD_TX_EN);
        } else if ntype == NicType::BroadcomB57 {
            init_broadcom(phys);
        }

        if ntype != NicType::RealtekR8169 && ntype != NicType::BroadcomB57 {
            // Ring'leri hazırla (Intel / VMware)
            init_rx_ring();
            init_tx_ring();
        }

        match ntype {
            NicType::IntelE1000 => {
                e1000_print("[NET] e1000 RX/TX ring'leri hazir, link acildi.\n");
            }
            NicType::VmwareVmxnet3 => {
                e1000_print("[NET] VMware VMXNET3 RX/TX ring'leri hazir, link acildi.\n");
            }
            NicType::RealtekR8169 => {
                e1000_print("[NET] Realtek PCIe GbE RX/TX ring'leri hazir, link acildi.\n");
            }
            NicType::BroadcomB57 => {
                e1000_print("[NET] Broadcom NetXtreme RX/TX ring'leri hazir, link acildi.\n");
            }
            NicType::RealtekRtl8139 => {
                // DMA halkası hazır; `init_rtl8139` sıfırlama, C+ modu, MAC
                // okuma, RX halkası ve RX/TX etkinleştirmesini üstlenir.
                if init_rtl8139(bus, dev, func, base_phys as u16) {
                    e1000_print("[NET] Realtek RTL8139 RX/TX hazir, link acildi.\n");
                } else {
                    return;
                }
            }
            NicType::Ne2000Pci => return,
        }
        // PCI kartın seçildiğini (e1000/VMXNET3/RTL816x/RTL8139/BCM) bildir.
        net::set_bus(BusKind::Pci);

        E1000_READY.store(true, Ordering::Relaxed);
    }
}
fn e1000_print(s: &str) {
    crate::serial::print(s);
}

fn e1000_print_hex(v: u8) {
    let hex = b"0123456789ABCDEF";
    let hi = (v >> 4) & 0xF;
    let lo = v & 0xF;
    e1000_print_byte(hex[hi as usize]);
    e1000_print_byte(hex[lo as usize]);
}

fn e1000_print_byte(b: u8) {
    crate::serial::write_byte(b);
}

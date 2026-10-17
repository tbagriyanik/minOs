#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel 82540EM (e1000) Ethernet sürücüsü — QEMU `-device e1000,netdev=n0`
//! ile uyumlu, MMIO tabanlı TX/RX ring'leri kullanır.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// I/O port yardımcıları (PCI için)
// ---------------------------------------------------------------------------

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

/// Intel 82540EM / e1000 PCI kimliği
const E1000_VENDOR: u32 = 0x8086;
// Aynı descriptor/MMIO düzenini kullanan klasik 8254x denetleyicileri.
const E1000_DEVICES: [u32; 5] = [0x100E, 0x100F, 0x1010, 0x1011, 0x1012];

// ---------------------------------------------------------------------------
// MMIO kayıt ofsetleri
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
    pub addr: u64,   // buffer fiziksel adresi
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

/// Reports physical Ethernet link state (STATUS.LU).
pub fn link_up() -> bool {
    is_ready() && unsafe { mmio_read32(REG_STATUS) & (1 << 1) != 0 }
}

// ---------------------------------------------------------------------------
// EEPROM'dan MAC okuma
// ---------------------------------------------------------------------------

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
// RX ring hazırlama
// ---------------------------------------------------------------------------

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
    mmio_write32(REG_RDBAL, (rings_addr + core::mem::offset_of!(E1000Rings, rx_ring) as u64) as u32);
    mmio_write32(REG_RDBAH, ((rings_addr + core::mem::offset_of!(E1000Rings, rx_ring) as u64) >> 32) as u32);
    mmio_write32(REG_RDLEN, (RX_RING_SIZE * 16) as u32);
    mmio_write32(REG_RDH, 0);
    mmio_write32(REG_RDT, RX_RING_SIZE as u32 - 1);
    // RCTL: enable, broadcast accept, unicast promiscuous, strip CRC, buffer 2048
    mmio_write32(REG_RCTL, RCTL_EN | RCTL_SBP | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_BSIZE_2048 | RCTL_LPE | RCTL_SECRC);
}

// ---------------------------------------------------------------------------
// TX ring hazırlama
// ---------------------------------------------------------------------------

unsafe fn init_tx_ring() {
    let rings_addr = RINGS_ADDR;
    mmio_write32(REG_TDBAL, (rings_addr + core::mem::offset_of!(E1000Rings, tx_ring) as u64) as u32);
    mmio_write32(REG_TDBAH, ((rings_addr + core::mem::offset_of!(E1000Rings, tx_ring) as u64) >> 32) as u32);
    mmio_write32(REG_TDLEN, (TX_RING_SIZE * 16) as u32);
    mmio_write32(REG_TDH, 0);
    mmio_write32(REG_TDT, 0);
    // TCTL: enable, pad short packets, collision threshold 15, collision distance 64
    mmio_write32(REG_TCTL, TCTL_EN | TCTL_PSP | (0x0F << TCTL_CT_SHIFT) | (0x40 << TCTL_COLD_SHIFT));
}

// ---------------------------------------------------------------------------
// Paket TX / RX
// ---------------------------------------------------------------------------

/// Ham bir Ethernet çerçevesini e1000 üzerinden iletir.
pub fn transmit(packet: &[u8]) -> bool {
    if !is_ready() || packet.len() > TX_BUF_SIZE {
        return false;
    }
    unsafe {
        let r = rings();
        let tail = TX_TAIL;
        let desc = &mut (*r).tx_ring[tail];
        let buf_phys = RINGS_ADDR
            + core::mem::offset_of!(E1000Rings, tx_bufs) as u64
            + (tail as u64) * TX_BUF_SIZE as u64;
        desc.addr = buf_phys;

        // buf'a kopyala (HHDM üzerinden)
        let buf_ptr = crate::drivers::phys_to_virt(buf_phys) as *mut u8;
        core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_ptr, packet.len());
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
                // Descriptor tamamlanmadı; çağırana başarılı iletim bildirme.
                return false;
            }
            core::hint::spin_loop();
        }
    }
}

/// Gelen bir paketi okur (varsa), `buf` içine kopyalar, uzunluk döner.
pub fn receive(buf: &mut [u8]) -> usize {
    if !is_ready() {
        return 0;
    }
    unsafe {
        let idx = RX_TAIL;
        let desc_ptr = &mut (*rings()).rx_ring[idx] as *mut RxDescriptor;
        let status = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).status));
        if status & 0x01 == 0 {
            return 0; // DD biti ayarlanmadı
        }
        let len = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).length)) as usize;
        let errors = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).errors));
        let addr = core::ptr::read_volatile(core::ptr::addr_of!((*desc_ptr).addr));

        if len == 0 || len > buf.len() || errors != 0 || status & 0x02 == 0 {
            // Bu buffer'ı yeniden kullan
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*desc_ptr).status), 0);
            mmio_write32(REG_RDT, idx as u32);
            RX_TAIL = (idx + 1) % RX_RING_SIZE;
            return 0;
        }
        let src = crate::drivers::phys_to_virt(addr) as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), len);

        // Descriptoru temizle ve RDT'yi ilerlet
        core::ptr::write_volatile(core::ptr::addr_of_mut!((*desc_ptr).status), 0);
        mmio_write32(REG_RDT, idx as u32);
        RX_TAIL = (idx + 1) % RX_RING_SIZE;
        len
    }
}

/// Gelen tüm paketleri işler (poll döngüsünden çağrılır).
pub fn poll() {
    let mut buf = [0u8; RX_BUF_SIZE];
    loop {
        let n = receive(&mut buf);
        if n == 0 {
            break;
        }
        crate::drivers::netstack::on_packet(&buf[..n]);
    }
}

// ---------------------------------------------------------------------------
// e1000 Init
// ---------------------------------------------------------------------------

pub fn init(pmm_dma_phys: Option<u64>) {
    unsafe {
        let phys = match pmm_dma_phys {
            Some(phys) => phys,
            None => {
                e1000_print("[NET] DMA ring havuzu ayrılamadı; e1000 başlatılmadı.\n");
                return;
            }
        };
        RINGS_ADDR = phys;

        // PCI'da e1000'ı bul
        let mut found: Option<(u8, u8, u8, u64)> = None;
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let vid = pci_read32(0, dev, func, 0x00) & 0xFFFF;
                let did = (pci_read32(0, dev, func, 0x00) >> 16) & 0xFFFF;
                if vid == E1000_VENDOR && E1000_DEVICES.contains(&did) {
                    // BAR0'ı oku (32-bit MMIO BAR)
                    let bar0_lo = pci_read32(0, dev, func, 0x10);
                    let base: u64 = bar0_lo as u64 & 0xFFFF_FFF0;
                    found = Some((0, dev, func, base));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }

        let (bus, dev, func, base_phys) = match found {
            Some(v) => v,
            None => {
                e1000_print("[NET] e1000 PCI'da bulunamadi.\n");
                return;
            }
        };
        let base = crate::drivers::phys_to_virt(base_phys);
        // Limine HHDM MMIO'yu map'lemediği için elle 2MB huge page ekliyoruz.
        crate::arch::x86_64::mmio_map_2mb(base_phys & !0x1F_FFFF);

        e1000_print("[NET] Intel e1000 bulundu. PCI 00:");
        e1000_print_hex(dev);
        e1000_print("\n");

        // PCI Komut: Bus Master + MMIO enable
        let cmd = pci_read32(bus, dev, func, 0x04);
        pci_write32(bus, dev, func, 0x04, (cmd & 0xFFFF_FFF0) | 0x6);

        MMIO_BASE = base;
        // Global reset ve link-up (MAC ve register yapılandırmasından ÖNCE yapılmalı!)
        mmio_write32(REG_CTRL, CTRL_LRST);
        for _ in 0..100_000 {
            core::hint::spin_loop();
        }
        mmio_write32(REG_CTRL, CTRL_SLU);

        // Ring yapısını HHDM üzerinden sıfırla
        let r = rings();
        core::ptr::write_bytes(r as *mut u8, 0, core::mem::size_of::<E1000Rings>());

        // EEPROM'dan MAC oku
        let mac = read_mac_from_eeprom();
        MAC_ADDR = mac;
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

        // RAL0/RAH0'a MAC yaz (Address Valid biti 0x8000_0000 ile)
        let ral = (mac[0] as u32) | ((mac[1] as u32) << 8) | ((mac[2] as u32) << 16) | ((mac[3] as u32) << 24);
        let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | 0x8000_0000;
        mmio_write32(REG_RAL0, ral);
        mmio_write32(REG_RAH0, rah);

        // Multicast Table Array (MTA: 0x5200..0x5400 - 128 dword) sıfırla
        for i in 0..128u32 {
            mmio_write32(0x5200 + i * 4, 0);
        }

        // Tüm kesme kaynaklarını kapat: yoklamayla (polling) çalışıyoruz
        mmio_write32(REG_IMC, 0xFFFF_FFFF);
        mmio_write32(REG_CTRL, CTRL_SLU);
        let _ = mmio_read32(REG_ICR); // bekleyen kesme nedenlerini temizle

        // Ring'leri hazırla
        init_rx_ring();
        init_tx_ring();

        e1000_print("[NET] e1000 RX/TX ring'leri hazir, link acildi.\n");
        E1000_READY.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Yardımcı: Serial yazdırma
// ---------------------------------------------------------------------------

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

#![allow(dead_code, static_mut_refs, unused_unsafe)]

//! USB HID (klavye + fare) sürücüsü — xHCI (USB 3.x) denetleyicileri üzerinden.
//!
//! Bu modül, PCI üzerinde bulunan xHCI denetleyicilerini başlatır, bağlı USB
//! aygıtlarını numaralandırır ve HID raporlarını okur. Raporlar mevcut PS/2
//! klavye/fare durumuna dönüştürülür; böylece masaüstü, uygulamalar ve
//! kısayollar USB donanımıyla da çalışır.
//!
//! Kapsam:
//! - xHCI (USB 3.0/3.1/3.2) denetleyicileri desteklenir. USB 1.1/2.0 aygıtları
//!   xHCI'nin USB 2.0 portları üzerinden çalışır.
//! - HID Boot Protocol (klavye protokol 1, fare protokol 2) ve boot protokolü
//!   sunmayan aygıtlar için rapor tanımlayıcısı (Report Descriptor) çözümlemesi
//!   desteklenir: Report ID, 8/12/16-bit eksenler, mutlak işaretçi, tekerlek
//!   ve klavye tuş dizisi.
//! - Tek aygıtta birden çok HID arayüzü (kablosuz klavye+fare alıcıları,
//!   multimedya klavyeler) aynı slot içinde ayrı kesme uç noktalarıyla çalışır.
//! - Caps Lock / Num Lock LED'leri SET_REPORT ile aygıta geri bildirilir.
//! - Kesme (interrupt) yerine yoklama (polling) kullanılır; masaüstü döngüsü
//!   olay halkasını her karede boşaltır.
//! - xHCI (USB 3.x), EHCI, UHCI ve USB Hub (Route String, Transaction
//!   Translator) arkasındaki cihazlar tam olarak desteklenir.
//! - Eski donanımlar ve USB 1.1/2.0 aygıtları için EHCI/UHCI/OHCI denetleyici
//!   başlatma/handoff ve transfer katmanı mevcuttur. UHCI/OHCI bugün HID
//!   kesme uç noktası yoklamasını sürer; HID raporları bu denetleyicilerden
//!   henüz ayrıştırılmaz (şimdilik firmware'in USB Legacy/8042 emülasyonu
//!   yedek yol olarak kullanılır).
//!
//! TR: USB klavye ve fare desteği; EN: USB keyboard and mouse support.

use core::sync::atomic::{fence, Ordering};

use crate::drivers::keyboard::KeyboardState;
use crate::drivers::mouse::MouseState;
use crate::drivers::ps2::InputUpdate;
use crate::mm::pmm::PhysicalMemoryManager;

// ---------------------------------------------------------------------------
// PCI yardımcıları
// ---------------------------------------------------------------------------

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[inline]
fn io_out32(port: u16, val: u32) {
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") val,
            options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn io_in32(port: u16) -> u32 {
    let ret: u32;
    unsafe {
        core::arch::asm!("in eax, dx", in("dx") port, out("eax") ret,
            options(nomem, nostack, preserves_flags));
    }
    ret
}

fn pci_read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in32(PCI_CONFIG_DATA)
}

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
// xHCI sabitleri
// ---------------------------------------------------------------------------

const MAX_CONTROLLERS: usize = 4;
const MAX_HID_DEVICES: usize = 8;
/// Tek bir USB aygıtında (ör. kablosuz klavye+fare alıcısı) desteklenen HID
/// arayüz sayısı. Her arayüz aynı slot içinde ayrı bir kesme uç noktası kullanır.
const MAX_HID_INTERFACES: usize = 4;
const MAX_PORTS: usize = 64;
const TRBS_PER_RING: usize = 256;
const RING_LINK_INDEX: usize = TRBS_PER_RING - 1;
const PAGE: u64 = 4096;

// Operasyonel kayıtlar (capability base'e göre)
const USBCMD: u64 = 0x00;
const USBSTS: u64 = 0x04;
const CRCR: u64 = 0x18;
const DCBAAP: u64 = 0x30;
const CONFIG: u64 = 0x38;
const PORTSC_BASE: u64 = 0x400;

const USBCMD_RUN: u32 = 1 << 0;
const USBCMD_HCRST: u32 = 1 << 1;
const USBCMD_INTE: u32 = 1 << 2;

const USBSTS_HCH: u32 = 1 << 0;
const USBSTS_HSE: u32 = 1 << 2;
const USBSTS_EINT: u32 = 1 << 3;
const USBSTS_PCD: u32 = 1 << 4;
const USBSTS_CNR: u32 = 1 << 11;
const USBSTS_CLEAR: u32 =
    (1 << 2) | (1 << 3) | (1 << 4) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 12);

const PORTSC_CCS: u32 = 1 << 0;
const PORTSC_PED: u32 = 1 << 1;
const PORTSC_PR: u32 = 1 << 4;
const PORTSC_PP: u32 = 1 << 9;
const PORTSC_SPEED_SHIFT: u32 = 10;
const PORTSC_PIC_MASK: u32 = 0x3 << 14;
const PORTSC_CSC: u32 = 1 << 17;
const PORTSC_PEC: u32 = 1 << 18;
const PORTSC_WRC: u32 = 1 << 19;
const PORTSC_OCC: u32 = 1 << 20;
const PORTSC_PRC: u32 = 1 << 21;
const PORTSC_PLC: u32 = 1 << 22;
const PORTSC_CEC: u32 = 1 << 23;
const PORTSC_CHANGE_BITS: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;
/// PORTSC yazarken korunması gereken yazılabilir bitler.
const PORTSC_KEEP: u32 = PORTSC_PP | PORTSC_PIC_MASK;
/// Warm Port Reset (USB3 portları için; xHCI 1.1+).
const PORTSC_WPR: u32 = 1 << 31;
/// Port Link State alanı (bit 5..8).
const PORTSC_PLS_SHIFT: u32 = 5;
const PORTSC_PLS_MASK: u32 = 0xF << PORTSC_PLS_SHIFT;

// TRB tipleri (control dword'ün 10..15. bitleri)
const TRB_TYPE_SHIFT: u32 = 10;
const TRB_C: u32 = 1 << 0;
const TRB_TC: u32 = 1 << 1;
const TRB_ISP: u32 = 1 << 2;
const TRB_CH: u32 = 1 << 4;
const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6;
const TRB_DIR: u32 = 1 << 16;
const TRB_TRT_SHIFT: u32 = 16;

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_DISABLE_SLOT: u32 = 10;
const TRB_ADDRESS_DEVICE: u32 = 11;
const TRB_CONFIGURE_ENDPOINT: u32 = 12;
const TRB_EVALUATE_CONTEXT: u32 = 13;
const TRB_RESET_ENDPOINT: u32 = 14;
const TRB_SET_TR_DEQUEUE: u32 = 16;

const TRB_TRANSFER_EVENT: u32 = 32;
const TRB_COMMAND_COMPLETION: u32 = 33;
const TRB_PORT_STATUS_CHANGE: u32 = 34;

const COMP_SUCCESS: u8 = 1;
const COMP_SHORT_PACKET: u8 = 13;

// USB hız kimlikleri (xHCI PORTSC)
const SPEED_FULL: u8 = 1;
const SPEED_LOW: u8 = 2;
const SPEED_HIGH: u8 = 3;
const SPEED_SUPER: u8 = 4;

// ---------------------------------------------------------------------------
// TRB ve halka (ring) yapıları
// ---------------------------------------------------------------------------

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct Trb {
    param: u64,
    status: u32,
    control: u32,
}

#[inline]
fn trb_type(control: u32) -> u32 {
    (control >> TRB_TYPE_SHIFT) & 0x3F
}

#[inline]
fn trb_slot(control: u32) -> u8 {
    ((control >> 24) & 0xFF) as u8
}

#[inline]
fn trb_endpoint(control: u32) -> u8 {
    ((control >> 16) & 0xFF) as u8
}

#[inline]
fn trb_completion(status: u32) -> u8 {
    ((status >> 24) & 0xFF) as u8
}

#[inline]
fn trb_residual(status: u32) -> usize {
    (status & 0x00FF_FFFF) as usize
}

#[inline]
unsafe fn zero_page(phys: u64) {
    core::ptr::write_bytes(crate::drivers::phys_to_virt(phys) as *mut u8, 0, PAGE as usize);
}

#[inline]
unsafe fn read_trb(ring_phys: u64, idx: usize) -> Trb {
    core::ptr::read_volatile((crate::drivers::phys_to_virt(ring_phys) as *const Trb).add(idx))
}

#[inline]
unsafe fn write_trb(ring_phys: u64, idx: usize, trb: Trb) {
    core::ptr::write_volatile(
        (crate::drivers::phys_to_virt(ring_phys) as *mut Trb).add(idx),
        trb,
    );
}

/// Komut ve transfer halkaları için ortak TRB halkası.
#[derive(Clone, Copy)]
struct TrbRing {
    phys: u64,
    idx: usize,
    cycle: u32,
}

impl TrbRing {
    const fn empty() -> Self {
        Self { phys: 0, idx: 0, cycle: 1 }
    }

    unsafe fn new(phys: u64) -> Self {
        zero_page(phys);
        let mut ring = Self { phys, idx: 0, cycle: 1 };
        ring.write_link(1);
        ring
    }

    unsafe fn write_link(&mut self, cycle: u32) {
        let link = Trb {
            param: self.phys,
            status: 0,
            control: (TRB_LINK << TRB_TYPE_SHIFT) | TRB_TC | (cycle & 1),
        };
        write_trb(self.phys, RING_LINK_INDEX, link);
        fence(Ordering::SeqCst);
    }

    /// Halkaya bir TRB ekler ve TRB'nin fiziksel adresini döner.
    unsafe fn push(&mut self, mut trb: Trb) -> u64 {
        // Son yuva Link TRB'ye ayrılmıştır; normalde buraya gelinmez.
        if self.idx >= RING_LINK_INDEX {
            self.write_link(self.cycle);
            self.idx = 0;
            self.cycle ^= 1;
        }
        trb.control = (trb.control & !TRB_C) | (self.cycle & 1);
        let addr = self.phys + (self.idx as u64) * 16;
        write_trb(self.phys, self.idx, trb);
        self.idx += 1;
        if self.idx == RING_LINK_INDEX {
            // Link TRB'nin cycle bitini bu turun değeriyle güncelle.
            self.write_link(self.cycle);
            self.idx = 0;
            self.cycle ^= 1;
        }
        fence(Ordering::SeqCst);
        addr
    }

    /// Denetleyicinin şu anda okuması gereken TRB adresi ve cycle biti.
    fn dequeue(&self) -> (u64, u32) {
        (self.phys + (self.idx as u64) * 16, self.cycle & 1)
    }
}

#[derive(Clone, Copy)]
struct CmdResult {
    code: u8,
    slot: u8,
}

// ---------------------------------------------------------------------------
// Bekleyen olaylar (komut bekleme sırasında gelen transfer olayları)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PendingEvent {
    controller: u8,
    trb: Trb,
}

static mut PENDING_EVENTS: [Option<PendingEvent>; 64] = [None; 64];

unsafe fn queue_pending(controller: usize, trb: Trb) {
    for entry in PENDING_EVENTS.iter_mut() {
        if entry.is_none() {
            *entry = Some(PendingEvent { controller: controller as u8, trb });
            return;
        }
    }
}

unsafe fn take_pending(
    controller: usize,
    event_type: u32,
    slot: Option<u8>,
    endpoint: Option<u8>,
) -> Option<Trb> {
    for entry in PENDING_EVENTS.iter_mut() {
        let Some(pending) = entry else { continue; };
        if pending.controller as usize != controller {
            continue;
        }
        let control = pending.trb.control;
        if trb_type(control) != event_type {
            continue;
        }
        if let Some(want_slot) = slot {
            if trb_slot(control) != want_slot {
                continue;
            }
        }
        if let Some(want_ep) = endpoint {
            if trb_endpoint(control) != want_ep {
                continue;
            }
        }
        let trb = pending.trb;
        *entry = None;
        return Some(trb);
    }
    None
}

// ---------------------------------------------------------------------------
// xHCI denetleyicisi
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct XhciController {
    index: u8,
    mmio: u64,
    op: u64,
    rt: u64,
    db: u64,
    bus: u8,
    dev: u8,
    func: u8,
    max_slots: u8,
    max_ports: u8,
    ctx_size: usize,
    cmd_ring: TrbRing,
    cmd_phys: u64,
    evt_phys: u64,
    evt_idx: usize,
    evt_cycle: u32,
    dcbaa_phys: u64,
    scratch_phys: u64,
    scratch_count: usize,
    last_scan_ms: u64,
    present: bool,
    /// Port başına slot: 0 = boş, 0xFF = HID değil/başarısız, diğer = HID slotu.
    port_slot: [u8; MAX_PORTS],
    port_connected: [bool; MAX_PORTS],
    /// Port başına aygıt başına ayrılan DMA bloğu (4 sayfa: input context,
    /// device context, EP0 halkası ve kontrol tamponu). Sıcak çıkarmada geri
    /// verilir; böylece tekrar tak/çıkar döngüleri bellek sızdırmaz.
    port_base_dma: [u64; MAX_PORTS],
}

impl XhciController {
    const fn empty() -> Self {
        Self {
            index: 0,
            mmio: 0,
            op: 0,
            rt: 0,
            db: 0,
            bus: 0,
            dev: 0,
            func: 0,
            max_slots: 0,
            max_ports: 0,
            ctx_size: 32,
            cmd_ring: TrbRing::empty(),
            cmd_phys: 0,
            evt_phys: 0,
            evt_idx: 0,
            evt_cycle: 1,
            dcbaa_phys: 0,
            scratch_phys: 0,
            scratch_count: 0,
            last_scan_ms: 0,
            present: false,
            port_slot: [0; MAX_PORTS],
            port_connected: [false; MAX_PORTS],
            port_base_dma: [0; MAX_PORTS],
        }
    }

    // MMIO erişimleri mutlak sanal adres alır. Capability alanı için
    // `self.mmio + ofset`, operasyonel alan için `self.op + ofset` geçilir.
    #[inline]
    unsafe fn rd32(&self, addr: u64) -> u32 {
        core::ptr::read_volatile(addr as *const u32)
    }

    #[inline]
    unsafe fn wr32(&self, addr: u64, val: u32) {
        core::ptr::write_volatile(addr as *mut u32, val);
    }

    #[inline]
    unsafe fn rd64(&self, addr: u64) -> u64 {
        core::ptr::read_volatile(addr as *const u64)
    }

    #[inline]
    unsafe fn wr64(&self, addr: u64, val: u64) {
        core::ptr::write_volatile(addr as *mut u64, val);
    }

    #[inline]
    fn port_offset(port: u8) -> u64 {
        // Her port 16 baytlık bir kayıt kümesine sahiptir; PORTSC bu kümenin
        // ilk 4 baytıdır (PORTSC, PORTPMSC, PORTLI, PORTHLPMC).
        PORTSC_BASE + (port.saturating_sub(1) as u64) * 0x10
    }

    unsafe fn doorbell(&self, slot: u8, target: u8) {
        let off = self.db + (slot as u64) * 4;
        core::ptr::write_volatile(off as *mut u32, target as u32);
        fence(Ordering::SeqCst);
    }

    unsafe fn has_event(&self) -> bool {
        let trb = (crate::drivers::phys_to_virt(self.evt_phys) as *const Trb).add(self.evt_idx);
        let event = core::ptr::read_volatile(trb);
        (event.control & 1) == (self.evt_cycle & 1)
    }

    unsafe fn pop_event(&mut self) -> Option<Trb> {
        let trb = (crate::drivers::phys_to_virt(self.evt_phys) as *const Trb).add(self.evt_idx);
        let event = core::ptr::read_volatile(trb);
        if (event.control & 1) != (self.evt_cycle & 1) {
            return None;
        }

        self.evt_idx += 1;
        if self.evt_idx == TRBS_PER_RING {
            self.evt_idx = 0;
            self.evt_cycle ^= 1;
        }

        // EHB (bit 3) yazılarak olay işleyicinin meşgul biti temizlenir.
        let erdp = (self.evt_phys + (self.evt_idx as u64) * 16) | (1 << 3);
        core::ptr::write_volatile((self.rt + 0x20 + 0x18) as *mut u64, erdp);
        fence(Ordering::SeqCst);
        Some(event)
    }

    unsafe fn wait_for_command(&mut self, addr: u64) -> CmdResult {
        let start = crate::boot::time::monotonic_ms();
        loop {
            if let Some(event) = take_pending(self.index as usize, TRB_COMMAND_COMPLETION, None, None) {
                if event.param == addr {
                    return CmdResult {
                        code: trb_completion(event.status),
                        slot: trb_slot(event.control),
                    };
                }
            }

            if let Some(event) = self.pop_event() {
                if trb_type(event.control) == TRB_COMMAND_COMPLETION && event.param == addr {
                    return CmdResult {
                        code: trb_completion(event.status),
                        slot: trb_slot(event.control),
                    };
                }
                queue_pending(self.index as usize, event);
            }

            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 1_500 {
                return CmdResult { code: 0xFF, slot: 0 };
            }
            core::hint::spin_loop();
        }
    }

    unsafe fn run_command(&mut self, trb: Trb) -> CmdResult {
        let addr = self.cmd_ring.push(trb);
        self.doorbell(0, 0);
        self.wait_for_command(addr)
    }

    unsafe fn wait_transfer(&mut self, slot: u8, dci: u8) -> Option<Trb> {
        let start = crate::boot::time::monotonic_ms();
        loop {
            if let Some(event) =
                take_pending(self.index as usize, TRB_TRANSFER_EVENT, Some(slot), Some(dci))
            {
                return Some(event);
            }

            if let Some(event) = self.pop_event() {
                if trb_type(event.control) == TRB_TRANSFER_EVENT
                    && trb_slot(event.control) == slot
                    && trb_endpoint(event.control) == dci
                {
                    return Some(event);
                }
                queue_pending(self.index as usize, event);
            }

            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 2_000 {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    unsafe fn disable_slot(&mut self, slot: u8) {
        let trb = Trb {
            param: 0,
            status: 0,
            control: (TRB_DISABLE_SLOT << TRB_TYPE_SHIFT) | ((slot as u32) << 24),
        };
        let _ = self.run_command(trb);
    }

    unsafe fn enable_slot(&mut self) -> Option<u8> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: TRB_ENABLE_SLOT << TRB_TYPE_SHIFT,
        };
        let result = self.run_command(trb);
        if result.code == COMP_SUCCESS && result.slot != 0 {
            Some(result.slot)
        } else {
            None
        }
    }

    unsafe fn address_device(&mut self, slot: u8, input_phys: u64) -> bool {
        let trb = Trb {
            param: input_phys,
            status: 0,
            control: (TRB_ADDRESS_DEVICE << TRB_TYPE_SHIFT) | ((slot as u32) << 24),
        };
        let result = self.run_command(trb);
        if result.code != COMP_SUCCESS {
            crate::serial::print("[USB-HID] Address Device tamamlanma kodu: ");
            crate::serial::print_dec(result.code as u64);
            crate::serial::print("\n");
        }
        result.code == COMP_SUCCESS
    }

    unsafe fn evaluate_context(&mut self, slot: u8, input_phys: u64) -> bool {
        let trb = Trb {
            param: input_phys,
            status: 0,
            control: (TRB_EVALUATE_CONTEXT << TRB_TYPE_SHIFT) | ((slot as u32) << 24),
        };
        self.run_command(trb).code == COMP_SUCCESS
    }

    unsafe fn configure_endpoint(&mut self, slot: u8, input_phys: u64) -> bool {
        let trb = Trb {
            param: input_phys,
            status: 0,
            control: (TRB_CONFIGURE_ENDPOINT << TRB_TYPE_SHIFT) | ((slot as u32) << 24),
        };
        self.run_command(trb).code == COMP_SUCCESS
    }

    unsafe fn reset_endpoint(&mut self, slot: u8, dci: u8) -> bool {
        let trb = Trb {
            param: 0,
            status: 0,
            control: (TRB_RESET_ENDPOINT << TRB_TYPE_SHIFT)
                | ((dci as u32) << 16)
                | ((slot as u32) << 24),
        };
        self.run_command(trb).code == COMP_SUCCESS
    }

    unsafe fn set_tr_dequeue(&mut self, slot: u8, dci: u8, address: u64) -> bool {
        let trb = Trb {
            param: address,
            status: 0,
            control: (TRB_SET_TR_DEQUEUE << TRB_TYPE_SHIFT)
                | ((dci as u32) << 16)
                | ((slot as u32) << 24),
        };
        self.run_command(trb).code == COMP_SUCCESS
    }

    unsafe fn ctx_write(&self, base_phys: u64, index: usize, dword: usize, value: u32) {
        let base = crate::drivers::phys_to_virt(base_phys) as *mut u32;
        let offset = index * (self.ctx_size / 4) + dword;
        core::ptr::write_volatile(base.add(offset), value);
    }

    /// Address Device için giriş bağlamını hazırlar.
    unsafe fn build_address_context(
        &self,
        input_phys: u64,
        speed: u8,
        port: u8,
        max_packet: u16,
        ep0_ring: &TrbRing,
        route_string: u32,
        tt_hub_slot: u8,
        tt_port: u8,
    ) {
        zero_page(input_phys);
        // Input Control Context: A0 (Slot Context) ve A1 (EP0) birlikte eklenmelidir.
        self.ctx_write(input_phys, 0, 1, (1 << 0) | (1 << 1));
        // Slot Context (input index 1). Route String | Speed | Context Entries = 1
        self.ctx_write(input_phys, 1, 0, (route_string & 0xFFFFF) | ((speed as u32) << 20) | (1 << 27));
        self.ctx_write(input_phys, 1, 1, (port as u32) << 16);
        if tt_hub_slot != 0 {
            self.ctx_write(input_phys, 1, 2, (tt_hub_slot as u32) | ((tt_port as u32) << 8));
        }
        // EP0 Context (input index 2 = DCI 1 + 1).
        self.ctx_write(input_phys, 2, 0, 0);
        self.ctx_write(
            input_phys,
            2,
            1,
            (3 << 1) | (4 << 3) | ((max_packet as u32) << 16),
        );
        let (deq, dcs) = ep0_ring.dequeue();
        self.ctx_write(input_phys, 2, 2, (deq as u32) | (dcs & 1));
        self.ctx_write(input_phys, 2, 3, (deq >> 32) as u32);
    }

    /// Evaluate Context: EP0 max packet boyutunu günceller.
    unsafe fn update_ep0_max_packet(
        &mut self,
        slot: u8,
        input_phys: u64,
        speed: u8,
        port: u8,
        max_packet: u16,
        ep0_ring: &TrbRing,
        route_string: u32,
        tt_hub_slot: u8,
        tt_port: u8,
    ) -> bool {
        zero_page(input_phys);
        self.ctx_write(input_phys, 0, 1, (1 << 0) | (1 << 1));
        self.ctx_write(input_phys, 1, 0, (route_string & 0xFFFFF) | ((speed as u32) << 20) | (1 << 27));
        self.ctx_write(input_phys, 1, 1, (port as u32) << 16);
        if tt_hub_slot != 0 {
            self.ctx_write(input_phys, 1, 2, (tt_hub_slot as u32) | ((tt_port as u32) << 8));
        }
        self.ctx_write(input_phys, 2, 1, (3 << 1) | (4 << 3) | ((max_packet as u32) << 16));
        let (deq, dcs) = ep0_ring.dequeue();
        self.ctx_write(input_phys, 2, 2, (deq as u32) | (dcs & 1));
        self.ctx_write(input_phys, 2, 3, (deq >> 32) as u32);
        self.evaluate_context(slot, input_phys)
    }

    unsafe fn control_transfer(
        &mut self,
        ep0_ring: &mut TrbRing,
        slot: u8,
        direction_in: bool,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_phys: u64,
        length: usize,
    ) -> Option<usize> {
        let setup = [
            request_type,
            request,
            (value & 0xFF) as u8,
            (value >> 8) as u8,
            (index & 0xFF) as u8,
            (index >> 8) as u8,
            (length & 0xFF) as u8,
            (length >> 8) as u8,
        ];
        let setup_word = u64::from_le_bytes(setup);
        let slot_bits = (slot as u32) << 24;
        let trt: u32 = if length == 0 {
            0
        } else if direction_in {
            3
        } else {
            2
        };

        let setup_trb = Trb {
            param: setup_word,
            status: 8,
            control: (TRB_SETUP << TRB_TYPE_SHIFT) | TRB_IDT | TRB_CH | (trt << TRB_TRT_SHIFT) | slot_bits,
        };
        ep0_ring.push(setup_trb);

        if length > 0 {
            let data_control = (TRB_DATA << TRB_TYPE_SHIFT)
                | TRB_CH
                | if direction_in { TRB_DIR } else { 0 }
                | slot_bits;
            let data_trb = Trb {
                param: data_phys,
                status: (length as u32) & 0x1FFFF,
                control: data_control,
            };
            ep0_ring.push(data_trb);
        }

        // Durum aşaması veri yönünün tersidir; veri yoksa IN yönündedir.
        let status_in = if length == 0 { true } else { !direction_in };
        let status_trb = Trb {
            param: 0,
            status: 0,
            control: (TRB_STATUS << TRB_TYPE_SHIFT)
                | TRB_IOC
                | if status_in { TRB_DIR } else { 0 }
                | slot_bits,
        };
        ep0_ring.push(status_trb);
        self.doorbell(slot, 1);

        let event = self.wait_transfer(slot, 1)?;
        let code = trb_completion(event.status);
        if code != COMP_SUCCESS && code != COMP_SHORT_PACKET {
            return None;
        }

        if direction_in {
            let residual = trb_residual(event.status);
            Some(length.saturating_sub(residual))
        } else {
            Some(length)
        }
    }

    unsafe fn control_in(
        &mut self,
        ep0_ring: &mut TrbRing,
        slot: u8,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_phys: u64,
        length: usize,
    ) -> Option<usize> {
        self.control_transfer(
            ep0_ring, slot, true, request_type, request, value, index, data_phys, length,
        )
    }

    unsafe fn control_out(
        &mut self,
        ep0_ring: &mut TrbRing,
        slot: u8,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_phys: u64,
        length: usize,
    ) -> Option<usize> {
        self.control_transfer(
            ep0_ring, slot, false, request_type, request, value, index, data_phys, length,
        )
    }

    /// Portu sıfırlar ve bağlı aygıtın hız kimliğini döner.
    unsafe fn reset_port(&mut self, port: u8) -> Option<u8> {
        let off = self.op + Self::port_offset(port);
        self.power_port(port);
        let mut status = self.rd32(off);
        if status & PORTSC_CCS == 0 {
            return None;
        }

        // Önceki değişiklik bayraklarını temizle, güç bitini koru.
        self.wr32(off, (status & PORTSC_KEEP) | (status & PORTSC_CHANGE_BITS));
        status = self.rd32(off);

        // Port Link State = Disabled (3) veya Compliance (4) durumundaki
        // portlar normal Reset (PR) komutunu yoksayar; bu durumda Warm Port
        // Reset (WPR) gerekir. USB3/SuperSpeed portlarında da WPR kullanılır.
        let pls = (status & PORTSC_PLS_MASK) >> PORTSC_PLS_SHIFT;
        let warm_reset = status & PORTSC_WPR != 0 || pls == 3 || pls == 4 || pls == 5;
        let reset_bit = if warm_reset { PORTSC_WPR } else { PORTSC_PR };
        self.wr32(off, (status & PORTSC_KEEP) | reset_bit);
        let start = crate::boot::time::monotonic_ms();
        loop {
            let current = self.rd32(off);
            let done = if warm_reset {
                current & (PORTSC_WRC | PORTSC_PRC) != 0
            } else {
                current & PORTSC_PRC != 0
            };
            if done {
                let speed = ((current >> PORTSC_SPEED_SHIFT) & 0xF) as u8;
                self.wr32(
                    off,
                    (current & PORTSC_KEEP) | (current & PORTSC_CHANGE_BITS),
                );
                if speed == 0 {
                    return None;
                }
                return Some(speed);
            }
            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 600 {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    /// Porta güç verir (PP biti 0 ise). PPC'siz denetleyicilerde PP daima 1
    /// okunur; yazma zararsızdır. Güç verildikten sonra aygıtın bağlanması
    /// için kısa bir bekleme yapılır.
    unsafe fn power_port(&mut self, port: u8) {
        let off = self.op + Self::port_offset(port);
        let status = self.rd32(off);
        if status & PORTSC_PP == 0 {
            self.wr32(off, (status & PORTSC_KEEP) | PORTSC_PP);
            let start = crate::boot::time::monotonic_ms();
            while crate::boot::time::monotonic_ms().wrapping_sub(start) < 20 {
                core::hint::spin_loop();
            }
        }
    }

    unsafe fn init_hardware(&mut self, pmm: &mut PhysicalMemoryManager) -> bool {
        let caplen = core::ptr::read_volatile(self.mmio as *const u8) as u64;
        let hcsparams1 = self.rd32(self.mmio + 0x04);
        let hcsparams2 = self.rd32(self.mmio + 0x08);
        let hccparams1 = self.rd32(self.mmio + 0x10);
        let dboff = self.rd32(self.mmio + 0x14) as u64;
        let rtsoff = self.rd32(self.mmio + 0x18) as u64;

        self.op = self.mmio + caplen;
        self.db = self.mmio + dboff;
        self.rt = self.mmio + rtsoff;
        self.ctx_size = if hccparams1 & (1 << 2) != 0 { 64 } else { 32 };
        self.max_slots = ((hcsparams1 & 0xFF) as u8).max(1);
        // HCSPARAMS1: MaxPorts alanı bit 24..31'dedir (bit 16..23 değil).
        self.max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

        crate::serial::print("[USB-HID] xHCI: slots=");
        crate::serial::print_dec(self.max_slots as u64);
        crate::serial::print(" ports=");
        crate::serial::print_dec(self.max_ports as u64);
        crate::serial::print(" ctx=");
        crate::serial::print_dec(self.ctx_size as u64);
        crate::serial::print("\n");

        if self.max_ports == 0 {
            return false;
        }

        // Denetleyiciyi durdur, sıfırla.
        let cmd = self.rd32(self.op + USBCMD);
        if cmd & USBCMD_RUN != 0 {
            self.wr32(self.op + USBCMD, cmd & !USBCMD_RUN);
            let start = crate::boot::time::monotonic_ms();
            while self.rd32(self.op + USBSTS) & USBSTS_HCH == 0 {
                if crate::boot::time::monotonic_ms().wrapping_sub(start) > 250 {
                    break;
                }
                core::hint::spin_loop();
            }
        }

        self.wr32(self.op + USBSTS, USBSTS_CLEAR);
        self.wr32(self.op + USBCMD, USBCMD_HCRST);
        let start = crate::boot::time::monotonic_ms();
        loop {
            let status = self.rd32(self.op + USBCMD);
            let state = self.rd32(self.op + USBSTS);
            if status & USBCMD_HCRST == 0 && state & USBSTS_CNR == 0 {
                break;
            }
            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 1_000 {
                crate::serial::print("[USB-HID] xHCI reset zaman aşımı.\n");
                return false;
            }
            core::hint::spin_loop();
        }
        crate::serial::print("[USB-HID] xHCI denetleyici sıfırlandı.\n");

        // Scratchpad buffer sayısı (HCSPARAMS2): Hi alanı bit 21..25,
        // Lo alanı bit 27..31'dedir. Sayı = (Hi << 5) | Lo.
        let scratch_hi = ((hcsparams2 >> 21) & 0x1F) as usize;
        let scratch_lo = ((hcsparams2 >> 27) & 0x1F) as usize;
        let mut scratch_count = (scratch_hi << 5) | scratch_lo;
        if scratch_count > 64 {
            scratch_count = 64;
        }
        self.scratch_count = scratch_count;

        // Komut halkası, olay halkası, ERST ve DCBAA için fiziksel sayfalar.
        let Some(cmd_phys) = pmm.allocate_frame() else { return false; };
        let Some(evt_phys) = pmm.allocate_frame() else {
            pmm.free_frame(cmd_phys);
            return false;
        };
        let Some(erst_phys) = pmm.allocate_frame() else {
            pmm.free_frame(cmd_phys);
            pmm.free_frame(evt_phys);
            return false;
        };

        let dcbaa_entries = self.max_slots as usize + 1 + scratch_count;
        let dcbaa_frames = ((dcbaa_entries * 8) + PAGE as usize - 1) / PAGE as usize;
        let Some(dcbaa_phys) = pmm.allocate_contiguous_frames(dcbaa_frames.max(1)) else {
            pmm.free_frame(cmd_phys);
            pmm.free_frame(evt_phys);
            pmm.free_frame(erst_phys);
            return false;
        };

        if scratch_count > 0 {
            match pmm.allocate_contiguous_frames(scratch_count) {
                Some(phys) => self.scratch_phys = phys,
                None => {
                    pmm.free_frame(cmd_phys);
                    pmm.free_frame(evt_phys);
                    pmm.free_frame(erst_phys);
                    for i in 0..dcbaa_frames.max(1) {
                        pmm.free_frame(dcbaa_phys + (i as u64) * PAGE);
                    }
                    return false;
                }
            }
        }

        zero_page(cmd_phys);
        zero_page(evt_phys);
        zero_page(erst_phys);
        for i in 0..dcbaa_frames.max(1) {
            zero_page(dcbaa_phys + (i as u64) * PAGE);
        }

        self.cmd_phys = cmd_phys;
        self.cmd_ring = TrbRing::new(cmd_phys);
        self.evt_phys = evt_phys;
        self.evt_idx = 0;
        self.evt_cycle = 1;
        self.dcbaa_phys = dcbaa_phys;

        // DCBAA: scratchpad arabellekleri max_slots sonrasına yerleştirilir.
        let dcbaa = crate::drivers::phys_to_virt(dcbaa_phys) as *mut u64;
        for i in 0..scratch_count {
            core::ptr::write_volatile(
                dcbaa.add(self.max_slots as usize + 1 + i),
                self.scratch_phys + (i as u64) * PAGE,
            );
        }

        // Olay Halkası Segment Tablosu (tek segment).
        let erst = crate::drivers::phys_to_virt(erst_phys) as *mut u32;
        core::ptr::write_volatile(erst, evt_phys as u32);
        core::ptr::write_volatile(erst.add(1), (evt_phys >> 32) as u32);
        core::ptr::write_volatile(erst.add(2), TRBS_PER_RING as u32);
        core::ptr::write_volatile(erst.add(3), 0);

        self.wr32(self.op + CONFIG, self.max_slots as u32);
        self.wr64(self.op + DCBAAP, dcbaa_phys);
        self.wr64(self.op + CRCR, cmd_phys | 1);

        // Interrupter 0 — kesme kapalı, yalnızca olay halkası yoklaması.
        let intr = self.rt + 0x20;
        core::ptr::write_volatile((intr + 0x00) as *mut u32, 0);
        core::ptr::write_volatile((intr + 0x04) as *mut u32, 0);
        core::ptr::write_volatile((intr + 0x08) as *mut u32, 1);
        core::ptr::write_volatile((intr + 0x10) as *mut u64, erst_phys);
        core::ptr::write_volatile((intr + 0x18) as *mut u64, evt_phys | (1 << 3));

        self.wr32(self.op + USBSTS, USBSTS_CLEAR);
        self.wr32(self.op + USBCMD, USBCMD_RUN);
        let start = crate::boot::time::monotonic_ms();
        loop {
            let state = self.rd32(self.op + USBSTS);
            if state & USBSTS_HCH == 0 {
                break;
            }
            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 500 {
                crate::serial::print("[USB-HID] xHCI çalıştırma zaman aşımı.\n");
                return false;
            }
            core::hint::spin_loop();
        }
        crate::serial::print("[USB-HID] xHCI çalışıyor.\n");

        let state = self.rd32(self.op + USBSTS);
        if state & (USBSTS_HSE | (1 << 12)) != 0 {
            crate::serial::print("[USB-HID] xHCI donanım hatası bildirdi.\n");
            return false;
        }

        self.present = true;
        true
    }

    /// xHCI USB Legacy Support: BIOS SMM devrini kapatır; böylece USB
    /// klavye/fare olayları 8042 emülasyonuyla ikinci kez üretilmez.
    unsafe fn legacy_handoff(&mut self) {
        let hccparams1 = self.rd32(self.mmio + 0x10);
        let mut offset = ((hccparams1 >> 16) & 0xFFFF) as u64;
        if offset == 0 {
            return;
        }
        offset <<= 2;

        for _ in 0..16 {
            if offset < 0x40 {
                break;
            }
            let addr = self.mmio + offset;
            let val = self.rd32(addr);
            let id = val & 0xFF;
            let next = ((val >> 8) & 0xFF) as u64;
            if id == 0x01 {
                self.wr32(addr, val | (1 << 24)); // OS Owned Semaphore
                let start = crate::boot::time::monotonic_ms();
                while self.rd32(addr) & (1 << 16) != 0 {
                    if crate::boot::time::monotonic_ms().wrapping_sub(start) > 1_000 {
                        crate::serial::print(
                            "[USB-HID] UYARI: BIOS USB legacy devri zaman aşımı; SMM kapatılıyor.\n",
                        );
                        break;
                    }
                    core::hint::spin_loop();
                }
                // USB SMI'larını kapat (USBLEGCTLSTS).
                self.wr32(addr + 4, 0);
                crate::serial::print("[USB-HID] USB Legacy devri tamamlandı (SMM kapalı).\n");
                return;
            }
            if next == 0 {
                break;
            }
            offset = next << 2;
        }
    }
}

static mut CONTROLLERS: [XhciController; MAX_CONTROLLERS] = [XhciController::empty(); MAX_CONTROLLERS];
static mut CONTROLLER_COUNT: usize = 0;
static mut INITIALIZED: bool = false;

// ---------------------------------------------------------------------------
// HID aygıtı ve rapor çözümleme
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum HidKind {
    Keyboard,
    Mouse,
}

impl HidKind {
    fn label(self) -> &'static str {
        match self {
            HidKind::Keyboard => "klavye",
            HidKind::Mouse => "fare",
        }
    }
}

#[derive(Clone, Copy)]
struct HidDevice {
    active: bool,
    controller: u8,
    slot: u8,
    port: u8,
    kind: HidKind,
    dci: u8,
    interface: u8,
    ep_ring: TrbRing,
    report_phys: u64,
    report_len: usize,
    prev_modifiers: u8,
    prev_keys: [u8; 6],
    /// Aygıt boot protokolünü kabul etmedi (SET_PROTOCOL başarısız) veya
    /// arayüz boot protokolü sunmuyor; raporlar aygıtın kendi biçiminde gelir
    /// ve `HidReportLayout` ile ayrıştırılır.
    custom: bool,
    layout: HidReportLayout,
    /// Tanılama: ilk birkaç raporun hex dökümü seri günlüğe yazılır.
    samples: u8,
    /// Bu aygıtın kesme halkası + rapor tamponu için ayrılan 2 sayfalık DMA.
    /// Çıkarmada serbest bırakılır.
    dma_phys: u64,
    /// Klavye LED çıktısı için kontrol aktarımı durumu. Yalnızca bir slotun
    /// ilk klavyesinde doludur (EP0 halkası slot genelinde ortaktır).
    ep0_ring: TrbRing,
    ctrl_phys: u64,
    led_capable: bool,
    led_report_id: u8,
    /// Aygıta en son gönderilen LED değeri; 0xFF = henüz gönderilmedi.
    leds: u8,
}

impl HidDevice {
    const fn empty() -> Self {
        Self {
            active: false,
            controller: 0,
            slot: 0,
            port: 0,
            kind: HidKind::Keyboard,
            dci: 0,
            interface: 0,
            ep_ring: TrbRing::empty(),
            report_phys: 0,
            report_len: 0,
            prev_modifiers: 0,
            prev_keys: [0; 6],
            custom: false,
            layout: HidReportLayout::mouse_boot(),
            samples: 0,
            dma_phys: 0,
            ep0_ring: TrbRing::empty(),
            ctrl_phys: 0,
            led_capable: false,
            led_report_id: 0,
            leds: 0xFF,
        }
    }
}

static mut HID_DEVICES: [HidDevice; MAX_HID_DEVICES] = [HidDevice::empty(); MAX_HID_DEVICES];

#[derive(Clone, Copy)]
struct HidCandidate {
    interface: u8,
    /// Protokol alanı veya rapor tanımlayıcısı ile belirlenir; `None` ise
    /// arayüz HID klavye/fare değildir ve atlanır.
    kind: Option<HidKind>,
    endpoint: u8,
    max_packet: usize,
    interval: u8,
    /// Arayüz boot protokolünü destekliyor (HID alt sınıf 1 + protokol 1/2).
    boot: bool,
    /// SET_PROTOCOL başarısız olduysa veya arayüz boot protokolü sunmuyorsa
    /// raporlar özel düzenle ayrıştırılır.
    custom: bool,
    /// Rapor tanımlayıcısından çıkarılan düzen.
    layout: HidReportLayout,
}

impl HidCandidate {
    const fn empty() -> Self {
        Self {
            interface: 0,
            kind: None,
            endpoint: 0,
            max_packet: 8,
            interval: 10,
            boot: false,
            custom: true,
            layout: HidReportLayout::empty(),
        }
    }
}

/// HID rapor tanımlayıcısından çıkarılan alan yerleşimi.
///
/// Hem fare/işaretçi (düğmeler, X/Y, tekerlek, pan, mutlak/göreli) hem de
/// klavye (modifier baytı, tuş dizisi) alanlarını taşır. Aynı aygıt iki top-level
/// koleksiyon içerebilir; bu durumda her iki bayrak da set edilebilir.
#[derive(Clone, Copy)]
struct HidReportLayout {
    /// Düğme alanının başlangıç biti ve bit sayısı.
    button_start: u8,
    buttons: u8,
    x_start: u8,
    x_size: u8,
    y_start: u8,
    y_size: u8,
    wheel_start: u8,
    wheel_size: u8,
    pan_start: u8,
    pan_size: u8,
    /// Aygıt çoklu rapor kullanıyorsa rapor gövdesinden önceki Report ID.
    report_id: u8,
    /// X/Y alanının mutlak (Absolute) olup olmadığı ve mantıksal maksimumu.
    x_absolute: bool,
    y_absolute: bool,
    x_max: u32,
    y_max: u32,
    /// Klavye alanları: modifier baytı ve tuş dizisinin bayt konumları.
    modifier_offset: u8,
    key_offset: u8,
    key_count: u8,
    /// LED çıktı raporu (Caps/Num/Scroll Lock) var mı?
    led_output: bool,
    led_offset: u8,
    led_report_id: u8,
    /// Rapor tanımlayıcısının içerdiği işlevsel sınıflar.
    keyboard: bool,
    pointer: bool,
}

impl HidReportLayout {
    const fn empty() -> Self {
        Self {
            button_start: 0,
            buttons: 0,
            x_start: 0,
            x_size: 0,
            y_start: 0,
            y_size: 0,
            wheel_start: 0,
            wheel_size: 0,
            pan_start: 0,
            pan_size: 0,
            report_id: 0,
            x_absolute: false,
            y_absolute: false,
            x_max: 0,
            y_max: 0,
            modifier_offset: 0,
            key_offset: 0,
            key_count: 0,
            led_output: false,
            led_offset: 0,
            led_report_id: 0,
            keyboard: false,
            pointer: false,
        }
    }

    /// Ayrıştırılamayan fareler için standart 3 baytlık (düğme, dX, dY) yerleşim.
    const fn mouse_boot() -> Self {
        Self {
            button_start: 0,
            buttons: 3,
            x_start: 8,
            x_size: 8,
            y_start: 16,
            y_size: 8,
            wheel_start: 0,
            wheel_size: 0,
            pan_start: 0,
            pan_size: 0,
            report_id: 0,
            x_absolute: false,
            y_absolute: false,
            x_max: 0,
            y_max: 0,
            modifier_offset: 0,
            key_offset: 0,
            key_count: 0,
            led_output: false,
            led_offset: 0,
            led_report_id: 0,
            keyboard: false,
            pointer: true,
        }
    }

    /// Ayrıştırılamayan klavyeler için boot protokolü yerleşimi.
    const fn keyboard_boot() -> Self {
        Self {
            button_start: 0,
            buttons: 0,
            x_start: 0,
            x_size: 0,
            y_start: 0,
            y_size: 0,
            wheel_start: 0,
            wheel_size: 0,
            pan_start: 0,
            pan_size: 0,
            report_id: 0,
            x_absolute: false,
            y_absolute: false,
            x_max: 0,
            y_max: 0,
            modifier_offset: 0,
            key_offset: 2,
            key_count: 6,
            led_output: true,
            led_offset: 0,
            led_report_id: 0,
            keyboard: true,
            pointer: false,
        }
    }

    /// Fare/işaretçi alanları geçerli mi? Göreli farelerde mantıksal maksimum
    /// bulunmaz; bu yüzden yalnızca X/Y alanlarının varlığı aranır.
    const fn pointer_valid(&self) -> bool {
        self.pointer && self.x_size > 0 && self.y_size > 0
    }

    /// Klavye alanları (8 bitlik tuş dizisi) geçerli mi?
    const fn keyboard_valid(&self) -> bool {
        self.keyboard && self.key_count > 0 && self.key_offset >= 1
    }
}

/// İşaret alanı için mantıksal maksimumu bit genişliğinden türetir.
const fn max_for_bits(bits: u8) -> u32 {
    match bits {
        0 => 0,
        1..=15 => (1u32 << bits) - 1,
        _ => u32::MAX,
    }
}

/// HID rapor tanımlayıcısını (Report Descriptor) ayrıştırır ve fare raporunun
/// alan yerleşimini (düğme sayısı, X/Y/tekerlek bit konumları, mutlak/göreli)
/// çıkarır. Klavye ise varsayılan düzen döner.
///
/// Desteklenen öğeler: Usage Page, Usage, Input, Report Count, Report Size,
/// Report ID, Collection/End Collection. Laptop yerleşik fareler ve
/// dokunmatik yüzeyler için yeterlidir.
fn parse_hid_report_descriptor(desc: &[u8]) -> HidReportLayout {
    let mut layout = HidReportLayout::empty();
    let mut page: u16 = 0;
    let mut usage: u32 = 0;
    // Bir Input öğesi birden çok kullanım bildirebilir (ör. X ve Y aynı
    // Report Count=2 öğesinde). Bu durumda alanlar sırayla eşlenir.
    let mut usages = [0u32; 8];
    let mut usage_count = 0usize;
    let mut usage_min: u32 = 0;
    let mut usage_max: u32 = 0;
    let mut logical_max: i32 = 0;
    let mut report_size: u32 = 0;
    let mut report_count: u32 = 0;
    let mut report_id: u8 = 0;
    let mut first_report_id: Option<u8> = None;
    let mut modifier_found = false;
    let mut key_found = false;
    // Giriş raporu bit konumu; Report ID gövdeye dahil değildir.
    let mut bit: u32 = 0;
    // Çıkış raporu bit konumu (LED'ler).
    let mut out_bit: u32 = 0;
    let mut depth: u32 = 0;
    let mut app_page: u16 = 0;
    let mut app_usage: u32 = 0;
    let mut i = 0usize;

    while i < desc.len() {
        let prefix = desc[i];
        i += 1;
        if prefix == 0xFE {
            // Long item: [0xFE][bDataSize][bLongItemTag][data...]
            if i + 1 >= desc.len() {
                break;
            }
            let len = desc[i] as usize;
            i += 2 + len;
            continue;
        }
        let b_size = (prefix & 0x03) as usize;
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        if i + b_size > desc.len() {
            break;
        }
        let mut value = 0u32;
        for k in 0..b_size {
            value |= (desc[i + k] as u32) << (8 * k);
        }
        i += b_size;
        let signed = match b_size {
            1 => value as u8 as i8 as i32,
            2 => value as u16 as i16 as i32,
            4 => value as i32,
            _ => 0,
        };

        match item_type {
            // ---- Global öğeler ----
            0x01 => match tag {
                0x00 => page = value as u16, // Usage Page
                0x01 => {}                   // Logical Minimum (işaretli alanlar için gerekmez)
                0x02 => logical_max = signed, // Logical Maximum
                0x07 => report_size = value & 0xFFFF,
                0x08 => {
                    let id = value as u8;
                    if let Some(first) = first_report_id {
                        // Tanımlayıcı birden çok rapor içeriyorsa ilk raporun
                        // alanlarını kullan; sonraki raporlara geçme.
                        if id != first && (key_found || layout.pointer || layout.led_output) {
                            break;
                        }
                    }
                    if first_report_id.is_none() {
                        first_report_id = Some(id);
                    }
                    report_id = id;
                }
                0x09 => report_count = value & 0xFFFF,
                _ => {}
            },
            // ---- Local öğeler ----
            0x02 => match tag {
                0x00 => {
                    if usage_count == 0 {
                        usage = value;
                    }
                    if usage_count < usages.len() {
                        usages[usage_count] = value;
                        usage_count += 1;
                    }
                }
                0x01 => usage_min = value,
                0x02 => usage_max = value,
                _ => {}
            },
            // ---- Main öğeler ----
            0x00 => match tag {
                0x08 => {
                    // Input öğesi bayrakları: bit0 0 = Data / 1 = Constant,
                    // bit1 1 = Variable / 0 = Array, bit2 0 = Absolute / 1 = Relative.
                    let is_data = value & 0x01 == 0;
                    let is_var = value & 0x02 != 0;
                    let is_abs = value & 0x04 == 0;
                    let bits = report_count.saturating_mul(report_size);
                    if is_data && bits > 0 {
                        // Düğmeler (Button sayfası, Variable).
                        if page == 0x09 && is_var && layout.buttons == 0 {
                            layout.button_start = bit.min(255) as u8;
                            layout.buttons = bits.min(8) as u8;
                        }
                        // Klavye modifier baytı (E0..E7).
                        else if page == 0x07
                            && is_var
                            && report_size == 1
                            && !modifier_found
                            && (usage == 0xE0
                                || (usage_min >= 0xE0 && usage_max <= 0xE7 && usage_max > usage_min))
                        {
                            layout.modifier_offset = (bit / 8).min(255) as u8;
                            modifier_found = true;
                        }
                        // Klavye tuş dizisi (Array, 8 bitlik kullanım kimlikleri).
                        else if page == 0x07
                            && !is_var
                            && report_size == 8
                            && report_count > 0
                            && !key_found
                        {
                            layout.key_offset = (bit / 8).min(255) as u8;
                            layout.key_count = report_count.min(6) as u8;
                            key_found = true;
                        }
                        // Variable alanlar: her alan kendi kullanımına karşılık
                        // gelir. X ve Y çoğu zaman tek Input öğesinde
                        // (Report Count=2) birlikte bildirilir.
                        else if is_var {
                            let fields = report_count.min(16);
                            for k in 0..fields {
                                let index = k as usize;
                                let field_usage = if index < usage_count {
                                    usages[index]
                                } else if usage_count > 0 {
                                    usages[usage_count - 1]
                                } else {
                                    0
                                };
                                let field_bit = bit.saturating_add(k.saturating_mul(report_size));

                                if (page == 0x01 || page == 0x0D)
                                    && field_usage == 0x30
                                    && layout.x_size == 0
                                {
                                    layout.x_start = field_bit.min(255) as u8;
                                    layout.x_size = report_size.min(32) as u8;
                                    layout.x_absolute = is_abs;
                                    layout.pointer = true;
                                    if is_abs {
                                        layout.x_max = if logical_max > 0 {
                                            logical_max as u32
                                        } else {
                                            max_for_bits(layout.x_size)
                                        };
                                    }
                                } else if (page == 0x01 || page == 0x0D)
                                    && field_usage == 0x31
                                    && layout.y_size == 0
                                {
                                    layout.y_start = field_bit.min(255) as u8;
                                    layout.y_size = report_size.min(32) as u8;
                                    layout.y_absolute = is_abs;
                                    layout.pointer = true;
                                    if is_abs {
                                        layout.y_max = if logical_max > 0 {
                                            logical_max as u32
                                        } else {
                                            max_for_bits(layout.y_size)
                                        };
                                    }
                                } else if page == 0x01
                                    && field_usage == 0x38
                                    && layout.wheel_size == 0
                                {
                                    layout.wheel_start = field_bit.min(255) as u8;
                                    layout.wheel_size = report_size.min(32) as u8;
                                } else if page == 0x0C
                                    && field_usage == 0x0238
                                    && layout.pan_size == 0
                                {
                                    layout.pan_start = field_bit.min(255) as u8;
                                    layout.pan_size = report_size.min(32) as u8;
                                }
                            }
                        }
                    }
                    bit = bit.saturating_add(bits);
                }
                0x09 => {
                    // Output öğesi (LED'ler). Main item tag 0x9 = Output.
                    let is_data = value & 0x01 == 0;
                    let is_var = value & 0x02 != 0;
                    let bits = report_count.saturating_mul(report_size);
                    if page == 0x08 && is_data && is_var && !layout.led_output && out_bit / 8 < 256 {
                        layout.led_output = true;
                        layout.led_offset = (out_bit / 8).min(255) as u8;
                        layout.led_report_id = report_id;
                    }
                    out_bit = out_bit.saturating_add(bits);
                }
                0x0A => {} // Feature öğesi: bu sürücü için kullanılmıyor.
                0x0B => {
                    if value == 1 && depth == 0 {
                        app_page = page;
                        app_usage = usage;
                    }
                    depth = depth.saturating_add(1);
                }
                0x0C => depth = depth.saturating_sub(1),
                _ => {}
            },
            _ => {}
        }

        // Local kullanım değerleri her Main öğesinden sonra sıfırlanır.
        if item_type == 0x00 {
            usage = 0;
            usage_count = 0;
            usage_min = 0;
            usage_max = 0;
        }
    }

    // Top-level uygulama kullanımı klavye/fare sınıfını netleştirir.
    if app_page == 0x01 && app_usage == 0x06 {
        layout.keyboard = true;
    }
    if app_page == 0x01 && app_usage == 0x02 {
        layout.pointer = true;
    }
    if app_page == 0x0D {
        layout.pointer = true;
    }
    if key_found {
        layout.keyboard = true;
    }
    layout.report_id = report_id;

    // Mutlak eksenlerde mantıksal maksimum eksikse bit genişliğinden türet.
    if layout.x_absolute && layout.x_max == 0 {
        layout.x_max = max_for_bits(layout.x_size);
    }
    if layout.y_absolute && layout.y_max == 0 {
        layout.y_max = max_for_bits(layout.y_size);
    }
    if layout.pointer && layout.buttons == 0 {
        layout.buttons = 3;
    }
    layout
}

/// Boot olmayan (özel düzenli) fare raporunu çıkarılan bit yerleşimine göre
/// MouseState'e uygular. Report ID içeren raporlarda ilk bayt atlanır.
fn handle_custom_mouse_report(
    data: &[u8],
    layout: &HidReportLayout,
    mouse: &mut MouseState,
    screen_w: i32,
    screen_h: i32,
) -> bool {
    let body = if layout.report_id != 0 {
        // Report ID taşıyan raporlarda ilk bayt kimliktir; eşleşmiyorsa bu
        // arayüzün ilgilendiğimiz raporu değildir.
        if data.is_empty() || data[0] != layout.report_id {
            return false;
        }
        &data[1..]
    } else {
        data
    };

    let x_end = layout.x_start as u32 + layout.x_size as u32;
    let y_end = layout.y_start as u32 + layout.y_size as u32;
    let wheel_end = layout.wheel_start as u32 + layout.wheel_size as u32;
    let button_end = layout.button_start as u32 + layout.buttons as u32;
    let bits_total = x_end.max(y_end).max(wheel_end).max(button_end);
    let bytes_needed = ((bits_total + 7) / 8) as usize;
    if body.len() < bytes_needed.max(3) {
        return false;
    }

    // Bit dizisinden alan çıkarma yardımcıları (LSB-first).
    let get_bits = |start: u8, bits: u8| -> u32 {
        let mut v = 0u32;
        for k in 0..bits as usize {
            let idx = start as usize + k;
            if idx / 8 >= body.len() {
                break;
            }
            let bitv = (body[idx / 8] >> (idx % 8)) & 1;
            v |= (bitv as u32) << k;
        }
        v
    };
    let sign_extend = |v: u32, bits: u8| -> i32 {
        if bits == 0 || bits >= 32 {
            return v as i32;
        }
        let shift = 32 - bits as u32;
        ((v << shift) as i32) >> shift
    };

    let buttons = if layout.buttons > 0 {
        get_bits(layout.button_start, layout.buttons.min(8)) as u8
    } else {
        0
    };

    if layout.x_absolute && layout.y_absolute {
        // Mutlak konum (dokunmatik yüzey / grafik tablet / QEMU usb-tablet).
        let x_max = if layout.x_max > 0 { layout.x_max } else { max_for_bits(layout.x_size) };
        let y_max = if layout.y_max > 0 { layout.y_max } else { max_for_bits(layout.y_size) };
        if x_max == 0 || y_max == 0 || screen_w <= 0 || screen_h <= 0 {
            return false;
        }
        let raw_x = get_bits(layout.x_start, layout.x_size);
        let raw_y = get_bits(layout.y_start, layout.y_size);
        // Yuvarlayarak ölçekle: 16383/32767 gibi orta noktalar tam merkeze düşsün.
        let scaled_x = (raw_x as i64 * screen_w as i64 + x_max as i64 / 2) / x_max as i64;
        let scaled_y = (raw_y as i64 * screen_h as i64 + y_max as i64 / 2) / y_max as i64;
        mouse.x = (scaled_x as i32).clamp(0, screen_w - 1);
        mouse.y = (scaled_y as i32).clamp(0, screen_h - 1);

        mouse.left_button = (buttons & crate::drivers::usb_mouse::MOUSE_BTN_LEFT) != 0;
        mouse.right_button = (buttons & crate::drivers::usb_mouse::MOUSE_BTN_RIGHT) != 0;
        mouse.middle_button = (buttons & crate::drivers::usb_mouse::MOUSE_BTN_MIDDLE) != 0;
    } else {
        // Göreli hareket; yüksek çözünürlüklü USB farelerdeki tam eksen verisi korunur.
        let raw_x = get_bits(layout.x_start, layout.x_size);
        let raw_y = get_bits(layout.y_start, layout.y_size);
        let dx = sign_extend(raw_x, layout.x_size).clamp(-2048, 2047);
        // HID'de pozitif Y aşağıdır; PS/2 katmanı pozitif Y yukarı bekler.
        let dy = (-sign_extend(raw_y, layout.y_size)).clamp(-2048, 2047);
        mouse.update_packet_i32(buttons, dx, dy, screen_w, screen_h);
    }

    if layout.wheel_size > 0 {
        let raw_w = get_bits(layout.wheel_start, layout.wheel_size);
        let wheel = sign_extend(raw_w, layout.wheel_size).clamp(-127, 127) as i8;
        if wheel != 0 {
            mouse.scroll_delta = mouse.scroll_delta.saturating_sub(wheel);
        }
    }

    true
}

/// Yapılandırma tanımlayıcısını tarayarak HID arayüzlerini bulur.
///
/// Öncelik: boot protokollü klavye > boot protokollü fare > boot olmayan fare
/// (rapor tanımlayıcısıyla) > boot olmayan klavye. Laptop yerleşik
/// dokunmatik yüzeyler ve modern fareler genellikle boot arayüzü sunmaz;
/// bu aygıtlar için rapor tanımlayıcısı okunur ve düzen çıkarılır.
/// Yapılandırma tanımlayıcısını tarayarak tüm HID arayüzlerini toplar.
///
/// Tek bir USB aygıtı (ör. kablosuz klavye+fare alıcısı veya multimedya
/// klavye) birden çok HID arayüzü sunabilir. Her arayüz aynı slot içinde kendi
/// kesme uç noktasına sahiptir; bu yüzden hepsi ayrı ayrı döndürülür.
fn parse_hid_interfaces(
    config: &[u8],
    out: &mut [HidCandidate; MAX_HID_INTERFACES],
) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    let mut current: Option<(u8, u8, u8, u8)> = None; // (iface, class, subclass, protocol)
    let mut endpoint_taken = false;

    while i + 2 <= config.len() {
        let len = config[i] as usize;
        let dtype = config[i + 1];
        if len < 2 || i + len > config.len() {
            break;
        }

        if dtype == 0x04 && len >= 9 {
            // Yalnızca varsayılan alternatif ayarı (bAlternateSetting = 0)
            // kullan; alternatif HID ayarlarındaki uç noktaları yapılandırma.
            if config[i + 3] == 0 {
                current = Some((config[i + 2], config[i + 5], config[i + 6], config[i + 7]));
            } else {
                current = None;
            }
            endpoint_taken = false;
        } else if dtype == 0x05 && len >= 7 {
            if let Some((interface, class, subclass, protocol)) = current {
                if class == 0x03 && !endpoint_taken {
                    let endpoint = config[i + 2];
                    let attributes = config[i + 3];
                    if endpoint & 0x80 != 0 && attributes & 0x03 == 0x03 && endpoint & 0x0F != 0 {
                        endpoint_taken = true;
                        if count < MAX_HID_INTERFACES {
                            let max_packet = ((config[i + 4] as usize)
                                | ((config[i + 5] as usize) << 8))
                                & 0x7FF;
                            // Protokol 0 ("none") yaygın olarak boot dışı
                            // klavye/farelerde görülür; sınıfı rapor
                            // tanımlayıcısı belirleyecek.
                            let kind = match protocol {
                                1 => Some(HidKind::Keyboard),
                                2 => Some(HidKind::Mouse),
                                _ => None,
                            };
                            let boot = subclass == 0x01 && (protocol == 1 || protocol == 2);
                            out[count] = HidCandidate {
                                interface,
                                kind,
                                endpoint,
                                max_packet: max_packet.max(1),
                                interval: config[i + 6],
                                boot,
                                custom: !boot,
                                layout: HidReportLayout::empty(),
                            };
                            count += 1;
                        }
                    }
                }
            }
        }

        i += len;
    }

    count
}

/// Aday arayüzün HID rapor tanımlayıcısını okur, alan yerleşimini çıkarır ve
/// sınıfı (klavye/fare) netleştirir.
///
/// Protokol alanı 1/2 olan arayüzler boot raporu kullanır; tanımlayıcı
/// okunamazsa boot yerleşimine düşülür. Protokol 0 arayüzlerde ise sınıf
/// yalnızca rapor tanımlayıcısından belirlenebilir.
unsafe fn load_report_layout(
    c: &mut XhciController,
    ep0_ring: &mut TrbRing,
    slot: u8,
    candidate: &mut HidCandidate,
    buffer_phys: u64,
    buffer: *mut u8,
) -> bool {
    let explicit_kind = candidate.kind;
    let mut layout = HidReportLayout::empty();

    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    if let Some(n) = c.control_in(
        ep0_ring,
        slot,
        0x81,
        0x06,
        0x2200,
        candidate.interface as u16,
        buffer_phys,
        PAGE as usize,
    ) {
        if n > 0 {
            let desc = core::slice::from_raw_parts(buffer, n.min(PAGE as usize));
            layout = parse_hid_report_descriptor(desc);
        }
    }

    match explicit_kind {
        Some(HidKind::Keyboard) => {
            if layout.keyboard_valid() {
                candidate.layout = layout;
            } else {
                // Boot protokolü (veya protocol=1) klavyesi: 8 baytlık boot raporu.
                candidate.layout = HidReportLayout::keyboard_boot();
                candidate.custom = !candidate.boot;
            }
            true
        }
        Some(HidKind::Mouse) => {
            if layout.pointer_valid() {
                candidate.layout = layout;
            } else {
                candidate.layout = HidReportLayout::mouse_boot();
                candidate.custom = !candidate.boot;
            }
            true
        }
        None => {
            if layout.keyboard_valid() {
                candidate.kind = Some(HidKind::Keyboard);
                candidate.layout = layout;
                candidate.custom = true;
                true
            } else if layout.pointer_valid() {
                candidate.kind = Some(HidKind::Mouse);
                candidate.layout = layout;
                candidate.custom = true;
                true
            } else {
                false
            }
        }
    }
}

/// HID kullanım kimliğini PS/2 set-1 tarama koduna çevirir.
/// Dönüş: (scancode, genişletilmiş mi?)
// ---------------------------------------------------------------------------
// Aygıt numaralandırma
// ---------------------------------------------------------------------------

unsafe fn initial_ep0_packet(speed: u8) -> u16 {
    match speed {
        SPEED_LOW => 8,
        SPEED_FULL => 8,
        SPEED_HIGH => 64,
        _ => 512,
    }
}

/// HID `bInterval` değerini xHCI uç nokta bağlamındaki `Interval` alanına çevirir.
///
/// Tam/az hızlı aygıtlarda alan doğrudan çerçeve (ms) sayısıdır. Yüksek ve
/// süper hızlı aygıtlarda ise 2^(n-1) mikroçerçeve ifade eden üs değeridir;
/// her iki durumda da `bInterval` değeri olduğu gibi kullanılır (1..=16).
fn report_interval(_speed: u8, interval: u8) -> u32 {
    (interval.max(1) as u32).min(16)
}

unsafe fn find_free_hid_slot() -> Option<usize> {
    HID_DEVICES.iter().position(|d| !d.active)
}

unsafe fn free_frames(pmm: &mut PhysicalMemoryManager, base: u64, pages: usize) {
    for i in 0..pages {
        pmm.free_frame(base + (i as u64) * PAGE);
    }
}

unsafe fn mark_port_disconnected(
    controller: usize,
    port: u8,
    pmm: &mut PhysicalMemoryManager,
) {
    let idx = (port - 1) as usize;
    let c = &mut CONTROLLERS[controller];
    let slot = c.port_slot[idx];
    let base = c.port_base_dma[idx];
    c.port_slot[idx] = 0;
    c.port_connected[idx] = false;
    c.port_base_dma[idx] = 0;

    // Slotu önce devre dışı bırak: hâlâ aygıt bağlamına erişiyor olabilir.
    if slot != 0 && slot != 0xFF {
        c.disable_slot(slot);
    }

    for device in HID_DEVICES.iter_mut() {
        if device.active && device.controller == controller as u8 && device.slot == slot {
            device.active = false;
            if device.dma_phys != 0 {
                free_frames(pmm, device.dma_phys, 2);
                device.dma_phys = 0;
            }
            crate::serial::print("[USB-HID] Aygıt çıkarıldı (port ");
            crate::serial::print_dec(port as u64);
            crate::serial::print(").\n");
        }
    }

    if base != 0 {
        free_frames(pmm, base, 4);
    }
}

/// Numaralandırma başarısız olduğunda slotu kapatır ve ayrılan DMA sayfalarını
/// (aygıt başına 4 sayfa + arayüz başına 2 sayfa) geri verir.
unsafe fn abort_slot(
    c: &mut XhciController,
    pmm: &mut PhysicalMemoryManager,
    port: u8,
    slot: u8,
    base: u64,
    blocks: &[u64; MAX_HID_INTERFACES],
    block_count: usize,
    is_root: bool,
) {
    c.disable_slot(slot);
    if is_root && port > 0 {
        let idx = (port - 1) as usize;
        if idx < MAX_PORTS {
            c.port_slot[idx] = 0xFF;
            c.port_connected[idx] = false;
            c.port_base_dma[idx] = 0;
        }
    }
    for i in 0..block_count {
        if blocks[i] != 0 {
            free_frames(pmm, blocks[i], 2);
        }
    }
    free_frames(pmm, base, 4);
}

/// Bir USB aygıtını numaralandırır (kök portta veya bir USB hub arkasında).
unsafe fn enumerate_device(
    controller: usize,
    root_port: u8,
    speed: u8,
    route_string: u32,
    tt_hub_slot: u8,
    tt_port: u8,
    pmm: &mut PhysicalMemoryManager,
) {
    let c = &mut CONTROLLERS[controller];
    let is_root = route_string == 0;
    let idx = (root_port - 1) as usize;

    let Some(slot) = c.enable_slot() else {
        crate::serial::print("[USB-HID] Slot ayrılamadı.\n");
        if is_root && idx < MAX_PORTS {
            c.port_slot[idx] = 0xFF;
        }
        return;
    };

    let Some(base) = pmm.allocate_contiguous_frames(4) else {
        crate::serial::print("[USB-HID] DMA belleği ayrılamadı.\n");
        c.disable_slot(slot);
        if is_root && idx < MAX_PORTS {
            c.port_slot[idx] = 0xFF;
        }
        return;
    };
    let input_phys = base;
    let device_phys = base + PAGE;
    let ep0_phys = base + 2 * PAGE;
    let buffer_phys = base + 3 * PAGE;
    for page in 0..4 {
        zero_page(base + page * PAGE);
    }

    let mut blocks = [0u64; MAX_HID_INTERFACES];
    let mut block_count = 0usize;

    let dcbaa = crate::drivers::phys_to_virt(c.dcbaa_phys) as *mut u64;
    core::ptr::write_volatile(dcbaa.add(slot as usize), device_phys);

    let mut ep0_ring = TrbRing::new(ep0_phys);
    let max_packet0 = initial_ep0_packet(speed);
    c.build_address_context(
        input_phys,
        speed,
        root_port,
        max_packet0,
        &ep0_ring,
        route_string,
        tt_hub_slot,
        tt_port,
    );

    if !c.address_device(slot, input_phys) {
        crate::serial::print("[USB-HID] Address Device başarısız.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    let buffer = crate::drivers::phys_to_virt(buffer_phys) as *mut u8;

    // İlk 8 baytlık aygıt tanımlayıcısı
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0100, 0, buffer_phys, 8) else {
        crate::serial::print("[USB-HID] Aygıt tanımlayıcısı okunamadı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    };
    if n < 8 {
        crate::serial::print("[USB-HID] Kısa aygıt tanımlayıcısı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    let reported_mps = core::ptr::read_volatile(buffer.add(7)) as u16;
    let ep0_mps = if matches!(reported_mps, 8 | 16 | 32 | 64 | 512) {
        reported_mps
    } else {
        max_packet0
    };
    if ep0_mps != max_packet0 {
        if !c.update_ep0_max_packet(
            slot,
            input_phys,
            speed,
            root_port,
            ep0_mps,
            &ep0_ring,
            route_string,
            tt_hub_slot,
            tt_port,
        ) {
            crate::serial::print("[USB-HID] EP0 max packet güncellenemedi.\n");
            abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
            return;
        }
    }

    // Tam aygıt tanımlayıcısı
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0100, 0, buffer_phys, 18) else {
        crate::serial::print("[USB-HID] Tam aygıt tanımlayıcısı okunamadı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    };
    if n < 18 {
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }
    let vendor = core::ptr::read_volatile(buffer.add(8)) as u16
        | ((core::ptr::read_volatile(buffer.add(9)) as u16) << 8);
    let product = core::ptr::read_volatile(buffer.add(10)) as u16
        | ((core::ptr::read_volatile(buffer.add(11)) as u16) << 8);
    let device_class = core::ptr::read_volatile(buffer.add(4));
    let device_protocol = core::ptr::read_volatile(buffer.add(6));
    crate::serial::print("[USB-HID] Aygıt VID:PID = ");
    crate::serial::print_hex(vendor as u64);
    crate::serial::print(":");
    crate::serial::print_hex(product as u64);
    crate::serial::print(" sınıf=");
    crate::serial::print_hex(device_class as u64);
    crate::serial::print("\n");

    // USB Hub algılandı! Hub arkasındaki aygıtları yapılandır ve tara.
    if device_class == 0x09 {
        crate::serial::print("[USB-HID] USB hub algılandı; hub portları başlatılıyor...\n");
        enumerate_hub(
            controller,
            root_port,
            speed,
            slot,
            base,
            buffer_phys,
            buffer,
            &mut ep0_ring,
            route_string,
            device_protocol,
            pmm,
        );
        return;
    }

    // Yapılandırma tanımlayıcısının başlığı
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0200, 0, buffer_phys, 9) else {
        crate::serial::print("[USB-HID] Yapılandırma başlığı okunamadı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    };
    if n < 9 {
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }
    let total = core::ptr::read_volatile(buffer.add(2)) as usize
        | ((core::ptr::read_volatile(buffer.add(3)) as usize) << 8);
    let config_value = core::ptr::read_volatile(buffer.add(5));
    if total < 9 || total > PAGE as usize {
        crate::serial::print("[USB-HID] Yapılandırma boyutu geçersiz.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0200, 0, buffer_phys, total) else {
        crate::serial::print("[USB-HID] Yapılandırma tanımlayıcısı okunamadı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    };
    if n < 9 {
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    let config_slice = core::slice::from_raw_parts(buffer, n.min(PAGE as usize));
    let mut candidates = [HidCandidate::empty(); MAX_HID_INTERFACES];
    let found = parse_hid_interfaces(config_slice, &mut candidates);
    if found == 0 {
        crate::serial::print("[USB-HID] HID arayüzü bulunamadı; aygıt atlandı.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    let mut usable = [HidCandidate::empty(); MAX_HID_INTERFACES];
    let mut usable_count = 0usize;
    for i in 0..found {
        if load_report_layout(
            c,
            &mut ep0_ring,
            slot,
            &mut candidates[i],
            buffer_phys,
            buffer,
        ) {
            usable[usable_count] = candidates[i];
            usable_count += 1;
        } else {
            crate::serial::print("[USB-HID] Arayüz ");
            crate::serial::print_dec(candidates[i].interface as u64);
            crate::serial::print(" HID klavye/fare değil; atlandı.\n");
        }
    }
    if usable_count == 0 {
        crate::serial::print("[USB-HID] Kullanılabilir HID klavye/fare arayüzü yok.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    let free_slots = HID_DEVICES.iter().filter(|d| !d.active).count();
    let count = usable_count.min(free_slots).min(MAX_HID_INTERFACES);
    if count == 0 {
        crate::serial::print("[USB-HID] Boş HID aygıt yuvası yok.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    // Yapılandırmayı seç
    if c.control_out(&mut ep0_ring, slot, 0x00, 0x09, config_value as u16, 0, 0, 0).is_none() {
        crate::serial::print("[USB-HID] SET_CONFIGURATION başarısız.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    for i in 0..count {
        if usable[i].boot {
            if c.control_out(
                &mut ep0_ring,
                slot,
                0x21,
                0x0B,
                0,
                usable[i].interface as u16,
                0,
                0,
            )
            .is_none()
            {
                usable[i].custom = true;
            } else {
                usable[i].custom = false;
            }
        } else {
            usable[i].custom = true;
        }
    }

    for i in 0..count {
        match pmm.allocate_contiguous_frames(2) {
            Some(block) => {
                zero_page(block);
                zero_page(block + PAGE);
                blocks[i] = block;
                block_count = i + 1;
            }
            None => {
                crate::serial::print("[USB-HID] Arayüz DMA belleği ayrılamadı.\n");
                abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
                return;
            }
        }
    }

    zero_page(input_phys);
    let mut ctx_flags = 1u32;
    let mut max_dci = 0u8;
    for i in 0..count {
        let dci = ((usable[i].endpoint & 0x0F) * 2 + 1) as u8;
        ctx_flags |= 1 << dci;
        if dci > max_dci {
            max_dci = dci;
        }
    }
    c.ctx_write(input_phys, 0, 1, ctx_flags);
    c.ctx_write(
        input_phys,
        1,
        0,
        (route_string & 0xFFFFF) | ((speed as u32) << 20) | (((max_dci as u32) & 0x1F) << 27),
    );
    c.ctx_write(input_phys, 1, 1, (root_port as u32) << 16);
    if tt_hub_slot != 0 {
        c.ctx_write(input_phys, 1, 2, (tt_hub_slot as u32) | ((tt_port as u32) << 8));
    }
    for i in 0..count {
        let dci = ((usable[i].endpoint & 0x0F) * 2 + 1) as u8;
        let ep_index = dci as usize + 1;
        let ring_phys = blocks[i];
        c.ctx_write(
            input_phys,
            ep_index,
            0,
            report_interval(speed, usable[i].interval) << 16,
        );
        c.ctx_write(
            input_phys,
            ep_index,
            1,
            (3 << 1) | (7 << 3) | ((usable[i].max_packet as u32) << 16),
        );
        c.ctx_write(input_phys, ep_index, 2, (ring_phys as u32) | 1);
        c.ctx_write(input_phys, ep_index, 3, (ring_phys >> 32) as u32);
    }

    if !c.configure_endpoint(slot, input_phys) {
        crate::serial::print("[USB-HID] Configure Endpoint başarısız.\n");
        abort_slot(c, pmm, root_port, slot, base, &blocks, block_count, is_root);
        return;
    }

    if is_root && idx < MAX_PORTS {
        c.port_base_dma[idx] = base;
        c.port_slot[idx] = slot;
        c.port_connected[idx] = true;
    }

    let mut led_owner = false;
    for i in 0..count {
        let Some(index) = find_free_hid_slot() else { break; };
        let kind = usable[i].kind.unwrap_or(HidKind::Keyboard);
        let ring_phys = blocks[i];
        let report_phys = ring_phys + PAGE;
        let dci = ((usable[i].endpoint & 0x0F) * 2 + 1) as u8;
        let report_len = usable[i].max_packet.clamp(1, 1024);
        let custom = usable[i].custom;
        let mut device = HidDevice {
            active: true,
            controller: controller as u8,
            slot,
            port: root_port,
            kind,
            dci,
            interface: usable[i].interface,
            ep_ring: TrbRing::new(ring_phys),
            report_phys,
            report_len,
            prev_modifiers: 0,
            prev_keys: [0; 6],
            custom,
            layout: usable[i].layout,
            samples: 0,
            dma_phys: ring_phys,
            ep0_ring: TrbRing::empty(),
            ctrl_phys: 0,
            led_capable: false,
            led_report_id: 0,
            leds: 0xFF,
        };

        if kind == HidKind::Keyboard
            && !led_owner
            && (usable[i].boot
                || (usable[i].layout.led_output && usable[i].layout.led_offset == 0))
        {
            device.led_capable = true;
            device.ep0_ring = ep0_ring;
            device.ctrl_phys = buffer_phys;
            device.led_report_id = if custom { usable[i].layout.led_report_id } else { 0 };
            led_owner = true;
        }

        HID_DEVICES[index] = device;
        queue_hid_transfer(&mut HID_DEVICES[index]);

        crate::serial::print("[USB-HID] HID ");
        crate::serial::print(kind.label());
        crate::serial::print(" hazır (arayüz ");
        crate::serial::print_dec(usable[i].interface as u64);
        crate::serial::print(", slot ");
        crate::serial::print_dec(slot as u64);
        crate::serial::print(", rapor ");
        crate::serial::print_dec(report_len as u64);
        crate::serial::print(" bayt");
        if custom {
            crate::serial::print(", özel düzen (düğmeler=");
            crate::serial::print_dec(usable[i].layout.buttons as u64);
            crate::serial::print(")");
        }
        crate::serial::print(").\n");
    }
}

/// USB Hub sürücüsü: Hub'ı başlatır, portlarına güç verir ve arkasındaki cihazları numaralandırır.
unsafe fn enumerate_hub(
    controller: usize,
    root_port: u8,
    hub_speed: u8,
    hub_slot: u8,
    base: u64,
    buffer_phys: u64,
    buffer: *mut u8,
    ep0_ring: &mut TrbRing,
    parent_route_string: u32,
    hub_protocol: u8,
    pmm: &mut PhysicalMemoryManager,
) {
    let c = &mut CONTROLLERS[controller];
    let is_root = parent_route_string == 0;
    let idx = (root_port - 1) as usize;

    // 1. Hub Yapılandırma tanımlayıcısını oku ve yapılandırmayı seç
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(ep0_ring, hub_slot, 0x80, 0x06, 0x0200, 0, buffer_phys, 9) else {
        crate::serial::print("[USB-HUB] Hub yapılandırma başlığı okunamadı.\n");
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    };
    if n < 9 {
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    }
    let config_val = core::ptr::read_volatile(buffer.add(5));

    if c.control_out(ep0_ring, hub_slot, 0x00, 0x09, config_val as u16, 0, 0, 0).is_none() {
        crate::serial::print("[USB-HUB] Hub SET_CONFIGURATION başarısız.\n");
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    }

    // 2. Hub Tanımlayıcısını oku (GET_DESCRIPTOR: Type 0x29)
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(ep0_ring, hub_slot, 0xA0, 0x06, 0x2900, 0, buffer_phys, 8) else {
        crate::serial::print("[USB-HUB] Hub tanımlayıcısı okunamadı.\n");
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    };
    if n < 3 {
        crate::serial::print("[USB-HUB] Kısa hub tanımlayıcısı.\n");
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    }
    let num_ports = core::ptr::read_volatile(buffer.add(2));
    crate::serial::print("[USB-HUB] USB Hub port sayısı = ");
    crate::serial::print_dec(num_ports as u64);
    crate::serial::print("\n");

    // 3. xHCI Slot Context içinde Hub=1 ve Number of Ports ayarla
    let input_phys = base;
    zero_page(input_phys);
    c.ctx_write(input_phys, 0, 1, 1); // Add A0 (Slot Context)
    let mtt = if hub_protocol == 2 { 1 << 25 } else { 0 };
    c.ctx_write(
        input_phys,
        1,
        0,
        (parent_route_string & 0xFFFFF) | ((hub_speed as u32) << 20) | mtt | (1 << 26) | (1 << 27),
    );
    c.ctx_write(
        input_phys,
        1,
        1,
        ((root_port as u32) << 16) | ((num_ports as u32) << 24),
    );
    if !c.configure_endpoint(hub_slot, input_phys) {
        crate::serial::print("[USB-HUB] Hub Configure Endpoint başarısız.\n");
        c.disable_slot(hub_slot);
        free_frames(pmm, base, 4);
        return;
    }

    if is_root && idx < MAX_PORTS {
        c.port_base_dma[idx] = base;
        c.port_slot[idx] = hub_slot;
        c.port_connected[idx] = true;
    }

    // 4. Her bir hub portuna güç ver (PORT_POWER = 8, SET_FEATURE = 0x03)
    for p in 1..=num_ports {
        c.control_out(ep0_ring, hub_slot, 0x23, 0x03, 8, p as u16, 0, 0);
    }

    // Port gücünün oturması için 50 ms bekle
    let start_pwr = crate::boot::time::monotonic_ms();
    while crate::boot::time::monotonic_ms().wrapping_sub(start_pwr) < 50 {
        core::hint::spin_loop();
    }

    // 5. Hub portlarını tara ve bağlı aygıtları numaralandır
    for p in 1..=num_ports.min(15) {
        core::ptr::write_bytes(buffer, 0, PAGE as usize);
        let Some(len) = c.control_in(ep0_ring, hub_slot, 0xA3, 0x00, 0, p as u16, buffer_phys, 4) else {
            continue;
        };
        if len < 4 {
            continue;
        }
        let port_status = core::ptr::read_volatile(buffer) as u16
            | ((core::ptr::read_volatile(buffer.add(1)) as u16) << 8);

        // Bit 0 = PORT_CONNECTION
        if port_status & 1 == 0 {
            continue;
        }

        crate::serial::print("[USB-HUB] Hub port ");
        crate::serial::print_dec(p as u64);
        crate::serial::print(" üzerinde aygıt algılandı, port sıfırlanıyor...\n");

        // Portu sıfırla (PORT_RESET = 4)
        c.control_out(ep0_ring, hub_slot, 0x23, 0x03, 4, p as u16, 0, 0);
        let start_rst = crate::boot::time::monotonic_ms();
        while crate::boot::time::monotonic_ms().wrapping_sub(start_rst) < 30 {
            core::hint::spin_loop();
        }

        // Port durumunu tekrar oku
        core::ptr::write_bytes(buffer, 0, PAGE as usize);
        let Some(len) = c.control_in(ep0_ring, hub_slot, 0xA3, 0x00, 0, p as u16, buffer_phys, 4) else {
            continue;
        };
        if len < 4 {
            continue;
        }
        let post_reset_status = core::ptr::read_volatile(buffer) as u16
            | ((core::ptr::read_volatile(buffer.add(1)) as u16) << 8);

        // Hız tespiti: bit 9 = LOW_SPEED, bit 10 = HIGH_SPEED, aksi takdirde FULL_SPEED
        let dev_speed = if post_reset_status & (1 << 9) != 0 {
            SPEED_LOW // 1
        } else if post_reset_status & (1 << 10) != 0 {
            SPEED_HIGH // 3
        } else {
            SPEED_FULL // 2
        };

        crate::serial::print("[USB-HUB] Hub port ");
        crate::serial::print_dec(p as u64);
        crate::serial::print(" aygıt hızı=");
        crate::serial::print_dec(dev_speed as u64);
        crate::serial::print("\n");

        // xHCI Route String hesaplama (4 bitlik katmanlı rota):
        let mut route_str = parent_route_string;
        let mut shift = 0;
        while shift < 20 && (route_str & (0xF << shift)) != 0 {
            shift += 4;
        }
        if shift < 20 {
            route_str |= ((p as u32) & 0xF) << shift;
        }

        // Transaction Translator (TT) kuralları:
        // Eğer hub High-Speed ise ve arkasındaki aygıt Low veya Full speed ise TT kullanılır.
        let (tt_slot, tt_p) = if hub_speed == SPEED_HIGH && (dev_speed == SPEED_LOW || dev_speed == SPEED_FULL) {
            (hub_slot, p)
        } else {
            (0, 0)
        };

        enumerate_device(
            controller,
            root_port,
            dev_speed,
            route_str,
            tt_slot,
            tt_p,
            pmm,
        );
    }
}

unsafe fn enumerate_port(controller: usize, port: u8, pmm: &mut PhysicalMemoryManager) {
    let c = &mut CONTROLLERS[controller];
    let idx = (port - 1) as usize;
    crate::serial::print("[USB-HID] Port ");
    crate::serial::print_dec(port as u64);
    crate::serial::print(" taranıyor...\n");

    let Some(speed) = c.reset_port(port) else {
        crate::serial::print("[USB-HID] Port reset başarısız.\n");
        c.port_slot[idx] = 0xFF;
        return;
    };
    crate::serial::print("[USB-HID] Port ");
    crate::serial::print_dec(port as u64);
    crate::serial::print(" hız kodu=");
    crate::serial::print_dec(speed as u64);
    crate::serial::print("\n");

    enumerate_device(controller, port, speed, 0, 0, 0, pmm);
}
unsafe fn queue_hid_transfer(device: &mut HidDevice) {
    let c = &mut CONTROLLERS[device.controller as usize];
    let trb = Trb {
        param: device.report_phys,
        status: device.report_len as u32,
        control: (TRB_NORMAL << TRB_TYPE_SHIFT)
            | TRB_IOC
            | TRB_ISP
            | ((device.slot as u32) << 24),
    };
    device.ep_ring.push(trb);
    c.doorbell(device.slot, device.dci);
}

unsafe fn handle_transfer_event(
    controller: usize,
    event: Trb,
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    let slot = trb_slot(event.control);
    let dci = trb_endpoint(event.control);
    let code = trb_completion(event.status);
    let residual = trb_residual(event.status);
    let mut updated = InputUpdate::default();

    for device in HID_DEVICES.iter_mut() {
        if !device.active
            || device.controller != controller as u8
            || device.slot != slot
            || device.dci != dci
        {
            continue;
        }

        if code == COMP_SUCCESS || code == COMP_SHORT_PACKET {
            let actual = device.report_len.saturating_sub(residual);
            if actual > 0 {
                let data = core::slice::from_raw_parts(
                    crate::drivers::phys_to_virt(device.report_phys) as *const u8,
                    actual,
                );
                match device.kind {
                    HidKind::Keyboard => {
                        let mut kbd_dev = crate::drivers::usb_keyboard::UsbKeyboardDevice {
                            active: device.active,
                            prev_modifiers: device.prev_modifiers,
                            prev_keys: device.prev_keys,
                            leds: 0,
                        };
                        let ok = if device.custom {
                            // Boot protokolü kullanılmıyorsa report ID ve alan
                            // konumları rapor tanımlayıcısından gelir.
                            let layout = crate::drivers::usb_keyboard::KeyboardReportLayout {
                                report_id: device.layout.report_id,
                                modifier_offset: device.layout.modifier_offset,
                                key_offset: device.layout.key_offset,
                                key_count: device.layout.key_count,
                            };
                            crate::drivers::usb_keyboard::process_keyboard_report_layout(
                                &mut kbd_dev,
                                data,
                                &layout,
                                keyboard,
                            )
                        } else {
                            crate::drivers::usb_keyboard::process_keyboard_report(
                                &mut kbd_dev,
                                data,
                                keyboard,
                            )
                        };
                        if ok {
                            device.prev_modifiers = kbd_dev.prev_modifiers;
                            device.prev_keys = kbd_dev.prev_keys;
                            updated.keyboard = true;
                        }
                        // Tanılama: ilk 8 klavye raporunun hex dökümü.
                        if device.samples < 8 {
                            device.samples += 1;
                            crate::serial::print("[USB-HID] klavye raporu: ");
                            for b in data.iter().take(8) {
                                crate::serial::print_hex(*b as u64);
                                crate::serial::print(" ");
                            }
                            crate::serial::print("\n");
                        }
                    }
                    HidKind::Mouse => {
                        // Mutlak/göreli karar rapor uzunluğuna göre değil,
                        // rapor tanımlayıcısındaki Input öğesinin Absolute
                        // bitine göre verilir. Boot protokolünde fare raporu
                        // her zaman görelidir.
                        let ok = if device.custom {
                            handle_custom_mouse_report(
                                data,
                                &device.layout,
                                mouse,
                                screen_w,
                                screen_h,
                            )
                        } else {
                            let mut mouse_dev = crate::drivers::usb_mouse::UsbMouseDevice {
                                active: device.active,
                                mode: crate::drivers::usb_mouse::UsbPointerMode::Relative,
                                last_buttons: 0,
                            };
                            crate::drivers::usb_mouse::process_mouse_report(
                                &mut mouse_dev,
                                data,
                                mouse,
                                screen_w,
                                screen_h,
                            )
                        };
                        if ok {
                            updated.mouse = true;
                        }
                        // Tanılama: ilk 8 fare raporunun hex dökümü.
                        if device.samples < 8 {
                            device.samples += 1;
                            crate::serial::print("[USB-HID] fare raporu: ");
                            for b in data.iter().take(8) {
                                crate::serial::print_hex(*b as u64);
                                crate::serial::print(" ");
                            }
                            crate::serial::print("\n");
                        }
                    }
                }
            }
            queue_hid_transfer(device);
        } else {
            // Hatalı transfer: uç noktayı sıfırlayıp yeniden dene.
            crate::serial::print("[USB-HID] Transfer hatası (kod ");
            crate::serial::print_dec(code as u64);
            crate::serial::print("), uç nokta kurtarılıyor.\n");
            let c = &mut CONTROLLERS[controller];
            let ring_dequeue = device.ep_ring.dequeue();
            let _ = c.reset_endpoint(slot, dci);
            if c.set_tr_dequeue(
                slot,
                dci,
                (ring_dequeue.0 & !0xF) | ((ring_dequeue.1 & 1) as u64),
            ) {
                queue_hid_transfer(device);
            } else {
                device.active = false;
            }
        }
        break;
    }

    updated
}

/// Klavye LED'lerini (Caps Lock / Num Lock) aygıtla eşitler.
///
/// HID LED çıktı raporu SET_REPORT (Output) kontrol aktarımıyla gönderilir.
/// Yalnızca durum değiştiğinde çağrılır; böylece her karede kontrol aktarımı
/// yapılmaz. EP0 halkası slot genelinde ortak olduğu için yalnızca slotun ilk
/// klavyesi bu işi üstlenir (`led_capable`).
unsafe fn sync_keyboard_leds(keyboard: &KeyboardState) {
    let desired = crate::drivers::usb_keyboard::led_value(
        keyboard.caps_lock,
        keyboard.num_lock,
        false,
    );

    for i in 0..HID_DEVICES.len() {
        if !HID_DEVICES[i].active
            || !HID_DEVICES[i].led_capable
            || HID_DEVICES[i].leds == desired
        {
            continue;
        }

        let ci = HID_DEVICES[i].controller as usize;
        let slot = HID_DEVICES[i].slot;
        let interface = HID_DEVICES[i].interface;
        let ctrl_phys = HID_DEVICES[i].ctrl_phys;
        let report_id = HID_DEVICES[i].led_report_id;
        if ci >= CONTROLLER_COUNT || ctrl_phys == 0 {
            HID_DEVICES[i].led_capable = false;
            continue;
        }

        let buffer = crate::drivers::phys_to_virt(ctrl_phys) as *mut u8;
        let length = if report_id != 0 {
            core::ptr::write_volatile(buffer, report_id);
            core::ptr::write_volatile(buffer.add(1), desired);
            2
        } else {
            core::ptr::write_volatile(buffer, desired);
            1
        };

        let mut ring = HID_DEVICES[i].ep0_ring;
        let c = &mut CONTROLLERS[ci];
        let ok = c
            .control_out(
                &mut ring,
                slot,
                0x21,
                0x09,
                0x0200,
                interface as u16,
                ctrl_phys,
                length,
            )
            .is_some();
        HID_DEVICES[i].ep0_ring = ring;

        if ok {
            HID_DEVICES[i].leds = desired;
            crate::serial::print("[USB-HID] Klavye LED güncellendi: ");
            crate::serial::print_hex(desired as u64);
            crate::serial::print("\n");
        } else {
            // Aygıt SET_REPORT'u reddetti; her karede yeniden denemeyi bırak.
            HID_DEVICES[i].led_capable = false;
            crate::serial::print(
                "[USB-HID] Klavye LED raporu gönderilemedi; LED eşitleme kapatıldı.\n",
            );
        }
    }
}

unsafe fn dispatch_event(
    controller: usize,
    event: Trb,
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    match trb_type(event.control) {
        TRB_TRANSFER_EVENT => {
            handle_transfer_event(controller, event, mouse, keyboard, screen_w, screen_h)
        }
        _ => InputUpdate::default(),
    }
}

// ---------------------------------------------------------------------------
// Başlatma ve yoklama
// ---------------------------------------------------------------------------

unsafe fn probe_controller(bus: u8, dev: u8, func: u8, pmm: &mut PhysicalMemoryManager) {
    let bar0 = pci_read32(bus, dev, func, 0x10);
    if bar0 & 1 != 0 {
        return; // xHCI her zaman MMIO kullanır.
    }
    let bar1 = pci_read32(bus, dev, func, 0x14);
    let is_64 = (bar0 & 0x06) == 0x04;
    let base_phys = if is_64 {
        ((bar0 & 0xFFFF_FFF0) as u64) | ((bar1 as u64) << 32)
    } else {
        (bar0 & 0xFFFF_FFF0) as u64
    };
    if base_phys == 0 {
        return;
    }

    crate::arch::x86_64::mmio_map_2mb(base_phys & !0x1F_FFFF);
    if (base_phys & 0x1F_FFFF) + 0x1_0000 > 0x20_0000 {
        crate::arch::x86_64::mmio_map_2mb((base_phys & !0x1F_FFFF) + 0x20_0000);
    }

    let cmd = pci_read32(bus, dev, func, 0x04) & 0xFFFF;
    pci_write32(bus, dev, func, 0x04, cmd | 0x0406); // MMIO + Bus Master, INTx kapalı

    let mut controller = XhciController::empty();
    controller.index = CONTROLLER_COUNT as u8;
    controller.mmio = crate::drivers::phys_to_virt(base_phys);
    controller.bus = bus;
    controller.dev = dev;
    controller.func = func;

    crate::serial::print("[USB-HID] xHCI adayı PCI ");
    crate::serial::print_dec(bus as u64);
    crate::serial::print(":");
    crate::serial::print_dec(dev as u64);
    crate::serial::print(".");
    crate::serial::print_dec(func as u64);
    crate::serial::print(" MMIO ");
    crate::serial::print_hex(base_phys);
    crate::serial::print("\n");

    if controller.init_hardware(pmm) {
        controller.legacy_handoff();
        CONTROLLERS[CONTROLLER_COUNT] = controller;
        CONTROLLER_COUNT += 1;
        crate::serial::print("[USB-HID] xHCI hazır.\n");
    } else {
        crate::serial::print("[USB-HID] xHCI başlatılamadı; aygıt atlandı.\n");
    }
}

pub fn init(pmm: &mut PhysicalMemoryManager) {
    unsafe {
        if INITIALIZED {
            return;
        }
        INITIALIZED = true;
        crate::serial::print("[USB-HID] xHCI USB klavye/fare taraması başlıyor...\n");

        for bus in 0..=255u8 {
            for dev in 0..32u8 {
                let id0 = pci_read32(bus, dev, 0, 0x00);
                if id0 & 0xFFFF == 0xFFFF || id0 & 0xFFFF == 0 {
                    continue;
                }
                let header = pci_read32(bus, dev, 0, 0x0C);
                let functions = if ((header >> 16) as u8) & 0x80 != 0 { 8 } else { 1 };
                for func in 0..functions {
                    if CONTROLLER_COUNT >= MAX_CONTROLLERS {
                        break;
                    }
                    let id = pci_read32(bus, dev, func, 0x00);
                    if id & 0xFFFF == 0xFFFF || id & 0xFFFF == 0 {
                        continue;
                    }
                    let class_rev = pci_read32(bus, dev, func, 0x08);
                    let class = ((class_rev >> 24) & 0xFF) as u8;
                    let subclass = ((class_rev >> 16) & 0xFF) as u8;
                    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
                    if class == 0x0C && subclass == 0x03 && prog_if == 0x30 {
                        probe_controller(bus, dev, func, pmm);
                    }
                }
            }
        }

        if CONTROLLER_COUNT == 0 {
            crate::serial::print(
                "[USB-HID] Yerel xHCI denetleyicisi bulunamadı; PS/2 fallback kullanılacak (BIOS Legacy USB/CSM açık olmalıdır).\n",
            );
            return;
        }

        for ci in 0..CONTROLLER_COUNT {
            let c = &mut CONTROLLERS[ci];
            if !c.present {
                continue;
            }
            // Port gücünü aç (PPC destekliyorsa).
            let hccparams1 = c.rd32(c.mmio + 0x10);
            if hccparams1 & (1 << 3) != 0 {
                for port in 1..=c.max_ports.min(MAX_PORTS as u8) {
                    let off = c.op + XhciController::port_offset(port);
                    let status = c.rd32(off);
                    c.wr32(off, (status & PORTSC_KEEP) | PORTSC_PP);
                }
                let start = crate::boot::time::monotonic_ms();
                while crate::boot::time::monotonic_ms().wrapping_sub(start) < 20 {
                    core::hint::spin_loop();
                }
            }
            scan_ports(ci, pmm, true);
        }

        let mut keyboards = 0usize;
        let mut mice = 0usize;
        for device in HID_DEVICES.iter() {
            if device.active {
                match device.kind {
                    HidKind::Keyboard => keyboards += 1,
                    HidKind::Mouse => mice += 1,
                }
            }
        }
        if keyboards == 0 && mice == 0 {
            crate::serial::print("[USB-HID] Bağlı USB HID klavye/fare bulunamadı; PS/2 fallback devrede (BIOS Legacy USB/CSM açık olmalıdır).\n");
        } else {
            crate::serial::print("[USB-HID] Hazır: ");
            crate::serial::print_dec(keyboards as u64);
            crate::serial::print(" klavye, ");
            crate::serial::print_dec(mice as u64);
            crate::serial::print(" fare.\n");
        }
    }
}

unsafe fn scan_ports(controller: usize, pmm: &mut PhysicalMemoryManager, initial: bool) {
    let (max_ports, _) = {
        let c = &CONTROLLERS[controller];
        (c.max_ports.min(MAX_PORTS as u8), c.present)
    };

    for port in 1..=max_ports {
        let idx = (port - 1) as usize;
        let c = &mut CONTROLLERS[controller];
        let off = c.op + XhciController::port_offset(port);
        let status = c.rd32(off);
        let connected = status & PORTSC_CCS != 0;
        let changed = status & PORTSC_CSC != 0;
        let known_slot = c.port_slot[idx];
        let was_connected = c.port_connected[idx];

        if changed {
            c.wr32(off, (status & PORTSC_KEEP) | (status & PORTSC_CHANGE_BITS));
        }

        if !connected {
            if was_connected || known_slot != 0 {
                mark_port_disconnected(controller, port, pmm);
            }
            continue;
        }

        c.port_connected[idx] = true;

        let should_enumerate = known_slot == 0 || changed || (initial && known_slot == 0);
        if should_enumerate {
            if known_slot != 0 && known_slot != 0xFF {
                // Port yeniden sıfırlandıysa eski aygıtı kapat.
                mark_port_disconnected(controller, port, pmm);
            }
            enumerate_port(controller, port, pmm);
        }
    }
}

/// Ana döngüden çağrılır: olay halkalarını boşaltır ve HID raporlarını uygular.
/// Yalnızca USB olay halkalarını boşaltır ve fare/klavye raporlarını işler (port taraması yapmaz, PMM istemez).
/// I/O ve dosya kaydetme/yükleme gibi bloke edici işlemlerde farenin donmaması için çağrılır.
pub fn poll_events(
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    let mut update = InputUpdate::default();
    unsafe {
        if !INITIALIZED || CONTROLLER_COUNT == 0 {
            return update;
        }

        // Donanım olay halkalarını boşalt.
        for ci in 0..CONTROLLER_COUNT {
            if !CONTROLLERS[ci].present {
                continue;
            }
            for _ in 0..64 {
                let event = {
                    let c = &mut CONTROLLERS[ci];
                    c.pop_event()
                };
                let Some(event) = event else { break; };
                let changed = dispatch_event(ci, event, mouse, keyboard, screen_w, screen_h);
                update.mouse |= changed.mouse;
                update.keyboard |= changed.keyboard;
            }
        }

        // Komut beklerken biriken olaylar.
        for ci in 0..CONTROLLER_COUNT {
            for _ in 0..64 {
                let event = {
                    let mut found = None;
                    for entry in PENDING_EVENTS.iter_mut() {
                        let Some(pending) = *entry else { continue; };
                        if pending.controller as usize == ci {
                            found = Some(pending.trb);
                            *entry = None;
                            break;
                        }
                    }
                    found
                };
                let Some(event) = event else { break; };
                let changed = dispatch_event(ci, event, mouse, keyboard, screen_w, screen_h);
                update.mouse |= changed.mouse;
                update.keyboard |= changed.keyboard;
            }
        }
    }
    update
}

pub fn poll_input(
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
    pmm: &mut PhysicalMemoryManager,
) -> InputUpdate {
    let mut update = InputUpdate::default();
    unsafe {
        if !INITIALIZED || CONTROLLER_COUNT == 0 {
            return update;
        }

        // Periyodik port taraması (tak/çıkar) ve PCD olayları.
        let now = crate::boot::time::monotonic_ms();
        for ci in 0..CONTROLLER_COUNT {
            if !CONTROLLERS[ci].present {
                continue;
            }
            let state = CONTROLLERS[ci].rd32(CONTROLLERS[ci].op + USBSTS);
            let pcd = state & USBSTS_PCD != 0;
            if pcd {
                CONTROLLERS[ci].wr32(CONTROLLERS[ci].op + USBSTS, USBSTS_PCD | USBSTS_EINT);
            }
            let due = now.wrapping_sub(CONTROLLERS[ci].last_scan_ms) >= 1_000;
            if pcd || due {
                CONTROLLERS[ci].last_scan_ms = now;
                scan_ports(ci, pmm, false);
            }
        }

        // Donanım olay halkalarını boşalt.
        for ci in 0..CONTROLLER_COUNT {
            if !CONTROLLERS[ci].present {
                continue;
            }
            for _ in 0..64 {
                let event = {
                    let c = &mut CONTROLLERS[ci];
                    c.pop_event()
                };
                let Some(event) = event else { break; };
                let changed = dispatch_event(ci, event, mouse, keyboard, screen_w, screen_h);
                update.mouse |= changed.mouse;
                update.keyboard |= changed.keyboard;
            }
        }

        // Komut beklerken biriken olaylar. Olayı göndermeden önce kuyruktan
        // çıkarırız; gönderim sırasında yeni olaylar eklenebilir.
        for ci in 0..CONTROLLER_COUNT {
            for _ in 0..64 {
                let event = {
                    let mut found = None;
                    for entry in PENDING_EVENTS.iter_mut() {
                        let Some(pending) = *entry else { continue; };
                        if pending.controller as usize == ci {
                            found = Some(pending.trb);
                            *entry = None;
                            break;
                        }
                    }
                    found
                };
                let Some(event) = event else { break; };
                let changed = dispatch_event(ci, event, mouse, keyboard, screen_w, screen_h);
                update.mouse |= changed.mouse;
                update.keyboard |= changed.keyboard;
            }
        }

        // Caps Lock / Num Lock LED'lerini klavye durumuyla eşitle.
        sync_keyboard_leds(keyboard);
    }
    update
}

pub fn ready() -> bool {
    unsafe { INITIALIZED && CONTROLLER_COUNT > 0 }
}

pub fn keyboard_present() -> bool {
    unsafe { HID_DEVICES.iter().any(|d| d.active && d.kind == HidKind::Keyboard) }
}

pub fn mouse_present() -> bool {
    unsafe { HID_DEVICES.iter().any(|d| d.active && d.kind == HidKind::Mouse) }
}

pub fn device_count() -> usize {
    unsafe { HID_DEVICES.iter().filter(|d| d.active).count() }
}

pub fn controller_count() -> usize {
    unsafe { CONTROLLER_COUNT }
}

pub fn keyboard_count() -> usize {
    unsafe { HID_DEVICES.iter().filter(|d| d.active && d.kind == HidKind::Keyboard).count() }
}

pub fn mouse_count() -> usize {
    unsafe { HID_DEVICES.iter().filter(|d| d.active && d.kind == HidKind::Mouse).count() }
}

pub fn has_event() -> bool {
    unsafe {
        if !INITIALIZED || CONTROLLER_COUNT == 0 {
            return false;
        }
        for entry in PENDING_EVENTS.iter() {
            if entry.is_some() {
                return true;
            }
        }
        for ci in 0..CONTROLLER_COUNT {
            if CONTROLLERS[ci].present && CONTROLLERS[ci].has_event() {
                return true;
            }
        }
        false
    }
}

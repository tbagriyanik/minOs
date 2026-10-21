#![allow(dead_code, static_mut_refs, unused_unsafe)]

//! USB HID (klavye + fare) sürücüsü — xHCI (USB 3.x) denetleyicileri üzerinden.
//!
//! Bu modül, PCI üzerinde bulunan xHCI denetleyicilerini başlatır, bağlı USB
//! aygıtlarını numaralandırır ve HID Boot Protocol kullanan klavye/fare
//! raporlarını okur. Raporlar mevcut PS/2 klavye/fare durumuna dönüştürülür;
//! böylece masaüstü, uygulamalar ve kısayollar USB donanımıyla da çalışır.
//!
//! Kapsam ve sınırlar:
//! - xHCI (USB 3.0/3.1/3.2) denetleyicileri desteklenir. USB 1.1/2.0 aygıtları
//!   xHCI'nin USB 2.0 portları üzerinden çalışır.
//! - Yalnızca HID sınıfı, alt sınıf 1 (Boot Interface) ve boot protokolü
//!   (klavye protokol 1, fare protokol 2) desteklenir.
//! - Kesme (interrupt) yerine yoklama (polling) kullanılır; masaüstü döngüsü
//!   olay halkasını her karede boşaltır.
//! - UHCI/OHCI/EHCI için yerel sürücü henüz yoktur; bu denetleyicilerde
//!   firmware'in 8042/USB Legacy emülasyonu PS/2 yoluyla çalışmaya devam eder.
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
        }
    }

    #[inline]
    unsafe fn rd32(&self, off: u64) -> u32 {
        core::ptr::read_volatile((self.mmio + off) as *const u32)
    }

    #[inline]
    unsafe fn wr32(&self, off: u64, val: u32) {
        core::ptr::write_volatile((self.mmio + off) as *mut u32, val);
    }

    #[inline]
    unsafe fn rd64(&self, off: u64) -> u64 {
        core::ptr::read_volatile((self.mmio + off) as *const u64)
    }

    #[inline]
    unsafe fn wr64(&self, off: u64, val: u64) {
        core::ptr::write_volatile((self.mmio + off) as *mut u64, val);
    }

    #[inline]
    fn port_offset(port: u8) -> u64 {
        PORTSC_BASE + (port.saturating_sub(1) as u64) * 4
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
        self.run_command(trb).code == COMP_SUCCESS
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
    ) {
        zero_page(input_phys);
        // Input Control Context: yalnızca EP0 eklenecek (DCI 1 -> bit 1).
        self.ctx_write(input_phys, 0, 1, 1 << 1);
        // Slot Context (input index 1). Context Entries = son geçerli DCI (EP0 = 1).
        self.ctx_write(input_phys, 1, 0, ((speed as u32) << 20) | (1 << 27));
        self.ctx_write(input_phys, 1, 1, (port as u32) << 16);
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
    ) -> bool {
        zero_page(input_phys);
        self.ctx_write(input_phys, 0, 1, 1 << 1);
        self.ctx_write(input_phys, 1, 0, ((speed as u32) << 20) | (1 << 27));
        self.ctx_write(input_phys, 1, 1, (port as u32) << 16);
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
        let off = Self::port_offset(port);
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
        let off = Self::port_offset(port);
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
        let hcsparams1 = self.rd32(0x04);
        let hcsparams2 = self.rd32(0x08);
        let hccparams1 = self.rd32(0x10);
        let dboff = self.rd32(0x14) as u64;
        let rtsoff = self.rd32(0x18) as u64;

        self.op = self.mmio + caplen;
        self.db = self.mmio + dboff;
        self.rt = self.mmio + rtsoff;
        self.ctx_size = if hccparams1 & (1 << 2) != 0 { 64 } else { 32 };
        self.max_slots = ((hcsparams1 & 0xFF) as u8).max(1);
        self.max_ports = ((hcsparams1 >> 16) & 0xFF) as u8;

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

        // Scratchpad buffer sayısı (HCSPARAMS2).
        let scratch_lo = ((hcsparams2 >> 21) & 0x1F) as usize;
        let scratch_hi = ((hcsparams2 >> 27) & 0x1F) as usize;
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
        let hccparams1 = self.rd32(0x10);
        let mut offset = ((hccparams1 >> 16) & 0xFFFF) as u64;
        if offset == 0 {
            return;
        }
        offset <<= 2;

        for _ in 0..16 {
            if offset < 0x40 {
                break;
            }
            let val = self.rd32(offset);
            let id = val & 0xFF;
            let next = ((val >> 8) & 0xFF) as u64;
            if id == 0x01 {
                self.wr32(offset, val | (1 << 24)); // OS Owned Semaphore
                let start = crate::boot::time::monotonic_ms();
                while self.rd32(offset) & (1 << 16) != 0 {
                    if crate::boot::time::monotonic_ms().wrapping_sub(start) > 1_000 {
                        crate::serial::print(
                            "[USB-HID] UYARI: BIOS USB legacy devri zaman aşımı; SMM kapatılıyor.\n",
                        );
                        break;
                    }
                    core::hint::spin_loop();
                }
                // USB SMI'larını kapat (USBLEGCTLSTS).
                self.wr32(offset + 4, 0);
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
    ep_ring: TrbRing,
    report_phys: u64,
    report_len: usize,
    prev_modifiers: u8,
    prev_keys: [u8; 6],
    /// Aygıt boot protokolünü kabul etmedi (SET_PROTOCOL başarısız); raporlar
    /// aygıtın kendi biçiminde gelir ve `HidReportLayout` ile ayrıştırılır.
    custom: bool,
    layout: HidReportLayout,
    /// Tanılama: ilk birkaç raporun hex dökümü seri günlüğe yazılır.
    samples: u8,
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
            ep_ring: TrbRing::empty(),
            report_phys: 0,
            report_len: 0,
            prev_modifiers: 0,
            prev_keys: [0; 6],
            custom: false,
            layout: HidReportLayout::default(),
            samples: 0,
        }
    }
}

static mut HID_DEVICES: [HidDevice; MAX_HID_DEVICES] = [HidDevice::empty(); MAX_HID_DEVICES];

#[derive(Clone, Copy)]
struct HidCandidate {
    interface: u8,
    kind: HidKind,
    endpoint: u8,
    max_packet: usize,
    interval: u8,
    /// Arayüz boot protokolünü destekliyor (HID alt sınıf 1 + protokol 1/2).
    boot: bool,
    /// Boot olmayan aygıtlar için rapor tanımlayıcısından çıkarılan düzen.
    layout: HidReportLayout,
}

/// Boot olmayan HID fare raporunun alan yerleşimi.
///
/// Laptop fareleri/dokunmatik yüzeyler tipik olarak düğme bitleri (1-5 bit),
/// 12-16 bitlik işaretli X/Y ve 8 bit tekerlek alanları kullanır. Bu yapı rapor
/// tanımlayıcısından çıkarılan bit yerleşimini taşır.
#[derive(Clone, Copy)]
struct HidReportLayout {
    /// Düğme bit sayısı (raporun ilk bitlerinden itibaren).
    buttons: u8,
    x_start: u8,
    x_size: u8,
    y_start: u8,
    y_size: u8,
    wheel_start: u8,
    wheel_size: u8,
    /// Aygıt çoklu rapor kullanıyorsa raporun ilk baytı Report ID'dir.
    report_id: u8,
    /// X/Y alanının mutlak (Absolute) olup olmadığı ve mantıksal maksimumu.
    x_absolute: bool,
    y_absolute: bool,
    x_max: u32,
    y_max: u32,
}

impl HidReportLayout {
    /// Ayrıştırılamayan aygıtlar için standart 3 baytlık (düğme, dX, dY) yerleşim.
    const fn default() -> Self {
        Self {
            buttons: 3,
            x_start: 8,
            x_size: 8,
            y_start: 16,
            y_size: 8,
            wheel_start: 0,
            wheel_size: 0,
            report_id: 0,
            x_absolute: false,
            y_absolute: false,
            x_max: 0,
            y_max: 0,
        }
    }

    /// Yerleşim gerçekten rapor tanımlayıcısından mı çıkarıldı?
    const fn is_valid(&self) -> bool {
        self.x_size > 0 && self.y_size > 0
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
    let mut layout = HidReportLayout::default();
    let mut page = 0u16;
    let mut usage = 0u16;
    let mut count = 0u32;
    let mut size = 0u32;
    let mut bit = 0u32;
    let mut wheel_bits = 0u32;
    let mut x_start: u32 = 0;
    let mut y_start: u32 = 0;
    let mut wheel_start: u32 = 0;
    let mut x_bits = 0u32;
    let mut y_bits = 0u32;
    let mut x_log_max: i32 = 0;
    let mut y_log_max: i32 = 0;
    let mut x_rel = true;
    let mut y_rel = true;
    let mut report_id = 0u8;
    let mut buttons_bits = 0u32;
    let mut i = 0usize;

    while i < desc.len() {
        let prefix = desc[i];
        i += 1;
        if prefix == 0xFE {
            // Long item; bu sürücü için gerekli değil.
            if i >= desc.len() { break; }
            let len = desc[i] as usize;
            i += 2 + len;
            continue;
        }
        let b_size = (prefix & 0x03) as usize;
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        if i + b_size > desc.len() { break; }
        let mut value = 0u32;
        for k in 0..b_size {
            value |= (desc[i + k] as u32) << (8 * k);
        }
        i += b_size;

        if item_type == 0x01 {
            // Global öğeler.
            match tag {
                0x04 => page = value as u16,               // Usage Page
                0x08 => usage = value as u16,              // Usage
                0x09 => count = value,                     // Report Count
                0x07 => size = value,                      // Report Size
                0x0A => {}                                 // Logical Minimum
                0x0B => {                                  // Logical Maximum
                    if usage == 0x30 { x_log_max = value as i32; }
                    if usage == 0x31 { y_log_max = value as i32; }
                }
                _ => {}
            }
        } else if item_type == 0x00 {
            // Main öğeler.
            match tag {
                0x08 => {
                    // Input: bit 0 = Data/Constant, bit 1 = Variable/Array,
                    // bit 2 = Absolute/Relative.
                    let is_data_var = value & 0x01 != 0 && value & 0x02 != 0;
                    let relative = value & 0x04 != 0;
                    if page == 0x09 {
                        // Düğme (Button) sayfası: raporun başındadır.
                        if buttons_bits == 0 {
                            buttons_bits = count * size;
                        }
                        bit += count * size;
                    } else if is_data_var && count == 1 {
                        match usage {
                            0x30 => {
                                x_start = bit;
                                x_bits = size;
                                x_rel = relative;
                            }
                            0x31 => {
                                y_start = bit;
                                y_bits = size;
                                y_rel = relative;
                            }
                            0x38 => {
                                wheel_start = bit;
                                wheel_bits = size;
                            }
                            _ => {}
                        }
                    }
                    bit += count * size;
                }
                0x85 => {
                    report_id = value as u8;
                }
                _ => {}
            }
        } else if item_type == 0x01 {
            // Global Logical Max değerlerini yakala (tag 0x0B yukarıda atlandı).
        }
    }

    // Logical Max değerlerini ikinci geçişte yakalayamadıysak mutlak
    // alanların üst sınırını 0 bırakıyoruz (rapor işleyici tam bayt aralığı
    // kullanır). İşaretli alanlar için tipik maksimumlar:
    // 8 bit: 127, 12 bit: 2047, 16 bit: 32767.
    if x_bits == 0 || y_bits == 0 {
        return HidReportLayout::default();
    }
    layout.buttons = buttons_bits.min(8) as u8;
    layout.x_start = x_start as u8;
    layout.x_size = x_bits as u8;
    layout.y_start = y_start as u8;
    layout.y_size = y_bits as u8;
    layout.wheel_start = wheel_start as u8;
    layout.wheel_size = wheel_bits as u8;
    layout.report_id = report_id;
    layout.x_absolute = !x_rel;
    layout.y_absolute = !y_rel;
    // Mutlak alanlar için mantıksal maksimum tanımlayıcıdan gelir (0 = bilinmiyor,
    // işleyici bu durumda 16 bitlik standart 32767 kabul eder).
    layout.x_max = if layout.x_absolute && x_log_max > 0 { x_log_max as u32 }
        else if layout.x_absolute { match x_bits { 1..=8 => 127, 9..=12 => 2047, 13..=16 => 32767, _ => 0 } }
        else { 0 };
    layout.y_max = if layout.y_absolute && y_log_max > 0 { y_log_max as u32 }
        else if layout.y_absolute { match y_bits { 1..=8 => 127, 9..=12 => 2047, 13..=16 => 32767, _ => 0 } }
        else { 0 };
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
    let mut data = data;
    if layout.report_id != 0 {
        if data.is_empty() || data[0] != layout.report_id {
            // İlk bayt Report ID ile eşleşmiyorsa bilinmeyen rapor türü.
            if data.is_empty() {
                return false;
            }
        }
        data = &data[1..];
    }

    let bits_total = layout.x_start as usize
        + layout.x_size as usize
        + layout.y_size as usize
        + layout.wheel_size as usize;
    let bytes_needed = (bits_total + 7) / 8;
    if data.len() < bytes_needed.max(3) {
        return false;
    }

    // Bit dizisinden alan çıkarma yardımcıları (LSB-first).
    let get_bits = |start: u8, bits: u8| -> u32 {
        let mut v = 0u32;
        for k in 0..bits as usize {
            let idx = start as usize + k;
            let bitv = (data[idx / 8] >> (idx % 8)) & 1;
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

    let mut buttons = 0u8;
    for b in 0..layout.buttons.min(8) {
        let bitv = get_bits(b, 1);
        buttons |= (bitv as u8) << b;
    }

    if layout.x_absolute && layout.y_absolute && layout.x_max > 0 {
        // Mutlak konum (dokunmatik yüzey/digitizer).
        let raw_x = get_bits(layout.x_start, layout.x_size);
        let raw_y = get_bits(layout.y_start, layout.y_size);
        mouse.x = ((raw_x as i32 * screen_w) / layout.x_max as i32).clamp(0, screen_w - 1);
        mouse.y = ((raw_y as i32 * screen_h) / layout.y_max as i32).clamp(0, screen_h - 1);
    } else {
        // Göreli hareket (klasik fare/dokunmatik yüzey akışı).
        let raw_x = get_bits(layout.x_start, layout.x_size);
        let raw_y = get_bits(layout.y_start, layout.y_size);
        let dx = sign_extend(raw_x, layout.x_size) as i8;
        // HID'de pozitif Y aşağıdır; PS2 durumunda pozitif Y yukarı beklenir.
        let dy = (sign_extend(raw_y, layout.y_size) as i8).wrapping_neg();
        mouse.update_packet(buttons, dx, dy, screen_w, screen_h);
    }

    if layout.wheel_size > 0 {
        let raw_w = get_bits(layout.wheel_start, layout.wheel_size);
        let wheel = sign_extend(raw_w, layout.wheel_size) as i8;
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
fn parse_hid_interface(config: &[u8]) -> Option<HidCandidate> {
    let mut i = 0usize;
    let mut current: Option<(u8, u8, u8, u8)> = None; // (iface, class, subclass, protocol)
    let mut best: Option<HidCandidate> = None;

    while i + 2 <= config.len() {
        let len = config[i] as usize;
        let dtype = config[i + 1];
        if len < 2 || i + len > config.len() {
            break;
        }

        if dtype == 0x04 && len >= 9 {
            current = Some((config[i + 2], config[i + 5], config[i + 6], config[i + 7]));
        } else if dtype == 0x05 && len >= 7 {
            if let Some((interface, class, subclass, protocol)) = current {
                if class == 0x03 {
                    let endpoint = config[i + 2];
                    let attributes = config[i + 3];
                    if endpoint & 0x80 != 0 && attributes & 0x03 == 0x03 {
                        let max_packet = ((config[i + 4] as usize) | ((config[i + 5] as usize) << 8)) & 0x7FF;
                        let boot = subclass == 0x01 && (protocol == 1 || protocol == 2);
                        let kind = if boot && protocol == 1 {
                            HidKind::Keyboard
                        } else if boot && protocol == 2 {
                            HidKind::Mouse
                        } else if protocol == 1 {
                            HidKind::Keyboard
                        } else {
                            HidKind::Mouse
                        };
                        let candidate = HidCandidate {
                            interface,
                            kind,
                            endpoint,
                            max_packet: max_packet.max(1),
                            interval: config[i + 6],
                            boot,
                            layout: HidReportLayout::default(),
                        };
                        // Aday öncelik sıralaması:
                        let rank = |c: &HidCandidate| match (c.boot, c.kind) {
                            (true, HidKind::Keyboard) => 0,
                            (true, HidKind::Mouse) => 1,
                            (false, HidKind::Mouse) => 2,
                            (false, HidKind::Keyboard) => 3,
                        };
                        if best.map_or(true, |b| rank(&candidate) < rank(&b)) {
                            best = Some(candidate);
                        }
                    }
                }
            }
        }

        i += len;
    }

    best
}

/// Yapılandırmadan seçilen aday için HID rapor tanımlayıcısını okur ve
/// boot olmayan farelerde alan yerleşimini çıkarır. Başarısız olursa
/// standart boot düzeni korunur.
unsafe fn load_report_layout(
    c: &mut XhciController,
    ep0_ring: &mut TrbRing,
    slot: u8,
    candidate: &mut HidCandidate,
    buffer_phys: u64,
    buffer: *mut u8,
) {
    if candidate.boot || candidate.kind != HidKind::Mouse {
        return;
    }
    // HID Tanımlayıcı (bDescriptorType = 0x21) uzunluğunu yapılandırma
    // tanımlayıcısından okumak yerine doğrudan GET_DESCRIPTOR ile alınır.
    let Some(n) = c.control_in(
        ep0_ring, slot, 0x81, 0x06,
        0x2200, candidate.interface as u16, buffer_phys, 512,
    ) else {
        return;
    };
    if n == 0 {
        return;
    }
    let desc = core::slice::from_raw_parts(buffer, n.min(512));
    let layout = parse_hid_report_descriptor(desc);
    if layout.is_valid() {
        candidate.layout = layout;
    }
}

/// HID kullanım kimliğini PS/2 set-1 tarama koduna çevirir.
/// Dönüş: (scancode, genişletilmiş mi?)
fn hid_key_to_scancode(usage: u8) -> Option<(u8, bool)> {
    Some(match usage {
        0x04 => (0x1E, false), // a
        0x05 => (0x30, false), // b
        0x06 => (0x2E, false), // c
        0x07 => (0x20, false), // d
        0x08 => (0x12, false), // e
        0x09 => (0x21, false), // f
        0x0A => (0x22, false), // g
        0x0B => (0x23, false), // h
        0x0C => (0x17, false), // i
        0x0D => (0x24, false), // j
        0x0E => (0x25, false), // k
        0x0F => (0x26, false), // l
        0x10 => (0x32, false), // m
        0x11 => (0x31, false), // n
        0x12 => (0x18, false), // o
        0x13 => (0x19, false), // p
        0x14 => (0x10, false), // q
        0x15 => (0x13, false), // r
        0x16 => (0x1F, false), // s
        0x17 => (0x14, false), // t
        0x18 => (0x16, false), // u
        0x19 => (0x2F, false), // v
        0x1A => (0x11, false), // w
        0x1B => (0x2D, false), // x
        0x1C => (0x15, false), // y
        0x1D => (0x2C, false), // z
        0x1E => (0x02, false), // 1
        0x1F => (0x03, false), // 2
        0x20 => (0x04, false), // 3
        0x21 => (0x05, false), // 4
        0x22 => (0x06, false), // 5
        0x23 => (0x07, false), // 6
        0x24 => (0x08, false), // 7
        0x25 => (0x09, false), // 8
        0x26 => (0x0A, false), // 9
        0x27 => (0x0B, false), // 0
        0x28 => (0x1C, false), // Enter
        0x29 => (0x01, false), // Escape
        0x2A => (0x0E, false), // Backspace
        0x2B => (0x0F, false), // Tab
        0x2C => (0x39, false), // Space
        0x2D => (0x0C, false), // -
        0x2E => (0x0D, false), // =
        0x2F => (0x1A, false), // [
        0x30 => (0x1B, false), // ]
        0x31 => (0x2B, false), // backslash
        0x32 => (0x27, false), // ;
        0x33 => (0x28, false), // '
        0x34 => (0x29, false), // `
        0x35 => (0x33, false), // ,
        0x36 => (0x34, false), // .
        0x37 => (0x35, false), // /
        0x38 => (0x3A, false), // Caps Lock
        0x39 => (0x3B, false), // F1
        0x3A => (0x3C, false),
        0x3B => (0x3D, false),
        0x3C => (0x3E, false),
        0x3D => (0x3F, false),
        0x3E => (0x40, false),
        0x3F => (0x41, false),
        0x40 => (0x42, false),
        0x41 => (0x43, false),
        0x42 => (0x44, false), // F10
        0x43 => (0x57, false), // F11
        0x44 => (0x58, false), // F12
        0x45 => (0x37, true),  // Print Screen
        0x46 => (0x46, false), // Scroll Lock
        0x47 => return None,   // Pause (E1 1D 45 dizisi ayrı ele alınmıyor)
        0x48 => (0x52, true),  // Insert
        0x49 => (0x47, true),  // Home
        0x4A => (0x49, true),  // Page Up
        0x4B => (0x53, true),  // Delete
        0x4C => (0x4F, true),  // End
        0x4D => (0x51, true),  // Page Down
        0x4E => (0x4D, true),  // Right
        0x4F => (0x4B, true),  // Left
        0x50 => (0x50, true),  // Down
        0x51 => (0x48, true),  // Up
        0x52 => (0x45, false), // Num Lock
        0x53 => (0x35, true),  // Keypad /
        0x54 => (0x37, false), // Keypad *
        0x55 => (0x4A, false), // Keypad -
        0x56 => (0x4E, false), // Keypad +
        0x57 => (0x1C, true),  // Keypad Enter
        0x58 => (0x4F, false), // Keypad 1
        0x59 => (0x50, false), // Keypad 2
        0x5A => (0x51, false), // Keypad 3
        0x5B => (0x4B, false), // Keypad 4
        0x5C => (0x4C, false), // Keypad 5
        0x5D => (0x4D, false), // Keypad 6
        0x5E => (0x47, false), // Keypad 7
        0x5F => (0x48, false), // Keypad 8
        0x60 => (0x49, false), // Keypad 9
        0x61 => (0x52, false), // Keypad 0
        0x62 => (0x53, false), // Keypad .
        0x63 => (0x59, false), // Keypad =
        0x64 => (0x56, false), // < >
        0x65 => (0x5D, true),  // Application
        0x7F => (0x20, true),  // Mute
        0x80 => (0x30, true),  // Volume Up
        0x81 => (0x2E, true),  // Volume Down
        _ => return None,
    })
}

fn emit_key(kb: &mut KeyboardState, usage: u8, pressed: bool) {
    let Some((code, extended)) = hid_key_to_scancode(usage) else { return; };
    let code = if pressed { code } else { code | 0x80 };
    if extended {
        kb.handle_scancode(0xE0);
    }
    kb.handle_scancode(code);
}

fn emit_modifier(kb: &mut KeyboardState, bit: u8, pressed: bool) {
    let (code, extended) = match bit {
        0 => (0x1D, false), // Left Ctrl
        1 => (0x2A, false), // Left Shift
        2 => (0x38, false), // Left Alt
        3 => (0x5B, false), // Left GUI
        4 => (0x1D, true),  // Right Ctrl
        5 => (0x36, false), // Right Shift
        6 => (0x38, true),  // Right Alt
        7 => (0x5C, true),  // Right GUI
        _ => return,
    };
    let code = if pressed { code } else { code | 0x80 };
    if extended {
        kb.handle_scancode(0xE0);
    }
    kb.handle_scancode(code);
}

fn handle_keyboard_report(device: &mut HidDevice, data: &[u8], kb: &mut KeyboardState) -> bool {
    if data.len() < 8 {
        return false;
    }

    let modifiers = data[0];
    let mut keys = [0u8; 6];
    keys.copy_from_slice(&data[2..8]);

    // Rollover (0x01) durumunda tuş listesini yoksay.
    let rollover = keys.iter().any(|&k| k == 0x01);

    if !rollover {
        // Bırakılan tuşlar.
        for i in 0..6 {
            let old = device.prev_keys[i];
            if old >= 0x02 && !keys.contains(&old) {
                emit_key(kb, old, false);
            }
        }

        // Modifier değişimleri (basılı tutulan shift yeni tuşlardan önce işlenir).
        let changed = device.prev_modifiers ^ modifiers;
        for bit in 0..8u8 {
            if changed & (1 << bit) != 0 {
                emit_modifier(kb, bit, modifiers & (1 << bit) != 0);
            }
        }

        // Yeni basılan tuşlar.
        for i in 0..6 {
            let key = keys[i];
            if key >= 0x02 && !device.prev_keys.contains(&key) {
                emit_key(kb, key, true);
            }
        }

        device.prev_keys = keys;
    }

    device.prev_modifiers = modifiers;
    true
}

fn handle_mouse_report(
    data: &[u8],
    mouse: &mut MouseState,
    screen_w: i32,
    screen_h: i32,
) -> bool {
    if data.len() < 3 {
        return false;
    }
    let buttons = data[0] & 0x07;
    let dx = data[1] as i8;
    // HID'de pozitif Y aşağıdır; PS/2 durumunda pozitif Y yukarı beklenir.
    let dy = (data[2] as i8).wrapping_neg();
    mouse.update_packet(buttons, dx, dy, screen_w, screen_h);
    if data.len() >= 4 {
        let wheel = data[3] as i8;
        mouse.scroll_delta = mouse.scroll_delta.saturating_sub(wheel);
    }
    true
}

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

unsafe fn mark_port_disconnected(controller: usize, port: u8) {
    let c = &mut CONTROLLERS[controller];
    let slot = c.port_slot[(port - 1) as usize];
    c.port_slot[(port - 1) as usize] = 0;
    c.port_connected[(port - 1) as usize] = false;
    if slot == 0 || slot == 0xFF {
        return;
    }
    for device in HID_DEVICES.iter_mut() {
        if device.active && device.controller == controller as u8 && device.slot == slot {
            device.active = false;
            crate::serial::print("[USB-HID] Aygıt çıkarıldı (port ");
            crate::serial::print_dec(port as u64);
            crate::serial::print(").\n");
        }
    }
    c.disable_slot(slot);
}

unsafe fn enumerate_port(controller: usize, port: u8, pmm: &mut PhysicalMemoryManager) {
    let c = &mut CONTROLLERS[controller];
    crate::serial::print("[USB-HID] Port ");
    crate::serial::print_dec(port as u64);
    crate::serial::print(" taranıyor...\n");

    let Some(speed) = c.reset_port(port) else {
        crate::serial::print("[USB-HID] Port reset başarısız.\n");
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };

    let Some(slot) = c.enable_slot() else {
        crate::serial::print("[USB-HID] Slot ayrılamadı.\n");
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };

    let Some(dma) = pmm.allocate_contiguous_frames(5) else {
        crate::serial::print("[USB-HID] DMA belleği ayrılamadı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };

    let input_phys = dma;
    let device_phys = dma + PAGE;
    let ep0_phys = dma + 2 * PAGE;
    let ep1_phys = dma + 3 * PAGE;
    let buffer_phys = dma + 4 * PAGE;
    for page in 0..5 {
        zero_page(dma + page * PAGE);
    }

    // Cihaz bağlamı DCBAA'ya yazılmalı.
    let dcbaa = crate::drivers::phys_to_virt(c.dcbaa_phys) as *mut u64;
    core::ptr::write_volatile(dcbaa.add(slot as usize), device_phys);

    let mut ep0_ring = TrbRing::new(ep0_phys);
    let ep1_ring = TrbRing::new(ep1_phys);
    let max_packet0 = initial_ep0_packet(speed);
    c.build_address_context(input_phys, speed, port, max_packet0, &ep0_ring);

    if !c.address_device(slot, input_phys) {
        crate::serial::print("[USB-HID] Address Device başarısız.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    // İlk 8 baytlık aygıt tanımlayıcısı (EP0 max packet boyutunu öğrenmek için).
    let buffer = crate::drivers::phys_to_virt(buffer_phys) as *mut u8;
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0100, 0, buffer_phys, 8) else {
        crate::serial::print("[USB-HID] Aygıt tanımlayıcısı okunamadı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };
    if n < 8 {
        crate::serial::print("[USB-HID] Kısa aygıt tanımlayıcısı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    let reported_mps = core::ptr::read_volatile(buffer.add(7)) as u16;
    let ep0_mps = if matches!(reported_mps, 8 | 16 | 32 | 64 | 512) {
        reported_mps
    } else {
        max_packet0
    };
    if ep0_mps != max_packet0 {
        if !c.update_ep0_max_packet(slot, input_phys, speed, port, ep0_mps, &ep0_ring) {
            crate::serial::print("[USB-HID] EP0 max packet güncellenemedi.\n");
            c.disable_slot(slot);
            c.port_slot[(port - 1) as usize] = 0xFF;
            return;
        }
    }

    // Tam aygıt tanımlayıcısı.
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0100, 0, buffer_phys, 18) else {
        crate::serial::print("[USB-HID] Tam aygıt tanımlayıcısı okunamadı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };
    if n < 18 {
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }
    let vendor = core::ptr::read_volatile(buffer.add(8)) as u16
        | ((core::ptr::read_volatile(buffer.add(9)) as u16) << 8);
    let product = core::ptr::read_volatile(buffer.add(10)) as u16
        | ((core::ptr::read_volatile(buffer.add(11)) as u16) << 8);
    crate::serial::print("[USB-HID] Aygıt VID:PID = ");
    crate::serial::print_hex(vendor as u64);
    crate::serial::print(":");
    crate::serial::print_hex(product as u64);
    crate::serial::print("\n");

    // Yapılandırma tanımlayıcısının başlığı.
    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0200, 0, buffer_phys, 9) else {
        crate::serial::print("[USB-HID] Yapılandırma başlığı okunamadı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };
    if n < 9 {
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }
    let total = core::ptr::read_volatile(buffer.add(2)) as usize
        | ((core::ptr::read_volatile(buffer.add(3)) as usize) << 8);
    let config_value = core::ptr::read_volatile(buffer.add(5));
    if total < 9 || total > PAGE as usize {
        crate::serial::print("[USB-HID] Yapılandırma boyutu geçersiz.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    core::ptr::write_bytes(buffer, 0, PAGE as usize);
    let Some(n) = c.control_in(&mut ep0_ring, slot, 0x80, 0x06, 0x0200, 0, buffer_phys, total) else {
        crate::serial::print("[USB-HID] Yapılandırma tanımlayıcısı okunamadı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };
    if n < 9 {
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    let config_slice = core::slice::from_raw_parts(buffer, n.min(PAGE as usize));
    let Some(mut candidate) = parse_hid_interface(config_slice) else {
        crate::serial::print("[USB-HID] HID boot arayüzü bulunamadı; aygıt atlandı.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };

    // HID rapor tanımlayıcısını (Report Descriptor) oku; boot olmayan fareler
    // için alan yerleşimini çıkar.
    unsafe {
        load_report_layout(
            c, &mut ep0_ring, slot, &mut candidate,
            buffer_phys, buffer,
        );
    }

    // Yapılandırmayı seç.
    if c.control_out(&mut ep0_ring, slot, 0x00, 0x09, config_value as u16, 0, 0, 0).is_none() {
        crate::serial::print("[USB-HID] SET_CONFIGURATION başarısız.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    // Boot protokollü arayüzlerde HID boot protokolüne geç (0 = boot).
    // Boot olmayan aygıtlarda bu istek desteklenmeyebilir; başarısızlık
    // aygıtı engellemez, raporlar kendi biçiminde ayrıştırılır.
    let mut custom = false;
    if candidate.boot {
        if c.control_out(&mut ep0_ring, slot, 0x21, 0x0B, 0, candidate.interface as u16, 0, 0).is_none() {
            crate::serial::print("[USB-HID] SET_PROTOCOL (boot) başarısız; özel rapor düzeni kullanılacak.\n");
            custom = true;
        }
    } else {
        custom = true;
        crate::serial::print("[USB-HID] Boot protokolü yok; rapor tanımlayıcısından düzen çıkarıldı.\n");
    }

    // Kesme IN uç noktasını yapılandır.
    let ep_number = candidate.endpoint & 0x0F;
    let dci = ep_number * 2 + 1;
    zero_page(input_phys);
    // Slot bağlamı (A0) ve kesme IN uç noktası (A<DCI>) eklenir. EP0 zaten
    // yapılandırıldığı için A1 (EP0) bayrağı ve EP0 bağlamı boş bırakılır.
    c.ctx_write(input_phys, 0, 1, (1 << 0) | (1 << dci));
    c.ctx_write(
        input_phys,
        1,
        0,
        ((speed as u32) << 20) | (((dci as u32) & 0x1F) << 27),
    );
    c.ctx_write(input_phys, 1, 1, (port as u32) << 16);
    let ep_index = dci as usize + 1;
    c.ctx_write(
        input_phys,
        ep_index,
        0,
        report_interval(speed, candidate.interval) << 16,
    );
    c.ctx_write(
        input_phys,
        ep_index,
        1,
        (3 << 1) | (7 << 3) | ((candidate.max_packet as u32) << 16),
    );
    c.ctx_write(input_phys, ep_index, 2, (ep1_ring.phys as u32) | 1);
    c.ctx_write(input_phys, ep_index, 3, (ep1_ring.phys >> 32) as u32);

    if !c.configure_endpoint(slot, input_phys) {
        crate::serial::print("[USB-HID] Configure Endpoint başarısız.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    }

    let Some(index) = find_free_hid_slot() else {
        crate::serial::print("[USB-HID] Boş HID aygıt yuvası yok.\n");
        c.disable_slot(slot);
        c.port_slot[(port - 1) as usize] = 0xFF;
        return;
    };

    let report_len = candidate.max_packet.clamp(1, 64);
    HID_DEVICES[index] = HidDevice {
        active: true,
        controller: controller as u8,
        slot,
        port,
        kind: candidate.kind,
        dci,
        ep_ring: ep1_ring,
        report_phys: buffer_phys,
        report_len,
        prev_modifiers: 0,
        prev_keys: [0; 6],
        custom,
        layout: candidate.layout,
        samples: 0,
    };
    c.port_slot[(port - 1) as usize] = slot;
    c.port_connected[(port - 1) as usize] = true;

    crate::serial::print("[USB-HID] HID ");
    crate::serial::print(candidate.kind.label());
    crate::serial::print(" hazır (slot ");
    crate::serial::print_dec(slot as u64);
    crate::serial::print(", rapor ");
    crate::serial::print_dec(report_len as u64);
    crate::serial::print(" bayt");
    if custom {
        crate::serial::print(", özel düzen (düğmeler=");
        crate::serial::print_dec(candidate.layout.buttons as u64);
        crate::serial::print(")");
    }
    crate::serial::print(").\n");

    queue_hid_transfer(&mut HID_DEVICES[index]);
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
                        if crate::drivers::usb_keyboard::process_keyboard_report(&mut kbd_dev, data, keyboard) {
                            device.prev_modifiers = kbd_dev.prev_modifiers;
                            device.prev_keys = kbd_dev.prev_keys;
                            updated.keyboard = true;
                        }
                    }
                    HidKind::Mouse => {
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
                                mode: if data.len() >= 6 && data.len() <= 8 {
                                    crate::drivers::usb_mouse::UsbPointerMode::Absolute
                                } else {
                                    crate::drivers::usb_mouse::UsbPointerMode::Relative
                                },
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
                        // Tanılama: özel düzende ilk 8 raporun hex dökümü.
                        if device.custom && device.samples < 8 {
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
            let hccparams1 = c.rd32(0x10);
            if hccparams1 & (1 << 3) != 0 {
                for port in 1..=c.max_ports.min(MAX_PORTS as u8) {
                    let off = XhciController::port_offset(port);
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
        let off = XhciController::port_offset(port);
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
                mark_port_disconnected(controller, port);
            }
            continue;
        }

        c.port_connected[idx] = true;

        let should_enumerate = known_slot == 0 || changed || (initial && known_slot == 0);
        if should_enumerate {
            if known_slot != 0 && known_slot != 0xFF {
                // Port yeniden sıfırlandıysa eski aygıtı kapat.
                mark_port_disconnected(controller, port);
            }
            enumerate_port(controller, port, pmm);
        }
    }
}

/// Ana döngüden çağrılır: olay halkalarını boşaltır ve HID raporlarını uygular.
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

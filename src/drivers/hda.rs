#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel High Definition Audio (HDA) sürücüsü — QEMU, VirtualBox, VMware ve
//! gerçek donanım ile tam uyumlu DMA akışı.
//!
//! Özellikler:
//!   * Evrensel PCI taraması: PCI Class 0x0403 (Audio Controller) + bilinen VID/DID kimlikleri
//!   * 64-bit BAR0 ve 2MB MMIO sayfa eşlemesi
//!   * Donanımsal CRST (Controller Reset) ve STATESTS codec algılaması
//!   * AFG (Audio Function Group) D0 + DAC (NID 2) + Çıkış Pini (NID 3) D0/unmute
//!   * Stream SRST (Stream Reset) protokolü ile donanım FIFO/LPIB sıfırlama
//!   * Donanımsal DMA Position Buffer (offset 0x70) + MMIO SDnLPIB hibrit pozisyon takibi
//!   * 32-bit atomik SDnCTL yazımları ile güvenilir Stream Tag + RUN kontrolü
//!   * Otomatik takılma (stall) tespiti ve PC Speaker yedek devresine şeffaf geçiş
//!   * Donanımsal mikser / kazanç (verb 0x3) ve anlık ses/sessiz (mute) kontrolü

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

fn serial_print(s: &str) {
    crate::serial::print(s);
}

// ---------------------------------------------------------------------------
// PCI ve MMIO yardımcıları
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

#[inline]
unsafe fn mmio_read32(base: u64, off: u32) -> u32 {
    core::ptr::read_volatile((base + off as u64) as *const u32)
}

#[inline]
unsafe fn mmio_write32(base: u64, off: u32, val: u32) {
    core::ptr::write_volatile((base + off as u64) as *mut u32, val);
}

#[inline]
unsafe fn mmio_read16(base: u64, off: u32) -> u16 {
    core::ptr::read_volatile((base + off as u64) as *const u16)
}

#[inline]
unsafe fn mmio_write16(base: u64, off: u32, val: u16) {
    core::ptr::write_volatile((base + off as u64) as *mut u16, val);
}

#[inline]
unsafe fn mmio_read8(base: u64, off: u32) -> u8 {
    core::ptr::read_volatile((base + off as u64) as *const u8)
}

#[inline]
unsafe fn mmio_write8(base: u64, off: u32, val: u8) {
    core::ptr::write_volatile((base + off as u64) as *mut u8, val);
}

// ---------------------------------------------------------------------------
// PCI Konfigürasyon Erişimi (port 0xCF8 / 0xCFC)
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
// HDA Register Tanımları (Intel HDA Spec 1.0a)
// ---------------------------------------------------------------------------

const GCAP: u32 = 0x00;
const GCTL: u32 = 0x08;
const STATESTS: u32 = 0x0E;
const INTCTL: u32 = 0x20;
const WALLCLOCK: u32 = 0x30;

const CORB_BASE_LO: u32 = 0x40;
const CORB_BASE_HI: u32 = 0x44;
const CORB_WP: u32 = 0x48;
const CORB_RP: u32 = 0x4A;
const CORB_CTL: u32 = 0x4C;
const CORB_SIZE_REG: u32 = 0x4E;

const RIRB_BASE_LO: u32 = 0x50;
const RIRB_BASE_HI: u32 = 0x54;
const RIRB_WP: u32 = 0x58;
const RIRB_INTCTL: u32 = 0x5C;
const RIRB_CTL: u32 = 0x5C;
const RIRB_SIZE_REG: u32 = 0x5E;

const DPLBASE: u32 = 0x70;
const DPUBASE: u32 = 0x74;

// Stream Descriptor Ofsetleri
const SD_CTL: u32 = 0x00;
const SD_STS: u32 = 0x03;
const SD_LPIB: u32 = 0x04;
const SD_CBL: u32 = 0x08;
const SD_LVI: u32 = 0x0C;
const SD_FMT: u32 = 0x12;
const SD_BDPL: u32 = 0x18;
const SD_BDPU: u32 = 0x1C;

// Codec Node ID'leri (QEMU hda-duplex / hda-output standardı)
const HDA_DAC_NID: u32 = 0x02;     // Çıkış DAC çeviricisi
const HDA_OUT_PIN_NID: u32 = 0x03; // Çıkış pini (kulaklık/hoparlör)
const OUT_STREAM_NUM: u8 = 1;      // Stream ID 1

// CORB / RIRB boyutları
const CORB_SIZE: usize = 256;
const RIRB_SIZE: usize = 256;

// BDL (Buffer Descriptor List): 4 tampon * 4096 byte = 16384 byte (~85ms @ 48kHz stereo)
const BDL_SIZE: usize = 4;
pub const BDL_BUF_SIZE: u32 = 4096;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HdaBdlEntry {
    pub address: u64,
    pub length: u32,
    pub ioc: u32,
}

// 128-byte hizalı DMA havuz yapısı
#[repr(C, align(256))]
pub struct HdaDmaPool {
    pub corb: [u32; CORB_SIZE],                      // 1024 B (0)
    pub rirb: [u64; RIRB_SIZE],                      // 2048 B (1024)
    pub bdl: [HdaBdlEntry; BDL_SIZE],                // 64 B   (3072)
    pub dma_pos: [u32; 16],                         // 64 B   (3136) DMA Position Buffer
    pub _pad: [u8; 128],                            // 128 B  (3200) -> PCM 3328'de (128B hizalı)
    pub pcm: [u8; BDL_SIZE * BDL_BUF_SIZE as usize], // 16384 B (3328)
    pub corb_wp: u16,
    pub rirb_wp: u16,
}

static mut DMA_POOL_ADDR: u64 = 0;

unsafe fn dma_pool_virt() -> u64 {
    crate::drivers::phys_to_virt(DMA_POOL_ADDR)
}

// ---------------------------------------------------------------------------
// HDA Sürücü Durumu
// ---------------------------------------------------------------------------

static HDA_READY: AtomicBool = AtomicBool::new(false);
static mut HDA_MMIO_BASE: u64 = 0;
static mut HDA_SD_BASE: u32 = 0x100;
static mut HDA_CAD: u8 = 0;
static mut HDA_OUT_SD_IDX: usize = 4;

static LAST_DMA_POS: AtomicU32 = AtomicU32::new(0);
static DMA_ADVANCES: AtomicU32 = AtomicU32::new(0);
static DMA_STALL_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn is_ready() -> bool {
    HDA_READY.load(Ordering::Relaxed)
}

/// DMA akışının sağlıklı ilerleyip ilerlemediğini döner.
pub fn is_dma_progressing() -> bool {
    DMA_ADVANCES.load(Ordering::Relaxed) > 0 && DMA_STALL_COUNT.load(Ordering::Relaxed) < 6
}

/// DMA'nın takılıp takılmadığını döner (aralıksız 6 push boyunca LPIB değişmediyse).
pub fn is_dma_stalled() -> bool {
    DMA_STALL_COUNT.load(Ordering::Relaxed) >= 6
}

/// Başlangıç veya ilerleme aşamasında DMA'nın geçerli olup olmadığını döner.
pub fn is_dma_progressing_or_starting() -> bool {
    is_ready() && !is_dma_stalled()
}

// CORB komut gönderme (verb)
unsafe fn send_verb(verb: u32) -> u64 {
    let mmio = HDA_MMIO_BASE;
    let pool = dma_pool_virt() as *mut HdaDmaPool;

    let mut wp = (*pool).corb_wp;
    wp = (wp + 1) % (CORB_SIZE as u16);
    (*pool).corb[wp as usize] = verb;
    (*pool).corb_wp = wp;
    mmio_write16(mmio, CORB_WP, wp);

    let mut spins = 0u32;
    loop {
        let rp = mmio_read16(mmio, CORB_RP);
        if (rp & 0xFF) == (wp & 0xFF) {
            break;
        }
        spins += 1;
        if spins > 5_000 {
            break;
        }
        core::hint::spin_loop();
    }

    let mut response = 0u64;
    let mut rirb_spins = 0u32;
    loop {
        let rwp = mmio_read16(mmio, RIRB_WP) & 0xFF;
        let cached = (*pool).rirb_wp & 0xFF;
        if rwp != cached {
            (*pool).rirb_wp = ((*pool).rirb_wp + 1) % (RIRB_SIZE as u16);
            let idx = (*pool).rirb_wp as usize;
            response = (*pool).rirb[idx];
            break;
        }
        rirb_spins += 1;
        if rirb_spins > 5_000 {
            break;
        }
        core::hint::spin_loop();
    }
    response
}

#[inline]
fn make_verb(cad: u8, nid: u32, verb: u32, payload: u32) -> u32 {
    if verb < 0x10 {
        ((cad as u32) << 28) | (nid << 20) | (verb << 16) | (payload & 0xFFFF)
    } else {
        ((cad as u32) << 28) | (nid << 20) | (verb << 8) | (payload & 0xFF)
    }
}

// ---------------------------------------------------------------------------
// Stream DMA Denetimleri & Pozisyon Okuma
// ---------------------------------------------------------------------------

#[inline]
pub unsafe fn stream_lpib() -> u32 {
    mmio_read32(HDA_MMIO_BASE, HDA_SD_BASE + SD_LPIB)
}

/// Donanımsal DMA pozisyonunu döner. Önce DMA Position Buffer (RAM),
/// yoksa donanım MMIO SD_LPIB register'ından okur.
pub fn current_dma_pos() -> u32 {
    if !is_ready() { return 0; }
    unsafe {
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        let pos_idx = HDA_OUT_SD_IDX * 2;
        if pos_idx < 16 {
            let ram_pos = (*pool).dma_pos[pos_idx];
            let max_bytes = (BDL_SIZE * BDL_BUF_SIZE as usize) as u32;
            if ram_pos > 0 && ram_pos < max_bytes {
                return ram_pos;
            }
        }
        stream_lpib()
    }
}

#[inline]
pub unsafe fn stream_clear_sts() {
    // W1C: BCIS (bit2), FIFOE (bit4), DESCE (bit5)
    let sts = mmio_read8(HDA_MMIO_BASE, HDA_SD_BASE + SD_STS);
    let w1c = sts & 0x34;
    if w1c != 0 {
        mmio_write8(HDA_MMIO_BASE, HDA_SD_BASE + SD_STS, w1c);
    }
}

pub unsafe fn stream_stop() {
    let mmio = HDA_MMIO_BASE;
    let sd = HDA_SD_BASE;
    let ctl = mmio_read32(mmio, sd + SD_CTL);
    mmio_write32(mmio, sd + SD_CTL, ctl & !0x02); // RUN = 0
    let mut s = 0u32;
    while (mmio_read32(mmio, sd + SD_CTL) & 0x02) != 0 && s < 100_000 {
        s += 1;
        core::hint::spin_loop();
    }
}

pub unsafe fn stream_start() -> bool {
    let mmio = HDA_MMIO_BASE;
    let sd = HDA_SD_BASE;
    stream_clear_sts();
    // 32-bit atomik yazım: Stream Tag (bits 23:20 = 1), RUN=1 (bit 1), SRST=0 (bit 0)
    let ctl_val = ((OUT_STREAM_NUM as u32) << 20) | 0x02;
    mmio_write32(mmio, sd + SD_CTL, ctl_val);
    let mut s = 0u32;
    while (mmio_read32(mmio, sd + SD_CTL) & 0x02) == 0 && s < 100_000 {
        s += 1;
        core::hint::spin_loop();
    }
    (mmio_read32(mmio, sd + SD_CTL) & 0x02) != 0
}

pub unsafe fn ensure_stream_running() -> bool {
    let mmio = HDA_MMIO_BASE;
    let sd = HDA_SD_BASE;
    if (mmio_read32(mmio, sd + SD_CTL) & 0x02) == 0 {
        return stream_start();
    }
    true
}

// ---------------------------------------------------------------------------
// HDA Sürücü Başlatımı (Universal PCI + Robust Init)
// ---------------------------------------------------------------------------

pub fn init(pmm_dma_phys: Option<u64>) {
    unsafe {
        let phys = match pmm_dma_phys {
            Some(phys) => phys,
            None => {
                serial_print("[HDA] DMA havuzu ayrılamadı; sürücü başlatılmadı.\n");
                return;
            }
        };
        DMA_POOL_ADDR = phys;

        // 1. Evrensel PCI Taraması: Class 0x0403 (Audio Controller) veya bilinen aygıtlar
        let mut found: Option<(u8, u8, u8, u64)> = None;
        for bus in 0..4u8 {
            for dev in 0..32u8 {
                for func in 0..8u8 {
                    let id = pci_read32(bus, dev, func, 0x00);
                    if id == 0xFFFF_FFFF || id == 0 {
                        if func == 0 { break; }
                        continue;
                    }
                    let vid = (id & 0xFFFF) as u32;
                    let did = (id >> 16) & 0xFFFF;
                    let class_rev = pci_read32(bus, dev, func, 0x08);
                    let base_class = (class_rev >> 24) & 0xFF;
                    let sub_class = (class_rev >> 16) & 0xFF;

                    let is_hda = (base_class == 0x04 && sub_class == 0x03)
                        || (vid == 0x8086 && (did == 0x2668 || did == 0x293E || did == 0x1C20 || did == 0x1E20 || did == 0x8D20 || did == 0x9D70));

                    if is_hda {
                        let bar0_lo = pci_read32(bus, dev, func, 0x10);
                        let is_64 = (bar0_lo & 0x04) != 0;
                        let bar0_hi = if is_64 { pci_read32(bus, dev, func, 0x14) as u64 } else { 0 };
                        let base = (bar0_hi << 32) | (bar0_lo as u64 & 0xFFFF_FFF0);
                        found = Some((bus, dev, func, base));
                        break;
                    }
                }
                if found.is_some() { break; }
            }
            if found.is_some() { break; }
        }

        let (bus, dev, func, mmio_base_phys) = match found {
            Some(v) => v,
            None => {
                serial_print("[HDA] Intel HDA denetleyicisi PCI'da bulunamadı.\n");
                return;
            }
        };

        let mmio_base = crate::drivers::phys_to_virt(mmio_base_phys);
        crate::arch::x86_64::mmio_map_2mb(mmio_base_phys & !0x1F_FFFF);
        serial_print("[HDA] Intel HDA bulundu: PCI ");
        crate::serial::print_dec(bus as u64);
        serial_print(":");
        print_hex8(dev);
        serial_print(".");
        print_hex8(func);
        serial_print(" | BAR0: 0x");
        crate::serial::print_hex(mmio_base_phys);
        serial_print("\n");

        // PCI Command: Bus Master + MMIO enable
        let cmd = pci_read32(bus, dev, func, 0x04);
        pci_write32(bus, dev, func, 0x04, (cmd & 0xFFFF_FFF0) | 0x6);

        // 2. Global Denetleyici Sıfırlama (CRST: 0 -> bekle -> 1 -> bekle)
        let gctl = mmio_read32(mmio_base, GCTL);
        mmio_write32(mmio_base, GCTL, gctl & !0x1);
        let mut s = 0;
        while (mmio_read32(mmio_base, GCTL) & 0x1) != 0 && s < 100_000 {
            s += 1;
            core::hint::spin_loop();
        }
        for _ in 0..10_000 { core::hint::spin_loop(); }

        mmio_write32(mmio_base, GCTL, 0x1);
        s = 0;
        while (mmio_read32(mmio_base, GCTL) & 0x1) == 0 && s < 50_000 {
            s += 1;
            core::hint::spin_loop();
        }
        // Codec keşfi için spec gereği en az 521 mikrosaniye bekle
        for _ in 0..10_000 { core::hint::spin_loop(); }

        let statests = mmio_read16(mmio_base, STATESTS);
        let cad = if statests & 0x01 != 0 { 0 } else if statests & 0x02 != 0 { 1 } else { 0 };
        HDA_CAD = cad;
        mmio_write16(mmio_base, STATESTS, statests); // Temizle
        HDA_MMIO_BASE = mmio_base;

        // IRQ fırtınasını önle (minOs polling modunda çalışır, INTCTL temizle)
        mmio_write32(mmio_base, INTCTL, 0x0000_0000);

        // 3. DMA Havuzunu Sıfırla
        let pool_phys = DMA_POOL_ADDR;
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        (*pool).corb = [0; CORB_SIZE];
        (*pool).rirb = [0; RIRB_SIZE];
        (*pool).bdl = [HdaBdlEntry { address: 0, length: 0, ioc: 0 }; BDL_SIZE];
        (*pool).dma_pos = [0; 16];
        (*pool).pcm = [0; BDL_SIZE * BDL_BUF_SIZE as usize];
        (*pool).corb_wp = 0;
        (*pool).rirb_wp = 0;

        // 4. CORB / RIRB Kurulumu
        let corb_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, corb) as u64;
        let rirb_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, rirb) as u64;

        // CORB / RIRB durdur
        mmio_write8(mmio_base, CORB_CTL, 0x00);
        mmio_write8(mmio_base, RIRB_INTCTL, 0x00);
        for _ in 0..2_000 { core::hint::spin_loop(); }

        // CORB RP reset (bit 15 toggle)
        mmio_write16(mmio_base, CORB_RP, 0x8000);
        s = 0;
        while (mmio_read16(mmio_base, CORB_RP) & 0x8000) == 0 && s < 2_000 { s += 1; core::hint::spin_loop(); }
        mmio_write16(mmio_base, CORB_RP, 0x0000);
        s = 0;
        while (mmio_read16(mmio_base, CORB_RP) & 0x8000) != 0 && s < 2_000 { s += 1; core::hint::spin_loop(); }
        mmio_write16(mmio_base, CORB_WP, 0x0000);

        // RIRB WP reset
        mmio_write16(mmio_base, RIRB_WP, 0x8000);

        mmio_write32(mmio_base, CORB_BASE_LO, corb_phys as u32);
        mmio_write32(mmio_base, CORB_BASE_HI, (corb_phys >> 32) as u32);
        mmio_write32(mmio_base, RIRB_BASE_LO, rirb_phys as u32);
        mmio_write32(mmio_base, RIRB_BASE_HI, (rirb_phys >> 32) as u32);

        // Boyutlar: 256 girdi (0x02)
        mmio_write8(mmio_base, CORB_SIZE_REG, 0x02);
        mmio_write8(mmio_base, RIRB_SIZE_REG, 0x02);

        // CORB & RIRB DMA Başlat
        mmio_write8(mmio_base, CORB_CTL, 0x02);
        mmio_write8(mmio_base, RIRB_INTCTL, 0x02);

        // 5. Donanımsal DMA Position Buffer (DPLBASE / DPUBASE)
        let dma_pos_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, dma_pos) as u64;
        mmio_write32(mmio_base, DPLBASE, (dma_pos_phys as u32) | 1); // bit 0 = DMA Position Buffer Enable
        mmio_write32(mmio_base, DPUBASE, (dma_pos_phys >> 32) as u32);

        // 6. Codec Konfigürasyonu (AFG + DAC + Pin D0 / Unmute)
        send_verb(make_verb(cad, 0x01, 0x705, 0)); // AFG (Audio Function Group) D0 power
        send_verb(make_verb(cad, HDA_DAC_NID, 0x705, 0)); // DAC D0 power
        send_verb(make_verb(cad, HDA_DAC_NID, 0x706, (OUT_STREAM_NUM as u32) << 4)); // Stream 1, Ch 0
        send_verb(make_verb(cad, HDA_DAC_NID, 0x2, 0x0011)); // 48kHz, 16-bit, 2 kanal

        send_verb(make_verb(cad, HDA_OUT_PIN_NID, 0x705, 0)); // Pin D0 power
        send_verb(make_verb(cad, HDA_OUT_PIN_NID, 0x707, 0xC0)); // Output + Headphone enable
        send_verb(make_verb(cad, HDA_DAC_NID, 0x3, 0xB07F)); // DAC amfi ses açık + maksimum kazanç
        send_verb(make_verb(cad, HDA_OUT_PIN_NID, 0x3, 0xB07F)); // Pin amfi ses açık

        // 7. Output Stream 0 DMA Kurulumu
        let gcap_val = mmio_read16(mmio_base, GCAP);
        let iss = ((gcap_val >> 8) & 0x0F) as u32;
        let bss = ((gcap_val >> 3) & 0x1F) as u32;
        let sd_idx = (iss + bss) as usize;
        let sd = 0x80 + (sd_idx as u32) * 0x20;
        HDA_SD_BASE = sd;
        HDA_OUT_SD_IDX = sd_idx;

        // Stream Reset Protokolü (SRST: 1 -> bekle -> 0 -> bekle)
        let ctl = mmio_read32(mmio_base, sd + SD_CTL);
        mmio_write32(mmio_base, sd + SD_CTL, ctl & !0x02); // RUN = 0
        s = 0;
        while (mmio_read32(mmio_base, sd + SD_CTL) & 0x02) != 0 && s < 5_000 { s += 1; core::hint::spin_loop(); }

        mmio_write32(mmio_base, sd + SD_CTL, 0x01); // SRST = 1
        s = 0;
        while (mmio_read32(mmio_base, sd + SD_CTL) & 0x01) == 0 && s < 5_000 { s += 1; core::hint::spin_loop(); }

        mmio_write32(mmio_base, sd + SD_CTL, 0x00); // SRST = 0
        s = 0;
        while (mmio_read32(mmio_base, sd + SD_CTL) & 0x01) != 0 && s < 5_000 { s += 1; core::hint::spin_loop(); }

        stream_clear_sts();

        // BDL Kayıtları
        let bdl_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, bdl) as u64;
        let pcm_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, pcm) as u64;
        for i in 0..BDL_SIZE {
            (*pool).bdl[i].address = pcm_phys + (i as u64) * (BDL_BUF_SIZE as u64);
            (*pool).bdl[i].length = BDL_BUF_SIZE;
            (*pool).bdl[i].ioc = 1; // Buffer Completion Interrupt bayrağı
        }

        mmio_write32(mmio_base, sd + SD_CBL, BDL_BUF_SIZE * BDL_SIZE as u32);
        mmio_write16(mmio_base, sd + SD_LVI, (BDL_SIZE - 1) as u16);
        mmio_write16(mmio_base, sd + SD_FMT, 0x0011);
        mmio_write32(mmio_base, sd + SD_BDPL, bdl_phys as u32);
        mmio_write32(mmio_base, sd + SD_BDPU, (bdl_phys >> 32) as u32);

        // Stream Tag ata (bits 23:20 = 1). İlk push_pcm gelene kadar RUN=0 tutulur.
        let tag_only = (OUT_STREAM_NUM as u32) << 20;
        mmio_write32(mmio_base, sd + SD_CTL, tag_only);

        serial_print("[HDA] Intel HDA başlatıldı. 48kHz 16-bit Stereo DMA akışı hazır.\n");
        HDA_READY.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// PCM Gönderimi & Donanımsal Ses / Kazanç Kontrolü
// ---------------------------------------------------------------------------

pub fn push_pcm(samples: &[i16]) {
    if !is_ready() {
        return;
    }
    unsafe {
        ensure_stream_running();
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        let ring_len = (BDL_SIZE * BDL_BUF_SIZE as usize) / 2;

        let cur_pos = current_dma_pos();
        let last = LAST_DMA_POS.swap(cur_pos, Ordering::Relaxed);
        let sts = mmio_read8(HDA_MMIO_BASE, HDA_SD_BASE + SD_STS);

        if cur_pos != last || (sts & 0x04) != 0 {
            DMA_ADVANCES.fetch_add(1, Ordering::Relaxed);
            DMA_STALL_COUNT.store(0, Ordering::Relaxed);
            stream_clear_sts();
        } else {
            DMA_STALL_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        static mut WRITE_POS: usize = 0;
        for &s in samples {
            (*pool).pcm[WRITE_POS * 2] = (s & 0xFF) as u8;
            (*pool).pcm[WRITE_POS * 2 + 1] = ((s >> 8) & 0xFF) as u8;
            WRITE_POS = (WRITE_POS + 1) % ring_len;
        }
    }
}

pub fn silence() {
    if !is_ready() { return; }
    unsafe {
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        core::ptr::write_bytes((*pool).pcm.as_mut_ptr(), 0, (*pool).pcm.len());
    }
}

pub fn set_volume(vol: u8) {
    if !is_ready() { return; }
    unsafe {
        let cad = HDA_CAD;
        let gain = ((vol.min(100) as u32 * 127) / 100) & 0x7F;
        let payload = 0xB000 | gain; // Output Amp, sol & sağ kazanç
        send_verb(make_verb(cad, HDA_DAC_NID, 0x3, payload));
    }
}

pub fn set_muted(muted: bool) {
    if !is_ready() { return; }
    unsafe {
        let cad = HDA_CAD;
        let payload = if muted { 0xB080 } else { 0xB07F };
        send_verb(make_verb(cad, HDA_DAC_NID, 0x3, payload));
    }
}

// ---------------------------------------------------------------------------
// Test Tonu ve DMA İlerleme Doğrulayıcısı
// ---------------------------------------------------------------------------

pub fn play_test_tone() -> bool {
    if !is_ready() {
        return false;
    }
    const SINE_48: [i16; 48] = [
        0, 2088, 4141, 6122, 7999, 9740, 11313, 12693, 13856, 14782, 15454, 15863,
        16000, 15863, 15454, 14782, 13856, 12693, 11313, 9740, 7999, 6122, 4141, 2088,
        0, -2088, -4141, -6122, -7999, -9740, -11313, -12693, -13856, -14782, -15454, -15863,
        -16000, -15863, -15454, -14782, -13856, -12693, -11313, -9740, -8000, -6122, -4141, -2088,
    ];

    unsafe {
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        let total_frames = (BDL_SIZE * BDL_BUF_SIZE as usize) / 4;
        for i in 0..total_frames {
            let val = SINE_48[i % 48];
            (*pool).pcm[i * 4] = (val & 0xFF) as u8;
            (*pool).pcm[i * 4 + 1] = ((val >> 8) & 0xFF) as u8;
            (*pool).pcm[i * 4 + 2] = (val & 0xFF) as u8;
            (*pool).pcm[i * 4 + 3] = ((val >> 8) & 0xFF) as u8;
        }

        // Stream'i başlat
        stream_start();

        let sd = HDA_SD_BASE;
        let mmio = HDA_MMIO_BASE;

        let lpib1 = current_dma_pos();
        let start_ms = crate::monotonic_ms();
        let mut lpib2 = lpib1;
        let mut bcis_seen = false;

        // 35 ms boyunca donanımın örnekleri tüketmesini gözlemle
        while crate::monotonic_ms().saturating_sub(start_ms) < 35 {
            lpib2 = current_dma_pos();
            let sts = mmio_read8(mmio, sd + SD_STS);
            if (sts & 0x04) != 0 {
                bcis_seen = true;
                stream_clear_sts();
            }
            if lpib2 != lpib1 || bcis_seen {
                break;
            }
            core::hint::spin_loop();
        }

        let ok = lpib2 != lpib1 || bcis_seen;
        if ok {
            DMA_ADVANCES.fetch_add(1, Ordering::Relaxed);
            DMA_STALL_COUNT.store(0, Ordering::Relaxed);
            serial_print("[HDA-TEST] DMA İlerlemesi BAŞARILI (LPIB: ");
            crate::serial::print_dec(lpib1 as u64);
            serial_print("B -> ");
            crate::serial::print_dec(lpib2 as u64);
            serial_print("B, BCIS: ");
            serial_print(if bcis_seen { "1" } else { "0" });
            serial_print(")\n");
        } else {
            serial_print("[HDA-TEST] DMA İlerlemesi Beklemede / Sanallaştırma Gecikmeli\n");
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// Yardımcı Yazıcılar
// ---------------------------------------------------------------------------

fn pci_interrupt_line(dev: u8, func: u8) -> u16 {
    ((pci_read32(0, dev, func, 0x3C) >> 8) & 0xFF) as u16
}

fn print_hex8(v: u8) {
    let hex = b"0123456789ABCDEF";
    let hi = (v >> 4) & 0xF;
    let lo = v & 0xF;
    unsafe {
        asm!("out dx, al", in("dx") 0x3F8u16, in("al") hex[hi as usize], options(nomem, nostack, preserves_flags));
        asm!("out dx, al", in("dx") 0x3F8u16, in("al") hex[lo as usize], options(nomem, nostack, preserves_flags));
    }
}

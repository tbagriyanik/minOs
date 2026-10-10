#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel High Definition Audio (HDA) sürücüsü — QEMU `intel-hda` + `hda-duplex`
//! cihazıyla çalışacak şekilde yazılmıştır.
//!
//! QEMU tarafında: `-device intel-hda -device hda-duplex,audiodev=snd0`
//!
//! Bu sürücü:
//!   * PCI Konfigürasyon Alanı üzerinden (bus 0, slot herhangi) HDA denetleyicisini bulur
//!     (VENDOR 0x8086, DEVICE 0x2668 → QEMU'nun "Intel 82801/IQ57" modeli).
//!   * MMIO BAR0 üzerinden 512 byte HDA register bloğunu kullanır.
//!   * 1 çıkış (output) DMA stream tanımlar ve 4 Descriptor Buffer ile BDL kurar.
//!   * PCM örnek verisini 16-bit stereo 48kHz olarak besler.
//!
//! no_std, yapılabilir ama spinlock'lu polling kullanımıyla minimal tutulmuştur.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

fn serial_print(s: &str) {
    for b in s.bytes() {
        unsafe {
            asm!("out dx, al", in("dx") 0x3F8u16, in("al") b, options(nomem, nostack, preserves_flags));
        }
    }
}

// ---------------------------------------------------------------------------
// PCI ve MMIO yardımcıları
// ---------------------------------------------------------------------------

#[inline]
fn io_out16(port: u16, val: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags)); }
    ret
}

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

/// Intel HDA denetleyicisi PCI kimliği
const HDA_VENDOR: u32 = 0x8086;
const HDA_DEVICE: u32 = 0x2668; // 82801FB/FBM Intel HD Audio (QEMU'nun taklidi)

pub struct HdaController {
    /// MMIO temel adres (BAR0)
    mmio_base: u64,
    /// Bulunan PCI adres (bus, dev, func)
    pci_bus: u8,
    pci_dev: u8,
    pci_func: u8,
}

/// HDA Register bloğunun ana ofsetleri (BAR0 üzerinden)
const GCAP: u32 = 0x00; // Global Capabilities
const GCMD: u32 = 0x08; // PCI Control
const INTCTL: u32 = 0x0C;
const INTSTS: u32 = 0x0D;
const SSYNC: u32 = 0x38;

const CORB_BASE_LO: u32 = 0x40;
const CORB_BASE_HI: u32 = 0x44;
const CORB_WP: u32 = 0x48;
const CORB_RP: u32 = 0x4A;
const CORB_CTL: u32 = 0x4C;
const CORBSTS: u32 = 0x4D;

const RIRB_BASE_LO: u32 = 0x50;
const RIRB_BASE_HI: u32 = 0x54;
const RIRB_WP: u32 = 0x58;
const RIRB_INTCTL: u32 = 0x5C;

const ICOI: u32 = 0x60;
const ICII: u32 = 0x60;
const ICIS: u32 = 0x64;

const DPLBASE: u32 = 0x70;
const DPUBASE: u32 = 0x74;

// Output Stream 0 (CD) register blok taban ofseti.
// HDA'da stream descriptor 3 tiptir (Input, Output, Bidirectional).
// QEMU'nun `hda-duplex` düzeneğinde: streams 0..3 input, 4..7 output/bidir.
// 4. numaralı stream'i (index=4) output olarak kullanacağız.
const OUT_STREAM_BASE: u32 = 0x180 + 4 * 0x20;

const SD_CTL: u32 = 0x00;
const SD_STS: u32 = 0x03;
const SD_LPIB: u32 = 0x04;
const SD_CBL: u32 = 0x08;
const SD_LVI: u32 = 0x0C;
const SD_FIFOW: u32 = 0x0E;
const SD_FIFOS: u32 = 0x10;
const SD_FMT: u32 = 0x12;
const SD_BDPL: u32 = 0x18;
const SD_BDPU: u32 = 0x1C;

// Codec Node ID'leri (QEMU hda-duplex için standart)
const CODEC_ROOT_NID: u32 = 0x01; // AFG
const HDA_DAC_NID: u32 = 0x02; // QEMU'da hda-duplex çıkış DAC
const HDA_OUT_PIN_NID: u32 = 0x03; // QEMU'da hda-duplex çıkış pini (jack)

// Çıkış stream değeri (stream 4)
const OUT_STREAM_NUM: u8 = 4;
const OUT_STREAM_FMT: u16 = 0; // stream numarası + format üst 4 bit

// ---------------------------------------------------------------------------
// DMA Bellek Yardımcıları (fiziksel adresler — HHDM / fiziksel eşleme gerekir)
// ---------------------------------------------------------------------------

// Kernel'da fiziksel = sanal eşleme. Kernel 0xFFFF_8000_0000_0000 (HHDM) altında
// fiziksel alanın eşlenmesini varsayar. Kernel __map_1to1 yok, fiziksel == sanal
// doğrudan adresler QEMU'da çalışır; şu an gerçek page tablosu yok.
#[inline(always)]
fn phys_to_virt(p: u64) -> u64 {
    p // Basit eşleme — QEMU'da sayfa tablosu henüz yok
}

// ---------------------------------------------------------------------------
// CORB / RIRB komut kuyruğu
// ---------------------------------------------------------------------------

const CORB_SIZE: usize = 256; // u32 sayısı (1KB / 4B)
const RIRB_SIZE: usize = 256; // u64 sayısı (2KB / 8B)

// BDL (Buffer Descriptor List) 4 buffer * 16 byte
const BDL_SIZE: usize = 4;
const BDL_BUF_SIZE: u32 = 4096; // Her buffer 4KB (16-bit stereo 48kHz ~ 42ms)

// CORB buffer (u32 * 256 = 1KB)
// RIRB buffer (u64 * 256 = 2KB)
// BDL buffer (u16 * 4 = 64B)
// PCM buffers (4 * 4096 = 16KB)
#[repr(C, align(256))]
pub struct HdaDmaPool {
    pub corb: [u32; CORB_SIZE],
    pub rirb: [u64; RIRB_SIZE],
    pub bdl: [u64; BDL_SIZE * 2], // her BDL girişi: u64 addr + u32 len + u32 ioc = 16B
    pub pcm: [u8; BDL_SIZE * BDL_BUF_SIZE as usize],
    pub corb_wp: u16,
    pub rirb_wp: u16,
}

const DMA_POOL_SIZE: usize = core::mem::size_of::<HdaDmaPool>();

static mut DMA_POOL_BUF: () = ();
// DMA pool'u bilinen bir FİZİKSEL adrese koy: 0x300000 (3MB). HHDM üzerinden erişilir.
const DMA_POOL_PHYS: u64 = 0x0030_0000;
static mut DMA_POOL_ADDR: u64 = DMA_POOL_PHYS;

// DMA pool'u HHDM sanal adresini döndürür.
unsafe fn dma_pool_virt() -> u64 {
    crate::drivers::phys_to_virt(DMA_POOL_PHYS)
}

// ---------------------------------------------------------------------------
// Yüksek seviye HDA API
// ---------------------------------------------------------------------------

static HDA_READY: AtomicBool = AtomicBool::new(false);
static mut HDA_MMIO_BASE: u64 = 0;

pub fn is_ready() -> bool {
    HDA_READY.load(Ordering::Relaxed)
}

// CORB komut gönderme (verb)
unsafe fn send_verb(verb: u32) -> u64 {
    let mmio = HDA_MMIO_BASE;
    let pool = dma_pool_virt() as *mut HdaDmaPool;

    // CORB buffer'ın bir sonraki slotuna yaz
    let mut wp = (*pool).corb_wp;
    wp = (wp + 1) % (CORB_SIZE as u16);
    (*pool).corb[wp as usize] = verb;
    (*pool).corb_wp = wp;
    mmio_write16(mmio, CORB_WP, wp);

    // RIRB'den yanıt bekle (basit polling) — QEMU hızlı cevap verir, kısa timeout yeter
    let mut spins = 0u32;
    loop {
        let rp = mmio_read16(mmio, CORB_RP);
        if rp == wp {
            break;
        }
        spins += 1;
        if spins > 100_000 {
            break;
        }
        core::hint::spin_loop();
    }

    // RIRB yanıtını oku
    let mut response = 0u64;
    let mut rirb_spins = 0u32;
    loop {
        let rwp = mmio_read16(mmio, RIRB_WP);
        if rwp != (*pool).rirb_wp {
            (*pool).rirb_wp = (rirb_wp_inc((*pool).rirb_wp)) % (RIRB_SIZE as u16);
            let idx = (*pool).rirb_wp as usize;
            response = (*pool).rirb[idx];
            break;
        }
        rirb_spins += 1;
        if rirb_spins > 100_000 {
            break;
        }
        core::hint::spin_loop();
    }
    response
}

#[inline]
fn rirb_wp_inc(v: u16) -> u16 {
    v + 1
}

#[inline]
fn mmio_write16(base: u64, off: u32, val: u16) {
    unsafe { core::ptr::write_volatile((base + off as u64) as *mut u16, val) }
}

#[inline]
fn mmio_read16(base: u64, off: u32) -> u16 {
    unsafe { core::ptr::read_volatile((base + off as u64) as *const u16) }
}

/// Codec verb'i paketler (32-bit CORB girişi).
#[inline]
fn make_verb(cad: u8, nid: u32, verb: u32, payload: u32) -> u32 {
    ((cad as u32) << 28) | (nid << 20) | (verb << 8) | (payload & 0xFFFF)
}

// ---------------------------------------------------------------------------
// HDA Init
// ---------------------------------------------------------------------------

/// HDA denetleyicisini tespit eder, MMIO'yu açar, codec'ı konfigüre eder ve
/// PCM çıkışını stream 4 üzerinde hazır hale getirir.
pub fn init() {
    unsafe {
        // 1. PCI'da HDA denetleyicisini ara (bus 0'da)
        let mut found: Option<(u8, u8, u8, u64)> = None;
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let vid = (pci_read32(0, dev, func, 0x00) & 0xFFFF) as u32;
                let did = (pci_read32(0, dev, func, 0x00) >> 16) & 0xFFFF;
                if vid == HDA_VENDOR && did == HDA_DEVICE {
                    let bar0_lo = pci_read32(0, dev, func, 0x10);
                    // 32-bit MMIO BAR: yüksek 32 bit okunmaz (BAR1 ayrı BAR'dır)
                    let base: u64 = bar0_lo as u64 & 0xFFFF_FFF0;
                    found = Some((0, dev, func, base));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }

        let (bus, dev, func, mmio_base_phys) = match found {
            Some(v) => v,
            None => {
                serial_print("[HDA] Intel HDA denetleyicisi PCI'da bulunamadı.\n");
                return;
            }
        };
        let mmio_base = crate::drivers::phys_to_virt(mmio_base_phys);
        // Limine HHDM MMIO'yu map'lemediği için elle 2MB huge page ekliyoruz.
        crate::arch::x86_64::mmio_map_2mb(mmio_base_phys & !0x1F_FFFF);
        serial_print("[HDA] BAR0 fiziksel: ");
        print_hex8((mmio_base_phys >> 28) as u8);
        print_hex8((mmio_base_phys >> 24) as u8);
        print_hex8((mmio_base_phys >> 20) as u8);
        print_hex8((mmio_base_phys >> 16) as u8);
        serial_print("\n");

        serial_print("[HDA] Intel HDA bulundu. PCI 00:");
        print_hex8(dev);
        serial_print(".");
        print_hex8(func);
        serial_print("\n");

        // PCI Command: Bus Master + MMIO enable
        let cmd = pci_read32(bus, dev, func, 0x04);
        pci_write32(bus, dev, func, 0x04, (cmd & 0xFFFF_FFF0) | 0x6);
        serial_print("[HDA] PCI cmd ayarlandi\n");

        // Global Control: HDA'yı aç (CRST biti 0 → sonra 1 yapılır)
        let gctl = mmio_read32(mmio_base, 0x08);
        mmio_write32(mmio_base, 0x08, gctl | 0x1); // CRST = 1 (controller out of reset)
        // bekle
        let mut s = 0;
        while (mmio_read32(mmio_base, 0x08) & 0x1) == 0 && s < 1_000_000 {
            s += 1;
            core::hint::spin_loop();
        }
        serial_print("[HDA] CRST tamam\n");

        HDA_MMIO_BASE = mmio_base;

        // 2. DMA pool'u hazırla (bilinen fiziksel adres, HHDM ile erişilir)
        let pool_phys = DMA_POOL_PHYS;
        DMA_POOL_ADDR = pool_phys;
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        (*pool).corb = [0; CORB_SIZE];
        (*pool).rirb = [0; RIRB_SIZE];
        (*pool).bdl = [0; BDL_SIZE * 2];
        (*pool).pcm = [0; BDL_SIZE * BDL_BUF_SIZE as usize];
        (*pool).corb_wp = 0;
        (*pool).rirb_wp = 0;

        // 3. CORB/RIRB ayarla
        let corb_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, corb) as u64;
        let rirb_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, rirb) as u64;

        mmio_write32(mmio_base, CORB_BASE_LO, corb_phys as u32);
        mmio_write32(mmio_base, CORB_BASE_HI, (corb_phys >> 32) as u32);
        mmio_write32(mmio_base, RIRB_BASE_LO, rirb_phys as u32);
        mmio_write32(mmio_base, RIRB_BASE_HI, (rirb_phys >> 32) as u32);
        // CORB/RIRB boyut yazımı QEMU'da r/o uyarısı verir; yalnız kontrol bitleri:
        mmio_write8(mmio_base, CORB_CTL, 0x02); // CORB Run (DMA engelini kaldır)
        mmio_write8(mmio_base, RIRB_INTCTL, 0x05); // RIRB int enable, DMA enable

        // 4. Codec Komutları — QEMU hda-duplex için kod mux'u
        serial_print("[HDA] codec verb'leri gonderiliyor\n");
        // DAC D0 conversion format: 48kHz, 16-bit stereo (0x0011)
        send_verb(make_verb(0, HDA_DAC_NID, 0x2, 0)); // set power state D0
        send_verb(make_verb(0, HDA_DAC_NID, 0x412, OUT_STREAM_NUM as u32)); // converter format stream num
        send_verb(make_verb(0, HDA_DAC_NID, 0x413, 0)); // stream channel
        // Converter format 0x0011: 48kHz (0x41 << 14 ...) 16-bit, 2 chan
        // QEMU sadece format demeti: 0x0011 = 48k? Aslında AC_FMT değeri
        // 16-bit stereo 48kHz: 0xC011 = (48k=0x41<<14)|(16-bit)|(2 ch)
        send_verb(make_verb(0, HDA_DAC_NID, 0x2, 0xC011));

        // Pin widget konfigüre (jeton: enable output, unmute)
        send_verb(make_verb(0, HDA_OUT_PIN_NID, 0x3, 0xE000)); // amp mute off, gain max
        send_verb(make_verb(0, HDA_OUT_PIN_NID, 0x707, 0)); // pin ctrl — unset
        send_verb(make_verb(0, HDA_OUT_PIN_NID, 0x707, 0xC0)); // out enable + HP out

        // DAC ampı aç
        send_verb(make_verb(0, HDA_DAC_NID, 0x3, 0xB000 | (0x3F as u32))); // gain
        send_verb(make_verb(0, HDA_DAC_NID, 0x35B, 0)); // amp left set

        // 5. Output Stream 0 DMA kurulumu
        let sd = OUT_STREAM_BASE;
        let bdl_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, bdl) as u64;
        let pcm_phys = pool_phys + core::mem::offset_of!(HdaDmaPool, pcm) as u64;

        // BDL girişlerini doldur: 4 buffer, her biri 4096 byte
        for i in 0..BDL_SIZE {
            let entry_off = i * 2;
            (*pool).bdl[entry_off] = pcm_phys + (i as u64) * (BDL_BUF_SIZE as u64); // buffer addr
            let packed: u64 = ((BDL_BUF_SIZE as u64) << 32) | 1; // length | IOC=1
            (*pool).bdl[entry_off + 1] = packed;
        }

        // Stream'i durdur
        mmio_write8(mmio_base, sd + SD_CTL, 0x0);
        let mut s2 = 0;
        while (mmio_read8(mmio_base, sd + SD_CTL) & 0x02) != 0 && s2 < 100_000 {
            s2 += 1;
            core::hint::spin_loop();
        }

        // CBL, LVI, FMT
        mmio_write32(mmio_base, sd + SD_CBL, BDL_BUF_SIZE * BDL_SIZE as u32);
        mmio_write16(mmio_base, sd + SD_LVI, (BDL_SIZE - 1) as u16);
        mmio_write16(mmio_base, sd + SD_FMT, 0x0011); // 16-bit, 48kHz, 2ch
        mmio_write32(mmio_base, sd + SD_BDPL, bdl_phys as u32);
        mmio_write32(mmio_base, sd + SD_BDPU, (bdl_phys >> 32) as u32);

        // Stream Run: 1 stripe, RUN biti açılır — taşınabilir
        mmio_write8(mmio_base, sd + SD_CTL, 0x1C); // 4 stripe, RUN=0
        mmio_write8(mmio_base, sd + SD_CTL, 0x02); // RUN=1

        serial_print("[HDA] HDA denetleyicisi hazir. Codec konfigurasyonu tamam.\n");
        HDA_READY.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// PCM Örnek Gönderimi
// ---------------------------------------------------------------------------

/// PCM tamponunu (16-bit stereo, 48kHz) HDA çıkış stream'ine yazar.
/// `sample` 16-bit i16 L/R çiftleri olarak yorumlanır.
pub fn push_pcm(samples: &[i16]) {
    if !is_ready() {
        return;
    }
    unsafe {
        let pool = dma_pool_virt() as *mut HdaDmaPool;
        let ring_len = (BDL_SIZE * BDL_BUF_SIZE as usize) / 2; // i16 sayısı
        static mut WRITE_POS: usize = 0;
        for &s in samples {
            (*pool).pcm[WRITE_POS * 2] = (s & 0xFF) as u8;
            (*pool).pcm[WRITE_POS * 2 + 1] = ((s >> 8) & 0xFF) as u8;
            WRITE_POS = (WRITE_POS + 1) % ring_len;
        }
    }
}

// ---------------------------------------------------------------------------
// Yardımcı: Serial hex yazdırma
// ---------------------------------------------------------------------------

fn print_hex8(v: u8) {
    let hex = b"0123456789ABCDEF";
    let hi = (v >> 4) & 0xF;
    let lo = v & 0xF;
    serial_print_byte(hex[hi as usize]);
    serial_print_byte(hex[lo as usize]);
}

fn serial_print_byte(b: u8) {
    unsafe {
        asm!("out dx, al", in("dx") 0x3F8u16, in("al") b, options(nomem, nostack, preserves_flags));
    }
}

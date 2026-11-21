#![allow(dead_code, unused_variables, static_mut_refs)]

//! AHCI (Advanced Host Controller Interface) SATA Driver
//!
//! AHCI, modern SATA denetleyicileri için standart bir arayüzdür.
//! ATA yerine daha gelişmiş özellikler (NCQ, hot-plug, port multipliers) sunar.
//!
//! TR: AHCI, modern SATA denetleyicileri için standart bir arayüzdür.
//! ATA yerine daha gelişmiş özellikler (NCQ, hot-plug, port multipliers) sunar.

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
    let _guard = crate::drivers::port_io::PCI_CONFIG_LOCK.lock();
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
    let _guard = crate::drivers::port_io::PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out32(PCI_CONFIG_DATA, val);
}

// ---------------------------------------------------------------------------
// AHCI PCI Class Kodları
// ---------------------------------------------------------------------------

const PCI_CLASS_SATA: u8 = 0x01;
const PCI_SUBCLASS_AHCI: u8 = 0x06;

// ---------------------------------------------------------------------------
// AHCI HBA (Host Bus Adapter) Kayıt Ofsetleri
// ---------------------------------------------------------------------------

const HBA_CAP: u32 = 0x00; // Host Capabilities
const HBA_GHC: u32 = 0x04; // Global Host Control
const HBA_IS: u32 = 0x08; // Interrupt Status
const HBA_PI: u32 = 0x0C; // Ports Implemented
const HBA_VS: u32 = 0x10; // Version

// GHC bitleri
const GHC_AE: u32 = 1 << 31; // AHCI Enable
const GHC_MRSM: u32 = 1 << 0; // MSI Revert to Single Message

// ---------------------------------------------------------------------------
// AHCI Port Kayıt Ofsetleri
// ---------------------------------------------------------------------------

const PORT_CLB: u32 = 0x00; // Command List Base
const PORT_CLB_U: u32 = 0x04; // Command List Base Upper
const PORT_FB: u32 = 0x08; // FIS Base
const PORT_FB_U: u32 = 0x0C; // FIS Base Upper
const PORT_IS: u32 = 0x10; // Interrupt Status
const PORT_IE: u32 = 0x14; // Interrupt Enable
const PORT_CMD: u32 = 0x18; // Command and Status
const PORT_TFD: u32 = 0x20; // Task File Data
const PORT_SIG: u32 = 0x24; // Signature
const PORT_SSTS: u32 = 0x28; // SATA Status
const PORT_SCTL: u32 = 0x2C; // SATA Control
const PORT_SERR: u32 = 0x30; // SATA Error
const PORT_CI: u32 = 0x38; // Command Issue

// PORT_CMD bitleri
const PORT_CMD_ST: u32 = 1 << 0; // Start
const PORT_CMD_SUD: u32 = 1 << 1; // Spin Up Device
const PORT_CMD_POD: u32 = 1 << 2; // Power On Device
const PORT_CMD_CLO: u32 = 1 << 3; // Command List Override
const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
const PORT_CMD_CR: u32 = 1 << 15; // Command List Running
const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Running
const PORT_CMD_ICC_MASK: u32 = 0xF << 28; // Interface Communication Control

// PORT_TFD bitleri (ATA durum baytı: ERR=bit0, DF=bit5)
const PORT_TFD_BSY: u8 = 1 << 7; // Busy
const PORT_TFD_DRQ: u8 = 1 << 3; // Data Request
const PORT_TFD_ERR: u8 = 1 << 0;
const PORT_TFD_DF: u8 = 1 << 5;

// ATA komutları (LBA48)
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_SECTOR_SIZE: usize = 512;

// PORT_SSTS bitleri
const PORT_SSTS_DET_MASK: u8 = 0x3; // Device Detection
const PORT_SSTS_DET_PRESENT: u8 = 0x3; // Device present

// ---------------------------------------------------------------------------
// Command Header Yapısı
// ---------------------------------------------------------------------------

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct CommandHeader {
    prdtl: u16, // Physical Region Descriptor Table Length
    prdbc: u16, // Physical Region Descriptor Byte Count
    ctba: u32,  // Command Table Descriptor Base Address
    ctbau: u32, // Command Table Descriptor Base Address Upper
    reserved: [u32; 4],
}

// ---------------------------------------------------------------------------
// FIS (Frame Information Structure) Türleri
// ---------------------------------------------------------------------------

const FIS_TYPE_REG_H2D: u8 = 0x27; // Register Host to Device
const FIS_TYPE_REG_D2H: u8 = 0x34; // Register Device to Host
const FIS_TYPE_DMA_ACT: u8 = 0x39; // DMA Activate
const FIS_TYPE_DMA_SETUP: u8 = 0x41; // DMA Setup
const FIS_TYPE_DATA: u8 = 0x46; // Data
const FIS_TYPE_BIST: u8 = 0x58; // BIST
const FIS_TYPE_PIO_SETUP: u8 = 0x5F; // PIO Setup
const FIS_TYPE_DEV_BITS: u8 = 0xA1; // Device Bits

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct FisRegH2D {
    fis_type: u8,
    pm_port: u8,
    command: u8,
    feature: u8,
    lba0: u8,
    lba1: u8,
    lba2: u8,
    device: u8,
    lba3: u8,
    lba4: u8,
    lba5: u8,
    feature_low: u8,
    count: u16,
    icc: u8,
    control: u8,
    reserved: [u8; 4],
}

// ---------------------------------------------------------------------------
// PRDT (Physical Region Descriptor Table)
// ---------------------------------------------------------------------------

/// PRDT girdisi 16 bayttır: 64-bit veri taban adresi, 32-bit rezerve ve
/// 32-bit bayt sayacı (alt 22 bit = bayt-1, bit31 = tamamlanma kesmesi).
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct PrdtEntry {
    dba: u64,
    reserved: u32,
    dbc: u32,
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut ABAR: u64 = 0; // HHDM sanal adresindeki AHCI Base Memory Register
static mut DETECTED: bool = false;
static mut PORT_COUNT: u32 = 0;
static mut PORT_BASE: u64 = 0;
static mut DMA_PHYS: u64 = 0; // Command List + Command Table + FIS alanı (fiziksel)
static mut INITIALIZED: bool = false;

/// DMA bölgesi yerleşimi (tek bitişik 8 KiB, 2 frame):
///   +0x000 Command List (32 × 32B başlık; yalnızca slot 0 kullanılır)
///   +0x400 Command Table (CFIS 64B + ACMD 16B + ayrılmış 48B + PRDT)
///   +0x800 FIS Receive alanı (256B)
const DMA_FRAMES: usize = 2;
const CLB_OFF: u64 = 0x000;
const CT_OFF: u64 = 0x400;
const FIS_OFF: u64 = 0x800;
const SLOT: u32 = 0;

#[inline]
fn dma_virt() -> u64 {
    unsafe { crate::drivers::phys_to_virt(DMA_PHYS) }
}

/// HHDM sanal adresini fiziksel adrese çevirir (kernel belleği
/// doğrudan eşlemeli olduğu için geçerlidir).
#[inline]
fn virt_to_phys(vaddr: u64) -> u64 {
    vaddr.wrapping_sub(crate::drivers::phys_to_virt(0))
}

// ---------------------------------------------------------------------------
// MMIO Yardımcı Fonksiyonları
// ---------------------------------------------------------------------------

#[inline]
fn hba_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (ABAR + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn hba_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (ABAR + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
}

#[inline]
fn port_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (PORT_BASE + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn port_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (PORT_BASE + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
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
                    let class = pci_read32(bus, dev, func, 0x08) >> 24;
                    let subclass = (pci_read32(bus, dev, func, 0x08) >> 16) & 0xFF;

                    if class as u8 == PCI_CLASS_SATA && subclass as u8 == PCI_SUBCLASS_AHCI {
                        // ABAR adresini al (BAR5)
                        let bar5 = pci_read32(bus, dev, func, 0x24);
                        if bar5 & 0x1 == 0 {
                            let abar_phys = (bar5 & 0xFFFF_FFF0) as u64;
                            crate::arch::x86_64::mmio_map_2mb(abar_phys & !0x1F_FFFF);
                            ABAR = crate::drivers::phys_to_virt(abar_phys);
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
        if INITIALIZED {
            return true;
        }
        if !DETECTED {
            return false;
        }

        // AHCI'yi etkinleştir
        let mut ghc = hba_read32(HBA_GHC);
        ghc |= GHC_AE;
        hba_write32(HBA_GHC, ghc);

        // Portları oku
        let pi = hba_read32(HBA_PI);
        PORT_COUNT = pi.count_ones();

        // İlk aktif portu seç
        for i in 0..32 {
            if pi & (1 << i) != 0 {
                PORT_BASE = ABAR + 0x100 + (i as u64 * 0x80);
                break;
            }
        }

        // Portu başlat
        // Stop command engine
        let mut cmd = port_read32(PORT_CMD);
        cmd &= !(PORT_CMD_ST | PORT_CMD_FRE);
        port_write32(PORT_CMD, cmd);

        // Wait for stop
        for _ in 0..100000 {
            if port_read32(PORT_CMD) & (PORT_CMD_CR | PORT_CMD_FR) == 0 {
                break;
            }
        }

        // FIS ve Command List belleğini ayır (PMM bitişik çerçeveler).
        DMA_PHYS = crate::drivers::dma::alloc_dma_frames(DMA_FRAMES).unwrap_or(0);
        if DMA_PHYS == 0 {
            crate::drivers::status::note_error(
                crate::drivers::status::DriverId::Ahci,
                crate::drivers::status::ERR_NO_DMA,
            );
            return false;
        }
        // DMA bölgesini sıfırla (Command Header/Table/FIS artıkları kalmasın).
        let dma_v = dma_virt() as *mut u8;
        core::ptr::write_bytes(dma_v, 0, DMA_FRAMES * 4096);

        // CLB / FIS taban adreslerini porta yaz.
        let clb = DMA_PHYS + CLB_OFF;
        let fis = DMA_PHYS + FIS_OFF;
        port_write32(PORT_CLB, clb as u32);
        port_write32(PORT_CLB_U, (clb >> 32) as u32);
        port_write32(PORT_FB, fis as u32);
        port_write32(PORT_FB_U, (fis >> 32) as u32);

        // Hata ve kesme durumlarını temizle.
        port_write32(PORT_SERR, 0xFFFF_FFFF);
        port_write32(PORT_IS, 0xFFFF_FFFF);

        // Start command engine
        cmd = port_read32(PORT_CMD);
        cmd |= PORT_CMD_ST | PORT_CMD_FRE;
        port_write32(PORT_CMD, cmd);

        INITIALIZED = true;
        true
    }
}

/// Port hazır ve komut motoru çalışıyor mu?
pub fn ready() -> bool {
    unsafe { DETECTED && INITIALIZED && DMA_PHYS != 0 }
}

// ---------------------------------------------------------------------------
// Komut Yürütme ve Sektör I/O
// ---------------------------------------------------------------------------

/// `buf`'u sayfa sınırlarına göre fiziksel olarak bitişik parçalara böler
/// ve PRDT girdilerini `prdt`'ye yazar. En fazla `max` girdi.
/// Dönen: kullanılan girdi sayısı.
fn build_prdt(buf_ptr: *const u8, len: usize, prdt: *mut PrdtEntry, max: usize) -> usize {
    let mut entries = 0usize;
    let mut off = 0usize;
    while off < len && entries < max {
        let cur = unsafe { buf_ptr.add(off) };
        let page_off = (cur as u64) & 0xFFF;
        let chunk = (4096 - page_off as usize).min(len - off);
        let phys = virt_to_phys(cur as u64);
        unsafe {
            let e = &mut *prdt.add(entries);
            e.dba = phys;
            e.reserved = 0;
            // DBC: bayt-1 (alt 22 bit); son girdide tamamlanma kesmesi.
            let last = off + chunk >= len || entries + 1 >= max;
            e.dbc = ((chunk as u32 - 1) & 0x3F_FFFF) | if last { 1 << 31 } else { 0 };
        }
        entries += 1;
        off += chunk;
    }
    entries
}

/// Tek bir ATA komutunu (LBA48, `sector_count` sektör) çalıştırır.
/// `is_write=true` → H2D (cihaz veri yazar), false → D2H (okur).
/// `device_reg`: FIS device baytı (sektör komutları 0x40, IDENTIFY 0xA0).
fn issue_sector_command(
    ata_cmd: u8,
    lba: u64,
    buf: *mut u8,
    len: usize,
    is_write: bool,
    device_reg: u8,
    sector_count: u16,
) -> bool {
    unsafe {
        if DMA_PHYS == 0 || PORT_BASE == 0 {
            return false;
        }

        let dma_v = dma_virt();
        let clb = (dma_v + CLB_OFF) as *mut u8;
        let ct = (dma_v + CT_OFF) as *mut u8;

        // Command Table: CFIS (64B) + ACMD (16B) + ayrılmış (48B) + PRDT
        core::ptr::write_bytes(ct, 0, 0x400);

        // CFIS — Register H2D FIS (32 bayt anlamlı, 64B alan)
        let cfis = ct;
        *cfis = FIS_TYPE_REG_H2D;
        *cfis.add(1) = 0; // PM port + control
        *cfis.add(2) = ata_cmd;
        *cfis.add(3) = 0; // feature
        *cfis.add(4) = lba as u8;
        *cfis.add(5) = (lba >> 8) as u8;
        *cfis.add(6) = (lba >> 16) as u8;
        *cfis.add(7) = device_reg;
        *cfis.add(8) = (lba >> 24) as u8;
        *cfis.add(9) = (lba >> 32) as u8;
        *cfis.add(10) = (lba >> 40) as u8;
        *cfis.add(11) = 0; // feature_low
        *cfis.add(12) = sector_count as u8;
        *cfis.add(13) = (sector_count >> 8) as u8;
        *cfis.add(14) = 0; // icc
        *cfis.add(15) = 0; // control

        // PRDT: 0x80 ofsetinden itibaren
        let prdt = ct.add(0x80) as *mut PrdtEntry;
        let prdtl = build_prdt(buf as *const u8, len, prdt, 56);
        if prdtl == 0 {
            return false;
        }

        // Command Header (slot 0): DW0 flags + prdtl + prdbc + ctba.
        // DW0 bayt0: CFL[4:0]=5, W[6] (H2D yazma), P[7] (prefetch).
        // Yazma: 0xC5, okuma: 0x85. Bayt1 (Reset alanı) 0 kalır.
        let hdr = clb;
        core::ptr::write_bytes(hdr, 0, 32);
        *hdr = if is_write { 0xC5 } else { 0x85 };
        (hdr.add(2) as *mut u16).write_unaligned(prdtl as u16);
        // DW1 prdbc = 0 (yazılırken güncellenir)
        let ct_phys = DMA_PHYS + CT_OFF;
        (hdr.add(8) as *mut u32).write_unaligned(ct_phys as u32);
        (hdr.add(12) as *mut u32).write_unaligned((ct_phys >> 32) as u32);

        // Port durumunu sıfırla ve komutu kuyruğa al.
        port_write32(PORT_IS, 0xFFFF_FFFF);
        port_write32(PORT_CI, 1 << SLOT);

        // Tamamlanmayı bekle: CI biti düşer veya IS.DHRS (bit0) set olur.
        let mut done = false;
        for _ in 0..1_000_000 {
            if port_read32(PORT_IS) & (1 << SLOT) != 0 {
                done = true;
                break;
            }
            if port_read32(PORT_CI) & (1 << SLOT) == 0 {
                done = true;
                break;
            }
        }
        if !done {
            port_write32(PORT_CI, 0);
            crate::drivers::status::note_error(
                crate::drivers::status::DriverId::Ahci,
                crate::drivers::status::ERR_TIMEOUT,
            );
            return false;
        }

        // Hata denetimi: TFD ERR/DF bitleri ve SERR.
        let tfd = port_read32(PORT_TFD) as u8;
        let serr = port_read32(PORT_SERR);
        if (tfd & (PORT_TFD_ERR | PORT_TFD_DF)) != 0 || serr != 0 {
            crate::drivers::status::note_error(
                crate::drivers::status::DriverId::Ahci,
                crate::drivers::status::ERR_INIT_FAILED,
            );
            return false;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn name() -> &'static str {
    "AHCI SATA"
}

pub fn name_tr() -> &'static str {
    "AHCI SATA"
}

pub fn port_count() -> u32 {
    unsafe { PORT_COUNT }
}

/// Sektör okuma (512 bayt, LBA48 + DMA PRDT)
pub fn read_sector(lba: u64, buffer: &mut [u8]) -> bool {
    unsafe {
        if !DETECTED || buffer.len() < ATA_SECTOR_SIZE {
            return false;
        }
        issue_sector_command(
            ATA_CMD_READ_DMA_EXT,
            lba,
            buffer.as_mut_ptr(),
            ATA_SECTOR_SIZE,
            false,
            0x40,
            1,
        )
    }
}

/// ATA IDENTIFY DEVICE (0xEC) ile 512 baytlık tanımlama bloğunu okur.
pub fn identify_device(buf: &mut [u8; ATA_SECTOR_SIZE]) -> bool {
    if !ready() {
        return false;
    }
    issue_sector_command(0xEC, 0, buf.as_mut_ptr(), ATA_SECTOR_SIZE, false, 0xA0, 0)
}

/// LBA48 sektör kapasitesini IDENTIFY DEVICE'tan çözer.
/// WORD 100-103: 28-bit max; WORD 120-125: 48-bit max LBA.
pub fn capacity_sectors() -> u64 {
    let mut id = [0u8; ATA_SECTOR_SIZE];
    if !identify_device(&mut id) {
        return 0;
    }
    let word = |w: usize| -> u64 {
        let lo = id[w * 2] as u64;
        let hi = id[w * 2 + 1] as u64;
        lo | (hi << 8)
    };
    let lba28 = ((word(60) << 16) | word(61)) & 0x0FFF_FFFF;
    // LBA48: word 100 = düşük 16 bit, 101-103 = sonraki 48 bit
    let lba48_low = word(100);
    let lba48 = lba48_low | (word(101) << 16) | (word(102) << 32) | (word(103) << 48);
    if lba48 > 0 && lba48 != u64::MAX {
        lba48
    } else {
        lba28
    }
}

/// Sektör yazma (512 bayt, LBA48 + DMA PRDT)
pub fn write_sector(lba: u64, buffer: &[u8]) -> bool {
    unsafe {
        if !DETECTED || buffer.len() < ATA_SECTOR_SIZE {
            return false;
        }
        issue_sector_command(
            ATA_CMD_WRITE_DMA_EXT,
            lba,
            buffer.as_ptr() as *mut u8,
            ATA_SECTOR_SIZE,
            true,
            0x40,
            1,
        )
    }
}

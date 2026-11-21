#![allow(dead_code, unused_variables, static_mut_refs)]

//! SD/MMC Card Driver - SD kart ve MMC kart desteği
//!
//! Bu driver, SD kartları ve MMC kartları destekler. SDHCI (Secure Digital Host
//! Controller Interface) üzerinden SD/MMC kartlarıyla iletişim kurar.
//!
//! TR: Bu sürücü, SD kartları ve MMC kartları destekler. SDHCI (Secure Digital
//! Host Controller Interface) üzerinden SD/MMC kartlarıyla iletişim kurar.

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

// ---------------------------------------------------------------------------
// SDHCI PCI Class Kodları
// ---------------------------------------------------------------------------

const PCI_CLASS_SYSTEM: u8 = 0x05;
const PCI_SUBCLASS_SDHCI: u8 = 0x08;

// ---------------------------------------------------------------------------
// SDHCI Kayıt Ofsetleri
// ---------------------------------------------------------------------------

const SDHCI_DMA_ADDRESS: u32 = 0x00;
const SDHCI_BLOCK_SIZE: u32 = 0x04;
const SDHCI_BLOCK_COUNT: u32 = 0x06;
const SDHCI_ARGUMENT: u32 = 0x08;
const SDHCI_TRANSFER_MODE: u32 = 0x0C;
const SDHCI_COMMAND: u32 = 0x0E;
const SDHCI_RESPONSE: u32 = 0x10;
const SDHCI_BUFFER: u32 = 0x20;
const SDHCI_PRESENT_STATE: u32 = 0x24;
const SDHCI_HOST_CONTROL: u32 = 0x28;
const SDHCI_POWER_CONTROL: u32 = 0x29;
const SDHCI_CLOCK_CONTROL: u32 = 0x2C;
const SDHCI_TIMEOUT_CONTROL: u32 = 0x2E;
const SDHCI_SOFTWARE_RESET: u32 = 0x2F;
const SDHCI_INT_STATUS: u32 = 0x30;
const SDHCI_INT_ENABLE: u32 = 0x34;
const SDHCI_SIGNAL_ENABLE: u32 = 0x38;
const SDHCI_ACMD12_ERR: u32 = 0x3C;
const SDHCI_CAPABILITIES: u32 = 0x40;
const SDHCI_MAX_CURRENT: u32 = 0x48;
const SDHCI_SLOT_INT_STATUS: u32 = 0xFC;

// SDHCI_COMMAND (0x0E, 16-bit) alanları — SD Host Controller
// basitleştirilmiş spesifikasyonuna göre: RESPTYP bit1:0, CCRCCEN bit3,
// CIDXCEN bit4, DPSEL bit5, CMDTYP bit7:6, CMDIDX bit13:8.
const SDHCI_CMD_INDEX_SHIFT: u16 = 8;
const SDHCI_CMD_TYPE_SHIFT: u16 = 6;
const SDHCI_CMD_TYPE_NORMAL: u16 = 0;
const SDHCI_CMD_TYPE_SUSPEND: u16 = 1;
const SDHCI_CMD_TYPE_RESUME: u16 = 2;
const SDHCI_CMD_TYPE_ABORT: u16 = 3;
const SDHCI_CMD_DATA_PRESENT: u16 = 1 << 5;
const SDHCI_CMD_INDEX_CHECK: u16 = 1 << 4;
const SDHCI_CMD_CRC_CHECK: u16 = 1 << 3;
/// Yanıt tipi seçimi (bit 1:0): 00=yok, 01=136-bit, 10=48-bit,
/// 11=48-bit + meşgul bekleme.
const SDHCI_RESP_NONE: u16 = 0;
const SDHCI_RESP_R136: u16 = 1;
const SDHCI_RESP_R48: u16 = 2;
const SDHCI_RESP_R48_BUSY: u16 = 3;

// SDHCI_PRESENT_STATE (0x24) bitleri
const SDHCI_PRS_CMD_INHIBIT: u32 = 1 << 0;
const SDHCI_PRS_DAT_INHIBIT: u32 = 1 << 1;
const SDHCI_PRS_BUF_READ_ENABLE: u32 = 1 << 2;
const SDHCI_PRS_BUF_WRITE_ENABLE: u32 = 1 << 3;

// SDHCI_INT_STATUS (0x30) bitleri
const SDHCI_INT_CMD_COMPLETE: u32 = 1 << 0;
const SDHCI_INT_XFER_COMPLETE: u32 = 1 << 1;
// SDHCI_INT_STATUS hata bitleri: bit15 genel hata, 16-22 komut/veri
// hataları (timeout, CRC, end-bit, index).
const SDHCI_INT_ERROR_MASK: u32 = 0x007F_8000;

// ---------------------------------------------------------------------------
// SDHCI Komutları
// ---------------------------------------------------------------------------

// SD Komutları
const SD_CMD_GO_IDLE_STATE: u8 = 0;
const SD_CMD_SEND_OP_COND: u8 = 1;
const SD_CMD_ALL_SEND_CID: u8 = 2;
const SD_CMD_SEND_RELATIVE_ADDR: u8 = 3;
const SD_CMD_SEND_IF_COND: u8 = 8;
const SD_CMD_SEND_CSD: u8 = 9;
const SD_CMD_SEND_CID: u8 = 10;
const SD_CMD_SELECT_CARD: u8 = 7;
const SD_CMD_STOP_TRANSMISSION: u8 = 12;
const SD_CMD_SEND_STATUS: u8 = 13;
const SD_CMD_SET_BLOCKLEN: u8 = 16;
const SD_CMD_READ_SINGLE_BLOCK: u8 = 17;
const SD_CMD_READ_MULTIPLE_BLOCK: u8 = 18;
const SD_CMD_WRITE_BLOCK: u8 = 24;
const SD_CMD_WRITE_MULTIPLE_BLOCK: u8 = 25;
const SD_CMD_APP_CMD: u8 = 55;

// SD Application Komutları
const SD_ACMD_SEND_OP_COND: u8 = 41;
const SD_ACMD_SET_BUS_WIDTH: u8 = 6;

// ---------------------------------------------------------------------------
// SDHCI Transfer Mode
// ---------------------------------------------------------------------------

const SDHCI_TRNS_DMA: u16 = 1 << 0;
const SDHCI_TRNS_BLK_CNT_EN: u16 = 1 << 1;
const SDHCI_TRNS_AUTO_CMD12: u16 = 1 << 2;
const SDHCI_TRNS_BLK_CNT_EN_MASK: u16 = 1 << 1;
const SDHCI_TRNS_READ: u16 = 1 << 4;
const SDHCI_TRNS_MULTI: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// SD Kart Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardType {
    None,
    SDSC, // Standard Capacity (≤2GB)
    SDHC, // High Capacity (≤32GB)
    SDXC, // Extended Capacity (≤2TB)
    MMC,
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut MMIO_BASE: u64 = 0;
static mut DETECTED: bool = false;
static mut CARD_TYPE: CardType = CardType::None;
static mut CARD_CAPACITY: u64 = 0;
static mut RCA: u16 = 0; // Relative Card Address
static mut OCR: u32 = 0; // Operating Condition Register

// ---------------------------------------------------------------------------
// MMIO Yardımcı Fonksiyonları
// ---------------------------------------------------------------------------

#[inline]
fn sdhci_read8(offset: u32) -> u8 {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *const u8;
        ptr.read_volatile()
    }
}

#[inline]
fn sdhci_write8(offset: u32, val: u8) {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *mut u8;
        ptr.write_volatile(val);
    }
}

#[inline]
fn sdhci_read16(offset: u32) -> u16 {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *const u16;
        ptr.read_volatile()
    }
}

#[inline]
fn sdhci_write16(offset: u32, val: u16) {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *mut u16;
        ptr.write_volatile(val);
    }
}

#[inline]
fn sdhci_read32(offset: u32) -> u32 {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *const u32;
        ptr.read_volatile()
    }
}

#[inline]
fn sdhci_write32(offset: u32, val: u32) {
    unsafe {
        let ptr = (MMIO_BASE + offset as u64) as *mut u32;
        ptr.write_volatile(val);
    }
}

// ---------------------------------------------------------------------------
// Komut Gönderme
// ---------------------------------------------------------------------------

/// SDHCI komut gönderir; R1 yanıtının düşük 32 biti (SDHCI_RESPONSE)
/// döner. Yanıtsız komutlarda 0 döner. Veri fazı ayrı yürütülür
/// (`transfer_data`); burada yalnızca komut + yanıt tamamlanır.
fn send_command_ex(cmd: u8, arg: u32, resp: u16, data_present: bool) -> Option<u32> {
    let mut cmd_reg = (cmd as u16) << SDHCI_CMD_INDEX_SHIFT;
    cmd_reg |= resp & 0x3;

    if data_present {
        cmd_reg |= SDHCI_CMD_DATA_PRESENT;
    }

    cmd_reg |= SDHCI_CMD_TYPE_NORMAL;
    cmd_reg |= SDHCI_CMD_CRC_CHECK;
    cmd_reg |= SDHCI_CMD_INDEX_CHECK;

    // Bekleyen kesme durumlarını temizle.
    sdhci_write32(SDHCI_INT_STATUS, 0xFFFF_FFFF);

    sdhci_write32(SDHCI_ARGUMENT, arg);
    sdhci_write16(SDHCI_COMMAND, cmd_reg);

    // Komutun bitmesini bekle. Veri fazlı komutlarda DAT inhibit veri
    // aktarımı bitene dek kalır; yalnızca CMD inhibit beklenir.
    let inhibit_mask = if data_present {
        SDHCI_PRS_CMD_INHIBIT
    } else {
        SDHCI_PRS_CMD_INHIBIT | SDHCI_PRS_DAT_INHIBIT
    };
    let mut ok = false;
    for _ in 0..1_000_000 {
        let state = sdhci_read32(SDHCI_PRESENT_STATE);
        if state & inhibit_mask == 0 {
            ok = true;
            break;
        }
    }
    if !ok {
        return None;
    }

    let ints = sdhci_read32(SDHCI_INT_STATUS);
    if ints & SDHCI_INT_ERROR_MASK != 0 {
        sdhci_write32(SDHCI_INT_STATUS, ints);
        return None;
    }

    if resp == SDHCI_RESP_NONE {
        None
    } else {
        Some(sdhci_read32(SDHCI_RESPONSE))
    }
}

/// Geriye uyumlu sarmalayıcı: veri fazsız komutlar için.
fn send_command(cmd: u8, arg: u32, data_present: bool) -> u32 {
    let resp = if cmd == SD_CMD_GO_IDLE_STATE {
        SDHCI_RESP_NONE
    } else {
        SDHCI_RESP_R48
    };
    send_command_ex(cmd, arg, resp, data_present).unwrap_or(0)
}

/// Tek blokluk PIO veri aktarımı (SDHCI DMA'sız buffer yolu).
///
/// `cmd` veri fazı başlatan komuttur (CMD9/CMD17/CMD24). Blok boyutu
/// `block_len` bayttır; `buf` en az `block_len` bayt olmalıdır.
/// SDHCI_BUFFER 32-bit register'larından PIO ile okunur/yazılır;
/// SD baytları MSB-öncelikli geldiği için her u32 big-endian saklanır.
fn transfer_data(cmd: u8, arg: u32, block_len: u16, buf: &mut [u8], is_write: bool) -> bool {
    sdhci_write32(SDHCI_DMA_ADDRESS, 0);
    sdhci_write16(SDHCI_BLOCK_SIZE, block_len);
    sdhci_write16(SDHCI_BLOCK_COUNT, 1);
    let trns = if !is_write { SDHCI_TRNS_READ } else { 0 };
    sdhci_write16(SDHCI_TRANSFER_MODE, trns);

    if send_command_ex(cmd, arg, SDHCI_RESP_R48, true).is_none() {
        crate::drivers::status::note_error(
            crate::drivers::status::DriverId::Sdmmc,
            crate::drivers::status::ERR_TIMEOUT,
        );
        return false;
    }

    let words = (block_len as usize) / 4;
    for i in 0..words {
        // Buffer Read/Write Enable beklenir; veri fazı tek blok için
        // sınırlı sürer, hata durumunda INT_STATUS üst bitleri dolar.
        let flag = if is_write {
            SDHCI_PRS_BUF_WRITE_ENABLE
        } else {
            SDHCI_PRS_BUF_READ_ENABLE
        };
        let mut ready = false;
        for _ in 0..1_000_000 {
            if sdhci_read32(SDHCI_PRESENT_STATE) & flag != 0 {
                ready = true;
                break;
            }
            if sdhci_read32(SDHCI_INT_STATUS) & SDHCI_INT_ERROR_MASK != 0 {
                break;
            }
        }
        if !ready {
            sdhci_write32(SDHCI_INT_STATUS, 0xFFFF_FFFF);
            crate::drivers::status::note_error(
                crate::drivers::status::DriverId::Sdmmc,
                crate::drivers::status::ERR_TIMEOUT,
            );
            return false;
        }

        let word = if is_write {
            let mut b = [0u8; 4];
            b.copy_from_slice(&buf[i * 4..i * 4 + 4]);
            u32::from_be_bytes(b)
        } else {
            sdhci_read32(SDHCI_BUFFER)
        };
        if is_write {
            sdhci_write32(SDHCI_BUFFER, word);
        } else {
            buf[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
    }

    // Transfer Complete (veya Block Gap) kesmesini bekle.
    let mut done = false;
    for _ in 0..1_000_000 {
        let ints = sdhci_read32(SDHCI_INT_STATUS);
        if ints & SDHCI_INT_ERROR_MASK != 0 {
            sdhci_write32(SDHCI_INT_STATUS, ints);
            crate::drivers::status::note_error(
                crate::drivers::status::DriverId::Sdmmc,
                crate::drivers::status::ERR_INIT_FAILED,
            );
            return false;
        }
        if ints & (SDHCI_INT_XFER_COMPLETE | SDHCI_INT_CMD_COMPLETE) != 0 {
            done = true;
            sdhci_write32(SDHCI_INT_STATUS, ints);
            break;
        }
    }
    done
}

/// CSD'nin `start..start+len` bitlik alanını little-endian bit
/// numaralandırmasıyla (bit0 = 128-bit değerin LSB'si) okur.
fn csd_bits(csd: &[u8; 16], start: u32, len: u32) -> u32 {
    let mut v = 0u32;
    for i in 0..len {
        let bit = start + i;
        let byte = 15 - (bit / 8) as usize;
        let bpos = (bit % 8) as u8;
        v |= (((csd[byte] >> bpos) & 1) as u32) << i;
    }
    v
}

/// CSD yapısına göre kart kapasitesini (bayt) hesaplar.
///
/// CSD v1 (SDSC): blok sayısı = (C_SIZE+1) * 2^(C_SIZE_MULT+2),
/// blok boyutu = 2^READ_BL_LEN bayt (typik 9 → 512).
/// CSD v2 (SDHC/SDXC): kapasite = (C_SIZE+1) * 512 KiB.
fn csd_capacity(csd: &[u8; 16]) -> u64 {
    let structure = csd[0] >> 6; // CSD_STRUCTURE: bit 127..126
    if structure == 0 {
        // v1 (SDSC)
        let c_size = csd_bits(csd, 62, 12);
        let c_size_mult = csd_bits(csd, 47, 3);
        let read_bl_len = csd_bits(csd, 80, 4);
        let blocks = (c_size as u64 + 1) << (c_size_mult + 2);
        let block_len = 1u64 << read_bl_len;
        blocks * block_len
    } else {
        // v2 (SDHC/SDXC): C_SIZE bitleri 48..69 (22 bit)
        let c_size = csd_bits(csd, 48, 22) as u64;
        (c_size + 1) * 512 * 1024
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

                    if class as u8 == PCI_CLASS_SYSTEM && subclass as u8 == PCI_SUBCLASS_SDHCI {
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

        // Software reset
        sdhci_write8(SDHCI_SOFTWARE_RESET, 0x01);
        for _ in 0..10000 {
            if sdhci_read8(SDHCI_SOFTWARE_RESET) & 0x01 == 0 {
                break;
            }
        }

        // Voltaj ayarla (3.3V)
        sdhci_write8(SDHCI_POWER_CONTROL, 0x0F);
        for _ in 0..10000 {
            if sdhci_read8(SDHCI_POWER_CONTROL) & 0x80 != 0 {
                break;
            }
        }

        // Clock başlat
        sdhci_write16(SDHCI_CLOCK_CONTROL, 0x0000);
        sdhci_write16(SDHCI_CLOCK_CONTROL, 0x0002); // Internal clock enable
        for _ in 0..10000 {
            if sdhci_read16(SDHCI_CLOCK_CONTROL) & 0x0002 != 0 {
                break;
            }
        }
        sdhci_write16(SDHCI_CLOCK_CONTROL, 0x0006); // Clock stable

        // Kart reset
        send_command(SD_CMD_GO_IDLE_STATE, 0, false);

        // SD versiyonu kontrol et
        send_command(SD_CMD_SEND_IF_COND, 0x000001AA, false);

        // ACMD41 ile OCR al
        let mut ocr = 0;
        for _ in 0..10000 {
            send_command(SD_CMD_APP_CMD, 0, false);
            ocr = send_command(SD_ACMD_SEND_OP_COND, 0x40FF8000, false);
            if ocr & 0x80000000 != 0 {
                break;
            }
        }
        OCR = ocr;

        // Kart tipini belirle: OCR bit22 (CCS) SDHC/SDXC gösterir.
        if OCR & (1 << 22) != 0 {
            CARD_TYPE = CardType::SDHC;
        } else {
            CARD_TYPE = CardType::SDSC;
        }

        // RCA al
        let rca = send_command(SD_CMD_SEND_RELATIVE_ADDR, 0, false);
        RCA = (rca >> 16) as u16;

        // CMD7 SELECT_CARD: RCA'yı seç (veri fazı yok).
        if send_command_ex(
            SD_CMD_SELECT_CARD,
            (RCA as u32) << 16,
            SDHCI_RESP_R48,
            false,
        )
        .is_none()
        {
            return false;
        }

        // CSD oku (16 bayt veri fazı) ve kapasiteyi hesapla.
        let mut csd = [0u8; 16];
        if transfer_data(SD_CMD_SEND_CSD, (RCA as u32) << 16, 16, &mut csd, false) {
            CARD_CAPACITY = csd_capacity(&csd);
            // SDXC, SDHC ile aynı CSD v2 formatını kullanır; 32 GiB
            // üstü kartlar SDXC olarak işaretlenir.
            if CARD_CAPACITY > 32 * 1024 * 1024 * 1024 {
                CARD_TYPE = CardType::SDXC;
            }
        } else {
            // CSD okunamazsa kapasite bilinmiyor; 0 bırakılır.
            CARD_CAPACITY = 0;
        }

        // Blok uzunluğunu 512 bayt olarak sabitle (SDSC için gerekli).
        send_command_ex(SD_CMD_SET_BLOCKLEN, 512, SDHCI_RESP_R48, false).is_some()
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn name() -> &'static str {
    "SD/MMC Card"
}

pub fn name_tr() -> &'static str {
    "SD/MMC Kart"
}

pub fn card_type() -> CardType {
    unsafe { CARD_TYPE }
}

pub fn capacity() -> u64 {
    unsafe { CARD_CAPACITY }
}

/// LBA → komut argümanı. SDSC bayt adresi, SDHC/SDXC blok numarası kullanır.
fn block_arg(lba: u64) -> u32 {
    unsafe {
        if CARD_TYPE == CardType::SDSC {
            (lba.saturating_mul(512)) as u32
        } else {
            lba as u32
        }
    }
}

/// Sektör okuma (512 bayt, PIO veri yolu)
pub fn read_sector(lba: u64, buffer: &mut [u8]) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }
        if buffer.len() < 512 {
            return false;
        }
        if CARD_CAPACITY != 0 && lba.saturating_mul(512) >= CARD_CAPACITY {
            return false;
        }
        transfer_data(SD_CMD_READ_SINGLE_BLOCK, block_arg(lba), 512, buffer, false)
    }
}

/// Sektör yazma (512 bayt, PIO veri yolu)
pub fn write_sector(lba: u64, buffer: &[u8]) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }
        if buffer.len() < 512 {
            return false;
        }
        if CARD_CAPACITY != 0 && lba.saturating_mul(512) >= CARD_CAPACITY {
            return false;
        }
        let mut tmp = [0u8; 512];
        tmp.copy_from_slice(&buffer[..512]);
        transfer_data(SD_CMD_WRITE_BLOCK, block_arg(lba), 512, &mut tmp, true)
    }
}

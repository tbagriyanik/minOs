#![allow(dead_code)]

//! Basit ATA/ATAPI (PATA) sürücüsü — PIO modunda 28-bit LBA okuma.
//!
//! Amaç: birincil kanalda bulunan USB depolama aygıtını (QEMU `usb-storage`)
//! ve ATAPI CD-ROM sürücüsünü okuyabilmek. Bu, MinOS'a USB bellek içeriğini
//! dosya listesinde gösterebilme yeteneği kazandırır.
//!
//! * Master (sürücü 0) → USB bellek / FAT32 bölümü
//! * Slave  (sürücü 1) → ATAPI CD-ROM
//!
//! Disk erişimi yalnızca PIO (polling) kullanır; DMA ve kesme yoktur.

use core::arch::asm;

// ---------------------------------------------------------------------------
// I/O portları (birincil kanal)
// ---------------------------------------------------------------------------
const IO_DATA: u16 = 0x1F0;
const IO_ERROR: u16 = 0x1F1;
const IO_SECCOUNT: u16 = 0x1F2;
const IO_LBA_LO: u16 = 0x1F3;
const IO_LBA_MID: u16 = 0x1F4;
const IO_LBA_HI: u16 = 0x1F5;
const IO_DRIVE: u16 = 0x1F6;
const IO_STATUS: u16 = 0x1F7;
const IO_COMMAND: u16 = 0x1F7;
const IO_ALT_STATUS: u16 = 0x3F6;
const IO_CONTROL: u16 = 0x3F6;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_READ_SECTORS_EXT: u8 = 0x24;
const CMD_IDENTIFY: u8 = 0xEC;
const CMD_PACKET: u8 = 0xA0;
const CMD_IDENTIFY_PACKET: u8 = 0xA1;

const ST_BSY: u8 = 0x80;
const ST_DRDY: u8 = 0x40;
const ST_DRQ: u8 = 0x08;
const ST_ERR: u8 = 0x01;

const SECTOR_SIZE: usize = 512;

/// Bir LBA'nın MAC adresi gibi dönen "failed packet" durumunu engellemek için
/// PIO döngülerinde üst sınır.
const SPIN_LIMIT: usize = 2_000_000;

// ---------------------------------------------------------------------------
// Port yardımcıları
// ---------------------------------------------------------------------------

#[inline]
fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe { asm!("in al, dx", in("dx") port, out("al") v, options(nomem, nostack, preserves_flags)); }
    v
}

#[inline]
fn outb(port: u16, v: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn inw(port: u16) -> u16 {
    let v: u16;
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") v, options(nomem, nostack, preserves_flags)); }
    v
}

#[inline]
fn outw(port: u16, v: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags)); }
}

/// 400ns ATA gecikmesi: alternatif durum portunu 4 kez oku.
#[inline]
fn io_delay() {
    for _ in 0..4 {
        let _ = inb(IO_ALT_STATUS);
    }
}

// ---------------------------------------------------------------------------
// Aygıt durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    None,
    Ata,
    Atapi,
}

#[derive(Clone, Copy)]
pub struct Device {
    pub kind: DeviceKind,
    /// ATA için toplam sektör sayısı (512 B).
    pub sectors: u64,
    /// ATA için model adı (40 bayt, ASCII).
    pub model: [u8; 40],
    pub model_len: usize,
}

impl Device {
    const fn empty() -> Self {
        Self { kind: DeviceKind::None, sectors: 0, model: [0; 40], model_len: 0 }
    }
}

static mut DEV0: Device = Device::empty();
static mut DEV1: Device = Device::empty();
static mut INITIALIZED: bool = false;
/// CD-ROM'dan okunan son ISOLINUX/El Torito sektör bilgisi (teşhis için).
static mut LAST_DISK_ERROR: u8 = 0;

fn serial(s: &str) {
    for b in s.bytes() {
        unsafe { asm!("out dx, al", in("dx") 0x3F8u16, in("al") b, options(nomem, nostack, preserves_flags)); }
    }
}

fn print_sectors(mut v: u64) {
    if v == 0 {
        serial("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
    }
    while n > 0 {
        n -= 1;
        let b = buf[n];
        serial(core::str::from_utf8(&[b]).unwrap_or("?"));
    }
}

// ---------------------------------------------------------------------------
// Aygıt seçimi ve yoklama
// ---------------------------------------------------------------------------

fn select(drive: u8) {
    // LBA biti (0x40) + master/slave seçimi
    outb(IO_DRIVE, 0xA0 | ((drive & 1) << 4));
    io_delay();
}

/// Seçili aygıtın hazır olmasını bekler (BSY temiz + DRDY set).
fn wait_ready() -> bool {
    let mut spins = 0;
    loop {
        let status = inb(IO_STATUS);
        if status & ST_BSY == 0 && status & ST_DRDY != 0 {
            return true;
        }
        if status & ST_ERR != 0 {
            return false;
        }
        spins += 1;
        if spins > SPIN_LIMIT {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// DRQ (veri hazır) bekler; hata durumunda son hata kodunu saklar.
fn wait_drq() -> bool {
    let mut spins = 0;
    loop {
        let status = inb(IO_STATUS);
        if status & ST_BSY == 0 {
            if status & ST_DRQ != 0 {
                return true;
            }
            if status & ST_ERR != 0 {
                unsafe { LAST_DISK_ERROR = inb(IO_ERROR); }
                return false;
            }
        }
        spins += 1;
        if spins > SPIN_LIMIT {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// ATA IDENTIFY komutu ile aygıt bilgisini doldurur.
fn identify(drive: u8, packet: bool) -> Device {
    let mut dev = Device::empty();
    select(drive);
    outb(IO_SECCOUNT, 0);
    outb(IO_LBA_LO, 0);
    outb(IO_LBA_MID, 0);
    outb(IO_LBA_HI, 0);
    outb(IO_COMMAND, if packet { CMD_IDENTIFY_PACKET } else { CMD_IDENTIFY });
    io_delay();

    let status = inb(IO_STATUS);
    // ATA/ATAPI ayrımı: hiç aygıt yoksa status 0 (ya da 0xFF) okunur.
    if status == 0 || status == 0xFF {
        return dev;
    }
    if !wait_drq() {
        return dev;
    }

    let mut data = [0u16; 256];
    for word in data.iter_mut() {
        *word = inw(IO_DATA);
    }

    // Kelime 0: 0x8000 | 0xC33 olmalı (atanmış bir ATA/ATAPI aygıtı)
    if data[0] != 0x8000 && data[0] != 0xC33 && data[0] != 0x848A {
        // Bazı aygıtlar farklı imza verir; yine de devam et.
    }

    dev.kind = if packet { DeviceKind::Atapi } else { DeviceKind::Ata };

    if !packet {
        // Kelime 60/61: 28-bit sektör sayısı (düşük/yüksek kelime)
        let lo = data[60] as u64;
        let hi = data[61] as u64;
        dev.sectors = (hi << 16) | lo;
        // Kelime 83 bit 10 → 48-bit LBA desteği
        let caps = data[83];
        if caps & (1 << 10) != 0 {
            let lba48 = [
                data[100] as u64,
                data[101] as u64,
                data[102] as u64,
                data[103] as u64,
            ];
            let total = lba48[0] | (lba48[1] << 16) | (lba48[2] << 32) | (lba48[3] << 48);
            if total > dev.sectors {
                dev.sectors = total;
            }
        }
    }

    // Kelime 27..46: model adı (byte-swap gerekir)
    let mut n = 0;
    for w in 27..47 {
        let word = data[w];
        let a = (word >> 8) as u8;
        let b = (word & 0xFF) as u8;
        if n < 40 && a != 0 {
            dev.model[n] = a;
            n += 1;
        }
        if n < 40 && b != 0 {
            dev.model[n] = b;
            n += 1;
        }
    }
    // Sondaki boşlukları kırp
    while n > 0 && dev.model[n - 1] == b' ' {
        n -= 1;
    }
    dev.model_len = n;
    dev
}

// ---------------------------------------------------------------------------
// Genel API
// ---------------------------------------------------------------------------

pub fn init() {
    unsafe {
        // Kesmeleri kapat (polling kullanıyoruz)
        outb(IO_CONTROL, 0x02); // nIEN
        if INITIALIZED {
            return;
        }
        DEV0 = identify(0, false);
        if DEV0.kind == DeviceKind::None {
            // Master bir ATAPI (CD-ROM) olabilir
            DEV0 = identify(0, true);
        }
        DEV1 = identify(1, false);
        if DEV1.kind == DeviceKind::None {
            DEV1 = identify(1, true);
        }

        serial("[ATA] kanal taramasi:\n");
        serial("[ATA] master tur: ");
        serial(match DEV0.kind {
            DeviceKind::None => "yok",
            DeviceKind::Ata => "ATA",
            DeviceKind::Atapi => "ATAPI",
        });
        serial("  sektor: ");
        print_sectors(DEV0.sectors);
        serial("  model: ");
        let m = core::str::from_utf8(&DEV0.model[..DEV0.model_len]).unwrap_or("");
        serial(m);
        serial("\n[ATA] slave tur: ");
        serial(match DEV1.kind {
            DeviceKind::None => "yok",
            DeviceKind::Ata => "ATA",
            DeviceKind::Atapi => "ATAPI",
        });
        serial("  model: ");
        let m1 = core::str::from_utf8(&DEV1.model[..DEV1.model_len]).unwrap_or("");
        serial(m1);
        serial("\n");

        INITIALIZED = true;
    }
}

/// Master sürücünün türü.
pub fn master_kind() -> DeviceKind {
    unsafe { DEV0.kind }
}

/// Slave sürücünün türü.
pub fn slave_kind() -> DeviceKind {
    unsafe { DEV1.kind }
}

/// Master ATA aygıtının toplam sektör sayısı (512 B).
pub fn master_sector_count() -> u64 {
    unsafe { DEV0.sectors }
}

/// Bir aygıtın model adı (kesme olmadan güvenli erişim).
pub fn model_of(drive: u8) -> &'static [u8] {
    unsafe {
        let dev = if drive == 0 { &*(&raw const DEV0) } else { &*(&raw const DEV1) };
        &dev.model[..dev.model_len]
    }
}

pub fn last_error() -> u8 {
    unsafe { LAST_DISK_ERROR }
}

/// `drive` aygıtından `lba` sektörünü `buf` (512 bayt) içine okur.
pub fn read_sector(drive: u8, lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    unsafe {
        let dev = if drive == 0 { &*(&raw const DEV0) } else { &*(&raw const DEV1) };
        if dev.kind != DeviceKind::Ata {
            return false;
        }
    }
    for attempt in 0..3 {
        if read_sector_once(drive, lba, buf) {
            return true;
        }
        if attempt < 2 {
            // Yeniden dene: hata kaydını temizle ve 400ns bekle
            let _ = inb(IO_ERROR);
            io_delay();
        }
    }
    false
}

fn read_sector_once(drive: u8, lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    select(drive);
    if !wait_ready() {
        return false;
    }
    outb(IO_SECCOUNT, 1);
    outb(IO_LBA_LO, (lba & 0xFF) as u8);
    outb(IO_LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(IO_LBA_HI, ((lba >> 16) & 0xFF) as u8);
    outb(IO_DRIVE, 0xE0 | ((drive & 1) << 4) | (((lba >> 24) & 0x0F) as u8));
    outb(IO_COMMAND, CMD_READ_SECTORS);
    io_delay();

    if !wait_drq() {
        return false;
    }
    for i in 0..SECTOR_SIZE / 2 {
        let word = inw(IO_DATA);
        buf[i * 2] = (word & 0xFF) as u8;
        buf[i * 2 + 1] = (word >> 8) as u8;
    }
    io_delay();
    true
}

/// Arka arkaya `count` sektör okur; okunan sektör sayısını döner.
pub fn read_sectors(drive: u8, lba: u32, count: usize, buf: &mut [u8]) -> usize {
    let max = buf.len() / SECTOR_SIZE;
    let n = count.min(max);
    let mut sector = [0u8; SECTOR_SIZE];
    for i in 0..n {
        if !read_sector(drive, lba + i as u32, &mut sector) {
            return i;
        }
        buf[i * SECTOR_SIZE..(i + 1) * SECTOR_SIZE].copy_from_slice(&sector);
    }
    n
}

// ---------------------------------------------------------------------------
// ATAPI (SFF-8020i) — 12 baytlık paket komutları
// ---------------------------------------------------------------------------

const CMD_READ_10: u8 = 0x28;
const CMD_READ_CAPACITY: u8 = 0x25;

/// ATAPI aygıta paket komut gönderir ve `buf` ile veri alır.
fn atapi_packet(drive: u8, packet: &[u8; 12], buf: &mut [u8]) -> bool {
    select(drive);
    if !wait_ready() {
        return false;
    }
    outb(IO_SECCOUNT, 0);
    outb(IO_LBA_LO, (buf.len() & 0xFF) as u8);
    outb(IO_LBA_MID, ((buf.len() >> 8) & 0xFF) as u8);
    outb(IO_LBA_HI, 0);
    outb(IO_COMMAND, CMD_PACKET);
    io_delay();

    if !wait_drq() {
        return false;
    }
    for i in 0..6 {
        let word = ((packet[i * 2] as u16) << 8) | packet[i * 2 + 1] as u16;
        outw(IO_DATA, word);
    }
    io_delay();

    if !wait_drq() {
        return false;
    }
    for i in 0..buf.len() / 2 {
        let word = inw(IO_DATA);
        buf[i * 2] = (word & 0xFF) as u8;
        buf[i * 2 + 1] = (word >> 8) as u8;
    }
    io_delay();
    true
}

/// ATAPI aygıtın sektör sayısını (2048 B) döner; başarısızlıkta None.
pub fn atapi_capacity(drive: u8) -> Option<u32> {
    let packet = [CMD_READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut data = [0u8; 8];
    if !atapi_packet(drive, &packet, &mut data) {
        return None;
    }
    // Son LBA (big-endian) + blok uzunluğu
    let last = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    Some(last.saturating_add(1))
}

/// ATAPI aygıttan 2048 baytlık bir blok okur (ISO9660 için).
pub fn atapi_read_block(drive: u8, lba: u32, buf: &mut [u8; 2048]) -> bool {
    let packet = [
        CMD_READ_10,
        0,
        ((lba >> 24) & 0xFF) as u8,
        ((lba >> 16) & 0xFF) as u8,
        ((lba >> 8) & 0xFF) as u8,
        (lba & 0xFF) as u8,
        0,
        0,
        1, // transfer length = 1 blok
        0,
        0,
        0,
    ];
    for attempt in 0..3 {
        if atapi_packet(drive, &packet, buf) {
            return true;
        }
        if attempt < 2 {
            let _ = inb(IO_ERROR);
            io_delay();
        }
    }
    false
}

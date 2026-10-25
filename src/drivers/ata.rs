#![allow(dead_code, static_mut_refs)]

//! ATA/ATAPI (PATA/IDE) ve Depolama Sürücüsü — PIO Modunda LBA28/LBA48 Okuma & Yazma.
//!
//! Bu sürücü Primary (0x1F0) ve Secondary (0x170) kanallarındaki tüm aygıtları tarar:
//! - Sürücü 0: Primary Master   (HDD veya USB Mass Storage)
//! - Sürücü 1: Primary Slave    (HDD veya ATAPI CD-ROM)
//! - Sürücü 2: Secondary Master (HDD veya ATAPI CD-ROM)
//! - Sürücü 3: Secondary Slave  (HDD veya ATAPI CD-ROM)
//!
//! Desteklenen Özellikler:
//! * Sektör Okuma (CMD_READ_SECTORS - 0x20)
//! * Sektör Yazma (CMD_WRITE_SECTORS - 0x30)
//! * Önbellek Boşaltma (CMD_FLUSH_CACHE - 0xE7)
//! * ATAPI Paket Okuma (SFF-8020i / SCSI 10-byte READ - 0x28)
//! * Aygıt Tanımlama (HDD, USB Flash, CD/DVD-ROM ayrımı)

use core::arch::asm;

// ---------------------------------------------------------------------------
// Komut ve Durum Sabitleri
// ---------------------------------------------------------------------------
pub const CMD_READ_SECTORS: u8 = 0x20;
pub const CMD_WRITE_SECTORS: u8 = 0x30;
pub const CMD_IDENTIFY: u8 = 0xEC;
pub const CMD_PACKET: u8 = 0xA0;
pub const CMD_IDENTIFY_PACKET: u8 = 0xA1;
pub const CMD_FLUSH_CACHE: u8 = 0xE7;

pub const ST_BSY: u8 = 0x80;
pub const ST_DRDY: u8 = 0x40;
pub const ST_DRQ: u8 = 0x08;
pub const ST_ERR: u8 = 0x01;

pub const SECTOR_SIZE: usize = 512;
const SPIN_LIMIT: usize = 2_000_000;

// ---------------------------------------------------------------------------
// Port Seçimi (Primary ve Secondary Kanal Desteği)
// ---------------------------------------------------------------------------

#[inline]
pub fn io_ports_for_drive(drive: u8) -> (u16, u16) {
    if drive < 2 {
        (0x1F0, 0x3F6)
    } else {
        (0x170, 0x376)
    }
}

#[inline]
fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") v, options(nomem, nostack, preserves_flags));
    }
    v
}

#[inline]
fn outb(port: u16, v: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn inw(port: u16) -> u16 {
    let v: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") v, options(nomem, nostack, preserves_flags));
    }
    v
}

#[inline]
fn outw(port: u16, v: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn io_delay(ctrl: u16) {
    for _ in 0..4 {
        let _ = inb(ctrl);
    }
}

// ---------------------------------------------------------------------------
// Aygıt Türleri ve Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    None,
    Ata,
    Atapi,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveRole {
    Unknown,
    HardDisk,
    UsbStorage,
    CdRom,
}

#[derive(Clone, Copy)]
pub struct Device {
    pub kind: DeviceKind,
    pub role: DriveRole,
    pub sectors: u64,
    pub model: [u8; 40],
    pub model_len: usize,
}

impl Device {
    pub const fn empty() -> Self {
        Self {
            kind: DeviceKind::None,
            role: DriveRole::Unknown,
            sectors: 0,
            model: [0; 40],
            model_len: 0,
        }
    }

    pub fn model_str(&self) -> &str {
        core::str::from_utf8(&self.model[..self.model_len]).unwrap_or("")
    }

    pub fn size_mb(&self) -> u64 {
        if self.kind == DeviceKind::Atapi {
            (self.sectors * 2048) / (1024 * 1024)
        } else {
            (self.sectors * 512) / (1024 * 1024)
        }
    }
}

pub static mut DEVICES: [Device; 4] = [Device::empty(); 4];
static mut INITIALIZED: bool = false;
static mut LAST_DISK_ERROR: u8 = 0;
static mut IO_PROGRESS_HOOK: Option<fn(*mut (), usize, usize)> = None;
static mut IO_PROGRESS_CONTEXT: *mut () = core::ptr::null_mut();
static mut SINGLE_SECTOR_WRITES: usize = 0;

/// Register a temporary UI progress callback for long multi-sector I/O.
pub fn set_io_progress_hook(hook: Option<fn(*mut (), usize, usize)>, context: *mut ()) {
    unsafe {
        IO_PROGRESS_HOOK = hook;
        IO_PROGRESS_CONTEXT = context;
    }
}

fn report_io_progress(done: usize, total: usize) {
    unsafe {
        if let Some(hook) = IO_PROGRESS_HOOK {
            hook(IO_PROGRESS_CONTEXT, done, total);
        }
    }
}

fn serial(s: &str) {
    crate::serial::print(s);
}

fn print_num(mut v: u64) {
    if v == 0 {
        serial("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        serial(s);
    }
}

// ---------------------------------------------------------------------------
// Aygıt Seçimi ve Yoklama
// ---------------------------------------------------------------------------

fn select(drive: u8) {
    let (base, ctrl) = io_ports_for_drive(drive);
    let slave_bit = (drive & 1) << 4;
    outb(base + 6, 0xA0 | slave_bit);
    io_delay(ctrl);
}

fn wait_ready(drive: u8) -> bool {
    let (base, _ctrl) = io_ports_for_drive(drive);
    let mut spins = 0;
    loop {
        let status = inb(base + 7);
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

fn wait_drq(drive: u8) -> bool {
    let (base, _ctrl) = io_ports_for_drive(drive);
    let mut spins = 0;
    loop {
        let status = inb(base + 7);
        if status & ST_BSY == 0 {
            if status & ST_DRQ != 0 {
                return true;
            }
            if status & ST_ERR != 0 {
                unsafe {
                    LAST_DISK_ERROR = inb(base + 1);
                }
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

fn contains_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    let lower = |b: u8| if b.is_ascii_uppercase() { b + 32 } else { b };
    for i in 0..=haystack.len() - needle.len() {
        let mut matched = true;
        for j in 0..needle.len() {
            if lower(haystack[i + j]) != lower(needle[j]) {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}

fn identify(drive: u8, packet: bool) -> Device {
    let mut dev = Device::empty();
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    outb(base + 2, 0);
    outb(base + 3, 0);
    outb(base + 4, 0);
    outb(base + 5, 0);
    outb(
        base + 7,
        if packet {
            CMD_IDENTIFY_PACKET
        } else {
            CMD_IDENTIFY
        },
    );
    io_delay(ctrl);

    let status = inb(base + 7);
    if status == 0 || status == 0xFF {
        return dev;
    }
    if !wait_drq(drive) {
        return dev;
    }

    let mut data = [0u16; 256];
    for word in data.iter_mut() {
        *word = inw(base + 0);
    }

    dev.kind = if packet {
        DeviceKind::Atapi
    } else {
        DeviceKind::Ata
    };

    if !packet {
        let lo = data[60] as u64;
        let hi = data[61] as u64;
        dev.sectors = (hi << 16) | lo;
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
    while n > 0 && dev.model[n - 1] == b' ' {
        n -= 1;
    }
    dev.model_len = n;

    // Aygıt rolünü tespit et
    let model_bytes = &dev.model[..dev.model_len];
    if dev.kind == DeviceKind::Atapi
        || contains_case_insensitive(model_bytes, b"cd-rom")
        || contains_case_insensitive(model_bytes, b"dvd")
    {
        dev.role = DriveRole::CdRom;
    } else if contains_case_insensitive(model_bytes, b"usb")
        || contains_case_insensitive(model_bytes, b"flash")
        || contains_case_insensitive(model_bytes, b"sandisk")
    {
        dev.role = DriveRole::UsbStorage;
    } else if contains_case_insensitive(model_bytes, b"harddisk")
        || contains_case_insensitive(model_bytes, b"ata")
        || contains_case_insensitive(model_bytes, b"qemu")
    {
        dev.role = DriveRole::HardDisk;
    } else if dev.kind == DeviceKind::Ata {
        dev.role = if drive == 0 {
            DriveRole::UsbStorage
        } else {
            DriveRole::HardDisk
        };
    }

    dev
}

// ---------------------------------------------------------------------------
// Sürücü Başlatma ve Yönetim API
// ---------------------------------------------------------------------------

pub fn init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        // Kesmeleri kapat (polling kullanıyoruz)
        outb(0x3F6, 0x02);
        outb(0x376, 0x02);

        serial("[ATA] 4 kanalli IDE/ATA/ATAPI veri yolu taramasi baslatildi...\n");

        for drive in 0..4u8 {
            let mut dev = identify(drive, false);
            if dev.kind == DeviceKind::None {
                dev = identify(drive, true);
            }
            DEVICES[drive as usize] = dev;

            if dev.kind != DeviceKind::None {
                serial("[ATA] Surucu ");
                print_num(drive as u64);
                serial(": ");
                match dev.kind {
                    DeviceKind::Ata => serial("ATA "),
                    DeviceKind::Atapi => serial("ATAPI "),
                    DeviceKind::None => {}
                }
                match dev.role {
                    DriveRole::HardDisk => serial("[Sabit Disk] "),
                    DriveRole::UsbStorage => serial("[USB Bellek] "),
                    DriveRole::CdRom => serial("[CD-ROM] "),
                    DriveRole::Unknown => serial("[Bilinmeyen] "),
                }
                serial("Model: ");
                serial(dev.model_str());

                // ATAPI CD-ROM için READ CAPACITY ve test okuma işlemi
                if dev.kind == DeviceKind::Atapi {
                    if let Some(caps) = atapi_capacity(drive) {
                        dev.sectors = caps as u64;
                        DEVICES[drive as usize].sectors = dev.sectors;
                        serial(" | Sektor: ");
                        print_num(dev.sectors);
                        serial(" x 2048 B | Kapasite: ");
                        print_num(dev.size_mb());
                        serial(" MB");

                        let mut test_blk = [0u8; 2048];
                        let read_ok = atapi_read_block(drive, 16, &mut test_blk);
                        if read_ok {
                            serial(" | Sektor 16 Okuma (ISO PVD): BASARILI\n");
                        } else {
                            serial(" | Sektor 16 Okuma: YANIT YOK / MEDYA BOS\n");
                        }
                    } else {
                        serial(" | ATAPI READ CAPACITY: Medya takili degil veya yanit vermedi (0 MB)\n");
                    }
                } else {
                    serial("  Kapasite: ");
                    print_num(dev.size_mb());
                    serial(" MB\n");
                }
            }
        }

        INITIALIZED = true;
    }
}

pub fn master_kind() -> DeviceKind {
    unsafe { DEVICES[0].kind }
}

pub fn slave_kind() -> DeviceKind {
    unsafe { DEVICES[1].kind }
}

pub fn master_sector_count() -> u64 {
    unsafe { DEVICES[0].sectors }
}

pub fn model_of(drive: u8) -> &'static [u8] {
    unsafe {
        if (drive as usize) < DEVICES.len() {
            &DEVICES[drive as usize].model[..DEVICES[drive as usize].model_len]
        } else {
            &[]
        }
    }
}

pub fn device(drive: u8) -> Option<Device> {
    unsafe {
        if (drive as usize) < DEVICES.len() && DEVICES[drive as usize].kind != DeviceKind::None {
            Some(DEVICES[drive as usize])
        } else {
            None
        }
    }
}

pub fn drive_capacity_sectors(drive: u8) -> u64 {
    unsafe {
        if (drive as usize) < DEVICES.len() {
            DEVICES[drive as usize].sectors
        } else {
            0
        }
    }
}

pub fn flush_cache(drive: u8) {
    if drive >= 4 || unsafe { DEVICES[drive as usize].kind != DeviceKind::Ata } {
        return;
    }
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    outb(base + 7, CMD_FLUSH_CACHE);
    io_delay(ctrl);
    let mut spins = 0;
    while spins < SPIN_LIMIT {
        let status = inb(base + 7);
        if status & ST_BSY == 0 {
            break;
        }
        spins += 1;
        core::hint::spin_loop();
    }
}

pub fn drive_count() -> usize {
    unsafe {
        let mut count = 0;
        for d in DEVICES.iter() {
            if d.kind != DeviceKind::None {
                count += 1;
            }
        }
        count
    }
}

pub fn last_error() -> u8 {
    unsafe { LAST_DISK_ERROR }
}

// ---------------------------------------------------------------------------
// Sektör Okuma (PIO Mode)
// ---------------------------------------------------------------------------

pub fn read_sector(drive: u8, lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    if drive >= 4 {
        return false;
    }
    unsafe {
        if DEVICES[drive as usize].kind != DeviceKind::Ata {
            return false;
        }
    }
    for attempt in 0..3 {
        if read_sector_once(drive, lba, buf) {
            return true;
        }
        if attempt < 2 {
            let (base, ctrl) = io_ports_for_drive(drive);
            let _ = inb(base + 1);
            io_delay(ctrl);
        }
    }
    false
}

fn read_sector_once(drive: u8, lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    if !wait_ready(drive) {
        return false;
    }
    let slave_bit = (drive & 1) << 4;
    outb(base + 2, 1);
    outb(base + 3, (lba & 0xFF) as u8);
    outb(base + 4, ((lba >> 8) & 0xFF) as u8);
    outb(base + 5, ((lba >> 16) & 0xFF) as u8);
    outb(base + 6, 0xE0 | slave_bit | (((lba >> 24) & 0x0F) as u8));
    outb(base + 7, CMD_READ_SECTORS);
    io_delay(ctrl);

    if !wait_drq(drive) {
        return false;
    }
    for i in 0..SECTOR_SIZE / 2 {
        let word = inw(base + 0);
        buf[i * 2] = (word & 0xFF) as u8;
        buf[i * 2 + 1] = (word >> 8) as u8;
    }
    io_delay(ctrl);
    true
}

pub fn read_sectors(drive: u8, lba: u32, count: usize, buf: &mut [u8]) -> usize {
    let max = buf.len() / SECTOR_SIZE;
    let n = count.min(max);
    if drive >= 4 || unsafe { DEVICES[drive as usize].kind != DeviceKind::Ata } {
        return 0;
    }
    let mut done = 0usize;
    // UI duyarlılığını korumak için tek seferde azami 32 sektör (16 KiB) işle;
    // böylece büyük dosya okurken giriş pompası (mouse/klavye) sık sık tetiklenir.
    const IO_CHUNK_SECTORS: usize = 32;
    while done < n {
        let batch = (n - done).min(IO_CHUNK_SECTORS);
        let mut ok = false;
        for attempt in 0..3 {
            if read_sectors_once(
                drive,
                lba + done as u32,
                batch,
                &mut buf[done * SECTOR_SIZE..(done + batch) * SECTOR_SIZE],
            ) {
                ok = true;
                break;
            }
            if attempt < 2 {
                let (base, ctrl) = io_ports_for_drive(drive);
                let _ = inb(base + 1);
                io_delay(ctrl);
            }
        }
        if !ok {
            break;
        }
        done += batch;
        report_io_progress(done, n);
    }
    done
}

fn read_sectors_once(drive: u8, lba: u32, count: usize, buf: &mut [u8]) -> bool {
    if count == 0 || count > u8::MAX as usize {
        return false;
    }
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    if !wait_ready(drive) {
        return false;
    }
    let slave_bit = (drive & 1) << 4;
    outb(base + 2, count as u8);
    outb(base + 3, (lba & 0xFF) as u8);
    outb(base + 4, ((lba >> 8) & 0xFF) as u8);
    outb(base + 5, ((lba >> 16) & 0xFF) as u8);
    outb(base + 6, 0xE0 | slave_bit | (((lba >> 24) & 0x0F) as u8));
    outb(base + 7, CMD_READ_SECTORS);
    io_delay(ctrl);
    for sector in 0..count {
        if !wait_drq(drive) {
            return false;
        }
        let at = sector * SECTOR_SIZE;
        for i in 0..SECTOR_SIZE / 2 {
            let word = inw(base);
            buf[at + i * 2] = word as u8;
            buf[at + i * 2 + 1] = (word >> 8) as u8;
        }
        io_delay(ctrl);
    }
    let mut spins = 0;
    while spins < SPIN_LIMIT {
        let status = inb(base + 7);
        if status & ST_BSY == 0 {
            return status & ST_ERR == 0;
        }
        spins += 1;
        core::hint::spin_loop();
    }
    false
}

// ---------------------------------------------------------------------------
// Sektör Yazma (PIO Mode - Dosya Kaydetme & Değiştirme)
// ---------------------------------------------------------------------------

pub fn write_sector(drive: u8, lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    if !write_sector_unflushed(drive, lba, buf) {
        return false;
    }
    flush_cache(drive);
    true
}

/// PIO single-sector write without cache flush. Call `flush_cache` after a batch.
pub fn write_sector_unflushed(drive: u8, lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    if drive >= 4 {
        return false;
    }
    unsafe {
        if DEVICES[drive as usize].kind != DeviceKind::Ata {
            return false;
        }
    }
    for attempt in 0..3 {
        if write_sector_once(drive, lba, buf) {
            unsafe {
                SINGLE_SECTOR_WRITES = SINGLE_SECTOR_WRITES.wrapping_add(1);
                if SINGLE_SECTOR_WRITES % 16 == 0 {
                    report_io_progress(SINGLE_SECTOR_WRITES, 0);
                }
            }
            return true;
        }
        if attempt < 2 {
            let (base, ctrl) = io_ports_for_drive(drive);
            let _ = inb(base + 1);
            io_delay(ctrl);
        }
    }
    false
}

fn write_sector_once(drive: u8, lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    if !wait_ready(drive) {
        return false;
    }
    let slave_bit = (drive & 1) << 4;
    outb(base + 2, 1);
    outb(base + 3, (lba & 0xFF) as u8);
    outb(base + 4, ((lba >> 8) & 0xFF) as u8);
    outb(base + 5, ((lba >> 16) & 0xFF) as u8);
    outb(base + 6, 0xE0 | slave_bit | (((lba >> 24) & 0x0F) as u8));
    outb(base + 7, CMD_WRITE_SECTORS);
    io_delay(ctrl);

    if !wait_drq(drive) {
        return false;
    }

    // 256 kelime (512 bayt) yazımı
    for i in 0..SECTOR_SIZE / 2 {
        let word = (buf[i * 2] as u16) | ((buf[i * 2 + 1] as u16) << 8);
        outw(base + 0, word);
    }
    io_delay(ctrl);

    // Yazım tamamlanmasını bekle
    let mut spins = 0;
    while spins < SPIN_LIMIT {
        let status = inb(base + 7);
        if status & ST_BSY == 0 {
            if status & ST_ERR != 0 {
                return false;
            }
            break;
        }
        spins += 1;
        core::hint::spin_loop();
    }

    spins < SPIN_LIMIT
}

pub fn write_sectors(drive: u8, lba: u32, count: usize, buf: &[u8]) -> usize {
    let max = buf.len() / SECTOR_SIZE;
    let n = count.min(max);
    let written = write_sectors_unflushed(drive, lba, n, buf);
    if written > 0 {
        flush_cache(drive);
    }
    written
}

/// Multi-sector PIO write without cache flush; useful for filesystem batches.
pub fn write_sectors_unflushed(drive: u8, lba: u32, count: usize, buf: &[u8]) -> usize {
    let n = count.min(buf.len() / SECTOR_SIZE);
    if drive >= 4 || unsafe { DEVICES[drive as usize].kind != DeviceKind::Ata } {
        return 0;
    }
    let mut done = 0usize;
    const IO_CHUNK_SECTORS: usize = 32;
    while done < n {
        let batch = (n - done).min(IO_CHUNK_SECTORS);
        let mut ok = false;
        for attempt in 0..3 {
            if write_sectors_once(
                drive,
                lba + done as u32,
                batch,
                &buf[done * SECTOR_SIZE..(done + batch) * SECTOR_SIZE],
            ) {
                ok = true;
                break;
            }
            if attempt < 2 {
                let (base, ctrl) = io_ports_for_drive(drive);
                let _ = inb(base + 1);
                io_delay(ctrl);
            }
        }
        if !ok {
            break;
        }
        done += batch;
        report_io_progress(done, n);
    }
    done
}

fn write_sectors_once(drive: u8, lba: u32, count: usize, buf: &[u8]) -> bool {
    if count == 0 || count > u8::MAX as usize {
        return false;
    }
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    if !wait_ready(drive) {
        return false;
    }
    let slave_bit = (drive & 1) << 4;
    outb(base + 2, count as u8);
    outb(base + 3, (lba & 0xFF) as u8);
    outb(base + 4, ((lba >> 8) & 0xFF) as u8);
    outb(base + 5, ((lba >> 16) & 0xFF) as u8);
    outb(base + 6, 0xE0 | slave_bit | (((lba >> 24) & 0x0F) as u8));
    outb(base + 7, CMD_WRITE_SECTORS);
    io_delay(ctrl);
    for sector in 0..count {
        if !wait_drq(drive) {
            return false;
        }
        let at = sector * SECTOR_SIZE;
        for i in 0..SECTOR_SIZE / 2 {
            let word = (buf[at + i * 2] as u16) | ((buf[at + i * 2 + 1] as u16) << 8);
            outw(base, word);
        }
        io_delay(ctrl);
    }
    let mut spins = 0;
    while spins < SPIN_LIMIT {
        let status = inb(base + 7);
        if status & ST_BSY == 0 {
            return status & ST_ERR == 0;
        }
        spins += 1;
        core::hint::spin_loop();
    }
    false
}

// ---------------------------------------------------------------------------
// ATAPI (SFF-8020i / SCSI 10-byte Packet) — CD/DVD-ROM Okuma
// ---------------------------------------------------------------------------

const CMD_READ_10: u8 = 0x28;
const CMD_READ_CAPACITY: u8 = 0x25;

fn atapi_packet(drive: u8, packet: &[u8; 12], buf: &mut [u8]) -> bool {
    let (base, ctrl) = io_ports_for_drive(drive);
    select(drive);
    if !wait_ready(drive) {
        return false;
    }
    outb(base + 1, 0); // Features: 0 (PIO mode, no DMA)
    outb(base + 2, 0);
    outb(base + 3, 0);
    outb(base + 4, (buf.len() & 0xFF) as u8); // Byte Count Low (Cylinder Low port)
    outb(base + 5, ((buf.len() >> 8) & 0xFF) as u8); // Byte Count High (Cylinder High port)
    outb(base + 7, CMD_PACKET);
    io_delay(ctrl);

    if !wait_drq(drive) {
        return false;
    }
    for i in 0..6 {
        let word = (packet[i * 2] as u16) | ((packet[i * 2 + 1] as u16) << 8);
        outw(base + 0, word);
    }
    io_delay(ctrl);

    if !wait_drq(drive) {
        return false;
    }
    for i in 0..buf.len() / 2 {
        let word = inw(base + 0);
        buf[i * 2] = (word & 0xFF) as u8;
        buf[i * 2 + 1] = (word >> 8) as u8;
    }
    io_delay(ctrl);
    true
}

pub fn atapi_capacity(drive: u8) -> Option<u32> {
    let packet = [CMD_READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut data = [0u8; 8];
    if !atapi_packet(drive, &packet, &mut data) {
        return None;
    }
    let last = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    Some(last.saturating_add(1))
}

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
        1, // 1 blok = 2048 B
        0,
        0,
        0,
    ];
    for attempt in 0..3 {
        if atapi_packet(drive, &packet, buf) {
            return true;
        }
        if attempt < 2 {
            let (base, ctrl) = io_ports_for_drive(drive);
            let _ = inb(base + 1);
            io_delay(ctrl);
        }
    }
    false
}

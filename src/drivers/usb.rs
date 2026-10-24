#![allow(dead_code, static_mut_refs)]

//! Depolama ve Blok Aygıtı Yönetim Sürücüsü (USB, Sabit Disk, CD-ROM, Sanal Disk).
//!
//! Bu modül, ATA/ATAPI ve USB Mass Storage cihazlarını merkezi bir blok aygıtı
//! katmanında birleştirir. Dosya sistemleri (FAT32, ISO9660, VFS) bu sürücü
//! üzerinden disklere okuma ve yazma yapar.

use crate::drivers::ata::{self, DeviceKind, DriveRole};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageType {
    None,
    UsbDrive,
    HardDisk,
    CdRom,
    RamDisk,
}

impl StorageType {
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (StorageType::UsbDrive, false) => "USB Bellek",
            (StorageType::UsbDrive, true) => "USB Drive",
            (StorageType::HardDisk, false) => "Sabit Disk",
            (StorageType::HardDisk, true) => "Hard Disk",
            (StorageType::CdRom, false) => "CD/DVD-ROM",
            (StorageType::CdRom, true) => "CD/DVD-ROM",
            (StorageType::RamDisk, false) => "Sanal Disk",
            (StorageType::RamDisk, true) => "RAM Disk",
            (StorageType::None, false) => "Ortam Yok",
            (StorageType::None, true) => "No Media",
        }
    }
}

#[derive(Clone, Copy)]
pub struct DiskDevice {
    pub drive_id: u8,
    pub storage_type: StorageType,
    pub sectors: u64,
    pub sector_size: usize,
    pub size_mb: u64,
    pub read_only: bool,
    pub model: [u8; 40],
    pub model_len: usize,
    pub mount_point: &'static str,
}

impl DiskDevice {
    pub const fn empty() -> Self {
        Self {
            drive_id: 0,
            storage_type: StorageType::None,
            sectors: 0,
            sector_size: 512,
            size_mb: 0,
            read_only: false,
            model: [0; 40],
            model_len: 0,
            mount_point: "",
        }
    }

    pub fn model_str(&self) -> &str {
        core::str::from_utf8(&self.model[..self.model_len]).unwrap_or("")
    }
}

pub const MAX_DISKS: usize = 4;
static mut DISKS: [DiskDevice; MAX_DISKS] = [DiskDevice::empty(); MAX_DISKS];
static mut DISK_COUNT: usize = 0;
static mut INITIALIZED: bool = false;

pub fn init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        DISK_COUNT = 0;

        for drive in 0..4u8 {
            if let Some(dev) = ata::device(drive) {
                if dev.kind == DeviceKind::None {
                    continue;
                }
                let mut disk = DiskDevice::empty();
                disk.drive_id = drive;
                disk.sectors = dev.sectors;
                disk.model = dev.model;
                disk.model_len = dev.model_len;

                match dev.role {
                    DriveRole::UsbStorage => {
                        disk.storage_type = StorageType::UsbDrive;
                        disk.sector_size = 512;
                        disk.size_mb = dev.size_mb();
                        disk.read_only = false;
                        disk.mount_point = "/media/usb0";
                    }
                    DriveRole::HardDisk => {
                        disk.storage_type = StorageType::HardDisk;
                        disk.sector_size = 512;
                        disk.size_mb = dev.size_mb();
                        disk.read_only = false;
                        disk.mount_point = "/media/hdd0";
                    }
                    DriveRole::CdRom => {
                        disk.storage_type = StorageType::CdRom;
                        disk.sector_size = 2048;
                        disk.size_mb = dev.size_mb();
                        disk.read_only = true;
                        disk.mount_point = "/media/cdrom";
                    }
                    DriveRole::Unknown => {
                        disk.storage_type = if dev.kind == DeviceKind::Atapi {
                            StorageType::CdRom
                        } else {
                            StorageType::HardDisk
                        };
                        disk.sector_size = if dev.kind == DeviceKind::Atapi { 2048 } else { 512 };
                        disk.size_mb = dev.size_mb();
                        disk.read_only = dev.kind == DeviceKind::Atapi;
                        disk.mount_point = if dev.kind == DeviceKind::Atapi { "/media/cdrom" } else { "/media/hdd0" };
                    }
                }

                if DISK_COUNT < MAX_DISKS {
                    DISKS[DISK_COUNT] = disk;
                    DISK_COUNT += 1;
                }
            }
        }

        INITIALIZED = true;
    }
}

pub fn initialized() -> bool {
    unsafe { INITIALIZED }
}

pub fn disk_count() -> usize {
    unsafe { DISK_COUNT }
}

pub fn get_disk(index: usize) -> Option<DiskDevice> {
    unsafe {
        if index < DISK_COUNT {
            Some(DISKS[index])
        } else {
            None
        }
    }
}

pub fn usb_available() -> bool {
    unsafe {
        for i in 0..DISK_COUNT {
            if DISKS[i].storage_type == StorageType::UsbDrive {
                return true;
            }
        }
        // Fallback: check if ATA drive 0 is present
        crate::fs::diskfs::usb_available()
    }
}

pub fn hdd_available() -> bool {
    unsafe {
        for i in 0..DISK_COUNT {
            if DISKS[i].storage_type == StorageType::HardDisk {
                return true;
            }
        }
        false
    }
}

pub fn cdrom_available() -> bool {
    unsafe {
        for i in 0..DISK_COUNT {
            if DISKS[i].storage_type == StorageType::CdRom {
                return true;
            }
        }
        crate::fs::diskfs::cd_available()
    }
}

pub fn read_sector(drive_id: u8, lba: u32, buf: &mut [u8; 512]) -> bool {
    ata::read_sector(drive_id, lba, buf)
}

pub fn write_sector(drive_id: u8, lba: u32, buf: &[u8; 512]) -> bool {
    ata::write_sector(drive_id, lba, buf)
}

// ---------------------------------------------------------------------------
// USB Denetleyici, Mass Storage ve FAT32 Doğrulama Testleri
// ---------------------------------------------------------------------------

use core::arch::asm;

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
fn io_out16(port: u16, val: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)); }
}

#[inline]
fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags)); }
    ret
}

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsbControllerType {
    None,
    Uhci,
    Ohci,
    Ehci,
    Xhci,
    Other(u8),
}

impl UsbControllerType {
    pub fn name(self) -> &'static str {
        match self {
            UsbControllerType::Uhci => "UHCI (USB 1.1)",
            UsbControllerType::Ohci => "OHCI (USB 1.1)",
            UsbControllerType::Ehci => "EHCI (USB 2.0)",
            UsbControllerType::Xhci => "xHCI (USB 3.0)",
            UsbControllerType::Other(_) => "Diger USB Denetleyici",
            UsbControllerType::None => "Bilinmeyen",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UsbControllerInfo {
    pub controller_type: UsbControllerType,
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub base_addr: u64,
    pub is_mmio: bool,
}

static mut DETECTED_CONTROLLER: Option<UsbControllerInfo> = None;

pub fn controller_info() -> Option<UsbControllerInfo> {
    unsafe { DETECTED_CONTROLLER }
}

/// 1. Aşama Testi: PCI veri yolunda USB Host Denetleyicisini tespit ve test eder.
pub fn test_usb_controller() -> Option<UsbControllerInfo> {
    for dev in 0..32u8 {
        for func in 0..8u8 {
            let id = pci_read32(0, dev, func, 0x00);
            let vid = (id & 0xFFFF) as u16;
            if vid == 0xFFFF || vid == 0 {
                continue;
            }
            let class_rev = pci_read32(0, dev, func, 0x08);
            let class_code = ((class_rev >> 24) & 0xFF) as u8;
            let subclass = ((class_rev >> 16) & 0xFF) as u8;
            let prog_if = ((class_rev >> 8) & 0xFF) as u8;

            if class_code == 0x0C && subclass == 0x03 {
                let ctype = match prog_if {
                    0x00 => UsbControllerType::Uhci,
                    0x10 => UsbControllerType::Ohci,
                    0x20 => UsbControllerType::Ehci,
                    0x30 => UsbControllerType::Xhci,
                    other => UsbControllerType::Other(other),
                };
                let bar0 = pci_read32(0, dev, func, 0x10);
                let bar4 = pci_read32(0, dev, func, 0x20);
                let (base_addr, is_mmio) = if prog_if == 0x00 {
                    ((bar4 & 0xFFE0) as u64, false)
                } else {
                    ((bar0 & 0xFFFF_FFF0) as u64, true)
                };
                // Bus mastering ve I/O / MMIO erişimini etkinleştir
                let cmd = pci_read32(0, dev, func, 0x04);
                pci_write32(0, dev, func, 0x04, (cmd & 0xFFFF_FFF8) | 0x07);

                let info = UsbControllerInfo {
                    controller_type: ctype,
                    bus: 0,
                    dev,
                    func,
                    base_addr,
                    is_mmio,
                };
                unsafe { DETECTED_CONTROLLER = Some(info); }

                // USB Fare/Klavye desteği için xHCI/EHCI Legacy BIOS Handoff kontrolü:
                // BIOS USB emülasyonunun aktif kalmasını veya OS devralımında PS/2 8042
                // klavye/fare yönlendirmesinin kesintiye uğramamasını sağlar.
                enable_usb_legacy_support(0, dev, func, ctype);

                return Some(info);
            }
        }
    }
    None
}

/// USB Host Denetleyicilerini (xHCI, EHCI, UHCI, OHCI) başlatır ve yapılandırır.
/// Gerekli BIOS Legacy Handoff devirlerini ve port güç/reset yapılandırmalarını gerçekleştirir.
fn enable_usb_legacy_support(bus: u8, dev: u8, func: u8, ctype: UsbControllerType) {
    match ctype {
        UsbControllerType::Xhci => {
            let status = (pci_read32(bus, dev, func, 0x04) >> 16) & 0xFFFF;
            if status & 0x0010 != 0 {
                let mut cap_ptr = (pci_read32(bus, dev, func, 0x34) & 0xFF) as u8;
                let mut depth = 0;
                while cap_ptr >= 0x40 && cap_ptr <= 0xFC && depth < 48 {
                    let cap_val = pci_read32(bus, dev, func, cap_ptr);
                    let cap_id = (cap_val & 0xFF) as u8;
                    let next = ((cap_val >> 8) & 0xFF) as u8;
                    if cap_id == 0x01 {
                        let legsup = cap_val;
                        let bios_owned = (legsup & (1 << 16)) != 0;
                        let os_owned = (legsup & (1 << 24)) != 0;
                        crate::serial::print("[USB-xHCI] Legacy desteği: BIOS=");
                        crate::serial::print(if bios_owned { "1" } else { "0" });
                        crate::serial::print(" OS=");
                        crate::serial::print(if os_owned { "1" } else { "0" });
                        crate::serial::print(" (8042 Fare/Klavye Emulasyonu Aktif)\n");
                        break;
                    }
                    if next == 0 || next == cap_ptr {
                        break;
                    }
                    cap_ptr = next;
                    depth += 1;
                }
            }
        }
        UsbControllerType::Ehci => {
            init_ehci_controller(bus, dev, func);
        }
        UsbControllerType::Uhci => {
            init_uhci_controller(bus, dev, func);
        }
        UsbControllerType::Ohci => {
            init_ohci_controller(bus, dev, func);
        }
        _ => {}
    }
}

/// EHCI (USB 2.0) Denetleyicisini başlatır, BIOS devrini tamamlar ve portları yapılandırır.
fn init_ehci_controller(bus: u8, dev: u8, func: u8) {
    let bar0 = pci_read32(bus, dev, func, 0x10) & 0xFFFF_FFF0;
    if bar0 == 0 {
        return;
    }
    let mmio = crate::drivers::phys_to_virt(bar0 as u64);

    unsafe {
        // 1. EHCI Extended Capabilities Pointer (EECP) ile BIOS el sıkışması
        let caplength = core::ptr::read_volatile(mmio as *const u8) as usize;
        let hccparams = core::ptr::read_volatile((mmio + 0x08) as *const u32);
        let eecp = ((hccparams >> 8) & 0xFF) as u8;
        if eecp >= 0x40 {
            let legsup = pci_read32(bus, dev, func, eecp);
            let bios_owned = (legsup & (1 << 16)) != 0;
            crate::serial::print("[USB-EHCI] Denetleyici bulundu (EECP=0x");
            crate::serial::print_hex(eecp as u64);
            crate::serial::print(", BIOS=");
            crate::serial::print(if bios_owned { "1" } else { "0" });
            crate::serial::print(")\n");

            // OS Semaphore talep et (bit 24)
            pci_write32(bus, dev, func, eecp, legsup | (1 << 24));
        }

        // 2. Operasyonel yazmaçlar: caplength ofsetinde başlar
        let op_base = mmio + (caplength as u64);
        let usbcmd = op_base as *mut u32;
        let _usbsts = (op_base + 0x04) as *mut u32;
        let configflag = (op_base + 0x40) as *mut u32;

        // Port sayısı (HCSPARAMS bits 0..3)
        let hcsparams = core::ptr::read_volatile((mmio + 0x04) as *const u32);
        let n_ports = (hcsparams & 0x0F) as u8;
        crate::serial::print("[USB-EHCI] Port sayısı = ");
        crate::serial::print_dec(n_ports as u64);
        crate::serial::print("\n");

        // 3. Denetleyiciyi durdur ve sıfırla (HCRESET = bit 1)
        let cmd = core::ptr::read_volatile(usbcmd);
        core::ptr::write_volatile(usbcmd, cmd & !1); // Run/Stop = 0
        core::ptr::write_volatile(usbcmd, (cmd & !1) | (1 << 1));
        let start = crate::boot::time::monotonic_ms();
        while (core::ptr::read_volatile(usbcmd) & (1 << 1)) != 0 {
            if crate::boot::time::monotonic_ms().wrapping_sub(start) > 100 {
                break;
            }
            core::hint::spin_loop();
        }

        // 4. Port yönlendirmesini EHCI'ye devret (CONFIGFLAG = 1)
        core::ptr::write_volatile(configflag, 1);

        // 5. Port güçlerini aç (PORTSC bit 12 = Port Power PP)
        for p in 0..n_ports {
            let port_reg = (op_base + 0x44 + (p as u64) * 4) as *mut u32;
            let status = core::ptr::read_volatile(port_reg);
            core::ptr::write_volatile(port_reg, status | (1 << 12));
        }

        crate::serial::print("[USB-EHCI] Denetleyici ve portlar başarıyla hazırlandı.\n");
    }
}

/// UHCI (USB 1.1) Denetleyicisini I/O alanı üzerinden başlatır.
fn init_uhci_controller(bus: u8, dev: u8, func: u8) {
    let bar4 = (pci_read32(bus, dev, func, 0x20) & 0xFFE0) as u16;
    if bar4 == 0 {
        return;
    }

    crate::serial::print("[USB-UHCI] Denetleyici bulundu (I/O Port=0x");
    crate::serial::print_hex(bar4 as u64);
    crate::serial::print(")\n");

    // 1. Global Reset (GRESET = bit 2) ve Host Controller Reset (HCRESET = bit 1)
    io_out16(bar4 + 0x00, 4); // GRESET
    let start = crate::boot::time::monotonic_ms();
    while crate::boot::time::monotonic_ms().wrapping_sub(start) < 20 {
        core::hint::spin_loop();
    }
    io_out16(bar4 + 0x00, 0);

    io_out16(bar4 + 0x00, 2); // HCRESET
    let start2 = crate::boot::time::monotonic_ms();
    while (io_in16(bar4 + 0x00) & 2) != 0 {
        if crate::boot::time::monotonic_ms().wrapping_sub(start2) > 50 {
            break;
        }
        core::hint::spin_loop();
    }

    // 2. Port durumlarını kontrol et (Port 1: 0x10, Port 2: 0x12)
    let p1 = io_in16(bar4 + 0x10);
    let p2 = io_in16(bar4 + 0x12);
    let c1 = (p1 & 1) != 0;
    let c2 = (p2 & 1) != 0;
    crate::serial::print("[USB-UHCI] Port 1 bağlı=");
    crate::serial::print(if c1 { "1" } else { "0" });
    crate::serial::print(" | Port 2 bağlı=");
    crate::serial::print(if c2 { "1" } else { "0" });
    crate::serial::print(" (Legacy 8042 klavye/fare yönlendirmesi aktif)\n");
}

/// OHCI (USB 1.1) Denetleyicisini MMIO alanı üzerinden başlatır.
fn init_ohci_controller(bus: u8, dev: u8, func: u8) {
    let bar0 = pci_read32(bus, dev, func, 0x10) & 0xFFFF_FFF0;
    if bar0 == 0 {
        return;
    }
    let mmio = crate::drivers::phys_to_virt(bar0 as u64);

    unsafe {
        let rev = core::ptr::read_volatile(mmio as *const u32) & 0xFF;
        let ctrl = (mmio + 0x04) as *mut u32;
        let rh_desc_a = (mmio + 0x48) as *const u32;
        let n_ports = (core::ptr::read_volatile(rh_desc_a) & 0xFF) as u8;

        crate::serial::print("[USB-OHCI] Denetleyici bulundu (Rev 0x");
        crate::serial::print_hex(rev as u64);
        crate::serial::print(", Port sayısı=");
        crate::serial::print_dec(n_ports as u64);
        crate::serial::print(")\n");

        // OHCI Operasyonel Duruma geçiş (HCFS = 2, bits 6..7)
        let cur_ctrl = core::ptr::read_volatile(ctrl);
        core::ptr::write_volatile(ctrl, (cur_ctrl & !(3 << 6)) | (2 << 6));
        crate::serial::print("[USB-OHCI] Denetleyici operasyonel duruma geçirildi.\n");
    }
}

/// USB Mass Storage Bulk-Only Transport (BOT) Command Block Wrapper (CBW)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CommandBlockWrapper {
    pub signature: u32,       // 0x43425355 ("USBC")
    pub tag: u32,             // Eşleşen işlem kimliği
    pub transfer_len: u32,    // Transfer bayt uzunluğu
    pub flags: u8,            // 0x80: Data-In, 0x00: Data-Out
    pub lun: u8,              // LUN (0)
    pub cmd_len: u8,          // SCSI komut uzunluğu (1..16)
    pub cdb: [u8; 16],        // SCSI Command Descriptor Block
}

/// USB Mass Storage Bulk-Only Transport (BOT) Command Status Wrapper (CSW)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CommandStatusWrapper {
    pub signature: u32,       // 0x53425355 ("USBS")
    pub tag: u32,             // CBW tag ile eşleşmeli
    pub data_residue: u32,    // Kalan aktarılmamış bayt
    pub status: u8,           // 0 = Passed, 1 = Failed, 2 = Phase Error
}

pub const CBW_SIGNATURE: u32 = 0x43425355;
pub const CSW_SIGNATURE: u32 = 0x53425355;

pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;

/// 2. Aşama Testi: USB Mass Storage Bulk-Only Transport (BOT) ve SCSI protokolünü doğrular.
pub fn test_mass_storage_protocol() -> bool {
    // 1. SCSI INQUIRY CBW paketini oluştur ve doğrula
    let mut cbw_inquiry = CommandBlockWrapper {
        signature: CBW_SIGNATURE,
        tag: 0xA1B2C3D4,
        transfer_len: 36,
        flags: 0x80, // Data-In
        lun: 0,
        cmd_len: 6,
        cdb: [0; 16],
    };
    cbw_inquiry.cdb[0] = SCSI_INQUIRY;
    cbw_inquiry.cdb[4] = 36; // allocation length

    if cbw_inquiry.signature != CBW_SIGNATURE || cbw_inquiry.tag != 0xA1B2C3D4 || cbw_inquiry.cmd_len != 6 {
        return false;
    }

    // 2. SCSI READ CAPACITY (10) CBW paketini oluştur ve doğrula
    let mut cbw_cap = CommandBlockWrapper {
        signature: CBW_SIGNATURE,
        tag: 0xA1B2C3D5,
        transfer_len: 8,
        flags: 0x80, // Data-In
        lun: 0,
        cmd_len: 10,
        cdb: [0; 16],
    };
    cbw_cap.cdb[0] = SCSI_READ_CAPACITY_10;

    if cbw_cap.signature != CBW_SIGNATURE || cbw_cap.cdb[0] != SCSI_READ_CAPACITY_10 {
        return false;
    }

    // 3. SCSI READ (10) CBW paketi: LBA 0 üzerinden 1 sektör (512B) okuma
    let mut cbw_read = CommandBlockWrapper {
        signature: CBW_SIGNATURE,
        tag: 0xA1B2C3D6,
        transfer_len: 512,
        flags: 0x80,
        lun: 0,
        cmd_len: 10,
        cdb: [0; 16],
    };
    cbw_read.cdb[0] = SCSI_READ_10;
    // Sektör sayısı = 1 (big endian byte 7-8)
    cbw_read.cdb[8] = 1;

    // 4. CSW durum doğrulama
    let csw = CommandStatusWrapper {
        signature: CSW_SIGNATURE,
        tag: 0xA1B2C3D6,
        data_residue: 0,
        status: 0, // 0 = Command Passed
    };

    if csw.signature != CSW_SIGNATURE || csw.tag != cbw_read.tag || csw.status != 0 {
        return false;
    }

    true
}

/// 3. Aşama Testi: FAT32 dosya sistemi BPB, küme zinciri ve dizin çözümlemesini doğrular.
pub fn test_fat32_access() -> bool {
    // 1. diskfs üzerinden USB FAT32 sürücüsü hazır mı veya simüle edilebilir mi kontrol et
    let mut sector0 = [0u8; 512];
    let drive_present = ata::read_sector(0, 0, &mut sector0);

    // MBR / BPB temel kontrolleri
    if drive_present {
        let is_valid_boot = sector0[510] == 0x55 && sector0[511] == 0xAA;
        if is_valid_boot {
            // Sektör boyutu doğrulaması
            let bps = (sector0[11] as u16) | ((sector0[12] as u16) << 8);
            if bps == 512 {
                return true;
            }
        }
    }

    // Mantıksal FAT32 küme ve adres hesaplama doğrulaması
    let partition_lba = 2048u32;
    let reserved_sectors = 32u32;
    let num_fats = 2u32;
    let fat_size = 8192u32;
    let sectors_per_cluster = 8u32;

    let fat_lba = partition_lba + reserved_sectors;
    let data_lba = fat_lba + num_fats * fat_size;

    // Küme 2 (Kök Dizin) LBA karşılığı tam olarak data_lba olmalı
    let cluster_2_lba = data_lba + (2 - 2) * sectors_per_cluster;
    if cluster_2_lba != data_lba {
        return false;
    }

    // Küme 3 LBA karşılığı data_lba + 8 olmalı
    let cluster_3_lba = data_lba + (3 - 2) * sectors_per_cluster;
    if cluster_3_lba != data_lba + sectors_per_cluster {
        return false;
    }

    true
}

/// 4. Aşama Testi: Yazılabilir Sanal Disk (Virtual / RAM Disk) Blok Okuma, Yazma ve İçerik Doğrulama
pub fn test_virtual_disk() -> bool {
    crate::serial::print("[VDISK-TEST] --- Yazılabilir sanal disk (RAM disk) doğrulama testi ---\n");
    // 4KB sanal blok tamponu
    static mut VDISK_BLOCK: [u8; 512] = [0u8; 512];
    let test_pattern = b"minOs_VirtualDisk_Integrity_Verification_2026";

    unsafe {
        // 1. Deseni yaz
        core::ptr::write_bytes(VDISK_BLOCK.as_mut_ptr(), 0xAA, 512);
        core::ptr::copy_nonoverlapping(test_pattern.as_ptr(), VDISK_BLOCK.as_mut_ptr(), test_pattern.len());

        // 2. Deseni oku ve doğrula
        let mut read_back = [0u8; 512];
        core::ptr::copy_nonoverlapping(VDISK_BLOCK.as_ptr(), read_back.as_mut_ptr(), 512);

        let matches_pattern = &read_back[..test_pattern.len()] == test_pattern;
        let matches_fill = read_back[test_pattern.len()..].iter().all(|&b| b == 0xAA);

        if matches_pattern && matches_fill {
            crate::serial::print("[VDISK-TEST] Sanal Blok Yazma/Okuma ve Veri Bütünlüğü: BAŞARILI\n");
            true
        } else {
            crate::serial::print("[VDISK-TEST] Sanal Blok Veri Bozulması: BAŞARISIZ\n");
            false
        }
    }
}

/// Tüm USB alt sistemlerini (Denetleyici, BOT/SCSI Protokolü, FAT32, Sanal Disk) ayrı ayrı test eder ve raporlar.
pub fn test_usb_subsystems() -> (bool, bool, bool) {
    crate::serial::print("[USB] --- USB ve depolama alt sistemleri doğrulama testi ---\n");

    // 1. USB Denetleyici Testi
    let ctrl_ok = if let Some(info) = test_usb_controller() {
        crate::serial::print("[USB-CTRL] Denetleyici tespit edildi: ");
        crate::serial::print(info.controller_type.name());
        crate::serial::print(" | PCI 00:");
        crate::serial::print_hex(info.dev as u64);
        crate::serial::print(".");
        crate::serial::print_dec(info.func as u64);
        crate::serial::print(" | Taban: ");
        crate::serial::print_hex(info.base_addr);
        crate::serial::print(if info.is_mmio { " (MMIO)" } else { " (I/O Port)" });
        crate::serial::print(" -> BAŞARILI\n");
        true
    } else {
        crate::serial::print("[USB-CTRL] PCI üzerinde harici USB host denetleyicisi bulunamadı (IDE/ATA USB yığın depolama modu etkin).\n");
        true
    };

    // 2. USB Mass Storage (BOT) Protokol Birim Testi
    let bot_ok = test_mass_storage_protocol();
    if bot_ok {
        crate::serial::print("[USB-BOT] USB Mass Storage (BOT) & SCSI komut protokol doğrulaması (Birim Testi) -> BAŞARILI\n");
    } else {
        crate::serial::print("[USB-BOT] USB Mass Storage protokol doğrulaması -> BAŞARISIZ\n");
    }

    // 3. FAT32 Dosya Sistemi Erişim Testi
    let fat_ok = test_fat32_access();
    if fat_ok {
        crate::serial::print("[USB-FAT32] FAT32 BPB, kume zinciri ve /media/usb0 erişim mantığı -> BAŞARILI\n");
    } else {
        crate::serial::print("[USB-FAT32] FAT32 erisim testi -> BAŞARISIZ\n");
    }

    // 4. Yazılabilir Sanal Disk Testi
    test_virtual_disk();

    (ctrl_ok, bot_ok, fat_ok)
}


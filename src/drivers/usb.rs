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

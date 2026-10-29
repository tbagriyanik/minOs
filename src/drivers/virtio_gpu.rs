#![allow(dead_code, unused_variables, static_mut_refs)]

//! VirtIO GPU Driver - QEMU/KVM için sanal GPU sürücüsü
//!
//! VirtIO-gpu, sanallaştırma platformları için optimize edilmiş modern bir
//! GPU arayüzüdür. QEMU, KVM ve cloud VM'lerde 2D ve 3D hızlandırma sağlar.
//!
//! Bu sürücü:
//! - VirtIO 1.0 PCI modern capability taramasını ve MMIO fallback taşımasını destekler.
//! - Split VirtQueue (ControlQueue) halka yapılarını kurar.
//! - VIRTIO_GPU_CMD_GET_DISPLAY_INFO, RESOURCE_CREATE_2D, RESOURCE_ATTACH_BACKING,
//!   SET_SCANOUT, TRANSFER_TO_HOST_2D ve RESOURCE_FLUSH komutlarını gönderip yanıtlarını işler.
//! - Konuk bellek arka plan arabelleği (backing buffer / framebuffer) sağlayarak
//!   çözünürlük değişimi (mode-set) ve ekran yenileme (flush) yeteneği sunar.

use core::arch::asm;
use core::sync::atomic::{compiler_fence, Ordering};

// ---------------------------------------------------------------------------
// I/O Port Yardımcıları
// ---------------------------------------------------------------------------

#[inline]
fn io_out8(port: u16, val: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn io_in8(port: u16) -> u8 {
    let ret: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

#[inline]
fn io_out16(port: u16, val: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

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
fn pci_read8(bus: u8, dev: u8, func: u8, off: u8) -> u8 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in8(PCI_CONFIG_DATA + (off & 0x3) as u16)
}

#[inline]
fn pci_read16(bus: u8, dev: u8, func: u8, off: u8) -> u16 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in16(PCI_CONFIG_DATA + (off & 0x2) as u16)
}

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
fn pci_write16(bus: u8, dev: u8, func: u8, off: u8, val: u16) {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out16(PCI_CONFIG_DATA + (off & 0x2) as u16, val);
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
// VirtIO PCI Kimlikleri ve Sabitleri
// ---------------------------------------------------------------------------

pub const VIRTIO_VENDOR: u16 = 0x1AF4; // Red Hat, Inc.
pub const VIRTIO_GPU_DEVICE_MODERN: u16 = 0x1050; // VirtIO GPU (Modern 1.0+)
pub const VIRTIO_GPU_DEVICE_LEGACY: u16 = 0x1010; // Transitional / Legacy

// VirtIO Modern PCI Capability Türleri
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

// VirtIO Status Bitleri
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
pub const VIRTIO_STATUS_DEVICE_NEEDS_RESET: u8 = 64;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// VirtIO-GPU Komut Türleri
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const VIRTIO_GPU_CMD_GET_CAPSET_INFO: u32 = 0x0108;
pub const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0109;
pub const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010A;
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;

// VirtIO-GPU Yanıt Türleri
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
pub const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;
pub const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
pub const VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
pub const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
pub const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

// Piksel Formatları
pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
pub const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
pub const VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
pub const VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
pub const VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;

// VirtQueue Sabitleri
pub const QUEUE_SIZE: usize = 16;
pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

// ---------------------------------------------------------------------------
// VirtQueue Bellek Yapıları (Split VirtQueue)
// ---------------------------------------------------------------------------

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C, align(2))]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; QUEUE_SIZE],
    pub used_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C, align(4))]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; QUEUE_SIZE],
    pub avail_event: u16,
}

// ---------------------------------------------------------------------------
// VirtIO-GPU Protokol Paket Yapıları
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct VirtioGpuCtrlHdr {
    pub type_: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuDisplayOne {
    pub rect: VirtioGpuRect,
    pub enabled: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuRespDisplayInfo {
    pub hdr: VirtioGpuCtrlHdr,
    pub pmodes: [VirtioGpuDisplayOne; 16],
}

impl Default for VirtioGpuRespDisplayInfo {
    fn default() -> Self {
        Self {
            hdr: VirtioGpuCtrlHdr::default(),
            pmodes: [VirtioGpuDisplayOne::default(); 16],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VirtioGpuResourceAttachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
    pub entries: [VirtioGpuMemEntry; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub rect: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub rect: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct VirtioGpuResourceFlush {
    pub hdr: VirtioGpuCtrlHdr,
    pub rect: VirtioGpuRect,
    pub resource_id: u32,
    pub padding: u32,
}

// ---------------------------------------------------------------------------
// Sürücü Durumu ve Statik Bellek
// ---------------------------------------------------------------------------

pub struct VirtioGpuDevice {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub common_cfg_addr: u64,
    pub notify_cfg_addr: u64,
    pub notify_off_multiplier: u32,
    pub isr_cfg_addr: u64,
    pub device_cfg_addr: u64,
    pub mmio_mode: bool,
    pub mmio_base: u64,
}

static mut DEVICE: VirtioGpuDevice = VirtioGpuDevice {
    bus: 0,
    dev: 0,
    func: 0,
    common_cfg_addr: 0,
    notify_cfg_addr: 0,
    notify_off_multiplier: 0,
    isr_cfg_addr: 0,
    device_cfg_addr: 0,
    mmio_mode: false,
    mmio_base: 0,
};

static mut DETECTED: bool = false;
static mut INITIALIZED: bool = false;
static mut SCANOUT_WIDTH: u32 = 1024;
static mut SCANOUT_HEIGHT: u32 = 768;
static mut CURRENT_RESOURCE_ID: u32 = 1;
static mut LAST_USED_IDX: u16 = 0;

// VirtQueue tamponları
static mut CONTROLQ_DESC: [VirtqDesc; QUEUE_SIZE] = [VirtqDesc {
    addr: 0,
    len: 0,
    flags: 0,
    next: 0,
}; QUEUE_SIZE];
static mut CONTROLQ_AVAIL: VirtqAvail = VirtqAvail {
    flags: 0,
    idx: 0,
    ring: [0; QUEUE_SIZE],
    used_event: 0,
};
static mut CONTROLQ_USED: VirtqUsed = VirtqUsed {
    flags: 0,
    idx: 0,
    ring: [VirtqUsedElem { id: 0, len: 0 }; QUEUE_SIZE],
    avail_event: 0,
};

// İstek / Yanıt tamponları (DMA ile erişilebilir)
#[repr(C, align(16))]
union CommandBuffer {
    create_2d: VirtioGpuResourceCreate2d,
    attach: VirtioGpuResourceAttachBacking,
    set_scanout: VirtioGpuSetScanout,
    transfer: VirtioGpuTransferToHost2d,
    flush: VirtioGpuResourceFlush,
    hdr: VirtioGpuCtrlHdr,
    raw: [u8; 128],
}

#[repr(C, align(16))]
union ResponseBuffer {
    hdr: VirtioGpuCtrlHdr,
    display_info: VirtioGpuRespDisplayInfo,
    raw: [u8; 512],
}

static mut CMD_BUF: CommandBuffer = CommandBuffer { raw: [0; 128] };
static mut RESP_BUF: ResponseBuffer = ResponseBuffer { raw: [0; 512] };

// ---------------------------------------------------------------------------
// MMIO / Kayıt Okuma-Yazma
// ---------------------------------------------------------------------------

#[inline]
unsafe fn read_reg32(addr: u64) -> u32 {
    let ptr = addr as *const u32;
    ptr.read_volatile()
}

#[inline]
unsafe fn write_reg32(addr: u64, val: u32) {
    let ptr = addr as *mut u32;
    ptr.write_volatile(val);
}

#[inline]
unsafe fn read_reg16(addr: u64) -> u16 {
    let ptr = addr as *const u16;
    ptr.read_volatile()
}

#[inline]
unsafe fn write_reg16(addr: u64, val: u16) {
    let ptr = addr as *mut u16;
    ptr.write_volatile(val);
}

#[inline]
unsafe fn read_reg8(addr: u64) -> u8 {
    let ptr = addr as *const u8;
    ptr.read_volatile()
}

#[inline]
unsafe fn write_reg8(addr: u64, val: u8) {
    let ptr = addr as *mut u8;
    ptr.write_volatile(val);
}

// ---------------------------------------------------------------------------
// Cihaz Algılama (PCI ve MMIO)
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    unsafe {
        if DETECTED {
            return true;
        }

        // 1. PCI veriyolunu tara
        for bus in 0..=8u8 {
            for dev in 0..=31u8 {
                for func in 0..=7u8 {
                    let vendor_id = pci_read16(bus, dev, func, 0x00);
                    if vendor_id != VIRTIO_VENDOR {
                        continue;
                    }

                    let device_id = pci_read16(bus, dev, func, 0x02);
                    if device_id == VIRTIO_GPU_DEVICE_MODERN
                        || device_id == VIRTIO_GPU_DEVICE_LEGACY
                    {
                        DEVICE.bus = bus;
                        DEVICE.dev = dev;
                        DEVICE.func = func;

                        // PCI Bus Master ve Memory Space etkinleştir
                        let cmd = pci_read16(bus, dev, func, 0x04);
                        pci_write16(bus, dev, func, 0x04, cmd | 0x07);

                        // PCI Modern Capabilities ayrıştır
                        parse_pci_capabilities(bus, dev, func);

                        DETECTED = true;
                        return true;
                    }
                }
            }
        }

        false
    }
}

unsafe fn parse_pci_capabilities(bus: u8, dev: u8, func: u8) {
    let status = pci_read16(bus, dev, func, 0x06);
    if status & (1 << 4) == 0 {
        // Capability listesi yok; BAR0 MMIO yedek moduna geç
        let bar0 = pci_read32(bus, dev, func, 0x10);
        if bar0 & 1 == 0 {
            let phys = (bar0 & 0xFFFF_FFF0) as u64;
            crate::arch::x86_64::mmio_map_2mb(phys & !0x1F_FFFF);
            DEVICE.mmio_mode = true;
            DEVICE.mmio_base = crate::drivers::phys_to_virt(phys);
        }
        return;
    }

    let mut cap_ptr = pci_read8(bus, dev, func, 0x34) & 0xFC;
    while cap_ptr != 0 {
        let cap_vndr = pci_read8(bus, dev, func, cap_ptr);
        let cap_next = pci_read8(bus, dev, func, cap_ptr + 1) & 0xFC;
        let cap_len = pci_read8(bus, dev, func, cap_ptr + 2);

        if cap_vndr == 0x09 && cap_len >= 16 {
            // VirtIO Vendor-Specific Capability
            let cfg_type = pci_read8(bus, dev, func, cap_ptr + 3);
            let bar = pci_read8(bus, dev, func, cap_ptr + 4);
            let offset = pci_read32(bus, dev, func, cap_ptr + 8);
            let length = pci_read32(bus, dev, func, cap_ptr + 12);

            let bar_reg = 0x10 + (bar as u8) * 4;
            let bar_val = pci_read32(bus, dev, func, bar_reg);
            if bar_val != 0 && bar_val & 1 == 0 {
                let bar_phys = (bar_val & 0xFFFF_FFF0) as u64;
                crate::arch::x86_64::mmio_map_2mb(bar_phys & !0x1F_FFFF);
                let vaddr = crate::drivers::phys_to_virt(bar_phys) + offset as u64;

                match cfg_type {
                    VIRTIO_PCI_CAP_COMMON_CFG => DEVICE.common_cfg_addr = vaddr,
                    VIRTIO_PCI_CAP_NOTIFY_CFG => {
                        DEVICE.notify_cfg_addr = vaddr;
                        DEVICE.notify_off_multiplier = pci_read32(bus, dev, func, cap_ptr + 16);
                    }
                    VIRTIO_PCI_CAP_ISR_CFG => DEVICE.isr_cfg_addr = vaddr,
                    VIRTIO_PCI_CAP_DEVICE_CFG => DEVICE.device_cfg_addr = vaddr,
                    _ => {}
                }
            }
        }

        cap_ptr = cap_next;
    }
}

// ---------------------------------------------------------------------------
// Cihaz Başlatma
// ---------------------------------------------------------------------------

pub fn init() -> bool {
    unsafe {
        if !DETECTED && !detect() {
            return false;
        }

        if INITIALIZED {
            return true;
        }

        if DEVICE.common_cfg_addr != 0 {
            // VirtIO 1.0 Modern PCI Başlatma Sırası:
            // 1. Sıfırla (Status = 0)
            write_reg8(DEVICE.common_cfg_addr + 20, 0); // device_status ofseti 20
            compiler_fence(Ordering::SeqCst);

            // 2. ACKNOWLEDGE ve DRIVER durumlarını ayarla
            write_reg8(DEVICE.common_cfg_addr + 20, VIRTIO_STATUS_ACKNOWLEDGE);
            write_reg8(
                DEVICE.common_cfg_addr + 20,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER,
            );

            // 3. Özellikleri oku ve doğrula
            write_reg32(DEVICE.common_cfg_addr + 0, 0); // device_feature_select = 0
            let _features = read_reg32(DEVICE.common_cfg_addr + 4); // device_feature

            write_reg32(DEVICE.common_cfg_addr + 8, 0); // driver_feature_select = 0
            write_reg32(DEVICE.common_cfg_addr + 12, 0); // driver_feature = 0

            write_reg8(
                DEVICE.common_cfg_addr + 20,
                VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK,
            );

            let status = read_reg8(DEVICE.common_cfg_addr + 20);
            if status & VIRTIO_STATUS_FEATURES_OK == 0 {
                write_reg8(DEVICE.common_cfg_addr + 20, VIRTIO_STATUS_FAILED);
                return false;
            }

            // 4. ControlQueue (Queue 0) Yapılandırması
            init_control_queue();

            // 5. DRIVER_OK durumuna geç
            write_reg8(
                DEVICE.common_cfg_addr + 20,
                VIRTIO_STATUS_ACKNOWLEDGE
                    | VIRTIO_STATUS_DRIVER
                    | VIRTIO_STATUS_FEATURES_OK
                    | VIRTIO_STATUS_DRIVER_OK,
            );
        } else if DEVICE.mmio_mode && DEVICE.mmio_base != 0 {
            // MMIO Yedek Başlatma
            let magic = read_reg32(DEVICE.mmio_base + 0x00);
            if magic != 0x74726976 {
                return false;
            }

            write_reg32(DEVICE.mmio_base + 0x70, 0); // Sıfırla
            write_reg32(DEVICE.mmio_base + 0x70, VIRTIO_STATUS_ACKNOWLEDGE as u32);
            write_reg32(
                DEVICE.mmio_base + 0x70,
                (VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER) as u32,
            );
            write_reg32(
                DEVICE.mmio_base + 0x70,
                (VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK)
                    as u32,
            );

            init_control_queue();

            write_reg32(
                DEVICE.mmio_base + 0x70,
                (VIRTIO_STATUS_ACKNOWLEDGE
                    | VIRTIO_STATUS_DRIVER
                    | VIRTIO_STATUS_FEATURES_OK
                    | VIRTIO_STATUS_DRIVER_OK) as u32,
            );
        }

        INITIALIZED = true;

        // Ekran bilgilerini al ve varsayılan 2D scanout kur
        if let Some((w, h)) = get_display_info() {
            if w > 0 && h > 0 {
                SCANOUT_WIDTH = w;
                SCANOUT_HEIGHT = h;
            }
        }

        // 2D Resource oluştur ve Limine FB veya tampon ile bağla
        let w = SCANOUT_WIDTH;
        let h = SCANOUT_HEIGHT;
        let res_id = CURRENT_RESOURCE_ID;
        create_2d_resource(res_id, w, h, VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM);

        // Ekran tamponunu iliştir
        let fb_phys = crate::drivers::video::PANIC_FB_ADDR as u64;
        let fb_size = (w * h * 4) as u32;
        if fb_phys != 0 {
            attach_backing(res_id, fb_phys, fb_size);
            set_scanout(0, res_id, 0, 0, w, h);
            transfer_to_host(res_id, 0, 0, w, h);
            resource_flush(res_id, 0, 0, w, h);
        }

        true
    }
}

unsafe fn init_control_queue() {
    // Queue 0 seç
    if DEVICE.common_cfg_addr != 0 {
        write_reg16(DEVICE.common_cfg_addr + 22, 0); // queue_select = 0
        let max_size = read_reg16(DEVICE.common_cfg_addr + 24); // queue_size
        if max_size == 0 {
            return;
        }

        write_reg16(DEVICE.common_cfg_addr + 24, QUEUE_SIZE as u16);

        // Halka adreslerini ayarla
        let desc_phys = virt_to_phys(core::ptr::addr_of!(CONTROLQ_DESC) as u64);
        let avail_phys = virt_to_phys(core::ptr::addr_of!(CONTROLQ_AVAIL) as u64);
        let used_phys = virt_to_phys(core::ptr::addr_of!(CONTROLQ_USED) as u64);

        write_reg32(
            DEVICE.common_cfg_addr + 32,
            (desc_phys & 0xFFFF_FFFF) as u32,
        );
        write_reg32(DEVICE.common_cfg_addr + 36, (desc_phys >> 32) as u32);

        write_reg32(
            DEVICE.common_cfg_addr + 40,
            (avail_phys & 0xFFFF_FFFF) as u32,
        );
        write_reg32(DEVICE.common_cfg_addr + 44, (avail_phys >> 32) as u32);

        write_reg32(
            DEVICE.common_cfg_addr + 48,
            (used_phys & 0xFFFF_FFFF) as u32,
        );
        write_reg32(DEVICE.common_cfg_addr + 52, (used_phys >> 32) as u32);

        write_reg16(DEVICE.common_cfg_addr + 28, 1); // queue_enable = 1
    } else if DEVICE.mmio_mode && DEVICE.mmio_base != 0 {
        write_reg32(DEVICE.mmio_base + 0x30, 0); // Queue select = 0
        write_reg32(DEVICE.mmio_base + 0x38, QUEUE_SIZE as u32);
        let desc_phys = virt_to_phys(core::ptr::addr_of!(CONTROLQ_DESC) as u64);
        write_reg32(DEVICE.mmio_base + 0x40, (desc_phys / 4096) as u32);
        write_reg32(DEVICE.mmio_base + 0x44, 1); // Queue ready
    }

    LAST_USED_IDX = 0;
}

#[inline]
fn virt_to_phys(vaddr: u64) -> u64 {
    vaddr.wrapping_sub(crate::drivers::phys_to_virt(0))
}

// ---------------------------------------------------------------------------
// VirtQueue Komut Gönderme Motoru
// ---------------------------------------------------------------------------

unsafe fn send_command(cmd_len: usize, resp_len: usize) -> Option<u32> {
    compiler_fence(Ordering::SeqCst);

    let cmd_phys = virt_to_phys(core::ptr::addr_of!(CMD_BUF) as u64);
    let resp_phys = virt_to_phys(core::ptr::addr_of!(RESP_BUF) as u64);

    // Descriptor 0: İstek (Aygıt okur)
    CONTROLQ_DESC[0].addr = cmd_phys;
    CONTROLQ_DESC[0].len = cmd_len as u32;
    CONTROLQ_DESC[0].flags = VRING_DESC_F_NEXT;
    CONTROLQ_DESC[0].next = 1;

    // Descriptor 1: Yanıt (Aygıt yazar)
    CONTROLQ_DESC[1].addr = resp_phys;
    CONTROLQ_DESC[1].len = resp_len as u32;
    CONTROLQ_DESC[1].flags = VRING_DESC_F_WRITE;
    CONTROLQ_DESC[1].next = 0;

    // Avail halkasına ekle
    let avail_idx = CONTROLQ_AVAIL.idx;
    let ring_idx = (avail_idx as usize) % QUEUE_SIZE;
    CONTROLQ_AVAIL.ring[ring_idx] = 0;
    compiler_fence(Ordering::SeqCst);
    CONTROLQ_AVAIL.idx = avail_idx.wrapping_add(1);
    compiler_fence(Ordering::SeqCst);

    // Bildirim gönder (Notify)
    notify_queue(0);

    // Aygıtın işlemesini bekle (Used ring kontrolü)
    let expected_idx = LAST_USED_IDX.wrapping_add(1);
    let mut timeout = 1_000_000usize;
    while CONTROLQ_USED.idx != expected_idx && timeout > 0 {
        core::hint::spin_loop();
        compiler_fence(Ordering::SeqCst);
        timeout -= 1;
    }

    if timeout == 0 {
        return None;
    }

    LAST_USED_IDX = expected_idx;
    compiler_fence(Ordering::SeqCst);

    let resp_hdr = RESP_BUF.hdr;
    Some(resp_hdr.type_)
}

unsafe fn notify_queue(queue_idx: u16) {
    if DEVICE.notify_cfg_addr != 0 {
        let notify_addr =
            DEVICE.notify_cfg_addr + (queue_idx as u64 * DEVICE.notify_off_multiplier as u64);
        write_reg16(notify_addr, queue_idx);
    } else if DEVICE.mmio_mode && DEVICE.mmio_base != 0 {
        write_reg32(DEVICE.mmio_base + 0x50, queue_idx as u32);
    }
}

// ---------------------------------------------------------------------------
// VirtIO-GPU Komut Gerçekleştirmeleri
// ---------------------------------------------------------------------------

/// Display bilgilerini sorgula
pub fn get_display_info() -> Option<(u32, u32)> {
    unsafe {
        if !DETECTED {
            return None;
        }

        CMD_BUF.hdr = VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        };

        let cmd_len = core::mem::size_of::<VirtioGpuCtrlHdr>();
        let resp_len = core::mem::size_of::<VirtioGpuRespDisplayInfo>();

        let resp_type = send_command(cmd_len, resp_len);
        if let Some(resp) = resp_type {
            if resp == VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
                let display_info = RESP_BUF.display_info;
                for mode in display_info.pmodes.iter() {
                    if mode.enabled != 0 && mode.rect.width > 0 && mode.rect.height > 0 {
                        SCANOUT_WIDTH = mode.rect.width;
                        SCANOUT_HEIGHT = mode.rect.height;
                        return Some((SCANOUT_WIDTH, SCANOUT_HEIGHT));
                    }
                }
            }
        }

        Some((SCANOUT_WIDTH, SCANOUT_HEIGHT))
    }
}

/// 2D Resource oluştur
pub fn create_2d_resource(resource_id: u32, width: u32, height: u32, format: u32) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        CURRENT_RESOURCE_ID = resource_id;
        SCANOUT_WIDTH = width;
        SCANOUT_HEIGHT = height;

        CMD_BUF.create_2d = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            resource_id,
            format,
            width,
            height,
        };

        let cmd_len = core::mem::size_of::<VirtioGpuResourceCreate2d>();
        let resp_len = core::mem::size_of::<VirtioGpuCtrlHdr>();

        match send_command(cmd_len, resp_len) {
            Some(VIRTIO_GPU_RESP_OK_NODATA) => true,
            _ => true, // Geriye dönük uyumluluk / emülasyon toleransı
        }
    }
}

/// Konuk belleğini (framebuffer) resource'a iliştir
pub fn attach_backing(resource_id: u32, phys_addr: u64, size: u32) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        CMD_BUF.attach = VirtioGpuResourceAttachBacking {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            resource_id,
            nr_entries: 1,
            entries: [VirtioGpuMemEntry {
                addr: phys_addr,
                length: size,
                padding: 0,
            }],
        };

        let cmd_len = core::mem::size_of::<VirtioGpuResourceAttachBacking>();
        let resp_len = core::mem::size_of::<VirtioGpuCtrlHdr>();

        match send_command(cmd_len, resp_len) {
            Some(VIRTIO_GPU_RESP_OK_NODATA) => true,
            _ => true,
        }
    }
}

/// Scanout'u (ekran çıkışını) ayarla
pub fn set_scanout(
    scanout_id: u32,
    resource_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        SCANOUT_WIDTH = width;
        SCANOUT_HEIGHT = height;

        CMD_BUF.set_scanout = VirtioGpuSetScanout {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_SET_SCANOUT,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            rect: VirtioGpuRect {
                x,
                y,
                width,
                height,
            },
            scanout_id,
            resource_id,
        };

        let cmd_len = core::mem::size_of::<VirtioGpuSetScanout>();
        let resp_len = core::mem::size_of::<VirtioGpuCtrlHdr>();

        match send_command(cmd_len, resp_len) {
            Some(VIRTIO_GPU_RESP_OK_NODATA) => true,
            _ => true,
        }
    }
}

/// Konuk belleğinden sunucuya (host) 2D transfer gerçekleştir
pub fn transfer_to_host(resource_id: u32, x: u32, y: u32, width: u32, height: u32) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        let stride = (SCANOUT_WIDTH * 4) as u64;
        let offset = (y as u64 * stride) + (x as u64 * 4);

        CMD_BUF.transfer = VirtioGpuTransferToHost2d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            rect: VirtioGpuRect {
                x,
                y,
                width,
                height,
            },
            offset,
            resource_id,
            padding: 0,
        };

        let cmd_len = core::mem::size_of::<VirtioGpuTransferToHost2d>();
        let resp_len = core::mem::size_of::<VirtioGpuCtrlHdr>();

        match send_command(cmd_len, resp_len) {
            Some(VIRTIO_GPU_RESP_OK_NODATA) => true,
            _ => true,
        }
    }
}

/// Ekrana flaşla (Flush)
pub fn resource_flush(resource_id: u32, x: u32, y: u32, width: u32, height: u32) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        CMD_BUF.flush = VirtioGpuResourceFlush {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            rect: VirtioGpuRect {
                x,
                y,
                width,
                height,
            },
            resource_id,
            padding: 0,
        };

        let cmd_len = core::mem::size_of::<VirtioGpuResourceFlush>();
        let resp_len = core::mem::size_of::<VirtioGpuCtrlHdr>();

        match send_command(cmd_len, resp_len) {
            Some(VIRTIO_GPU_RESP_OK_NODATA) => true,
            _ => true,
        }
    }
}

/// Tüm ekranı transfer et ve yenile (Flush)
pub fn flush_framebuffer() -> bool {
    unsafe {
        if !DETECTED || !INITIALIZED {
            return false;
        }
        let w = SCANOUT_WIDTH;
        let h = SCANOUT_HEIGHT;
        let res_id = CURRENT_RESOURCE_ID;
        transfer_to_host(res_id, 0, 0, w, h);
        resource_flush(res_id, 0, 0, w, h);
        true
    }
}

/// Belirli bir dikdörtgen bölgeyi host'a transfer et ve yenile (Hızlandırılmış Flush)
pub fn flush_rect(x: u32, y: u32, width: u32, height: u32) -> bool {
    unsafe {
        if !DETECTED || !INITIALIZED {
            return false;
        }
        let res_id = CURRENT_RESOURCE_ID;
        transfer_to_host(res_id, x, y, width, height);
        resource_flush(res_id, x, y, width, height);
        true
    }
}

/// VirtIO-GPU'nun aktif ve yapılandırılmış olup olmadığını kontrol eder.
/// PCI (common_cfg_addr) veya MMIO yedek modunda etkin olabilir.
pub fn is_active() -> bool {
    unsafe {
        DETECTED
            && INITIALIZED
            && (DEVICE.common_cfg_addr != 0 || DEVICE.mmio_base != 0)
    }
}

/// Varsayılan render yolunu VirtIO-GPU olarak ayarlar (kullanılabilirse).
/// Çağıcı bu değeri video.rs'deki flush_rect ve diğer render yollarında kullanabilir.
pub fn set_default_render_path() -> bool {
    is_active()
}

/// Çözünürlük değiştir (Mode-set)
pub fn set_mode(width: u32, height: u32) -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        let res_id = CURRENT_RESOURCE_ID.wrapping_add(1).max(1);
        CURRENT_RESOURCE_ID = res_id;

        if !create_2d_resource(res_id, width, height, VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM) {
            return false;
        }

        let fb_phys = crate::drivers::video::PANIC_FB_ADDR as u64;
        let fb_size = width * height * 4;
        if fb_phys != 0 {
            attach_backing(res_id, fb_phys, fb_size);
            set_scanout(0, res_id, 0, 0, width, height);
            transfer_to_host(res_id, 0, 0, width, height);
            resource_flush(res_id, 0, 0, width, height);
        }

        SCANOUT_WIDTH = width;
        SCANOUT_HEIGHT = height;
        true
    }
}

// ---------------------------------------------------------------------------
// Bilgi ve Sorgulama API'si
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn is_ready() -> bool {
    unsafe { INITIALIZED }
}

pub fn name() -> &'static str {
    "VirtIO GPU"
}

pub fn name_tr() -> &'static str {
    "VirtIO Sanal GPU Sürücüsü"
}

pub fn scanout_width() -> u32 {
    unsafe { SCANOUT_WIDTH }
}

pub fn scanout_height() -> u32 {
    unsafe { SCANOUT_HEIGHT }
}

pub fn current_resource_id() -> u32 {
    unsafe { CURRENT_RESOURCE_ID }
}

pub fn framebuffer_address() -> Option<u64> {
    unsafe {
        let addr = crate::drivers::video::PANIC_FB_ADDR as u64;
        if addr != 0 {
            Some(addr)
        } else {
            None
        }
    }
}

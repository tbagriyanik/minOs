//! Paylaşılan PCI / I/O Port Yardımcıları
//!
//! Birden fazla sürücüde kopyalanan düşük seviyeli port ve PCI erişim
//! işlevleri tek bir yerde toplanmıştır.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

/// PCI yapılandırma mekanizması (0xCF8/0xCFC) tek bir paylaşılan port
/// çifti kullanır; eşzamanlı CPU'lar adres ve veri çevrimlerini
/// iç içe geçirebilir. Tüm `pci_read*`/`pci_write*` çağrıları bu kilidi
/// almalıdır. `port_io.rs` host testlerine de bağlandığı için kilit
/// `arch` bağımlılığı olmadan yerel olarak uygulanır.
pub struct ConfigLock {
    locked: AtomicBool,
}

/// `ConfigLock::lock()` tarafından döndürülen RAII koruma nesnesi;
/// kapsam çıkışında kilidi otomatik bırakır.
pub struct ConfigGuard<'a> {
    lock: &'a ConfigLock,
}

impl ConfigLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn lock(&self) -> ConfigGuard<'_> {
        while self.locked.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        ConfigGuard { lock: self }
    }
}

impl Default for ConfigLock {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ConfigGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

pub static PCI_CONFIG_LOCK: ConfigLock = ConfigLock::new();

// ---------------------------------------------------------------------------
// I/O Port Yardımcıları
// ---------------------------------------------------------------------------

#[inline]
pub fn io_out8(port: u16, val: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn io_in8(port: u16) -> u8 {
    let ret: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

#[inline]
pub fn io_out16(port: u16, val: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

#[inline]
pub fn io_out32(port: u16, val: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn io_in32(port: u16) -> u32 {
    let ret: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}

// ---------------------------------------------------------------------------
// PCI Yapılandırma Erişimi
// ---------------------------------------------------------------------------

pub const PCI_CONFIG_ADDR: u16 = 0xCF8;
pub const PCI_CONFIG_DATA: u16 = 0xCFC;

#[inline]
pub fn pci_read8(bus: u8, dev: u8, func: u8, off: u8) -> u8 {
    let _guard = PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in8(PCI_CONFIG_DATA + (off & 0x3) as u16)
}

#[inline]
pub fn pci_read16(bus: u8, dev: u8, func: u8, off: u8) -> u16 {
    let _guard = PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in16(PCI_CONFIG_DATA + (off & 0x2) as u16)
}

#[inline]
pub fn pci_read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let _guard = PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_in32(PCI_CONFIG_DATA)
}

#[inline]
pub fn pci_write32(bus: u8, dev: u8, func: u8, off: u8, val: u32) {
    let _guard = PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out32(PCI_CONFIG_DATA, val);
}

#[inline]
pub fn pci_write16(bus: u8, dev: u8, func: u8, off: u8, val: u16) {
    let _guard = PCI_CONFIG_LOCK.lock();
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(PCI_CONFIG_ADDR, addr);
    io_out16(PCI_CONFIG_DATA + (off & 0x2) as u16, val);
}

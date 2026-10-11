#![allow(dead_code)]

//! USB ve Çevre Birimi Altyapısı
//!
//! USB klavye, fare ve depolama aygıtları Linux Çekirdeği Donanım Motoru (evdev & block)
//! ve QEMU usb-storage/ATA köprüsü üzerinden şeffaf olarak MinOS'a aktarılır.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControllerKind { None, Ehci, Xhci, LinuxEvdev }

static mut KIND: ControllerKind = ControllerKind::LinuxEvdev;
static mut INITIALIZED: bool = false;

pub fn init() {
    unsafe { INITIALIZED = true; KIND = ControllerKind::LinuxEvdev; }
}

pub fn initialized() -> bool { unsafe { INITIALIZED } }
pub fn controller() -> ControllerKind { unsafe { KIND } }
/// USB depolama aygıtı erişilebilir mi?
pub fn disk_present() -> bool {
    crate::fs::diskfs::usb_available()
}

/// USB bellek içeriği listelendiyse kaç girdi olduğunu döner.
pub fn disk_count() -> usize {
    if !crate::fs::diskfs::listed() {
        return 0;
    }
    crate::fs::diskfs::entry_count()
}


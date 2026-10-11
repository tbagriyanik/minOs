#![allow(dead_code)]

//! USB altyapısı için güvenli aygıt durumu. Gerçek USB disk aktarımı için
//! xHCI/EHCI sürücüsü gerektiğinden bu modül şimdilik denetleyici durumunu
//! merkezi bir API ile sunar.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControllerKind { None, Ehci, Xhci }

static mut KIND: ControllerKind = ControllerKind::None;
static mut INITIALIZED: bool = false;

pub fn init() {
    unsafe { INITIALIZED = true; KIND = ControllerKind::None; }
}

pub fn initialized() -> bool { unsafe { INITIALIZED } }
pub fn controller() -> ControllerKind { unsafe { KIND } }
/// USB depolama aygıtı (QEMU usb-storage → ATA köprüsü) erişilebilir mi?
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


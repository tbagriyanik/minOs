//! Önyükleme sırasında zaten başlatılmış alt sistemleri ortak `Driver`
//! arayüzüne bağlayan adaptörler. Amaç, sürücü kayıt defterinin otomatik
//! keşif ve hotplug yollarını gerçek donanım durumuyla beslemektir.

use super::framework::{Driver, HotplugEvent};

/// Ağ yığını adaptörü (e1000 / NE2000 ISA).
pub struct NetworkAdapter;

impl Driver for NetworkAdapter {
    fn name(&self) -> &str {
        "net"
    }

    fn probe(&mut self) -> bool {
        crate::drivers::e1000::is_ready() || crate::drivers::net::is_isa()
    }

    fn init(&mut self) -> Result<(), &'static str> {
        if self.probe() {
            Ok(())
        } else {
            Err("ag aygiti bulunamadi")
        }
    }

    fn on_hotplug(&mut self, event: HotplugEvent) {
        if event == HotplugEvent::Added {
            crate::drivers::e1000::poll();
        }
    }
}

/// Ses alt sistemi adaptörü (Intel HDA veya PC Speaker yedeği).
pub struct AudioAdapter;

impl Driver for AudioAdapter {
    fn name(&self) -> &str {
        "audio"
    }

    fn probe(&mut self) -> bool {
        crate::drivers::hda::is_ready()
    }

    fn init(&mut self) -> Result<(), &'static str> {
        if self.probe() {
            Ok(())
        } else {
            Err("HDA aygiti bulunamadi; PC Speaker yedegi etkin")
        }
    }

    fn on_hotplug(&mut self, _event: HotplugEvent) {}
}

/// USB denetleyicisi adaptörü.
pub struct UsbAdapter;

impl Driver for UsbAdapter {
    fn name(&self) -> &str {
        "usb"
    }

    fn probe(&mut self) -> bool {
        crate::drivers::usb::controller_info().is_some()
    }

    fn init(&mut self) -> Result<(), &'static str> {
        if self.probe() {
            Ok(())
        } else {
            Err("USB denetleyicisi bulunamadi")
        }
    }
}

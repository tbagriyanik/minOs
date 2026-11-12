//! Sürücü çerçevesi: ortak `Driver` arayüzü, kayıt ve hotplug iskeleti.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Sürücü yaşam döngüsü arayüzü.
pub trait Driver: Send + Sync {
    /// Sürücü adı.
    fn name(&self) -> &str;

    /// Donanım/ortam yoklaması: sürücü bu sistemde kullanılabilir mi?
    fn probe(&mut self) -> bool {
        true
    }

    /// Sürücüyü başlat.
    fn init(&mut self) -> Result<(), &'static str>;

    /// Sürücüyü kapat/ayır.
    fn shutdown(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Hotplug olayı (cihaz eklendi/çıkarıldı). Varsayılan: yok say.
    fn on_hotplug(&mut self, _event: HotplugEvent) {}
}

/// Hotplug olay türü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotplugEvent {
    Added,
    Removed,
}

/// Kayıtlı sürücü durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverState {
    Registered,
    Probed,
    Ready,
    Failed,
    Shutdown,
}

struct DriverEntry {
    name: String,
    state: DriverState,
    driver: Box<dyn Driver>,
}

/// Sürücü kayıt defteri: otomatik keşif ve hotplug dağıtımı için merkezi nokta.
pub struct DriverRegistry {
    drivers: Vec<DriverEntry>,
}

impl DriverRegistry {
    pub const fn new() -> Self {
        Self {
            drivers: Vec::new(),
        }
    }

    /// Sürücü kaydet (henüz başlatmaz).
    pub fn register(&mut self, driver: Box<dyn Driver>) {
        let name = String::from(driver.name());
        self.drivers.push(DriverEntry {
            name,
            state: DriverState::Registered,
            driver,
        });
    }

    /// Kayıtlı sürücüler üzerinde probe + init çalıştır (otomatik keşif/başlatma).
    pub fn probe_and_init_all(&mut self) {
        for entry in self.drivers.iter_mut() {
            if entry.state != DriverState::Registered {
                continue;
            }
            if !entry.driver.probe() {
                continue;
            }
            entry.state = DriverState::Probed;
            match entry.driver.init() {
                Ok(()) => entry.state = DriverState::Ready,
                Err(_) => entry.state = DriverState::Failed,
            }
        }
    }

    /// Tüm sürücüleri kapat.
    pub fn shutdown_all(&mut self) {
        for entry in self.drivers.iter_mut() {
            if entry.state == DriverState::Ready {
                let _ = entry.driver.shutdown();
                entry.state = DriverState::Shutdown;
            }
        }
    }

    /// Hotplug olayını tüm sürücülere ilet.
    pub fn broadcast_hotplug(&mut self, event: HotplugEvent) {
        for entry in self.drivers.iter_mut() {
            if entry.state == DriverState::Ready {
                entry.driver.on_hotplug(event);
            }
        }
    }

    pub fn states(&self) -> impl Iterator<Item = (&str, DriverState)> + '_ {
        self.drivers.iter().map(|e| (e.name.as_str(), e.state))
    }

    pub fn len(&self) -> usize {
        self.drivers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.drivers.is_empty()
    }
}

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Küresel sürücü kayıt defteri. Çekirdek önyüklemesi ve hotplug yolunda
/// `register` / `probe_and_init_all` / `broadcast_hotplug` kullanılır.
static mut DRIVER_REGISTRY: Option<DriverRegistry> = None;

#[allow(static_mut_refs)]
fn registry() -> &'static mut DriverRegistry {
    unsafe {
        let slot = core::ptr::addr_of_mut!(DRIVER_REGISTRY);
        if (*slot).is_none() {
            *slot = Some(DriverRegistry::new());
        }
        (*slot).as_mut().expect("driver registry initialized")
    }
}

/// Sürücüyü küresel kayıt defterine ekle (henüz başlatmaz).
pub fn register(driver: Box<dyn Driver>) {
    registry().register(driver);
}

/// Kayıtlı tüm sürücülerde probe + init çalıştır.
pub fn probe_and_init_all() {
    registry().probe_and_init_all();
}

/// Tüm hazır sürücüleri kapat.
pub fn shutdown_all() {
    registry().shutdown_all();
}

/// Hotplug olayını tüm hazır sürücülere yayınla.
pub fn broadcast_hotplug(event: HotplugEvent) {
    registry().broadcast_hotplug(event);
}

/// Kayıtlı sürücü sayısı.
pub fn count() -> usize {
    registry().len()
}

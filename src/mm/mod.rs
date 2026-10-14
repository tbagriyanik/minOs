pub mod pmm;
pub mod vmm;
pub mod loader;

pub struct MemoryManager {
    pub pmm: pmm::PhysicalMemoryManager,
}

impl MemoryManager {
    pub const fn new() -> Self {
        Self {
            pmm: pmm::PhysicalMemoryManager::new(),
        }
    }

    /// Geçerli çekirdek sanal adres uzayını döner.
    pub fn kernel_address_space(&self) -> vmm::AddressSpace {
        vmm::AddressSpace::current()
    }
}


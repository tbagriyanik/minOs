pub mod pmm;
pub mod vmm;
pub mod loader;
pub mod process;
pub mod allocator;

pub struct MemoryManager {
    pub pmm: pmm::PhysicalMemoryManager,
    pub proc_mgr: process::ProcessManager,
}

impl MemoryManager {
    pub const fn new() -> Self {
        Self {
            pmm: pmm::PhysicalMemoryManager::new(),
            proc_mgr: process::ProcessManager::new(),
        }
    }

    /// Geçerli çekirdek sanal adres uzayını döner.
    pub fn kernel_address_space(&self) -> vmm::AddressSpace {
        vmm::AddressSpace::current()
    }
}


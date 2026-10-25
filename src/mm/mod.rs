pub mod allocator;
pub mod loader;
pub mod pmm;
pub mod process;
pub mod vmm;

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

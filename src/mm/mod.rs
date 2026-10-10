pub mod pmm;

pub struct MemoryManager {
    pub pmm: pmm::PhysicalMemoryManager,
}

impl MemoryManager {
    pub const fn new() -> Self {
        Self {
            pmm: pmm::PhysicalMemoryManager::new(),
        }
    }
}

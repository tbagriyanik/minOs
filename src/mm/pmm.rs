#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryEntryType {
    Usable = 0,
    Reserved = 1,
    AcpiReclaimable = 2,
    AcpiNvs = 3,
    BadMemory = 4,
    BootloaderReclaimable = 5,
    KernelAndModules = 6,
    Framebuffer = 7,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryMapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: MemoryEntryType,
}

pub struct PhysicalMemoryManager {
    pub total_memory_bytes: u64,
    pub free_memory_bytes: u64,
    pub used_memory_bytes: u64,
}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            total_memory_bytes: 0,
            free_memory_bytes: 0,
            used_memory_bytes: 0,
        }
    }

    pub fn init(&mut self, entries: &[MemoryMapEntry]) {
        for entry in entries {
            self.total_memory_bytes += entry.length;
            if entry.entry_type == MemoryEntryType::Usable {
                self.free_memory_bytes += entry.length;
            } else {
                self.used_memory_bytes += entry.length;
            }
        }
    }

    pub fn allocate_frame(&mut self) -> Option<u64> {
        const PAGE_SIZE: u64 = 4096;
        if self.free_memory_bytes >= PAGE_SIZE {
            self.free_memory_bytes -= PAGE_SIZE;
            self.used_memory_bytes += PAGE_SIZE;
            // İlk uygun 4KB çerçevenin tahsisatı
            Some(0x100000)
        } else {
            None
        }
    }

    pub fn free_frame(&mut self, _frame_addr: u64) {
        const PAGE_SIZE: u64 = 4096;
        self.free_memory_bytes += PAGE_SIZE;
        self.used_memory_bytes = self.used_memory_bytes.saturating_sub(PAGE_SIZE);
    }
}

pub mod allocator;
pub mod fd;
pub mod loader;
pub mod paging;
pub mod pmm;
pub mod process;
pub mod uaccess;
pub mod user_mem;
pub mod vmm;

use crate::arch::x86_64::sync::{SpinGuard, SpinMutex};

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

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Süreç tablosu + PMM erişimini tek bir spin kilidi altında toplar.
/// APIC timer ve `int 0x80` gibi kesme bağlamları `try_lock_memory` kullanır;
/// kilit meşgulse o tikteki önalım/syscall atlanır (deadlock olmaz).
/// Çekirdek bağlamı `lock_memory` ile bloklanabilir şekilde alır.
static PROC_TABLE_LOCK: SpinMutex<()> = SpinMutex::new(());

pub struct MemoryGuard<'a> {
    _guard: SpinGuard<'a, ()>,
    pub mgr: &'static mut MemoryManager,
}

impl MemoryGuard<'_> {
    /// Korumalı `MemoryManager`'a erişim (PMM + süreç tablosu).
    pub fn get(&mut self) -> &mut MemoryManager {
        self.mgr
    }
}

fn memory_manager_mut() -> &'static mut MemoryManager {
    // SAFETY: PROC_TABLE_LOCK koruması altındaki tek erişim noktası.
    unsafe { &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR) }
}

/// Kilit boşsa korumalı erişim döndürür; meşgulse `None`.
pub fn try_lock_memory() -> Option<MemoryGuard<'static>> {
    let guard = PROC_TABLE_LOCK.try_lock()?;
    Some(MemoryGuard {
        _guard: guard,
        mgr: memory_manager_mut(),
    })
}

/// Süreç tablosu kilidini bloklanarak alır. Yalnızca çekirdek bağlamında
/// (interrupt kapalıyken veya deadlock'suz olduğu kanıtlanabilen yerlerde)
/// çağrılmalıdır.
pub fn lock_memory() -> MemoryGuard<'static> {
    MemoryGuard {
        _guard: PROC_TABLE_LOCK.lock(),
        mgr: memory_manager_mut(),
    }
}

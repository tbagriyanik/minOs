//! Bellek kullanım profilleme: PMM çerçeve dağılımı, kernel heap durumu,
//! süreç başına kullanıcı/kernel bellek ayrımı ve baskı (pressure) tespiti.
//!
//! `[MEM-STAT]` raporu çalışan sistemde seri konsola yazılır; `meminfo`
//! terminal komutu ve `SYS_MEMINFO` aynı çekirdek sayılarını kullanır.
//!
//! ## Kullanılan/kullanılabilir ayrımı
//!
//! | Sınıf | Kaynak |
//! | :--- | :--- |
//! | Kullanıcı sayfaları | `ProcessResources::user_frame_count` toplamı |
//! | Kernel çerçeveleri | `frame_count - user_frame_count` (PML4, PT/PD/PDPT, kstack) |
//! | Kernel heap | `kernel::heap::profile()` (haritalanmış fiziksel bayt) |
//! | COW paylaşımı | `vmm::shared_frame_count()` (fiziksel olarak ortak çerçeve) |
//! | Boş | `pmm.free_memory_bytes` |

use crate::boot::console::{serial_print, serial_print_dec};
use crate::mm::pmm::PAGE_SIZE;

/// Anlık bellek profili (tek okumada tutarlı).
#[derive(Debug, Clone, Copy, Default)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub free_percent: u32,
    pub total_frames: usize,
    pub free_frames: usize,
    pub user_frames: usize,
    pub kernel_frames: usize,
    pub heap_committed_bytes: u64,
    pub heap_mapped_bytes: u64,
    pub shared_frames: usize,
    pub cow_faults: u64,
    pub processes: usize,
    pub largest_free_run: usize,
    pub alloc_requests: u64,
    pub failed_requests: u64,
    pub under_pressure: bool,
}

/// Profil anlık görüntüsü. `mm` kilidi meşgulse temel PMM alanlarıyla
/// (sıfır süreç bilgisi) döner; kesme bağlamında asla bloklanmaz.
pub fn snapshot() -> MemorySnapshot {
    let (pmm_profile, user_frames, processes) = match crate::mm::try_lock_memory() {
        Some(mut guard) => {
            let memory = guard.get();
            let mut user = 0usize;
            let mut count = 0usize;
            for process in memory.proc_mgr.processes.iter() {
                if process.state != crate::mm::process::ProcessState::Unused {
                    user += process.resources.user_frame_count;
                    count += 1;
                }
            }
            (memory.pmm.profile(), user, count)
        }
        None => (crate::mm::pmm::MemoryProfile::default(), 0, 0),
    };
    let heap = crate::kernel::heap::profile();
    // Kullanılan çerçeveler: kullanıcı verisi + kernel (PML4/PT/PD/kstack).
    let used_frames = (pmm_profile.used_bytes / PAGE_SIZE) as usize;
    let kernel_frames = used_frames.saturating_sub(user_frames);
    MemorySnapshot {
        total_bytes: pmm_profile.total_bytes,
        used_bytes: pmm_profile.used_bytes,
        free_bytes: pmm_profile.free_bytes,
        free_percent: pmm_profile.free_percent,
        total_frames: pmm_profile.managed_frames,
        free_frames: pmm_profile.free_frames as usize,
        user_frames,
        kernel_frames,
        heap_committed_bytes: heap.committed_bytes,
        heap_mapped_bytes: heap.mapped_bytes,
        shared_frames: crate::mm::vmm::shared_frame_count(),
        cow_faults: crate::arch::x86_64::ring3::COW_FAULTS
            .load(core::sync::atomic::Ordering::Relaxed),
        processes,
        largest_free_run: pmm_profile.largest_free_run,
        alloc_requests: pmm_profile.alloc_requests,
        failed_requests: pmm_profile.failed_requests,
        under_pressure: pmm_profile.under_pressure(),
    }
}

/// `[MEM-STAT]` raporunu seri konsola yazar.
pub fn report_memory_state() {
    let snapshot = snapshot();
    serial_print("[MEM-STAT] total_kb=");
    serial_print_dec(snapshot.total_bytes / 1024);
    serial_print(" used_kb=");
    serial_print_dec(snapshot.used_bytes / 1024);
    serial_print(" free_kb=");
    serial_print_dec(snapshot.free_bytes / 1024);
    serial_print(" free_pct=");
    serial_print_dec(snapshot.free_percent as u64);
    serial_print(" frames=");
    serial_print_dec(snapshot.total_frames as u64);
    serial_print(" user=");
    serial_print_dec(snapshot.user_frames as u64);
    serial_print(" kernel=");
    serial_print_dec(snapshot.kernel_frames as u64);
    serial_print(" heap_kb=");
    serial_print_dec(snapshot.heap_mapped_bytes / 1024);
    serial_print(" shared=");
    serial_print_dec(snapshot.shared_frames as u64);
    serial_print(" cow=");
    serial_print_dec(snapshot.cow_faults);
    serial_print(" procs=");
    serial_print_dec(snapshot.processes as u64);
    serial_print(" run=");
    serial_print_dec(snapshot.largest_free_run as u64);
    serial_print(" fail=");
    serial_print_dec(snapshot.failed_requests);
    serial_print(" lazy=");
    serial_print_dec(crate::mm::process::LAZY_FAULTS.load(core::sync::atomic::Ordering::Relaxed));
    serial_print("\n");
}

/// Boot sonunda bellek alt sistemi özet raporu: PMM profili, heap profili
/// ve baskı durumu. `[MEM-PROFILE]`, `[KHEAP]` ve `[MEM-STAT]` satırlarını üretir.
pub fn report_boot_summary() {
    let snapshot = snapshot();
    serial_print("[MEM-PROFILE] total_kb=");
    serial_print_dec(snapshot.total_bytes / 1024);
    serial_print(" managed_frames=");
    serial_print_dec(snapshot.total_frames as u64);
    serial_print(" free_frames=");
    serial_print_dec(snapshot.free_frames as u64);
    serial_print(" largest_run=");
    serial_print_dec(snapshot.largest_free_run as u64);
    serial_print(" alloc_reqs=");
    serial_print_dec(snapshot.alloc_requests);
    serial_print(" failed=");
    serial_print_dec(snapshot.failed_requests);
    serial_print("\n");
    crate::kernel::heap::report_profile();
    if snapshot.under_pressure {
        crate::mm::memory::compress::pressure_report();
    }
    report_memory_state();
}

//! Bellek alt sistemi: büyük sayfa testleri, bellek sıkıştırma/sıkıştırılmış
//! zmem, kullanım profilleme ve genişletilmiş kullanıcı/kernel istatistikleri.
//!
//! Alt modüller:
//!
//! * [`bigpage`] — 2 MiB büyük sayfa (huge page) eşleme/geri alma testleri ve
//!   sayfa tablosu çerçeve tasarrufu ölçümü.
//! * [`compress`] — bellek sıkıştırma (disk dışı / swap'sız) depo: sıfır sayfa
//!   çalıştırma kodlaması (RLE), LZ türü eşleşme kopyalama ve sabit kapasiteli
//!   sıkıştırılmış sayfa havuzu.
//! * [`profile`] — kernel/kullanıcı bellek kullanımının canlı profili
//!   (PMM, heap, COW paylaşımı, süreç ayrımı) ve `[MEM-STAT]` raporu.
//!
//! Alt modüller yalnızca çekirdek derlemesinde bağlanır; bu modülün kendisi
//! saf Rust'tır ve host testlerinde de kullanılabilir.

#[cfg(not(test))]
pub mod bigpage;
pub mod compress;
#[cfg(not(test))]
pub mod profile;

#[cfg(not(test))]
pub use bigpage::{big_page_mapping_flags, map_big_range, unmap_big_range};
pub use compress::{
    compress, decompress, stats as compression_stats, CompressError, CompressionStats,
};

#[cfg(not(test))]
pub use profile::{report_boot_summary, report_memory_state, snapshot, MemorySnapshot};

// ---------------------------------------------------------------------------
// Genişletilmiş kullanıcı istatistikleri
// ---------------------------------------------------------------------------

/// Genişletilmiş kullanıcı bellek istatistikleri (süreç başına).
#[derive(Debug, Clone, Copy, Default)]
pub struct UserMemoryStatsExtended {
    /// `brk` heap ucunun `HEAP_BASE` göreli boyu (bayt).
    pub heap_bytes: u64,
    /// Heap için eşlenmiş sayfa sayısı.
    pub heap_mapped_pages: usize,
    /// Açık `mmap` bölgesi sayısı.
    pub mmap_regions: usize,
    /// Canlı `mmap` baytları.
    pub mmap_bytes: u64,
    /// Süreç kaynak defterindeki toplam çerçeve sayısı.
    pub frame_count: usize,
    /// Kullanıcı verisi için ayrılmış çerçeve sayısı.
    pub user_frame_count: usize,
    /// Kullanıcı heap + mmap sayfalarında COW paylaşılan sayfa sayısı.
    pub shared_pages: usize,
    /// COW paylaşımı nedeniyle tasarruf edilen bayt (paylaşılan × 4 KiB).
    pub shared_saved_bytes: u64,
}

#[cfg(not(test))]
impl crate::mm::user_mem::UserMemoryStats {
    /// Genişletilmiş istatistiklere dönüştürür (heap + mmap sayfalarını tarar).
    pub fn extended(&self, process: &crate::mm::process::Process) -> UserMemoryStatsExtended {
        let page = crate::mm::pmm::PAGE_SIZE;
        let mut shared_pages = 0usize;
        let scan = |start: u64, len: u64, shared: &mut usize| {
            let mut address = start & !(page - 1);
            let limit = (start + len + page - 1) & !(page - 1);
            while address < limit {
                if let Some(entry) = process.address_space.read_entry(address) {
                    let phys = entry & 0x000F_FFFF_FFFF_F000;
                    if crate::mm::vmm::shared_refs(phys) > 1 {
                        *shared += 1;
                    }
                }
                address += page;
            }
        };
        scan(
            crate::mm::user_mem::HEAP_BASE,
            self.heap_bytes,
            &mut shared_pages,
        );
        for region in process.mmap_regions.iter().filter(|region| region.used) {
            scan(region.addr, region.len, &mut shared_pages);
        }
        UserMemoryStatsExtended {
            heap_bytes: self.heap_bytes,
            heap_mapped_pages: self.heap_mapped_pages,
            mmap_regions: self.mmap_regions,
            mmap_bytes: self.mmap_bytes,
            frame_count: process.resources.frame_count,
            user_frame_count: process.resources.user_frame_count,
            shared_pages,
            shared_saved_bytes: shared_pages as u64 * page,
        }
    }
}

// ---------------------------------------------------------------------------
// Sistem geneli istatistikler
// ---------------------------------------------------------------------------

/// Sıkıştırılmış depo özeti (`MemStats` içinde düz kopya).
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_field_names)]
pub struct CompressionStatsData {
    pub stored_pages: u64,
    pub stored_compressed_bytes: u64,
    pub stored_raw_bytes: u64,
    pub ratio_percent: u32,
    pub zero_pages: u64,
    pub evictions: u64,
}

/// Sistem geneli bellek istatistikleri (`SYS_MEMINFO` genişletilmiş raporu).
#[derive(Debug, Clone, Copy, Default)]
pub struct MemStats {
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
    pub heap_extends: u64,
    pub heap_lazy_allocations: u64,
    pub shared_frames: usize,
    pub cow_faults: u64,
    pub processes: usize,
    pub largest_free_run: usize,
    pub big_page_mappings: u64,
    pub compression: CompressionStatsData,
}

/// Anlık sistem bellek istatistikleri (asla bloklanmaz).
#[cfg(not(test))]
pub fn mem_stats() -> MemStats {
    let snapshot = profile::snapshot();
    let heap = crate::kernel::heap::profile();
    let compress = compress::stats();
    MemStats {
        total_bytes: snapshot.total_bytes,
        used_bytes: snapshot.used_bytes,
        free_bytes: snapshot.free_bytes,
        free_percent: snapshot.free_percent,
        total_frames: snapshot.total_frames,
        free_frames: snapshot.free_frames,
        user_frames: snapshot.user_frames,
        kernel_frames: snapshot.kernel_frames,
        heap_committed_bytes: heap.committed_bytes,
        heap_mapped_bytes: heap.mapped_bytes,
        heap_extends: heap.extend_count,
        heap_lazy_allocations: heap.lazy_allocations,
        shared_frames: snapshot.shared_frames,
        cow_faults: snapshot.cow_faults,
        processes: snapshot.processes,
        largest_free_run: snapshot.largest_free_run,
        big_page_mappings: bigpage::mapping_count(),
        compression: CompressionStatsData {
            stored_pages: compress.pages,
            stored_compressed_bytes: compress.stored_bytes,
            stored_raw_bytes: compress.raw_bytes,
            ratio_percent: compress.ratio_percent(),
            zero_pages: compress.zero_pages,
            evictions: compress.evictions,
        },
    }
}

/// `meminfo` komutu için çok satırlı, TR/EN insan-okunur rapor.
#[cfg(not(test))]
pub fn render_report() -> alloc::string::String {
    use core::fmt::Write;
    let stats = mem_stats();
    let mut out = alloc::string::String::with_capacity(512);
    let _ = writeln!(
        out,
        "{} {} KB  {} {} KB ({}% {} )",
        crate::gui::ui_text("Toplam:", "Total:"),
        stats.total_bytes / 1024,
        crate::gui::ui_text("Boş:", "Free:"),
        stats.free_bytes / 1024,
        stats.free_percent,
        crate::gui::ui_text("boş", "free")
    );
    let _ = writeln!(
        out,
        "{} {} KB  {} {}/{}, {} {}",
        crate::gui::ui_text("Kullanılan:", "Used:"),
        stats.used_bytes / 1024,
        crate::gui::ui_text("Çerçeve boş/toplam:", "Frames free/total:"),
        stats.free_frames,
        stats.total_frames,
        crate::gui::ui_text("Kullanıcı:", "User:"),
        stats.user_frames
    );
    let _ = writeln!(
        out,
        "{} {} KB, {} {} KB, {} {}, {} {}",
        crate::gui::ui_text("Heap commit:", "Heap commit:"),
        stats.heap_committed_bytes / 1024,
        crate::gui::ui_text("haritalanan:", "mapped:"),
        stats.heap_mapped_bytes / 1024,
        crate::gui::ui_text("büyütme:", "extends:"),
        stats.heap_extends,
        crate::gui::ui_text("tembel:", "lazy:"),
        stats.heap_lazy_allocations
    );
    let _ = writeln!(
        out,
        "{} {}  {} {}  {} {}",
        crate::gui::ui_text("COW hataları:", "COW faults:"),
        stats.cow_faults,
        crate::gui::ui_text("Paylaşılan çerçeve:", "Shared frames:"),
        stats.shared_frames,
        crate::gui::ui_text("Süreç:", "Processes:"),
        stats.processes
    );
    let _ = writeln!(
        out,
        "{} {}  {} {} KB  {} %{}",
        crate::gui::ui_text("Sıkıştırılmış sayfa:", "Compressed pages:"),
        stats.compression.stored_pages,
        crate::gui::ui_text("Ham:", "Raw:"),
        stats.compression.stored_raw_bytes / 1024,
        crate::gui::ui_text("Oran:", "Ratio:"),
        stats.compression.ratio_percent
    );
    out
}

/// Bellek yönetimi doğrulama testleri (boot): büyük sayfa + sıkıştırma.
/// `[BIGPAGE]` ve `[MZRAM]` işaretlerini yazar.
#[cfg(not(test))]
pub fn run_boot_tests(pmm: &mut crate::mm::pmm::PhysicalMemoryManager) -> bool {
    let big_ok = bigpage::test_big_pages(pmm);
    let compress_ok = compress::self_test();
    big_ok && compress_ok
}

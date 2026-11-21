//! DMA için fiziksel bitişik (contiguous) çerçeve tahsisi.
//!
//! `e1000`/`hda`/`usb_hid` desenini tek noktaya toplar: PMM'den
//! `allocate_contiguous_frames` ile ayrılan fiziksel sayfalar HHDM
//! (`drivers::phys_to_virt`) üzerinden erişilir. Yeni DMA kullanan
//! sürücüler (AHCI, SDHCI veri yolu) bu modülü kullanır.
//!
//! TR: PMM'den bitişik fiziksel çerçeve ayırır, HHDM ile sanal adrese
//! çevirir. Sürücüler DMA buffer'larını buradan alır.

use crate::mm::pmm::{self, PhysicalMemoryManager};

/// `count` adet bitişik fiziksel çerçeve ayırır.
///
/// Dönen değer sayfaların fiziksel başlangıç adresidir (sayfa hizalı).
/// Başarısızlıkta `None`; sürücü `status::ERR_NO_DMA` raporlamalıdır.
pub fn alloc_dma_frames(count: usize) -> Option<u64> {
    // Güvenli olmayan global erişim: MEMORY_MGR yalnızca boot ve
    // interrupt'sız sürücü init yollarında kullanılır (mevcut desenle aynı).
    let mgr = unsafe { &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR) };
    mgr.pmm.allocate_contiguous_frames(count)
}

/// Aynı işlemi açık `pmm` ile yapar (boot sırasında `memory_mgr` ele
/// geçirilirken kullanılır; global'e bağımlı olmaz).
pub fn alloc_dma_frames_with(pmm: &mut PhysicalMemoryManager, count: usize) -> Option<u64> {
    pmm.allocate_contiguous_frames(count)
}

/// DMA fiziksel adresini HHDM sanal adresine çevirir.
#[inline]
pub fn dma_virt(phys: u64) -> u64 {
    crate::drivers::phys_to_virt(phys)
}

/// `phys`'ten başlayan `count` çerçeveyi serbest bırakır.
pub fn free_dma_frames(phys: u64, count: usize) {
    if phys == 0 || count == 0 {
        return;
    }
    let mgr = unsafe { &mut *core::ptr::addr_of_mut!(crate::MEMORY_MGR) };
    mgr.pmm.free_contiguous_frames(phys, count);
}

/// `bytes` boyutunda DMA buffer için gereken sayfa sayısı.
#[inline]
pub fn frames_for(bytes: usize) -> usize {
    bytes.div_ceil(pmm::PAGE_SIZE as usize)
}

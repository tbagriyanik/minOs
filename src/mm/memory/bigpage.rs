//! 2 MiB büyük sayfa (huge page) desteği: eşleme, çözümleme, geri alma ve
//! sayfa tablosu çerçeve tasarrufu ölçümü.
//!
//! ## Neden büyük sayfa?
//!
//! 4 KiB sayfalarla 2 MiB'lık bir aralık eşlemek için bir PT tablosu
//! (1 çerçeve) + 512 PTE gerekir ve her erişimde TLB'de tek girdi tutulur.
//! 2 MiB büyük sayfada aynı aralık **tek PD girdisi** ile temsil edilir:
//!
//! * Sayfa tablosu bellek tüketimi 512 kat azalır (2 MiB başına 1 adet PD
//!   girdisi; PT çerçevesi hiç tahsis edilmez).
//! * TLB kapsama alanı 512 kat artar (iTLB/dTLB baskısı düşer).
//!
//! ## Sınırlar
//!
//! * Fiziksel blok `pmm::allocate_big_page()` ile 2 MiB hizalı ve bitişik
//!   olmalıdır; `vmm::AddressSpace::map_big_page` hizasız adresi reddeder.
//! * Büyük sayfa 4 KiB eşlemesinin üzerine yazmaz (çakışma reddedilir).
//! * COW ve `brk` daraltma yolları büyük sayfaları **bölmez**; yalnız
//!   `unmap_big_page`/`free_user_address_space_tables` tüm bloğu iade eder.

use crate::boot::console::{serial_print, serial_print_dec, serial_print_hex};
use crate::mm::pmm::{PhysicalMemoryManager, BIG_PAGE_SIZE, PAGE_SIZE};
use crate::mm::vmm::{AddressSpace, PageFlags};
use core::sync::atomic::{AtomicU64, Ordering};

/// Başarılı büyük sayfa eşleme sayısı (sistem geneli, teşhis).
pub static BIG_PAGE_MAPPINGS: AtomicU64 = AtomicU64::new(0);

/// Şimdiye kadar eşlenen toplam büyük sayfa sayısı.
pub fn mapping_count() -> u64 {
    BIG_PAGE_MAPPINGS.load(Ordering::Relaxed)
}

/// 2 MiB büyük sayfa eşlemesi için kullanılacak bayraklar.
/// Bayraklar 4 KiB karşılıklarıyla aynı anlamdadır; `HUGE_PAGE` biti
/// (`map_big_page` içinde) PD seviyesine otomatik eklenir.
pub const BIG_PAGE_KERNEL_RW: u64 = PageFlags::PRESENT | PageFlags::WRITABLE;
pub const BIG_PAGE_USER_DATA: u64 =
    PageFlags::PRESENT | PageFlags::USER | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

/// Bir sanal aralığı büyük sayfa ile eşler. `virt` ve `phys` 2 MiB hizalı,
/// `len` 2 MiB'ın katı olmalıdır.
///
/// Dönüş: eşlenen büyük sayfa sayısı. Hata durumunda o ana kadar eşlenenler
/// geri alınır (atomik).
pub fn map_big_range(
    space: &mut AddressSpace,
    virt: u64,
    phys: u64,
    len: u64,
    flags: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<usize, &'static str> {
    if len == 0 || !len.is_multiple_of(BIG_PAGE_SIZE) {
        return Err("Buyuk sayfa araligi 2MB'in kati olmali");
    }
    if !virt.is_multiple_of(BIG_PAGE_SIZE) || !phys.is_multiple_of(BIG_PAGE_SIZE) {
        return Err("Buyuk sayfa adresleri 2MB hizali olmali");
    }
    let count = (len / BIG_PAGE_SIZE) as usize;
    let mut mapped = 0usize;
    for index in 0..count {
        let v = virt + (index as u64) * BIG_PAGE_SIZE;
        let p = phys + (index as u64) * BIG_PAGE_SIZE;
        match space.map_big_page(v, p, flags, pmm) {
            Ok(()) => {
                mapped += 1;
                BIG_PAGE_MAPPINGS.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                for rollback in 0..mapped {
                    let v = virt + (rollback as u64) * BIG_PAGE_SIZE;
                    if let Some(base) = space.unmap_big_page(v, pmm) {
                        pmm.free_big_page(base);
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(mapped)
}

/// Büyük sayfa eşlemesini kaldırır ve bloğu PMM'e iade eder.
pub fn unmap_big_range(
    space: &mut AddressSpace,
    virt: u64,
    count: usize,
    pmm: &mut PhysicalMemoryManager,
) -> usize {
    let mut freed = 0usize;
    for index in 0..count {
        let v = virt + (index as u64) * BIG_PAGE_SIZE;
        if let Some(base) = space.unmap_big_page(v, pmm) {
            pmm.free_big_page(base);
            freed += 1;
        }
    }
    freed
}

/// `len` baytlık 4 KiB eşleme ile `len` baytlık büyük sayfa eşlemesinin
/// gerektirdiği sayfa tablosu çerçevesi sayısını karşılaştırır.
///
/// 4 KiB yolunda 2 MiB başına 1 PT çerçevesi gerekir; büyük sayfada 0.
pub fn page_table_savings(len: u64) -> (usize, usize) {
    let big_pages = (len / BIG_PAGE_SIZE) as usize;
    let four_k_tables = big_pages; // her 2 MiB için 1 adet PT
    let huge_tables = 0usize;
    (four_k_tables, huge_tables)
}

/// Büyük sayfanın bayrakları (huge biti dahil) — teşhis yardımcısı.
pub fn big_page_mapping_flags(space: &AddressSpace, virt: u64) -> Option<u64> {
    space.big_page_flags(virt)
}

/// Boot doğrulama testi (`[BIGPAGE]` işareti): hizalı tahsis + eşleme +
/// çözümleme + büyük sayfa bayrağı + iade (sızıntısız).
pub fn test_big_pages(pmm: &mut PhysicalMemoryManager) -> bool {
    serial_print("[BIGPAGE] --- 2MB buyuk sayfa dogrulama testi ---\n");
    let free_before = pmm.free_memory_bytes;
    let Some(phys) = pmm.allocate_big_page() else {
        serial_print("[BIGPAGE] Hizali buyuk sayfa tahsisi: BASARISIZ (bitisik bellek yok)\n");
        return false;
    };
    if !phys.is_multiple_of(BIG_PAGE_SIZE) {
        serial_print("[BIGPAGE] Tahsis hizasi yanlis: BASARISIZ\n");
        pmm.free_big_page(phys);
        return false;
    }
    let virt: u64 = 0x0000_7000_0000_0000;
    let mut space = AddressSpace::current();
    let mapped = space.map_big_page(virt, phys, BIG_PAGE_KERNEL_RW, pmm);
    if mapped.is_err() {
        serial_print("[BIGPAGE] Buyuk sayfa esleme: BASARISIZ\n");
        pmm.free_big_page(phys);
        return false;
    }
    BIG_PAGE_MAPPINGS.fetch_add(1, Ordering::Relaxed);

    // Coarse eşleme: aralığın içindeki 4 KiB adresler aynı bloğa çözülmeli.
    let mid_ok = space.translate(virt) == Some(phys)
        && space.translate(virt + 0x10_0000) == Some(phys + 0x10_0000)
        && space.translate(virt + BIG_PAGE_SIZE - PAGE_SIZE)
            == Some(phys + BIG_PAGE_SIZE - PAGE_SIZE);
    let flag_ok = space
        .big_page_flags(virt)
        .is_some_and(|flags| flags & PageFlags::HUGE_PAGE != 0);

    // TLB geçersizleştirme gerekmez: bu adres aktif eşlemede değildi.
    let unmapped = space.unmap_big_page(virt, pmm);
    if let Some(base) = unmapped {
        pmm.free_big_page(base);
    }
    let leak_free = pmm.free_memory_bytes == free_before;
    let ok = mid_ok && flag_ok && unmapped == Some(phys) && leak_free;

    serial_print("[BIGPAGE] 2MB hizali esleme (virt=");
    serial_print_hex(virt);
    serial_print(" phys=");
    serial_print_hex(phys);
    serial_print("), coz=512x4KB, PT tasarrufu=");
    let (four, huge) = page_table_savings(BIG_PAGE_SIZE);
    serial_print_dec((four - huge) as u64);
    serial_print(" cerceve");
    serial_print(if ok { ": BASARILI\n" } else { ": BASARISIZ\n" });
    ok
}

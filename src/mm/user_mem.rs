//! Kullanıcı Alanı Bellek Yöneticisi: Heap (`brk`) ve `mmap`.
//!
//! Ring 3 programları belleklerini iki yolla büyütür:
//!
//! 1. **Heap (`brk`/`sbrk`)** — `HEAP_BASE` adresinden yukarı doğru büyüyen,
//!    yalnızca uçtaki sayfaları eşleyen bir aralık. `brk(new_end)` çağrısı
//!    yeni uçtaki sayfaları sıfırlanmış fiziksel çerçevelerle eşler; geri
//!    küçültüldüğünde uçtan düşen sayfaların çerçeveleri PMM'e iade edilir
//!    (sıfırlanır, böylece eski veri yeni sahibine sızmaz).
//! 2. **mmap** — her çağrıda `mmap_base + mmap_used` adresine rastgele
//!    (ASLR ile seçilmiş) tabandan itibaren sayfa hizalı bir bölge ekler.
//!    `munmap` bölgeyi adres anahtarıyla bulur ve sayfalarını serbest bırakır.
//!
//! Her iki bölge de çerçeveleri süreç kaynak defterine (`record_user_frame`)
//! işler; süreç sonlandığında `reap_process_resources` bunları zaten iade eder.

use crate::drivers::phys_to_virt;
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::process::{Process, PAGE_COW};
use crate::mm::vmm::PageFlags;

/// Kullanıcı heap'inin taban adresi (kod 4 MiB'de bittiği için güvenli boşluk).
pub const HEAP_BASE: u64 = 0x0000_0000_0100_0000; // 16 MiB
/// Heap'in büyüyebileceği üst sınır (mmap bölgesiyle çakışmaz).
pub const HEAP_LIMIT: u64 = 0x0000_0000_0400_0000; // 64 MiB
/// Süreç başına takip edilebilen azami mmap bölgesi.
pub const MAX_MMAP_REGIONS: usize = 16;
/// Mmap arenasının üst sınırı; kullanıcı yığınından (≈128 TiB) uzak kalır.
pub const MMAP_LIMIT: u64 = 0x0000_0000_4000_0000; // 1 GiB

/// Mmap bölge tablosu (sabit boyutlu; `Process` `Copy` kalabilsin).
pub type MmapTable = [MmapRegion; MAX_MMAP_REGIONS];

/// Tek bir mmap bölgesi.
#[derive(Debug, Clone, Copy, Default)]
pub struct MmapRegion {
    pub addr: u64,
    pub len: u64,
    pub used: bool,
}

fn page_of(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}

fn page_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// `[start, end)` aralığındaki sayfaları sıfırlanmış çerçevelerle eşler.
/// Başarısızlıkta o ana kadar eşlenen sayfalar geri alınır.
pub fn map_zeroed_range(
    process: &mut Process,
    start: u64,
    end: u64,
    flags: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(), &'static str> {
    let start_page = page_of(start);
    let mut address = start_page;
    let limit = page_up(end);
    while address < limit {
        if process.address_space.translate(address).is_some() {
            // Kısmi eşleme bırakma: bu çağrıda eşlenen sayfaları geri al.
            unmap_range(process, start_page, address, pmm);
            return Err("Bellek araligi zaten eslenmis");
        }
        let Some(frame) = pmm.allocate_frame() else {
            unmap_range(process, start_page, address, pmm);
            return Err("PMM yetersiz bellek: kullanici sayfasi");
        };
        unsafe {
            core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE_SIZE as usize);
        }
        if !process.resources.record_user_frame(frame) {
            pmm.free_frame(frame);
            unmap_range(process, start_page, address, pmm);
            return Err("Surec kaynak listesi dolu");
        }
        if process
            .address_space
            .map_page(address, frame, flags, pmm)
            .is_err()
        {
            process.resources.take_user_frame(frame);
            pmm.free_frame(frame);
            unmap_range(process, start_page, address, pmm);
            return Err("Kullanici sayfasi eslenemedi");
        }
        address += PAGE_SIZE;
    }
    Ok(())
}

/// `[start, end)` aralığındaki sayfaları kaldırır, çerçeveleri iade eder.
/// COW paylaşımlı çerçeveler yalnızca defterden referans bırakır.
pub fn unmap_range(
    process: &mut Process,
    start: u64,
    end: u64,
    pmm: &mut PhysicalMemoryManager,
) -> usize {
    let mut address = page_of(start);
    let limit = page_up(end);
    let mut freed = 0usize;
    while address < limit {
        let Some(frame) = process.address_space.unmap_page(address) else {
            address += PAGE_SIZE;
            continue;
        };
        let _ = process.resources.take_user_frame(frame);
        match crate::mm::vmm::release_frame_shared(frame) {
            Some(false) => {}
            _ => pmm.free_frame(frame),
        }
        freed += 1;
        address += PAGE_SIZE;
    }
    freed
}

/// Heap ucunu `HEAP_BASE + brk` konumuna taşır. Dönen değer yeni uçtur
/// (`HEAP_BASE + brk`). `brk` sınırı aşarsa istek reddedilir ve eski uç korunur
/// (u64::MAX döner).
pub fn brk(process: &mut Process, new_brk: u64, pmm: &mut PhysicalMemoryManager) -> u64 {
    let old_end = HEAP_BASE + process.heap_brk;
    if new_brk > HEAP_LIMIT || new_brk > process.resources.limits.max_heap_bytes {
        return u64::MAX;
    }
    let new_end = HEAP_BASE + new_brk;
    if new_end > old_end {
        if map_zeroed_range(process, old_end, new_end, PageFlags::USER_DATA, pmm).is_err() {
            // Kısmi eşleme olmadığından başarısız istekte eski uç geçerli.
            return u64::MAX;
        }
    } else if new_end < old_end {
        unmap_range(process, new_end, old_end, pmm);
    }
    process.heap_brk = new_brk;
    HEAP_BASE + process.heap_brk
}

/// `len` baytlık yeni bir mmap bölgesi ayırır (sayfa hizalı, sıfırlanmış).
/// `flags` backend'lerin (anonim, dosya-tabanlı vb.) bayrağı olabilir; varsayılan
/// anonim özel eşleme `USER_DATA`'dır.
pub fn mmap(
    process: &mut Process,
    len: u64,
    flags: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<u64, &'static str> {
    let len = page_up(len.max(1));
    let Some(slot) = process.mmap_regions.iter().position(|region| !region.used) else {
        return Err("Mmap bolge tablosu dolu");
    };
    let addr = process.mmap_base + process.mmap_used;
    let end = addr.checked_add(len).ok_or("Mmap adres tasmasi")?;
    // Süreç başına mmap bayt kotası (varsayılan 64 MiB). Kota **canlı**
    // bölgelerin toplamına göre hesaplanır; `mmap_used` arena imleci olduğu
    // için munmap sonrası küçülmez.
    let live_bytes: u64 = process
        .mmap_regions
        .iter()
        .filter(|region| region.used)
        .map(|region| region.len)
        .sum();
    let projected = live_bytes.checked_add(len).ok_or("Mmap kota tasmasi")?;
    if projected > process.resources.limits.max_mmap_bytes {
        return Err("Mmap kaynak limiti asildi");
    }
    // Ayrılan bölge mmap arenasının üst sınırını aşamaz (kullanıcı yığınından
    // zaten çok uzaktadır).
    if end > MMAP_LIMIT {
        return Err("Mmap arenasi sinira ulasti");
    }
    let page_flags = if flags & PageFlags::USER != 0 {
        flags
    } else {
        PageFlags::USER_DATA
    };
    map_zeroed_range(process, addr, end, page_flags, pmm)?;
    process.mmap_regions[slot] = MmapRegion {
        addr,
        len,
        used: true,
    };
    process.mmap_used += len;
    Ok(addr)
}

/// `addr`/`len` ile belirtilen mmap bölgesini kaldırır. Tam eşleşme aranır.
pub fn munmap(process: &mut Process, addr: u64, len: u64, pmm: &mut PhysicalMemoryManager) -> bool {
    let Some(slot) = process
        .mmap_regions
        .iter()
        .position(|region| region.used && region.addr <= addr && addr < region.addr + region.len)
    else {
        return false;
    };
    let region = process.mmap_regions[slot];
    let start = region.addr;
    let end = region.addr + region.len;
    let _ = len;
    unmap_range(process, start, end, pmm);
    process.mmap_regions[slot] = MmapRegion::default();
    // Mmap tabanı ASLR için değişmez; serbest bölgeler yeniden kullanılmaz
    // (adres alanı yalıtımı ve deterministik davranış için).
    true
}

/// Heap ve mmap bölgelerindeki tüm kullanıcı sayfalarının **eşlemelerini**
/// kaldırır ve bölge defterini sıfırlar; fiziksel çerçeveleri serbest
/// bırakmaz. Süreç sonlandırılırken `reap_process_resources` bu sayfaları
/// kaynak defteri (`allocated_frames`) üzerinden tek seferde iade eder —
/// böylece çift iade / sayaç kayması olmaz.
pub fn release_user_memory(process: &mut Process, _pmm: &mut PhysicalMemoryManager) -> usize {
    let mut unmapped = unmap_range_keep_frames(process, HEAP_BASE, HEAP_BASE + process.heap_brk);
    for slot in 0..process.mmap_regions.len() {
        let region = process.mmap_regions[slot];
        if region.used {
            unmapped += unmap_range_keep_frames(process, region.addr, region.addr + region.len);
            process.mmap_regions[slot] = MmapRegion::default();
        }
    }
    process.heap_brk = 0;
    process.mmap_used = 0;
    unmapped
}

/// `[start, end)` eşlemelerini kaldırır ancak çerçeveleri PMM'e **iade etmez**;
/// yalnızca süreç adres uzayından çıkarır. Kaldırılan sayfa sayısını döner.
/// Sahiplik çağıranın kaynak defterindedir.
pub fn unmap_range_keep_frames(process: &mut Process, start: u64, end: u64) -> usize {
    let mut address = page_of(start);
    let limit = page_up(end);
    let mut unmapped = 0usize;
    while address < limit {
        if process.address_space.unmap_page(address).is_some() {
            unmapped += 1;
        }
        address += PAGE_SIZE;
    }
    unmapped
}

/// Bir sürecin kullanıcı sayfalarının okunabilir özeti (teşhis / test).
#[derive(Debug, Clone, Copy, Default)]
pub struct UserMemoryStats {
    pub heap_bytes: u64,
    pub heap_mapped_pages: usize,
    pub mmap_regions: usize,
    pub mmap_bytes: u64,
}

pub fn stats(process: &Process) -> UserMemoryStats {
    let heap_pages = ((HEAP_BASE + process.heap_brk).saturating_add(PAGE_SIZE - 1) / PAGE_SIZE)
        .saturating_sub(HEAP_BASE / PAGE_SIZE) as usize;
    let mut regions = 0usize;
    let mut bytes = 0u64;
    for region in process.mmap_regions.iter() {
        if region.used {
            regions += 1;
            bytes += region.len;
        }
    }
    UserMemoryStats {
        heap_bytes: process.heap_brk,
        heap_mapped_pages: heap_pages,
        mmap_regions: regions,
        mmap_bytes: bytes,
    }
}

/// Bir aralıkta COW işaretli sayfa var mı? (test yardımcısı)
pub fn any_cow_page(process: &Process, start: u64, end: u64) -> bool {
    let mut address = page_of(start);
    while address < page_up(end) {
        if let Some(entry) = process.address_space.read_entry(address) {
            if entry & PAGE_COW != 0 {
                return true;
            }
        }
        address += PAGE_SIZE;
    }
    false
}

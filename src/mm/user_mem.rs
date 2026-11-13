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
//!
//! ## Tembel (demand) ahırlama
//!
//! Hem `brk` hem de anonim `mmap` sayfaları **tembel** olarak eşlenir: PTE
//! `PRESENT | USER | NX | PAGE_LAZY` (yazılabilir değil) bayraklarıyla
//! paylaşılan **sıfır sayfası** çerçevesini gösterir. Okumalar sıfır döner;
//! ilk yazma koruma ihlali `#PF` üretir ve `process::handle_lazy_fault`
//! sıfırlanmış özel bir çerçeve ayırıp PTE'yi kalıcı bayraklarla değiştirir.
//! Böylece ayrılıp hiç yazılmayan `mmap` bölgeleri ve `brk` kuyruğu fiziksel
//! bellek harcamaz. Fork, tembel sayfaları aynen klonlar; her iki taraf da
//! ilk yazmada kendi çerçevesini alır.

use crate::drivers::phys_to_virt;
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::process::{Process, PAGE_COW, PAGE_LAZY};
use crate::mm::vmm::{self, PageFlags};
use core::sync::atomic::{AtomicU64, Ordering};

/// Yazma paylaşımlı (shared/map-shared) bölge bayrağı.
///
/// `mmap` çağrısında `flags` içinde bu bit varsa sayfa fiziksel olarak
/// süreçler arasında paylaşılır: tüm eşlemeler aynı çerçeveye işaret eder ve
/// `munmap` eden süreç yalnız kendi referansını bırakır. Sayfa paylaşımı
/// optimizasyonu böylece kopya olmadan (zero-copy) çalışır ve paylaşılan
/// veri değişiklikleri tüm süreçlerde g görünür.
pub const MAP_SHARED_FILE: u64 = 1 << 20;
/// Eşlemenin paylaşımlı (`MAP_SHARED_FILE`) olduğunu gösteren PTE yazılım biti.
pub const PAGE_SHARED: u64 = 1 << 11;

/// Kullanıcı heap'inin taban adresi (kod 4 MiB'de bittiği için güvenli boşluk).
pub const HEAP_BASE: u64 = 0x0000_0000_0100_0000; // 16 MiB
/// Heap'in büyüyebileceği üst sınır (mmap bölgesiyle çakışmaz).
pub const HEAP_LIMIT: u64 = 0x0000_0000_0400_0000; // 64 MiB
/// Süreç başına takip edilebilen azami mmap bölgesi.
pub const MAX_MMAP_REGIONS: usize = 16;
/// Mmap arenasının üst sınırı; kullanıcı yığınından (≈128 TiB) uzak kalır.
pub const MMAP_LIMIT: u64 = 0x0000_0000_4000_0000; // 1 GiB

/// Tembel sayfaların gösterdiği kalıcı sıfır çerçevesi (fiziksel adres).
///
/// Lazy PTE'ler bu çerçeveyi **salt-okunur** gösterir: okumalar sıfır döner,
/// ilk yazma koruma ihlali `#PF` üretir ve `handle_lazy_fault` özel çerçeve
/// ayırır. Çerçeve tüm süreçlerce paylaşılır; asla PMM'e iade edilmez ve
/// hiçbir süreç kaynak defterine işlenmez.
static ZERO_FRAME: AtomicU64 = AtomicU64::new(0);

/// Sıfır sayfası çerçevesini döndürür; ilk çağrıda PMM'den ayrılıp
/// sıfırlanır. Yarışta kaybeden taraf fazladan çerçeveyi iade eder.
fn zero_page_frame(pmm: &mut PhysicalMemoryManager) -> Option<u64> {
    let existing = ZERO_FRAME.load(Ordering::Acquire);
    if existing != 0 {
        return Some(existing);
    }
    let frame = pmm.allocate_frame()?;
    unsafe {
        core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE_SIZE as usize);
    }
    match ZERO_FRAME.compare_exchange(0, frame, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Some(frame),
        Err(winner) => {
            pmm.free_frame(frame);
            Some(winner)
        }
    }
}

/// Verilen fiziksel çerçeve paylaşılan sıfır sayfası mı?
pub(crate) fn is_zero_frame(frame: u64) -> bool {
    let zero = ZERO_FRAME.load(Ordering::Acquire);
    zero != 0 && frame == zero
}

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
        // Çerçeve türü: yalnızca kullanıcı veri/yığın sayfaları kullanıcı
        // kotasını (25 çerçeve) harcar; salt-okunur sayfalar yalnız toplam
        // çerçeve defterine işlenir (kota çift sayılmaz).
        let writable = flags & PageFlags::WRITABLE != 0 || flags & PageFlags::USER == 0;
        let recorded = if writable {
            process.resources.record_user_frame(frame)
        } else {
            process.resources.record_frame(frame)
        };
        if !recorded {
            pmm.free_frame(frame);
            unmap_range(process, start_page, address, pmm);
            return Err("Surec kaynak listesi dolu");
        }
        if process
            .address_space
            .map_page(address, frame, flags, pmm)
            .is_err()
        {
            if writable {
                process.resources.take_user_frame(frame);
            } else {
                process.resources.take_frame(frame);
            }
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
        if is_zero_frame(frame) {
            // Paylaşılan sıfır sayfası: süreçlerce ortak, asla iade edilmez.
            freed += 1;
            address += PAGE_SIZE;
            continue;
        }
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

/// `[start, end)` aralığını **tembel** olarak eşler: her sayfa için PTE
/// `PRESENT | USER | NX | PAGE_LAZY` (yazılabilir **değil**) olarak yazılır ve
/// paylaşılan sıfır sayfası çerçevesini gösterir. Okumalar sıfır döner; ilk
/// yazma koruma ihlali `#PF` üretir ve `process::handle_lazy_fault` özel bir
/// çerçeve ayırır (demand paging).
///
/// Adres aralığı daha önce eşlenmişse hata döner; kısmi eşleme bırakılmaz.
pub fn map_lazy_range(
    process: &mut Process,
    start: u64,
    end: u64,
    flags: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(), &'static str> {
    let start_page = page_of(start);
    let limit = page_up(end);
    let mut address = start_page;
    while address < limit {
        if process.address_space.read_entry(address).is_some() {
            unmap_range_keep_frames(process, start_page, address);
            return Err("Bellek araligi zaten eslenmis");
        }
        address += PAGE_SIZE;
    }
    // Veri sayfası semantiği: kullanıcı + NX; yazma koruma ihlaliyle #PF üretir.
    let _ = flags;
    let Some(zero) = zero_page_frame(pmm) else {
        return Err("PMM yetersiz bellek: sifir sayfasi");
    };
    let lazy_flags = (PageFlags::USER_DATA & !PageFlags::WRITABLE) | PAGE_LAZY;
    address = start_page;
    while address < limit {
        if process
            .address_space
            .map_page(address, zero, lazy_flags, pmm)
            .is_err()
        {
            unmap_range_keep_frames(process, start_page, address);
            return Err("Tembel sayfa eslenemedi");
        }
        address += PAGE_SIZE;
    }
    Ok(())
}

/// Tembel bir kullanıcı sayfasını gerçekleştirir: sıfırlanmış çerçeve
/// ayırır, süreç kaynak defterine işler, PTE'yi kalıcı bayraklarla değiştirir.
///
/// Dönüş: `true` → komut yeniden çalıştırılmalı; `false` → hata gerçek
/// (sayfa tembel değil veya bellek yok).
pub fn realize_lazy_page(
    process: &mut Process,
    fault_addr: u64,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    let page = page_of(fault_addr);
    let Some(entry) = process.address_space.read_entry(page) else {
        return false;
    };
    if entry & PAGE_LAZY == 0 || entry & PageFlags::USER == 0 {
        return false;
    }
    // Tembel PTE paylaşılan sıfır sayfasını gösterir ve yazılabilir değildir;
    // çerçeve alanı bu yüzden yok sayılır ve özel çerçeveyle değiştirilir.
    let Some(frame) = pmm.allocate_frame() else {
        return false;
    };
    unsafe {
        core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE_SIZE as usize);
    }
    if !process.resources.record_user_frame(frame) {
        pmm.free_frame(frame);
        return false;
    }
    let final_flags = (entry & !PAGE_LAZY & !0x000F_FFFF_FFFF_F000) | PageFlags::WRITABLE;
    if process
        .address_space
        .map_page(page, frame, final_flags, pmm)
        .is_err()
    {
        process.resources.take_user_frame(frame);
        pmm.free_frame(frame);
        return false;
    }
    true
}

/// Aralıkta tembel işaretli sayfa sayısı (teşhis / test).
pub fn lazy_page_count(process: &Process, start: u64, end: u64) -> usize {
    let mut address = page_of(start);
    let limit = page_up(end);
    let mut count = 0usize;
    while address < limit {
        if let Some(entry) = process.address_space.read_entry(address) {
            if entry & PAGE_LAZY != 0 {
                count += 1;
            }
        }
        address += PAGE_SIZE;
    }
    count
}

/// Bir çerçeveyi paylaşım defterine kaydeder. Defter doluysa `false`.
pub fn share_frame(frame: u64) -> bool {
    vmm::mark_frame_shared(frame)
}

// ---------------------------------------------------------------------------
// Paylaşımlı bölge kaydı (süreçler arası zero-copy sayfa paylaşımı)
// ---------------------------------------------------------------------------

/// Paylaşımlı bölge kayıtları (bu süreç tarafından oluşturulan çerçeve havuzları).
///
/// `mmap(MAP_SHARED_FILE)` çağrısı bölge için yeni çerçeveler kurar ve buraya
/// kaydeder; aynı sanal adresi eşleyen sonraki süreçler çerçeveleri kopya
/// olmadan yeniden kullanır. Kayıtlar referans sayımlıdır: son eşleme
/// bırakıldığında kayıt silinir.
pub const MAX_SHARED_REGIONS: usize = 8;
/// Paylaşımlı bölge başına azami sayfa sayısı.
pub const MAX_SHARED_REGION_PAGES: usize = 32;

#[derive(Clone, Copy)]
pub struct SharedRegion {
    pub addr: u64,
    pub len: u64,
    pub pages: usize,
    pub refs: u16,
    pub frames: [u64; MAX_SHARED_REGION_PAGES],
    pub used: bool,
}

impl SharedRegion {
    pub const fn empty() -> Self {
        Self {
            addr: 0,
            len: 0,
            pages: 0,
            refs: 0,
            frames: [0; MAX_SHARED_REGION_PAGES],
            used: false,
        }
    }
}

static SHARED_REGIONS: crate::arch::x86_64::sync::SpinMutex<[SharedRegion; MAX_SHARED_REGIONS]> =
    crate::arch::x86_64::sync::SpinMutex::new([SharedRegion::empty(); MAX_SHARED_REGIONS]);

/// `addr`/`len` ile eşleşen paylaşımlı bölgenin çerçeve listesini döner ve
/// referansını artırır.
pub fn shared_region_frames(addr: u64, len: u64) -> Option<alloc::vec::Vec<u64>> {
    let mut table = SHARED_REGIONS.lock();
    for region in table.iter_mut() {
        if region.used && region.addr == addr && region.len == len {
            region.refs = region.refs.saturating_add(1);
            let mut frames = alloc::vec::Vec::with_capacity(region.pages);
            frames.extend_from_slice(&region.frames[..region.pages]);
            return Some(frames);
        }
    }
    None
}

fn remember_shared_region(process: &Process, addr: u64, len: u64) {
    if process.resources.user_frame_count > MAX_SHARED_REGION_PAGES {
        // Kayıt taşması: paylaşım yine geçerlidir (çerçeve defteri), yalnız
        // başka süreç zero-copy yoluyla yeniden kullanamaz.
        return;
    }
    const EMPTY: SharedRegion = SharedRegion::empty();
    let mut frames = EMPTY.frames;
    let mut pages = 0usize;
    let mut address = page_of(addr);
    while address < page_up(addr + len) && pages < MAX_SHARED_REGION_PAGES {
        if let Some(frame) = process.address_space.translate_user(address) {
            frames[pages] = frame & !(PAGE_SIZE - 1);
            pages += 1;
        }
        address += PAGE_SIZE;
    }
    let mut table = SHARED_REGIONS.lock();
    if let Some(region) = table
        .iter_mut()
        .find(|region| !region.used || (region.addr == addr && region.len == len))
    {
        *region = SharedRegion {
            addr,
            len,
            pages,
            refs: 1,
            frames,
            used: pages > 0,
        };
    }
}

/// Eşleşen bir paylaşımlı bölgenin mevcut çerçevelerini bu sürece eşler.
fn map_shared_reuse(
    process: &mut Process,
    start: u64,
    end: u64,
    frames: &[u64],
    pmm: &mut PhysicalMemoryManager,
) -> Result<(), &'static str> {
    let needed = (page_up(end - start) / PAGE_SIZE) as usize;
    if frames.len() < needed {
        return Err("Paylasim kaydi eksik cerceve");
    }
    let simple = start;
    let flags = PageFlags::USER_DATA | PAGE_SHARED;
    let mut address = simple;
    for frame in frames.iter().take(needed) {
        if process.address_space.read_entry(address).is_some() {
            unmap_range_keep_frames(process, simple, address);
            return Err("Bellek araligi zaten eslenmis");
        }
        // Paylaşım defteri artırılır; sahiplik referansı sürece aittir.
        if !vmm::mark_frame_shared(*frame) {
            unmap_range_keep_frames(process, simple, address);
            return Err("Paylasim defteri dolu");
        }
        if !process.resources.record_user_frame(*frame) {
            let _ = vmm::release_frame_shared(*frame);
            unmap_range_keep_frames(process, simple, address);
            return Err("Surec kaynak listesi dolu");
        }
        if process
            .address_space
            .map_page(address, *frame, flags, pmm)
            .is_err()
        {
            process.resources.take_user_frame(*frame);
            let _ = vmm::release_frame_shared(*frame);
            unmap_range_keep_frames(process, simple, address);
            return Err("Paylasimli sayfa eslenemedi");
        }
        address += PAGE_SIZE;
    }
    Ok(())
}

/// Paylaşımlı bölge kaydını bırakır (son referansta silinir).
pub fn release_shared_region(addr: u64, len: u64) {
    let mut table = SHARED_REGIONS.lock();
    for region in table.iter_mut() {
        if region.used && region.addr == addr && region.len == len {
            region.refs = region.refs.saturating_sub(1);
            if region.refs == 0 {
                *region = SharedRegion::empty();
            }
            return;
        }
    }
    // Bölge kaydedilmemişse (kayıt tablosu dolu) yapacak iş yok.
}

/// Test yardımı: paylaşımlı bölge kaydı var mı?
pub fn shared_region_registered(addr: u64, len: u64) -> bool {
    SHARED_REGIONS
        .lock()
        .iter()
        .any(|region| region.used && region.addr == addr && region.len == len)
}

/// Tüm paylaşım bölge kayıtlarını siler (süreç yeniden başlatma testleri).
pub fn release_shared_regions() {
    let mut table = SHARED_REGIONS.lock();
    for region in table.iter_mut() {
        *region = SharedRegion::empty();
    }
}

/// Çerçevenin hâlâ başka bir eşleme tarafından paylaşıldığını bildirir.
pub fn frame_still_shared(frame: u64) -> bool {
    vmm::is_frame_shared(frame)
}

/// Paylaşımlı (`MAP_SHARED_FILE`) aralık eşlemesi. `[start, end)` için tek
/// bir aralık çerçeve havuzu oluşturur: her sayfa ayrı sanal tabanda
/// görünür ama süreçler arasında aynı fiziksel çerçeveyi paylaşır.
fn map_shared_range(
    process: &mut Process,
    start: u64,
    end: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(), &'static str> {
    let start_page = page_of(start);
    let limit = page_up(end);
    let mut address = start_page;
    while address < limit {
        if process.address_space.read_entry(address).is_some() {
            unmap_range_keep_frames(process, start_page, address);
            return Err("Bellek araligi zaten eslenmis");
        }
        address += PAGE_SIZE;
    }
    let flags = PageFlags::USER_DATA | PAGE_SHARED;
    address = start_page;
    while address < limit {
        let Some(frame) = pmm.allocate_frame() else {
            unmap_range(process, start_page, address, pmm);
            return Err("PMM yetersiz bellek: paylasimli sayfa");
        };
        unsafe {
            core::ptr::write_bytes(phys_to_virt(frame) as *mut u8, 0, PAGE_SIZE as usize);
        }
        if !vmm::mark_frame_shared(frame) || !process.resources.record_user_frame(frame) {
            pmm.free_frame(frame);
            unmap_range(process, start_page, address, pmm);
            return Err("Paylasim defteri veya kaynak listesi dolu");
        }
        if process
            .address_space
            .map_page(address, frame, flags, pmm)
            .is_err()
        {
            process.resources.take_user_frame(frame);
            let _ = vmm::release_frame_shared(frame);
            pmm.free_frame(frame);
            unmap_range(process, start_page, address, pmm);
            return Err("Paylasimli sayfa eslenemedi");
        }
        address += PAGE_SIZE;
    }
    Ok(())
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
        // Heap büyümesi tembel: sayfalar ilk yazmada ayrılır.
        if map_lazy_range(process, old_end, new_end, PageFlags::USER_DATA, pmm).is_err() {
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
    if flags & MAP_SHARED_FILE != 0 {
        // Yazma paylaşımlı bölge: daha önce aynı bölgeyi eşlemiş başka bir
        // süreç varsa çerçeveler **kopyalanmadan** paylaşılır (zero-copy);
        // yoksa yeni çerçeve havuzu kurulur ve kayıt edilir. `munmap` yalnız
        // referans bırakır, çerçeve son sahibine kadar yaşar.
        if let Some(frames) = shared_region_frames(addr, len) {
            map_shared_reuse(process, addr, end, &frames, pmm)?;
        } else {
            map_shared_range(process, addr, end, pmm)?;
            remember_shared_region(process, addr, len);
        }
    } else {
        // Anonim bölge tembel eşlenir: yazılmayan sayfalar fiziksel bellek harcamaz.
        map_lazy_range(process, addr, end, page_flags, pmm)?;
    }
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
    if process
        .address_space
        .read_entry(start)
        .is_some_and(|entry| entry & PAGE_SHARED != 0)
    {
        release_shared_region(start, region.len);
    }
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
    // Heap sayfa sayısı = heap aralığının sayfa karşılığı (brk 0 ise 0).
    let heap_pages = (process.heap_brk.div_ceil(PAGE_SIZE)) as usize;
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

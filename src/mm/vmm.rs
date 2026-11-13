//! x86_64 4-Seviyeli (4-Level Paging) Sanal Bellek Yöneticisi (VMM).
//!
//! Bu modül:
//! 1. PML4, PDPT, PD, PT sayfa tablolarının yönetimini,
//! 2. Sayfa izin bayraklarını (PRESENT, WRITABLE, USER, NO_EXECUTE),
//! 3. HHDM (Higher-Half Direct Map) üzerinden fiziksel adres <-> sanal adres erişimini,
//! 4. 4KB sanal sayfaları fiziksel sayfa çerçevelerine (PMM üzerinden) eşlemeyi (map/unmap),
//! 5. Çekirdek ve uygulama adres uzaylarının ayrımını sağlar.
//! 6. Copy-on-write (COW) paylaşımlı çerçeve defterini tutar.

use crate::arch::x86_64::sync::SpinMutex;
use crate::drivers::phys_to_virt;
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};

/// COW paylaşım defterinin takip edebileceği azami çerçeve sayısı.
pub const MAX_SHARED_FRAMES: usize = 512;

/// Aynı fiziksel çerçeveyi birden fazla adres uzayının paylaştığı
/// (copy-on-write) çerçevelerin referans sayacı.
#[derive(Clone, Copy)]
struct SharedFrame {
    phys: u64,
    refs: u32,
}

struct SharedFrameTable {
    entries: [SharedFrame; MAX_SHARED_FRAMES],
    used: usize,
}

impl SharedFrameTable {
    const fn new() -> Self {
        const EMPTY: SharedFrame = SharedFrame { phys: 0, refs: 0 };
        Self {
            entries: [EMPTY; MAX_SHARED_FRAMES],
            used: 0,
        }
    }

    fn find(&self, phys: u64) -> Option<usize> {
        self.entries[..self.used]
            .iter()
            .position(|e| e.phys == phys)
    }
}

static SHARED_FRAMES: SpinMutex<SharedFrameTable> = SpinMutex::new(SharedFrameTable::new());

/// Bir çerçeveyi COW paylaşımı olarak işaretler (ilk paylaşımda refs = 2).
/// Defter doluysa `false` döner; bu durumda çağıran çerçeveyi özel kopya
/// olarak bırakmalıdır (COW kurulamadı).
pub fn mark_frame_shared(phys: u64) -> bool {
    let mut table = SHARED_FRAMES.lock();
    if let Some(index) = table.find(phys) {
        let refs = table.entries[index].refs + 1;
        table.entries[index].refs = refs;
        return true;
    }
    if table.used >= MAX_SHARED_FRAMES {
        return false;
    }
    let used = table.used;
    table.entries[used] = SharedFrame { phys, refs: 2 };
    table.used = used + 1;
    true
}

/// Bir paylaşım referansını bırakır (`refs` = bu çerçeveyi eşleyen adres
/// uzayı sayısı).
///
/// * `Some(false)` — bıraktıktan sonra çerçeveyi hâlâ başka bir adres uzayı
///   eşliyor; fiziksel çerçeve **serbest bırakılmamalıdır**.
/// * `Some(true)` — bu son referanstı; çerçeve artık kimseye ait değil ve
///   çağıran onu PMM'e iade edebilir.
/// * `None` — çerçeve defterde yok (zaten özel).
///
/// DİKKAT: iki sahipli paylaşımda `refs = 2`'dir. Bir sahip ayrıldığında
/// `refs = 1` kalır ve bu hâlâ *paylaşılan* (özel olmayan) bir çerçevedir;
/// dolayısıyla `Some(false)` döner. Yalnızca `refs == 0` gerçek serbest
/// bırakmayı gösterir.
pub fn release_frame_shared(phys: u64) -> Option<bool> {
    let mut table = SHARED_FRAMES.lock();
    let index = table.find(phys)?;
    let refs = table.entries[index].refs.saturating_sub(1);
    table.entries[index].refs = refs;
    if refs == 0 {
        let used = table.used - 1;
        table.entries[index] = table.entries[used];
        table.entries[used] = SharedFrame { phys: 0, refs: 0 };
        table.used = used;
        Some(true)
    } else {
        Some(false)
    }
}

/// Çerçeveyi eşleyen adres uzayı sayısı (defterde yoksa `0`).
/// `mark_frame_shared` ilk paylaşımda 2 döner (ebeveyn + çocuk).
pub fn shared_refs(phys: u64) -> u32 {
    let table = SHARED_FRAMES.lock();
    table
        .find(phys)
        .map(|index| table.entries[index].refs)
        .unwrap_or(0)
}

/// Çerçevenin hâlâ COW paylaşımında olup olmadığını bildirir.
pub fn is_frame_shared(phys: u64) -> bool {
    let table = SHARED_FRAMES.lock();
    table.find(phys).is_some()
}

/// Paylaşılan çerçeve sayısı (özyin testleri ve teşhis için).
pub fn shared_frame_count() -> usize {
    SHARED_FRAMES.lock().used
}

/// Sayfa tablosu giriş bayrakları (Page Table Entry Flags)
pub struct PageFlags;

impl PageFlags {
    /// Sayfa fiziksel bellekte mevcut (Present)
    pub const PRESENT: u64 = 1 << 0;
    /// Yazılabilir (Read/Write)
    pub const WRITABLE: u64 = 1 << 1;
    /// Kullanıcı kipi erişebilir (Ring 3 / User-mode)
    pub const USER: u64 = 1 << 2;
    /// Write-through önbellekleme
    pub const WRITE_THROUGH: u64 = 1 << 3;
    /// Önbellek devre dışı (Cache Disable)
    pub const NO_CACHE: u64 = 1 << 4;
    /// Erişildi biti (Accessed)
    pub const ACCESSED: u64 = 1 << 5;
    /// Kirli biti (Dirty - yalnızca L1/PT için)
    pub const DIRTY: u64 = 1 << 6;
    /// 2MB / 1GB Huge Page biti (L2/L3 için)
    pub const HUGE_PAGE: u64 = 1 << 7;
    /// Global sayfa (CR4.PGE ile TLB'de kalır)
    pub const GLOBAL: u64 = 1 << 8;
    /// Yürütme Yasak (No-Execute / NX biti - EFER.NXE ile)
    pub const NO_EXECUTE: u64 = 1 << 63;

    /// Çekirdek salt okunur kod/veri bayrağı
    pub const KERNEL_RO: u64 = Self::PRESENT;
    /// Çekirdek yazılabilir veri bayrağı
    pub const KERNEL_RW: u64 = Self::PRESENT | Self::WRITABLE;
    /// Kullanıcı kodu: Present + User + Executable
    pub const USER_CODE: u64 = Self::PRESENT | Self::USER;
    /// Kullanıcı verisi: Present + User + Writable + No-Execute
    pub const USER_DATA: u64 = Self::PRESENT | Self::USER | Self::WRITABLE | Self::NO_EXECUTE;
    /// Kullanıcı yığını: Present + User + Writable + No-Execute
    pub const USER_STACK: u64 = Self::PRESENT | Self::USER | Self::WRITABLE | Self::NO_EXECUTE;
}

/// 512 girişli x86_64 Sayfa Tablosu yapısı (4096 bayt hizalı)
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub const fn empty() -> Self {
        Self { entries: [0; 512] }
    }

    pub fn zero(&mut self) {
        self.entries.fill(0);
    }
}

/// Sanal Bellek Adres Uzayı
#[derive(Debug, Clone, Copy)]
pub struct AddressSpace {
    /// PML4 kök tablosunun fiziksel adresi (CR3'e yüklenecek değer)
    pub pml4_phys: u64,
}

impl AddressSpace {
    const USER_ADDRESS_END: u64 = 0x0000_8000_0000_0000;

    /// Mevcut etkin olan çekirdek adres uzayını (aktif CR3) okuyarak nesne oluşturur.
    pub fn current() -> Self {
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        }
        Self {
            pml4_phys: cr3 & 0x000F_FFFF_FFFF_F000,
        }
    }

    /// Fiziksel bir sayfa tablosu adresini HHDM üzerinden erişilebilir işaretçiye çevirir.
    #[inline(always)]
    pub fn table_ptr(phys: u64) -> *mut PageTable {
        phys_to_virt(phys) as *mut PageTable
    }

    /// Aktif adres uzayında bir sonraki seviye tablosunu bulur veya PMM'den tahsis eder.
    fn get_or_create_next_level(
        entry: &mut u64,
        pmm: &mut PhysicalMemoryManager,
        user: bool,
    ) -> Option<u64> {
        if (*entry & PageFlags::PRESENT) != 0 {
            // Bir sonraki tablo zaten mevcut; gerekirse USER bayrağını da ekle
            if user && (*entry & PageFlags::USER) == 0 {
                *entry |= PageFlags::USER;
            }
            Some(*entry & 0x000F_FFFF_FFFF_F000)
        } else {
            // PMM'den yeni bir 4KB fiziksel çerçeve al
            let next_phys = pmm.allocate_frame()?;
            // Sıfırla (temiz sayfa tablosu)
            unsafe {
                let ptr = Self::table_ptr(next_phys);
                core::ptr::write_bytes(ptr as *mut u8, 0, PAGE_SIZE as usize);
            }
            let mut flags = PageFlags::PRESENT | PageFlags::WRITABLE;
            if user {
                flags |= PageFlags::USER;
            }
            *entry = next_phys | flags;
            Some(next_phys)
        }
    }

    /// 4KB'lık bir sanal sayfayı (`virt_addr`) belirli bir fiziksel çerçeveye (`phys_addr`)
    /// verilen bayraklar (`flags`) ile eşler (map).
    pub fn map_page(
        &mut self,
        virt_addr: u64,
        phys_addr: u64,
        flags: u64,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        if !virt_addr.is_multiple_of(PAGE_SIZE) || !phys_addr.is_multiple_of(PAGE_SIZE) {
            return Err("Adresler 4KB sayfa sinirina hizali degil");
        }
        if flags & PageFlags::USER != 0 && virt_addr >= Self::USER_ADDRESS_END {
            return Err("Kullanici sayfasi kullanici adres alaninin disinda");
        }

        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

        let user = (flags & PageFlags::USER) != 0;

        unsafe {
            // Seviye 4: PML4
            let pml4 = &mut *Self::table_ptr(self.pml4_phys);
            let pdpt_phys = Self::get_or_create_next_level(&mut pml4.entries[pml4_idx], pmm, user)
                .ok_or("PMM yetersiz bellek: PDPT tahsis edilemedi")?;

            // Seviye 3: PDPT
            let pdpt = &mut *Self::table_ptr(pdpt_phys);
            let pd_phys = Self::get_or_create_next_level(&mut pdpt.entries[pdpt_idx], pmm, user)
                .ok_or("PMM yetersiz bellek: PD tahsis edilemedi")?;

            // Seviye 2: PD
            let pd = &mut *Self::table_ptr(pd_phys);
            let pt_phys = Self::get_or_create_next_level(&mut pd.entries[pd_idx], pmm, user)
                .ok_or("PMM yetersiz bellek: PT tahsis edilemedi")?;

            // Seviye 1: PT
            let pt = &mut *Self::table_ptr(pt_phys);
            pt.entries[pt_idx] = (phys_addr & 0x000F_FFFF_FFFF_F000) | flags;

            // TLB Girişini Geçersiz Kıl (Invalpg)
            core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }

        Ok(())
    }

    /// Var olan bir 4KB eşlemesinin bayraklarını değiştirir; tablo tahsis etmez.
    /// GERÇEK DONANIMDA DİKKAT: çekirdek (HHDM) eşlemesi de 4KB'tan küçük
    /// olduğu için yalnız sembolik olarak vardır. `target_pml4` altındaki
    /// eşleme değişir ve TLB geçersizleştirilir; `target_pml4` aktif CR3 ise
    /// çağıran, değişen sayfaya çekirdek üzerinden erişim yapmamalıdır.
    pub fn set_flags(
        &mut self,
        virt_addr: u64,
        flags: u64,
        _pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        if !virt_addr.is_multiple_of(PAGE_SIZE) {
            return Err("Adres 4KB sayfa sinirina hizali degil");
        }
        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *Self::table_ptr(self.pml4_phys);
            let pml4_entry = pml4.entries[pml4_idx];
            if (pml4_entry & PageFlags::PRESENT) == 0 {
                return Err("set_flags: eşlenmemiş adres");
            }
            let pdpt = &mut *Self::table_ptr(pml4_entry & 0x000F_FFFF_FFFF_F000);
            let pdpt_entry = pdpt.entries[pdpt_idx];
            if (pdpt_entry & PageFlags::PRESENT) == 0 {
                return Err("set_flags: eşlenmemiş adres");
            }
            let pd = &mut *Self::table_ptr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
            let pd_entry = pd.entries[pd_idx];
            if (pd_entry & PageFlags::PRESENT) == 0 {
                return Err("set_flags: eşlenmemiş adres");
            }
            if (pd_entry & PageFlags::HUGE_PAGE) != 0 {
                return Err("set_flags: 2MB huge page eşlemesi bayrak değişimini desteklemiyor");
            }
            let pt = &mut *Self::table_ptr(pd_entry & 0x000F_FFFF_FFFF_F000);
            let entry = pt.entries[pt_idx];
            if (entry & PageFlags::PRESENT) == 0 {
                return Err("set_flags: eşlenmemiş adres");
            }
            pt.entries[pt_idx] = (entry & 0x000F_FFFF_FFFF_F000) | flags;
            core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }
        Ok(())
    }

    /// Var olan bir 4KB eşlemesinin fiziksel çerçevesini değiştirir.
    pub fn remap_page(
        &mut self,
        virt_addr: u64,
        phys_addr: u64,
        _pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        if !virt_addr.is_multiple_of(PAGE_SIZE) || !phys_addr.is_multiple_of(PAGE_SIZE) {
            return Err("Adresler 4KB sayfa sinirina hizali degil");
        }
        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
        unsafe {
            let pml4 = &mut *Self::table_ptr(self.pml4_phys);
            let pml4_entry = pml4.entries[pml4_idx];
            if (pml4_entry & PageFlags::PRESENT) == 0 {
                return Err("remap_page: eşlenmemiş adres");
            }
            let pdpt = &mut *Self::table_ptr(pml4_entry & 0x000F_FFFF_FFFF_F000);
            let pdpt_entry = pdpt.entries[pdpt_idx];
            if (pdpt_entry & PageFlags::PRESENT) == 0 {
                return Err("remap_page: eşlenmemiş adres");
            }
            let pd = &mut *Self::table_ptr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
            let pd_entry = pd.entries[pd_idx];
            if (pd_entry & PageFlags::PRESENT) == 0 || (pd_entry & PageFlags::HUGE_PAGE) != 0 {
                return Err("remap_page: geçersiz PD girdisi");
            }
            let pt = &mut *Self::table_ptr(pd_entry & 0x000F_FFFF_FFFF_F000);
            let entry = pt.entries[pt_idx];
            if (entry & PageFlags::PRESENT) == 0 {
                return Err("remap_page: eşlenmemiş adres");
            }
            pt.entries[pt_idx] =
                (phys_addr & 0x000F_FFFF_FFFF_F000) | (entry & !0x000F_FFFF_FFFF_F000);
            core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }
        Ok(())
    }

    /// Sanal adresin son seviye (PTE) girdisini olduğu gibi döner.
    pub fn read_entry(&self, virt_addr: u64) -> Option<u64> {
        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
        unsafe {
            let pml4 = &*Self::table_ptr(self.pml4_phys);
            let pml4_entry = pml4.entries[pml4_idx];
            if (pml4_entry & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pdpt = &*Self::table_ptr(pml4_entry & 0x000F_FFFF_FFFF_F000);
            let pdpt_entry = pdpt.entries[pdpt_idx];
            if (pdpt_entry & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pd = &*Self::table_ptr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
            let pd_entry = pd.entries[pd_idx];
            if (pd_entry & PageFlags::PRESENT) == 0 {
                return None;
            }
            if (pd_entry & PageFlags::HUGE_PAGE) != 0 {
                return Some(pd_entry);
            }
            let pt = &*Self::table_ptr(pd_entry & 0x000F_FFFF_FFFF_F000);
            let entry = pt.entries[pt_idx];
            if (entry & PageFlags::PRESENT) == 0 {
                return None;
            }
            Some(entry)
        }
    }

    /// Eşlenmiş bir sanal adresin son seviye bayraklarını döner.
    pub fn flags(&self, virt_addr: u64) -> Option<u64> {
        self.read_entry(virt_addr)
            .map(|entry| entry & 0xFFF_8000_0000_0FFFu64)
    }

    /// Bir sanal adresin eşlemesini (mapping) kaldırır (unmap) ve fiziksel adresi döner.
    pub fn unmap_page(&mut self, virt_addr: u64) -> Option<u64> {
        if !virt_addr.is_multiple_of(PAGE_SIZE) {
            return None;
        }

        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *Self::table_ptr(self.pml4_phys);
            if (pml4.entries[pml4_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pdpt_phys = pml4.entries[pml4_idx] & 0x000F_FFFF_FFFF_F000;

            let pdpt = &mut *Self::table_ptr(pdpt_phys);
            if (pdpt.entries[pdpt_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pd_phys = pdpt.entries[pdpt_idx] & 0x000F_FFFF_FFFF_F000;

            let pd = &mut *Self::table_ptr(pd_phys);
            if (pd.entries[pd_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pt_phys = pd.entries[pd_idx] & 0x000F_FFFF_FFFF_F000;

            let pt = &mut *Self::table_ptr(pt_phys);
            let entry = pt.entries[pt_idx];
            if (entry & PageFlags::PRESENT) == 0 {
                return None;
            }

            pt.entries[pt_idx] = 0;

            // TLB flush
            core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));

            Some(entry & 0x000F_FFFF_FFFF_F000)
        }
    }

    /// `unmap_page` + boşalan alt seviye tabloların geri kazanımı.
    ///
    /// `unmap_page` yalnızca yaprak girdiyi temizler; PT/PD/PDPT çerçeveleri
    /// tahsis edilmiş olarak kalır. Bu fonksiyon yaprak girdi silindikten
    /// sonra boşalan her alt tabloyu PMM'e iade eder ve üst girdiyi de
    /// temizler. Böylece uzun ömürlü süreçlerde tekrarlı `munmap`/`brk`
    /// daraltma işlemleri sayfa tablosu çerçevelerini sızdırmaz.
    ///
    /// Dönüş: kaldırılan fiziksel çerçeve (yaprak), eşleme yoksa `None`.
    ///
    /// # Safety
    /// Çağıran, tabloların başka bir adres uzayıyla paylaşılmadığından emin
    /// olmalıdır. Çekirdek yarı (PML4 indeks 256-511) için çağrılmamalıdır;
    /// üst yarı girdileri asla serbest bırakılmaz.
    pub fn unmap_page_reclaim(
        &mut self,
        virt_addr: u64,
        pmm: &mut PhysicalMemoryManager,
    ) -> Option<u64> {
        let leaf = self.unmap_page(virt_addr)?;
        if (virt_addr >> 39) & 0x1FF >= 256 {
            // Çekirdek yarı: paylaşılan üst tablolar dokunulmaz.
            return Some(leaf);
        }

        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;

        // Bosluk denetimi: ayni tabloya ikinci bir referans olusturmamak icin
        // tablolar ham isaretci uzerinden okunur.
        unsafe {
            let pml4 = Self::table_ptr(self.pml4_phys);
            if (*pml4).entries[pml4_idx] & PageFlags::PRESENT == 0 {
                return Some(leaf);
            }
            let pdpt_phys = (*pml4).entries[pml4_idx] & 0x000F_FFFF_FFFF_F000;

            let pdpt = Self::table_ptr(pdpt_phys);
            if (*pdpt).entries[pdpt_idx] & PageFlags::PRESENT == 0 {
                return Some(leaf);
            }
            if (*pdpt).entries[pdpt_idx] & PageFlags::HUGE_PAGE != 0 {
                // 1 GB buyuk sayfa: alt tablosu yok.
                return Some(leaf);
            }
            let pd_phys = (*pdpt).entries[pdpt_idx] & 0x000F_FFFF_FFFF_F000;

            let pd = Self::table_ptr(pd_phys);
            if (*pd).entries[pd_idx] & PageFlags::PRESENT == 0 {
                return Some(leaf);
            }
            if (*pd).entries[pd_idx] & PageFlags::HUGE_PAGE != 0 {
                // 2 MB buyuk sayfa: PT'si yok, alt tablo geri kazanimi yok.
                return Some(leaf);
            }
            let pt_phys = (*pd).entries[pd_idx] & 0x000F_FFFF_FFFF_F000;

            // PT tamamen bosaldi mi?
            if !Self::table_is_empty(pt_phys) {
                return Some(leaf);
            }
            (*pd).entries[pd_idx] = 0;
            pmm.free_frame(pt_phys);

            // PD tamamen bosaldi mi?
            if !Self::table_is_empty(pd_phys) {
                return Some(leaf);
            }
            (*pdpt).entries[pdpt_idx] = 0;
            pmm.free_frame(pd_phys);

            // PDPT tamamen bosaldi mi?
            if !Self::table_is_empty(pdpt_phys) {
                return Some(leaf);
            }
            (*pml4).entries[pml4_idx] = 0;
            pmm.free_frame(pdpt_phys);

            Some(leaf)
        }
    }

    /// Bir sayfa tablosundaki tum girdilerin sifir oldugunu dondurur.
    ///
    /// # Safety
    /// `table_phys` gecerli ve hizali bir sayfa tablosu cercevesine isaret
    /// etmeli; ayni tabloya canli bir `&mut` referansi bulunmamalidir.
    unsafe fn table_is_empty(table_phys: u64) -> bool {
        let table = Self::table_ptr(table_phys);
        (*table).entries.iter().all(|entry| *entry == 0)
    }

    /// Sanal bir adresi sayfa tablolarında arayarak eşlendiği fiziksel adresi çözümler (translate).
    pub fn translate(&self, virt_addr: u64) -> Option<u64> {
        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
        let page_offset = virt_addr & (PAGE_SIZE - 1);

        unsafe {
            let pml4 = &*Self::table_ptr(self.pml4_phys);
            if (pml4.entries[pml4_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }

            let pdpt_phys = pml4.entries[pml4_idx] & 0x000F_FFFF_FFFF_F000;

            let pdpt = &*Self::table_ptr(pdpt_phys);
            if (pdpt.entries[pdpt_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }
            let pd_phys = pdpt.entries[pdpt_idx] & 0x000F_FFFF_FFFF_F000;

            let pd = &*Self::table_ptr(pd_phys);
            if (pd.entries[pd_idx] & PageFlags::PRESENT) == 0 {
                return None;
            }

            // 2MB Huge page kontrolü
            if (pd.entries[pd_idx] & PageFlags::HUGE_PAGE) != 0 {
                let phys_2mb = pd.entries[pd_idx] & 0x000F_FFFF_FFE0_0000;
                let offset_2mb = virt_addr & 0x1F_FFFF;
                return Some(phys_2mb + offset_2mb);
            }

            let pt_phys = pd.entries[pd_idx] & 0x000F_FFFF_FFFF_F000;
            let pt = &*Self::table_ptr(pt_phys);
            let entry = pt.entries[pt_idx];
            if (entry & PageFlags::PRESENT) == 0 {
                return None;
            }

            Some((entry & 0x000F_FFFF_FFFF_F000) + page_offset)
        }
    }

    /// Translate an address only if every paging level grants user access.
    pub fn translate_user(&self, virt_addr: u64) -> Option<u64> {
        if virt_addr >= Self::USER_ADDRESS_END {
            return None;
        }
        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
        let page_offset = virt_addr & (PAGE_SIZE - 1);
        let user_present = PageFlags::PRESENT | PageFlags::USER;

        unsafe {
            let pml4 = &*Self::table_ptr(self.pml4_phys);
            let pml4_entry = pml4.entries[pml4_idx];
            if pml4_entry & user_present != user_present {
                return None;
            }

            let pdpt = &*Self::table_ptr(pml4_entry & 0x000F_FFFF_FFFF_F000);
            let pdpt_entry = pdpt.entries[pdpt_idx];
            if pdpt_entry & user_present != user_present {
                return None;
            }

            let pd = &*Self::table_ptr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
            let pd_entry = pd.entries[pd_idx];
            if pd_entry & user_present != user_present {
                return None;
            }
            if (pd_entry & PageFlags::HUGE_PAGE) != 0 {
                let phys_2mb = pd_entry & 0x000F_FFFF_FFE0_0000;
                return Some(phys_2mb + (virt_addr & 0x1F_FFFF));
            }

            let pt = &*Self::table_ptr(pd_entry & 0x000F_FFFF_FFFF_F000);
            let pt_entry = pt.entries[pt_idx];
            if pt_entry & user_present != user_present {
                return None;
            }

            Some((pt_entry & 0x000F_FFFF_FFFF_F000) + page_offset)
        }
    }

    /// Kullanıcı adres uzayının altındaki tüm sayfa tablosu çerçevelerini
    /// (PDPT/PD/PT) PMM'e iade eder. Yalnızca PML4'ün kullanıcı yarısı
    /// (indeks 0-255) yürütülür; üst yarı (256-511) çekirdek adres uzacıyla
    /// paylaşılan klonlanmış tabloları işaret eder ve ASLA serbest bırakılmaz.
    ///
    /// Çağıranın süreç artık kendi CR3'ü altında çalışmıyor olması gerekir;
    /// aksi halde serbest bırakılan tablolar anında kullanıma açık CR3'ü bozar.
    pub fn free_user_page_tables(&mut self, pmm: &mut PhysicalMemoryManager) {
        self.free_user_address_space_tables(pmm, false);
    }

    /// Releases user mappings, their page tables, and the PML4 root.
    /// Use only for a newly-created address space whose user frames are owned
    /// exclusively by that address space (for example, a failed exec image).
    /// COW paylaşımlı çerçeveler bu yolda da serbest bırakılmaz.
    pub fn destroy_user_address_space(&mut self, pmm: &mut PhysicalMemoryManager) {
        self.free_user_address_space_tables(pmm, true);
        if self.pml4_phys != 0 {
            pmm.free_frame(self.pml4_phys);
            self.pml4_phys = 0;
        }
    }

    fn free_user_address_space_tables(
        &mut self,
        pmm: &mut PhysicalMemoryManager,
        free_leaf_frames: bool,
    ) {
        if self.pml4_phys == 0 {
            return;
        }
        unsafe {
            let pml4 = &mut *Self::table_ptr(self.pml4_phys);
            for entry_slot in pml4.entries.iter_mut().take(256) {
                let entry = *entry_slot;
                if entry & PageFlags::PRESENT == 0 {
                    continue;
                }
                *entry_slot = 0;
                let child = entry & 0x000F_FFFF_FFFF_F000;
                // PDPT girdisi 1 GB huge page ise alt seviye yoktur.
                if entry & PageFlags::HUGE_PAGE == 0 {
                    Self::free_lower_tables(child, pmm, 3, free_leaf_frames);
                }
            }
        }
    }

    /// `depth` = 3 (PDPT → PD), 2 (PD → PT), 1 (PT → yaprak).
    /// Her seviyede PRESENT girdi çocuk tabloya işaret eder; HUGE_PAGE
    /// girdileri sayfa tablosu üretmez (veri sayfası kaynak listesinde izlenir).
    unsafe fn free_lower_tables(
        table_phys: u64,
        pmm: &mut PhysicalMemoryManager,
        depth: u32,
        free_leaf_frames: bool,
    ) {
        let table = &mut *Self::table_ptr(table_phys);
        for entry_slot in table.entries.iter_mut() {
            let entry = *entry_slot;
            if entry & PageFlags::PRESENT == 0 {
                continue;
            }
            *entry_slot = 0;
            let child = entry & 0x000F_FFFF_FFFF_F000;
            match depth {
                3 if entry & PageFlags::HUGE_PAGE == 0 => {
                    Self::free_lower_tables(child, pmm, 2, free_leaf_frames);
                }
                2 if entry & PageFlags::HUGE_PAGE == 0 => {
                    Self::free_lower_tables(child, pmm, 1, free_leaf_frames);
                }
                1 if free_leaf_frames && entry & PageFlags::USER != 0 => {
                    // Copy-on-write: çerçeve paylaşılıyorsa yalnız referansı bırak,
                    // fiziksel çerçeveyi serbest bırakma.
                    match release_frame_shared(child) {
                        Some(false) => {}
                        _ => pmm.free_frame(child),
                    }
                }
                _ => {}
            }
        }
        pmm.free_frame(table_phys);
    }

    /// Mevcut kernel adres uzayından kernel mapping'leri bu adres uzayına kopyalar.
    /// Bu, kullanıcı süreçlerinin syscall sonrası kernel'e güvenle dönmesi için gereklidir.
    /// Kernel mapping'ler PML4'in üst yarısında (canonical üst uzay, indeks 256-511) bulunur.
    pub fn clone_kernel_mappings(
        &mut self,
        _pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        let kernel_as = AddressSpace::current();

        unsafe {
            let src_pml4 = &*Self::table_ptr(kernel_as.pml4_phys);
            let dst_pml4 = &mut *Self::table_ptr(self.pml4_phys);

            // Kernel mapping'ler PML4 indeks 256-511 aralığında (canonical üst uzay)
            for (i, dst_entry) in dst_pml4.entries.iter_mut().enumerate().skip(256) {
                let entry = src_pml4.entries[i];
                if (entry & PageFlags::PRESENT) != 0 {
                    // Kernel girişlerini doğrudan kopyala (USER bayrağı olmamalı)
                    *dst_entry = entry & !PageFlags::USER;
                }
            }
        }

        Ok(())
    }
}

/// Bellek yöneticisi, sayfalama ve kullanıcı kipi izolasyon doğrulama testi.
pub fn test_memory_and_isolation(pmm: &mut PhysicalMemoryManager) -> bool {
    crate::serial_print("[MM-TEST] --- Bellek Tahsisi ve Yalıtım Doğrulama Testi ---\n");

    // 1. PMM Çerçeve Tahsis ve Serbest Bırakma Testi
    let free_before = pmm.free_memory_bytes;
    let frame1 = match pmm.allocate_frame() {
        Some(f) => f,
        None => {
            crate::serial_print("[MM-TEST] PMM çerçeve tahsisi: BAŞARISIZ\n");
            return false;
        }
    };
    let frame2 = match pmm.allocate_frame() {
        Some(f) => f,
        None => {
            crate::serial_print("[MM-TEST] PMM 2. çerçeve tahsisi: BAŞARISIZ\n");
            pmm.free_frame(frame1);
            return false;
        }
    };
    if frame1 == frame2 || (frame1 % PAGE_SIZE != 0) || (frame2 % PAGE_SIZE != 0) {
        crate::serial_print("[MM-TEST] PMM çerçeve çakışması veya hizalama hatası: BAŞARISIZ\n");
        pmm.free_frame(frame1);
        pmm.free_frame(frame2);
        return false;
    }
    crate::serial_print("[MM-TEST] 1. PMM tahsisi (4 KB hizalı, benzersiz çerçeveler): BAŞARILI\n");

    pmm.free_frame(frame1);
    pmm.free_frame(frame2);
    if pmm.free_memory_bytes != free_before {
        crate::serial_print("[MM-TEST] PMM serbest bırakma sonrası bellek sızıntısı: BAŞARISIZ\n");
        return false;
    }
    crate::serial_print("[MM-TEST] 2. PMM serbest bırakma (sızıntısız iade): BAŞARILI\n");

    // 2. VMM Eşleme (Map) ve Çözümleme (Translate) Testi
    let test_frame = match pmm.allocate_frame() {
        Some(f) => f,
        None => return false,
    };
    let test_virt: u64 = 0x0000_7000_1000_0000; // Kullanıcı alanı sanal adresi
    let mut current_as = AddressSpace::current();

    // Kullanıcı kipi (Ring 3) veri sayfası bayraklarıyla eşle
    let map_res = current_as.map_page(test_virt, test_frame, PageFlags::USER_DATA, pmm);
    if map_res.is_err() {
        crate::serial_print("[MM-TEST] VMM sayfa eşleme hatası: BAŞARISIZ\n");
        pmm.free_frame(test_frame);
        return false;
    }

    // Çözümlemeyi kontrol et
    let translated = current_as.translate(test_virt);
    if translated != Some(test_frame) {
        crate::serial_print("[MM-TEST] VMM sanal->fiziksel çözümlemesi eşleşmedi: BAŞARISIZ\n");
        let _ = current_as.unmap_page(test_virt);
        pmm.free_frame(test_frame);
        return false;
    }
    crate::serial_print("[MM-TEST] 3. VMM Sayfa Eşleme & Çözümleme (0x");
    crate::serial::print_hex(test_virt);
    crate::serial_print(" -> 0x");
    crate::serial::print_hex(test_frame);
    crate::serial_print("): BAŞARILI\n");

    // 3. Kullanıcı Kipi ve İzolasyon Sınırları Kontrolü
    // Eşlenen sayfanın PT girdisini doğrudan okuyarak USER, WRITABLE ve NO_EXECUTE bitlerini doğrula
    let pml4_idx = ((test_virt >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((test_virt >> 30) & 0x1FF) as usize;
    let pd_idx = ((test_virt >> 21) & 0x1FF) as usize;
    let pt_idx = ((test_virt >> 12) & 0x1FF) as usize;

    let flags_valid = unsafe {
        let pml4 = &*AddressSpace::table_ptr(current_as.pml4_phys);
        let pdpt_phys = pml4.entries[pml4_idx] & 0x000F_FFFF_FFFF_F000;
        let pdpt = &*AddressSpace::table_ptr(pdpt_phys);
        let pd_phys = pdpt.entries[pdpt_idx] & 0x000F_FFFF_FFFF_F000;
        let pd = &*AddressSpace::table_ptr(pd_phys);
        let pt_phys = pd.entries[pd_idx] & 0x000F_FFFF_FFFF_F000;
        let pt = &*AddressSpace::table_ptr(pt_phys);
        let entry = pt.entries[pt_idx];

        let is_user = (entry & PageFlags::USER) != 0;
        let is_write = (entry & PageFlags::WRITABLE) != 0;
        let is_nx = (entry & PageFlags::NO_EXECUTE) != 0;
        is_user && is_write && is_nx
    };

    if flags_valid {
        crate::serial_print(
            "[MM-TEST] 4. Kullanıcı kipi yalıtım bayrakları (USER | WRITABLE | NX): DOĞRULANDI\n",
        );
    } else {
        crate::serial_print("[MM-TEST] Kullanıcı kipi güvenlik bayrakları eksik: BAŞARISIZ\n");
    }

    // 4. Eşlemeyi Kaldırma (Unmap) ve Geçersiz Kılma (TLB Flush) Testi
    let unmapped = current_as.unmap_page(test_virt);
    let after_unmap_trans = current_as.translate(test_virt);
    pmm.free_frame(test_frame);

    if unmapped == Some(test_frame) && after_unmap_trans.is_none() {
        crate::serial_print(
            "[MM-TEST] 5. VMM eşlemeyi kaldırma (unmap ve TLB geçersizleştirme): BAŞARILI\n",
        );
    } else {
        crate::serial_print("[MM-TEST] VMM eşlemeyi kaldırma işlemi başarısız: BAŞARISIZ\n");
        return false;
    }

    // 5. Bosalan alt seviye sayfa tablolarinin geri kazanimi (unmap + reclaim)
    let reclaim_base: u64 = 0x0000_7000_3000_0000;
    let reclaim_pages = 4usize;
    let reclaim_free_before = pmm.free_memory_bytes;
    let mut reclaim_frames = alloc::vec::Vec::new();
    let mut reclaim_mapped = true;
    for page in 0..reclaim_pages {
        let address = reclaim_base + page as u64 * PAGE_SIZE;
        let Some(frame) = pmm.allocate_frame() else {
            reclaim_mapped = false;
            break;
        };
        if current_as
            .map_page(address, frame, PageFlags::USER_DATA, pmm)
            .is_err()
        {
            pmm.free_frame(frame);
            reclaim_mapped = false;
            break;
        }
        reclaim_frames.push((address, frame));
    }

    let mut reclaim_ok = reclaim_mapped && reclaim_frames.len() == reclaim_pages;
    if reclaim_ok {
        // Her sayfayi geri kazanarak kaldir: yaprak cerceveleri ve bosalan
        // PT/PD/PDPT tablolari PMM'e iade edilmeli. `unmap_page_reclaim` yaprak
        // cerceveyi dondurur; serbest birakma cagarana aittir.
        for (address, frame) in &reclaim_frames {
            match current_as.unmap_page_reclaim(*address, pmm) {
                Some(leaf) if leaf == *frame => pmm.free_frame(leaf),
                _ => reclaim_ok = false,
            }
        }
        // Ikinci cagri: esleme yok, None donmeli.
        if reclaim_ok && current_as.unmap_page_reclaim(reclaim_base, pmm).is_some() {
            reclaim_ok = false;
        }
        // Tum adresler cevrilemez olmali.
        for (address, _) in &reclaim_frames {
            if current_as.translate(*address).is_some() {
                reclaim_ok = false;
            }
        }
        // Sizinti denetimi: yaprak cerceveler + tablolar geri gelmeli.
        if pmm.free_memory_bytes != reclaim_free_before {
            reclaim_ok = false;
        }
    }

    // Geri al: test basarisiz olsa bile surekli sizinti birakma. Yalnizca
    // hala eslenmis sayfalari kaldir; zaten iade edilenlere dokunma.
    for (address, frame) in reclaim_frames {
        if current_as.unmap_page(address).is_some() {
            pmm.free_frame(frame);
        }
    }

    if reclaim_ok {
        crate::serial_print(
            "[MM-TEST] 6. Bosalan alt sayfa tablolarinin geri kazanimi (PT/PD/PDPT): BASARILI\n",
        );
    } else {
        crate::serial_print("[MM-TEST] Alt sayfa tablosu geri kazanimi sizinti: BASARISIZ\n");
    }

    flags_valid && reclaim_ok
}

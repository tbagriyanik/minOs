//! x86_64 4-Seviyeli (4-Level Paging) Sanal Bellek Yöneticisi (VMM).
//!
//! Bu modül:
//! 1. PML4, PDPT, PD, PT sayfa tablolarının yönetimini,
//! 2. Sayfa izin bayraklarını (PRESENT, WRITABLE, USER, NO_EXECUTE),
//! 3. HHDM (Higher-Half Direct Map) üzerinden fiziksel adres <-> sanal adres erişimini,
//! 4. 4KB sanal sayfaları fiziksel sayfa çerçevelerine (PMM üzerinden) eşlemeyi (map/unmap),
//! 5. Çekirdek ve uygulama adres uzaylarının ayrımını sağlar.

use crate::drivers::phys_to_virt;
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};

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
pub struct AddressSpace {
    /// PML4 kök tablosunun fiziksel adresi (CR3'e yüklenecek değer)
    pub pml4_phys: u64,
}

impl AddressSpace {
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
    fn table_ptr(phys: u64) -> *mut PageTable {
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
        if (virt_addr % PAGE_SIZE) != 0 || (phys_addr % PAGE_SIZE) != 0 {
            return Err("Adresler 4KB sayfa sinirina hizali degil");
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

    /// Bir sanal adresin eşlemesini (mapping) kaldırır (unmap) ve fiziksel adresi döner.
    pub fn unmap_page(&mut self, virt_addr: u64) -> Option<u64> {
        if (virt_addr % PAGE_SIZE) != 0 {
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
}


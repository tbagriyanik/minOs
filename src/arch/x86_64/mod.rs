pub mod idt;
pub mod sync;
pub mod gdt;
pub mod smp;
pub mod ring3;
pub mod apic;

pub static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

static mut MMIO_FRAME_ALLOCATOR: Option<fn() -> Option<u64>> = None;

pub fn set_mmio_frame_allocator(alloc_fn: fn() -> Option<u64>) {
    unsafe { MMIO_FRAME_ALLOCATOR = Some(alloc_fn); }
}

fn alloc_table_frame() -> Option<u64> {
    unsafe {
        if let Some(alloc) = MMIO_FRAME_ALLOCATOR {
            alloc()
        } else {
            None
        }
    }
}

pub fn init_interrupts() {
    unsafe {
        // Erken CPU GDT + TSS yükle (BSP = 0)
        gdt::load_for_cpu(0);
        // IDT kapılarını erkenden yükle (#DE, #UD, #DF, #SS, #GP, #PF, 0x80)
        ring3::install_gates();
        let idt_ptr = core::ptr::addr_of_mut!(IDT);
        (*idt_ptr).load();
    }
}

pub fn enable_sse() {
    unsafe {
        let mut cr0: u64;
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        cr0 &= !(1 << 2); // EM = 0
        cr0 |= 1 << 1;    // MP = 1
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack, preserves_flags));

        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= (1 << 9) | (1 << 10); // OSFXSR = 1, OSXMMEXCPT = 1
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
    }
}

pub fn disable_pic() {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0xFFu8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0xFFu8, options(nomem, nostack, preserves_flags));
    }
}

/// Cihaz MMIO bölgesini (2MB hizalı) mevcut sayfa tablosuna HHDM üzerinden
/// 2MB huge page olarak ekler. Limine HHDM yalnızca RAM'i map'lediğinden
/// PCI MMIO bölgesi için bu elle eşleme gerekir.
pub fn mmio_map_2mb(phys_2mb_aligned: u64) {
    unsafe {
        let hhdm = crate::drivers::phys_to_virt(0);
        let v = hhdm + phys_2mb_aligned;

        let mut cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        let pml4_phys = cr3 & 0x000F_FFFF_FFFF_F000;
        let pml4 = (hhdm + pml4_phys) as *mut u64;

        let i4 = ((v >> 39) & 511) as usize;
        let mut pml4e = pml4.add(i4).read_volatile();
        if (pml4e & 1) == 0 {
            if let Some(table_phys) = alloc_table_frame() {
                core::ptr::write_bytes((hhdm + table_phys) as *mut u8, 0, 4096);
                pml4e = table_phys | 0x03;
                pml4.add(i4).write_volatile(pml4e);
            } else {
                return;
            }
        }

        let pdpt_phys = pml4e & 0x000F_FFFF_FFFF_F000;
        if pdpt_phys == 0 { return; }
        let pdpt = (hhdm + pdpt_phys) as *mut u64;

        let i3 = ((v >> 30) & 511) as usize;
        let mut pdpte = pdpt.add(i3).read_volatile();

        // 1GB büyük sayfa (huge page) kontrolü:
        // Eğer Present (bit 0) ve PS (bit 7) bayrakları kuruluysa,
        // Limine bu 1GB fiziksel bölgeyi zaten sanal belleğe eşlemiştir.
        // Mevcut eşlemeyi bozmamak için üzerine yazılmamalıdır.
        if (pdpte & 1) != 0 && (pdpte & 0x80) != 0 {
            return;
        }

        if (pdpte & 1) == 0 {
            if let Some(table_phys) = alloc_table_frame() {
                core::ptr::write_bytes((hhdm + table_phys) as *mut u8, 0, 4096);
                pdpte = table_phys | 0x03;
                pdpt.add(i3).write_volatile(pdpte);
            } else {
                return;
            }
        }

        let pd_phys = pdpte & 0x000F_FFFF_FFFF_F000;
        if pd_phys == 0 { return; }
        let pd = (hhdm + pd_phys) as *mut u64;

        let i2 = ((v >> 21) & 511) as usize;
        let pde = pd.add(i2).read_volatile();

        // Zaten eşlenmişse kontrol et
        if (pde & 1) != 0 {
            if (pde & 0x80) != 0 {
                // Zaten 2MB huge page olarak tüm aralık eşlenmiş
                return;
            }
            // 4KB sayfa tablosu (PT) mevcut; Limine'in yarım bıraktığı 4KB sayfaları tamamla
            let pt_phys = pde & 0x000F_FFFF_FFFF_F000;
            if pt_phys != 0 {
                let pt = (hhdm + pt_phys) as *mut u64;
                for pti in 0..512 {
                    let pte = pt.add(pti).read_volatile();
                    if (pte & 1) == 0 {
                        let page_phys = phys_2mb_aligned + (pti as u64 * 4096);
                        pt.add(pti).write_volatile(page_phys | 0x13);
                    }
                }
            }
            return;
        }

        // 2MB sayfa: Present (0x1) | RW (0x2) | PCD (0x10) | PS (0x80) = 0x93
        pd.add(i2).write_volatile(phys_2mb_aligned | 0x93);

        // Sanal adres için TLB'yi temizle
        core::arch::asm!("invlpg [{}]", in(reg) v, options(nostack, preserves_flags));
    }
}

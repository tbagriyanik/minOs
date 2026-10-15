pub mod idt;
pub mod sync;
pub mod gdt;
pub mod smp;
pub mod ring3;

pub static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

pub fn init_interrupts() {
    unsafe {
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
        let pml4 = (hhdm + pml4_phys) as *const u64;

        let i4 = ((v >> 39) & 511) as usize;
        let pml4e = pml4.add(i4).read_volatile();
        let pdpt_phys = pml4e & 0x000F_FFFF_FFFF_F000;
        let pdpt = (hhdm + pdpt_phys) as *const u64;

        let i3 = ((v >> 30) & 511) as usize;
        let pdpte = pdpt.add(i3).read_volatile();
        let pd_phys = pdpte & 0x000F_FFFF_FFFF_F000;
        let pd = (hhdm + pd_phys) as *mut u64;

        let i2 = ((v >> 21) & 511) as usize;
        // 2MB page: Present | RW | PS(1<<7)
        pd.add(i2).write_volatile(phys_2mb_aligned | 0x83);

        // TLB temizle (CR3 yeniden yükle)
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));
    }
}


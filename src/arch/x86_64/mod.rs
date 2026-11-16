pub mod apic;
pub mod context;
pub mod gdt;
pub mod idt;
pub mod ioapic;
pub mod pku;
pub mod ring3;
pub mod smp;
pub mod sync;

pub static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

static mut MMIO_FRAME_ALLOCATOR: Option<fn() -> Option<u64>> = None;

pub fn set_mmio_frame_allocator(alloc_fn: fn() -> Option<u64>) {
    unsafe {
        MMIO_FRAME_ALLOCATOR = Some(alloc_fn);
    }
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

/// `cpu` çağıran CPU'nun indeksi olmalıdır: her çekirdek kendi GDT/TSS
/// slotunu yükler. AP'lerin `cpu = 0` geçmesi, AP'nin TSS'ini BSP'nin
/// kernel yığınına (`TSSS[0].rsp0`) bağlayacağı için yasaktır.
pub fn init_interrupts(cpu: usize) {
    unsafe {
        // Erken CPU GDT + TSS yükle (çağıranın kendi slotu)
        gdt::load_for_cpu(cpu);
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
        cr0 |= 1 << 1; // MP = 1
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack, preserves_flags));

        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= (1 << 9) | (1 << 10); // OSFXSR = 1, OSXMMEXCPT = 1
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
    }
}

/// CR4.PCIDE (bit 17) açar: Process-Context Identifiers.
/// PCID ile CR3 değişikliklerinde TLB tam temizleme (full flush) gerekmez;
/// her PCID için ayrı TLB girdileri korunur. Kernel PCID = 0, user süreçler
/// için PCID atamaları yapılır.
///
/// TR / EN: Enable CR4.PCIDE (bit 17): Process-Context Identifiers.
/// With PCID, CR3 changes do not require full TLB flush; separate TLB entries
/// are maintained per PCID. Kernel PCID = 0, user processes get assigned PCIDs.
pub fn enable_pcid() {
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= 1 << 17; // PCIDE = 1
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
    }
}

/// CPUID.07H:EBX.INVPCID (bit 10): PCID'e özgü TLB geçersizleştirme
/// desteği. `INVLPG` yalnızca geçerli PCID'yi etkilediğinden, paylaşılan
/// kernel-yarı sayfa tabloları değiştiğinde diğer süreç PCID'lerinin
/// bayat girdilerini temizlemek için INVPCID şarttır. Destek yoksa
/// `alloc_pcid` tüm süreçlere PCID=0 verir (tek TLB bağlamı).
pub fn invpcid_supported() -> bool {
    (core::arch::x86_64::__cpuid_count(7, 0).ebx & (1 << 10)) != 0
}

/// INVPCID tip 2: tüm PCID'ler için tüm TLB çevirilerini (global sayfalar
/// dahil) geçersiz kılar. Kernel-yarısı sayfa tabloları tüm adres
/// uzaylarıyla paylaşıldığından (clone_kernel_mappings), bu tablolarda
/// yapılan her değişiklikten sonra çağrılmalıdır — aksi halde diğer
/// süreçlerin TLB'leri bayat fiziksel çerçeveleri göstermeye devam eder.
///
/// Yalnızca çağıran mantıksal işlemciyi etkiler; SMP'te diğer CPU'lar için
/// IPI tabanlı shootdown ayrıca gerekir (mevcut `invlpg` yolları da aynı
/// sınırlamaya sahiptir).
pub fn invpcid_flush_all() {
    if !invpcid_supported() {
        return;
    }
    unsafe {
        // INVPCID type, m128 descriptor: desc[0] = PCID (kullanılmaz),
        // desc[1] = doğrusal adres (kullanılmaz). Tip 2 için tüm alanlar 0.
        let desc = [0u64; 2];
        core::arch::asm!(
            "invpcid {}, [{}]",
            in(reg) 2u64,
            in(reg) desc.as_ptr(),
            options(nostack, preserves_flags),
        );
    }
}

/// INVPCID tip 1: verilen PCID'ye ait tüm eşlemeleri (global sayfalar
/// hariç) geçersiz kılar. Bir PCID yeniden kullanılırken önceki sahibin
/// bayat çevirilerini temizlemek için `alloc_pcid` tarafından çağrılır.
pub fn invpcid_flush_context(pcid: u16) {
    if !invpcid_supported() {
        return;
    }
    unsafe {
        // Descriptor: bit 11..0 = PCID, bit 63..12 = 0, bit 127..64 =
        // doğrusal adres (tip 1'de yok sayılır).
        let desc = [pcid as u64, 0u64];
        core::arch::asm!(
            "invpcid {}, [{}]",
            in(reg) 1u64,
            in(reg) desc.as_ptr(),
            options(nostack, preserves_flags),
        );
    }
}

pub fn disable_pic() {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0xFFu8, options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0xFFu8, options(nomem, nostack, preserves_flags));
    }
}

/// NX (yürütülemez) sayfa bayrağı (x86-64 PTE/PDE bit 63).
const NX_BIT: u64 = 1 << 63;

/// Cihaz MMIO bölgesini (2MB hizalı) mevcut sayfa tablosuna HHDM üzerinden
/// 2MB huge page olarak ekler. Limine HHDM yalnızca RAM'i map'lediğinden
/// PCI MMIO bölgesi için bu elle eşleme gerekir.
pub fn mmio_map_2mb(phys_2mb_aligned: u64) {
    mmio_map_2mb_flags(phys_2mb_aligned, 0x93, 0x13);
}

/// Aynı eşlemeyi verilen PDE/PTE bayraklarıyla yapar.
///
/// `pde_flags`: 2MB huge page girdisi için bayraklar (P|RW|PCD|PS = 0x93).
/// `pte_flags`: 4KB sayfa girdisi için bayraklar (P|RW|US = 0x13).
/// Salt-okunur veri bölgeleri için `NX_BIT` eklenerek yürütülemez işaretlenir.
fn mmio_map_2mb_flags(phys_2mb_aligned: u64, pde_flags: u64, pte_flags: u64) {
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
        if pdpt_phys == 0 {
            return;
        }
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
        if pd_phys == 0 {
            return;
        }
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
                        pt.add(pti).write_volatile(page_phys | pte_flags);
                    }
                }
            }
            return;
        }

        // 2MB sayfa: Present (0x1) | RW (0x2) | PCD (0x10) | PS (0x80) = 0x93
        pd.add(i2).write_volatile(phys_2mb_aligned | pde_flags);

        // Sanal adres için TLB'yi temizle
        core::arch::asm!("invlpg [{}]", in(reg) v, options(nostack, preserves_flags));
    }
}

/// Verilen fiziksel adresin HHDM üzerinden **okunabilir** olmasını sağlar.
///
/// Limine'nin HHDM eşlemesi yalnızca "RAM" olarak sınıflandırdığı fiziksel
/// bölgeleri kapsar. ACPI tablolarının bulunduğu ROM / "ACPI reclaimable"
/// sayfalar (QEMU'da RSDP ve MADT 0xE0000 F-segmentinde) HHDM'de
/// *not-present* kalır; eşlenmeden okunmaya çalışılırsa #PF üretilir.
///
/// Fonksiyon, hedef adresin içinde bulunduğu 2 MiB bloğunu HHDM'ye ekler ve
/// sayfaları yürütülemez (NX) işaretler.
pub fn map_phys_readable(phys: u64) {
    mmio_map_2mb_flags(phys & !0x1F_FFFF, 0x93 | NX_BIT, 0x13 | NX_BIT);
}

/// Düşük fiziksel bölgeyi (0..2 MiB: EBDA, BIOS ROM / F-segment) HHDM'de
/// erişilebilir hâle getirir.
///
/// ACPI tablo taramaları RSDP'yi bu bölgede arar; Limine bu sayfaları
/// "RAM değil" sayıp HHDM'ye eşlemediği için tarama öncesinde çağrılmalıdır.
pub fn map_legacy_acpi_region() {
    map_phys_readable(0);
}

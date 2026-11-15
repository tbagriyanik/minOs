//! IOAPIC (I/O Advanced Programmable Interrupt Controller) sürücüsü.
//!
//! ISA IRQ hatları (klavye IRQ1, fare IRQ12) 8259 PIC yerine IOAPIC
//! üzerinden Local APIC'e yönlendirilir. PIC `disable_pic()` ile tümüyle
//! maskeli olduğundan IOAPIC tek teslimat yoludur.
//!
//! Akış:
//! 1. [`discover`] ACPI MADT tablosunu bulur, IOAPIC denetleyicilerini
//!    MMIO'ya eşler ve tüm yeniden yönlendirme kayıtlarını maskeler.
//! 2. [`route_gsi`] seçilen GSI'yi istenen IDT vektörüne yönlendirir.
//! 3. Kesme geldiğinde işleyici [`eoi`] ile IOAPIC + LAPIC EOI gönderir.

use core::sync::atomic::{AtomicBool, Ordering};

/// Desteklenen azami IOAPIC denetleyicisi sayısı.
const MAX_IOAPICS: usize = 4;

/// IOAPIC kayıt seçme (IOREGSEL) yazmacı ofseti.
const REG_IOREGSEL: u32 = 0x00;
/// IOAPIC pencere (IOWIN) yazmacı ofseti.
const REG_IOWIN: u32 = 0x10;
/// IOAPIC EOI yazmacı ofseti (vektör doğrudan yazılır).
const REG_EOI: u32 = 0x40;
/// Yönlendirme tablosu ilk kaydının register indeksi.
const REG_REDTBL_BASE: u32 = 0x10;

/// Keşfedilmiş tek bir IOAPIC denetleyicisi.
#[derive(Clone, Copy)]
struct Ioapic {
    /// MMIO sanal taban adresi (u32 yazmaçlar).
    base: *mut u32,
    /// Bu denetleyicinin karşıladığı ilk GSI numarası.
    gsi_base: u32,
    /// Yönlendirme kaydı sayısı.
    entries: u32,
}

static mut IOAPICS: [Ioapic; MAX_IOAPICS] = [Ioapic {
    base: core::ptr::null_mut(),
    gsi_base: 0,
    entries: 0,
}; MAX_IOAPICS];
static IOAPIC_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// `route_gsi` ile kurulmuş yönlendirmeler; EOI'nin doğru IOAPIC'e
/// ulaşması için vektör → denetleyici eşlemesi tutulur.
#[derive(Clone, Copy)]
struct Route {
    vector: u8,
    ioapic: usize,
}

static mut ROUTES: [Route; 8] = [Route {
    vector: 0,
    ioapic: usize::MAX,
}; 8];
static ROUTE_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

static DISCOVERED: AtomicBool = AtomicBool::new(false);

// --- ACPI tablo keşfi ---------------------------------------------------------

#[repr(C)]
struct RsdpDescriptor {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C)]
struct AcpiHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// RSDP üzerinden XSDT/RSDT kök tablo sanal adresini döndürür.
fn find_root_table() -> Option<u64> {
    let rsdp_vaddr = crate::drivers::acpi_battery::find_rsdp()?;
    let rsdp = unsafe { (rsdp_vaddr as *const RsdpDescriptor).read_unaligned() };
    if &rsdp.signature != b"RSD PTR " {
        return None;
    }

    if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
        // XSDT fiziksel adresi RAM dışında bir bölgede olabilir; okumadan
        // önce HHDM'de erişilebilir hâle getir.
        crate::arch::x86_64::map_phys_readable(rsdp.xsdt_address);
        let v = crate::drivers::phys_to_virt(rsdp.xsdt_address);
        if unsafe { (v as *const AcpiHeader).read_unaligned().length } >= 36 {
            return Some(v);
        }
    }
    if rsdp.rsdt_address != 0 {
        crate::arch::x86_64::map_phys_readable(rsdp.rsdt_address as u64);
        let v = crate::drivers::phys_to_virt(rsdp.rsdt_address as u64);
        if unsafe { (v as *const AcpiHeader).read_unaligned().length } >= 36 {
            return Some(v);
        }
    }
    None
}

/// XSDT/RSDT girdilerini gezer; "APIC" (MADT) tablosunun sanal adresini bulur.
fn find_madt() -> Option<u64> {
    let root_vaddr = find_root_table()?;
    let root = unsafe { (root_vaddr as *const AcpiHeader).read_unaligned() };
    if root.length < 36 || root.length > 0x100000 {
        return None;
    }

    let xsdt = root.signature == *b"XSDT";
    let entries_len = (root.length as usize).saturating_sub(36);
    let data_ptr = root_vaddr + 36;

    if xsdt {
        let count = entries_len / 8;
        for i in 0..count {
            let table_phys =
                unsafe { ((data_ptr + (i * 8) as u64) as *const u64).read_unaligned() };
            if table_phys != 0 {
                // MADT genellikle ROM/"reclaimable" bölgede durur; eşle.
                crate::arch::x86_64::map_phys_readable(table_phys);
                if let Some(madt) = check_madt(crate::drivers::phys_to_virt(table_phys)) {
                    return Some(madt);
                }
            }
        }
    } else {
        let count = entries_len / 4;
        for i in 0..count {
            let table_phys =
                unsafe { ((data_ptr + (i * 4) as u64) as *const u32).read_unaligned() as u64 };
            if table_phys != 0 {
                crate::arch::x86_64::map_phys_readable(table_phys);
                if let Some(madt) = check_madt(crate::drivers::phys_to_virt(table_phys)) {
                    return Some(madt);
                }
            }
        }
    }
    None
}

fn check_madt(vaddr: u64) -> Option<u64> {
    let hdr = unsafe { (vaddr as *const AcpiHeader).read_unaligned() };
    if &hdr.signature == b"APIC" && hdr.length >= 44 && hdr.length <= 0x10000 {
        Some(vaddr)
    } else {
        None
    }
}

// --- IOAPIC kayıt erişimi -----------------------------------------------------

#[inline(always)]
unsafe fn ioapic_write_reg(apic: usize, reg: u32, val: u32) {
    let base = unsafe { (*core::ptr::addr_of!(IOAPICS))[apic].base };
    unsafe {
        core::ptr::write_volatile(base.byte_add(REG_IOREGSEL as usize), reg);
        core::ptr::write_volatile(base.byte_add(REG_IOWIN as usize), val);
    }
}

#[inline(always)]
unsafe fn ioapic_read_reg(apic: usize, reg: u32) -> u32 {
    let base = unsafe { (*core::ptr::addr_of!(IOAPICS))[apic].base };
    unsafe {
        core::ptr::write_volatile(base.byte_add(REG_IOREGSEL as usize), reg);
        core::ptr::read_volatile(base.byte_add(REG_IOWIN as usize))
    }
}

/// Tüm IOAPIC'lerin tüm yönlendirme kayıtlarını maskeler (güvenli başlangıç).
unsafe fn mask_all_entries() {
    let count = IOAPIC_COUNT.load(Ordering::Relaxed);
    for apic in 0..count.min(MAX_IOAPICS) {
        let entries = unsafe { (*core::ptr::addr_of!(IOAPICS))[apic].entries };
        for idx in 0..entries {
            let reg = REG_REDTBL_BASE + idx * 2;
            // Mevcut düşük yazmacı koru, yalnızca mask bitini (16) kur.
            let low = unsafe { ioapic_read_reg(apic, reg) };
            unsafe { ioapic_write_reg(apic, reg, low | (1 << 16)) };
        }
    }
}

// --- Genel API ----------------------------------------------------------------

/// ACPI MADT'yi tarar, IOAPIC'leri eşler ve kayıtları maskeler.
/// Yalnızca BSP, `mmio_map_2mb` hazır olduktan sonra çağırmalıdır.
/// Başarılıysa true döner.
pub fn discover() -> bool {
    if DISCOVERED.swap(true, Ordering::SeqCst) {
        return IOAPIC_COUNT.load(Ordering::Relaxed) > 0;
    }

    let Some(madt_vaddr) = find_madt() else {
        crate::serial_print("[IOAPIC] MADT tablosu bulunamadı; IRQ yönlendirme kapalı.\n");
        return false;
    };
    crate::serial_print("[IOAPIC] MADT @0x");
    crate::serial::print_hex(madt_vaddr);
    crate::serial_print("\n");

    let madt_len = unsafe { (madt_vaddr as *const AcpiHeader).read_unaligned().length as usize };
    let mut pos = madt_vaddr + 44; // MADT başlığı 44 bayt.
    let end = madt_vaddr + madt_len as u64;

    let mut iso_irq1: Option<(u32, u16)> = None;
    let mut iso_irq12: Option<(u32, u16)> = None;

    while pos + 2 <= end {
        let entry_type = unsafe { (pos as *const u8).read_unaligned() };
        let entry_len = unsafe { ((pos + 1) as *const u8).read_unaligned() } as usize;
        if entry_len < 2 || pos + entry_len as u64 > end {
            break;
        }

        match entry_type {
            // IOAPIC denetleyicisi: id(1) reserved(1) address(4) gsi_base(4)
            1 if entry_len >= 12 => {
                let phys = unsafe { ((pos + 4) as *const u32).read_unaligned() } as u64;
                let gsi_base = unsafe { ((pos + 8) as *const u32).read_unaligned() };
                if phys != 0 && IOAPIC_COUNT.load(Ordering::Relaxed) < MAX_IOAPICS {
                    let aligned = phys & !0x1F_FFFF;
                    crate::arch::x86_64::mmio_map_2mb(aligned);
                    let virt = crate::drivers::phys_to_virt(phys);
                    let idx = IOAPIC_COUNT.fetch_add(1, Ordering::SeqCst);
                    unsafe {
                        let apics = &mut *core::ptr::addr_of_mut!(IOAPICS);
                        apics[idx] = Ioapic {
                            base: virt as *mut u32,
                            gsi_base,
                            entries: 24,
                        };
                    }
                    crate::serial_print("[IOAPIC] Denetleyici @0x");
                    crate::serial::print_hex(phys);
                    crate::serial_print(" GSI taban=");
                    crate::serial::print_dec(gsi_base as u64);
                    crate::serial_print("\n");
                }
            }
            // Interrupt Source Override: bus(1) source(1) gsi(4) flags(2)
            2 if entry_len >= 10 => {
                let bus = unsafe { ((pos + 2) as *const u8).read_unaligned() };
                let source = unsafe { ((pos + 3) as *const u8).read_unaligned() };
                let gsi = unsafe { ((pos + 4) as *const u32).read_unaligned() };
                let flags = unsafe { ((pos + 8) as *const u16).read_unaligned() };
                if bus == 0 {
                    // ISA veriyolu. Bit1 = aktif düşük polarite.
                    match source {
                        1 => iso_irq1 = Some((gsi, flags)),
                        12 => iso_irq12 = Some((gsi, flags)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        pos += entry_len as u64;
    }

    let count = IOAPIC_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        crate::serial_print("[IOAPIC] IOAPIC denetleyicisi bulunamadı.\n");
        return false;
    }

    unsafe { mask_all_entries() };

    // ISO bilgilerini sakla: GSI çözümü `gsi_for_irq` içinde yapılır.
    unsafe {
        ISO_IRQ1 = iso_irq1;
        ISO_IRQ12 = iso_irq12;
    }

    crate::serial_print("[IOAPIC] Keşif tamamlandı; kayıtlar maskelendi.\n");
    true
}

static mut ISO_IRQ1: Option<(u32, u16)> = None;
static mut ISO_IRQ12: Option<(u32, u16)> = None;

/// ISA IRQ numarası için GSI ve ISO bayraklarını döndürür.
/// ISO kaydı yoksa GSI = IRQ (doğrudan eşleme) varsayılır.
pub fn gsi_for_irq(irq: u8) -> Option<(u32, u16)> {
    unsafe {
        match irq {
            1 => Some(ISO_IRQ1.unwrap_or((1, 0))),
            12 => Some(ISO_IRQ12.unwrap_or((12, 0))),
            _ => Some((irq as u32, 0)),
        }
    }
}

/// `gsi`'yi `vector` IDT vektörüne yönlendirir; hedef `dest` LAPIC kimliği.
/// `active_low`/`level` ISO bayraklarından gelir. Yalnızca BSP çağırmalıdır.
pub fn route_gsi(gsi: u32, vector: u8, active_low: bool, level: bool, dest: u32) -> bool {
    let count = IOAPIC_COUNT.load(Ordering::Relaxed).min(MAX_IOAPICS);
    for apic in 0..count {
        let (base_gsi, entries) = unsafe {
            let a = &(*core::ptr::addr_of!(IOAPICS))[apic];
            (a.gsi_base, a.entries)
        };
        if gsi >= base_gsi && gsi < base_gsi + entries {
            let idx = gsi - base_gsi;
            let mut low = vector as u32; // Fixed delivery mode (000), physical dest.
            if active_low {
                low |= 1 << 13; // Polarity: aktif düşük
            }
            if level {
                low |= 1 << 15; // Trigger: seviye
            }
            // Mask bitini temizle (teslimata aç).
            low &= !(1 << 16);

            // Varış alanı: üst yazmacın bit 56-63'ü = LAPIC kimliği.
            // high dword'un bit 24-31'i fiziksel hedefin 8 bitidir.
            let high = (dest & 0xFF) << 24;

            unsafe {
                let reg = REG_REDTBL_BASE + idx * 2;
                ioapic_write_reg(apic, reg, low);
                ioapic_write_reg(apic, reg + 1, high);
            }

            // EOI eşlemesini kaydet.
            let routes = unsafe { &mut *core::ptr::addr_of_mut!(ROUTES) };
            let rc = ROUTE_COUNT.load(Ordering::Relaxed);
            if rc < routes.len() {
                routes[rc] = Route {
                    vector,
                    ioapic: apic,
                };
                ROUTE_COUNT.store(rc + 1, Ordering::SeqCst);
            }
            return true;
        }
    }
    false
}

/// Kesme işleyicisinden çağrılır: IOAPIC EOI + LAPIC EOI gönderir.
/// `vector`, yönlendirilen IDT vektörüdür.
pub fn eoi(vector: u8) {
    let rc = ROUTE_COUNT.load(Ordering::Relaxed).min(8);
    unsafe {
        let routes = &*core::ptr::addr_of!(ROUTES);
        for r in routes.iter().take(rc) {
            if r.vector == vector && r.ioapic != usize::MAX {
                let base = (*core::ptr::addr_of!(IOAPICS))[r.ioapic].base;
                core::ptr::write_volatile(base.byte_add(REG_EOI as usize), vector as u32);
                break;
            }
        }
    }
    crate::arch::x86_64::apic::send_eoi();
}

/// IOAPIC keşfinin başarılı olup olmadığını bildirir.
pub fn is_available() -> bool {
    DISCOVERED.load(Ordering::Acquire) && IOAPIC_COUNT.load(Ordering::Relaxed) > 0
}

//! Çekirdek GDT + TSS kurulumu (SMP uyumlu, çekirdek başına).
//!
//! Limine'in GDT'si yalnızca Ring0 segmentleri içerir. Ring3 izolasyonu için
//! her CPU kendi GDT'sini yükler:
//!
//! ```text
//! 0x00: null
//! 0x08: Ring0 kod   (KCODE)
//! 0x10: Ring0 veri  (KDATA)
//! 0x1B: Ring3 kod   (UCODE, RPL=3)
//! 0x23: Ring3 veri  (UDATA, RPL=3)
//! 0x28: TSS (16 bayt, 2 slot)
//! ```
//!
//! TSS yalnızca `rsp0` (Ring3 -> Ring0 geçiş yığını) için kullanılır.

use super::smp::MAX_CPUS;

pub const SEL_KCODE: u16 = 0x08;
pub const SEL_KDATA: u16 = 0x10;
pub const SEL_UCODE: u16 = 0x1B;
pub const SEL_UDATA: u16 = 0x23;
pub const SEL_TSS: u16 = 0x28;

/// 64-bit TSS (tam 104 bayt, donanım standardı).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Tss {
    pub reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: u64,
    pub ist: [u64; 7],
    pub reserved2: u64,
    pub reserved3: u16,
    pub iomap_base: u16,
}

impl Tss {
    const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iomap_base: 104,
        }
    }
}

/// CPU başına GDT (7 x u64: null, kcode, kdata, ucode, udata, tss_lo, tss_hi).
#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct CpuGdt([u64; 7]);

impl CpuGdt {
    const fn new() -> Self {
        Self([
            0x0000_0000_0000_0000, // null
            0x00AF_9A00_0000_FFFF, // 0x08: Ring0 64-bit kod
            0x00CF_9200_0000_FFFF, // 0x10: Ring0 veri
            0x00AF_FA00_0000_FFFF, // 0x1B: Ring3 64-bit kod (DPL=3)
            0x00CF_F200_0000_FFFF, // 0x23: Ring3 veri (DPL=3)
            0, // TSS düşük (init sırasında doldurulur)
            0, // TSS yüksek
        ])
    }
}

#[repr(C, align(16))]
pub struct AlignedDfStack(pub [u8; 4096]);

static mut GDTS: [CpuGdt; MAX_CPUS] = [CpuGdt::new(); MAX_CPUS];
static mut TSSS: [Tss; MAX_CPUS] = [Tss::new(); MAX_CPUS];
static mut GDT_READY: [bool; MAX_CPUS] = [false; MAX_CPUS];
static mut DF_STACKS: [AlignedDfStack; MAX_CPUS] = [const { AlignedDfStack([0; 4096]) }; MAX_CPUS];

#[repr(C, packed)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

fn tss_descriptor(tss: &Tss) -> (u64, u64) {
    let base = tss as *const Tss as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    let lo = (limit & 0xFFFF)
        | ((base & 0xFFFF) << 16)
        | (((base >> 16) & 0xFF) << 32)
        | (0x89 << 40) // P=1, type=0x9 (kullanılabilir 64-bit TSS)
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    let hi = base >> 32;
    (lo, hi)
}

/// `cpu` için GDT+TSS kurar ve yükler. BSP için `cpu = 0`.
/// SAFETY: her CPU yalnızca kendi slotuna dokunmalıdır.
pub unsafe fn load_for_cpu(cpu: usize) {
    if cpu >= MAX_CPUS {
        return;
    }
    let gdt = &mut *core::ptr::addr_of_mut!(GDTS[cpu]);
    let tss = &mut *core::ptr::addr_of_mut!(TSSS[cpu]);
    *gdt = CpuGdt::new();
    // IST1 = Çift hata (#DF) yığını (üçlü hatayı ve donanım resetini önler)
    let df_top = (core::ptr::addr_of!(DF_STACKS[cpu].0) as u64 + 4096) & !0xF;
    core::ptr::write_unaligned(core::ptr::addr_of_mut!(tss.ist[0]), df_top);
    let (lo, hi) = tss_descriptor(&*tss);
    gdt.0[5] = lo;
    gdt.0[6] = hi;

    let ptr = GdtPtr {
        limit: (core::mem::size_of::<CpuGdt>() - 1) as u16,
        base: gdt as *const CpuGdt as u64,
    };
    core::arch::asm!(
        "lgdt [{ptr}]",
        "mov ax, {kdata:x}",
        "mov ds, ax",
        "mov es, ax",
        "mov ss, ax",
        "mov ax, 0",
        "mov fs, ax",
        "mov gs, ax",
        // CS'yi uzun dönüşle yeniden yükle: push KCODE, push hedef, retfq.
        "lea rax, [rip + 2f]",
        "push {kcode}",
        "push rax",
        "retfq",
        "2:",
        // Görev yazmacını (TR) TSS'ye bağla.
        "mov ax, {tss_sel:x}",
        "ltr ax",
        ptr = in(reg) &ptr,
        kdata = in(reg) SEL_KDATA,
        kcode = in(reg) SEL_KCODE as u64,
        tss_sel = in(reg) SEL_TSS,
        options(),
    );
    *core::ptr::addr_of_mut!(GDT_READY[cpu]) = true;
}

/// Ring3 -> Ring0 geçişlerinde kullanılacak çekirdek yığın tepesini ayarlar.
pub fn set_kernel_stack(cpu: usize, rsp0_top: u64) {
    if cpu >= MAX_CPUS {
        return;
    }
    unsafe {
        let tss = &mut *core::ptr::addr_of_mut!(TSSS[cpu]);
        core::ptr::write_unaligned(core::ptr::addr_of_mut!(tss.rsp0), rsp0_top);
    }
}

pub fn is_loaded(cpu: usize) -> bool {
    if cpu >= MAX_CPUS {
        return false;
    }
    unsafe { *core::ptr::addr_of!(GDT_READY[cpu]) }
}

/// Tanılama: TSS adresi, rsp0 ve GDT TSS tanımlayıcısı.
pub fn tss_addr(cpu: usize) -> u64 {
    unsafe { core::ptr::addr_of!(TSSS[cpu.min(MAX_CPUS - 1)]) as u64 }
}

pub fn rsp0(cpu: usize) -> u64 {
    unsafe {
        let tss = &*core::ptr::addr_of!(TSSS[cpu.min(MAX_CPUS - 1)]);
        core::ptr::read_unaligned(core::ptr::addr_of!(tss.rsp0))
    }
}

pub fn desc(cpu: usize, idx: usize) -> u64 {
    unsafe {
        let g = &*core::ptr::addr_of!(GDTS[cpu.min(MAX_CPUS - 1)]);
        g.0[idx.min(6)]
    }
}

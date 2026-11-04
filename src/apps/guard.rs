//! Yerleşik uygulamalar için hata izolasyonu (fault containment).
//!
//! Yerleşik uygulamalar çekirdek adres alanını ve ring 0'ı paylaşır. Bu nedenle
//! hatalı bir işaretçi MMU tarafından durdurulamaz. Bu modül, uygulama kodu
//! çalıştırılmadan önce bir kurtarma noktası kaydeder; uygulama içinde oluşan
//! sayfa hatası / genel koruma hatası / panic durumunda yığın ve callee-saved
//! yazmaçlar geri yüklenir, hatalı uygulama kapatılır ve çekirdek çalışmaya
//! devam eder.
//!
//! Bu tam bir bellek izolasyonu değildir: aynı adres alanındaki hatalı bir
//! yazma, hata oluşmadan çekirdek verisini bozabilir. Gerçek izolasyon için
//! uygulamaların ring 3 süreçleri olarak ayrı adres alanlarında çalıştırılması
//! gerekir. Buradaki katman, hatalı uygulamanın tüm sistemi düşürmesini
//! engelleyen savunma hattıdır.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::x86_64::gdt;
use crate::arch::x86_64::ring3::{EF_RFLAGS, EF_RIP, EF_CS, F_R15};
use crate::arch::x86_64::smp::{current_cpu_index, MAX_CPUS};

/// Hiçbir uygulamaya bağlı olmayan bağlam (masaüstü / çekirdek kodu).
pub const NO_APP: usize = usize::MAX;
pub const NO_WINDOW: usize = usize::MAX;

/// İç içe koruma (ör. görev çağrısı) için azami derinlik.
const MAX_GUARD_DEPTH: usize = 4;

/// `guard_begin_naked` tarafından doldurulan, kurtarma anında geri yüklenen
/// çekirdek bağlamı. Alan sırası asm koduyla birebir aynıdır.
#[repr(C)]
#[derive(Clone, Copy)]
struct GuardContext {
    rsp: u64,
    rbp: u64,
    rbx: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rflags: u64,
    app_id: u64,
    window_idx: u64,
    valid: u64,
    recoverable: u64,
}

impl GuardContext {
    const EMPTY: Self = Self {
        rsp: 0,
        rbp: 0,
        rbx: 0,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
        rflags: 0,
        app_id: NO_APP as u64,
        window_idx: NO_WINDOW as u64,
        valid: 0,
        recoverable: 0,
    };
}

static mut CONTEXTS: [[GuardContext; MAX_GUARD_DEPTH]; MAX_CPUS] =
    [[GuardContext::EMPTY; MAX_GUARD_DEPTH]; MAX_CPUS];
static mut DEPTHS: [usize; MAX_CPUS] = [0; MAX_CPUS];
static ACTIVE_GUARDS: AtomicUsize = AtomicUsize::new(0);

/// Hata türü; seri günlüğü ve bildirim metni için kullanılır.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    PageFault,
    GeneralProtection,
    DivideByZero,
    InvalidOpcode,
    StackFault,
    Panic,
    Exception,
}

impl FaultKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PageFault => "#PF page fault",
            Self::GeneralProtection => "#GP general protection",
            Self::DivideByZero => "#DE divide error",
            Self::InvalidOpcode => "#UD invalid opcode",
            Self::StackFault => "#SS stack fault",
            Self::Panic => "panic",
            Self::Exception => "exception",
        }
    }
}

/// Kurtarılan hata hakkında, normal bağlama dönüldükten sonra raporlanacak bilgi.
#[derive(Clone, Copy)]
pub struct FaultInfo {
    pub app_id: usize,
    pub window_idx: usize,
    pub kind: FaultKind,
    pub vector: u32,
    pub error_code: u64,
    pub rip: u64,
    pub cr2: u64,
    pub file: &'static str,
    pub line: u32,
}

impl FaultInfo {
    const EMPTY: Self = Self {
        app_id: NO_APP,
        window_idx: NO_WINDOW,
        kind: FaultKind::Exception,
        vector: 0,
        error_code: 0,
        rip: 0,
        cr2: 0,
        file: "",
        line: 0,
    };

    pub fn is_app_fault(self) -> bool {
        self.app_id != NO_APP
    }
}

static mut PENDING: [FaultInfo; MAX_CPUS] = [FaultInfo::EMPTY; MAX_CPUS];
static mut PENDING_VALID: [bool; MAX_CPUS] = [false; MAX_CPUS];

/// Korunan blok. `Drop`, normal çıkışlarda (continue/break dahil) bağlamı temizler.
/// Hata durumunda `Drop` çalışmaz; bağlamı kesici işleyicisi temizler.
pub struct GuardToken {
    cpu: usize,
    depth: usize,
}

impl Drop for GuardToken {
    fn drop(&mut self) {
        if self.depth == usize::MAX {
            return;
        }
        unsafe {
            if DEPTHS[self.cpu] == self.depth + 1 {
                let contexts = core::ptr::addr_of_mut!(CONTEXTS);
                (*contexts)[self.cpu][self.depth].valid = 0;
                DEPTHS[self.cpu] = self.depth;
                ACTIVE_GUARDS.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

/// Uygulama kodunun geçerli bağlamını değiştirir (render / olay işleme sırasında).
/// Korunan blok yoksa etkisizdir.
pub fn set_context(app_id: usize, window_idx: usize) {
    if ACTIVE_GUARDS.load(Ordering::Acquire) == 0 {
        return;
    }
    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        let depth = DEPTHS[cpu];
        if depth == 0 {
            return;
        }
        let contexts = core::ptr::addr_of_mut!(CONTEXTS);
        let ctx = &mut (*contexts)[cpu][depth - 1];
        if ctx.valid == 0 {
            return;
        }
        ctx.app_id = app_id as u64;
        ctx.window_idx = window_idx as u64;
        ctx.recoverable = u64::from(app_id != NO_APP);
    }
}

fn current_context() -> (usize, usize) {
    if ACTIVE_GUARDS.load(Ordering::Acquire) == 0 {
        return (NO_APP, NO_WINDOW);
    }
    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        let depth = DEPTHS[cpu];
        if depth == 0 {
            return (NO_APP, NO_WINDOW);
        }
        let contexts = core::ptr::addr_of!(CONTEXTS);
        let ctx = &(*contexts)[cpu][depth - 1];
        (ctx.app_id as usize, ctx.window_idx as usize)
    }
}

/// Bir uygulama bağlamı kurar ve blok bitince önceki bağlama döner.
pub struct ContextScope {
    previous: (usize, usize),
}

impl ContextScope {
    pub fn new(app_id: usize, window_idx: usize) -> Self {
        let previous = current_context();
        set_context(app_id, window_idx);
        Self { previous }
    }
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        set_context(self.previous.0, self.previous.1);
    }
}

/// Uygulama kodu için korumalı bölge açar. Dönüş değeri `None` ise bölge
/// çalıştırılmamalıdır: bir hata yakalanmış ve `take_pending` ile raporlanmayı
/// bekliyordur.
#[inline(never)]
pub fn enter(app_id: usize, window_idx: usize) -> Option<GuardToken> {
    enter_inner(app_id, window_idx, app_id != NO_APP)
}

/// Uygulama dışındaki (ör. görev zamanlayıcısı) kodu da korur.
#[inline(never)]
pub fn enter_recoverable(app_id: usize, window_idx: usize) -> Option<GuardToken> {
    enter_inner(app_id, window_idx, true)
}

#[inline(never)]
fn enter_inner(app_id: usize, window_idx: usize, recoverable: bool) -> Option<GuardToken> {
    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        let depth = DEPTHS[cpu];
        if depth >= MAX_GUARD_DEPTH {
            return Some(GuardToken {
                cpu,
                depth: usize::MAX,
            });
        }

        let contexts = core::ptr::addr_of_mut!(CONTEXTS);
        let ctx = &mut (*contexts)[cpu][depth];
        ctx.app_id = app_id as u64;
        ctx.window_idx = window_idx as u64;
        ctx.recoverable = u64::from(recoverable);
        // `valid` alanı, begin_naked yığını kaydettikten sonra kurulur.
        DEPTHS[cpu] = depth + 1;
        ACTIVE_GUARDS.fetch_add(1, Ordering::AcqRel);

        let result = guard_begin_naked(ctx as *mut GuardContext, app_id, window_idx);
        if result == 0 {
            Some(GuardToken { cpu, depth })
        } else {
            None
        }
    }
}

/// Bekleyen bir hata var mı? (Aynı CPU üzerinde çağrılmalıdır.)
pub fn has_pending() -> bool {
    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe { PENDING_VALID[cpu] }
}

/// Bekleyen hata bilgisini alır ve temizler.
pub fn take_pending() -> Option<FaultInfo> {
    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        if !PENDING_VALID[cpu] {
            return None;
        }
        PENDING_VALID[cpu] = false;
        let pending = core::ptr::addr_of!(PENDING);
        Some((*pending)[cpu])
    }
}

#[inline(always)]
fn read_cr2() -> u64 {
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }
    cr2
}

fn kind_for_vector(vector: u32) -> FaultKind {
    match vector {
        0 => FaultKind::DivideByZero,
        6 => FaultKind::InvalidOpcode,
        12 => FaultKind::StackFault,
        13 => FaultKind::GeneralProtection,
        14 => FaultKind::PageFault,
        _ => FaultKind::Exception,
    }
}

/// Kernel modunda oluşan bir istisnayı, aktif koruma varsa kurtarır.
///
/// `frame`, `ring3.rs` içindeki hata çerçevesidir. Başarıda çerçeve, kurtarma
/// trampolinine dönecek şekilde yeniden yazılır ve `true` döner.
///
/// # Safety
/// `frame` geçerli bir kesici çerçevesini göstermelidir.
pub unsafe fn try_recover_exception(frame: *mut u64, error_code: u64, vector: u32) -> bool {
    if ACTIVE_GUARDS.load(Ordering::Acquire) == 0 {
        return false;
    }
    // Çift hata IST yığınında oluşur; kurtarma güvenli değildir.
    if vector == 8 {
        return false;
    }

    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        let depth = DEPTHS[cpu];
        if depth == 0 {
            return false;
        }
        let index = depth - 1;
        let contexts = core::ptr::addr_of_mut!(CONTEXTS);
        let ctx = &mut (*contexts)[cpu][index];
        if ctx.valid != 1 || ctx.recoverable == 0 {
            return false;
        }

        let rip = core::ptr::read_volatile(frame.add(EF_RIP));
        let cr2 = if vector == 14 { read_cr2() } else { 0 };
        let pending = core::ptr::addr_of_mut!(PENDING);
        (*pending)[cpu] = FaultInfo {
            app_id: ctx.app_id as usize,
            window_idx: ctx.window_idx as usize,
            kind: kind_for_vector(vector),
            vector,
            error_code,
            rip,
            cr2,
            file: "",
            line: 0,
        };
        PENDING_VALID[cpu] = true;

        // Bu koruma seviyesini kapat; dış seviyeler (varsa) açık kalır.
        ctx.valid = 0;
        DEPTHS[cpu] = index;
        ACTIVE_GUARDS.fetch_sub(1, Ordering::AcqRel);

        let ctx_ptr = ctx as *mut GuardContext as u64;
        core::ptr::write_volatile(
            frame.add(EF_RIP),
            guard_recover_naked as *const () as u64,
        );
        core::ptr::write_volatile(frame.add(EF_CS), gdt::SEL_KCODE as u64);
        core::ptr::write_volatile(
            frame.add(EF_RFLAGS),
            (ctx.rflags & !0x400) | 0x202, // DF=0, IF=1, bit1=1
        );
        // Kesici epilogu r15'i bu değerle yükler; kurtarma trampolini
        // bağlam işaretçisini buradan alır (CPU'lar arası yarış yok).
        core::ptr::write_volatile(frame.add(F_R15), ctx_ptr);
    }
    true
}

/// Panic sırasında aktif koruma varsa aynı kurtarma yolunu kullanır.
/// `true` dönerse panic işleyicisi normal raporlamaya devam etmemelidir;
/// bu durumda fonksiyon aslında geri dönmez.
pub fn try_recover_panic(info: &core::panic::PanicInfo) -> bool {
    if ACTIVE_GUARDS.load(Ordering::Acquire) == 0 {
        return false;
    }

    let cpu = current_cpu_index().min(MAX_CPUS - 1);
    unsafe {
        let depth = DEPTHS[cpu];
        if depth == 0 {
            return false;
        }
        let index = depth - 1;
        let contexts = core::ptr::addr_of_mut!(CONTEXTS);
        let ctx = &mut (*contexts)[cpu][index];
        if ctx.valid != 1 || ctx.recoverable == 0 {
            return false;
        }

        let location = info.location();
        let pending = core::ptr::addr_of_mut!(PENDING);
        (*pending)[cpu] = FaultInfo {
            app_id: ctx.app_id as usize,
            window_idx: ctx.window_idx as usize,
            kind: FaultKind::Panic,
            vector: 0,
            error_code: 0,
            rip: 0,
            cr2: 0,
            file: location.map_or("", |loc| loc.file()),
            line: location.map_or(0, |loc| loc.line()),
        };
        PENDING_VALID[cpu] = true;

        ctx.valid = 0;
        DEPTHS[cpu] = index;
        ACTIVE_GUARDS.fetch_sub(1, Ordering::AcqRel);

        guard_panic_recover_naked(ctx as *mut GuardContext)
    }
}

/// `enter` tarafından çağrılır. Yazmaçları ve yığın işaretçisini kaydeder.
/// Dönüş: 0 = normal giriş, 1 = hata sonrası kurtarma.
#[no_mangle]
#[unsafe(naked)]
unsafe extern "C" fn guard_begin_naked(
    _ctx: *mut GuardContext,
    _app_id: usize,
    _window_idx: usize,
) -> u64 {
    core::arch::naked_asm!(
        // rdi = ctx, rsi = app_id, rdx = window_idx
        "mov [rdi + 0], rsp",
        "mov [rdi + 8], rbp",
        "mov [rdi + 16], rbx",
        "mov [rdi + 24], r12",
        "mov [rdi + 32], r13",
        "mov [rdi + 40], r14",
        "mov [rdi + 48], r15",
        "pushfq",
        "pop rax",
        "mov [rdi + 56], rax",
        "mov [rdi + 64], rsi",
        "mov [rdi + 72], rdx",
        "mov qword ptr [rdi + 80], 1",
        "xor eax, eax",
        "ret",
    )
}

/// İstisna çerçevesinden dönüş trampolini. R15, `GuardContext` işaretçisini
/// taşır (kesici epilogu tarafından yüklenir). Yığını geri yükler, `enter`
/// çağrısına 1 döndürür.
#[no_mangle]
#[unsafe(naked)]
unsafe extern "C" fn guard_recover_naked() -> ! {
    core::arch::naked_asm!(
        "mov rax, r15",
        "mov rsp, [rax + 0]",
        "mov rbp, [rax + 8]",
        "mov rbx, [rax + 16]",
        "mov r12, [rax + 24]",
        "mov r13, [rax + 32]",
        "mov r14, [rax + 40]",
        "mov r15, [rax + 48]",
        "mov rax, 1",
        "ret",
    )
}

/// Panic işleyicisinden doğrudan kurtarma trampolini (rdi = GuardContext).
#[no_mangle]
#[unsafe(naked)]
unsafe extern "C" fn guard_panic_recover_naked(_ctx: *mut GuardContext) -> ! {
    core::arch::naked_asm!(
        "mov rsp, [rdi + 0]",
        "mov rbp, [rdi + 8]",
        "mov rbx, [rdi + 16]",
        "mov r12, [rdi + 24]",
        "mov r13, [rdi + 32]",
        "mov r14, [rdi + 40]",
        "mov r15, [rdi + 48]",
        "mov rax, 1",
        "ret",
    )
}

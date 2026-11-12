//! Process & System Call Management / Surec ve Sistem cagrilari Yonetimi.
//!
//! This module:
//! 1. Process Control Block (PCB),
//! 2. Process states (Ready, Running, Blocked, Terminated),
//! 3. x86_64 Register/Context structure and process memory/resource tracking,
//! 4. Process Table: Process spawn/exec, terminate (exit/kill),
//! 5. System calls (Syscalls): `sys_yield`, `sys_exit`, `sys_write`, `sys_read`,
//!    `sys_alloc_frame`, `sys_free_frame`, `sys_get_pid`, `sys_time`, etc.
//! 6. Separate virtual address space (AddressSpace / Page Table) management per process.
//!
//! Bu modul:
//! 1. Surec Kontrol Blogu (PCB - Process Control Block),
//! 2. Surec durumlari (Hazir, calisiyor, Uyuyor, Sonlandi),
//! 3. x86_64 Register/Context yapisi ve surec bellek/kaynak takibi,
//! 4. Surec tablosu (Process Table): Surec baslatma (spawn/exec), sonlandirma (exit/kill),
//! 5. Sistem cagrilari (Syscalls): `sys_yield`, `sys_exit`, `sys_write`, `sys_read`,
//!    `sys_alloc_frame`, `sys_free_frame`, `sys_get_pid`, `sys_time` vb.
//! 6. Her surec icin ayri sanal adres uzayi (AddressSpace / Page Table) yonetimi.

use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::AddressSpace;

/// Azami eszamanli aktif surec sayisi
pub const MAX_PROCESSES: usize = 32;

/// Surec Durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Unused,
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// Surec onceligi
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

/// CPU Yazmac/Baglam Durumu (Saved Context on Interrupt/Syscall)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr3: u64,
}

impl ProcessContext {
    /// Mevcut CPU bağlamını kaydeder ve bu yapıya doldurur.
    /// Genellikle interrupt veya syscall sırasında çağrılır.
    pub unsafe fn save_current() -> Self {
        let mut ctx = Self::default();
        core::arch::asm!(
            "mov {rax}, rax",
            "mov {rbx}, rbx",
            "mov {rcx}, rcx",
            "mov {rdx}, rdx",
            "mov {rsi}, rsi",
            "mov {rdi}, rdi",
            "mov {rbp}, rbp",
            "mov {rsp}, rsp",
            "mov {r8}, r8",
            "mov {r9}, r9",
            "mov {r10}, r10",
            "mov {r11}, r11",
            "mov {r12}, r12",
            "mov {r13}, r13",
            "mov {r14}, r14",
            "mov {r15}, r15",
            "lea {rip}, [rip + 1]", // Sonraki instruction adresi
            "pushfq",
            "pop {rflags}",
            "mov {cr3}, cr3",
            rax = out(reg) ctx.rax,
            rbx = out(reg) ctx.rbx,
            rcx = out(reg) ctx.rcx,
            rdx = out(reg) ctx.rdx,
            rsi = out(reg) ctx.rsi,
            rdi = out(reg) ctx.rdi,
            rbp = out(reg) ctx.rbp,
            rsp = out(reg) ctx.rsp,
            r8 = out(reg) ctx.r8,
            r9 = out(reg) ctx.r9,
            r10 = out(reg) ctx.r10,
            r11 = out(reg) ctx.r11,
            r12 = out(reg) ctx.r12,
            r13 = out(reg) ctx.r13,
            r14 = out(reg) ctx.r14,
            r15 = out(reg) ctx.r15,
            rip = out(reg) ctx.rip,
            rflags = out(reg) ctx.rflags,
            cr3 = out(reg) ctx.cr3,
            options(nostack, preserves_flags),
        );
        ctx
    }

    /// Bu bağlamı CPU'ya geri yükler ve süreci devam ettirir.
    /// CR3 değişikliği ve register geri yükleme içerir.
    pub unsafe fn restore(&self) {
        core::arch::asm!(
            "mov cr3, {cr3}",
            "mov rax, {rax}",
            "mov rbx, {rbx}",
            "mov rcx, {rcx}",
            "mov rdx, {rdx}",
            "mov rsi, {rsi}",
            "mov rdi, {rdi}",
            "mov rbp, {rbp}",
            "mov rsp, {rsp}",
            "mov r8, {r8}",
            "mov r9, {r9}",
            "mov r10, {r10}",
            "mov r11, {r11}",
            "mov r12, {r12}",
            "mov r13, {r13}",
            "mov r14, {r14}",
            "mov r15, {r15}",
            "push {rflags}",
            "popfq",
            "jmp {rip}",
            cr3 = in(reg) self.cr3,
            rax = in(reg) self.rax,
            rbx = in(reg) self.rbx,
            rcx = in(reg) self.rcx,
            rdx = in(reg) self.rdx,
            rsi = in(reg) self.rsi,
            rdi = in(reg) self.rdi,
            rbp = in(reg) self.rbp,
            rsp = in(reg) self.rsp,
            r8 = in(reg) self.r8,
            r9 = in(reg) self.r9,
            r10 = in(reg) self.r10,
            r11 = in(reg) self.r11,
            r12 = in(reg) self.r12,
            r13 = in(reg) self.r13,
            r14 = in(reg) self.r14,
            r15 = in(reg) self.r15,
            rflags = in(reg) self.rflags,
            rip = in(reg) self.rip,
            options(noreturn, nostack),
        );
    }
}

/// Bir surece tahsis edilen kaynaklar (Sayfa cerceveleri, dosya taniticilari vb.)
pub const MAX_PROC_FRAMES: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct ProcessResources {
    /// Surecin sahip oldugu tahsis edilmis fiziksel sayfa cerceveleri
    pub allocated_frames: [u64; MAX_PROC_FRAMES],
    pub frame_count: usize,
    /// Surec tarafindan acilan dosya veya pencere taniticisi (Handle)
    pub window_idx: Option<usize>,
    pub app_id: usize,
}

impl ProcessResources {
    pub const fn empty() -> Self {
        Self {
            allocated_frames: [0; MAX_PROC_FRAMES],
            frame_count: 0,
            window_idx: None,
            app_id: 0,
        }
    }

    pub fn record_frame(&mut self, frame: u64) -> bool {
        if self.frame_count < MAX_PROC_FRAMES {
            self.allocated_frames[self.frame_count] = frame;
            self.frame_count += 1;
            true
        } else {
            false
        }
    }
}

/// Surec Kontrol Blogu (PCB - Process Control Block)
#[derive(Debug, Clone, Copy)]
pub struct Process {
    pub pid: usize,
    pub ppid: usize,
    pub name: [u8; 32],
    pub name_len: usize,
    pub state: ProcessState,
    pub priority: ProcessPriority,
    pub context: ProcessContext,
    pub resources: ProcessResources,
    pub address_space: AddressSpace,
    pub cpu_time_ticks: u64,
    pub exit_code: i32,
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            pid: 0,
            ppid: 0,
            name: [0; 32],
            name_len: 0,
            state: ProcessState::Unused,
            priority: ProcessPriority::Normal,
            context: ProcessContext {
                rax: 0,
                rbx: 0,
                rcx: 0,
                rdx: 0,
                rsi: 0,
                rdi: 0,
                rbp: 0,
                rsp: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
                rip: 0,
                rflags: 0x202,
                cr3: 0,
            },
            resources: ProcessResources::empty(),
            address_space: AddressSpace { pml4_phys: 0 },
            cpu_time_ticks: 0,
            exit_code: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("unknown")
    }
}

/// Surec Tablosu ve Zamanlayici Yoneticisi
pub struct ProcessManager {
    pub processes: [Process; MAX_PROCESSES],
    pub current_pid: usize,
    pub next_pid: usize,
    pub active_count: usize,
}

impl ProcessManager {
    pub const fn new() -> Self {
        Self {
            processes: [Process::empty(); MAX_PROCESSES],
            current_pid: 0,
            next_pid: 1,
            active_count: 0,
        }
    }

    /// Initialize kernel process (PID 0 / init / kernel_core).
    /// cekirdek baslangic surecini (PID 0 / init / kernel_core) kaydeder.
    pub fn init_kernel_process(&mut self) {
        let mut p = Process::empty();
        p.pid = 0;
        p.ppid = 0;
        let name = b"kernel_core";
        p.name[..name.len()].copy_from_slice(name);
        p.name_len = name.len();
        p.state = ProcessState::Running;
        p.priority = ProcessPriority::Realtime;
        // Kernel sureci mevcut adres uzayini kullanir
        p.address_space = AddressSpace::current();
        self.processes[0] = p;
        self.current_pid = 0;
        self.active_count = 1;
    }

    /// Spawn a new user or system application into the process table.
    /// Creates a separate virtual address space (page table) and performs necessary mappings.
    /// Yeni bir kullanici veya sistem uygulamasini surec tablosuna ekler (Spawn).
    /// Ayrı bir sanal adres uzayı (page table) oluşturur ve gerekli eşlemeleri yapar.
    pub fn spawn(
        &mut self,
        name: &str,
        entry_point: u64,
        stack_top: u64,
        app_id: usize,
        window_idx: Option<usize>,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<usize, &'static str> {
        let slot = self
            .processes
            .iter()
            .position(|p| p.state == ProcessState::Unused || p.state == ProcessState::Terminated)
            .ok_or("Surec tablosu dolu: Yeni surec baslatilamadi")?;

        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);

        // Yeni bir PML4 kök tablosu tahsis et (yeni adres uzayı için)
        let pml4_frame = pmm
            .allocate_frame()
            .ok_or("PMM yetersiz bellek: PML4 tablosu tahsis edilemedi")?;

        // PML4'ü sıfırla
        unsafe {
            let pml4_ptr = crate::drivers::phys_to_virt(pml4_frame) as *mut u8;
            core::ptr::write_bytes(pml4_ptr, 0, 4096);
        }

        let mut address_space = AddressSpace {
            pml4_phys: pml4_frame,
        };

        // Kernel mapping'leri yeni adres uzayına kopyala (tam izolasyon için gerekli)
        // Kernel eşlemesi kullanıcı adres uzayına kopyalanmalı (tam izolasyon için).
        if let Err(e) = address_space.clone_kernel_mappings(pmm) {
            // Hata durumunda tahsis edilen frame'i iade et
            pmm.free_frame(pml4_frame);
            return Err(e);
        }

        let mut p = Process::empty();
        p.pid = pid;
        p.ppid = self.current_pid;
        let n = name.len().min(32);
        p.name[..n].copy_from_slice(&name.as_bytes()[..n]);
        p.name_len = n;
        p.state = ProcessState::Ready;
        p.priority = ProcessPriority::Normal;

        // Baglami hazirla
        p.context.rip = entry_point;
        p.context.rsp = stack_top;
        p.context.cr3 = pml4_frame; // Yeni CR3 değeri
        p.context.rflags = 0x202; // IF = 1 (Kesmeler acik)

        p.resources.app_id = app_id;
        p.resources.window_idx = window_idx;
        p.address_space = address_space;

        // Yeni PML4'ü kaynağa ekle
        p.resources.record_frame(pml4_frame);

        self.processes[slot] = p;
        self.active_count += 1;

        Ok(pid)
    }

    /// Terminate a process by PID and free its resources.
    /// Belirtilen PID'ye sahip sureci sonlandirir (Kill/Terminate) ve kaynaklarini serbest birakir.
    pub fn terminate(
        &mut self,
        pid: usize,
        exit_code: i32,
        pmm: &mut PhysicalMemoryManager,
    ) -> bool {
        if pid == 0 {
            // cekirdek ana sureci sonlandirilamaz!
            return false;
        }

        if let Some(p) = self.processes.iter_mut().find(|p| {
            p.pid == pid && p.state != ProcessState::Unused && p.state != ProcessState::Terminated
        }) {
            p.state = ProcessState::Terminated;
            p.exit_code = exit_code;

            // 1. Surecin sahip oldugu tum fiziksel sayfa cercevelerini PMM'e iade et
            for i in 0..p.resources.frame_count {
                let frame = p.resources.allocated_frames[i];
                if frame != 0 {
                    pmm.free_frame(frame);
                }
            }
            p.resources.frame_count = 0;

            // 2. Adres uzayini temizle (süreç sayfa tabloları zaten frame listesinde)
            // PML4 ve diğer tablolar yukarıdaki döngüde serbest bırakıldı

            self.active_count = self.active_count.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Find and terminate a running process by window index.
    /// Pencere indeksine gore calisan sureci bulur ve sonlandirir.
    pub fn terminate_by_window(&mut self, win_idx: usize, pmm: &mut PhysicalMemoryManager) -> bool {
        let target_pid = self
            .processes
            .iter()
            .find(|p| {
                p.state != ProcessState::Unused
                    && p.state != ProcessState::Terminated
                    && p.resources.window_idx == Some(win_idx)
            })
            .map(|p| p.pid);

        if let Some(pid) = target_pid {
            self.terminate(pid, 0, pmm)
        } else {
            false
        }
    }

    /// Mevcut çalışan süreci fault sonrası temizler (Ring 3 fault cleanup).
    /// Sürecin kaynaklarını serbest bırakır ve durumunu Terminated olarak işaretler.
    pub fn cleanup_faulted_process(&mut self, pmm: &mut PhysicalMemoryManager) -> bool {
        let current = self.current_pid;
        if current == 0 {
            // Kernel süreci temizlenemez
            return false;
        }

        if let Some(p) = self.processes.iter_mut().find(|p| p.pid == current) {
            p.state = ProcessState::Terminated;
            p.exit_code = -11; // SIGSEGV

            // Kaynakları temizle
            for i in 0..p.resources.frame_count {
                let frame = p.resources.allocated_frames[i];
                if frame != 0 {
                    pmm.free_frame(frame);
                }
            }
            p.resources.frame_count = 0;

            self.active_count = self.active_count.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Bir sonraki calistirilabilir sureci doner (oncelikli Preemptive Round-Robin Zamanlayici).
    /// Kullanici sureclerini oncelik sirasina (Realtime > High > Normal > Low) gore zamanlar,
    /// calistirilabilir kullanici sureci yoksa PID 0 (kernel_core) fallback doner.
    pub fn schedule_next(&mut self) -> Option<usize> {
        if self.active_count == 0 {
            return None;
        }

        // Mevcut calisan surecin slot indeksini bul
        let curr_slot = self
            .processes
            .iter()
            .position(|p| p.pid == self.current_pid && p.state != ProcessState::Unused)
            .unwrap_or(0);

        // 1. once calistirilmaya hazir kullanici sureclerini oncelik sirasiyla ara (Realtime -> Low)
        for target_prio in [
            ProcessPriority::Realtime,
            ProcessPriority::High,
            ProcessPriority::Normal,
            ProcessPriority::Low,
        ] {
            let total = MAX_PROCESSES;
            for i in 1..=total {
                let slot = (curr_slot + i) % total;
                let p = &mut self.processes[slot];
                if p.pid != 0
                    && (p.state == ProcessState::Ready || p.state == ProcessState::Running)
                    && p.priority == target_prio
                {
                    self.current_pid = p.pid;
                    p.state = ProcessState::Running;
                    p.cpu_time_ticks = p.cpu_time_ticks.saturating_add(1);
                    return Some(p.pid);
                }
            }
        }

        // 2. Baska calistirilabilir kullanici sureci yoksa cekirdek ana surecine (PID 0) don
        if self.processes[0].state == ProcessState::Running
            || self.processes[0].state == ProcessState::Ready
        {
            self.current_pid = 0;
            self.processes[0].state = ProcessState::Running;
            self.processes[0].cpu_time_ticks = self.processes[0].cpu_time_ticks.saturating_add(1);
            return Some(0);
        }

        Some(0) // kernel_core fallback
    }

    /// Get process reference by PID.
    /// PID ile surec referansi alir.
    pub fn get_process(&self, pid: usize) -> Option<&Process> {
        self.processes
            .iter()
            .find(|p| p.pid == pid && p.state != ProcessState::Unused)
    }

    /// Activate the specified process's address space (switch CR3).
    /// This function should be called during context switch.
    /// Belirtilen surecin adres uzayını aktif eder (CR3 değiştirir).
    /// Bu fonksiyon context switch sırasında çağrılmalıdır.
    pub fn switch_address_space(&self, pid: usize) {
        if let Some(p) = self.get_process(pid) {
            if p.address_space.pml4_phys != 0 {
                unsafe {
                    // CR3'ü yükle (adres uzayını değiştir)
                    core::arch::asm!("mov cr3, {}", in(reg) p.address_space.pml4_phys, options(nostack, preserves_flags));
                }
            }
        }
    }

    /// Mevcut sürecin bağlamını kaydeder ve yeni sürece geçiş yapar (context switch).
    /// Şu an basit bir implementasyon: sadece CR3 değiştirir ve context'i günceller.
    /// Tam preemptive scheduling için interrupt handler'dan çağrılmalıdır.
    pub fn context_switch(&mut self, next_pid: usize) {
        let current_pid = self.current_pid;

        // Mevcut sürecin bağlamını kaydet (gerekirse)
        if let Some(curr_p) = self.processes.iter_mut().find(|p| p.pid == current_pid) {
            if curr_p.state == ProcessState::Running {
                unsafe {
                    curr_p.context = ProcessContext::save_current();
                }
                curr_p.state = ProcessState::Ready;
            }
        }

        // Adres uzayını ve bağlamı yeni süreç için hazırla
        let next_pml4 = self
            .get_process(next_pid)
            .map(|p| p.address_space.pml4_phys);
        let next_context = self.get_process(next_pid).map(|p| p.context);

        // Yeni sürece geç
        if let Some(next_p) = self.processes.iter_mut().find(|p| p.pid == next_pid) {
            self.current_pid = next_pid;
            next_p.state = ProcessState::Running;

            // Adres uzayını değiştir (doğrudan CR3)
            if let Some(pml4) = next_pml4 {
                if pml4 != 0 {
                    unsafe {
                        core::arch::asm!("mov cr3, {}", in(reg) pml4, options(nostack, preserves_flags));
                    }
                }
            }

            // Bağlamı geri yükle (şu an sadece CR3 ve basic register'lar)
            if let Some(ctx) = next_context {
                unsafe {
                    ctx.restore();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System Call Interface / Sistem cagrisi (Syscall) Arabirimi
// ---------------------------------------------------------------------------

/// Standard System Call Numbers
/// Standart Sistem cagrisi Numaralari
pub mod sys_nos {
    pub const SYS_YIELD: u64 = 0;
    pub const SYS_EXIT: u64 = 1;
    pub const SYS_GETPID: u64 = 2;
    pub const SYS_ALLOC_FRAME: u64 = 3;
    pub const SYS_FREE_FRAME: u64 = 4;
    pub const SYS_WRITE: u64 = 5;
    pub const SYS_READ: u64 = 6;
    pub const SYS_SPAWN: u64 = 7;
}

/// System call dispatcher.
/// x86_64 ABI:
/// RAX = Syscall No
/// RDI = Arg 1
/// RSI = Arg 2
/// RDX = Arg 3
/// R10 = Arg 4
/// Return value: RAX (Result code / address)
/// Sistem cagrisi sevkedicisi (Syscall Dispatcher).
/// x86_64 ABI:
/// RAX = Syscall No
/// RDI = Arg 1
/// RSI = Arg 2
/// RDX = Arg 3
/// R10 = Arg 4
/// Donen deger: RAX (Sonuc kodu / adres)
pub fn syscall_dispatch(
    sys_no: u64,
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> u64 {
    match sys_no {
        sys_nos::SYS_YIELD => {
            proc_mgr.schedule_next();
            0
        }
        sys_nos::SYS_EXIT => {
            let exit_code = arg1 as i32;
            let current = proc_mgr.current_pid;
            proc_mgr.terminate(current, exit_code, pmm);
            0
        }
        sys_nos::SYS_GETPID => proc_mgr.current_pid as u64,
        sys_nos::SYS_ALLOC_FRAME => {
            if let Some(frame) = pmm.allocate_frame() {
                // Mevcut surecin kaynaklarina kaydet
                let current = proc_mgr.current_pid;
                if let Some(p) = proc_mgr.processes.iter_mut().find(|p| p.pid == current) {
                    p.resources.record_frame(frame);
                }
                frame
            } else {
                0
            }
        }
        sys_nos::SYS_FREE_FRAME => {
            let frame = arg1;
            pmm.free_frame(frame);
            0
        }
        sys_nos::SYS_WRITE => {
            // arg1 = tampon sanal adresi, arg2 = uzunluk
            let ptr = arg1 as *const u8;
            let len = arg2 as usize;
            if !ptr.is_null() && len > 0 && len <= 1024 {
                // Guvenli tampon kopyalama ve seriye yazdirma
                let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::serial::print(s);
                }
            }
            len as u64
        }
        _ => u64::MAX, // Desteklenmeyen cagri
    }
}

/// Test process lifecycle, crash/termination, invalid syscalls, and resource cleanup.
/// Surec yasam dongusu, kaza (crash/termination), gecersiz sistem cagrisi ve kaynak temizleme testi.
pub fn test_process_lifecycle_and_fault_recovery(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print(
        "[PROC-TEST] --- Süreç çökmesi, syscall hataları ve kaynak temizleme testi ---\n",
    );

    let initial_free = pmm.free_memory_bytes;
    let initial_active = proc_mgr.active_count;

    // 1. Yeni bir test sureci baslat (spawn)
    let pid = match proc_mgr.spawn("fault_test_proc", 0x1000, 0x7000_2000, 99, Some(99), pmm) {
        Ok(id) => id,
        Err(_) => {
            crate::serial_print("[PROC-TEST] Süreç başlatma: BAŞARISIZ\n");
            return false;
        }
    };
    crate::serial_print("[PROC-TEST] 1. Test süreci başlatıldı (PID ");
    crate::serial::print_dec(pid as u64);
    crate::serial_print("): BAŞARILI\n");

    // 2. Surece kaynak tahsis et (PMM cerceveleri)
    let frame1 = match pmm.allocate_frame() {
        Some(f) => f,
        None => return false,
    };
    let frame2 = match pmm.allocate_frame() {
        Some(f) => f,
        None => {
            pmm.free_frame(frame1);
            return false;
        }
    };
    if let Some(p) = proc_mgr.processes.iter_mut().find(|p| p.pid == pid) {
        p.resources.record_frame(frame1);
        p.resources.record_frame(frame2);
    }
    crate::serial_print(
        "[PROC-TEST] 2. Sürece 2 sayfa çerçevesi tahsis edildi (Kaynak takibi aktif)\n",
    );

    // 3. Hatali / Gecersiz Sistem cagrisi Testi (Gecersiz Syscall No)
    let invalid_res = syscall_dispatch(9999, 0, 0, 0, proc_mgr, pmm);
    let null_ptr_write = syscall_dispatch(sys_nos::SYS_WRITE, 0, 100, 0, proc_mgr, pmm);
    if invalid_res == u64::MAX && null_ptr_write == 100 {
        crate::serial_print(
            "[PROC-TEST] 3. Geçersiz syscall numarası ve NULL pointer sınır kontrolü: BAŞARILI\n",
        );
    } else {
        crate::serial_print("[PROC-TEST] Hatalı sistem çağrısı sınır koruması: BAŞARISIZ\n");
        return false;
    }

    // 4. Surec cokmesi / Hata Simulasyonu (SIGSEGV / Fault Exit Code -11)
    let terminated = proc_mgr.terminate(pid, -11, pmm);
    if terminated {
        crate::serial_print(
            "[PROC-TEST] 4. Süreç çökmesi simüle edildi (Exit Code: -11 / SIGSEGV)\n",
        );
    } else {
        crate::serial_print("[PROC-TEST] Süreç sonlandırma: BAŞARISIZ\n");
        return false;
    }

    // 5. Kaynak Temizligi ve Bellek Sizintisi Kontrolu
    let free_after = pmm.free_memory_bytes;
    let active_after = proc_mgr.active_count;

    if free_after != initial_free || active_after != initial_active {
        crate::serial_print("[PROC-TEST] Kaynak temizleme sızıntısı: BAŞARISIZ\n");
        return false;
    }
    crate::serial_print(
        "[PROC-TEST] 5. Süreç kaynakları PMM'e eksiksiz iade edildi (sızıntı: 0 bayt): BAŞARILI\n",
    );

    // 6. oncelikli Zamanlayici (Priority Preemptive Scheduler) Testi
    let p_low = proc_mgr
        .spawn("low_proc", 0x1000, 0x7000_1000, 1, None, pmm)
        .unwrap_or(0);
    let p_high = proc_mgr
        .spawn("high_proc", 0x2000, 0x7000_2000, 1, None, pmm)
        .unwrap_or(0);
    if let Some(p) = proc_mgr.processes.iter_mut().find(|p| p.pid == p_high) {
        p.priority = ProcessPriority::High;
    }
    if let Some(p) = proc_mgr.processes.iter_mut().find(|p| p.pid == p_low) {
        p.priority = ProcessPriority::Low;
    }

    let next_pid = proc_mgr.schedule_next();
    let sched_ok = next_pid == Some(p_high);
    let _ = proc_mgr.terminate(p_low, 0, pmm);
    let _ = proc_mgr.terminate(p_high, 0, pmm);

    if sched_ok {
        crate::serial_print("[PROC-TEST] 6. öncelik tabanlı zamanlayıcı (High > Low): BAŞARILI\n");
        true
    } else {
        crate::serial_print("[PROC-TEST] 6. Zamanlayıcı öncelik testi: BAŞARISIZ\n");
        false
    }
}

/// Ring 3 adres uzayı izolasyonu ve kernel mapping kopyalama testi.
/// Yeni bir süreç oluşturur, kernel mapping'lerin kopyalandığını doğrular.
pub fn test_ring3_isolation(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print("[RING3-TEST] --- Ring 3 İzolasyon ve Kernel Mapping Testi ---\n");

    let initial_free = pmm.free_memory_bytes;

    // 1. Yeni bir kullanıcı süreci oluştur
    let pid = match proc_mgr.spawn("ring3_test", 0x1000, 0x7000_2000, 100, None, pmm) {
        Ok(id) => id,
        Err(e) => {
            crate::serial_print("[RING3-TEST] Süreç oluşturma hatası: ");
            crate::serial_print(e);
            crate::serial_print("\n");
            return false;
        }
    };
    crate::serial_print("[RING3-TEST] 1. Süreç oluşturuldu (PID ");
    crate::serial::print_dec(pid as u64);
    crate::serial_print("): BAŞARILI\n");

    // 2. Kernel mapping kopyalanmasını ve supervisor-only izinlerini doğrula.
    if let Some(p) = proc_mgr.get_process(pid) {
        let aspace = p.address_space;
        let kernel_cr3 = AddressSpace::current().pml4_phys;
        if aspace.pml4_phys == kernel_cr3 || p.context.cr3 != aspace.pml4_phys {
            crate::serial_print("[RING3-TEST] Ayrı CR3/adres uzayı: BAŞARISIZ\n");
            let _ = proc_mgr.terminate(pid, 0, pmm);
            return false;
        }

        // Kernel mapping'ler PML4 üst yarısında (indeks 256-511) olmalı
        unsafe {
            let pml4 = &*crate::mm::vmm::AddressSpace::table_ptr(aspace.pml4_phys);
            let mut kernel_entries = 0usize;
            let mut user_visible_kernel_entries = 0usize;
            for i in 256..512 {
                if (pml4.entries[i] & crate::mm::vmm::PageFlags::PRESENT) != 0 {
                    kernel_entries += 1;
                    if (pml4.entries[i] & crate::mm::vmm::PageFlags::USER) != 0 {
                        user_visible_kernel_entries += 1;
                    }
                }
            }

            if kernel_entries > 0 && user_visible_kernel_entries == 0 {
                crate::serial_print("[RING3-TEST] 2. Kernel mapping kopyalandı (");
                crate::serial::print_dec(kernel_entries as u64);
                crate::serial_print(" giriş), USER biti yok: BAŞARILI\n");
                crate::serial_print("[P1] CR3 ayri ve kernel mapping Ring3'e kapali: BASARILI\n");
            } else {
                crate::serial_print(
                    "[RING3-TEST] Kernel mapping USER erişimine açık veya yok: BAŞARISIZ\n",
                );
                let _ = proc_mgr.terminate(pid, 0, pmm);
                return false;
            }
        }
    }

    // 3. Süreci temizle ve kaynak iadesini kontrol et
    let terminated = proc_mgr.terminate(pid, 0, pmm);
    if !terminated {
        crate::serial_print("[RING3-TEST] 3. Süreç sonlandırma: BAŞARISIZ\n");
        return false;
    }

    let free_after = pmm.free_memory_bytes;
    if free_after != initial_free {
        crate::serial_print("[RING3-TEST] 3. Kaynak temizleme sızıntısı: BAŞARISIZ\n");
        return false;
    }

    crate::serial_print("[RING3-TEST] 3. Kaynak temizleme sızıntısız: BAŞARILI\n");
    crate::serial_print("[P1] process kaynak temizligi: BASARILI\n");
    true
}

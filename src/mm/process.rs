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

use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::vmm::{AddressSpace, PageFlags};

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
    pub cs: u64,
    pub rflags: u64,
    pub ss: u64,
    pub cr3: u64,
    /// Sürecin kendi çekirdek yığını üzerinde, kayıtlı yazmaç alanının
    /// başlangıcını gösteren RSP değeri. Timer/syscall kesmesinin hazırladığı
    /// `frame` işaretçisi buraya yazılır; `resume_user_frame` bu adresten
    /// yazmaçları ve `iretq` çerçevesini geri yükler.
    pub kernel_rsp: u64,
}

impl ProcessContext {
    /// Mevcut CPU bağlamını kaydeder ve bu yapıya doldurur.
    /// Genellikle interrupt veya syscall sırasında çağrılır.
    ///
    /// # Safety
    /// Kesme veya syscall bağlamında çağrılmalıdır; `options(nostack)`
    /// kullandığından yığın gereksinimi düşüktür ancak bağlamın tutarlı
    /// olması çağıranın sorumluluğundadır.
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
    ///
    /// # Safety
    /// `self` geçerli bir `ProcessContext` olmalıdır; `cr3` alanı geçerli,
    /// eşlenmiş bir PML4 fiziksel adresine işaret etmelidir. Çağrı sonrası
    /// eski yığına dönülemez.
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
pub const MAX_PROC_USER_FRAMES: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct ProcessResources {
    /// Surecin sahip oldugu tahsis edilmis fiziksel sayfa cerceveleri
    pub allocated_frames: [u64; MAX_PROC_FRAMES],
    pub frame_count: usize,
    pub user_allocated_frames: [u64; MAX_PROC_USER_FRAMES],
    pub user_frame_count: usize,
    /// Surec tarafindan acilan dosya veya pencere taniticisi (Handle)
    pub window_idx: Option<usize>,
    pub app_id: usize,
}

impl ProcessResources {
    pub const fn empty() -> Self {
        Self {
            allocated_frames: [0; MAX_PROC_FRAMES],
            frame_count: 0,
            user_allocated_frames: [0; MAX_PROC_USER_FRAMES],
            user_frame_count: 0,
            window_idx: None,
            app_id: 0,
        }
    }

    pub fn record_frame(&mut self, frame: u64) -> bool {
        if self.frame_count >= MAX_PROC_FRAMES
            || self.allocated_frames[..self.frame_count].contains(&frame)
        {
            return false;
        }
        self.allocated_frames[self.frame_count] = frame;
        self.frame_count += 1;
        true
    }

    pub fn take_frame(&mut self, frame: u64) -> bool {
        let Some(index) = self.allocated_frames[..self.frame_count]
            .iter()
            .position(|owned| *owned == frame)
        else {
            return false;
        };
        self.frame_count -= 1;
        self.allocated_frames[index] = self.allocated_frames[self.frame_count];
        self.allocated_frames[self.frame_count] = 0;
        true
    }

    pub fn record_user_frame(&mut self, frame: u64) -> bool {
        if self.user_frame_count >= MAX_PROC_USER_FRAMES || !self.record_frame(frame) {
            return false;
        }
        self.user_allocated_frames[self.user_frame_count] = frame;
        self.user_frame_count += 1;
        true
    }

    pub fn take_user_frame(&mut self, frame: u64) -> bool {
        let Some(index) = self.user_allocated_frames[..self.user_frame_count]
            .iter()
            .position(|owned| *owned == frame)
        else {
            return false;
        };
        if !self.take_frame(frame) {
            return false;
        }
        self.user_frame_count -= 1;
        self.user_allocated_frames[index] = self.user_allocated_frames[self.user_frame_count];
        self.user_allocated_frames[self.user_frame_count] = 0;
        true
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
    /// Sürece özel çekirdek yığını (Ring3→Ring0 geçişlerinde TSS.rsp0).
    /// İlk kullanıcı girişinde tembel (lazy) olarak tahsis edilir.
    pub kstack_phys: u64,
    /// Çekirdek yığınının üst sanal adresi (HHDM üzerinden, 16 bayt hizalı).
    pub kstack_top: u64,
    /// Kaynakları (sayfa çerçeveleri + sayfa tabloları) iade edildi mi?
    pub resources_reaped: bool,
    pub cpu_time_ticks: u64,
    pub exit_code: i32,
    pub has_saved_user_context: bool,
    /// Olay/timeout üzerine engellenen surecin bekledigi olay anahtari (None = calistirilir).
    pub wait_event: Option<usize>,
    /// block_until ile kaydedilen mutlak sonrasi (ms). Dolduysa zamanlayici uyandirir.
    pub wait_deadline: Option<u64>,
    pub waiting_child: Option<usize>,
    pub wait_status_addr: u64,
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
                cs: 0,
                rflags: 0x202,
                ss: 0,
                cr3: 0,
                kernel_rsp: 0,
            },
            resources: ProcessResources::empty(),
            address_space: AddressSpace { pml4_phys: 0 },
            kstack_phys: 0,
            kstack_top: 0,
            resources_reaped: false,
            cpu_time_ticks: 0,
            exit_code: 0,
            has_saved_user_context: false,
            wait_event: None,
            wait_deadline: None,
            waiting_child: None,
            wait_status_addr: 0,
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
    /// Kullanıcı süreçleri için round-robin imleci (çekirdek slotu atlanır).
    user_rr: usize,
}

/// Sürece özel çekirdek yığını sayfa sayısı (4 x 4 KiB = 16 KiB).
pub const KSTACK_PAGES: usize = 4;

impl ProcessManager {
    pub const fn new() -> Self {
        Self {
            processes: [Process::empty(); MAX_PROCESSES],
            current_pid: 0,
            next_pid: 1,
            active_count: 0,
            user_rr: 1,
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
            .position(|p| p.state == ProcessState::Unused)
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
        if !p.resources.record_frame(pml4_frame) {
            pmm.free_frame(pml4_frame);
            return Err("Surec kaynak listesi dolu");
        }

        self.processes[slot] = p;
        if !self.ensure_kernel_stack(pid, pmm) {
            let process = &mut self.processes[slot];
            process.state = ProcessState::Terminated;
            Self::reap_process_resources(process, pmm);
            *process = Process::empty();
            return Err("PMM yetersiz bellek: surec cekirdek yigini tahsis edilemedi");
        }
        self.active_count += 1;

        Ok(pid)
    }

    /// Sürecin kullanıcı sayfa tablolarını (PDPT/PD/PT) ve kayıtlı tüm
    /// fiziksel çerçeveleri (PML4, çekirdek yığını, kullanıcı sayfaları)
    /// PMM'e iade eder. Yalnızca sürecin CR3'ü artık aktif değilken çağrılmalı.
    fn reap_process_resources(p: &mut Process, pmm: &mut PhysicalMemoryManager) {
        if p.resources_reaped {
            return;
        }
        // Önce alt tablolar: PML4 hâlâ geçerliyken çocuk tabloları çöz.
        p.address_space.free_user_page_tables(pmm);
        for &frame in p
            .resources
            .allocated_frames
            .iter()
            .take(p.resources.frame_count)
        {
            if frame != 0 {
                pmm.free_frame(frame);
            }
        }
        p.resources.frame_count = 0;
        p.resources.user_frame_count = 0;
        p.resources_reaped = true;
    }

    /// Terminate a process by PID and free its resources.
    /// Belirtilen PID'ye sahip sureci sonlandirir (Kill/Terminate) ve kaynaklarini serbest birakir.
    /// Çalışan sürecin kendisi için kaynak iadesi `reap_terminated`'e ertelenir;
    /// kendi CR3'ü altındaki sayfa tablolarını yerinde imha etmemek için.
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

        let is_current = pid == self.current_pid;
        let Some(slot) = self.processes.iter().position(|p| {
            p.pid == pid && p.state != ProcessState::Unused && p.state != ProcessState::Terminated
        }) else {
            return false;
        };
        let parent_pid = self.processes[slot].ppid;
        self.processes[slot].state = ProcessState::Terminated;
        self.processes[slot].exit_code = exit_code;
        self.active_count = self.active_count.saturating_sub(1);

        // 1. Kullanıcı alt sayfa tablolarını serbest bırak (PML4 klonları hariç)
        // 2. Sürecin sahip olduğu tüm fiziksel sayfa çerçevelerini PMM'e iade et
        //    (PML4, çekirdek yığını ve kayıtlı kullanıcı sayfaları dahil)
        if !is_current {
            Self::reap_process_resources(&mut self.processes[slot], pmm);
        }
        self.wake_waiting_parent(pid, parent_pid, exit_code);
        true
    }

    /// Süreci sonlandırılmış olarak işaretler; kaynak iadesi `reap_terminated`
    /// çağrısına ertelenir. `int 0x80` (SYS_EXIT) gibi sürecin kendi CR3'ü
    /// altında çalışan bağlamlardan güvenle çağrılabilir.
    pub fn mark_exited(&mut self, pid: usize, exit_code: i32) -> bool {
        if pid == 0 {
            return false;
        }
        let Some(slot) = self.processes.iter().position(|p| {
            p.pid == pid && p.state != ProcessState::Unused && p.state != ProcessState::Terminated
        }) else {
            return false;
        };
        let parent_pid = self.processes[slot].ppid;
        self.processes[slot].state = ProcessState::Terminated;
        self.processes[slot].exit_code = exit_code;
        self.active_count = self.active_count.saturating_sub(1);
        for child in self.processes.iter_mut() {
            if child.ppid == pid && child.state != ProcessState::Unused {
                child.ppid = 0;
            }
        }
        self.wake_waiting_parent(pid, parent_pid, exit_code);
        true
    }

    fn wake_waiting_parent(&mut self, pid: usize, parent_pid: usize, exit_code: i32) {
        let Some(slot) = self.processes.iter().position(|parent| {
            parent.pid == parent_pid
                && parent.state == ProcessState::Blocked
                && parent
                    .waiting_child
                    .is_some_and(|waiting| waiting == 0 || waiting == pid)
        }) else {
            return;
        };
        let parent = &mut self.processes[slot];
        parent.context.rax = pid as u64;
        if parent.wait_status_addr != 0 {
            Self::write_wait_status(parent, parent.wait_status_addr, exit_code);
        }
        parent.state = ProcessState::Ready;
        parent.waiting_child = None;
        parent.wait_status_addr = 0;
    }

    fn write_wait_status(parent: &Process, address: u64, exit_code: i32) -> bool {
        for (offset, byte) in exit_code.to_le_bytes().into_iter().enumerate() {
            let Some(user_address) = address.checked_add(offset as u64) else {
                return false;
            };
            let Some(physical_address) = parent.address_space.translate_user(user_address) else {
                return false;
            };
            let status = crate::drivers::phys_to_virt(physical_address) as *mut u8;
            unsafe { core::ptr::write_volatile(status, byte) };
        }
        true
    }

    /// `Terminated` durumundaki ve kaynakları henüz iade edilmemiş süreçlerin
    /// sayfa çerçevelerini, alt sayfa tablolarını ve çekirdek yığınlarını salar.
    /// Yalnızca çekirdek CR3'ü aktifken (kullanıcı modunda değilken) çağrılmalı.
    pub fn reap_terminated(&mut self, pmm: &mut PhysicalMemoryManager) -> usize {
        let mut reaped = 0usize;
        for p in self.processes.iter_mut() {
            if p.state == ProcessState::Terminated && !p.resources_reaped {
                Self::reap_process_resources(p, pmm);
                reaped += 1;
            }
        }
        reaped
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
            self.active_count = self.active_count.saturating_sub(1);
            Self::reap_process_resources(p, pmm);
            true
        } else {
            false
        }
    }

    /// Sürece özel çekirdek yığınını (Ring3→Ring0 geçiş yığını) tembel olarak
    /// tahsis eder. 4 fiziksel sayfa (16 KiB) contiguous ayrılır, HHDM sanal
    /// adresi `kstack_top` olarak kaydedilir ve çerçeveler kaynak listesine
    /// işlenir (böylece `reap_process_resources` onları da iade eder).
    pub fn ensure_kernel_stack(&mut self, pid: usize, pmm: &mut PhysicalMemoryManager) -> bool {
        let Some(p) = self
            .processes
            .iter_mut()
            .find(|p| p.pid == pid && p.state != ProcessState::Unused)
        else {
            return false;
        };
        if p.kstack_top != 0 {
            return true;
        }
        let Some(base) = pmm.allocate_contiguous_frames(KSTACK_PAGES) else {
            return false;
        };
        // Yığını sıfırla; ardından tepesini 16 bayt hizasına indir.
        unsafe {
            let vbase = crate::drivers::phys_to_virt(base) as *mut u8;
            core::ptr::write_bytes(vbase, 0, (KSTACK_PAGES as u64 * PAGE_SIZE) as usize);
        }
        let top_virt = crate::drivers::phys_to_virt(base + KSTACK_PAGES as u64 * PAGE_SIZE);
        // Çerçeveleri kaynak listesine işle; biri bile sığmazsa geri al.
        let mut recorded = 0usize;
        for i in 0..KSTACK_PAGES {
            let frame = base + i as u64 * PAGE_SIZE;
            if !p.resources.record_frame(frame) {
                break;
            }
            recorded += 1;
        }
        if recorded != KSTACK_PAGES {
            for i in 0..recorded {
                let frame = base + i as u64 * PAGE_SIZE;
                p.resources.take_frame(frame);
                pmm.free_frame(frame);
            }
            return false;
        }
        p.kstack_phys = base;
        p.kstack_top = top_virt & !0xF;
        true
    }

    unsafe fn clone_user_table(
        table_phys: u64,
        level: u8,
        prefix: u64,
        child: &mut Process,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        let entries = (&*AddressSpace::table_ptr(table_phys)).entries;
        let shift = 12 + 9 * (level as u32 - 1);
        for (index, entry) in entries.into_iter().enumerate() {
            if entry & (PageFlags::PRESENT | PageFlags::USER)
                != PageFlags::PRESENT | PageFlags::USER
            {
                continue;
            }
            let virtual_address = prefix | ((index as u64) << shift);
            if level == 1 {
                let frame = pmm
                    .allocate_frame()
                    .ok_or("PMM yetersiz bellek: fork sayfasi tahsis edilemedi")?;
                let source =
                    crate::drivers::phys_to_virt(entry & 0x000F_FFFF_FFFF_F000) as *const u8;
                let destination = crate::drivers::phys_to_virt(frame) as *mut u8;
                core::ptr::copy_nonoverlapping(source, destination, PAGE_SIZE as usize);
                let flags = entry & !0x000F_FFFF_FFFF_F000;
                if !child.resources.record_frame(frame) {
                    pmm.free_frame(frame);
                    return Err("Surec kaynak listesi dolu: fork");
                }
                if child
                    .address_space
                    .map_page(virtual_address, frame, flags, pmm)
                    .is_err()
                {
                    child.resources.take_frame(frame);
                    pmm.free_frame(frame);
                    return Err("Fork sayfasi eslenemedi");
                }
            } else {
                if entry & PageFlags::HUGE_PAGE != 0 {
                    return Err("Fork buyuk kullanici sayfalarini desteklemiyor");
                }
                Self::clone_user_table(
                    entry & 0x000F_FFFF_FFFF_F000,
                    level - 1,
                    virtual_address,
                    child,
                    pmm,
                )?;
            }
        }
        Ok(())
    }

    /// Creates a child with private copies of the parent's user mappings.
    pub fn fork_process(
        &mut self,
        parent_pid: usize,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<usize, &'static str> {
        let parent = self
            .get_process(parent_pid)
            .copied()
            .filter(|process| process.state != ProcessState::Terminated)
            .ok_or("Fork ana sureci bulunamadi")?;
        let slot = self
            .processes
            .iter()
            .position(|process| process.state == ProcessState::Unused)
            .ok_or("Surec tablosu dolu: Fork yapilamadi")?;
        let root = pmm
            .allocate_frame()
            .ok_or("PMM yetersiz bellek: fork PML4 tahsis edilemedi")?;
        unsafe {
            core::ptr::write_bytes(
                crate::drivers::phys_to_virt(root) as *mut u8,
                0,
                PAGE_SIZE as usize,
            );
        }
        let mut child = Process::empty();
        child.pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        child.ppid = parent_pid;
        child.name = parent.name;
        child.name_len = parent.name_len;
        child.state = ProcessState::Ready;
        child.priority = parent.priority;
        child.context = parent.context;
        child.context.cr3 = root;
        child.context.rax = 0;
        child.context.kernel_rsp = 0;
        child.has_saved_user_context = parent.has_saved_user_context;
        child.resources.app_id = parent.resources.app_id;
        child.resources.window_idx = parent.resources.window_idx;
        child.address_space = AddressSpace { pml4_phys: root };
        child.resources.record_frame(root);

        if let Err(error) = child.address_space.clone_kernel_mappings(pmm) {
            pmm.free_frame(root);
            return Err(error);
        }
        let clone_result = unsafe {
            let source = parent.address_space.pml4_phys;
            let pml4 = &*AddressSpace::table_ptr(source);
            let mut result = Ok(());
            for (index, entry) in pml4.entries.iter().enumerate().take(256) {
                if entry & (PageFlags::PRESENT | PageFlags::USER)
                    == PageFlags::PRESENT | PageFlags::USER
                {
                    let prefix = (index as u64) << 39;
                    if let Err(error) = Self::clone_user_table(
                        entry & 0x000F_FFFF_FFFF_F000,
                        3,
                        prefix,
                        &mut child,
                        pmm,
                    ) {
                        result = Err(error);
                        break;
                    }
                }
            }
            result
        };
        if let Err(error) = clone_result {
            child.address_space.free_user_page_tables(pmm);
            for &frame in child
                .resources
                .allocated_frames
                .iter()
                .take(child.resources.frame_count)
            {
                pmm.free_frame(frame);
            }
            return Err(error);
        }
        if !Self::allocate_kernel_stack_for(&mut child, pmm) {
            child.address_space.free_user_page_tables(pmm);
            for &frame in child
                .resources
                .allocated_frames
                .iter()
                .take(child.resources.frame_count)
            {
                pmm.free_frame(frame);
            }
            return Err("PMM yetersiz bellek: fork cekirdek yigini tahsis edilemedi");
        }

        self.processes[slot] = child;
        self.active_count += 1;
        if let Some(parent) = self.get_process_mut(parent_pid) {
            parent.context.rax = child.pid as u64;
        }
        Ok(child.pid)
    }

    fn allocate_kernel_stack_for(p: &mut Process, pmm: &mut PhysicalMemoryManager) -> bool {
        if p.kstack_top != 0 {
            return true;
        }
        let Some(base) = pmm.allocate_contiguous_frames(KSTACK_PAGES) else {
            return false;
        };
        unsafe {
            core::ptr::write_bytes(
                crate::drivers::phys_to_virt(base) as *mut u8,
                0,
                (KSTACK_PAGES as u64 * PAGE_SIZE) as usize,
            );
        }
        for i in 0..KSTACK_PAGES {
            let frame = base + i as u64 * PAGE_SIZE;
            if !p.resources.record_frame(frame) {
                for allocated in 0..i {
                    let owned = base + allocated as u64 * PAGE_SIZE;
                    p.resources.take_frame(owned);
                    pmm.free_frame(owned);
                }
                pmm.free_contiguous_frames(base + i as u64 * PAGE_SIZE, KSTACK_PAGES - i);
                return false;
            }
        }
        p.kstack_phys = base;
        p.kstack_top = crate::drivers::phys_to_virt(base + KSTACK_PAGES as u64 * PAGE_SIZE) & !0xF;
        true
    }

    /// Executes a new raw or ELF image in a fresh address space, preserving only
    /// the process's kernel stack. A failed load leaves the old image intact.
    pub fn exec_process(
        &mut self,
        pid: usize,
        image: &[u8],
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), &'static str> {
        let old = self
            .get_process(pid)
            .copied()
            .filter(|process| process.state != ProcessState::Terminated)
            .ok_or("Exec sureci bulunamadi")?;
        let root = pmm
            .allocate_frame()
            .ok_or("PMM yetersiz bellek: exec PML4 tahsis edilemedi")?;
        unsafe {
            core::ptr::write_bytes(
                crate::drivers::phys_to_virt(root) as *mut u8,
                0,
                PAGE_SIZE as usize,
            );
        }
        let mut new_space = AddressSpace { pml4_phys: root };
        if let Err(error) = new_space.clone_kernel_mappings(pmm) {
            pmm.free_frame(root);
            return Err(error);
        }
        let loaded = match crate::mm::loader::AppLoader::load_binary(image, &mut new_space, pmm) {
            Ok(loaded) if loaded.allocated_pages <= crate::mm::loader::MAX_LOADED_FRAMES => loaded,
            Ok(_) => {
                new_space.destroy_user_address_space(pmm);
                return Err("Exec imaji kaynak sinirini asti");
            }
            Err(error) => {
                new_space.destroy_user_address_space(pmm);
                return Err(error);
            }
        };

        let mut new_resources = ProcessResources::empty();
        new_resources.app_id = old.resources.app_id;
        new_resources.window_idx = old.resources.window_idx;
        if !new_resources.record_frame(root) {
            new_space.destroy_user_address_space(pmm);
            return Err("Exec kaynak listesi dolu");
        }
        let stack_frame_count = if old.kstack_phys == 0 {
            0
        } else {
            KSTACK_PAGES
        };
        for i in 0..stack_frame_count {
            let frame = old.kstack_phys + i as u64 * PAGE_SIZE;
            if !new_resources.record_frame(frame) {
                new_space.destroy_user_address_space(pmm);
                return Err("Exec cekirdek yigini kaynak listesine sigmadi");
            }
        }
        for &frame in loaded.allocated_frames.iter().take(loaded.allocated_pages) {
            if !new_resources.record_frame(frame) {
                new_space.destroy_user_address_space(pmm);
                return Err("Exec kaynak listesi dolu");
            }
        }

        let was_active = AddressSpace::current().pml4_phys == old.address_space.pml4_phys;
        let old_resources = old.resources;
        let mut old_space = old.address_space;
        if let Some(process) = self.get_process_mut(pid) {
            process.address_space = new_space;
            process.resources = new_resources;
            process.resources_reaped = false;
            process.context.rip = loaded.entry_point;
            process.context.rsp = loaded.stack_pointer;
            process.context.cr3 = root;
            process.context.kernel_rsp = 0;
            process.has_saved_user_context = false;
        }
        if was_active {
            unsafe {
                core::arch::asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
            }
        }
        old_space.free_user_page_tables(pmm);
        for &frame in old_resources
            .allocated_frames
            .iter()
            .take(old_resources.frame_count)
        {
            let is_kernel_stack = old.kstack_phys != 0
                && frame >= old.kstack_phys
                && frame < old.kstack_phys + KSTACK_PAGES as u64 * PAGE_SIZE;
            if frame != 0 && !is_kernel_stack {
                pmm.free_frame(frame);
            }
        }
        Ok(())
    }

    /// Reaps one terminated child, or reports that a live child is still running.
    pub fn wait_child(
        &mut self,
        parent_pid: usize,
        child_pid: Option<usize>,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<Option<(usize, i32)>, &'static str> {
        let matching_child = self.processes.iter().position(|child| {
            child.state != ProcessState::Unused
                && child.pid != 0
                && child.ppid == parent_pid
                && child_pid.is_none_or(|pid| child.pid == pid)
        });
        let Some(slot) = matching_child else {
            return Err("Beklenen alt surec bulunamadi");
        };
        if self.processes[slot].state != ProcessState::Terminated {
            return Ok(None);
        }
        let pid = self.processes[slot].pid;
        let exit_code = self.processes[slot].exit_code;
        Self::reap_process_resources(&mut self.processes[slot], pmm);
        self.processes[slot] = Process::empty();
        Ok(Some((pid, exit_code)))
    }

    /// PID ile değiştirilebilir süreç referansı.
    pub fn get_process_mut(&mut self, pid: usize) -> Option<&mut Process> {
        self.processes
            .iter_mut()
            .find(|p| p.pid == pid && p.state != ProcessState::Unused)
    }

    /// Kullanıcı süreçleri zamanlayıcısı için: `Ready` durumunda ve gerçek
    /// bir kullanıcı giriş noktası olan (rip/rsp tanımlı) ilk süreci
    /// round-robin sırasıyla döndürür. GUI sözde-süreçleri (rip=0) atlanır.
    pub fn next_ready_user_process(&mut self) -> Option<usize> {
        for _ in 0..MAX_PROCESSES {
            let idx = self.user_rr % MAX_PROCESSES;
            self.user_rr = (self.user_rr + 1) % MAX_PROCESSES;
            let p = &self.processes[idx];
            if p.pid != 0
                && p.state == ProcessState::Ready
                && !p.resources_reaped
                && p.context.rip != 0
                && p.context.rsp != 0
            {
                return Some(p.pid);
            }
        }
        None
    }

    /// APIC timer preemption adayı: `after` dışında, daha önce kesme ile
    /// kaydedilmiş kullanıcı bağlamı (`kernel_rsp != 0`) olan `Ready` süreç.
    /// Henüz hiç çalışmamış süreçler burada döndürülmez; onlar çekirdek
    /// tarafındaki `enter` yoluyla başlatılır.
    pub fn next_user_process_after(&mut self, after: usize) -> Option<usize> {
        for _ in 0..MAX_PROCESSES {
            let idx = self.user_rr % MAX_PROCESSES;
            self.user_rr = (self.user_rr + 1) % MAX_PROCESSES;
            let p = &self.processes[idx];
            if p.pid != 0
                && p.pid != after
                && p.state == ProcessState::Ready
                && !p.resources_reaped
                && p.context.kernel_rsp != 0
            {
                return Some(p.pid);
            }
        }
        None
    }

    /// Bir sonraki calistirilabilir sureci doner (oncelikli Preemptive Round-Robin Zamanlayici).
    /// Kullanici sureclerini oncelik sirasina (Realtime > High > Normal > Low) gore zamanlar,
    /// calistirilabilir kullanici sureci yoksa PID 0 (kernel_core) fallback doner.
    pub fn schedule_next(&mut self) -> Option<usize> {
        // Sure dolmus engellenmis surecleri once uyandir (item 7).
        let _ = self.wake_timeouts(crate::monotonic_ms());

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

    /// Mevcut calisan surecin kayitli baglamini gunceller, yeni surecen CR3
    /// degerini yukler ve kayitli yaymac durumunu geri yukler (tam context
    /// switch). CR3 degisimi her geciste yapilir; ayni PML4'ye donuldugu takdirde
    /// gereksiz TLB temizleme önlenir.
    ///
    /// # Safety
    /// Donanım kesmesi veya scheduler sinirlari cagrisinda kullanilir.
    /// `next_pid` gecerli, calistirilir durumdaki bir surec olmalidir.
    pub unsafe fn context_switch(&mut self, next_pid: usize) {
        let current_pid = self.current_pid;

        // Mevcut calisan surecin baglamını kaydet (gerekirse)
        if let Some(curr_p) = self.processes.iter_mut().find(|p| p.pid == current_pid) {
            if curr_p.state == ProcessState::Running {
                curr_p.context = ProcessContext::save_current();
                curr_p.state = ProcessState::Ready;
            }
        }

        // Engellenen veya sonlanmis bir sureci secmeyin; schedule_next zaten
        // sadece Ready/Running secer, ancak guvenlik icin burada da kontrol edelim.
        let next_ready = self
            .get_process(next_pid)
            .is_some_and(|p| p.state == ProcessState::Ready || p.state == ProcessState::Running);

        if !next_ready {
            // Calistirilebilir surec yok; kernel_core'a (PID 0) don.
            return self.context_switch_locked(0);
        }

        // Adres uzayini ve baglami yeni surec icin hazirla
        let next_pml4 = self
            .get_process(next_pid)
            .map(|p| p.address_space.pml4_phys);
        let next_context = self.get_process(next_pid).map(|p| p.context);

        let is_same_space = current_pid == next_pid
            || next_pml4
                == Some(
                    self.get_process(current_pid)
                        .map(|p| p.address_space.pml4_phys)
                        .unwrap_or(0),
                );

        // Yeni sürece geç
        if let Some(next_p) = self.processes.iter_mut().find(|p| p.pid == next_pid) {
            self.current_pid = next_pid;
            next_p.state = ProcessState::Running;
            next_p.cpu_time_ticks = next_p.cpu_time_ticks.saturating_add(1);

            // CR3 degisimi: adres uzayi degisse PML4'yi yukle (item 4).
            if let Some(pml4) = next_pml4 {
                if pml4 != 0 && !is_same_space {
                    core::arch::asm!(
                        "mov cr3, {}",
                        in(reg) pml4,
                        options(nostack, preserves_flags)
                    );
                }
            }

            // Kayitli baglami geri yukle ve sureci devam ettir (item 3).
            if let Some(ctx) = next_context {
                ctx.restore();
            }
        }
    }

    /// `context_switch` icin kilitli (yeniden girilebilir olmayan) yardimci:
    /// sadece CR3'yi ayarlar ve PID 0'a doner, restore cagirmayip geri doner.
    unsafe fn context_switch_locked(&mut self, next_pid: usize) {
        if let Some(next_p) = self.processes.iter_mut().find(|p| p.pid == next_pid) {
            self.current_pid = next_pid;
            next_p.state = ProcessState::Running;
            if let Some(pml4) = self
                .get_process(next_pid)
                .map(|p| p.address_space.pml4_phys)
            {
                if pml4 != 0 {
                    core::arch::asm!(
                        "mov cr3, {}",
                        in(reg) pml4,
                        options(nostack, preserves_flags)
                    );
                }
            }
        }
    }

    /// Sureci bir olay (veya mutlak sure dolmasi) uzerine engeller.
    /// `pid` calistirilirken cagrilmalidir; surec Blocked durumuna alinir ve
    /// `schedule_next` bunu atlar. `deadline_ms` verildiyse sure dolunca
    /// `wake_timeouts` ile otomatik uyandirilir.
    pub fn sleep(&mut self, pid: usize, event: usize, deadline_ms: Option<u64>) -> bool {
        if pid == 0 {
            return false;
        }
        if let Some(p) = self.processes.iter_mut().find(|p| p.pid == pid) {
            if p.state == ProcessState::Running || p.state == ProcessState::Ready {
                p.state = ProcessState::Blocked;
                p.wait_event = Some(event);
                p.wait_deadline = deadline_ms;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Belirli bir olaya bekleyen tum surecleri uyandirir ve calistirilir yapar.
    /// Uyandirilan surec sayisini dondurur.
    pub fn wake_event(&mut self, event: usize) -> usize {
        let mut count = 0usize;
        for p in self.processes.iter_mut() {
            if p.state == ProcessState::Blocked && p.wait_event == Some(event) {
                p.state = ProcessState::Ready;
                p.wait_event = None;
                p.wait_deadline = None;
                count += 1;
            }
        }
        count
    }

    /// Sure dolmus engellenmis surecleri uyandirir. `now_ms` guncel monotonik
    /// zaman olmali; gecmis `wait_deadline` olan surecler Ready'ye doner.
    /// Uyandirilan sayiyi dondurur.
    pub fn wake_timeouts(&mut self, now_ms: u64) -> usize {
        let mut count = 0usize;
        for p in self.processes.iter_mut() {
            if p.state == ProcessState::Blocked {
                if let Some(deadline) = p.wait_deadline {
                    if now_ms >= deadline {
                        p.state = ProcessState::Ready;
                        p.wait_event = None;
                        p.wait_deadline = None;
                        count += 1;
                    }
                }
            }
        }
        count
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
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
    pub const SYS_EXEC: u64 = 8;
    pub const SYS_FORK: u64 = 9;
    pub const SYS_WAIT: u64 = 10;
}

pub const MAX_EXEC_IMAGE_SIZE: usize = 112 * 1024;

fn copy_user_bytes(process: &Process, address: u64, len: usize) -> Option<alloc::vec::Vec<u8>> {
    let mut bytes = alloc::vec::Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    for offset in 0..len {
        let user_address = address.checked_add(offset as u64)?;
        let physical_address = process.address_space.translate_user(user_address)?;
        let physical_byte = crate::drivers::phys_to_virt(physical_address) as *const u8;
        bytes.push(unsafe { core::ptr::read_volatile(physical_byte) });
    }
    Some(bytes)
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
            proc_mgr.mark_exited(current, exit_code);
            0
        }
        sys_nos::SYS_GETPID => proc_mgr.current_pid as u64,
        sys_nos::SYS_ALLOC_FRAME => {
            if let Some(frame) = pmm.allocate_frame() {
                let current = proc_mgr.current_pid;
                if let Some(p) = proc_mgr.processes.iter_mut().find(|p| p.pid == current) {
                    if p.resources.record_user_frame(frame) {
                        return frame;
                    }
                }
                pmm.free_frame(frame);
                0
            } else {
                0
            }
        }
        sys_nos::SYS_FREE_FRAME => {
            let frame = arg1;
            let current = proc_mgr.current_pid;
            let owned = proc_mgr
                .processes
                .iter_mut()
                .find(|p| p.pid == current)
                .is_some_and(|p| p.resources.take_user_frame(frame));
            if owned {
                pmm.free_frame(frame);
                0
            } else {
                u64::MAX
            }
        }
        sys_nos::SYS_WRITE => {
            let len = arg2 as usize;
            if len == 0 || len > 1024 {
                return 0;
            }
            let Some(process) = proc_mgr.get_process(proc_mgr.current_pid) else {
                return 0;
            };
            let Some(buffer) = copy_user_bytes(process, arg1, len) else {
                return 0;
            };
            if let Ok(text) = core::str::from_utf8(&buffer) {
                crate::serial::print(text);
            }
            len as u64
        }
        sys_nos::SYS_FORK => proc_mgr
            .fork_process(proc_mgr.current_pid, pmm)
            .map(|pid| pid as u64)
            .unwrap_or(u64::MAX),
        sys_nos::SYS_WAIT => {
            let parent_pid = proc_mgr.current_pid;
            let child_pid = (arg1 != 0).then_some(arg1 as usize);
            if arg2 != 0 {
                let Some(parent) = proc_mgr.get_process(parent_pid) else {
                    return u64::MAX;
                };
                for offset in 0..core::mem::size_of::<i32>() {
                    let Some(address) = arg2.checked_add(offset as u64) else {
                        return u64::MAX;
                    };
                    if parent.address_space.translate_user(address).is_none() {
                        return u64::MAX;
                    }
                }
            }
            match proc_mgr.wait_child(parent_pid, child_pid, pmm) {
                Ok(Some((pid, exit_code))) => {
                    if arg2 != 0 {
                        let Some(parent) = proc_mgr.get_process(parent_pid) else {
                            return u64::MAX;
                        };
                        if !ProcessManager::write_wait_status(parent, arg2, exit_code) {
                            return u64::MAX;
                        }
                    }
                    pid as u64
                }
                Ok(None) => {
                    let Some(parent) = proc_mgr.get_process_mut(parent_pid) else {
                        return u64::MAX;
                    };
                    parent.state = ProcessState::Blocked;
                    parent.waiting_child = Some(child_pid.unwrap_or(0));
                    parent.wait_status_addr = arg2;
                    parent.context.rax = 0;
                    parent.has_saved_user_context = true;
                    0
                }
                Err(_) => u64::MAX,
            }
        }
        sys_nos::SYS_EXEC => {
            let len = arg2 as usize;
            if len == 0 || len > MAX_EXEC_IMAGE_SIZE {
                return u64::MAX;
            }
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process(current) else {
                return u64::MAX;
            };
            let Some(image) = copy_user_bytes(process, arg1, len) else {
                return u64::MAX;
            };
            if proc_mgr.exec_process(current, &image, pmm).is_ok() {
                0
            } else {
                u64::MAX
            }
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
    let caller_pid = proc_mgr.current_pid;
    proc_mgr.current_pid = pid;
    let syscall_frame = syscall_dispatch(sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, proc_mgr, pmm);
    let owned_free = syscall_dispatch(sys_nos::SYS_FREE_FRAME, syscall_frame, 0, 0, proc_mgr, pmm);
    let owned_free_succeeded = syscall_frame != 0 && owned_free == 0;
    proc_mgr.current_pid = 0;
    let free_before_unowned = pmm.free_memory_bytes;
    let unowned_free = syscall_dispatch(sys_nos::SYS_FREE_FRAME, frame2, 0, 0, proc_mgr, pmm);
    let unowned_free_rejected =
        unowned_free == u64::MAX && pmm.free_memory_bytes == free_before_unowned;
    proc_mgr.current_pid = caller_pid;
    if invalid_res == u64::MAX
        && null_ptr_write == 0
        && owned_free_succeeded
        && unowned_free_rejected
    {
        crate::serial_print(
            "[PROC-TEST] 3. Geçersiz syscall, kullanıcı tamponu ve sahiplik sınırları: BAŞARILI\n",
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

    // 7. Esit onemli surecler icin round-robin dogrulamasi.
    let rr_a = proc_mgr
        .spawn("rr_a", 0x3000, 0x7000_3000, 1, None, pmm)
        .unwrap_or(0);
    let rr_b = proc_mgr
        .spawn("rr_b", 0x4000, 0x7000_4000, 1, None, pmm)
        .unwrap_or(0);
    let first = proc_mgr.schedule_next();
    let second = proc_mgr.schedule_next();
    let rr_ok = matches!(first, Some(x) if x == rr_a || x == rr_b)
        && matches!(second, Some(y) if y == rr_a || y == rr_b)
        && first != second;
    let _ = proc_mgr.terminate(rr_a, 0, pmm);
    let _ = proc_mgr.terminate(rr_b, 0, pmm);

    proc_mgr.current_pid = 0;
    proc_mgr.reap_terminated(pmm);
    let lifecycle_free_before = pmm.free_memory_bytes;
    let lifecycle_parent = proc_mgr.spawn(
        "lifecycle_parent",
        crate::mm::loader::USER_CODE_BASE,
        crate::mm::loader::USER_STACK_TOP - 16,
        0,
        None,
        pmm,
    );
    let mut lifecycle_ok = false;
    if let Ok(parent_pid) = lifecycle_parent {
        let data_address = 0x0000_0000_0050_0000;
        let status_address = data_address + 8;
        let source_frame = pmm.allocate_frame();
        if let (Some(frame), Some(parent)) = (source_frame, proc_mgr.get_process_mut(parent_pid)) {
            unsafe {
                let source = crate::drivers::phys_to_virt(frame) as *mut u8;
                core::ptr::write_bytes(source, 0, PAGE_SIZE as usize);
                core::ptr::write_volatile(source, 0x5A);
            }
            if parent
                .address_space
                .map_page(data_address, frame, PageFlags::USER_DATA, pmm)
                .is_ok()
                && parent.resources.record_user_frame(frame)
            {
                let free_before_fork = pmm.free_memory_bytes;
                proc_mgr.current_pid = parent_pid;
                let child_raw = syscall_dispatch(sys_nos::SYS_FORK, 0, 0, 0, proc_mgr, pmm);
                if child_raw != u64::MAX {
                    let child_pid = child_raw as usize;
                    let parent_process = proc_mgr.get_process(parent_pid).copied();
                    let child_process = proc_mgr.get_process(child_pid).copied();
                    let copied_pages_match = match (parent_process, child_process) {
                        (Some(parent), Some(child)) => {
                            let parent_frame = parent.address_space.translate_user(data_address);
                            let child_frame = child.address_space.translate_user(data_address);
                            let distinct_stacks = parent.kstack_top != 0
                                && child.kstack_top != 0
                                && parent.kstack_phys != child.kstack_phys
                                && parent.address_space.pml4_phys != child.address_space.pml4_phys;
                            match (parent_frame, child_frame) {
                                (Some(parent_frame), Some(child_frame)) => {
                                    let parent_byte = unsafe {
                                        core::ptr::read_volatile(crate::drivers::phys_to_virt(
                                            parent_frame,
                                        )
                                            as *const u8)
                                    };
                                    let child_byte = unsafe {
                                        core::ptr::read_volatile(crate::drivers::phys_to_virt(
                                            child_frame,
                                        )
                                            as *const u8)
                                    };
                                    parent_frame != child_frame
                                        && parent_byte == 0x5A
                                        && child_byte == 0x5A
                                        && distinct_stacks
                                }
                                _ => false,
                            }
                        }
                        _ => false,
                    };

                    let pending_wait = syscall_dispatch(
                        sys_nos::SYS_WAIT,
                        child_pid as u64,
                        status_address,
                        0,
                        proc_mgr,
                        pmm,
                    );
                    let parent_blocked = pending_wait == 0
                        && proc_mgr
                            .get_process(parent_pid)
                            .is_some_and(|parent| parent.state == ProcessState::Blocked);
                    proc_mgr.current_pid = child_pid;
                    let child_exit = syscall_dispatch(sys_nos::SYS_EXIT, 42, 0, 0, proc_mgr, pmm);
                    let waiter_woken = child_exit == 0
                        && proc_mgr.get_process(parent_pid).is_some_and(|parent| {
                            parent.state == ProcessState::Ready
                                && parent.context.rax == child_pid as u64
                        });
                    proc_mgr.current_pid = parent_pid;
                    let wait_result = syscall_dispatch(
                        sys_nos::SYS_WAIT,
                        child_pid as u64,
                        status_address,
                        0,
                        proc_mgr,
                        pmm,
                    );
                    let status = proc_mgr.get_process(parent_pid).is_some_and(|parent| {
                        parent
                            .address_space
                            .translate_user(status_address)
                            .is_some_and(|physical| unsafe {
                                core::ptr::read_volatile(
                                    crate::drivers::phys_to_virt(physical) as *const i32
                                ) == 42
                            })
                    });
                    let child_reaped = wait_result == child_pid as u64
                        && proc_mgr.get_process(child_pid).is_none()
                        && pmm.free_memory_bytes == free_before_fork;

                    let root_before_exec = proc_mgr
                        .get_process(parent_pid)
                        .map(|parent| parent.address_space.pml4_phys)
                        .unwrap_or(0);
                    let failed_exec = syscall_dispatch(
                        sys_nos::SYS_EXEC,
                        0x0000_0000_6000_0000,
                        16,
                        0,
                        proc_mgr,
                        pmm,
                    ) == u64::MAX
                        && proc_mgr.get_process(parent_pid).is_some_and(|parent| {
                            parent.address_space.pml4_phys == root_before_exec
                        });

                    let raw_image = [0x31, 0xFF, 0xB8, 1, 0, 0, 0, 0xCD, 0x80];
                    if let Some(parent) = proc_mgr.get_process(parent_pid) {
                        if let Some(physical) = parent.address_space.translate_user(data_address) {
                            unsafe {
                                let destination = crate::drivers::phys_to_virt(physical) as *mut u8;
                                core::ptr::copy_nonoverlapping(
                                    raw_image.as_ptr(),
                                    destination.add(32),
                                    raw_image.len(),
                                );
                            }
                        }
                    }
                    let exec_result = syscall_dispatch(
                        sys_nos::SYS_EXEC,
                        data_address + 32,
                        raw_image.len() as u64,
                        0,
                        proc_mgr,
                        pmm,
                    );
                    let exec_ok = exec_result == 0
                        && proc_mgr.get_process(parent_pid).is_some_and(|parent| {
                            parent.address_space.pml4_phys != root_before_exec
                                && parent.context.rip == crate::mm::loader::USER_CODE_BASE
                                && parent
                                    .address_space
                                    .translate_user(crate::mm::loader::USER_STACK_TOP - 16)
                                    .is_some()
                                && parent.kstack_top != 0
                        });
                    proc_mgr.current_pid = 0;
                    let parent_terminated = proc_mgr.terminate(parent_pid, 0, pmm);
                    proc_mgr.reap_terminated(pmm);
                    lifecycle_ok = copied_pages_match
                        && parent_blocked
                        && waiter_woken
                        && status
                        && child_reaped
                        && failed_exec
                        && exec_ok
                        && parent_terminated
                        && pmm.free_memory_bytes == lifecycle_free_before;
                }
            } else if let Some(frame) = source_frame {
                pmm.free_frame(frame);
            }
        } else if let Some(frame) = source_frame {
            pmm.free_frame(frame);
        }
        if !lifecycle_ok {
            proc_mgr.current_pid = 0;
            let _ = proc_mgr.terminate(parent_pid, -1, pmm);
            proc_mgr.reap_terminated(pmm);
        }
    }

    if sched_ok && rr_ok && lifecycle_ok {
        crate::serial_print(
            "[PROC-TEST] 6/7. Oncelik/round-robin ve fork/exec/wait yasam dongusu: BASARILI\n",
        );
        true
    } else {
        crate::serial_print(
            "[PROC-TEST] Zamanlayici veya fork/exec/wait yasam dongusu: BASARISIZ\n",
        );
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
            for entry in pml4.entries[256..512].iter() {
                if (entry & crate::mm::vmm::PageFlags::PRESENT) != 0 {
                    kernel_entries += 1;
                    if (entry & crate::mm::vmm::PageFlags::USER) != 0 {
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

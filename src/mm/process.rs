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

use crate::mm::fd::{self, FdTable, FileDescriptor, FileKind, FileTable};
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::uaccess;
use crate::mm::vmm::{AddressSpace, PageFlags};

/// Azami eszamanli aktif surec sayisi
pub const MAX_PROCESSES: usize = 32;

/// Copy-on-write yazılım biti: x86_64 mimarisi bit 9'u (Ignore) yok sayar,
/// bu yüzden aynı önbellek hattını paylaşımayan güvenli bir yazılım biti olarak
/// serbest kullanılabilir. Fork, yazılabilir sayfaları iki tarafta da bu
/// bayrakla salt-okunur bırakır; ilk yazma #PF üretir ve `handle_cow_fault`
/// sayfayı özel kopyaya dönüştürür.
pub const PAGE_COW: u64 = 1 << 9;

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
/// Süreç başına azami mmap baytı (64 MiB).
pub const MAX_PROC_MMAP_BYTES: u64 = 64 * 1024 * 1024;
/// Süreç başına azami heap baytı (offset; `user_mem::HEAP_LIMIT` ile sınırlanır).
pub const MAX_PROC_HEAP_BYTES: u64 = 64 * 1024 * 1024;

/// Süreç başına kaynak limitleri / per-process resource limits.
///
/// Limitler `fork` ile devralınır, `exec` sonrası korunur ve her tahsis
/// yolunda (`record_frame`, `record_user_frame`, `mmap`, `brk`, `open`)
/// zorunlu olarak denetlenir. Kullanıcı süreçleri `SYS_RESINFO` ile kullanım
/// ve limitlerini okuyabilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Kaynak defterindeki toplam fiziksel çerçeve (kernel yığını, PML4, imaj).
    pub max_frames: usize,
    /// Kullanıcı verisi için ayrılan çerçeve (heap, mmap, COW kopyaları).
    pub max_user_frames: usize,
    /// Açık dosya tanıtıcısı sayısı (0/1/2 dahil).
    pub max_fds: usize,
    /// Toplam mmap bölge baytı.
    pub max_mmap_bytes: u64,
    /// Heap ucunun `HEAP_BASE` göreli azami değeri.
    pub max_heap_bytes: u64,
}

impl ResourceLimits {
    /// Kullanıcı süreçleri için güvenli varsayılanlar.
    pub const fn user_default() -> Self {
        Self {
            max_frames: MAX_PROC_FRAMES,
            max_user_frames: MAX_PROC_USER_FRAMES,
            max_fds: fd::MAX_FDS,
            max_mmap_bytes: MAX_PROC_MMAP_BYTES,
            max_heap_bytes: MAX_PROC_HEAP_BYTES,
        }
    }

    /// Sınırsız (çekirdek süreci / PID 0) limitler.
    pub const fn unlimited() -> Self {
        Self {
            max_frames: usize::MAX,
            max_user_frames: usize::MAX,
            max_fds: fd::MAX_FDS,
            max_mmap_bytes: u64::MAX,
            max_heap_bytes: u64::MAX,
        }
    }
}

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
    /// Bu sürecin kaynak limitleri.
    pub limits: ResourceLimits,
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
            limits: ResourceLimits::user_default(),
        }
    }

    pub fn record_frame(&mut self, frame: u64) -> bool {
        if self.frame_count >= self.limits.max_frames.min(MAX_PROC_FRAMES)
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
        if self.user_frame_count >= self.limits.max_user_frames.min(MAX_PROC_USER_FRAMES)
            || !self.record_frame(frame)
        {
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
    /// `brk` ile yönetilen heap ucunun `mm::user_mem::HEAP_BASE` göreli boyu.
    pub heap_brk: u64,
    /// ASLR ile seçilmiş mmap tabanı (süreç ömrü boyunca sabit).
    pub mmap_base: u64,
    /// mmap bölgelerinin taban göreli toplam boyu.
    pub mmap_used: u64,
    /// Açık mmap bölgeleri.
    pub mmap_regions: crate::mm::user_mem::MmapTable,
    /// Süreç dosya tanıtıcısı tablosu (0/1/2 standart, 3+ `open`).
    pub fds: FdTable,
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
            heap_brk: 0,
            mmap_base: 0,
            mmap_used: 0,
            mmap_regions: [crate::mm::user_mem::MmapRegion {
                addr: 0,
                len: 0,
                used: false,
            }; crate::mm::user_mem::MAX_MMAP_REGIONS],
            fds: [FileDescriptor::empty(); crate::mm::fd::MAX_FDS],
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("unknown")
    }

    /// `brk` heap'inde `offset` baytındaki sanal adres.
    pub fn heap_address(&self, offset: u64) -> u64 {
        crate::mm::user_mem::HEAP_BASE + offset
    }

    /// Heap ucu (son geçerli bayttan bir sonraki adres).
    pub fn heap_end(&self) -> u64 {
        crate::mm::user_mem::HEAP_BASE + self.heap_brk
    }
}

/// Surec Tablosu ve Zamanlayici Yoneticisi
pub struct ProcessManager {
    pub processes: [Process; MAX_PROCESSES],
    pub current_pid: usize,
    pub next_pid: usize,
    pub active_count: usize,
    /// Çekirdek genelinde açık dosya nesneleri (fd içerikleri, refcount).
    pub files: FileTable,
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
            files: FileTable::new(),
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
        p.resources.limits = ResourceLimits::unlimited();
        // Kernel sureci mevcut adres uzayini kullanir
        p.address_space = AddressSpace::current();
        fd::init_standard_fds(&mut p.fds);
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
        fd::init_standard_fds(&mut p.fds);
        // ASLR: her süreç için mmap tabanı rastgele seçilir.
        p.mmap_base = crate::mm::loader::randomized_mmap_base();

        // Yeni PML4'ü kaynağa ekle
        if !p.resources.record_frame(pml4_frame) {
            pmm.free_frame(pml4_frame);
            return Err("Surec kaynak listesi dolu");
        }

        self.processes[slot] = p;
        if !self.ensure_kernel_stack(pid, pmm) {
            let process = &mut self.processes[slot];
            process.state = ProcessState::Terminated;
            Self::reap_process_resources(process, &mut self.files, pmm);
            *process = Process::empty();
            return Err("PMM yetersiz bellek: surec cekirdek yigini tahsis edilemedi");
        }
        self.active_count += 1;

        Ok(pid)
    }

    /// Sürecin kullanıcı sayfa tablolarını (PDPT/PD/PT) ve kayıtlı tüm
    /// fiziksel çerçeveleri (PML4, çekirdek yığını, kullanıcı sayfaları)
    /// PMM'e iade eder. Yalnızca sürecin CR3'ü artık aktif değilken çağrılmalı.
    fn reap_process_resources(
        p: &mut Process,
        files: &mut FileTable,
        pmm: &mut PhysicalMemoryManager,
    ) {
        if p.resources_reaped {
            return;
        }
        // Açık dosya tanıtıcılarını kapat; son referans içeriği serbest bırakır.
        Self::close_fds(p, files);
        // Heap/mmap eşlemelerini kaldır; fiziksel çerçeveler aşağıdaki kaynak
        // defteri geçişinde tek seferde iade edilir (çift iade olmaz).
        let _ = crate::mm::user_mem::release_user_memory(p, pmm);
        // Sonra alt sayfa tabloları: PML4 hâlâ geçerliyken PDPT/PD/PT
        // çerçeveleri çözülür. PML4 defterdedir, aşağıdaki döngüde iade edilir.
        // COW paylaşımlı çerçeveler yalnız defterden referans bırakır.
        p.address_space.free_user_page_tables(pmm);
        for &frame in p.resources.allocated_frames[..p.resources.frame_count].iter() {
            if frame == 0 {
                continue;
            }
            // Her çerçeve için tek referans bırakılır. Çerçeve hâlâ başka bir
            // adres uzayı tarafından paylaşılıyorsa (`Some(false)`) fiziksel
            // bellek serbest bırakılmaz.
            let still_shared = matches!(crate::mm::vmm::release_frame_shared(frame), Some(false));
            if !still_shared {
                pmm.free_frame(frame);
            }
        }
        p.resources.frame_count = 0;
        p.resources.user_frame_count = 0;
        p.resources.allocated_frames = [0; MAX_PROC_FRAMES];
        p.resources.user_allocated_frames = [0; MAX_PROC_USER_FRAMES];
        p.resources_reaped = true;
    }

    /// Sürecin tüm kullanıcı fd'lerini kapatır, nesne referanslarını bırakır ve
    /// 0/1/2 standart tanıtıcılarını yeniden kurar.
    fn close_fds(p: &mut Process, files: &mut FileTable) {
        for object in fd::close_all_user_fds(&mut p.fds) {
            files.release(object);
        }
        fd::init_standard_fds(&mut p.fds);
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
            Self::reap_process_resources(&mut self.processes[slot], &mut self.files, pmm);
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
        // Çıkışta fd'ler hemen kapanır; bellek iadesi reaping'e ertelenir.
        Self::close_fds(&mut self.processes[slot], &mut self.files);
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
        uaccess::copy_to_user(&parent.address_space, address, &exit_code.to_le_bytes()).is_ok()
    }

    /// `Terminated` durumundaki ve kaynakları henüz iade edilmemiş süreçlerin
    /// sayfa çerçevelerini, alt sayfa tablolarını ve çekirdek yığınlarını salar.
    /// Yalnızca çekirdek CR3'ü aktifken (kullanıcı modunda değilken) çağrılmalı.
    pub fn reap_terminated(&mut self, pmm: &mut PhysicalMemoryManager) -> usize {
        let mut reaped = 0usize;
        for p in self.processes.iter_mut() {
            if p.state == ProcessState::Terminated && !p.resources_reaped {
                Self::reap_process_resources(p, &mut self.files, pmm);
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
            Self::reap_process_resources(p, &mut self.files, pmm);
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

    /// Çocuk adres uzayını copy-on-write semantiğiyle kurar: ebeveynin
    /// yazılabilir kullanıcı sayfaları iki adres uzayında da salt-okunur
    /// yapılıp paylaşılır (`PAGE_COW`), ilk yazmada `handle_cow_fault`
    /// sayfayı özel kopyaya dönüştürür. Yalnızca yazılabilirlik biti hiç
    /// taşımamış (salt-okunur kod, RO veri) sayfalar doğrudan paylaşılır.
    ///
    /// `parent_frames` / `child_frames` ilk hata durumunda geri alınacak
    /// "bu fork sırasında paylaşıma açılan" çerçeveleri taşır.
    unsafe fn cow_clone_user_table(
        table_phys: u64,
        level: u8,
        prefix: u64,
        parent: &mut Process,
        child: &mut Process,
        parent_frames: &mut alloc::vec::Vec<u64>,
        child_frames: &mut alloc::vec::Vec<u64>,
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
                let frame = entry & 0x000F_FFFF_FFFF_F000;
                let writable = entry & PageFlags::WRITABLE != 0;
                let mut child_flags = entry & !0x000F_FFFF_FFFF_F000;
                if writable && !crate::mm::vmm::mark_frame_shared(frame) {
                    // COW defteri dolu: çerçeveyi özel kopya olarak ayır.
                    let copy = pmm
                        .allocate_frame()
                        .ok_or("PMM yetersiz bellek: fork sayfasi tahsis edilemedi")?;
                    let source = crate::drivers::phys_to_virt(frame) as *const u8;
                    let destination = crate::drivers::phys_to_virt(copy) as *mut u8;
                    core::ptr::copy_nonoverlapping(source, destination, PAGE_SIZE as usize);
                    if !child.resources.record_frame(copy) {
                        pmm.free_frame(copy);
                        return Err("Surec kaynak listesi dolu: fork");
                    }
                    if child
                        .address_space
                        .map_page(virtual_address, copy, child_flags, pmm)
                        .is_err()
                    {
                        child.resources.take_frame(copy);
                        pmm.free_frame(copy);
                        return Err("Fork sayfasi eslenemedi");
                    }
                    continue;
                }
                if writable {
                    // Paylaşım kuruldu: iki taraf da salt-okunur + COW.
                    child_flags &= !PageFlags::WRITABLE;
                    child_flags |= PAGE_COW;
                    if parent
                        .address_space
                        .set_flags(virtual_address, child_flags, pmm)
                        .is_err()
                    {
                        // Ebeveyn sayfası COW yapılamadı: sayaç kaydını geri al.
                        let _ = crate::mm::vmm::release_frame_shared(frame);
                        return Err("Fork: ebeveyn sayfasi COW yapilamadi");
                    }
                    parent_frames.push(frame);
                    child_frames.push(frame);
                }
                if child
                    .address_space
                    .map_page(virtual_address, frame, child_flags, pmm)
                    .is_err()
                {
                    return Err("Fork sayfasi eslenemedi");
                }
            } else {
                if entry & PageFlags::HUGE_PAGE != 0 {
                    return Err("Fork buyuk kullanici sayfalarini desteklemiyor");
                }
                Self::cow_clone_user_table(
                    entry & 0x000F_FFFF_FFFF_F000,
                    level - 1,
                    virtual_address,
                    parent,
                    child,
                    parent_frames,
                    child_frames,
                    pmm,
                )?;
            }
        }
        Ok(())
    }

    /// COW sayfalarını eski haline getirir (fork geri alınırken).
    fn rollback_cow_shares(
        parent: &mut Process,
        child: &mut Process,
        shared: &[u64],
        pmm: &mut PhysicalMemoryManager,
    ) {
        for &frame in shared {
            let _ = crate::mm::vmm::release_frame_shared(frame);
        }
        // Ebeveynin yazılabilirliğini geri yükle.
        let pml4 = parent.address_space.pml4_phys;
        let child_pml4 = child.address_space.pml4_phys;
        if pml4 == 0 || child_pml4 == 0 {
            return;
        }
        // Yalnızca hâlâ COW bayraklı (yani bu fork'ta paylaşılan ve henüz
        // yazma ile özel kopyaya ayrılmamış) sayfalar geri alınır.
        unsafe {
            let source = &*AddressSpace::table_ptr(child_pml4);
            for (index, pdpt_entry) in source.entries.iter().enumerate().take(256) {
                if pdpt_entry & PageFlags::PRESENT == 0 || pdpt_entry & PageFlags::HUGE_PAGE != 0 {
                    continue;
                }
                let pdpt = &*AddressSpace::table_ptr(pdpt_entry & 0x000F_FFFF_FFFF_F000);
                for (pdpt_idx, pd_entry) in pdpt.entries.iter().enumerate() {
                    if pd_entry & PageFlags::PRESENT == 0 || pd_entry & PageFlags::HUGE_PAGE != 0 {
                        continue;
                    }
                    let pd = &*AddressSpace::table_ptr(pd_entry & 0x000F_FFFF_FFFF_F000);
                    for (pd_idx, pt_entry) in pd.entries.iter().enumerate() {
                        if pt_entry & PageFlags::PRESENT == 0
                            || pt_entry & PageFlags::HUGE_PAGE != 0
                        {
                            continue;
                        }
                        let pt = &*AddressSpace::table_ptr(pt_entry & 0x000F_FFFF_FFFF_F000);
                        for (pt_idx, leaf) in pt.entries.iter().enumerate() {
                            if leaf & (PageFlags::PRESENT | PAGE_COW)
                                != PageFlags::PRESENT | PAGE_COW
                            {
                                continue;
                            }
                            let address = ((index as u64) << 39)
                                | ((pdpt_idx as u64) << 30)
                                | ((pd_idx as u64) << 21)
                                | ((pt_idx as u64) << 12);
                            let restored = (leaf & !PAGE_COW) | PageFlags::WRITABLE;
                            let _ = parent.address_space.set_flags(address, restored, pmm);
                        }
                    }
                }
            }
        }
    }

    /// Creates a child whose user mappings are shared copy-on-write with the
    /// parent. Frames become private copies on the first write (`handle_cow_fault`).
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
        // Kaynak limitleri fork ile devralınır; çerçeve defteri bu limitlerle
        // doldurulmadan önce kopyalanmalıdır.
        child.resources.limits = parent.resources.limits;
        // Heap ve mmap durumu da miras alınır (fork semantiği).
        child.heap_brk = parent.heap_brk;
        child.mmap_base = parent.mmap_base;
        child.mmap_used = parent.mmap_used;
        child.mmap_regions = parent.mmap_regions;
        // Fork, açık fd tablosunu devralır (POSIX dup semantiği); nesne
        // referansları yalnızca fork başarıyla tamamlanınca artırılır.
        child.fds = parent.fds;
        child.address_space = AddressSpace { pml4_phys: root };
        child.resources.record_frame(root);

        if let Err(error) = child.address_space.clone_kernel_mappings(pmm) {
            pmm.free_frame(root);
            return Err(error);
        }
        let mut parent_process = parent;
        let mut cow_frames: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        // Not: aşağıdaki `cow_clone_user_table` çağrısı ebeveynin PTE'lerini
        // salt-okunur + PAGE_COW yapar; bu yüzden tablo girişleri
        // `parent_process.address_space` üzerinden okunur.
        let clone_result = unsafe {
            let source = parent_process.address_space.pml4_phys;
            let pml4 = &*AddressSpace::table_ptr(source);
            let mut result = Ok(());
            for (index, entry) in pml4.entries.iter().enumerate().take(256) {
                if entry & (PageFlags::PRESENT | PageFlags::USER)
                    == PageFlags::PRESENT | PageFlags::USER
                {
                    let prefix = (index as u64) << 39;
                    let mut child_frames: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
                    if let Err(error) = Self::cow_clone_user_table(
                        entry & 0x000F_FFFF_FFFF_F000,
                        3,
                        prefix,
                        &mut parent_process,
                        &mut child,
                        &mut cow_frames,
                        &mut child_frames,
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
            Self::rollback_cow_shares(&mut parent_process, &mut child, &cow_frames, pmm);
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
        // Paylaşılan çerçeveleri çocuk kaynak listesine işle; böylece çocuk
        // çıkışında referans sayaçları doğru bırakılır.
        for &frame in &cow_frames {
            let _ = child.resources.record_frame(frame);
        }
        if !Self::allocate_kernel_stack_for(&mut child, pmm) {
            Self::rollback_cow_shares(&mut parent_process, &mut child, &cow_frames, pmm);
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

        let child_pid = child.pid;
        // Devralınan fd'ler için nesne referanslarını artır.
        for entry in child.fds.iter() {
            if entry.used && entry.kind == FileKind::Regular {
                if let Some(object) = self.files.objects.get_mut(entry.object as usize) {
                    if object.used {
                        object.refs = object.refs.saturating_add(1);
                    }
                }
            }
        }
        self.processes[slot] = child;
        self.active_count += 1;
        // Ebeveynin çerçeve bayrakları (COW demote) güncellendi; süreç
        // kaydını geri yaz.
        if let Some(parent) = self.get_process_mut(parent_pid) {
            *parent = parent_process;
            parent.context.rax = child_pid as u64;
        }
        Ok(child_pid)
    }

    /// Copy-on-write sayfa hatasını işler. `fault_addr` içeren 4KB sayfa
    /// `PAGE_COW` işaretliyse çerçeve özel kopyaya dönüştürülür ve TLB
    /// geçersizleştirilir; çağıran kesme çerçevesini yeniden denemek üzere
    /// bırakır. Sayfa COW değilse `false` döner (gerçek hata).
    ///
    /// Başka bir adres uzayındaki çerçeve HHDM üzerinden kopyalanır; bu
    /// yüzden yalnızca çekirdek CR3'ü aktifken de güvenlidir.
    pub fn handle_cow_fault(
        &mut self,
        pid: usize,
        fault_addr: u64,
        pmm: &mut PhysicalMemoryManager,
    ) -> bool {
        let page = fault_addr & !(PAGE_SIZE - 1);
        let Some(slot) = self
            .processes
            .iter()
            .position(|p| p.pid == pid && p.state != ProcessState::Terminated)
        else {
            return false;
        };
        let entry = match self.processes[slot].address_space.read_entry(page) {
            Some(entry) => entry,
            None => return false,
        };
        if entry & PAGE_COW == 0 || entry & PageFlags::WRITABLE != 0 {
            return false;
        }
        let shared_frame = entry & 0x000F_FFFF_FFFF_F000;
        let private_flags = (entry & !PAGE_COW) | PageFlags::WRITABLE;

        // Paylaşım tek eldeyse kopya gerekmez: çerçeveyi yerinde yazılabilir yap.
        let copy_needed = !matches!(
            crate::mm::vmm::release_frame_shared(shared_frame),
            Some(true)
        );
        if !copy_needed {
            return self.processes[slot]
                .address_space
                .set_flags(page, private_flags, pmm)
                .is_ok();
        }

        let Some(copy) = pmm.allocate_frame() else {
            // Bellek yok: sayfayı paylaşımlı (salt-okunur) bırak, yazma tekrar hata verir.
            let _ = crate::mm::vmm::mark_frame_shared(shared_frame);
            return false;
        };
        unsafe {
            let source = crate::drivers::phys_to_virt(shared_frame) as *const u8;
            let destination = crate::drivers::phys_to_virt(copy) as *mut u8;
            core::ptr::copy_nonoverlapping(source, destination, PAGE_SIZE as usize);
        }
        if !self.processes[slot].resources.record_user_frame(copy) {
            pmm.free_frame(copy);
            let _ = crate::mm::vmm::mark_frame_shared(shared_frame);
            return false;
        }
        if self.processes[slot]
            .address_space
            .remap_page(page, copy, pmm)
            .is_err()
        {
            self.processes[slot].resources.take_user_frame(copy);
            pmm.free_frame(copy);
            let _ = crate::mm::vmm::mark_frame_shared(shared_frame);
            return false;
        }
        let resources = &mut self.processes[slot].resources;
        if !resources.take_user_frame(shared_frame) && !resources.take_frame(shared_frame) {
            return false;
        }
        // Yeni çerçeve özel; paylaşım kaydından çıkartıldı.
        self.processes[slot]
            .address_space
            .set_flags(page, private_flags, pmm)
            .is_ok()
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
        // Kaynak limitleri exec boyunca korunur (yalnızca fd tablosu sıfırlanır).
        new_resources.limits = old.resources.limits;
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
        // Başarılı exec: tüm kullanıcı fd'leri kapanır (CLOEXEC davranışı) ve
        // 0/1/2 standart tanıtıcıları yeniden kurulur.
        if let Some(slot) = self.processes.iter().position(|p| p.pid == pid) {
            Self::close_fds(&mut self.processes[slot], &mut self.files);
        }
        if let Some(process) = self.get_process_mut(pid) {
            process.address_space = new_space;
            process.resources = new_resources;
            process.resources_reaped = false;
            process.context.rip = loaded.entry_point;
            process.context.rsp = loaded.stack_pointer;
            process.context.cr3 = root;
            process.context.kernel_rsp = 0;
            process.has_saved_user_context = false;
            // Yeni imaj: heap/mm bölgeleri sıfırlanır, mmap tabanı yeniden
            // rastgeleleştirilir (exec sonrası ASLR).
            process.heap_brk = 0;
            process.mmap_used = 0;
            process.mmap_regions = Process::empty().mmap_regions;
            process.mmap_base = crate::mm::loader::randomized_mmap_base();
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
            if frame != 0
                && !is_kernel_stack
                && !matches!(crate::mm::vmm::release_frame_shared(frame), Some(false))
            {
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
        Self::reap_process_resources(&mut self.processes[slot], &mut self.files, pmm);
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
    /// Heap ucunu ayarlar: RDI = HEAP_BASE'e göre yeni uç (bayt). Dönüş: yeni uç.
    pub const SYS_BRK: u64 = 11;
    /// Anonim mmap: RDI = uzunluk, RSI = bayraklar. Dönüş: adres (0 = hata).
    pub const SYS_MMAP: u64 = 12;
    /// mmap bölgesini bırakır: RDI = adres, RSI = uzunluk. Dönüş: 0 / u64::MAX.
    pub const SYS_MUNMAP: u64 = 13;
    /// Kullanıcı bellek özeti: [0] heap_brk, [1] mmap bölge sayısı,
    /// [2] mmap bayt, [3] eşlenmiş heap sayfası. RDI = çıktı tamponu (u64 x 4).
    pub const SYS_MEMINFO: u64 = 14;
    /// Dosya açar: RDI = yol (`ptr`, NUL sonlu), RSI = bayraklar. Dönüş: fd / `MAX`.
    pub const SYS_OPEN: u64 = 15;
    /// fd kapatır: RDI = fd. Dönüş: `0` / `MAX`.
    pub const SYS_CLOSE: u64 = 16;
    /// Kaynak kullanımı/limit raporu: RDI = çıktı tamponu (u64 x 8).
    pub const SYS_RESINFO: u64 = 17;
    /// fd konumlandırma: RDI = fd, RSI = offset (i64), RDX = whence (0/1/2).
    pub const SYS_LSEEK: u64 = 18;
}

pub const MAX_EXEC_IMAGE_SIZE: usize = 112 * 1024;

fn copy_user_bytes(process: &Process, address: u64, len: usize) -> Option<alloc::vec::Vec<u8>> {
    uaccess::copy_from_user(&process.address_space, address, len).ok()
}

/// System call dispatcher (test/çekirdek içi kolaylık sarmalayıcısı).
/// `arg3` = RDX (3. argüman); 4. argüman (`R10`) yok sayılır.
/// Ayrıntılı ABI için `syscall_dispatch_abi` kullanın.
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
    arg3: u64,
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> u64 {
    syscall_dispatch_abi(sys_no, arg1, arg2, arg3, 0, proc_mgr, pmm)
}

/// Tam ABI sevkedicisi: RDX (`arg3`) ve R10 (`arg4`) ayrı taşınır.
pub fn syscall_dispatch_abi(
    sys_no: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> u64 {
    let _ = arg4;
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
                if uaccess::validate_user_range(
                    &parent.address_space,
                    arg2,
                    core::mem::size_of::<i32>(),
                    true,
                )
                .is_err()
                {
                    return u64::MAX;
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
        sys_nos::SYS_BRK => {
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return u64::MAX;
            };
            crate::mm::user_mem::brk(process, arg1, pmm)
        }
        sys_nos::SYS_MMAP => {
            if arg1 == 0 || arg1 > 64 * 1024 * 1024 {
                return 0;
            }
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return 0;
            };
            crate::mm::user_mem::mmap(process, arg1, arg2, pmm).unwrap_or(0)
        }
        sys_nos::SYS_MUNMAP => {
            if arg1 == 0 || arg2 == 0 {
                return u64::MAX;
            }
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return u64::MAX;
            };
            if crate::mm::user_mem::munmap(process, arg1, arg2, pmm) {
                0
            } else {
                u64::MAX
            }
        }
        sys_nos::SYS_MEMINFO => {
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process(current) else {
                return u64::MAX;
            };
            let stats = crate::mm::user_mem::stats(process);
            let report = [
                stats.heap_bytes,
                stats.mmap_regions as u64,
                stats.mmap_bytes,
                stats.heap_mapped_pages as u64,
            ];
            if arg1 == 0 {
                return report
                    .iter()
                    .copied()
                    .fold(0u64, |acc, v| acc ^ v.rotate_left(7));
            }
            let mut bytes = [0u8; 32];
            for (index, value) in report.iter().enumerate() {
                bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
            if uaccess::copy_to_user(&process.address_space, arg1, &bytes).is_err() {
                return u64::MAX;
            }
            0
        }
        sys_nos::SYS_OPEN => {
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process(current) else {
                return u64::MAX;
            };
            let Ok(path_bytes) =
                uaccess::copy_cstring_from_user(&process.address_space, arg1, fd::MAX_FD_PATH)
            else {
                return u64::MAX;
            };
            if path_bytes.is_empty() {
                return u64::MAX;
            }
            let Ok(path) = core::str::from_utf8(&path_bytes) else {
                return u64::MAX;
            };
            let flags = arg2 as u32;
            let Ok(object) = proc_mgr.files.open(path, flags) else {
                return u64::MAX;
            };
            let allocation = {
                let Some(process) = proc_mgr.get_process_mut(current) else {
                    return u64::MAX;
                };
                let fd_limit = process.resources.limits.max_fds.min(fd::MAX_FDS);
                if fd::used_fd_count(&process.fds) >= fd_limit {
                    Err(())
                } else {
                    fd::alloc_fd(&mut process.fds, object, flags).map_err(|_| ())
                }
            };
            match allocation {
                Ok(fd_index) => fd_index as u64,
                Err(()) => {
                    proc_mgr.files.release(object);
                    u64::MAX
                }
            }
        }
        sys_nos::SYS_CLOSE => {
            let fd_index = arg1 as usize;
            if !(fd::FIRST_USER_FD..fd::MAX_FDS).contains(&fd_index) {
                return u64::MAX;
            }
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return u64::MAX;
            };
            match fd::close_fd(&mut process.fds, fd_index) {
                Some(object) => {
                    proc_mgr.files.release(object);
                    0
                }
                None => u64::MAX,
            }
        }
        sys_nos::SYS_READ => {
            let current = proc_mgr.current_pid;
            let fd_index = arg1 as usize;
            let buffer_address = arg2;
            let len = arg3 as usize;
            if len == 0 {
                return 0;
            }
            if len > uaccess::MAX_USER_COPY {
                return u64::MAX;
            }
            let descriptor = {
                let Some(process) = proc_mgr.get_process(current) else {
                    return u64::MAX;
                };
                match process.fds.get(fd_index) {
                    Some(entry) if entry.used => *entry,
                    _ => return u64::MAX,
                }
            };
            if descriptor.kind == FileKind::Stdin {
                return 0;
            }
            if descriptor.kind != FileKind::Regular {
                return u64::MAX;
            }
            let mut buffer = alloc::vec::Vec::new();
            if buffer.try_reserve_exact(len).is_err() {
                return u64::MAX;
            }
            buffer.resize(len, 0);
            let read =
                proc_mgr
                    .files
                    .read_at(descriptor.object as usize, descriptor.offset, &mut buffer);
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return u64::MAX;
            };
            let address_space = process.address_space;
            if uaccess::copy_to_user(&address_space, buffer_address, &buffer[..read]).is_err() {
                return u64::MAX;
            }
            if let Some(entry) = process.fds.get_mut(fd_index) {
                entry.offset = entry.offset.saturating_add(read as u64);
            }
            read as u64
        }
        sys_nos::SYS_LSEEK => {
            let current = proc_mgr.current_pid;
            let fd_index = arg1 as usize;
            let delta = arg2 as i64;
            let whence = arg3;
            let descriptor = {
                let Some(process) = proc_mgr.get_process(current) else {
                    return u64::MAX;
                };
                match process.fds.get(fd_index) {
                    Some(entry) if entry.used => *entry,
                    _ => return u64::MAX,
                }
            };
            if descriptor.kind != FileKind::Regular {
                return u64::MAX;
            }
            let Some(size) = proc_mgr.files.object_size(descriptor.object as usize) else {
                return u64::MAX;
            };
            let base = match whence {
                0 => 0i128,
                1 => descriptor.offset as i128,
                2 => size as i128,
                _ => return u64::MAX,
            };
            let target = base + delta as i128;
            if target < 0 || target > size as i128 {
                return u64::MAX;
            }
            let Some(process) = proc_mgr.get_process_mut(current) else {
                return u64::MAX;
            };
            let Some(entry) = process.fds.get_mut(fd_index) else {
                return u64::MAX;
            };
            entry.offset = target as u64;
            target as u64
        }
        sys_nos::SYS_RESINFO => {
            let current = proc_mgr.current_pid;
            let Some(process) = proc_mgr.get_process(current) else {
                return u64::MAX;
            };
            let stats = crate::mm::user_mem::stats(process);
            let report = [
                process.resources.limits.max_frames as u64,
                process.resources.frame_count as u64,
                process.resources.limits.max_user_frames as u64,
                process.resources.user_frame_count as u64,
                process.resources.limits.max_fds as u64,
                fd::used_fd_count(&process.fds) as u64,
                process.resources.limits.max_mmap_bytes,
                stats.mmap_bytes,
            ];
            let mut bytes = [0u8; 64];
            for (index, value) in report.iter().enumerate() {
                bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
            }
            if uaccess::copy_to_user(&process.address_space, arg1, &bytes).is_err() {
                return u64::MAX;
            }
            0
        }
        _ => u64::MAX, // Desteklenmeyen cagri
    }
}

/// Kullanıcı adresine 8 bayt yazar; hedef sayfa yazılabilir (`WRITABLE`) ve
/// kullanıcı erişimli değilse reddeder. Salt-okunur kod sayfasına yazma
/// girişimi `false` döner (çekirdek #PF üretmez).
pub fn write_user_u64(process: &Process, address: u64, value: u64) -> bool {
    uaccess::write_user_u64(&process.address_space, address, value).is_ok()
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
                    let (cow_shared, cow_split) = match (parent_process, child_process) {
                        (Some(parent), Some(child)) => {
                            let parent_frame = parent.address_space.translate_user(data_address);
                            let child_frame = child.address_space.translate_user(data_address);
                            let distinct_stacks = parent.kstack_top != 0
                                && child.kstack_top != 0
                                && parent.kstack_phys != child.kstack_phys
                                && parent.address_space.pml4_phys != child.address_space.pml4_phys;
                            // Copy-on-write: fork sonrası iki süreç de aynı
                            // fiziksel çerçeveyi paylaşır, iki tarafta da PTE
                            // salt-okunur + PAGE_COW olur ve içerik korunur.
                            let shared = match (parent_frame, child_frame) {
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
                                    let parent_entry =
                                        parent.address_space.read_entry(data_address).unwrap_or(0);
                                    let child_entry =
                                        child.address_space.read_entry(data_address).unwrap_or(0);
                                    parent_frame == child_frame
                                        && parent_byte == 0x5A
                                        && child_byte == 0x5A
                                        && parent_entry & PageFlags::WRITABLE == 0
                                        && child_entry & PageFlags::WRITABLE == 0
                                        && parent_entry & PAGE_COW != 0
                                        && child_entry & PAGE_COW != 0
                                        && crate::mm::vmm::is_frame_shared(parent_frame)
                                        && distinct_stacks
                                }
                                _ => false,
                            };
                            // Çocuğun ilk yazması kendi özel kopyasını üretmeli,
                            // ebeveynin sayfası değişmemeli.
                            let split = match (parent_frame, child_frame) {
                                (Some(shared_frame), Some(_)) => {
                                    let child_before =
                                        child.address_space.translate_user(data_address);
                                    let entry_before =
                                        child.address_space.read_entry(data_address).unwrap_or(0);
                                    let handled =
                                        proc_mgr.handle_cow_fault(child.pid, data_address, pmm);
                                    let child_after = proc_mgr
                                        .get_process(child.pid)
                                        .and_then(|c| c.address_space.translate_user(data_address));
                                    let parent_after = proc_mgr
                                        .get_process(parent_pid)
                                        .and_then(|p| p.address_space.translate_user(data_address));
                                    let ok = handled
                                        && child_after
                                            .is_some_and(|private| private != shared_frame)
                                        && parent_after == Some(shared_frame)
                                        && child_before == Some(shared_frame);
                                    if !ok {
                                        crate::serial_print("\n[PROC-TEST] split: handled=");
                                        crate::serial::print_dec(handled as u64);
                                        crate::serial_print(" shared=");
                                        crate::serial::print_hex(shared_frame);
                                        crate::serial_print(" child_before=");
                                        crate::serial::print_hex(child_before.unwrap_or(u64::MAX));
                                        crate::serial_print(" entry=");
                                        crate::serial::print_hex(entry_before);
                                        crate::serial_print(" child_after=");
                                        crate::serial::print_hex(child_after.unwrap_or(u64::MAX));
                                        crate::serial_print(" parent_after=");
                                        crate::serial::print_hex(parent_after.unwrap_or(u64::MAX));
                                        crate::serial_print("\n");
                                    }
                                    ok
                                }
                                _ => false,
                            };
                            (shared, split)
                        }
                        _ => (false, false),
                    };

                    // `SYS_WAIT` status işaretçisini yazmadan önce
                    // `uaccess::validate_user_range(.., write = true)` doğrular;
                    // fork sonrası sayfa `PAGE_COW` + salt-okunur olduğundan bu
                    // yazma reddedilir. Gerçek kullanıcı programı ilk yazmasında
                    // #PF alıp COW kopyasına geçer; burada o adımı simüle ediyoruz.
                    let status_cow = proc_mgr.handle_cow_fault(parent_pid, status_address, pmm);
                    if !status_cow {
                        crate::serial_print("[PROC-TEST] status COW cozulemedi\n");
                    }
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
                    if !parent_blocked {
                        crate::serial_print("[PROC-TEST] wait1 ret=");
                        crate::serial::print_hex(pending_wait);
                        crate::serial_print(" status_addr=");
                        crate::serial::print_hex(status_address);
                        crate::serial_print(" entry=");
                        crate::serial::print_hex(
                            proc_mgr
                                .get_process(parent_pid)
                                .and_then(|p| p.address_space.read_entry(status_address))
                                .unwrap_or(0),
                        );
                        crate::serial_print(" state=");
                        crate::serial::print_dec(
                            proc_mgr
                                .get_process(parent_pid)
                                .map(|p| match p.state {
                                    ProcessState::Ready => 1,
                                    ProcessState::Running => 2,
                                    ProcessState::Blocked => 3,
                                    ProcessState::Terminated => 4,
                                    ProcessState::Unused => 5,
                                })
                                .unwrap_or(9),
                        );
                        crate::serial_print("\n");
                    }
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
                    let reaped_ret = wait_result == child_pid as u64;
                    let reaped_gone = proc_mgr.get_process(child_pid).is_none();
                    let reaped_free = pmm.free_memory_bytes == free_before_fork;
                    if !(reaped_ret && reaped_gone && reaped_free) {
                        crate::serial_print("[PROC-TEST] reap: ret=");
                        crate::serial::print_hex(wait_result);
                        crate::serial_print(" pid=");
                        crate::serial::print_dec(child_pid as u64);
                        crate::serial_print(" gone=");
                        crate::serial::print_dec(reaped_gone as u64);
                        crate::serial_print(" free_before=");
                        crate::serial::print_dec(free_before_fork);
                        crate::serial_print(" free_now=");
                        crate::serial::print_dec(pmm.free_memory_bytes);
                        crate::serial_print("\n");
                    }
                    let child_reaped = reaped_ret && reaped_gone && reaped_free;

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
                    if !(cow_shared
                        && cow_split
                        && parent_blocked
                        && waiter_woken
                        && status
                        && child_reaped
                        && failed_exec
                        && exec_ok)
                    {
                        crate::serial_print("[PROC-TEST] fork/exec detay: cow_shared=");
                        crate::serial::print_dec(cow_shared as u64);
                        crate::serial_print(" cow_split=");
                        crate::serial::print_dec(cow_split as u64);
                        crate::serial_print(" blocked=");
                        crate::serial::print_dec(parent_blocked as u64);
                        crate::serial_print(" woken=");
                        crate::serial::print_dec(waiter_woken as u64);
                        crate::serial_print(" status=");
                        crate::serial::print_dec(status as u64);
                        crate::serial_print(" reaped=");
                        crate::serial::print_dec(child_reaped as u64);
                        crate::serial_print(" bad_exec=");
                        crate::serial::print_dec(failed_exec as u64);
                        crate::serial_print(" exec=");
                        crate::serial::print_dec(exec_ok as u64);
                        crate::serial_print("\n");
                    }
                    lifecycle_ok = cow_shared
                        && cow_split
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

/// Syscall kullanıcı işaretçi (pointer) doğrulamalarını tek noktada toplayan test.
///
/// Kapsam: `SYS_WRITE`, `SYS_WAIT`, `SYS_EXEC`, `SYS_MMAP`, `SYS_MUNMAP` ve
/// `SYS_MEMINFO` yollarındaki tüm kullanıcı adresleri `translate_user` ile
/// doğrulanır. Bu test; NULL, hizasız, yarı eşlenmiş, çekirdek yarı adresi ve
/// sınır aşan uzunlukların reddedildiğini, geçerli çağrıların ise çalıştığını
/// kanıtlar. Ayrıca hatalı çağrıların PMM'de çerçeve sızdırmadığını denetler.
pub fn test_syscall_pointer_validation(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print("[PTR-TEST] --- Syscall kullanıcı işaretçi doğrulama testi ---\n");
    let caller_pid = proc_mgr.current_pid;
    let free_before = pmm.free_memory_bytes;

    let Some(pid) = proc_mgr
        .spawn(
            "ptr_test",
            crate::mm::loader::USER_CODE_BASE,
            crate::mm::loader::USER_STACK_TOP - 16,
            0,
            None,
            pmm,
        )
        .ok()
    else {
        crate::serial_print("[PTR-TEST] Süreç oluşturulamadı: BAŞARISIZ\n");
        return false;
    };
    proc_mgr.current_pid = pid;

    // Test tamponu: tek sayfa, kullanıcı veri bayraklarıyla eşlenir.
    let buffer_address: u64 = 0x0000_0000_0060_0000;
    let Some(buffer_frame) = pmm.allocate_frame() else {
        crate::serial_print("[PTR-TEST] Çerçeve tahsisi: BAŞARISIZ\n");
        proc_mgr.current_pid = caller_pid;
        let _ = proc_mgr.terminate(pid, -1, pmm);
        proc_mgr.reap_terminated(pmm);
        return false;
    };
    let mapped = proc_mgr.get_process_mut(pid).is_some_and(|process| {
        process
            .address_space
            .map_page(buffer_address, buffer_frame, PageFlags::USER_DATA, pmm)
            .is_ok()
            && process.resources.record_user_frame(buffer_frame)
    });
    if !mapped {
        pmm.free_frame(buffer_frame);
        proc_mgr.current_pid = caller_pid;
        let _ = proc_mgr.terminate(pid, -1, pmm);
        proc_mgr.reap_terminated(pmm);
        crate::serial_print("[PTR-TEST] Tampon eşlenemedi: BAŞARISIZ\n");
        return false;
    }
    // Tamponu taninabilir bir desenle doldur (WRITE geri okumasini dogrulamak icin).
    let filled = proc_mgr.get_process(pid).is_some_and(|process| {
        let physical = process.address_space.translate_user(buffer_address);
        physical.is_some_and(|phys| unsafe {
            let base = crate::drivers::phys_to_virt(phys) as *mut u8;
            for offset in 0..64usize {
                core::ptr::write_volatile(base.add(offset), b'A');
            }
            true
        })
    });
    if !filled {
        crate::serial_print("[PTR-TEST] Tampon doldurulamadi: BAŞARISIZ\n");
    }

    // --- Reddedilmesi gereken işaretçiler -------------------------------------
    // NULL: arg1 = 0 uzunluk 0 -> etkisiz; uzunluk > 0 -> reddedilmeli.
    let null_write = syscall_dispatch(sys_nos::SYS_WRITE, 0, 64, 0, proc_mgr, pmm);
    // Hizasiz (sayfa sonundan sonraki bayt): tampon + 4096 tam olarak bir
    // sonraki, eslenmemis sayfaya denk gelir.
    let misaligned_write = syscall_dispatch(
        sys_nos::SYS_WRITE,
        buffer_address + PAGE_SIZE,
        8,
        0,
        proc_mgr,
        pmm,
    );
    // Yari eslenmis tampon: ilk 8 bayt gecerli, kalan 8 bayt eslenmemis
    // sayfaya tasiyor; tum cagri reddedilmeli.
    let straddle_write = syscall_dispatch(
        sys_nos::SYS_WRITE,
        buffer_address + PAGE_SIZE - 8,
        16,
        0,
        proc_mgr,
        pmm,
    );
    // Cekirdek yarisi isaretcisi (cekirdek metni; USER adres alaninin ustunde).
    let kernel_address = test_syscall_pointer_validation as *const () as u64;
    let kernel_write = syscall_dispatch(sys_nos::SYS_WRITE, kernel_address, 8, 0, proc_mgr, pmm);
    // Gecersiz uzunluklar.
    let empty_write = syscall_dispatch(sys_nos::SYS_WRITE, buffer_address, 0, 0, proc_mgr, pmm);
    let oversized_write =
        syscall_dispatch(sys_nos::SYS_WRITE, buffer_address, 4096, 0, proc_mgr, pmm);
    // SYS_WAIT: arg2 (durum isaretcisi) eslenmemis.
    let wait_bad_status = syscall_dispatch(
        sys_nos::SYS_WAIT,
        0,
        buffer_address + PAGE_SIZE,
        0,
        proc_mgr,
        pmm,
    );
    // SYS_EXEC: arg1 NULL/eslenmemis, arg2 sifir veya asiri buyuk.
    let exec_null = syscall_dispatch(sys_nos::SYS_EXEC, 0, 16, 0, proc_mgr, pmm);
    let exec_unmapped = syscall_dispatch(
        sys_nos::SYS_EXEC,
        buffer_address + PAGE_SIZE,
        16,
        0,
        proc_mgr,
        pmm,
    );
    let exec_oversized = syscall_dispatch(
        sys_nos::SYS_EXEC,
        buffer_address,
        MAX_EXEC_IMAGE_SIZE as u64 + 1,
        0,
        proc_mgr,
        pmm,
    );
    // SYS_MMAP: sifir/asiri uzunluk reddedilir (0 doner).
    let mmap_zero = syscall_dispatch(sys_nos::SYS_MMAP, 0, 0, 0, proc_mgr, pmm);
    let mmap_oversized =
        syscall_dispatch(sys_nos::SYS_MMAP, 64 * 1024 * 1024 + 1, 0, 0, proc_mgr, pmm);
    // SYS_MUNMAP: NULL adres ve sifir uzunluk reddedilir.
    let munmap_null = syscall_dispatch(sys_nos::SYS_MUNMAP, 0, PAGE_SIZE, 0, proc_mgr, pmm);
    let munmap_zero_len =
        syscall_dispatch(sys_nos::SYS_MUNMAP, buffer_address, 0, 0, proc_mgr, pmm);
    // SYS_MEMINFO: eslenmemis rapor adresi reddedilir.
    let meminfo_bad = syscall_dispatch(
        sys_nos::SYS_MEMINFO,
        buffer_address + PAGE_SIZE,
        0,
        0,
        proc_mgr,
        pmm,
    );

    let rejected = null_write == 0
        && misaligned_write == 0
        && straddle_write == 0
        && kernel_write == 0
        && empty_write == 0
        && oversized_write == 0
        && wait_bad_status == u64::MAX
        && exec_null == u64::MAX
        && exec_unmapped == u64::MAX
        && exec_oversized == u64::MAX
        && mmap_zero == 0
        && mmap_oversized == 0
        && munmap_null == u64::MAX
        && munmap_zero_len == u64::MAX
        && meminfo_bad == u64::MAX;

    // --- Kabul edilmesi gereken cagrilar -------------------------------------
    let valid_write = syscall_dispatch(sys_nos::SYS_WRITE, buffer_address, 8, 0, proc_mgr, pmm);
    let write_ok = valid_write == 8;
    // Tamponun son baytina kadar (sayfa sinirinin hemen altinda) yazma.
    let edge_write = syscall_dispatch(
        sys_nos::SYS_WRITE,
        buffer_address + PAGE_SIZE - 8,
        8,
        0,
        proc_mgr,
        pmm,
    );
    let edge_ok = edge_write == 8;
    // Eslenmemis bir mmap bölgesi üzerinde munmap tekrar reddedilmeli.
    let unmap_unmapped = syscall_dispatch(
        sys_nos::SYS_MUNMAP,
        buffer_address + 2 * PAGE_SIZE,
        PAGE_SIZE,
        0,
        proc_mgr,
        pmm,
    );
    let double_unmap_rejected = unmap_unmapped == u64::MAX;

    proc_mgr.current_pid = caller_pid;
    let terminated = proc_mgr.terminate(pid, 0, pmm);
    proc_mgr.reap_terminated(pmm);
    // Sizinti denetimi: surec sonlandirilip kaynaklari iade edildikten sonra
    // PMM tekrar baslangic degerine donmeli.
    let no_leak = pmm.free_memory_bytes == free_before;
    let reclaimed = no_leak && terminated;

    let ok = filled && rejected && write_ok && edge_ok && double_unmap_rejected && reclaimed;
    if ok {
        crate::serial_print(
            "[PTR-TEST] NULL/hizasiz/yari-eslenmis/cekirdek isaretcileri ve sinir uzunluklari reddedildi, gecerli cagrilar calisti, sizinti yok: BASARILI\n",
        );
    } else {
        crate::serial_print("[PTR-TEST] Syscall isaretci dogrulamasi: BASARISIZ (");
        crate::serial_print(if filled { "fill " } else { "fill! " });
        crate::serial_print(if rejected { "reject " } else { "reject! " });
        crate::serial_print(if write_ok { "write " } else { "write! " });
        crate::serial_print(if edge_ok { "edge " } else { "edge! " });
        crate::serial_print(if double_unmap_rejected {
            "2munmap "
        } else {
            "2munmap! "
        });
        crate::serial_print(if no_leak { "noleak " } else { "noleak! " });
        crate::serial_print(if reclaimed { "reclaim" } else { "reclaim!" });
        crate::serial_print(")\n");
    }
    ok
}

/// Kullanıcı heap/`brk`, `mmap`/`munmap`, ASLR ve copy-on-write doğrulama testi.
///
/// Sıfırlamış çerçevelerle çalışır (gerçek kullanıcı yığını/kodu değil); amaç
/// bellek semantiğini gerçek sayfa tabloları ve PMM üzerinde doğrulamaktır.
pub fn test_user_memory(proc_mgr: &mut ProcessManager, pmm: &mut PhysicalMemoryManager) -> bool {
    crate::serial_print("[USER-MEM] --- Kullanıcı heap/mmap, ASLR ve COW doğrulama testi ---\n");
    let caller_pid = proc_mgr.current_pid;
    let free_before = pmm.free_memory_bytes;

    let Some(pid) = proc_mgr
        .spawn(
            "usermem",
            crate::mm::loader::USER_CODE_BASE,
            crate::mm::loader::USER_STACK_TOP - 16,
            0,
            None,
            pmm,
        )
        .ok()
    else {
        crate::serial_print("[USER-MEM] Süreç oluşturulamadı: BAŞARISIZ\n");
        return false;
    };
    proc_mgr.current_pid = pid;

    // 1. brk: heap büyüt, çerçeveler sıfır gelmeli.
    let heap_end = syscall_dispatch(sys_nos::SYS_BRK, 3 * PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let heap_ok = heap_end == crate::mm::user_mem::HEAP_BASE + 3 * PAGE_SIZE
        && proc_mgr.get_process(pid).is_some_and(|p| {
            (0..3).all(|i| {
                let address = p.heap_address(i as u64 * PAGE_SIZE);
                p.address_space
                    .translate_user(address)
                    .and_then(|phys| {
                        p.address_space
                            .read_entry(address)
                            .map(|entry| (phys, entry))
                    })
                    .is_some_and(|(phys, entry)| {
                        entry & PageFlags::WRITABLE != 0
                            && entry & PageFlags::NO_EXECUTE != 0
                            && unsafe {
                                core::ptr::read_volatile(
                                    crate::drivers::phys_to_virt(phys) as *const u8
                                )
                            } == 0
                    })
            })
        });
    // 1b. Heap uçlarında yazma/okuma çalışır (in bounds).
    let heap_write_ok = proc_mgr
        .get_process(pid)
        .is_some_and(|p| write_user_u64(p, p.heap_address(0), 0xDEAD_BEEF_CAFE_F00D));
    // 1c. Hemen üstündeki eşlenmemiş sayfaya çekirdek erişimi reddedilir.
    let heap_bounds_ok = proc_mgr
        .get_process(pid)
        .is_some_and(|p| !write_user_u64(p, p.heap_address(3 * PAGE_SIZE), 1));

    // 2. brk küçültme: yalnız kalan son sayfa serbest bırakılır.
    let free_before_shrink = pmm.free_memory_bytes;
    let shrunk = syscall_dispatch(sys_nos::SYS_BRK, 2 * PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let shrink_ok = shrunk == crate::mm::user_mem::HEAP_BASE + 2 * PAGE_SIZE
        && pmm.free_memory_bytes == free_before_shrink + PAGE_SIZE
        && proc_mgr.get_process(pid).is_some_and(|p| {
            p.address_space
                .translate(p.heap_address(2 * PAGE_SIZE))
                .is_none()
        });

    // 3. mmap/munmap: ASLR tabanından ayrık bölge ve tam iade.
    let region = syscall_dispatch(sys_nos::SYS_MMAP, 2 * 4096, 0, 0, proc_mgr, pmm);
    let region_pages = proc_mgr.get_process(pid).is_some_and(|p| {
        region == p.mmap_base
            && region.is_multiple_of(PAGE_SIZE)
            && (crate::mm::loader::ASLR_MMAP_MIN..crate::mm::loader::ASLR_MMAP_MAX)
                .contains(&region)
            && (0..2).all(|i| {
                p.address_space
                    .translate_user(region + i * PAGE_SIZE)
                    .is_some_and(|phys| unsafe {
                        core::ptr::read_volatile(crate::drivers::phys_to_virt(phys) as *const u64)
                            == 0
                    })
            })
    });
    let second = syscall_dispatch(sys_nos::SYS_MMAP, PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let second_ok = second == region + 2 * PAGE_SIZE;
    let free_before_unmap = pmm.free_memory_bytes;
    let unmap = syscall_dispatch(sys_nos::SYS_MUNMAP, region, 2 * 4096, 0, proc_mgr, pmm);
    let free_after_unmap = pmm.free_memory_bytes;
    // `munmap` yalnız `region` aralığını (2 sayfa) iade eder; `second` hâlâ
    // eşlenmiş kalır. Bu yüzden deltası tam olarak 2 sayfa olmalıdır.
    let unmap_ok = unmap == 0
        && proc_mgr
            .get_process(pid)
            .is_some_and(|p| p.address_space.translate(region).is_none())
        && free_after_unmap == free_before_unmap + 2 * PAGE_SIZE;
    let unmap_twice = syscall_dispatch(sys_nos::SYS_MUNMAP, region, 2 * 4096, 0, proc_mgr, pmm);
    let mmap_ok = region_pages && second_ok && unmap_ok && unmap_twice == u64::MAX;

    // 4. MEMINFO syscall'u (R10 yolu) özeti doğru raporlar.
    let heap_address = proc_mgr
        .get_process(pid)
        .map(|p| p.heap_address(0))
        .unwrap_or(0);
    let report_addr = proc_mgr
        .get_process(pid)
        .and_then(|p| p.address_space.translate_user(second))
        .map(|_| second)
        .unwrap_or(0);
    let meminfo = syscall_dispatch(sys_nos::SYS_MEMINFO, report_addr, 0, 0, proc_mgr, pmm);
    let meminfo_ok = report_addr != 0
        && meminfo == 0
        && proc_mgr.get_process(pid).is_some_and(|p| {
            let heap = p
                .address_space
                .translate_user(report_addr)
                .map(|phys| unsafe {
                    core::ptr::read_volatile(crate::drivers::phys_to_virt(phys) as *const u64)
                })
                .unwrap_or(u64::MAX);
            heap == 2 * PAGE_SIZE && heap_address != 0
        });

    // 5. ASLR: iki süreç farklı mmap tabanı seçmeli (entropi testi).
    let mut bases = alloc::vec::Vec::new();
    bases.push(proc_mgr.get_process(pid).map(|p| p.mmap_base).unwrap_or(0));
    let mut aslr_pids = alloc::vec::Vec::new();
    for _ in 0..6u64 {
        if let Ok(other) = proc_mgr.spawn("aslr", 0x1000, 0x7000_0000, 0, None, pmm) {
            if let Some(p) = proc_mgr.get_process(other) {
                bases.push(p.mmap_base);
            }
            aslr_pids.push(other);
        }
    }
    let aslr_distinct = {
        let mut seen = alloc::vec::Vec::new();
        for base in &bases {
            if !seen.contains(base) {
                seen.push(*base);
            }
        }
        seen.len() > 1 && bases.iter().all(|b| b % 0x20_0000 == 0)
    };
    // ASLR yığını: rastgele üst seçimi kullanıcı yığınından aşağı, hizalı olmalı.
    let stack_tops: alloc::vec::Vec<u64> = (0..8)
        .map(|_| crate::mm::loader::randomized_stack_top())
        .collect();
    let stack_ok = stack_tops.iter().all(|top| {
        *top <= crate::mm::loader::USER_STACK_TOP
            && *top > crate::mm::loader::USER_STACK_TOP - crate::mm::loader::ASLR_STACK_MAX_SLIDE
            && top % PAGE_SIZE == 0
    }) && stack_tops.iter().any(|top| *top != stack_tops[0]);

    for other in aslr_pids {
        let _ = proc_mgr.terminate(other, 0, pmm);
    }

    proc_mgr.current_pid = 0;
    let terminated = proc_mgr.terminate(pid, 0, pmm);
    proc_mgr.reap_terminated(pmm);
    proc_mgr.current_pid = caller_pid;
    let no_leak = pmm.free_memory_bytes == free_before
        && crate::mm::vmm::shared_frame_count() == 0
        && terminated;

    if heap_ok
        && heap_write_ok
        && heap_bounds_ok
        && shrink_ok
        && mmap_ok
        && meminfo_ok
        && aslr_distinct
        && stack_ok
        && no_leak
    {
        crate::serial_print(
            "[USER-MEM] heap/mmap/ASLR: brk buyut+kucult, mmap/munmap, MEMINFO ve sizintisiz iade: BASARILI\n",
        );
        true
    } else {
        crate::serial_print("[USER-MEM] heap/mmap/ASLR testi: BASARISIZ (");
        crate::serial_print(if heap_ok { "heap " } else { "heap! " });
        crate::serial_print(if heap_write_ok { "hw " } else { "hw! " });
        crate::serial_print(if heap_bounds_ok { "hb " } else { "hb! " });
        crate::serial_print(if shrink_ok { "shrink " } else { "shrink! " });
        crate::serial_print(if region_pages { "region " } else { "region! " });
        crate::serial_print(if mmap_ok { "mmap " } else { "mmap! " });
        crate::serial_print(if second_ok { "2nd " } else { "2nd! " });
        crate::serial_print(if unmap_ok { "unmap " } else { "unmap! " });
        crate::serial_print(if meminfo_ok { "meminfo " } else { "meminfo! " });
        crate::serial_print(if aslr_distinct && stack_ok {
            "aslr "
        } else {
            "aslr! "
        });
        crate::serial_print(if no_leak { "leak-free" } else { "leak!" });
        crate::serial_print(")\n");
        false
    }
}

/// Dosya tanıtıcısı yaşam döngüsünü doğrular: açma, okuma, konumlandırma,
/// kapatma, `fork` devralma/refcount, `exec` sıfırlama ve fd kotası.
///
/// Kullanıcı tamponu gerçek bir heap sayfasıdır; yollar `uaccess` katmanından
/// geçtiği için test aynı zamanda güvenli kopyalama sözleşmesini de sınar.
pub fn test_file_descriptor_lifecycle(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print("[FD-TEST] --- Dosya tanıtıcısı yaşam döngüsü testi ---\n");
    let caller_pid = proc_mgr.current_pid;
    let free_before = pmm.free_memory_bytes;
    let open_before = proc_mgr.files.open_count();
    let refs_before = proc_mgr.files.reference_count();

    let Some(pid) = proc_mgr
        .spawn(
            "fdtest",
            crate::mm::loader::USER_CODE_BASE,
            crate::mm::loader::USER_STACK_TOP - 16,
            0,
            None,
            pmm,
        )
        .ok()
    else {
        crate::serial_print("[FD-TEST] Süreç oluşturulamadı: BAŞARISIZ\n");
        return false;
    };
    proc_mgr.current_pid = pid;

    // 1. 0/1/2 standart tanıtıcıları hazır olmalı.
    let std_fds_ok = proc_mgr.get_process(pid).is_some_and(|p| {
        p.fds[fd::FD_STDIN].used
            && p.fds[fd::FD_STDOUT].used
            && p.fds[fd::FD_STDERR].used
            && fd::used_fd_count(&p.fds) == 3
    });

    // 2. Heap'te yol ve okuma tamponu hazırla.
    let heap_end = syscall_dispatch(sys_nos::SYS_BRK, PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let heap_base = proc_mgr
        .get_process(pid)
        .map(|p| p.heap_address(0))
        .unwrap_or(0);
    let path_addr = heap_base;
    let buffer_addr = heap_base + 128;
    let path_ok = heap_base != 0
        && heap_end == heap_base + PAGE_SIZE
        && proc_mgr.get_process(pid).is_some_and(|p| {
            uaccess::copy_to_user(&p.address_space, path_addr, b"/etc/fd-test\0").is_ok()
        });

    // 3. open + read: içerik ve imleç ilerlemesi.
    let fd = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
    let object = proc_mgr
        .get_process(pid)
        .and_then(|p| p.fds.get(fd as usize))
        .filter(|entry| entry.used)
        .map(|entry| entry.object as usize);
    let size = object
        .and_then(|index| proc_mgr.files.object_size(index))
        .unwrap_or(0);
    let open_ok = fd == fd::FIRST_USER_FD as u64 && size > 0;
    // Güvenli kopyalama: NULL ve eşlenmemiş kullanıcı işaretçileri reddedilir.
    let null_open = syscall_dispatch(sys_nos::SYS_OPEN, 0, 0, 0, proc_mgr, pmm);
    let unmapped_open = syscall_dispatch(
        sys_nos::SYS_OPEN,
        heap_base + PAGE_SIZE,
        0,
        0,
        proc_mgr,
        pmm,
    );
    let null_read = syscall_dispatch_abi(sys_nos::SYS_READ, fd, 0, 4, 0, proc_mgr, pmm);
    let null_resinfo = syscall_dispatch(sys_nos::SYS_RESINFO, 0, 0, 0, proc_mgr, pmm);
    let safe_copy_ok = null_open == u64::MAX
        && unmapped_open == u64::MAX
        && null_read == u64::MAX
        && null_resinfo == u64::MAX;
    let read_count =
        syscall_dispatch_abi(sys_nos::SYS_READ, fd, buffer_addr, size, 0, proc_mgr, pmm);
    let read_ok = read_count == size
        && proc_mgr.get_process(pid).is_some_and(|p| {
            uaccess::copy_from_user(&p.address_space, buffer_addr, size as usize)
                .map(|bytes| bytes.starts_with(b"minOs file descriptor lifecycle test"))
                .unwrap_or(false)
        });

    // 4. lseek SET/CUR/END + aralık dışı reddi.
    let seek_set = syscall_dispatch(sys_nos::SYS_LSEEK, fd, 0, 0, proc_mgr, pmm);
    let seek_cur = syscall_dispatch(sys_nos::SYS_LSEEK, fd, 4, 1, proc_mgr, pmm);
    let seek_end = syscall_dispatch(sys_nos::SYS_LSEEK, fd, (-1i64) as u64, 2, proc_mgr, pmm);
    let seek_over = syscall_dispatch(sys_nos::SYS_LSEEK, fd, size + 1, 0, proc_mgr, pmm);
    let last_byte = syscall_dispatch_abi(
        sys_nos::SYS_READ,
        fd,
        buffer_addr + 256,
        1,
        0,
        proc_mgr,
        pmm,
    );
    let last_ok = proc_mgr.get_process(pid).is_some_and(|p| {
        uaccess::copy_from_user(&p.address_space, buffer_addr + 256, 1)
            .map(|bytes| bytes.len() == 1 && bytes[0] == b'\n')
            .unwrap_or(false)
    });
    let seek_ok = seek_set == 0
        && seek_cur == 4
        && seek_end == size.saturating_sub(1)
        && seek_over == u64::MAX
        && last_byte == 1
        && last_ok;

    // 5. close: fd serbest kalır, sonraki okuma reddedilir.
    let close_result = syscall_dispatch(sys_nos::SYS_CLOSE, fd, 0, 0, proc_mgr, pmm);
    let read_closed = syscall_dispatch_abi(sys_nos::SYS_READ, fd, buffer_addr, 4, 0, proc_mgr, pmm);
    let close_ok = close_result == 0
        && read_closed == u64::MAX
        && proc_mgr
            .get_process(pid)
            .is_some_and(|p| fd::used_fd_count(&p.fds) == 3);

    // 6. Aynı yol iki kez açılırsa tek nesne + iki referans.
    let dup_a = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
    let dup_b = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
    let dup_object = proc_mgr
        .get_process(pid)
        .and_then(|p| p.fds.get(dup_a as usize))
        .filter(|entry| entry.used)
        .map(|entry| entry.object as usize);
    let refs_two = dup_object
        .and_then(|index| proc_mgr.files.objects.get(index))
        .map(|entry| entry.refs)
        .unwrap_or(0);
    let duplicate_open_ok = dup_a == fd::FIRST_USER_FD as u64
        && dup_b == (fd::FIRST_USER_FD + 1) as u64
        && refs_two == 2
        && dup_object
            .is_some_and(|index| proc_mgr.files.objects[index].path_str() == Some("/etc/fd-test"));
    let close_dup_b = syscall_dispatch(sys_nos::SYS_CLOSE, dup_b, 0, 0, proc_mgr, pmm);
    let refs_one = dup_object
        .and_then(|index| proc_mgr.files.objects.get(index))
        .map(|entry| entry.refs)
        .unwrap_or(0);
    let close_dup_a = syscall_dispatch(sys_nos::SYS_CLOSE, dup_a, 0, 0, proc_mgr, pmm);
    let refs_zero = dup_object
        .and_then(|index| proc_mgr.files.objects.get(index))
        .map(|entry| entry.refs)
        .unwrap_or(0);
    let duplicate_close_ok =
        close_dup_b == 0 && refs_one == 1 && close_dup_a == 0 && refs_zero == 0;

    // 7. fork: fd tablosunu devralır ve nesne referansı iki olur.
    let parent_fd = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
    let parent_object = proc_mgr
        .get_process(pid)
        .and_then(|p| p.fds.get(parent_fd as usize))
        .filter(|entry| entry.used)
        .map(|entry| entry.object as usize);
    let child = proc_mgr.fork_process(pid, pmm).ok();
    let fork_inherit_ok = child.is_some_and(|child_pid| {
        proc_mgr.get_process(child_pid).is_some_and(|p| {
            p.fds
                .get(parent_fd as usize)
                .is_some_and(|entry| entry.used)
                && p.fds[parent_fd as usize].object as usize == parent_object.unwrap_or(usize::MAX)
        }) && parent_object
            .and_then(|index| proc_mgr.files.objects.get(index))
            .is_some_and(|entry| entry.refs == 2)
    });
    let mut child_close_ok = false;
    if let Some(child_pid) = child {
        proc_mgr.current_pid = child_pid;
        let closed = syscall_dispatch(sys_nos::SYS_CLOSE, parent_fd, 0, 0, proc_mgr, pmm);
        child_close_ok = closed == 0
            && proc_mgr
                .get_process(pid)
                .is_some_and(|p| p.fds[parent_fd as usize].used)
            && parent_object
                .and_then(|index| proc_mgr.files.objects.get(index))
                .is_some_and(|entry| entry.refs == 1);
        proc_mgr.current_pid = 0;
        let _ = proc_mgr.terminate(child_pid, 0, pmm);
    }
    let _ = syscall_dispatch(sys_nos::SYS_CLOSE, parent_fd, 0, 0, proc_mgr, pmm);
    proc_mgr.current_pid = pid;

    // 8. exec: tüm kullanıcı fd'leri kapanır, 0/1/2 yeniden kurulur.
    let _exec_fd = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
    let exec_image = crate::mm::loader::AppLoader::create_sample_elf();
    let exec_result = proc_mgr.exec_process(pid, &exec_image, pmm);
    let exec_ok = exec_result.is_ok()
        && proc_mgr.get_process(pid).is_some_and(|p| {
            fd::used_fd_count(&p.fds) == 3
                && p.fds
                    .iter()
                    .skip(fd::FIRST_USER_FD)
                    .all(|entry| !entry.used)
                && p.fds[fd::FD_STDOUT].used
        })
        && proc_mgr.files.reference_count() == refs_before;

    // 9. fd kotası: limit 4 iken 3 standart + 1 açık dosya sığar.
    let quota_heap = syscall_dispatch(sys_nos::SYS_BRK, PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let quota_base = proc_mgr
        .get_process(pid)
        .map(|p| p.heap_address(0))
        .unwrap_or(0);
    let quota_path_ok = quota_base != 0
        && quota_heap == quota_base + PAGE_SIZE
        && proc_mgr.get_process(pid).is_some_and(|p| {
            uaccess::copy_to_user(&p.address_space, quota_base, b"/etc/fd-test\0").is_ok()
        });
    let mut quota_ok = false;
    if quota_path_ok {
        let saved_limit = proc_mgr.get_process_mut(pid).map(|p| {
            let old = p.resources.limits.max_fds;
            p.resources.limits.max_fds = 4;
            old
        });
        if let Some(saved_limit) = saved_limit {
            let first = syscall_dispatch(sys_nos::SYS_OPEN, quota_base, 0, 0, proc_mgr, pmm);
            let second = syscall_dispatch(sys_nos::SYS_OPEN, quota_base, 0, 0, proc_mgr, pmm);
            let closed = syscall_dispatch(sys_nos::SYS_CLOSE, first, 0, 0, proc_mgr, pmm);
            quota_ok = first == fd::FIRST_USER_FD as u64 && second == u64::MAX && closed == 0;
            if let Some(p) = proc_mgr.get_process_mut(pid) {
                p.resources.limits.max_fds = saved_limit;
            }
        }
    }

    proc_mgr.current_pid = 0;
    let terminated = proc_mgr.terminate(pid, 0, pmm);
    proc_mgr.reap_terminated(pmm);
    proc_mgr.current_pid = caller_pid;
    let no_leak = terminated
        && pmm.free_memory_bytes == free_before
        && proc_mgr.files.reference_count() == refs_before
        && proc_mgr.files.open_count() <= open_before
        && crate::mm::vmm::shared_frame_count() == 0;
    if !no_leak {
        crate::serial_print("[FD-TEST] cleanup-dbg terminated=");
        crate::serial::print_dec(terminated as u64);
        crate::serial_print(" free_before=");
        crate::serial::print_dec(free_before);
        crate::serial_print(" free_now=");
        crate::serial::print_dec(pmm.free_memory_bytes);
        crate::serial_print(" refs_before=");
        crate::serial::print_dec(refs_before as u64);
        crate::serial_print(" refs_now=");
        crate::serial::print_dec(proc_mgr.files.reference_count() as u64);
        crate::serial_print(" open_before=");
        crate::serial::print_dec(open_before as u64);
        crate::serial_print(" open_now=");
        crate::serial::print_dec(proc_mgr.files.open_count() as u64);
        crate::serial_print(" shared=");
        crate::serial::print_dec(crate::mm::vmm::shared_frame_count() as u64);
        crate::serial_print("\n");
    }

    if std_fds_ok
        && path_ok
        && open_ok
        && safe_copy_ok
        && read_ok
        && seek_ok
        && close_ok
        && duplicate_open_ok
        && duplicate_close_ok
        && fork_inherit_ok
        && child_close_ok
        && exec_ok
        && quota_ok
        && no_leak
    {
        crate::serial_print(
            "[FD-TEST] open/read/lseek/close, refcount, fork/exec ve fd kotası: BASARILI\n",
        );
        true
    } else {
        crate::serial_print("[FD-TEST] fd yaşam döngüsü testi: BAŞARISIZ (");
        crate::serial_print(if std_fds_ok { "std " } else { "std! " });
        crate::serial_print(if path_ok { "path " } else { "path! " });
        crate::serial_print(if open_ok { "open " } else { "open! " });
        crate::serial_print(if safe_copy_ok {
            "safe-copy "
        } else {
            "safe-copy! "
        });
        crate::serial_print(if read_ok { "read " } else { "read! " });
        crate::serial_print(if seek_ok { "seek " } else { "seek! " });
        crate::serial_print(if close_ok { "close " } else { "close! " });
        crate::serial_print(if duplicate_open_ok {
            "dup-open "
        } else {
            "dup-open! "
        });
        crate::serial_print(if duplicate_close_ok {
            "dup-close "
        } else {
            "dup-close! "
        });
        crate::serial_print(if fork_inherit_ok { "fork " } else { "fork! " });
        crate::serial_print(if child_close_ok {
            "child-close "
        } else {
            "child-close! "
        });
        crate::serial_print(if exec_ok { "exec " } else { "exec! " });
        crate::serial_print(if quota_ok { "quota " } else { "quota! " });
        crate::serial_print(if no_leak { "leak-free" } else { "leak!" });
        crate::serial_print(")\n");
        false
    }
}

/// Süreç başına kaynak limitlerini doğrular: `RESINFO` raporu, heap/mmap/fd/
/// çerçeve kotası ve `fork` ile limit mirası.
pub fn test_resource_limits(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print("[RLIMIT-TEST] --- Kaynak limitleri testi ---\n");
    let caller_pid = proc_mgr.current_pid;
    let free_before = pmm.free_memory_bytes;
    let open_before = proc_mgr.files.open_count();
    let refs_before = proc_mgr.files.reference_count();

    let Some(pid) = proc_mgr
        .spawn(
            "rlimit",
            crate::mm::loader::USER_CODE_BASE,
            crate::mm::loader::USER_STACK_TOP - 16,
            0,
            None,
            pmm,
        )
        .ok()
    else {
        crate::serial_print("[RLIMIT-TEST] Süreç oluşturulamadı: BAŞARISIZ\n");
        return false;
    };
    proc_mgr.current_pid = pid;

    // 1. Varsayılan limitler.
    let defaults_ok = proc_mgr.get_process(pid).is_some_and(|p| {
        let limits = p.resources.limits;
        limits.max_frames == MAX_PROC_FRAMES
            && limits.max_user_frames == MAX_PROC_USER_FRAMES
            && limits.max_fds == fd::MAX_FDS
            && limits.max_mmap_bytes == MAX_PROC_MMAP_BYTES
            && limits.max_heap_bytes == MAX_PROC_HEAP_BYTES
    });

    // 2. RESINFO: 8 x u64 rapor (limit + kullanım).
    let heap_end = syscall_dispatch(sys_nos::SYS_BRK, PAGE_SIZE, 0, 0, proc_mgr, pmm);
    let heap_base = proc_mgr
        .get_process(pid)
        .map(|p| p.heap_address(0))
        .unwrap_or(0);
    let report_ok = heap_base != 0
        && heap_end == heap_base + PAGE_SIZE
        && syscall_dispatch(sys_nos::SYS_RESINFO, heap_base, 0, 0, proc_mgr, pmm) == 0
        && proc_mgr.get_process(pid).is_some_and(|p| {
            uaccess::copy_from_user(&p.address_space, heap_base, 64)
                .map(|bytes| {
                    let value = |index: usize| {
                        let mut raw = [0u8; 8];
                        raw.copy_from_slice(&bytes[index * 8..index * 8 + 8]);
                        u64::from_le_bytes(raw)
                    };
                    value(0) == MAX_PROC_FRAMES as u64
                        && value(1) == p.resources.frame_count as u64
                        && value(2) == MAX_PROC_USER_FRAMES as u64
                        && value(3) == p.resources.user_frame_count as u64
                        && value(4) == fd::MAX_FDS as u64
                        && value(5) == fd::used_fd_count(&p.fds) as u64
                        && value(6) == MAX_PROC_MMAP_BYTES
                        && value(7) == 0
                })
                .unwrap_or(false)
        });

    // 3. Heap kotası: limit 2 sayfa iken 3 sayfa reddedilir, 2 sayfa kabul edilir.
    let heap_limit_ok = {
        let saved = proc_mgr.get_process_mut(pid).map(|p| {
            let old = p.resources.limits.max_heap_bytes;
            p.resources.limits.max_heap_bytes = 2 * PAGE_SIZE;
            old
        });
        if let Some(saved) = saved {
            let over = syscall_dispatch(sys_nos::SYS_BRK, 3 * PAGE_SIZE, 0, 0, proc_mgr, pmm);
            let stayed = proc_mgr
                .get_process(pid)
                .is_some_and(|p| p.heap_brk == PAGE_SIZE);
            let within = syscall_dispatch(sys_nos::SYS_BRK, 2 * PAGE_SIZE, 0, 0, proc_mgr, pmm);
            let grew = proc_mgr
                .get_process(pid)
                .is_some_and(|p| p.heap_brk == 2 * PAGE_SIZE);
            let shrunk = syscall_dispatch(sys_nos::SYS_BRK, PAGE_SIZE, 0, 0, proc_mgr, pmm);
            if let Some(p) = proc_mgr.get_process_mut(pid) {
                p.resources.limits.max_heap_bytes = saved;
            }
            over == u64::MAX
                && stayed
                && within == heap_base + 2 * PAGE_SIZE
                && grew
                && shrunk == heap_base + PAGE_SIZE
        } else {
            false
        }
    };

    // 4. mmap kotası canlı bölge baytlarını sayar; munmap kotayı geri açar.
    let mmap_limit_ok = {
        let saved = proc_mgr.get_process_mut(pid).map(|p| {
            let old = p.resources.limits.max_mmap_bytes;
            p.resources.limits.max_mmap_bytes = 2 * PAGE_SIZE;
            old
        });
        if let Some(saved) = saved {
            let region = syscall_dispatch(sys_nos::SYS_MMAP, 2 * PAGE_SIZE, 0, 0, proc_mgr, pmm);
            let over = syscall_dispatch(sys_nos::SYS_MMAP, PAGE_SIZE, 0, 0, proc_mgr, pmm);
            let unmapped = if region != 0 {
                syscall_dispatch(sys_nos::SYS_MUNMAP, region, 2 * PAGE_SIZE, 0, proc_mgr, pmm)
            } else {
                u64::MAX
            };
            let region2 = syscall_dispatch(sys_nos::SYS_MMAP, PAGE_SIZE, 0, 0, proc_mgr, pmm);
            let unmapped2 = if region2 != 0 {
                syscall_dispatch(sys_nos::SYS_MUNMAP, region2, PAGE_SIZE, 0, proc_mgr, pmm)
            } else {
                u64::MAX
            };
            if let Some(p) = proc_mgr.get_process_mut(pid) {
                p.resources.limits.max_mmap_bytes = saved;
            }
            region != 0
                && over == 0
                && unmapped == 0
                && region2 != 0
                && region2 != region
                && unmapped2 == 0
        } else {
            false
        }
    };

    // 5. Çerçeve kotası: mevcut kullanım + 2 çerçeve; üçüncü istek reddedilir.
    let frame_limit_ok = {
        let state = proc_mgr.get_process_mut(pid).map(|p| {
            let old = p.resources.limits.max_user_frames;
            p.resources.limits.max_user_frames = p.resources.user_frame_count + 2;
            (old, p.resources.user_frame_count)
        });
        if let Some((saved, current)) = state {
            let first = syscall_dispatch(sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, proc_mgr, pmm);
            let second = syscall_dispatch(sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, proc_mgr, pmm);
            let third = syscall_dispatch(sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, proc_mgr, pmm);
            let free_first = if first != 0 {
                syscall_dispatch(sys_nos::SYS_FREE_FRAME, first, 0, 0, proc_mgr, pmm)
            } else {
                u64::MAX
            };
            let free_second = if second != 0 {
                syscall_dispatch(sys_nos::SYS_FREE_FRAME, second, 0, 0, proc_mgr, pmm)
            } else {
                u64::MAX
            };
            let restored = proc_mgr
                .get_process(pid)
                .is_some_and(|p| p.resources.user_frame_count == current);
            if let Some(p) = proc_mgr.get_process_mut(pid) {
                p.resources.limits.max_user_frames = saved;
            }
            first != 0
                && second != 0
                && first != second
                && third == 0
                && free_first == 0
                && free_second == 0
                && restored
        } else {
            false
        }
    };

    // 6. fd kotası: limit 4 iken yalnızca bir kullanıcı fd'si açılabilir.
    let fd_limit_ok = {
        let path_addr = heap_base + 256;
        let path_ok = heap_base != 0
            && proc_mgr.get_process(pid).is_some_and(|p| {
                uaccess::copy_to_user(&p.address_space, path_addr, b"/etc/fd-test\0").is_ok()
            });
        if !path_ok {
            false
        } else if let Some(saved) = proc_mgr.get_process_mut(pid).map(|p| {
            let old = p.resources.limits.max_fds;
            p.resources.limits.max_fds = 4;
            old
        }) {
            let first = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
            let second = syscall_dispatch(sys_nos::SYS_OPEN, path_addr, 0, 0, proc_mgr, pmm);
            let closed = syscall_dispatch(sys_nos::SYS_CLOSE, first, 0, 0, proc_mgr, pmm);
            if let Some(p) = proc_mgr.get_process_mut(pid) {
                p.resources.limits.max_fds = saved;
            }
            first == fd::FIRST_USER_FD as u64 && second == u64::MAX && closed == 0
        } else {
            false
        }
    };

    // 7. fork limitleri devralır (son adım: sonrasında kullanıcı belleğine
    // yazılmaz, çünkü fork sayfaları COW olarak işaretler).
    let mut child_pid = None;
    let inheritance_ok = if let Some(parent_limits) = proc_mgr.get_process_mut(pid).map(|p| {
        p.resources.limits.max_mmap_bytes = 5 * PAGE_SIZE;
        p.resources.limits
    }) {
        child_pid = proc_mgr.fork_process(pid, pmm).ok();
        child_pid.is_some_and(|child| {
            proc_mgr
                .get_process(child)
                .is_some_and(|p| p.resources.limits == parent_limits)
        })
    } else {
        false
    };

    proc_mgr.current_pid = 0;
    if let Some(child) = child_pid {
        let _ = proc_mgr.terminate(child, 0, pmm);
    }
    let terminated = proc_mgr.terminate(pid, 0, pmm);
    proc_mgr.reap_terminated(pmm);
    proc_mgr.current_pid = caller_pid;
    let no_leak = terminated
        && pmm.free_memory_bytes == free_before
        && proc_mgr.files.reference_count() == refs_before
        && proc_mgr.files.open_count() <= open_before
        && crate::mm::vmm::shared_frame_count() == 0;

    if defaults_ok
        && report_ok
        && heap_limit_ok
        && mmap_limit_ok
        && frame_limit_ok
        && fd_limit_ok
        && inheritance_ok
        && no_leak
    {
        crate::serial_print(
            "[RLIMIT-TEST] RESINFO, heap/mmap/fd/çerçeve kotası ve fork mirası: BASARILI\n",
        );
        true
    } else {
        crate::serial_print("[RLIMIT-TEST] kaynak limitleri testi: BAŞARISIZ (");
        crate::serial_print(if defaults_ok {
            "defaults "
        } else {
            "defaults! "
        });
        crate::serial_print(if report_ok { "resinfo " } else { "resinfo! " });
        crate::serial_print(if heap_limit_ok { "heap " } else { "heap! " });
        crate::serial_print(if mmap_limit_ok { "mmap " } else { "mmap! " });
        crate::serial_print(if frame_limit_ok { "frame " } else { "frame! " });
        crate::serial_print(if fd_limit_ok { "fd " } else { "fd! " });
        crate::serial_print(if inheritance_ok {
            "inherit "
        } else {
            "inherit! "
        });
        crate::serial_print(if no_leak { "leak-free" } else { "leak!" });
        crate::serial_print(")\n");
        false
    }
}

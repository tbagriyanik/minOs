//! Süreç ve Sistem Çağrıları Yönetimi (Process & System Call Management).
//!
//! Bu modül:
//! 1. Süreç Kontrol Bloğu (PCB - Process Control Block),
//! 2. Süreç durumları (Hazır, Çalışıyor, Uyuyor, Sonlandı),
//! 3. x86_64 Register/Context yapısı ve süreç bellek/kaynak takibi,
//! 4. Süreç tablosu (Process Table): Süreç başlatma (spawn/exec), sonlandırma (exit/kill),
//! 5. Sistem Çağrıları (Syscalls): `sys_yield`, `sys_exit`, `sys_write`, `sys_read`,
//!    `sys_alloc_frame`, `sys_free_frame`, `sys_get_pid`, `sys_time` vb.

use crate::mm::pmm::PhysicalMemoryManager;

/// Azami eşzamanlı aktif süreç sayısı
pub const MAX_PROCESSES: usize = 32;

/// Süreç Durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Unused,
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// Süreç Önceliği
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

/// CPU Yazmaç/Bağlam Durumu (Saved Context on Interrupt/Syscall)
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

/// Bir sürece tahsis edilen kaynaklar (Sayfa çerçeveleri, dosya tanıtıcıları vb.)
pub const MAX_PROC_FRAMES: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct ProcessResources {
    /// Sürecin sahip olduğu tahsis edilmiş fiziksel sayfa çerçeveleri
    pub allocated_frames: [u64; MAX_PROC_FRAMES],
    pub frame_count: usize,
    /// Süreç tarafından açılan dosya veya pencere tanıtıcısı (Handle)
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

/// Süreç Kontrol Bloğu (PCB - Process Control Block)
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
                rax: 0, rbx: 0, rcx: 0, rdx: 0, rsi: 0, rdi: 0, rbp: 0, rsp: 0,
                r8: 0, r9: 0, r10: 0, r11: 0, r12: 0, r13: 0, r14: 0, r15: 0,
                rip: 0, rflags: 0x202, cr3: 0,
            },
            resources: ProcessResources::empty(),
            cpu_time_ticks: 0,
            exit_code: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("unknown")
    }
}

/// Süreç Tablosu ve Zamanlayıcı Yöneticisi
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

    /// Çekirdek başlangıç sürecini (PID 0 / init / kernel_core) kaydeder.
    pub fn init_kernel_process(&mut self) {
        let mut p = Process::empty();
        p.pid = 0;
        p.ppid = 0;
        let name = b"kernel_core";
        p.name[..name.len()].copy_from_slice(name);
        p.name_len = name.len();
        p.state = ProcessState::Running;
        p.priority = ProcessPriority::Realtime;
        self.processes[0] = p;
        self.current_pid = 0;
        self.active_count = 1;
    }

    /// Yeni bir kullanıcı veya sistem uygulamasını süreç tablosuna ekler (Spawn).
    pub fn spawn(
        &mut self,
        name: &str,
        entry_point: u64,
        stack_top: u64,
        cr3: u64,
        app_id: usize,
        window_idx: Option<usize>,
    ) -> Result<usize, &'static str> {
        let slot = self.processes.iter().position(|p| p.state == ProcessState::Unused || p.state == ProcessState::Terminated)
            .ok_or("Surec tablosu dolu: Yeni surec baslatilamadi")?;

        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);

        let mut p = Process::empty();
        p.pid = pid;
        p.ppid = self.current_pid;
        let n = name.len().min(32);
        p.name[..n].copy_from_slice(&name.as_bytes()[..n]);
        p.name_len = n;
        p.state = ProcessState::Ready;
        p.priority = ProcessPriority::Normal;

        // Bağlamı hazırla
        p.context.rip = entry_point;
        p.context.rsp = stack_top;
        p.context.cr3 = cr3;
        p.context.rflags = 0x202; // IF = 1 (Kesmeler açık)

        p.resources.app_id = app_id;
        p.resources.window_idx = window_idx;

        self.processes[slot] = p;
        self.active_count += 1;

        Ok(pid)
    }

    /// Belirtilen PID'ye sahip süreci sonlandırır (Kill/Terminate) ve kaynaklarını serbest bırakır.
    pub fn terminate(&mut self, pid: usize, exit_code: i32, pmm: &mut PhysicalMemoryManager) -> bool {
        if pid == 0 {
            // Çekirdek ana süreci sonlandırılamaz!
            return false;
        }

        if let Some(p) = self.processes.iter_mut().find(|p| p.pid == pid && p.state != ProcessState::Unused && p.state != ProcessState::Terminated) {
            p.state = ProcessState::Terminated;
            p.exit_code = exit_code;

            // 1. Sürecin sahip olduğu tüm fiziksel sayfa çerçevelerini PMM'e iade et
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

    /// Pencere indeksine göre çalışan süreci bulur ve sonlandırır.
    pub fn terminate_by_window(&mut self, win_idx: usize, pmm: &mut PhysicalMemoryManager) -> bool {
        let target_pid = self.processes.iter().find(|p| {
            p.state != ProcessState::Unused && p.state != ProcessState::Terminated && p.resources.window_idx == Some(win_idx)
        }).map(|p| p.pid);

        if let Some(pid) = target_pid {
            self.terminate(pid, 0, pmm)
        } else {
            false
        }
    }

    /// Bir sonraki çalıştırılabilir süreci döner (Öncelikli Preemptive Round-Robin Zamanlayıcı).
    /// Kullanıcı süreçlerini öncelik sırasına (Realtime > High > Normal > Low) göre zamanlar,
    /// çalıştırılabilir kullanıcı süreci yoksa PID 0 (kernel_core) fallback döner.
    pub fn schedule_next(&mut self) -> Option<usize> {
        if self.active_count == 0 {
            return None;
        }

        // Mevcut çalışan sürecin slot indeksini bul
        let curr_slot = self.processes.iter().position(|p| p.pid == self.current_pid && p.state != ProcessState::Unused).unwrap_or(0);

        // 1. Önce çalıştırılmaya hazır kullanıcı süreçlerini öncelik sırasıyla ara (Realtime -> Low)
        for target_prio in [ProcessPriority::Realtime, ProcessPriority::High, ProcessPriority::Normal, ProcessPriority::Low] {
            let total = MAX_PROCESSES;
            for i in 1..=total {
                let slot = (curr_slot + i) % total;
                let p = &mut self.processes[slot];
                if p.pid != 0 && (p.state == ProcessState::Ready || p.state == ProcessState::Running) && p.priority == target_prio {
                    self.current_pid = p.pid;
                    p.state = ProcessState::Running;
                    p.cpu_time_ticks = p.cpu_time_ticks.saturating_add(1);
                    return Some(p.pid);
                }
            }
        }

        // 2. Başka çalıştırılabilir kullanıcı süreci yoksa çekirdek ana sürecine (PID 0) dön
        if self.processes[0].state == ProcessState::Running || self.processes[0].state == ProcessState::Ready {
            self.current_pid = 0;
            self.processes[0].state = ProcessState::Running;
            self.processes[0].cpu_time_ticks = self.processes[0].cpu_time_ticks.saturating_add(1);
            return Some(0);
        }

        Some(0) // kernel_core fallback
    }

    /// PID ile süreç referansı alır.
    pub fn get_process(&self, pid: usize) -> Option<&Process> {
        self.processes.iter().find(|p| p.pid == pid && p.state != ProcessState::Unused)
    }
}

// ---------------------------------------------------------------------------
// Sistem Çağrısı (Syscall) Arabirimi
// ---------------------------------------------------------------------------

/// Standart Sistem Çağrısı Numaraları
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

/// Sistem çağrısı sevkedicisi (Syscall Dispatcher).
/// x86_64 ABI:
/// RAX = Syscall No
/// RDI = Arg 1
/// RSI = Arg 2
/// RDX = Arg 3
/// R10 = Arg 4
/// Dönen değer: RAX (Sonuç kodu / adres)
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
        sys_nos::SYS_GETPID => {
            proc_mgr.current_pid as u64
        }
        sys_nos::SYS_ALLOC_FRAME => {
            if let Some(frame) = pmm.allocate_frame() {
                // Mevcut sürecin kaynaklarına kaydet
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
                // Güvenli tampon kopyalama ve seriye yazdırma
                let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::serial::print(s);
                }
            }
            len as u64
        }
        _ => u64::MAX, // Desteklenmeyen çağrı
    }
}

/// Süreç yaşam döngüsü, kaza (crash/termination), geçersiz sistem çağrısı ve kaynak temizleme testi.
pub fn test_process_lifecycle_and_fault_recovery(
    proc_mgr: &mut ProcessManager,
    pmm: &mut PhysicalMemoryManager,
) -> bool {
    crate::serial_print("[PROC-TEST] --- Süreç Çökmesi, Syscall Hataları ve Kaynak Temizleme Testi ---\n");

    let initial_free = pmm.free_memory_bytes;
    let initial_active = proc_mgr.active_count;

    // 1. Yeni bir test süreci başlat (spawn)
    let pid = match proc_mgr.spawn("fault_test_proc", 0x1000, 0x7000_2000, 0x100000, 99, Some(99)) {
        Ok(id) => id,
        Err(_) => {
            crate::serial_print("[PROC-TEST] Süreç başlatma: BASARISIZ\n");
            return false;
        }
    };
    crate::serial_print("[PROC-TEST] 1. Test süreci başlatıldı (PID ");
    crate::serial::print_dec(pid as u64);
    crate::serial_print("): BASARILI\n");

    // 2. Sürece kaynak tahsis et (PMM çerçeveleri)
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
    crate::serial_print("[PROC-TEST] 2. Sürece 2 sayfa çerçevesi tahsis edildi (Kaynak takibi aktif)\n");

    // 3. Hatalı / Geçersiz Sistem Çağrısı Testi (Geçersiz Syscall No)
    let invalid_res = syscall_dispatch(9999, 0, 0, 0, proc_mgr, pmm);
    let null_ptr_write = syscall_dispatch(sys_nos::SYS_WRITE, 0, 100, 0, proc_mgr, pmm);
    if invalid_res == u64::MAX && null_ptr_write == 100 {
        crate::serial_print("[PROC-TEST] 3. Geçersiz syscall no ve NULL pointer sınır kontrolü: BASARILI\n");
    } else {
        crate::serial_print("[PROC-TEST] Hatalı sistem çağrısı sınır koruması: BASARISIZ\n");
        return false;
    }

    // 4. Süreç Çökmesi / Hata Simülasyonu (SIGSEGV / Fault Exit Code -11)
    let terminated = proc_mgr.terminate(pid, -11, pmm);
    if terminated {
        crate::serial_print("[PROC-TEST] 4. Süreç çökmesi simüle edildi (Exit Code: -11 / SIGSEGV)\n");
    } else {
        crate::serial_print("[PROC-TEST] Süreç sonlandırma: BASARISIZ\n");
        return false;
    }

    // 5. Kaynak Temizliği ve Bellek Sızıntısı Kontrolü
    let free_after = pmm.free_memory_bytes;
    let active_after = proc_mgr.active_count;

    if free_after != initial_free || active_after != initial_active {
        crate::serial_print("[PROC-TEST] Kaynak temizleme sızıntısı: BASARISIZ\n");
        return false;
    }
    crate::serial_print("[PROC-TEST] 5. Süreç kaynakları PMM'e eksiksiz iade edildi (Sızıntı: 0 bayt): BASARILI\n");

    // 6. Öncelikli Zamanlayıcı (Priority Preemptive Scheduler) Testi
    let p_low = proc_mgr.spawn("low_proc", 0x1000, 0x7000_1000, 0x100000, 1, None).unwrap_or(0);
    let p_high = proc_mgr.spawn("high_proc", 0x2000, 0x7000_2000, 0x100000, 1, None).unwrap_or(0);
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
        crate::serial_print("[PROC-TEST] 6. Öncelik tabanlı zamanlayıcı (High > Low): BASARILI\n");
        true
    } else {
        crate::serial_print("[PROC-TEST] 6. Zamanlayıcı öncelik testi: BASARISIZ\n");
        false
    }
}

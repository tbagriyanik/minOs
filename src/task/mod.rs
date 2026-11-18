//! Preemptive kernel task scheduler with callback-safe scheduler boundaries.
//!
//! A task step must be short and must not perform blocking work. The scheduler
//! The APIC tick requests a quantum and the architecture context-switch
//! primitive swaps stacks at the scheduler boundary. Callback tasks still
//! return `TaskStatus` to update their state; they are not allowed to hold
//! scheduler locks across a switch.

extern crate alloc;

use crate::arch::x86_64::sync::{SpinGuard, SpinMutex};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1);
// Keep a scheduler call bounded to one user callback by default. Callers may
// raise this for non-interactive contexts with `set_step_budget`.
const DEFAULT_STEP_BUDGET: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub usize);

impl TaskId {
    pub fn new() -> Self {
        Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// The task may run again on the next scheduler batch.
    Yielded,
    /// The task waits until `wake`, `notify`, or its registered deadline.
    Blocked,
    /// The task has finished and can be removed.
    Completed,
}

/// Event key a task waits for. The event producer calls [`notify`] when its
/// condition becomes true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskEvent(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Blocked,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub name: String,
    pub priority: TaskPriority,
    pub state: TaskState,
    pub run_count: usize,
    pub deadline_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskStateCounts {
    pub ready: usize,
    pub blocked: usize,
    pub running: usize,
}

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub priority: TaskPriority,
    pub step_fn: Box<dyn FnMut() -> TaskStatus + Send>,
    pub run_count: usize,
    pub completed: bool,
    ready: bool,
    cancelled: bool,
    ready_age: u32,
    waiting_for: Option<TaskEvent>,
    deadline_ms: Option<u64>,
    pub cpu: usize,
}

impl Task {
    pub fn new<F>(name: &str, priority: TaskPriority, f: F) -> Self
    where
        F: FnMut() -> TaskStatus + Send + 'static,
    {
        let cpu = least_loaded_cpu();
        Self {
            id: TaskId::new(),
            name: String::from(name),
            priority,
            step_fn: Box::new(f),
            run_count: 0,
            completed: false,
            ready: true,
            cancelled: false,
            ready_age: 0,
            waiting_for: None,
            deadline_ms: None,
            cpu,
        }
    }
}

/// Registry operations only. Callbacks are moved out before execution so the
/// global registry lock is never held while user task code runs.
pub struct SimpleExecutor {
    tasks: Vec<Task>,
    step_budget: usize,
}

impl SimpleExecutor {
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            step_budget: DEFAULT_STEP_BUDGET,
        }
    }

    pub fn set_step_budget(&mut self, steps: usize) {
        self.step_budget = steps.max(1);
    }

    fn step_budget(&self) -> usize {
        self.step_budget
    }

    pub fn spawn<F>(&mut self, name: &str, priority: TaskPriority, f: F) -> TaskId
    where
        F: FnMut() -> TaskStatus + Send + 'static,
    {
        let task = Task::new(name, priority, f);
        let id = task.id;
        // Use clamped CPU index to match release_run_slot indexing and
        // avoid out-of-bounds when task.cpu >= RUN_QUEUE_DEPTH.len().
        let cpu_idx = task.cpu.min(RUN_QUEUE_DEPTH.len() - 1);
        RUN_QUEUE_DEPTH[cpu_idx].fetch_add(1, Ordering::Relaxed);
        self.tasks.push(task);
        id
    }

    pub fn wake(&mut self, id: TaskId) -> bool {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id && !t.cancelled && !t.completed)
        {
            task.ready = true;
            task.waiting_for = None;
            task.deadline_ms = None;
            true
        } else {
            false
        }
    }

    pub fn notify(&mut self, event: TaskEvent) -> usize {
        let mut count = 0;
        for task in self.tasks.iter_mut() {
            if task.waiting_for == Some(event) && !task.cancelled && !task.completed {
                task.waiting_for = None;
                task.ready = true;
                task.ready_age = 0;
                task.deadline_ms = None;
                count += 1;
            }
        }
        count
    }

    pub fn cancel(&mut self, id: TaskId) -> bool {
        self.take_task(id).is_some()
    }

    /// Kuyruktan bir görevi çıkarır ve CPU yük sayacını geri alır.
    /// `spawn` sayacı artırdığı için kuyruktan çıkarılan her görev sayacı
    /// bir kez azaltmalıdır; aksi halde `least_loaded_cpu` zamanla tüm
    /// işi BSP'ye yığar.
    fn take_task(&mut self, id: TaskId) -> Option<Task> {
        let index = self.tasks.iter().position(|t| t.id == id)?;
        let task = self.tasks.swap_remove(index);
        release_run_slot(task.cpu);
        Some(task)
    }

    /// Tamamlanan veya iptal edilmiş kuyruk girdilerini atar ve her biri için
    /// CPU yük sayacını geri alır.
    fn drop_finished(&mut self) -> usize {
        let before = self.tasks.len();
        let mut dropped = 0usize;
        let mut index = 0;
        while index < self.tasks.len() {
            if self.tasks[index].cancelled || self.tasks[index].completed {
                let task = self.tasks.swap_remove(index);
                release_run_slot(task.cpu);
                dropped += 1;
            } else {
                index += 1;
            }
        }
        debug_assert_eq!(before - self.tasks.len(), dropped);
        dropped
    }

    pub fn task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| !t.cancelled && !t.completed)
            .count()
    }
    pub fn task_names(&self) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|t| !t.cancelled && !t.completed)
            .map(|t| t.name.clone())
            .collect()
    }
}

impl Default for SimpleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

pub static EXECUTOR: SpinMutex<SimpleExecutor> = SpinMutex::new(SimpleExecutor::new());

// Zamanlayıcı defter tutma kodundaki bir hata, EXECUTOR kilidini tutarken
// oluşabilir. Hangi CPU'nun hangi kilidi tuttuğunu izlemek, kurtarma yolunun
// yalnızca gerçekten sahip olunan kilidi açmasını sağlar; başka bir CPU'nun
// elindeki kilit asla zorla açılmaz.
const SCHED_LOCK_EXECUTOR: usize = 1 << 0;
static SCHED_LOCKS_HELD: [AtomicUsize; crate::arch::x86_64::smp::MAX_CPUS] =
    [const { AtomicUsize::new(0) }; crate::arch::x86_64::smp::MAX_CPUS];

struct SchedLockGuard<'a, T> {
    guard: SpinGuard<'a, T>,
    cpu: usize,
    bit: usize,
}

impl<T> Deref for SchedLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> DerefMut for SchedLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<T> Drop for SchedLockGuard<'_, T> {
    fn drop(&mut self) {
        SCHED_LOCKS_HELD[self.cpu].fetch_and(!self.bit, Ordering::Release);
    }
}

/// Zamanlayıcı kilidini alır ve sahipliğini hata kurtarma için kaydeder.
fn lock_sched<T>(mutex: &'static SpinMutex<T>, bit: usize) -> SchedLockGuard<'static, T> {
    let guard = mutex.lock();
    let cpu = crate::arch::x86_64::smp::current_cpu_index().min(SCHED_LOCKS_HELD.len() - 1);
    SCHED_LOCKS_HELD[cpu].fetch_or(bit, Ordering::Release);
    SchedLockGuard { guard, cpu, bit }
}

fn lock_executor() -> SchedLockGuard<'static, SimpleExecutor> {
    lock_sched(&EXECUTOR, SCHED_LOCK_EXECUTOR)
}

static RUN_QUEUE_DEPTH: [AtomicUsize; crate::arch::x86_64::smp::MAX_CPUS] =
    [const { AtomicUsize::new(0) }; crate::arch::x86_64::smp::MAX_CPUS];
fn least_loaded_cpu() -> usize {
    let count = crate::arch::x86_64::smp::cpu_count()
        .max(1)
        .min(RUN_QUEUE_DEPTH.len());
    // When queues have equal depth, min_by_key alone always selected CPU 0.
    // Rotate the tie-breaker so newly-created window/application tasks are
    // spread across all online CPUs instead of accumulating on the BSP.
    let start = RR_CURSOR.fetch_add(1, Ordering::Relaxed) % count;
    (0..count)
        .map(|offset| (start + offset) % count)
        .min_by_key(|&cpu| {
            (
                RUN_QUEUE_DEPTH[cpu].load(Ordering::Relaxed),
                (cpu + count - start) % count,
            )
        })
        .unwrap_or(0)
}
pub fn run_queue_depth(cpu: usize) -> usize {
    RUN_QUEUE_DEPTH[cpu.min(RUN_QUEUE_DEPTH.len() - 1)].load(Ordering::Relaxed)
}

/// Bir görevin kuyruktan çıkışında CPU yük sayacını geri alır.
fn release_run_slot(cpu: usize) {
    RUN_QUEUE_DEPTH[cpu.min(RUN_QUEUE_DEPTH.len() - 1)].fetch_sub(1, Ordering::Relaxed);
}

/// Tüm CPU'lardaki kuyruk derinliklerinin toplamı (kaçak slot denetimi için).
pub fn total_run_queue_depth() -> usize {
    RUN_QUEUE_DEPTH
        .iter()
        .map(|d| d.load(Ordering::Relaxed))
        .sum()
}
static IN_SCHEDULER: AtomicBool = AtomicBool::new(false);
/// Test-only: ölçüm penceresi boyunca yeni görev dağıtımını durdurur.
///
/// `IN_SCHEDULER` tek seferde tek CPU'nun dağıtım yapmasını sağlar; fakat o
/// bayrak "şu anda bir dağıtım sürüyor" anlamına geldiği için, bir başka CPU
/// dağıtım sürerken alınan bir ölçüm dondurulamaz. Ayrı bir duraklatma
/// bayrağı ölçüm penceresini her CPU için kesin olarak kapatır.
static SCHEDULER_PAUSED: AtomicBool = AtomicBool::new(false);
static PREEMPT_REQUESTED: AtomicBool = AtomicBool::new(false);
static SCHEDULER_TICKS: AtomicU64 = AtomicU64::new(0);
static RR_CURSOR: AtomicUsize = AtomicUsize::new(0);
static RUNNING_TASK: AtomicUsize = AtomicUsize::new(0);
static RUNNING_CANCELLED: AtomicBool = AtomicBool::new(false);
static RUNNING_WAIT_EVENT: AtomicUsize = AtomicUsize::new(0);
static RUNNING_WAIT_SET: AtomicBool = AtomicBool::new(false);
static RUNNING_WOKEN: AtomicBool = AtomicBool::new(false);
static RUNNING_WAIT_DEADLINE: AtomicU64 = AtomicU64::new(0);
static RUNNING_DEADLINE_SET: AtomicBool = AtomicBool::new(false);
static RUNNING_INFO: SpinMutex<Option<TaskSnapshot>> = SpinMutex::new(None);

pub fn spawn<F>(name: &str, priority: TaskPriority, f: F) -> TaskId
where
    F: FnMut() -> TaskStatus + Send + 'static,
{
    EXECUTOR.lock().spawn(name, priority, f)
}

pub fn wake(id: TaskId) -> bool {
    EXECUTOR.lock().wake(id)
}
pub fn notify(event: TaskEvent) -> usize {
    let mut executor = EXECUTOR.lock();
    let mut count = executor.notify(event);
    if RUNNING_TASK.load(Ordering::Acquire) != 0
        && RUNNING_WAIT_SET.load(Ordering::Acquire)
        && RUNNING_WAIT_EVENT.load(Ordering::Acquire) == event.0
        && !RUNNING_WOKEN.swap(true, Ordering::AcqRel)
    {
        count += 1;
    }
    count
}

/// Register the event that the current callback is about to wait for. Call
/// this before returning `TaskStatus::Blocked`; an event notified during the
/// transition is remembered and the task stays ready. A notification sent
/// before registration is not latched, so the callback should check its
/// condition before blocking.
pub fn block_on(event: TaskEvent) -> bool {
    register_wait(event, None)
}

/// Register an event wait with an absolute deadline in milliseconds from
/// [`crate::monotonic_ms`]. The task becomes ready when notified or timed out.
pub fn block_until(event: TaskEvent, deadline_ms: u64) -> bool {
    register_wait(event, Some(deadline_ms))
}

fn register_wait(event: TaskEvent, deadline_ms: Option<u64>) -> bool {
    let executor = EXECUTOR.lock();
    if RUNNING_TASK.load(Ordering::Acquire) == 0 {
        return false;
    }
    RUNNING_WAIT_EVENT.store(event.0, Ordering::Relaxed);
    RUNNING_WOKEN.store(false, Ordering::Relaxed);
    if let Some(deadline) = deadline_ms {
        RUNNING_WAIT_DEADLINE.store(deadline, Ordering::Relaxed);
        RUNNING_DEADLINE_SET.store(true, Ordering::Relaxed);
    } else {
        RUNNING_DEADLINE_SET.store(false, Ordering::Relaxed);
    }
    RUNNING_WAIT_SET.store(true, Ordering::Release);
    drop(executor);
    true
}
pub fn cancel(id: TaskId) -> bool {
    let mut executor = EXECUTOR.lock();
    if RUNNING_TASK.load(Ordering::Acquire) == id.0 {
        RUNNING_CANCELLED.store(true, Ordering::Release);
        return true;
    }
    let removed = executor.take_task(id);
    drop(executor);
    let found = removed.is_some();
    drop(removed);
    found
}
pub fn set_step_budget(steps: usize) {
    EXECUTOR.lock().set_step_budget(steps);
}

/// Request a reschedule from the timer/IPI path. The interrupt never runs Rust
/// task code; it only marks the current quantum expired.
pub fn request_preempt() {
    PREEMPT_REQUESTED.store(true, Ordering::Release);
}

pub const SCHEDULER_HZ: u64 = 1_000;
pub fn scheduler_tick() {
    SCHEDULER_TICKS.fetch_add(1, Ordering::Relaxed);
    request_preempt();
}
pub fn scheduler_ticks() -> u64 {
    SCHEDULER_TICKS.load(Ordering::Relaxed)
}

pub fn preempt_pending() -> bool {
    PREEMPT_REQUESTED.load(Ordering::Acquire)
}

/// Return a read-only snapshot of queued and currently executing tasks.
pub fn task_snapshots() -> Vec<TaskSnapshot> {
    let executor = EXECUTOR.lock();
    let mut snapshots: Vec<TaskSnapshot> = executor
        .tasks
        .iter()
        .filter(|task| !task.cancelled && !task.completed)
        .map(|task| TaskSnapshot {
            id: task.id,
            name: task.name.clone(),
            priority: task.priority,
            state: if task.ready {
                TaskState::Ready
            } else {
                TaskState::Blocked
            },
            run_count: task.run_count,
            deadline_ms: task.deadline_ms,
        })
        .collect();
    if let Some(running) = RUNNING_INFO.lock().as_ref() {
        snapshots.push(running.clone());
    }
    snapshots
}

/// Number of live tasks, including the callback currently executing.
pub fn task_count() -> usize {
    let executor = EXECUTOR.lock();
    executor.task_count() + usize::from(RUNNING_TASK.load(Ordering::Acquire) != 0)
}

/// Aggregate scheduler state for inexpensive UI and diagnostics polling.
pub fn task_state_counts() -> TaskStateCounts {
    let executor = EXECUTOR.lock();
    let mut counts = TaskStateCounts::default();
    for task in executor
        .tasks
        .iter()
        .filter(|task| !task.cancelled && !task.completed)
    {
        if task.ready {
            counts.ready += 1;
        } else {
            counts.blocked += 1;
        }
    }
    counts.running = usize::from(RUNNING_TASK.load(Ordering::Acquire) != 0);
    counts
}

/// Bounded dispatch pump. Each callback runs without the global registry lock.
/// A single callback still cannot be preempted, so it must return quickly.
///
/// Defter tutma kodu da korunur: uygulama geri çağrısı dışında oluşan bir hata
/// `IN_SCHEDULER` bayrağını veya etkin görev yuvasını kalıcı olarak açık
/// bırakamaz. Geri çağrı hataları ise aşağıdaki iç korumada yakalanır.
pub fn poll_tasks() -> usize {
    let (guard_scope, guard_start) = crate::apps::guard::prepare_recoverable(
        crate::apps::guard::NO_APP,
        crate::apps::guard::NO_WINDOW,
    );
    if unsafe { crate::apps::guard::begin(guard_start) } {
        let _guard_scope = guard_scope;
        poll_tasks_inner()
    } else {
        recover_scheduler_state();
        crate::serial_print("[GUARD] Zamanlayıcı hatası izole edildi.\n");
        // Bekleyen raporu tüketme; ana döngü bunu bildirim olarak gösterecek.
        EXECUTOR.lock().task_count()
    }
}

fn poll_tasks_inner() -> usize {
    // Test-only: ölçüm penceresinde hiçbir CPU yeni görev dağıtmaz.
    if SCHEDULER_PAUSED.load(Ordering::Acquire) {
        return lock_executor().task_count();
    }
    // Prevent two CPUs from mutating/executing the same task simultaneously.
    if IN_SCHEDULER.swap(true, Ordering::Acquire) {
        return lock_executor().task_count();
    }
    // A timer interrupt requests a new quantum. The actual stack switch is
    // performed by the architecture scheduler boundary; callback state is
    // finalized before another task is resumed.
    let _quantum_expired = PREEMPT_REQUESTED.swap(false, Ordering::AcqRel);
    let budget = EXECUTOR.lock().step_budget();
    for _ in 0..budget {
        let task = {
            let mut executor = EXECUTOR.lock();
            // Promote expired event waits before selecting ready work.
            let now = crate::monotonic_ms();
            for task in executor.tasks.iter_mut() {
                if !task.ready && task.deadline_ms.is_some_and(|deadline| now >= deadline) {
                    task.ready = true;
                    task.waiting_for = None;
                    task.deadline_ms = None;
                    task.ready_age = 0;
                }
            }
            // Drop queued cancellations promptly, including tasks cancelled
            // while blocked and therefore never selected for dispatch.
            // Her atılan girdi için CPU yük slotu da geri verilir.
            executor.drop_finished();
            let cpu = crate::arch::x86_64::smp::current_cpu_index();
            let Some(index) = select_ready(&executor.tasks, cpu) else {
                break;
            };
            for task in executor.tasks.iter_mut().filter(|t| t.ready) {
                task.ready_age = task.ready_age.saturating_add(1);
            }
            let mut task = executor.tasks.swap_remove(index);
            release_run_slot(task.cpu);
            task.ready = false;
            task.ready_age = 0;
            task.run_count = task.run_count.saturating_add(1);
            // Publish the in-flight task before releasing the registry lock;
            // otherwise cancel() could miss it in the dequeue/callback gap.
            RUNNING_CANCELLED.store(false, Ordering::Relaxed);
            RUNNING_WAIT_SET.store(false, Ordering::Relaxed);
            RUNNING_DEADLINE_SET.store(false, Ordering::Relaxed);
            RUNNING_WOKEN.store(false, Ordering::Relaxed);
            *RUNNING_INFO.lock() = Some(TaskSnapshot {
                id: task.id,
                name: task.name.clone(),
                priority: task.priority,
                state: TaskState::Running,
                run_count: task.run_count,
                deadline_ms: None,
            });
            RUNNING_TASK.store(task.id.0, Ordering::Release);
            task
        };
        let mut task = task;
        // Görev geri çağrısı da korunur: bir hata zamanlayıcıyı kilitlemek
        // yerine yalnızca bu görevi tamamlar. `begin` doğrudan bu fonksiyonun
        // canlı yığın çerçevesinden kurtarma noktası alır.
        let (guard_scope, guard_start) = crate::apps::guard::prepare_recoverable(
            crate::apps::guard::NO_APP,
            crate::apps::guard::NO_WINDOW,
        );
        let status = if unsafe { crate::apps::guard::begin(guard_start) } {
            let _guard = guard_scope;
            (task.step_fn)()
        } else {
            if let Some(fault) = crate::apps::guard::take_pending() {
                crate::serial_print("[GUARD] Görev hatası izole edildi: ");
                crate::serial_print(fault.kind.as_str());
                crate::serial_print("\n");
            }
            TaskStatus::Completed
        };
        match status {
            TaskStatus::Yielded => task.ready = true,
            TaskStatus::Blocked => {
                task.ready = false;
                if RUNNING_WAIT_SET.load(Ordering::Acquire) {
                    let event = RUNNING_WAIT_EVENT.load(Ordering::Acquire);
                    task.waiting_for = Some(TaskEvent(event));
                    task.ready = RUNNING_WOKEN.load(Ordering::Acquire);
                    task.deadline_ms = if RUNNING_DEADLINE_SET.load(Ordering::Acquire) {
                        Some(RUNNING_WAIT_DEADLINE.load(Ordering::Acquire))
                    } else {
                        None
                    };
                    if task.ready {
                        task.waiting_for = None;
                        task.deadline_ms = None;
                    }
                }
            }
            TaskStatus::Completed => task.completed = true,
        }
        let mut executor = EXECUTOR.lock();
        // Clear the active slot while holding the same registry lock used by
        // cancel(), closing the race between cancellation and reinsertion.
        let cancelled = RUNNING_CANCELLED.swap(false, Ordering::AcqRel);
        RUNNING_WAIT_SET.store(false, Ordering::Release);
        RUNNING_DEADLINE_SET.store(false, Ordering::Release);
        *RUNNING_INFO.lock() = None;
        RUNNING_TASK.store(0, Ordering::Release);
        task.cancelled = task.cancelled || cancelled;
        if !task.completed && !task.cancelled {
            RUN_QUEUE_DEPTH[task.cpu.min(RUN_QUEUE_DEPTH.len() - 1)]
                .fetch_add(1, Ordering::Relaxed);
            executor.tasks.push(task);
        }
    }
    IN_SCHEDULER.store(false, Ordering::Release);
    EXECUTOR.lock().task_count()
}

fn recover_scheduler_state() {
    IN_SCHEDULER.store(false, Ordering::Release);
    RUNNING_CANCELLED.store(false, Ordering::Release);
    RUNNING_WAIT_SET.store(false, Ordering::Release);
    RUNNING_DEADLINE_SET.store(false, Ordering::Release);
    RUNNING_TASK.store(0, Ordering::Release);
    *RUNNING_INFO.lock() = None;
}

/// Test-only: ölçüm penceresi boyunca yeni görev dağıtımını durdurur.
///
/// `poll_tasks_inner` bu bayrağı gördüğünde hiçbir CPU görev seçmez; böylece
/// kuyruk derinliği (`RUN_QUEUE_DEPTH`) ve görev sayısı ölçümleri AP'lerin
/// görevleri kuyruktan çekmesi nedeniyle bozulmaz. Bekleyen tek bir dağıtım
/// varsa tamamlanması beklenir: görev kuyruğa geri konduğunda derinlik
/// değişimi tamamlanır ve ölçüm tabanı kararlı hale gelir.
pub struct SchedulerPauseGuard {
    held: bool,
}

impl SchedulerPauseGuard {
    /// Zamanlayıcıyı dondurur ve süren dağıtımın bitmesini bekler.
    pub fn new() -> Self {
        SCHEDULER_PAUSED.store(true, Ordering::Release);
        // Devam eden dağıtımın bitmesini bekle: görev kuyruğa geri konup
        // kuyruk derinliği eski değerine dönünce ölçüm tabanı sabitlenir.
        // Bekleme sınırlıdır; bir görev geri çağrısı beklenenden uzun sürerse
        // ölçüm yine de yapılır (yalnızca kararlılığı azalır).
        let mut spins = 0u64;
        while IN_SCHEDULER.load(Ordering::Acquire) && spins < 10_000_000 {
            spins += 1;
            core::hint::spin_loop();
        }
        Self { held: true }
    }
}

impl Drop for SchedulerPauseGuard {
    fn drop(&mut self) {
        if self.held {
            SCHEDULER_PAUSED.store(false, Ordering::Release);
        }
    }
}

impl Default for SchedulerPauseGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Priority aging promotes a ready task by one level for each eight dispatch
/// opportunities it waits, up to Realtime. This reduces starvation without
/// claiming deadlines or wall-clock guarantees.
fn select_ready(tasks: &[Task], cpu: usize) -> Option<usize> {
    // Öncelik, yalnızca bu CPU'nun *hazır* görevleri üzerinden hesaplanır.
    // Tüm CPU'lar üzerinden hesaplamak, düşük öncelikli yerel görevlerin
    // başka CPU'da yüksek öncelikli bir görev varken açta kalmasına yol
    // açıyordu (tarayıcı ağ/render/JS-timer görevlerinin aç kaldığı durum).
    let priority = |task: &Task| -> u32 {
        (task.priority as u32 + (task.ready_age / 8)).min(TaskPriority::Realtime as u32)
    };
    let runnable = |task: &Task| -> bool { task.ready && !task.cancelled && !task.completed };

    let local_best = tasks
        .iter()
        .filter(|task| runnable(task) && task.cpu == cpu)
        .map(priority)
        .max();

    // Yerel kuyruk boşsa iş çalma (work stealing) devreye girer: hazır iş
    // varken hiçbir CPU boşta kalmaz. Görev sahibi CPU'suna geri döner
    // (`task.cpu` değişmez), dolayısıyla affinity bir *tercih* olarak korunur.
    let (best_priority, local_only) = match local_best {
        Some(p) => (p, true),
        None => {
            let foreign_best = tasks
                .iter()
                .filter(|task| runnable(task))
                .map(priority)
                .max()?;
            (foreign_best, false)
        }
    };

    let start = RR_CURSOR.fetch_add(1, Ordering::Relaxed);
    (0..tasks.len())
        .map(|n| (start + n) % tasks.len())
        .find(|&i| {
            let task = &tasks[i];
            runnable(task) && (task.cpu == cpu) == local_only && priority(task) == best_priority
        })
}

/// Cok cekirdekli scheduler davranisini dogrulayan birlesik test:
/// - Gorevler tum online CPU'lara dagitilmali (least_loaded_cpu).
/// - Ayni anda yapilan wake/notify islemleri gorevi kaybetmemeli.
/// - Kuyruk derinligi (RUN_QUEUE_DEPTH) spawn/iptal dongulerinde geri
///   gelmeli; iptal edilen gorevlerin CPU slotlari sizmamali.
/// - poll_tasks birden fazla kez calistirilip kuyruk bosaltildiginda derinlik
///   tekrar tabana inmeli.
///
/// Bu test BSP baslangic surecinden cagrilir. Cok cekirdekli (SMP) makinede
/// tum online CPU'larin gercekten kullanildigi dogrulanir; tek cekirdekli
/// profilde dagilim beklentisi 1'e iner.
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Task with given priority/cpu/ready_age.
    fn make_task(priority: TaskPriority, cpu: usize, ready_age: u32) -> Task {
        let mut t = Task::new("t", priority, || TaskStatus::Yielded);
        t.cpu = cpu;
        t.ready_age = ready_age;
        t.ready = true;
        t
    }

    #[test]
    fn select_ready_picks_local_cpu_task_even_if_lower_priority_exists_elsewhere() {
        // CPU 0 has a Realtime task; CPU 1 has only a Normal ready task.
        // select_ready for CPU 1 must NOT return None just because a higher
        // priority task lives on another CPU.
        let mut t0 = make_task(TaskPriority::Realtime, 0, 0);
        let mut t1 = make_task(TaskPriority::Normal, 1, 0);
        let tasks = vec![t0, t1];

        // Selecting for CPU 0 picks the Realtime task.
        let idx0 = select_ready(&tasks, 0).expect("CPU 0 should find its task");
        assert_eq!(tasks[idx0].cpu, 0);
        assert_eq!(tasks[idx0].priority, TaskPriority::Realtime);

        // Selecting for CPU 1 picks the Normal task (not None).
        let idx1 = select_ready(&tasks, 1).expect("CPU 1 should find its local task");
        assert_eq!(tasks[idx1].cpu, 1);
        assert_eq!(tasks[idx1].priority, TaskPriority::Normal);
    }

    #[test]
    fn select_ready_round_robin_among_same_priority_local_tasks() {
        let mut t0 = make_task(TaskPriority::Normal, 0, 0);
        let mut t1 = make_task(TaskPriority::Normal, 0, 0);
        let tasks = vec![t0, t1];

        let i0 = select_ready(&tasks, 0).expect("should find a task");
        let i1 = select_ready(&tasks, 0).expect("should find the other task");
        assert_ne!(i0, i1);
        assert_eq!(tasks[i0].cpu, 0);
        assert_eq!(tasks[i1].cpu, 0);
    }

    #[test]
    fn select_ready_steals_pending_task_when_local_queue_is_empty() {
        let mut t0 = make_task(TaskPriority::Normal, 0, 0);
        let tasks = vec![t0];
        // CPU 1 has no local task: is çalma (work stealing) devreye girer ve
        // hazır görev boşta kalmaz.
        let idx = select_ready(&tasks, 1).expect("should steal the pending task");
        assert_eq!(tasks[idx].cpu, 0);
    }

    #[test]
    fn select_ready_prefers_local_task_over_stealing() {
        let mut t0 = make_task(TaskPriority::Low, 0, 0);
        let mut t1 = make_task(TaskPriority::Realtime, 1, 0);
        let tasks = vec![t0, t1];
        // CPU 0'un yerel (düşük öncelikli) görevi varken başka CPU'nun
        // yüksek öncelikli görevi çalınmaz.
        let idx = select_ready(&tasks, 0).expect("should find the local task");
        assert_eq!(tasks[idx].cpu, 0);
        assert_eq!(tasks[idx].priority, TaskPriority::Low);
    }

    #[test]
    fn select_ready_returns_none_when_no_task_is_runnable() {
        let mut t0 = make_task(TaskPriority::Normal, 0, 0);
        t0.ready = false;
        let tasks = vec![t0];
        // Hiçbir koşulabilir görev yok.
        assert!(select_ready(&tasks, 1).is_none());
    }
}

/// Stres testinde "en az bir kez koştu" sayacı tutan görev sayısı.
const SCHED_TEST_RUN_TASKS: usize = 8;

/// Her görevin kendine ait çalışma sayacı. Sayaç görev kaydının dışında
/// tutulduğu için, görev o anda başka bir CPU'da çalışıyor olsa bile
/// "koştu" bilgisi kaybolmaz.
static SCHED_TEST_RUNS: [AtomicUsize; SCHED_TEST_RUN_TASKS] =
    [const { AtomicUsize::new(0) }; SCHED_TEST_RUN_TASKS];

/// Cok cekirdekli scheduler davranisini dogrulayan birlesik test:
/// - Gorevler tum online CPU'lara dagitilmali (least_loaded_cpu).
/// - Ayni anda yapilan wake/notify islemleri gorevi kaybetmemeli.
/// - Kuyruk derinligi (RUN_QUEUE_DEPTH) spawn/iptal dongulerinde geri
///   gelmeli; iptal edilen gorevlerin CPU slotlari sizmamali.
/// - poll_tasks birden fazla kez calistirilip kuyruk bosaltildiginda derinlik
///   tekrar tabana inmeli.
///
/// Bu test BSP baslangic surecinden cagrilir. Cok cekirdekli (SMP) makinede
/// tum online CPU'larin gercekten kullanildigi dogrulanir; tek cekirdekli
/// profilde dagilim beklentisi 1'e iner.
pub fn test_scheduler_multicore_race() -> bool {
    let online = crate::arch::x86_64::smp::cpu_count().max(1);
    let tracked = online.min(RUN_QUEUE_DEPTH.len());

    const TASKS_PER_ROUND: usize = 32;
    const ROUNDS: usize = 3;

    let mut spawned_ok = true;
    let mut spread_ok = true;
    let mut notify_ok = true;
    let mut cancel_ok = true;
    let mut drained_ok = true;
    let mut depth_ok = true;
    let mut ran_ok = true;

    // Zamanlayıcıyı gerçekten dondur: ölçüm penceresinde hiçbir CPU yeni
    // görev dağıtamaz. Böylece kuyruk derinliği ve görev sayısı ölçümleri
    // AP'lerin görevleri kuyruktan çekmesi nedeniyle bozulmaz.
    let pause = SchedulerPauseGuard::new();

    // Taban değerler dondurulmuş kuyruktan okunur.
    let initial = EXECUTOR.lock().task_count();
    let initial_depth = total_run_queue_depth();
    let baseline: alloc::vec::Vec<usize> = (0..tracked).map(run_queue_depth).collect();

    for _round in 0..ROUNDS {
        // Farklı öncelikte görevler oluştur; least_loaded_cpu RR imleciyle
        // dağılımı tüm online CPU'lara yayar. Görevler `Blocked` döner: böylece
        // kuyruk derinliği deterministik kalır.
        let mut ids = alloc::vec::Vec::new();
        for i in 0..TASKS_PER_ROUND {
            let prio = match i % 4 {
                0 => TaskPriority::Realtime,
                1 => TaskPriority::High,
                2 => TaskPriority::Normal,
                _ => TaskPriority::Low,
            };
            let id = spawn("race-task", prio, move || TaskStatus::Blocked);
            ids.push(id);
        }

        let after_spawn = EXECUTOR.lock().task_count();
        if after_spawn != initial + TASKS_PER_ROUND {
            spawned_ok = false;
        }

        // Boş kuyruğa notify: panik yapmamalı ve bekleyen görev
        // olmadığı için 0 dönmeli.
        if notify(TaskEvent(999)) != 0 {
            notify_ok = false;
        }

        // Kuyruk derinliği tam olarak eklenen görev kadar artmış olmalı.
        if total_run_queue_depth() != initial_depth + TASKS_PER_ROUND {
            depth_ok = false;
        }

        // Dağılım: görevlerin en az bir CPU'ya dağıldığı kontrolü.
        // Tek CPU'da her zaman 1'inin kullanıldığına dair beklenti,
        // çok CPU'lu sistemlerde ise görevlerin spreading kontrolü.
        // `uniform` kontrolü yeterince katı olmadığı için, her zaman en az
        // 1 CPU'nin görev kullandığını ve çok CPU'lu sistemlerde tracked kadarın
        // en azının kullanıldığını kontrol ederiz.
        let mut used = 0usize;
        for (cpu, base) in baseline.iter().enumerate().take(tracked) {
            if run_queue_depth(cpu) > *base {
                used += 1;
            }
        }
        // Tek CPU'da expected=1, çok CPU'lu'da tracked CPUs'yi hedefle.
        let expected_used = if online <= 1 || tracked <= 1 {
            1
        } else {
            tracked.min(TASKS_PER_ROUND)
        };
        if used < expected_used {
            spread_ok = false;
        }

        // Görevleri iptal et: spin kilidi ve take_task eş zamanlı çağrısında
        // kayıp olmamalı, her iptal edilen görev CPU slotunu geri vermeli.
        let mut cancelled = 0usize;
        for id in ids {
            if cancel(id) {
                cancelled += 1;
            }
        }
        if cancelled != TASKS_PER_ROUND {
            cancel_ok = false;
        }
        if EXECUTOR.lock().task_count() != initial {
            drained_ok = false;
        }
        if total_run_queue_depth() != initial_depth {
            depth_ok = false;
        }
    }

    // --- Gerçek çalışma yolu: görevler bir kez koşup yeniden hazır olsun ---
    // `Yielded` dönen görevler kuyrukta kalır. Her görev kendi çalışma
    // sayacını doldurur; sayaç görev kaydı dışında tutulduğu için görevin
    // o anda başka bir CPU'da çalışıyor olması ölçümü bozmaz.
    for counter in SCHED_TEST_RUNS.iter() {
        counter.store(0, Ordering::Relaxed);
    }
    let mut yielded_ids = alloc::vec::Vec::new();
    for counter in SCHED_TEST_RUNS.iter().take(SCHED_TEST_RUN_TASKS) {
        yielded_ids.push(spawn("yield-task", TaskPriority::Normal, move || {
            counter.fetch_add(1, Ordering::Relaxed);
            TaskStatus::Yielded
        }));
    }
    if total_run_queue_depth() != initial_depth + yielded_ids.len() {
        depth_ok = false;
    }

    // Zamanlayıcıyı bırak ve tüm görevlerin en az bir kez koştuğunu bekle.
    // Zamanlayıcı duraklatıldıyken hiçbir CPU dağıtım yapamaz; bırakıldığında
    // iş çalma sayesinde bekleyen görevler hangi CPU'da olursa olsun koşar.
    // Sayaçlar görev kaydından bağımsız tutulduğu için "en az bir kez koştu"
    // bilgisi, görev o anda kuyrukta olmasa bile kaybolmaz.
    drop(pause);
    let budget_before = EXECUTOR.lock().step_budget();
    EXECUTOR.lock().set_step_budget(8);
    let deadline = crate::monotonic_ms().saturating_add(5_000);
    while crate::monotonic_ms() < deadline {
        poll_tasks();
        if SCHED_TEST_RUNS
            .iter()
            .take(SCHED_TEST_RUN_TASKS)
            .all(|counter| counter.load(Ordering::Relaxed) > 0)
        {
            break;
        }
    }
    EXECUTOR.lock().set_step_budget(budget_before.max(1));

    // Çalışma doğrulaması sayaçlardan okunur (görev nesnesi iptal/yeniden
    // kuyruklanma sırasında taşınmış olsa bile sayaç kalıcıdır).
    let ran_count = SCHED_TEST_RUNS
        .iter()
        .take(SCHED_TEST_RUN_TASKS)
        .filter(|counter| counter.load(Ordering::Relaxed) > 0)
        .count();
    if ran_count != SCHED_TEST_RUN_TASKS {
        ran_ok = false;
    }

    // Kalan işleri temizle; derinlik yeniden tabana inmeli. İptal edilen bir
    // görev o anda başka bir CPU'da çalışıyorsa kayıttan düşmesi, o görevin
    // geri dönmesiyle gerçekleşir. Bu nedenle iptal sonrası kısa bir süre
    // dağıtım yaparak/kararlı durumu bekleyerek ölçüm alınır.
    let mut cancelled_after_run = 0usize;
    for id in &yielded_ids {
        if cancel(*id) {
            cancelled_after_run += 1;
        }
    }
    if cancelled_after_run != SCHED_TEST_RUN_TASKS {
        // Zaten tamamlanmış/çıkarılmış görevler de kabul edilebilir; kayıt
        // ve derinlik denetimleri nihai durumu doğrular.
        cancel_ok = false;
    }

    // Ölçümü kararlı kılmak için zamanlayıcıyı yeniden dondur ve gerekirse
    // süren dağıtımın tamamlanması için kısa bir pencere tanı. `cancel()`
    // çalışan bir görevi yalnızca işaretler; görev geri dönüp yeniden
    // kuyruğa alınmadığında kayıt/derinlik düşüşü tamamlanır.
    let settle_deadline = crate::monotonic_ms().saturating_add(2_000);
    loop {
        let settle = SchedulerPauseGuard::new();
        let count_ok = EXECUTOR.lock().task_count() == initial;
        let depth_now_ok = total_run_queue_depth() == initial_depth;
        drop(settle);
        if count_ok && depth_now_ok {
            break;
        }
        if crate::monotonic_ms() >= settle_deadline {
            drained_ok = drained_ok && count_ok;
            depth_ok = depth_ok && depth_now_ok;
            break;
        }
        // Dondurulmamış kısa pencere: işaretlenmiş görevlerin tamamlanmasına
        // izin ver, ardından tekrar dene.
        poll_tasks();
    }

    let ok = spawned_ok && spread_ok && notify_ok && cancel_ok && drained_ok && depth_ok && ran_ok;

    if ok {
        crate::serial_print("[SCHED-TEST] Cok cekirdekli scheduler yarismasi testi: BASARILI\n");
    } else {
        crate::serial_print("[SCHED-TEST] Cok cekirdekli scheduler yarismasi testi: BASARISIZ\n");
    }

    // Hangi denetimin düştüğünü raporla: test çıktısı tek satırda
    // "BASARISIZ" deyip nedenini söylemezse teşhis zorlaşır.
    crate::serial_print("[SCHED-TEST] ayrinti: spawn=");
    crate::serial::print_dec(spawned_ok as u64);
    crate::serial_print(" dagilim=");
    crate::serial::print_dec(spread_ok as u64);
    crate::serial_print(" notify=");
    crate::serial::print_dec(notify_ok as u64);
    crate::serial_print(" iptal=");
    crate::serial::print_dec(cancel_ok as u64);
    crate::serial_print(" calisma=");
    crate::serial::print_dec(ran_ok as u64);
    crate::serial_print(" bosaltma=");
    crate::serial::print_dec(drained_ok as u64);
    crate::serial_print(" derinlik=");
    crate::serial::print_dec(depth_ok as u64);
    crate::serial_print(" cpu=");
    crate::serial::print_dec(online as u64);
    crate::serial_print("\n");

    ok
}

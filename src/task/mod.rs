//! Cooperative kernel task scheduler.
//!
//! A task step must be short and must not perform blocking work. The scheduler
//! cannot preempt a step: its budget limits the number of steps dispatched in
//! one batch, not the duration of an individual step.

extern crate alloc;

use crate::arch::x86_64::sync::SpinMutex;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
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
}

impl Task {
    pub fn new<F>(name: &str, priority: TaskPriority, f: F) -> Self
    where
        F: FnMut() -> TaskStatus + Send + 'static,
    {
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

    fn take_task(&mut self, id: TaskId) -> Option<Task> {
        let index = self.tasks.iter().position(|t| t.id == id)?;
        Some(self.tasks.swap_remove(index))
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

pub static EXECUTOR: SpinMutex<SimpleExecutor> = SpinMutex::new(SimpleExecutor::new());
static IN_SCHEDULER: AtomicBool = AtomicBool::new(false);
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
    {
        if !RUNNING_WOKEN.swap(true, Ordering::AcqRel) {
            count += 1;
        }
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
pub fn poll_tasks() -> usize {
    // Prevent two CPUs from mutating/executing the same task simultaneously.
    if IN_SCHEDULER.swap(true, Ordering::Acquire) {
        return EXECUTOR.lock().task_count();
    }
    let budget = EXECUTOR.lock().step_budget();
    for _ in 0..budget {
        let task = {
            let mut executor = EXECUTOR.lock();
            // Promote expired event waits before selecting ready work.
            let now = crate::monotonic_ms();
            for task in executor.tasks.iter_mut() {
                if !task.ready && task.deadline_ms.map_or(false, |deadline| now >= deadline) {
                    task.ready = true;
                    task.waiting_for = None;
                    task.deadline_ms = None;
                    task.ready_age = 0;
                }
            }
            // Drop queued cancellations promptly, including tasks cancelled
            // while blocked and therefore never selected for dispatch.
            executor.tasks.retain(|t| !t.cancelled && !t.completed);
            let Some(index) = select_ready(&executor.tasks) else {
                break;
            };
            for task in executor.tasks.iter_mut().filter(|t| t.ready) {
                task.ready_age = task.ready_age.saturating_add(1);
            }
            let mut task = executor.tasks.swap_remove(index);
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
        let status = (task.step_fn)();
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
            executor.tasks.push(task);
        }
    }
    IN_SCHEDULER.store(false, Ordering::Release);
    EXECUTOR.lock().task_count()
}

/// Priority aging promotes a ready task by one level for each eight dispatch
/// opportunities it waits, up to Realtime. This reduces starvation without
/// claiming deadlines or wall-clock guarantees.
fn select_ready(tasks: &[Task]) -> Option<usize> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.ready && !task.cancelled && !task.completed)
        .max_by_key(|(_, task)| {
            (
                (task.priority as u32 + (task.ready_age / 8)).min(TaskPriority::Realtime as u32),
                task.ready_age,
            )
        })
        .map(|(index, _)| index)
}

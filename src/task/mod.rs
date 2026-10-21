//! Çok Görevlilik ve İş Parçacığı Yöneticisi (Multitasking & Cooperative Task Executor).
//!
//! Bu modül, çekirdek ve uygulamalar için hafif (lightweight) eşzamanlılık sunar:
//! 1. **Cooperative Görev Modeli (Cooperative Multitasking):** Her görev belirli
//!    aralıklarla işlemciyi diğer görevlere devreder (`TaskStatus::Yielded`).
//! 2. **Görev Kuyruğu ve Öncelik:** Görevler öncelik seviyelerine (Düşük, Normal, Yüksek, Gerçek Zamanlı)
//!    göre sıralanır ve adil paylaşımla çalıştırılır.
//! 3. **Periyodik Arka Plan İşleri:** Ağ yoklaması (`poll`), ses çalma arabelleği ve
//!    uzun süreli hesaplamalar GUI donmadan arka planda eşzamanlı yürütülür.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::arch::x86_64::sync::SpinMutex;

static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1);

/// Görev Kimliği
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub usize);

impl TaskId {
    pub fn new() -> Self {
        Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Görev Durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Görev çalışmaya devam edecek (işlemciyi bir sonraki adıma bıraktı).
    Yielded,
    /// Görev tamamlandı ve kuyruktan çıkarılabilir.
    Completed,
}

/// Görev Önceliği
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Realtime = 3,
}

/// Çekirdek Görevi
pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub priority: TaskPriority,
    pub step_fn: Box<dyn FnMut() -> TaskStatus + Send>,
    pub run_count: usize,
    pub completed: bool,
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
        }
    }

    pub fn poll(&mut self) -> TaskStatus {
        self.run_count = self.run_count.saturating_add(1);
        let status = (self.step_fn)();
        if status == TaskStatus::Completed {
            self.completed = true;
        }
        status
    }
}

/// Çok Görevlilik Yürütücüsü (Cooperative Task Executor)
pub struct SimpleExecutor {
    tasks: Vec<Task>,
}

impl SimpleExecutor {
    pub const fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Yeni bir görevi yürütücüye kaydeder.
    pub fn spawn<F>(&mut self, name: &str, priority: TaskPriority, f: F) -> TaskId
    where
        F: FnMut() -> TaskStatus + Send + 'static,
    {
        let task = Task::new(name, priority, f);
        let id = task.id;
        self.tasks.push(task);
        // Önceliğe göre sırala (High -> Low)
        self.tasks.sort_by(|a, b| b.priority.cmp(&a.priority));
        id
    }

    /// Kuyruktaki görevlerin her birini bir tur çalıştırır.
    pub fn run_ready_tasks(&mut self) -> usize {
        let mut completed_indices = Vec::new();

        for (idx, task) in self.tasks.iter_mut().enumerate() {
            if task.poll() == TaskStatus::Completed {
                completed_indices.push(idx);
            }
        }

        // Tamamlanan görevleri tersten temizle
        for idx in completed_indices.into_iter().rev() {
            self.tasks.swap_remove(idx);
        }

        self.tasks.len()
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn task_names(&self) -> Vec<String> {
        self.tasks.iter().map(|t| t.name.clone()).collect()
    }
}

/// Küresel görev yürütücüsü (Global Executor)
pub static EXECUTOR: SpinMutex<SimpleExecutor> = SpinMutex::new(SimpleExecutor::new());

/// Yeni görev ekleme genel fonksiyonu
pub fn spawn<F>(name: &str, priority: TaskPriority, f: F) -> TaskId
where
    F: FnMut() -> TaskStatus + Send + 'static,
{
    EXECUTOR.lock().spawn(name, priority, f)
}

/// Ana olay döngüsünden her karede çağrılarak görevleri yürütür.
pub fn poll_tasks() -> usize {
    EXECUTOR.lock().run_ready_tasks()
}


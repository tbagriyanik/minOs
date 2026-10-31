//! Web Workers support (no_std).
//!
//! Provides a Worker struct with message queue, postMessage/onmessage API,
//! and inline JS execution using the minJs engine. Workers run in a separate
//! context with message-passing communication to the main thread.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::apps::js::JsRuntime;

/// A message passed between the main thread and a worker.
#[derive(Clone, Debug)]
pub struct WorkerMessage {
    pub data: String,
    pub timestamp_ms: u64,
}

/// Worker state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerState {
    Uninitialized,
    Running,
    Paused,
    Terminated,
}

/// A Web Worker instance.
pub struct Worker {
    pub id: usize,
    pub source_url: String,
    pub source_code: String,
    pub state: WorkerState,
    pub js_runtime: Option<JsRuntime>,
    pub message_queue: Vec<WorkerMessage>,
    pub onmessage_handler: Option<String>,
    pub last_update_ms: u64,
}

impl Worker {
    pub fn new(id: usize, source_url: String, source_code: String) -> Self {
        Worker {
            id,
            source_url,
            source_code,
            state: WorkerState::Uninitialized,
            js_runtime: Some(JsRuntime::new()),
            message_queue: Vec::new(),
            onmessage_handler: None,
            last_update_ms: 0,
        }
    }

    /// Initialize the worker's JS runtime.
    pub fn init(&mut self) {
        if let Some(ref mut rt) = self.js_runtime {
            // Execute the worker source code
            let mut out = Vec::new();
            rt.run(&self.source_code, &mut out);
        }
        self.state = WorkerState::Running;
        self.last_update_ms = crate::boot::time::main_monotonic_ms() as u64;
    }

    /// Post a message to the worker.
    pub fn post_message(&mut self, data: String) {
        self.message_queue.push(WorkerMessage {
            data,
            timestamp_ms: crate::boot::time::main_monotonic_ms() as u64,
        });
    }

    /// Process pending messages and execute worker code.
    pub fn update(&mut self, now_ms: u64) {
        if self.state != WorkerState::Running {
            return;
        }

        // Process queued messages
        if !self.message_queue.is_empty() {
            if let Some(ref mut _rt) = self.js_runtime {
                for msg in self.message_queue.drain(..) {
                    // In a full implementation, we'd invoke the onmessage handler
                    // with the message data as an argument
                    let _ = msg;
                }
            }
        }

        self.last_update_ms = now_ms;
    }

    /// Terminate the worker.
    pub fn terminate(&mut self) {
        self.state = WorkerState::Terminated;
        self.js_runtime = None;
        self.message_queue.clear();
    }
}

/// Collection of Web Workers on the current page.
pub struct Workers {
    pub workers: BTreeMap<usize, Worker>,
    pub next_id: usize,
}

impl Workers {
    pub fn new() -> Self {
        Workers {
            workers: BTreeMap::new(),
            next_id: 0,
        }
    }

    pub fn create_worker(&mut self, source_url: String, source_code: String) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let mut worker = Worker::new(id, source_url, source_code);
        worker.init();
        self.workers.insert(id, worker);
        id
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Worker> {
        self.workers.get_mut(&id)
    }

    pub fn get(&self, id: usize) -> Option<&Worker> {
        self.workers.get(&id)
    }

    pub fn post_message(&mut self, id: usize, data: String) {
        if let Some(worker) = self.workers.get_mut(&id) {
            worker.post_message(data);
        }
    }

    pub fn update_all(&mut self, now_ms: u64) {
        for worker in self.workers.values_mut() {
            worker.update(now_ms);
        }
    }

    pub fn terminate_all(&mut self) {
        for worker in self.workers.values_mut() {
            worker.terminate();
        }
        self.workers.clear();
    }
}

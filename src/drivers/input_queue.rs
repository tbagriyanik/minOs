//! Global input event queue — no_std + no alloc (fixed-size ring buffer).
//!
//! Drivers (PS/2, USB HID) push normalized input events here. The event loop
//! drains the queue instead of polling hardware every frame. When the queue
//! is empty the loop can skip the expensive port reads entirely.

use core::cell::{Cell, UnsafeCell};

/// A single normalized input event.
///
/// All drivers (PS/2 keyboard, PS/2 mouse, USB HID, touch) reduce their
/// hardware-specific reports to one of these variants. The event loop only
/// needs to understand this enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Keyboard scancode byte arrived. The driver decodes it into state.
    KeyPress(u8),
    KeyRelease(u8),
    /// Mouse packet: signed deltas + button bitmask (bit0=left, bit1=right,
    /// bit2=middle). Wheel is reported separately via MouseWheel.
    MouseMove {
        dx: i8,
        dy: i8,
        buttons: u8,
    },
    /// Mouse wheel delta (positive = up).
    MouseWheel(i8),
    /// Touchpad touch start.
    TouchDown {
        x: i32,
        y: i32,
    },
    /// Touchpad touch end.
    TouchUp,
    /// Touchpad move while touching.
    TouchMove {
        dx: i32,
        dy: i32,
    },
    /// USB HID device hot-plug event.
    UsbHotplug {
        connect: bool,
        kind: u8,
        port: u8,
    },
}

/// Fixed-capacity ring buffer for input events.
///
/// Capacity 256 is enough for a full keyboard burst (a fast typist can
/// generate ~50 scancodes/sec; even at 1000 Hz polling we never need more
/// than a few frames' worth buffered).
pub const INPUT_QUEUE_CAP: usize = 256;

pub struct InputQueue {
    // UnsafeCell: kuyruk tek iş parçacıklı (olay döngüsü) tüketilir; ISR
    // yazıcıları `&self` üzerinden yazar. Hücreler asla eşzamanlı &mut
    // paylaşmaz — `head`/`tail` Cell'leri tek yazıcı/silici sırasını korur.
    buf: [UnsafeCell<Option<InputEvent>>; INPUT_QUEUE_CAP],
    head: Cell<usize>,
    tail: Cell<usize>,
}

unsafe impl Send for InputQueue {}
unsafe impl Sync for InputQueue {}

impl InputQueue {
    pub const fn new() -> Self {
        Self {
            buf: [const { UnsafeCell::new(None) }; INPUT_QUEUE_CAP],
            head: Cell::new(0),
            tail: Cell::new(0),
        }
    }

    /// Push one event. Returns the event back if the queue is full.
    pub fn push(&self, ev: InputEvent) -> Option<InputEvent> {
        let head = self.head.get();
        let next = (head + 1) % INPUT_QUEUE_CAP;
        if next == self.tail.get() {
            return Some(ev); // full
        }
        // SAFETY: `head` yalnızca burada ve `pop` içinde değişir; ikisi de
        // tek iş parçacıklı olay döngüsü tarafından, kesmeler kapalı bölgede
        // veya kesme bağlamında çağrılır. `UnsafeCell` paylaşılan erişime izin verir.
        unsafe {
            *self.buf.get_unchecked(head).get() = Some(ev);
        }
        self.head.set(next);
        None
    }

    /// Pop the oldest event. Returns None when empty.
    pub fn pop(&self) -> Option<InputEvent> {
        let tail = self.tail.get();
        if tail == self.head.get() {
            return None;
        }
        // SAFETY: `push` ile aynı tek iş parçacıklı invariant.
        let cell = unsafe { self.buf.get_unchecked(tail) };
        let ev = unsafe { (*cell.get()).take() };
        self.tail.set((tail + 1) % INPUT_QUEUE_CAP);
        ev
    }

    pub fn is_empty(&self) -> bool {
        self.head.get() == self.tail.get()
    }

    pub fn len(&self) -> usize {
        let (h, t) = (self.head.get(), self.tail.get());
        if h >= t {
            h - t
        } else {
            INPUT_QUEUE_CAP - t + h
        }
    }

    /// Drain all events into a caller-provided buffer, returning the count.
    /// Useful for batch processing.
    pub fn drain(&self, out: &mut [Option<InputEvent>; 64]) -> usize {
        let mut n = 0;
        while n < out.len() {
            match self.pop() {
                Some(ev) => out[n] = Some(ev),
                None => break,
            }
            n += 1;
        }
        n
    }
}

impl Default for InputQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// The global input queue. Drivers push to it; the event loop drains it.
pub static INPUT_QUEUE: InputQueue = InputQueue::new();

/// Convenience: push a keyboard scancode.
pub fn push_key(press: bool, scancode: u8) {
    let ev = if press {
        InputEvent::KeyPress(scancode)
    } else {
        InputEvent::KeyRelease(scancode)
    };
    let _ = INPUT_QUEUE.push(ev);
}

/// Convenience: push a mouse movement.
pub fn push_mouse(dx: i8, dy: i8, buttons: u8) {
    let _ = INPUT_QUEUE.push(InputEvent::MouseMove { dx, dy, buttons });
}

/// Convenience: push a wheel event.
pub fn push_wheel(delta: i8) {
    let _ = INPUT_QUEUE.push(InputEvent::MouseWheel(delta));
}

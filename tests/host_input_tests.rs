//! `input_queue` normalized klavye/fare kuyruğu testleri.

use crate::input_queue::{self, InputEvent, InputQueue};

#[test]
fn self_test_passes() {
    assert!(input_queue::self_test());
}

#[test]
fn push_pop_preserves_fifo_order() {
    let queue = InputQueue::new();
    assert!(queue.is_empty());

    let events = [
        InputEvent::KeyPress(0x1E),
        InputEvent::MouseMove {
            dx: 5,
            dy: -5,
            buttons: 1,
        },
        InputEvent::MouseWheel(-1),
        InputEvent::TouchDown { x: 7, y: 9 },
        InputEvent::TouchUp,
    ];
    for &event in events.iter() {
        assert!(queue.push(event).is_none());
    }
    assert_eq!(queue.len(), events.len());
    assert!(!queue.is_empty());

    for &expected in events.iter() {
        assert_eq!(queue.pop(), Some(expected));
    }
    assert!(queue.is_empty());
    assert_eq!(queue.pop(), None);
}

#[test]
fn full_queue_rejects_extra_event() {
    let queue = InputQueue::new();
    let capacity = input_queue::INPUT_QUEUE_CAP - 1;
    for index in 0..capacity {
        let wheel = (index % 127) as i8;
        assert!(queue.push(InputEvent::MouseWheel(wheel)).is_none());
    }
    assert_eq!(queue.len(), capacity);
    assert!(queue.push(InputEvent::TouchUp).is_some());
    assert_eq!(queue.len(), capacity);
}

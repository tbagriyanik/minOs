//! `input_queue` normalized klavye/fare kuyruğu testleri.

use crate::input_queue::{self, InputEvent, InputQueue};
use crate::mouse::MouseState;

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

#[test]
fn relative_mouse_look_accumulates_past_screen_edges() {
    let mut mouse = MouseState::new();
    mouse.update_packet_i32(0, 10, 4, 105, 105);
    mouse.update_packet_i32(0, 10, 0, 105, 105);

    assert_eq!((mouse.x, mouse.y), (104, 96));
    assert!(mouse.has_relative_motion());
    assert_eq!(mouse.take_look_delta(), (20, -4));
    assert_eq!(mouse.take_look_delta(), (0, 0));
}

#[test]
fn absolute_mouse_position_resets_relative_look_state() {
    let mut mouse = MouseState::new();
    mouse.update_packet_i32(0, 8, 3, 800, 600);
    mouse.set_absolute_position(400, 300);

    assert!(!mouse.has_relative_motion());
    assert_eq!((mouse.x, mouse.y), (400, 300));
    assert_eq!(mouse.take_look_delta(), (0, 0));
}

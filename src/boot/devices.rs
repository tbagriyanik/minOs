//! PS/2 giris aygitlarinin global durumlari (Asama 3'te masaustu oturumuna tasinacak).

use crate::drivers;

static mut GLOBAL_MOUSE: drivers::mouse::MouseState = drivers::mouse::MouseState::new();
static mut GLOBAL_KEYBOARD: drivers::keyboard::KeyboardState = drivers::keyboard::KeyboardState::new();

pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub left_button: bool,
    pub right_button: bool,
    pub middle_button: bool,
    pub scroll_delta: i8,
    /// 1..=10; 5 is the raw PS/2 speed.
    pub speed: u8,
    pub reverse_scroll: bool,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            x: 100,
            y: 100,
            left_button: false,
            right_button: false,
            middle_button: false,
            scroll_delta: 0,
            speed: 5,
            reverse_scroll: false,
        }
    }

    pub fn update_packet(&mut self, flags: u8, dx: i8, dy: i8, max_w: i32, max_h: i32) {
        self.left_button = (flags & 0x01) != 0;
        self.right_button = (flags & 0x02) != 0;
        self.middle_button = (flags & 0x04) != 0;

        let speed = self.speed.clamp(1, 10) as i32;
        let scale = |delta: i8| {
            if delta == 0 { return 0; }
            let scaled = delta as i32 * speed / 5;
            if scaled == 0 { (delta as i32).signum() } else { scaled }
        };
        self.x += scale(dx);
        self.y -= scale(dy); // PS/2 Y ekseni tersinedir

        if self.x < 0 { self.x = 0; }
        if self.y < 0 { self.y = 0; }
        if self.x >= max_w { self.x = max_w - 1; }
        if self.y >= max_h { self.y = max_h - 1; }
    }

    pub fn take_scroll_delta(&mut self) -> i8 {
        let val = self.scroll_delta;
        self.scroll_delta = 0;
        if self.reverse_scroll { val.saturating_neg() } else { val }
    }
}

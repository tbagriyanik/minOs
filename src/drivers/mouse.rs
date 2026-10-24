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
    /// Dokunmatik yüzey (touchpad) dokunarak tıklama (tap-to-click) ayarı
    pub tap_to_click: bool,
    pub is_touchpad: bool,
    // Dokunma jesti takip durumları
    touch_active: bool,
    touch_start_ms: u64,
    touch_start_x: i32,
    touch_start_y: i32,
    synthetic_click_remaining: u8,
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
            tap_to_click: true,
            is_touchpad: false,
            touch_active: false,
            touch_start_ms: 0,
            touch_start_x: 0,
            touch_start_y: 0,
            synthetic_click_remaining: 0,
        }
    }

    pub fn update_packet(&mut self, flags: u8, dx: i8, dy: i8, max_w: i32, max_h: i32) {
        self.update_packet_i32(flags, dx as i32, dy as i32, max_w, max_h);
    }

    /// Genişletilmiş (i32 dx/dy) hareket güncellemesi.
    /// Yüksek çözünürlüklü ekranlarda (1920x1080 ve üzeri) takılmayı önlemek ve
    /// pürüzsüz akıcılık sağlamak için çözünürlük tabanlı dinamik ölçekleme ve balistik ivmelenme uygular.
    pub fn update_packet_i32(&mut self, flags: u8, dx: i32, dy: i32, max_w: i32, max_h: i32) {
        let raw_left = (flags & 0x01) != 0;
        self.right_button = (flags & 0x02) != 0;
        self.middle_button = (flags & 0x04) != 0;

        let speed = self.speed.clamp(1, 10) as i32;

        // 1080p ve üzeri çözünürlüklerde doğal ve kontrollü taban oran:
        let (num, den) = if max_w >= 2560 || max_h >= 1440 {
            (7, 5) // 2K / 4K için ~1.4x
        } else if max_w >= 1920 || max_h >= 1080 {
            (6, 5) // 1080p için ~1.2x (doğal, kontrollü)
        } else {
            (5, 5) // 1.0x (1024x768 vb.)
        };

        let scale = |delta: i32| -> i32 {
            if delta == 0 { return 0; }
            let abs_d = delta.abs();
            // Hafif, dengeli ivmelenme (uçuşa geçmeden rahat kontrol):
            let accel = if abs_d > 12 {
                2
            } else {
                1
            };

            let scaled = (delta * speed * num * accel) / (den * 5);
            if scaled == 0 { delta.signum() } else { scaled }
        };

        self.x += scale(dx);
        self.y -= scale(dy); // PS/2 ve ters Y uyumluluğu

        if self.x < 0 { self.x = 0; }
        if self.y < 0 { self.y = 0; }
        if self.x >= max_w { self.x = max_w - 1; }
        if self.y >= max_h { self.y = max_h - 1; }

        // Touchpad Tap-to-Click jest denetimi
        if self.synthetic_click_remaining > 0 {
            self.left_button = true;
            self.synthetic_click_remaining -= 1;
        } else {
            self.left_button = raw_left;
        }
    }

    pub fn process_touch_event(&mut self, touching: bool, now_ms: u64) {
        if !self.tap_to_click {
            return;
        }

        if touching && !self.touch_active {
            // Parmak yüzeye ilk temas etti
            self.touch_active = true;
            self.touch_start_ms = now_ms;
            self.touch_start_x = self.x;
            self.touch_start_y = self.y;
        } else if !touching && self.touch_active {
            // Parmak yüzeyden ayrıldı
            self.touch_active = false;
            let duration = now_ms.saturating_sub(self.touch_start_ms);
            let dist_x = (self.x - self.touch_start_x).abs();
            let dist_y = (self.y - self.touch_start_y).abs();

            // 180 ms'den kısa sürede parmak dokunup kalktıysa ve hareket < 8px ise: TIKLAMA!
            if duration <= 180 && dist_x < 8 && dist_y < 8 {
                self.synthetic_click_remaining = 2; // 2 kare boyunca sol tık aktif kalır
                self.left_button = true;
            }
        }
    }

    pub fn take_scroll_delta(&mut self) -> i8 {
        let val = self.scroll_delta;
        self.scroll_delta = 0;
        if self.reverse_scroll { val.saturating_neg() } else { val }
    }
}

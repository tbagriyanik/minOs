use crate::gui::theme;

pub const MAX_SCREEN_WIDTH: usize = 1920;
pub const MAX_SCREEN_HEIGHT: usize = 1080;
pub const BACKBUFFER_LEN: usize = MAX_SCREEN_WIDTH * MAX_SCREEN_HEIGHT;
static mut BACKBUFFER: [u32; BACKBUFFER_LEN] = [0; BACKBUFFER_LEN];
const CURSOR_SAVE_W: usize = 24;
const CURSOR_SAVE_H: usize = 24;

/// İki dikdörtgenin en küçük kapsayıcı birleşimini (bounding box) döndürür.
#[inline(always)]
pub fn rect_union(r1: (usize, usize, usize, usize), r2: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let x0 = r1.0.min(r2.0);
    let y0 = r1.1.min(r2.1);
    let x1 = (r1.0 + r1.2).max(r2.0 + r2.2);
    let y1 = (r1.1 + r1.3).max(r2.1 + r2.3);
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

/// Dikdörtgen kırpma (clip) bölgesi. Tüm çizimler bu sınırların içinde kalır.
#[derive(Copy, Clone)]
pub struct ClipRect {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

pub struct Framebuffer {
    pub address: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
    /// Aktif kırpma bölgesi. Varsayılan: tüm ekran.
    clip: ClipRect,
    cursor_under: [u32; CURSOR_SAVE_W * CURSOR_SAVE_H],
    cursor_x: usize,
    cursor_y: usize,
    cursor_w: usize,
    cursor_h: usize,
    cursor_saved: bool,
    /// Donanım imleci aktifse yazılım sprite'ı atlanır (yalnızca konum izlenir).
    pub hw_cursor: bool,
    /// Fiziksel mod değişemediğinde mantıksal ölçek (sanal çözünürlük).
    pub scale: usize,
}

#[inline(always)]
fn rgb(color: u32) -> u32 {
    theme::resolve(color)
}

#[inline(always)]
fn readable_text_color(foreground: u32, background: u32) -> u32 {
    let luminance = |color: u32| {
        let red = (color >> 16) & 0xFF;
        let green = (color >> 8) & 0xFF;
        let blue = color & 0xFF;
        (red * red * 299 + green * green * 587 + blue * blue * 114) / 1000
    };
    let fg_luma = luminance(foreground);
    let bg_luma = luminance(background);
    let high = fg_luma.max(bg_luma) + 3251;
    let low = fg_luma.min(bg_luma) + 3251;
    if high * 2 >= low * 9 {
        return foreground;
    }

    let white_contrast = 68_276 * 100 / (bg_luma + 3251);
    let black_contrast = (bg_luma + 3251) * 100 / 3251;
    if white_contrast >= black_contrast { 0xFFFFFF } else { 0x000000 }
}

/// 4x4 Bayer dither matrisi - renk geçişlerinde kademelenmeyi (color banding) tamamen ortadan kaldırır.
const BAYER_4X4: [[i32; 4]; 4] = [
    [-8,  0, -6,  2],
    [ 4, -4,  6, -2],
    [-5,  3, -7,  1],
    [ 7, -1,  5, -3],
];

/// Dithering destekli pürüzsüz renk geçişi (kademelenmeyi yok eder).
#[allow(dead_code)]
#[inline(always)]
fn lerp_color_dither(a: u32, b: u32, t: usize, n: usize, cx: usize, cy: usize) -> u32 {
    if n == 0 {
        return a;
    }
    let d = BAYER_4X4[cy & 3][cx & 3];
    let inv = n - t;
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;

    let r = (((ar * inv as u32 + br * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
    let g = (((ag * inv as u32 + bg * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
    let bl = (((ab * inv as u32 + bb * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
    (r << 16) | (g << 8) | bl
}

pub static mut PANIC_FB_ADDR: *mut u32 = core::ptr::null_mut();
pub static mut PANIC_FB_W: usize = 0;
pub static mut PANIC_FB_H: usize = 0;
pub static mut PANIC_FB_PITCH: usize = 0;
pub static mut PANIC_FB_BPP: usize = 0;

impl Framebuffer {
    pub unsafe fn new(address: *mut u32, width: usize, height: usize, pitch: usize, bpp: usize) -> Self {
        PANIC_FB_ADDR = address;
        PANIC_FB_W = width;
        PANIC_FB_H = height;
        PANIC_FB_PITCH = pitch;
        PANIC_FB_BPP = bpp;
        Self {
            address,
            width,
            height,
            pitch,
            bpp,
            clip: ClipRect { x0: 0, y0: 0, x1: width, y1: height },
            cursor_under: [0; CURSOR_SAVE_W * CURSOR_SAVE_H],
            cursor_x: 0,
            cursor_y: 0,
            cursor_w: 0,
            cursor_h: 0,
            cursor_saved: false,
            hw_cursor: false,
            scale: 1,
        }
    }

    /// Donanım imlecini aç/kapat. Açıkken `save/restore_cursor_background`
    /// no-op olur; konum `drivers::gpu` üzerinden izlenir.
    pub fn set_hw_cursor(&mut self, enable: bool) {
        self.hw_cursor = enable;
        crate::drivers::gpu::hw_cursor_enable(enable);
        if enable { self.cursor_saved = false; }
    }

    /// BGA/VESA mod değişimi sonrası geometriyi günceller.
    /// Adres aynı kalır (BGA LFB sabittir); backbuffer sınırları korunur.
    pub fn resize(&mut self, width: usize, height: usize, pitch: usize) -> bool {
        if width == 0 || height == 0 { return false; }
        let total = match width.checked_mul(height) {
            Some(p) => p, None => return false,
        };
        if total > BACKBUFFER_LEN { return false; }
        if pitch < width * 4 || pitch % 4 != 0 { return false; }
        self.width = width;
        self.height = height;
        self.pitch = pitch;
        unsafe {
            PANIC_FB_W = width;
            PANIC_FB_H = height;
            PANIC_FB_PITCH = pitch;
        }
        self.reset_clip();
        self.cursor_saved = false;
        self.clear(0x000000);
        self.flush();
        true
    }

    /// Çizim sınırları, kırpma, taşma güvenliği, renk derinliği ve pitch hesaplamasını doğrular.
    /// Gerçek ekran ve GUI çizimini bozmamak için sentinellerle izole çalışır ve test sonunda orijinal durumu korur.
    pub fn verify_bounds(&mut self) -> bool {
        // 1. width * height çarpımını taşma güvenli (checked_mul) kontrol et
        if self.width == 0 || self.height == 0 {
            return false;
        }
        let total_pixels = match self.width.checked_mul(self.height) {
            Some(p) => p,
            None => return false,
        };
        if total_pixels > BACKBUFFER_LEN {
            return false;
        }

        // 2. Desteklenen renk derinliği (bpp) ve pitch hesaplama doğrulaması
        if self.bpp != 32 {
            return false;
        }
        let bytes_per_pixel = self.bpp / 8; // 32 bpp = 4 bayt
        let min_pitch = match self.width.checked_mul(bytes_per_pixel) {
            Some(mp) => mp,
            None => return false,
        };
        if self.pitch < min_pitch || self.pitch % 4 != 0 {
            return false;
        }
        let pitch_words = self.pitch / 4;
        if pitch_words < self.width {
            return false;
        }
        // Tüm framebuffer bellek gereksiniminin taşmadığını doğrula
        let total_fb_bytes = match self.pitch.checked_mul(self.height) {
            Some(tb) => tb,
            None => return false,
        };
        if total_fb_bytes == 0 {
            return false;
        }

        // 3. Testlerde ekran dışı çizim öncesi/sonrası sentinel (canary) değerleri karşılaştır
        const SENTINEL_HEAD: u32 = 0xDEADBEEF;
        const SENTINEL_MID: u32 = 0xCAFEBABE;
        const SENTINEL_TAIL: u32 = 0x5A5AA5A5;

        let head_idx = 0usize;
        let mid_idx = total_pixels - 1;
        let tail_idx = BACKBUFFER_LEN - 1;

        let orig_head = unsafe { BACKBUFFER[head_idx] };
        let orig_mid = unsafe { BACKBUFFER[mid_idx] };
        let orig_tail = unsafe { BACKBUFFER[tail_idx] };

        unsafe {
            BACKBUFFER[head_idx] = SENTINEL_HEAD;
            BACKBUFFER[mid_idx] = SENTINEL_MID;
            BACKBUFFER[tail_idx] = SENTINEL_TAIL;
        }

        // 4. Sınır testlerini gerçek ekran çiziminden ayır:
        // Ekran dışı koordinatlara kasıtlı çizim denemeleri yap
        // (Bu çağrılar asla sentinelleri değiştirmemeli ve donanım VRAM'ine dokunmamalıdır)
        self.put_pixel(self.width + 100, self.height + 100, 0x11223344);
        self.put_pixel(self.width, self.height, 0x11223344);
        self.put_pixel(usize::MAX - 10, usize::MAX - 10, 0x11223344);
        self.draw_rect(self.width, self.height, 50, 50, 0x11223344);
        self.draw_rect(self.width + 20, 0, 100, self.height, 0x11223344);
        self.draw_rect(0, self.height + 20, self.width, 100, 0x11223344);
        self.draw_line(self.width + 10, self.height + 10, self.width + 100, self.height + 100, 0x11223344);
        self.draw_rounded_rect(self.width + 20, self.height + 20, 40, 40, 5, 0x11223344);
        self.draw_vgradient(self.width + 30, self.height + 30, 20, 20, 0xFF0000, 0x0000FF);
        self.draw_shadow(self.width + 10, self.height + 10, 50, 50, 0x000000);

        // Ekran dışı flush_rect güvenliği: x >= width veya y >= height iken VRAM kopyası yapılmadan dönmeli
        self.flush_rect((self.width + 100, self.height + 100, 50, 50));
        self.flush_rect((usize::MAX - 50, usize::MAX - 50, 100, 100));

        // 5. Uç piksel çizim testi (Son geçerli piksel: width-1, height-1)
        let last_x = self.width.saturating_sub(1);
        let last_y = self.height.saturating_sub(1);
        let last_pixel_idx = last_y * self.width + last_x;
        let orig_last = unsafe { BACKBUFFER[last_pixel_idx] };
        self.put_pixel(last_x, last_y, 0x00AABBCC);
        let corner_written = unsafe { BACKBUFFER[last_pixel_idx] == 0x00AABBCC };
        unsafe { BACKBUFFER[last_pixel_idx] = orig_last; }

        // Sentinel doğrulaması: Ekran dışı hiçbir çizim sentinelleri bozmamış olmalı
        let sentinels_intact = unsafe {
            BACKBUFFER[head_idx] == SENTINEL_HEAD
                && BACKBUFFER[mid_idx] == SENTINEL_MID
                && BACKBUFFER[tail_idx] == SENTINEL_TAIL
        };

        // Test tamamlandıktan sonra sentinel konumlarını orijinal haline geri yükle
        unsafe {
            BACKBUFFER[head_idx] = orig_head;
            BACKBUFFER[mid_idx] = orig_mid;
            BACKBUFFER[tail_idx] = orig_tail;
        }

        if !sentinels_intact || !corner_written {
            return false;
        }

        // Kırpma (clipping) sınır testi
        self.set_clip(10, 10, 20, 20);
        let clip = self.clip();
        let clip_ok = clip.x0 == 10 && clip.y0 == 10 && clip.x1 == 30 && clip.y1 == 30;

        // Kırpma dışına piksel çizilemediğini doğrula
        let clip_out_idx = 5 * self.width + 5;
        let orig_clip_out = unsafe { BACKBUFFER[clip_out_idx] };
        self.put_pixel(5, 5, 0x00123456);
        let clip_protected = unsafe { BACKBUFFER[clip_out_idx] == orig_clip_out };

        self.reset_clip();

        clip_ok && clip_protected
    }

    /// İmlecin kaplayacağı arka planı, kısmi imleç çizimleri için saklar.
    /// Donanım imleci aktifse yalnızca konumu GPU'ya bildirir.
    pub fn save_cursor_background(&mut self, x: usize, y: usize) {
        // Konumu her durumda GPU'ya bildir (VMware/VGA HW hazır olunca kullanılır).
        crate::drivers::gpu::hw_cursor_move(x.min(32767) as u16, y.min(32767) as u16);
        // Yeniden boyutlandırma imleci fare noktasının 3 piksel üst/soluna taşar.
        self.cursor_x = x.saturating_sub(3).min(self.width);
        self.cursor_y = y.saturating_sub(3).min(self.height);
        self.cursor_w = CURSOR_SAVE_W.min(self.width.saturating_sub(self.cursor_x));
        self.cursor_h = CURSOR_SAVE_H.min(self.height.saturating_sub(self.cursor_y));
        for row in 0..self.cursor_h {
            for col in 0..self.cursor_w {
                let offset = (self.cursor_y + row) * self.width + self.cursor_x + col;
                self.cursor_under[row * CURSOR_SAVE_W + col] = unsafe {
                    if offset < BACKBUFFER_LEN { BACKBUFFER[offset] } else { 0 }
                };
            }
        }
        self.cursor_saved = true;
    }

    /// Önceki imlecin arka planını geri yükler ve güncellenmesi gereken alanı döndürür.
    pub fn restore_cursor_background(&mut self) -> Option<(usize, usize, usize, usize)> {
        if !self.cursor_saved { return None; }
        for row in 0..self.cursor_h {
            for col in 0..self.cursor_w {
                let offset = (self.cursor_y + row) * self.width + self.cursor_x + col;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = self.cursor_under[row * CURSOR_SAVE_W + col];
                    }
                }
            }
        }
        self.cursor_saved = false;
        Some((self.cursor_x, self.cursor_y, self.cursor_w, self.cursor_h))
    }

    pub fn cursor_rect(&self) -> Option<(usize, usize, usize, usize)> {
        if self.cursor_saved {
            Some((self.cursor_x, self.cursor_y, self.cursor_w, self.cursor_h))
        } else {
            None
        }
    }

    /// Yalnızca verilen dikdörtgeni framebuffer'a kopyalar (Donanım 2D Hızlandırmalı Bit Blit).
    pub fn flush_rect(&mut self, rect: (usize, usize, usize, usize)) {
        let (x, y, w, h) = rect;
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        if x >= x_end || y >= y_end { return; }
        let copy_w = x_end - x;
        let copy_h = y_end - y;
        let pitch_words = self.pitch / 4;
        let src_ptr = core::ptr::addr_of_mut!(BACKBUFFER) as *const u32;

        crate::drivers::gpu::hw_accel_bit_blit(
            src_ptr,
            self.width,
            self.address,
            pitch_words,
            x,
            y,
            x,
            y,
            copy_w,
            copy_h,
        );
    }

    /// Aktif kırpma bölgesini ayarlar (verilen dikdörtgene kısıtlar).
    pub fn set_clip(&mut self, x: usize, y: usize, w: usize, h: usize) {
        self.clip = ClipRect {
            x0: x.min(self.width),
            y0: y.min(self.height),
            x1: (x + w).min(self.width),
            y1: (y + h).min(self.height),
        };
    }

    /// Kırpma bölgesini tüm ekrana geri alır.
    pub fn reset_clip(&mut self) {
        self.clip = ClipRect { x0: 0, y0: 0, x1: self.width, y1: self.height };
    }

    /// Aktif kırpma bölgesi.
    #[inline(always)]
    pub fn clip(&self) -> ClipRect {
        self.clip
    }

    /// Bir noktanın aktif kırpma bölgesi içinde olup olmadığı.
    #[inline(always)]
    fn in_clip(&self, x: usize, y: usize) -> bool {
        x >= self.clip.x0 && x < self.clip.x1 && y >= self.clip.y0 && y < self.clip.y1
    }

    #[inline(always)]
    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if self.in_clip(x, y) {
            let c = rgb(color);
            unsafe {
                let offset = y * self.width + x;
                if offset < BACKBUFFER_LEN {
                    BACKBUFFER[offset] = c;
                }
            }
        }
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            let offset = y * self.width + x;
            unsafe {
                if offset < BACKBUFFER_LEN {
                    return BACKBUFFER[offset];
                }
            }
        }
        0
    }

    /// RGB alt piksel kapsamıyla mevcut arka plan üzerine metin pikseli karıştırır.
    /// `coverage` her kanal için 0..255 aralığında örtüşme miktarıdır.
    pub fn put_subpixel_text(&mut self, x: usize, y: usize, color: u32, coverage: [u8; 3]) {
        self.blend_subpixel_text(x, y, color, coverage, false);
    }

    pub fn put_subpixel_text_contrast(&mut self, x: usize, y: usize, color: u32, coverage: [u8; 3]) {
        self.blend_subpixel_text(x, y, color, coverage, true);
    }

    fn blend_subpixel_text(&mut self, x: usize, y: usize, color: u32, coverage: [u8; 3], ensure_contrast: bool) {
        if !self.in_clip(x, y) { return; }
        let fg = rgb(color);
        let offset = y * self.width + x;
        unsafe {
            if offset >= BACKBUFFER_LEN { return; }
            let bg = BACKBUFFER[offset];
            let fg = if ensure_contrast { readable_text_color(fg, bg) } else { fg };
            let mut out = 0u32;
            for channel in 0..3 {
                let shift = 16 - channel * 8;
                let f = (fg >> shift) & 0xFF;
                let b = (bg >> shift) & 0xFF;
                let a = coverage[channel] as u32;
                out |= (b * (255 - a) + f * a + 127) / 255 << shift;
            }
            BACKBUFFER[offset] = out;
        }
    }

    pub fn clear(&mut self, color: u32) {
        let c = rgb(color);
        unsafe {
            let limit = (self.width * self.height).min(BACKBUFFER_LEN);
            BACKBUFFER[..limit].fill(c);
        }
    }

    /// Backbuffer'ın mevcut piksel içeriğini hedef arabelleğe kopyalar.
    pub unsafe fn copy_backbuffer_to_raw(&self, dst: *mut u32, max_len: usize) {
        let len = (self.width * self.height).min(BACKBUFFER_LEN).min(max_len);
        core::ptr::copy_nonoverlapping(core::ptr::addr_of!(BACKBUFFER) as *const u32, dst, len);
    }

    /// Kaynak piksel arabelleğini doğrudan backbuffer'a kopyalar.
    pub unsafe fn copy_backbuffer_from_raw(&mut self, src: *const u32, max_len: usize) {
        let len = (self.width * self.height).min(BACKBUFFER_LEN).min(max_len);
        core::ptr::copy_nonoverlapping(src, core::ptr::addr_of_mut!(BACKBUFFER) as *mut u32, len);
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let c = rgb(color);
        // Kırpma bölgesi ile kesiştir.
        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }
        let fill_w = x_end - x0;
        let fill_h = y_end - y0;
        let dst_ptr = core::ptr::addr_of_mut!(BACKBUFFER) as *mut u32;

        crate::drivers::gpu::hw_accel_fill_rect(
            dst_ptr,
            self.width,
            x0,
            y0,
            fill_w,
            fill_h,
            c,
        );
    }

    /// 1px kalınlığında içi boş dikdörtgen (çerçeve).
    pub fn draw_rect_outline(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.draw_rect(x, y, w, 1, color);
        self.draw_rect(x, y + h.saturating_sub(1), w, 1, color);
        self.draw_rect(x, y, 1, h, color);
        self.draw_rect(x + w.saturating_sub(1), y, 1, h, color);
    }

    /// İki nokta arasına 1px kalınlığında çizgi çizer.
    pub fn draw_line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
        if y0 == y1 {
            let (min_x, max_x) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
            self.draw_rect(min_x, y0, max_x - min_x + 1, 1, color);
            return;
        }
        if x0 == x1 {
            let (min_y, max_y) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
            self.draw_rect(x0, min_y, 1, max_y - min_y + 1, color);
            return;
        }
        let mut x = x0 as isize;
        let mut y = y0 as isize;
        let dx = (x1 as isize - x0 as isize).abs();
        let dy = -(y1 as isize - y0 as isize).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x >= 0 && y >= 0 {
                self.draw_rect(x as usize, y as usize, 1, 1, color);
            }
            if x == x1 as isize && y == y1 as isize {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Yukarıdan aşağıya doğrusal renk geçişi (dikey gradyan).
    pub fn draw_vgradient(&mut self, x: usize, y: usize, w: usize, h: usize, top: u32, bottom: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let ct = rgb(top);
        let cb = rgb(bottom);
        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }
        let n = h.saturating_sub(1).max(1);
        let ar = (ct >> 16) & 0xFF;
        let ag = (ct >> 8) & 0xFF;
        let ab = ct & 0xFF;
        let br = (cb >> 16) & 0xFF;
        let bg = (cb >> 8) & 0xFF;
        let bb = cb & 0xFF;

        for cy in y0..y_end {
            let t = cy - y;
            let inv = n - t;
            let row_dither = BAYER_4X4[cy & 3];
            let mut dither_pixels = [0u32; 4];
            for i in 0..4 {
                let d = row_dither[i];
                let r = (((ar * inv as u32 + br * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                let g = (((ag * inv as u32 + bg * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                let bl = (((ab * inv as u32 + bb * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                dither_pixels[i] = (r << 16) | (g << 8) | bl;
            }

            let row_offset = cy * self.width;
            let start = row_offset + x0;
            let end = row_offset + x_end;
            unsafe {
                if start < BACKBUFFER_LEN {
                    let real_end = end.min(BACKBUFFER_LEN);
                    let slice = &mut BACKBUFFER[start..real_end];
                    for (idx, p) in slice.iter_mut().enumerate() {
                        *p = dither_pixels[(x0 + idx) & 3];
                    }
                }
            }
        }
    }

    /// Yuvarlatılmış köşeli dolu dikdörtgen.
    pub fn draw_rounded_rect(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2);
        if r == 0 {
            self.draw_rect(x, y, w, h, color);
            return;
        }
        let c = rgb(color);
        let r_sq = (r * r) as isize;

        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }

        let mid_y0 = (y + r).max(y0);
        let mid_y1 = (y + h.saturating_sub(r)).min(y_end);

        // Üst köşe satırları
        for cy in y0..mid_y0.min(y_end) {
            let dy = (y + r - 1) as isize - cy as isize;
            self.draw_rounded_row(x, w, x0, x_end, cy, r, dy, r_sq, c);
        }

        // Orta gövde satırları (doğrudan hızlı bellek dolgusu)
        for cy in mid_y0..mid_y1 {
            let row_offset = cy * self.width;
            let start = row_offset + x0;
            let end = row_offset + x_end;
            unsafe {
                if start < BACKBUFFER_LEN {
                    let real_end = end.min(BACKBUFFER_LEN);
                    BACKBUFFER[start..real_end].fill(c);
                }
            }
        }

        // Alt köşe satırları
        for cy in mid_y1.max(y0)..y_end {
            let dy = cy as isize - (y + h - r) as isize;
            self.draw_rounded_row(x, w, x0, x_end, cy, r, dy, r_sq, c);
        }
    }

    /// Yuvarlatılmış köşeli yarı saydam (alpha blended) dolu dikdörtgen.
    /// `alpha`: 0 (tam şeffaf) .. 255 (tam opak). %50 saydamlık için 128 verilebilir.
    pub fn draw_rounded_rect_alpha(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, color: u32, alpha: u8) {
        if w == 0 || h == 0 || alpha == 0 {
            return;
        }
        if alpha == 255 {
            self.draw_rounded_rect(x, y, w, h, r, color);
            return;
        }
        let r = r.min(w / 2).min(h / 2);
        let c = rgb(color);
        let br = (c >> 16) & 0xFF;
        let bg = (c >> 8) & 0xFF;
        let bb = c & 0xFF;

        let a = alpha as u32;
        let inv_a = 256 - a;
        let br_a = br * a;
        let bg_a = bg * a;
        let bb_a = bb * a;

        let r_sq = (r * r) as isize;

        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }

        let blend_pixel = |dst: u32| -> u32 {
            let dr = (dst >> 16) & 0xFF;
            let dg = (dst >> 8) & 0xFF;
            let db = dst & 0xFF;
            let nr = (dr * inv_a + br_a) >> 8;
            let ng = (dg * inv_a + bg_a) >> 8;
            let nb = (db * inv_a + bb_a) >> 8;
            (nr << 16) | (ng << 8) | nb
        };

        let mid_y0 = (y + r).max(y0);
        let mid_y1 = (y + h.saturating_sub(r)).min(y_end);

        // Satır bazında alfa harmanlayıcı yardımcı
        let blend_row = |cy: usize, dy: isize, check_corners: bool| {
            let row_offset = cy * self.width;
            let left_corner_end = if check_corners { (x + r).min(x_end) } else { x0 };
            let right_corner_start = if check_corners { (x + w.saturating_sub(r)).max(x0) } else { x_end };

            if check_corners {
                for cx in x0..left_corner_end {
                    let dx = (x + r - 1) as isize - cx as isize;
                    if dx * dx + dy * dy <= r_sq {
                        let offset = row_offset + cx;
                        unsafe {
                            if offset < BACKBUFFER_LEN {
                                BACKBUFFER[offset] = blend_pixel(BACKBUFFER[offset]);
                            }
                        }
                    }
                }
            }

            for cx in left_corner_end..right_corner_start {
                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = blend_pixel(BACKBUFFER[offset]);
                    }
                }
            }

            if check_corners {
                for cx in right_corner_start..x_end {
                    let dx = cx as isize - (x + w - r) as isize;
                    if dx * dx + dy * dy <= r_sq {
                        let offset = row_offset + cx;
                        unsafe {
                            if offset < BACKBUFFER_LEN {
                                BACKBUFFER[offset] = blend_pixel(BACKBUFFER[offset]);
                            }
                        }
                    }
                }
            }
        };

        // Üst köşe satırları
        for cy in y0..mid_y0.min(y_end) {
            let dy = (y + r - 1) as isize - cy as isize;
            blend_row(cy, dy, true);
        }

        // Orta gövde satırları
        for cy in mid_y0..mid_y1 {
            blend_row(cy, 0, false);
        }

        // Alt köşe satırları
        for cy in mid_y1.max(y0)..y_end {
            let dy = cy as isize - (y + h - r) as isize;
            blend_row(cy, dy, true);
        }
    }

    #[inline(always)]
    fn draw_rounded_row(
        &mut self,
        x: usize,
        w: usize,
        x0: usize,
        x_end: usize,
        cy: usize,
        r: usize,
        dy: isize,
        r_sq: isize,
        color: u32,
    ) {
        let row_offset = cy * self.width;
        let left_corner_end = (x + r).min(x_end);
        let right_corner_start = (x + w.saturating_sub(r)).max(x0);

        // Sol köşe eğrisi
        for cx in x0..left_corner_end {
            let dx = (x + r - 1) as isize - cx as isize;
            if dx * dx + dy * dy <= r_sq {
                unsafe {
                    let offset = row_offset + cx;
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = color;
                    }
                }
            }
        }

        // Satırın düz orta kısmı (hızlı dolgu)
        let mid_start = left_corner_end.max(x0);
        let mid_end = right_corner_start.min(x_end);
        if mid_start < mid_end {
            let start = row_offset + mid_start;
            let end = row_offset + mid_end;
            unsafe {
                if start < BACKBUFFER_LEN {
                    let real_end = end.min(BACKBUFFER_LEN);
                    BACKBUFFER[start..real_end].fill(color);
                }
            }
        }

        // Sağ köşe eğrisi
        for cx in right_corner_start..x_end {
            let dx = cx as isize - (x + w - r) as isize;
            if dx * dx + dy * dy <= r_sq {
                unsafe {
                    let offset = row_offset + cx;
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = color;
                    }
                }
            }
        }
    }

    /// Yuvarlatılmış köşeli 1px çerçeve.
    pub fn draw_rounded_rect_outline(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2);
        if r == 0 {
            self.draw_rect_outline(x, y, w, h, color);
            return;
        }
        let c = rgb(color);
        let r_outer_sq = (r * r) as isize;
        let r_inner = if r > 1 { r - 1 } else { 0 };
        let r_inner_sq = (r_inner * r_inner) as isize;

        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }

        for cy in y0..y_end {
            let is_edge_y = cy == y || cy == y + h - 1;
            let dy = if cy < y + r {
                (y + r - 1) as isize - cy as isize
            } else if cy >= y + h - r {
                cy as isize - (y + h - r) as isize
            } else {
                0
            };

            let row_offset = cy * self.width;
            for cx in x0..x_end {
                let is_edge_x = cx == x || cx == x + w - 1;

                let dx = if cx < x + r {
                    (x + r - 1) as isize - cx as isize
                } else if cx >= x + w - r {
                    cx as isize - (x + w - r) as isize
                } else {
                    0
                };

                let d_sq = dx * dx + dy * dy;
                let is_stroke = if dx > 0 || dy > 0 {
                    d_sq <= r_outer_sq && d_sq >= r_inner_sq
                } else {
                    is_edge_x || is_edge_y
                };

                if is_stroke {
                    let offset = row_offset + cx;
                    unsafe {
                        if offset < BACKBUFFER_LEN {
                            BACKBUFFER[offset] = c;
                        }
                    }
                }
            }
        }
    }

    /// Yuvarlatılmış köşeli dikey gradyan.
    pub fn draw_rounded_vgradient(&mut self, x: usize, y: usize, w: usize, h: usize, r: usize, top: u32, bottom: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2);
        if r == 0 {
            self.draw_vgradient(x, y, w, h, top, bottom);
            return;
        }
        let ct = rgb(top);
        let cb = rgb(bottom);
        let r_sq = (r * r) as isize;

        let x0 = x.max(self.clip.x0);
        let y0 = y.max(self.clip.y0);
        let x_end = (x + w).min(self.width).min(self.clip.x1);
        let y_end = (y + h).min(self.height).min(self.clip.y1);
        if x0 >= x_end || y0 >= y_end {
            return;
        }

        let n = h.saturating_sub(1).max(1);
        let ar = (ct >> 16) & 0xFF;
        let ag = (ct >> 8) & 0xFF;
        let ab = ct & 0xFF;
        let br = (cb >> 16) & 0xFF;
        let bg = (cb >> 8) & 0xFF;
        let bb = cb & 0xFF;

        let mid_y0 = (y + r).max(y0);
        let mid_y1 = (y + h.saturating_sub(r)).min(y_end);
        let left_corner_end = (x + r).min(x_end);
        let right_corner_start = (x + w.saturating_sub(r)).max(x0);

        for cy in y0..y_end {
            let t = cy - y;
            let inv = n - t;
            let row_dither = BAYER_4X4[cy & 3];
            let mut dither_pixels = [0u32; 4];
            for i in 0..4 {
                let d = row_dither[i];
                let r_val = (((ar * inv as u32 + br * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                let g_val = (((ag * inv as u32 + bg * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                let b_val = (((ab * inv as u32 + bb * t as u32) as i32 + d * (n as i32) / 16) / n as i32).clamp(0, 255) as u32;
                dither_pixels[i] = (r_val << 16) | (g_val << 8) | b_val;
            }

            let row_offset = cy * self.width;

            // Orta gövde satırları: köşe testi yok, doğrudan satır dolgusu
            if cy >= mid_y0 && cy < mid_y1 {
                let start = row_offset + x0;
                let end = row_offset + x_end;
                unsafe {
                    if start < BACKBUFFER_LEN {
                        let real_end = end.min(BACKBUFFER_LEN);
                        let slice = &mut BACKBUFFER[start..real_end];
                        for (idx, p) in slice.iter_mut().enumerate() {
                            *p = dither_pixels[(x0 + idx) & 3];
                        }
                    }
                }
            } else {
                let dy = if cy < y + r {
                    (y + r - 1) as isize - cy as isize
                } else {
                    cy as isize - (y + h - r) as isize
                };

                // Sol köşe
                for cx in x0..left_corner_end {
                    let dx = (x + r - 1) as isize - cx as isize;
                    if dx * dx + dy * dy <= r_sq {
                        unsafe {
                            let offset = row_offset + cx;
                            if offset < BACKBUFFER_LEN {
                                BACKBUFFER[offset] = dither_pixels[cx & 3];
                            }
                        }
                    }
                }

                // Düz orta kısım
                let mid_start = left_corner_end.max(x0);
                let mid_end = right_corner_start.min(x_end);
                if mid_start < mid_end {
                    let start = row_offset + mid_start;
                    let end = row_offset + mid_end;
                    unsafe {
                        if start < BACKBUFFER_LEN {
                            let real_end = end.min(BACKBUFFER_LEN);
                            let slice = &mut BACKBUFFER[start..real_end];
                            for (idx, p) in slice.iter_mut().enumerate() {
                                *p = dither_pixels[(mid_start + idx) & 3];
                            }
                        }
                    }
                }

                // Sağ köşe
                for cx in right_corner_start..x_end {
                    let dx = cx as isize - (x + w - r) as isize;
                    if dx * dx + dy * dy <= r_sq {
                        unsafe {
                            let offset = row_offset + cx;
                            if offset < BACKBUFFER_LEN {
                                BACKBUFFER[offset] = dither_pixels[cx & 3];
                            }
                        }
                    }
                }
            }
        }
    }

    /// Yumuşak gölge (cast shadow) çizer: pencere alt/sağ kenarına saydamlaşan koyu halka.
    pub fn draw_shadow(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let cx = rgb(color);
        let br = (cx >> 16) & 0xFF;
        let bg = (cx >> 8) & 0xFF;
        let bb = cx & 0xFF;
        // Alt kenar (yumuşak katman)
        for i in 0..5 {
            let alpha = (150 - i * 28).max(0) as u32;
            let inv_alpha = 256 - alpha;
            let br_a = br * alpha;
            let bg_a = bg * alpha;
            let bb_a = bb * alpha;
            let yy = y + h + i;
            if yy < self.clip.y0 || yy >= self.clip.y1 || yy >= self.height {
                continue;
            }
            let x0 = x.saturating_sub(2).max(self.clip.x0);
            let x1 = (x + w + 3).min(self.width).min(self.clip.x1);
            if x0 >= x1 { continue; }
            let row_off = yy * self.width;
            for px in x0..x1 {
                let off = row_off + px;
                unsafe {
                    if off < BACKBUFFER_LEN {
                        let base = BACKBUFFER[off];
                        let r = ((base >> 16) & 0xFF) as u32;
                        let g = ((base >> 8) & 0xFF) as u32;
                        let b = base & 0xFF;
                        let nr = (r * inv_alpha + br_a) >> 8;
                        let ng = (g * inv_alpha + bg_a) >> 8;
                        let nb = (b * inv_alpha + bb_a) >> 8;
                        BACKBUFFER[off] = (nr << 16) | (ng << 8) | nb;
                    }
                }
            }
        }
        // Sağ kenar
        for i in 0..3 {
            let alpha = (110 - i * 30).max(0) as u32;
            let inv_alpha = 256 - alpha;
            let br_a = br * alpha;
            let bg_a = bg * alpha;
            let bb_a = bb * alpha;
            let xx = x + w + i;
            if xx < self.clip.x0 || xx >= self.clip.x1 || xx >= self.width {
                continue;
            }
            let y0 = (y + 2).max(self.clip.y0);
            let y1 = (y + h).min(self.height).min(self.clip.y1);
            for py in y0..y1 {
                let off = py * self.width + xx;
                unsafe {
                    if off < BACKBUFFER_LEN {
                        let base = BACKBUFFER[off];
                        let r = ((base >> 16) & 0xFF) as u32;
                        let g = ((base >> 8) & 0xFF) as u32;
                        let b = base & 0xFF;
                        let nr = (r * inv_alpha + br_a) >> 8;
                        let ng = (g * inv_alpha + bg_a) >> 8;
                        let nb = (b * inv_alpha + bb_a) >> 8;
                        BACKBUFFER[off] = (nr << 16) | (ng << 8) | nb;
                    }
                }
            }
        }
    }

    pub fn flush(&mut self) {
        let pitch_words = self.pitch / 4;
        unsafe {
            if pitch_words == self.width {
                let total_pixels = (self.width * self.height).min(BACKBUFFER_LEN);
                core::ptr::copy_nonoverlapping(core::ptr::addr_of_mut!(BACKBUFFER) as *mut u32, self.address, total_pixels);
            } else {
                for y in 0..self.height {
                    let src_offset = y * self.width;
                    let dst_offset = y * pitch_words;
                    let row_len = self.width.min(BACKBUFFER_LEN.saturating_sub(src_offset));
                    core::ptr::copy_nonoverlapping(
                        (core::ptr::addr_of_mut!(BACKBUFFER) as *mut u32).add(src_offset),
                        self.address.add(dst_offset),
                        row_len,
                    );
                }
            }
        }
    }

    /// Mavi ışık filtresini seçilen yüzdeyle uygular (0 kapalı, 100 en sıcak).
    pub fn apply_night_light(&mut self, strength: u8) {
        let strength = strength.min(100) as u32;
        if strength == 0 { return; }
        let limit=(self.width*self.height).min(BACKBUFFER_LEN);
        unsafe { for i in 0..limit { let c=BACKBUFFER[i]; let r=(c>>16)&255; let g=(c>>8)&255; let b=c&255; let nr=(r+((255-r)*12*strength/10_000)).min(255); let ng=(g+((255-g)*4*strength/10_000)).min(255); let nb=b*(100-45*strength/100)/100; BACKBUFFER[i]=(nr<<16)|(ng<<8)|nb; } }
    }
}

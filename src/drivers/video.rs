use crate::gui::theme;

static mut BACKBUFFER: [u32; 1280 * 720] = [0; 1280 * 720];
const BACKBUFFER_LEN: usize = 1280 * 720;
const CURSOR_SAVE_W: usize = 16;
const CURSOR_SAVE_H: usize = 16;

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
    /// Aktif kırpma bölgesi. Varsayılan: tüm ekran.
    clip: ClipRect,
    cursor_under: [u32; CURSOR_SAVE_W * CURSOR_SAVE_H],
    cursor_x: usize,
    cursor_y: usize,
    cursor_w: usize,
    cursor_h: usize,
    cursor_saved: bool,
}

#[inline(always)]
fn rgb(color: u32) -> u32 {
    theme::resolve(color)
}

/// 4x4 Bayer dither matrisi - renk geçişlerinde kademelenmeyi (color banding) tamamen ortadan kaldırır.
const BAYER_4X4: [[i32; 4]; 4] = [
    [-8,  0, -6,  2],
    [ 4, -4,  6, -2],
    [-5,  3, -7,  1],
    [ 7, -1,  5, -3],
];

/// Dithering destekli pürüzsüz renk geçişi (kademelenmeyi yok eder).
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

impl Framebuffer {
    pub unsafe fn new(address: *mut u32, width: usize, height: usize, pitch: usize) -> Self {
        Self {
            address,
            width,
            height,
            pitch,
            clip: ClipRect { x0: 0, y0: 0, x1: width, y1: height },
            cursor_under: [0; CURSOR_SAVE_W * CURSOR_SAVE_H],
            cursor_x: 0,
            cursor_y: 0,
            cursor_w: 0,
            cursor_h: 0,
            cursor_saved: false,
        }
    }

    /// İmlecin kaplayacağı arka planı, kısmi imleç çizimleri için saklar.
    pub fn save_cursor_background(&mut self, x: usize, y: usize) {
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

    /// Yalnızca verilen dikdörtgeni framebuffer'a kopyalar.
    pub fn flush_rect(&mut self, rect: (usize, usize, usize, usize)) {
        let (x, y, w, h) = rect;
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        if x >= x_end || y >= y_end { return; }
        let pitch_words = self.pitch / 4;
        for row in y..y_end {
            let src_offset = row * self.width + x;
            if src_offset >= BACKBUFFER_LEN { continue; }
            let len = (x_end - x).min(BACKBUFFER_LEN - src_offset);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (core::ptr::addr_of_mut!(BACKBUFFER) as *mut u32).add(src_offset),
                    self.address.add(row * pitch_words + x),
                    len,
                );
            }
        }
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

    /// RGB alt piksel kapsamıyla mevcut arka plan üzerine metin pikseli karıştırır.
    /// `coverage` her kanal için 0..255 aralığında örtüşme miktarıdır.
    pub fn put_subpixel_text(&mut self, x: usize, y: usize, color: u32, coverage: [u8; 3]) {
        if !self.in_clip(x, y) { return; }
        let fg = rgb(color);
        let offset = y * self.width + x;
        unsafe {
            if offset >= BACKBUFFER_LEN { return; }
            let bg = BACKBUFFER[offset];
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
            for i in 0..limit {
                BACKBUFFER[i] = c;
            }
        }
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
        for cy in y0..y_end {
            let row_offset = cy * self.width;
            for cx in x0..x_end {
                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = c;
                    }
                }
            }
        }
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
        if x0 >= x_end {
            return;
        }
        let n = h.saturating_sub(1).max(1);
        for cy in y0..y_end {
            let t = cy - y;
            let row_offset = cy * self.width;
            for cx in x0..x_end {
                let c = lerp_color_dither(ct, cb, t, n, cx, cy);
                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = c;
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

        for cy in y0..y_end {
            let dy = if cy < y + r {
                (y + r - 1) as isize - cy as isize
            } else if cy >= y + h - r {
                cy as isize - (y + h - r) as isize
            } else {
                0
            };

            let row_offset = cy * self.width;
            for cx in x0..x_end {
                let dx = if cx < x + r {
                    (x + r - 1) as isize - cx as isize
                } else if cx >= x + w - r {
                    cx as isize - (x + w - r) as isize
                } else {
                    0
                };

                if dx * dx + dy * dy > r_sq {
                    continue;
                }

                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = c;
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
        for cy in y0..y_end {
            let dy = if cy < y + r {
                (y + r - 1) as isize - cy as isize
            } else if cy >= y + h - r {
                cy as isize - (y + h - r) as isize
            } else {
                0
            };

            let t = cy - y;
            let row_offset = cy * self.width;

            for cx in x0..x_end {
                let dx = if cx < x + r {
                    (x + r - 1) as isize - cx as isize
                } else if cx >= x + w - r {
                    cx as isize - (x + w - r) as isize
                } else {
                    0
                };

                if dx * dx + dy * dy > r_sq {
                    continue;
                }

                let c = lerp_color_dither(ct, cb, t, n, cx, cy);
                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = c;
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
        // alpha: 256 = tamamen gölge
        let blend_px = |fb: &mut Framebuffer, px: usize, py: usize, alpha: u32| {
            if !fb.in_clip(px, py) {
                return;
            }
            let off = py * fb.width + px;
            if off >= BACKBUFFER_LEN {
                return;
            }
            unsafe {
                let base = BACKBUFFER[off];
                let r = ((base >> 16) & 0xFF) as u32;
                let g = ((base >> 8) & 0xFF) as u32;
                let b = base & 0xFF;
                let nr = (r * (256 - alpha) + br * alpha) >> 8;
                let ng = (g * (256 - alpha) + bg * alpha) >> 8;
                let nb = (b * (256 - alpha) + bb * alpha) >> 8;
                BACKBUFFER[off] = (nr << 16) | (ng << 8) | nb;
            }
        };
        // Alt kenar (yumuşak katman)
        for i in 0..5 {
            let alpha = 150 - i * 28;
            let yy = y + h + i;
            let x0 = x.saturating_sub(2);
            let x1 = (x + w + 3).min(self.width);
            for px in x0..x1 {
                blend_px(self, px, yy, alpha.max(0) as u32);
            }
        }
        // Sağ kenar
        for i in 0..3 {
            let alpha = 110 - i * 30;
            let xx = x + w + i;
            for py in y + 2..(y + h).min(self.height) {
                blend_px(self, xx, py, alpha.max(0) as u32);
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

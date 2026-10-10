static mut BACKBUFFER: [u32; 1280 * 720] = [0; 1280 * 720];
const BACKBUFFER_LEN: usize = 1280 * 720;

pub struct Framebuffer {
    pub address: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
}

impl Framebuffer {
    pub unsafe fn new(address: *mut u32, width: usize, height: usize, pitch: usize) -> Self {
        Self {
            address,
            width,
            height,
            pitch,
        }
    }

    #[inline(always)]
    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            unsafe {
                let offset = y * self.width + x;
                if offset < BACKBUFFER_LEN {
                    BACKBUFFER[offset] = color;
                }
            }
        }
    }

    pub fn clear(&mut self, color: u32) {
        unsafe {
            let limit = (self.width * self.height).min(BACKBUFFER_LEN);
            for i in 0..limit {
                BACKBUFFER[i] = color;
            }
        }
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for cy in y..y_end {
            let row_offset = cy * self.width;
            for cx in x..x_end {
                let offset = row_offset + cx;
                unsafe {
                    if offset < BACKBUFFER_LEN {
                        BACKBUFFER[offset] = color;
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
}

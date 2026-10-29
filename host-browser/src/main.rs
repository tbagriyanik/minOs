#![no_std]
#![no_main]

extern crate alloc;

use linked_list_allocator::LockedHeap;

use core::panic::PanicInfo;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_SIZE: usize = 16 * 1024 * 1024;

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

mod app;
mod color;
mod css;
mod html;
mod imgdecode;
mod types;

use host_browser::app::{HostBrowser, Painter};

// Framebuffer işaretçisi çekirdek tarafından çalışma zamanında ayarlanabilir.
// Şu anda null (no-op); çekirdek entegrasyonu eklendiğinde gerçek FB adresi buraya yazılabilir.
static mut FB_ADDR: *mut u32 = core::ptr::null_mut();
static mut FB_WIDTH: usize = 0;
static mut FB_HEIGHT: usize = 0;
static mut FB_PITCH: usize = 0;

/// 5x7 piksel bitmap font (karakter başına 7 satır, her satır 5 bit: bit0 = soldan ilk sütun).
/// Karakterler 32-126 arası tanımlı; diğerleri boş bırakılır.
const FONT_5X7: [[u8; 7]; 120] = [
    // 0-31: boş
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0],
    // 32 space
    [0, 0, 0, 0, 0, 0, 0],
    // 33 !
    [4, 4, 4, 4, 4, 0, 4],
    // 34 "
    [14, 0, 14, 0, 14, 0, 0],
    // 35 #
    [4, 21, 4, 21, 4, 4, 4],
    // 36 $
    [4, 10, 21, 10, 4, 4, 4],
    // 37 %
    [17, 10, 4, 10, 17, 4, 4],
    // 38 &
    [14, 17, 23, 17, 14, 0, 0],
    // 39 '
    [4, 0, 0, 0, 0, 0, 0],
    // 40 (
    [4, 8, 16, 8, 4, 0, 0],
    // 41 )
    [4, 2, 1, 2, 4, 0, 0],
    // 42 *
    [4, 10, 17, 4, 17, 10, 4],
    // 43 +
    [0, 4, 4, 31, 4, 4, 0],
    // 44 ,
    [0, 0, 0, 4, 4, 8, 0],
    // 45 -
    [0, 0, 0, 31, 0, 0, 0],
    // 46 .
    [0, 0, 0, 0, 4, 0, 0],
    // 47 /
    [16, 8, 4, 2, 1, 0, 0],
    // 48 0
    [14, 17, 19, 21, 25, 17, 14],
    // 49 1
    [4, 12, 4, 4, 4, 4, 31],
    // 50 2
    [14, 17, 1, 6, 8, 16, 31],
    // 51 3
    [14, 17, 1, 6, 1, 17, 14],
    // 52 4
    [1, 3, 5, 9, 31, 1, 1],
    // 53 5
    [31, 16, 30, 1, 1, 17, 14],
    // 54 6
    [14, 16, 30, 17, 17, 17, 14],
    // 55 7
    [31, 1, 2, 4, 8, 8, 8],
    // 56 8
    [14, 17, 17, 14, 17, 17, 14],
    // 57 9
    [14, 17, 17, 15, 1, 17, 14],
    // 58 :
    [0, 4, 0, 4, 0, 0, 0],
    // 59 ;
    [0, 4, 0, 4, 4, 8, 0],
    // 60 <
    [1, 2, 4, 8, 4, 2, 1],
    // 61 =
    [0, 0, 31, 0, 31, 0, 0],
    // 62 >
    [16, 8, 4, 2, 4, 8, 16],
    // 63 ?
    [14, 17, 1, 4, 8, 0, 8],
    // 64 @
    [14, 17, 21, 21, 21, 17, 14],
    // 65 A
    [4, 10, 17, 31, 17, 17, 17],
    // 66 B
    [30, 17, 17, 30, 17, 17, 30],
    // 67 C
    [14, 17, 16, 16, 16, 17, 14],
    // 68 D
    [30, 17, 17, 17, 17, 17, 30],
    // 69 E
    [31, 16, 16, 30, 16, 16, 31],
    // 70 F
    [31, 16, 16, 30, 16, 16, 16],
    // 71 G
    [14, 17, 16, 23, 17, 17, 14],
    // 72 H
    [17, 17, 17, 31, 17, 17, 17],
    // 73 I
    [31, 4, 4, 4, 4, 4, 31],
    // 74 J
    [1, 1, 1, 1, 17, 17, 14],
    // 75 K
    [17, 18, 20, 24, 20, 18, 17],
    // 76 L
    [16, 16, 16, 16, 16, 16, 31],
    // 77 M
    [17, 27, 21, 21, 17, 17, 17],
    // 78 N
    [17, 25, 21, 19, 17, 17, 17],
    // 79 O
    [14, 17, 17, 17, 17, 17, 14],
    // 80 P
    [30, 17, 17, 30, 16, 16, 16],
    // 81 Q
    [14, 17, 17, 17, 21, 17, 14],
    // 82 R
    [30, 17, 17, 30, 20, 18, 17],
    // 83 S
    [14, 17, 16, 14, 1, 17, 14],
    // 84 T
    [31, 4, 4, 4, 4, 4, 4],
    // 85 U
    [17, 17, 17, 17, 17, 17, 14],
    // 86 V
    [17, 17, 17, 17, 17, 10, 4],
    // 87 W
    [17, 17, 17, 21, 21, 27, 17],
    // 88 X
    [17, 17, 10, 4, 10, 17, 17],
    // 89 Y
    [17, 17, 10, 4, 4, 4, 4],
    // 90 Z
    [31, 1, 2, 4, 8, 16, 31],
    // 91 [
    [31, 16, 16, 16, 16, 17, 14],
    // 92 \
    [16, 8, 4, 2, 1, 0, 0],
    // 93 ]
    [14, 1, 1, 1, 1, 17, 14],
    // 94 ^
    [4, 10, 21, 0, 0, 0, 0],
    // 95 _
    [0, 0, 0, 0, 0, 0, 31],
    // 96 `
    [4, 0, 0, 0, 0, 0, 0],
    // 97 a
    [0, 0, 14, 17, 31, 17, 17],
    // 98 b
    [16, 16, 30, 17, 17, 17, 30],
    // 99 c
    [0, 0, 14, 17, 16, 17, 14],
    // 100 d
    [1, 1, 30, 17, 17, 17, 30],
    // 101 e
    [0, 0, 30, 17, 31, 16, 30],
    // 102 f
    [6, 9, 31, 16, 16, 16, 16],
    // 103 g
    [0, 0, 14, 17, 31, 17, 14],
    // 104 h
    [16, 16, 30, 17, 17, 17, 17],
    // 105 i
    [31, 4, 4, 4, 4, 4, 31],
    // 106 j
    [1, 1, 1, 1, 17, 17, 14],
    // 107 k
    [16, 18, 20, 24, 20, 18, 17],
    // 108 l
    [31, 4, 4, 4, 4, 4, 4],
    // 109 m
    [0, 0, 31, 21, 21, 21, 21],
    // 110 n
    [0, 0, 31, 17, 17, 17, 17],
    // 111 o
    [0, 0, 14, 17, 17, 17, 14],
    // 112 p
    [0, 0, 30, 17, 30, 16, 16],
    // 113 q
    [0, 0, 14, 17, 30, 17, 17],
    // 114 r
    [0, 0, 31, 17, 16, 16, 16],
    // 115 s
    [0, 0, 14, 16, 14, 1, 14],
    // 116 t
    [16, 16, 31, 16, 16, 16, 16],
    // 117 u
    [0, 0, 17, 17, 17, 17, 14],
    // 118 v
    [0, 0, 17, 17, 17, 10, 4],
    // 119 w
    [0, 0, 17, 17, 21, 21, 10],
    // 120 x
    [0, 0, 17, 10, 4, 10, 17],
    // 121 y
    [0, 0, 17, 17, 30, 17, 17],
    // 122 z
    [0, 0, 31, 2, 4, 8, 31],
    // 123 {
    [4, 8, 16, 8, 4, 8, 16],
    // 124 |
    [4, 4, 4, 4, 4, 4, 4],
    // 125 }
    [4, 2, 1, 2, 4, 2, 1],
    // 126 ~
    [0, 0, 10, 21, 0, 0, 0],
    // 127 del
    [0, 0, 0, 0, 0, 0, 0],
];

/// Çekirdek framebuffer üzerine Painter trait'i gerçekleştirici.
#[derive(Copy, Clone)]
struct FbPainter {
    width: usize,
    height: usize,
    pitch: usize,
}

impl Painter for FbPainter {
    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        unsafe {
            let addr = FB_ADDR;
            if addr.is_null() {
                return;
            }
            let pitch_words = self.pitch / 4;
            for dy in 0..h {
                let yy = y + dy;
                if yy >= self.height {
                    break;
                }
                let row_base = yy * pitch_words + x;
                for dx in 0..w {
                    let xx = x + dx;
                    if xx >= self.width {
                        break;
                    }
                    let idx = row_base + dx;
                    if idx < self.width * self.height {
                        *addr.add(idx) = color;
                    }
                }
            }
        }
    }

    fn clear(&mut self, color: u32) {
        self.rect(0, 0, self.width, self.height, color);
    }

    fn text(&mut self, x: usize, y: usize, text: &str, color: u32, scale: usize) {
        unsafe {
            let addr = FB_ADDR;
            if addr.is_null() {
                return;
            }
            let pitch_words = self.pitch / 4;
            let s = scale.max(1);
            let mut cx = x;
            for ch in text.chars() {
                let ci = ch as usize;
                if ci >= 128 {
                    cx += 6 * s;
                    continue;
                }
                let glyph = FONT_5X7[ci];
                for row in 0..7 {
                    let bits = glyph[row];
                    for col in 0..5 {
                        if (bits >> col) & 1 == 1 {
                            for sy in 0..s {
                                for sx in 0..s {
                                    let px = cx + col * s + sx;
                                    let py = y + row * s + sy;
                                    if px < self.width && py < self.height {
                                        let idx = py * pitch_words + px;
                                        if idx < self.width * self.height {
                                            *addr.add(idx) = color;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                cx += 6 * s;
            }
        }
    }

    fn text_width(&self, text: &str, scale: usize) -> usize {
        text.len() * 6 * scale.max(1)
    }

    fn text_height(&self, scale: usize) -> usize {
        7 * scale.max(1)
    }

    fn image(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: &[u8]) {
        unsafe {
            let addr = FB_ADDR;
            if addr.is_null() {
                return;
            }
            let pitch_words = self.pitch / 4;
            for dy in 0..h {
                let yy = y + dy;
                if yy >= self.height {
                    break;
                }
                for dx in 0..w {
                    let xx = x + dx;
                    if xx >= self.width {
                        break;
                    }
                    let i = (dy * w + dx) * 3;
                    if i + 2 >= rgb.len() {
                        break;
                    }
                    let r = rgb[i] as u32;
                    let g = rgb[i + 1] as u32;
                    let b = rgb[i + 2] as u32;
                    let c = (r << 16) | (g << 8) | b;
                    let idx = yy * pitch_words + xx;
                    if idx < self.width * self.height {
                        *addr.add(idx) = c;
                    }
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE);
    }

    let fb_width = unsafe { FB_WIDTH };
    let fb_height = unsafe { FB_HEIGHT };
    let fb_pitch = unsafe { FB_PITCH };

    let painter = FbPainter {
        width: fb_width,
        height: fb_height,
        pitch: fb_pitch,
    };

    let mut browser = HostBrowser::new();
    browser.navigate("minos://help");

    // Ana çizim döngüsü: her çerçevede tarayıcı içeriğini framebuffer'a çizer.
    loop {
        let mut p = painter;
        browser.draw(&mut p, 0, 0, fb_width, fb_height);
        core::hint::spin_loop();
    }
}

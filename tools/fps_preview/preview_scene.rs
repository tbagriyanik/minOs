//! FPS 3D oyununun gerçek sahne render'ını host'ta uretir ve PPM olarak yazar.
//!
//! Bu program, cekirdekteki `fps_game.rs` dosyasini oldugu gibi derleyip
//! gercek `drivers::video::Framebuffer` uzerine cizer. Boylece oyunun
//! gorsel dogrulamasi (doku, sprite, durum cubugu, perspektif) ekran
//! goruntusu olarak incelenebilir.
//!
//! Kullanim:
//!   rustc --edition 2021 tools\fps_preview\preview_scene.rs -o target\fps_preview_scene.exe
//!   target\fps_preview_scene.exe target\fps_scene.ppm 1024 768 0
//!   python tools\ppm2png.py target\fps_scene.ppm target\fps_scene.png
//!
//! Argumanlar: <cikti.ppm> [genislik] [yukseklik] [seviye] [x] [y] [aci]

extern crate alloc;

use std::fs::File;
use std::io::Write;

// ---------------------------------------------------------------------------
// Cekirdek bagimliliklari icin stub'lar
// ---------------------------------------------------------------------------

/// Tema cozumlemesi: tema isareti tasimayan renkleri oldugu gibi gecirir.
pub mod gui {
    pub mod theme {
        #[inline(always)]
        pub fn resolve(color: u32) -> u32 {
            color
        }
    }

    pub use crate::textbuf_real::TextBuf;

    /// Basit arayuz renkleri (host onizleme icin).
    pub struct Color;

    impl Color {
        pub const TITLE_BAR: u32 = 0x2B3240;
        pub const TEXT_MAIN: u32 = 0xE6EDF5;
        pub const TEXT_MUTED: u32 = 0x9AA7B5;
        pub const ACCENT_GREEN: u32 = 0x4CC38A;
        pub const ACCENT_RED: u32 = 0xE05561;
        pub const ACCENT_YELLOW: u32 = 0xE8C547;
        pub const ACCENT_PINK: u32 = 0xE06C9F;
    }

    /// Metin cizimi: host onizlemede yerlesimi gormek icin kaba kutular.
    pub struct MinGuiCompositor;

    impl MinGuiCompositor {
        pub fn text_width(text: &str, scale: usize) -> usize {
            text.chars().count() * 6 * scale
        }

        pub fn draw_string(
            fb: &mut crate::drivers::video::Framebuffer,
            x: usize,
            y: usize,
            text: &str,
            color: u32,
        ) {
            let scale = 1usize;
            let w = Self::text_width(text, scale);
            // Metin arka planini hafif koyulastir, sonra harf basina kutu ciz.
            fb.draw_rect(x, y, w, 7 * scale, 0x000000 | (color & 0x101010));
            for (i, ch) in text.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let gx = x + i * 6 * scale;
                fb.draw_rect(gx, y + 1, 4 * scale, 5 * scale, color);
            }
        }
    }

    /// Turkce/Ingilizce metin secimi: Turkce varsayilan.
    pub fn ui_text<'a>(tr: &'a str, _en: &'a str) -> &'a str {
        tr
    }
}

pub mod drivers {
    pub mod video {
        pub use crate::video_real::*;
    }

    /// Donanım imleci / hızlandırılmış blit: host onizlemede yok.
    pub mod gpu {
        pub fn hw_cursor_enable(_enable: bool) {}
        pub fn hw_cursor_move(_x: u16, _y: u16) {}
        pub unsafe fn hw_accel_bit_blit(
            _src: *const u32,
            _dst: *mut u32,
            _stride: usize,
            _x: usize,
            _y: usize,
            _w: usize,
            _h: usize,
        ) {
        }
        pub unsafe fn hw_accel_fill_rect(
            _dst: *mut u32,
            _width: usize,
            _x0: usize,
            _y0: usize,
            _w: usize,
            _h: usize,
            _color: u32,
        ) {
        }
    }

    /// Virtio-GPU: host onizlemede pasif.
    pub mod virtio_gpu {
        pub fn is_active() -> bool {
            false
        }
        pub fn flush_rect(_x: u32, _y: u32, _w: u32, _h: u32) {}
    }

    pub mod audio {
        /// Ses cikisi: host onizlemede sessiz.
        pub struct AudioDevice {
            pub volume_left: u8,
            pub is_muted: bool,
        }

        impl AudioDevice {
            pub const fn new() -> Self {
                Self {
                    volume_left: 0,
                    is_muted: true,
                }
            }

            pub fn set_pc_speaker_tone(&mut self, _frequency_hz: u32) {}
        }

        impl Default for AudioDevice {
            fn default() -> Self {
                Self::new()
            }
        }
    }
}

pub mod mm {
    pub mod memory {
        pub struct MemStats {
            pub used_bytes: u64,
            pub free_bytes: u64,
            pub heap_committed_bytes: u64,
        }

        pub fn mem_stats() -> MemStats {
            MemStats {
                used_bytes: 0,
                free_bytes: 0,
                heap_committed_bytes: 0,
            }
        }
    }
}

pub mod boot {
    pub mod console {
        pub fn serial_print(text: &str) {
            print!("{text}");
        }

        pub fn serial_print_dec(value: u64) {
            print!("{value}");
        }
    }
}

pub mod apps {
    pub use crate::fps_level_real as fps_level;
    pub use crate::fps_sprites_real as fps_sprites;
    pub use crate::fps_textures_real as fps_textures;
    pub use crate::gfx3d_real as gfx3d;
}

static mut CLOCK_MS: u64 = 0;

pub fn monotonic_ms() -> u64 {
    unsafe {
        CLOCK_MS += 1;
        CLOCK_MS
    }
}

// ---------------------------------------------------------------------------
// Gercek cekirdek modulleri
// ---------------------------------------------------------------------------

#[path = "../../src/gui/textbuf.rs"]
pub mod textbuf_real;

#[path = "../../src/drivers/video.rs"]
pub mod video_real;

#[path = "../../src/time_budget.rs"]
pub mod time_budget;

#[path = "../../src/apps/gfx3d.rs"]
pub mod gfx3d_real;

#[path = "../../src/apps/fps_textures.rs"]
pub mod fps_textures_real;

#[path = "../../src/apps/fps_sprites.rs"]
pub mod fps_sprites_real;

#[path = "../../src/apps/fps_level.rs"]
pub mod fps_level_real;

#[path = "../../src/apps/fps_game.rs"]
pub mod fps_game_real;

// ---------------------------------------------------------------------------
// Onizleme
// ---------------------------------------------------------------------------

fn write_ppm(path: &str, width: usize, height: usize, pixels: &[u32]) {
    let mut file = File::create(path).expect("ppm acilamadi");
    write!(file, "P6\n{width} {height}\n255\n").expect("ppm yazilamadi");
    let mut row = vec![0u8; width * 3];
    for y in 0..height {
        for x in 0..width {
            let c = pixels[y * width + x];
            row[x * 3] = ((c >> 16) & 0xFF) as u8;
            row[x * 3 + 1] = ((c >> 8) & 0xFF) as u8;
            row[x * 3 + 2] = (c & 0xFF) as u8;
        }
        file.write_all(&row).expect("ppm yazilamadi");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/fps_scene.ppm".to_string());
    let width: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(1024);
    let height: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(768);
    let level: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(0);
    let px: f32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(15.5);
    let py: f32 = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(9.5);
    let angle: f32 = args.get(7).and_then(|v| v.parse().ok()).unwrap_or(0.0);

    // Framebuffer: gercek modulu kullanilir, ekran adresi kullanilmaz
    // (backbuffer'a yazilir).
    let mut fb = unsafe {
        crate::drivers::video::Framebuffer::new(
            core::ptr::null_mut(),
            width,
            height,
            width * 4,
            32,
        )
    };

    let mut game = crate::apps::fps_game_real::FpsGameApp::new();
    // Menuye gir, sonra oyuna basla (render Playing durumunu bekler).
    game.set_result_playing_for_test();
    if level > 0 {
        game.load_level_for_test(level);
    }
    game.set_view_for_test(px, py, angle);

    let _ = game.render_content_with_budget(&mut fb, 0, 0, width, height, u64::MAX);

    let mut pixels = vec![0u32; width * height];
    unsafe {
        fb.copy_backbuffer_to_raw(pixels.as_mut_ptr(), pixels.len());
    }
    write_ppm(&out, width, height, &pixels);
    println!("{out} yazildi ({width}x{height})");
}

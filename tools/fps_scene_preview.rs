//! Host tarafında FPS 3B sahne önizlemesi.
//!
//! Çekirdekteki **gerçek** render kodunu (`src/apps/fps_game.rs`) host'ta
//! çalıştırır ve PPM (P6) dosyası üretir. Amaç, 3B geometriyi (zemin, tavan,
//! duvar, düşman) QEMU olmadan görsel olarak doğrulayabilmektir.
//!
//! Kullanım:
//!   rustc --edition 2021 tools/fps_scene_preview.rs -o fps_preview.exe
//!   ./fps_preview.exe <out.ppm> [pitch_deg] [pos_x] [pos_y] [dir_deg]
//!
//! Not: `src/apps/*.rs` modülleri çekirdekte `no_std` çalışır; burada
//! `std` ile derlenirler ve yalnızca ihtiyaç duyulan çekirdek API'leri
//! (framebuffer, ses, zamanlayıcı) minimal olarak taklit edilir.

#![allow(dead_code)]

extern crate alloc;

use std::io::Write;

// ---------------------------------------------------------------------------
// Çekirdek API taklitleri (yalnızca FPS render'ının ihtiyaç duyduğu kadar)
// ---------------------------------------------------------------------------

pub mod drivers {
    pub mod audio {
        pub struct AudioDevice {
            pub is_muted: bool,
            pub volume_left: u8,
        }
        impl AudioDevice {
            pub fn set_pc_speaker_tone(&mut self, _hz: u32) {}
        }
    }

    pub mod keyboard {
        pub struct KeyboardState {
            pub shift: bool,
        }
        impl KeyboardState {
            pub fn is_held(&self, _scancode: u8) -> bool {
                false
            }
            pub fn is_held_extended(&self, _scancode: u8) -> bool {
                false
            }
        }
    }

    pub mod video {
        /// Basit RGB framebuffer (0xRRGGBB).
        pub struct Framebuffer {
            pub width: usize,
            pub height: usize,
            pub pixels: alloc::vec::Vec<u32>,
        }

        impl Framebuffer {
            pub fn new(width: usize, height: usize) -> Self {
                Self {
                    width,
                    height,
                    pixels: alloc::vec![0u32; width * height],
                }
            }

            #[inline(always)]
            pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
                if x < self.width && y < self.height {
                    self.pixels[y * self.width + x] = color & 0xFFFFFF;
                }
            }

            pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
                if w == 0 || h == 0 {
                    return;
                }
                let x1 = x.saturating_add(w).min(self.width);
                let y1 = y.saturating_add(h).min(self.height);
                for py in y..y1 {
                    for px in x..x1 {
                        self.pixels[py * self.width + px] = color & 0xFFFFFF;
                    }
                }
            }

            pub fn draw_rect_outline(
                &mut self,
                x: usize,
                y: usize,
                w: usize,
                h: usize,
                color: u32,
            ) {
                if w == 0 || h == 0 {
                    return;
                }
                self.draw_rect(x, y, w, 1, color);
                self.draw_rect(x, y + h.saturating_sub(1), w, 1, color);
                self.draw_rect(x, y, 1, h, color);
                self.draw_rect(x + w.saturating_sub(1), y, 1, h, color);
            }

            pub fn draw_rounded_rect_alpha(
                &mut self,
                x: usize,
                y: usize,
                w: usize,
                h: usize,
                _r: usize,
                _color: u32,
                _alpha: u8,
            ) {
                self.draw_rect(x, y, w, h, 0x000000);
            }
        }
    }
}

pub mod gui {
    pub struct Color;
    impl Color {
        pub const TITLE_BAR: u32 = 0x1E2430;
        pub const TEXT_MAIN: u32 = 0xE8EEF7;
        pub const TEXT_MUTED: u32 = 0x8A97A8;
        pub const ACCENT_YELLOW: u32 = 0xF0B040;
        pub const ACCENT_RED: u32 = 0xE05A5A;
        pub const ACCENT_GREEN: u32 = 0x60C860;
        pub const ACCENT_BLUE: u32 = 0x60A0F0;
        pub const ACCENT_PINK: u32 = 0xF070C0;
        pub const TASKBAR_BG: u32 = 0x141922;
    }

    pub struct MinGuiCompositor;
    impl MinGuiCompositor {
        pub fn draw_string(
            _fb: &mut crate::drivers::video::Framebuffer,
            _x: usize,
            _y: usize,
            _text: &str,
            _color: u32,
        ) {
        }
        pub fn text_width(_text: &str, _scale: usize) -> usize {
            0
        }
        pub fn shade(color: u32, delta: i32) -> u32 {
            let r = (((color >> 16) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
            let g = (((color >> 8) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
            let b = ((color & 0xFF) as i32 + delta).clamp(0, 255) as u32;
            (r << 16) | (g << 8) | b
        }
    }

    pub fn sp(base: usize) -> usize {
        base
    }

    pub fn ui_text<'a>(tr: &'a str, _en: &'a str) -> &'a str {
        tr
    }

    /// Basit metin tamponu (HUD için).
    pub struct TextBuf {
        pub text: alloc::string::String,
    }
    impl TextBuf {
        pub fn new() -> Self {
            Self {
                text: alloc::string::String::new(),
            }
        }
        pub fn clear(&mut self) {
            self.text.clear();
        }
        pub fn push_str(&mut self, s: &str) {
            self.text.push_str(s);
        }
        pub fn push_u2(&mut self, v: u32) {
            self.text.push_str(&v.to_string());
        }
        pub fn push_usize(&mut self, v: usize) {
            self.text.push_str(&v.to_string());
        }
        pub fn push_i32(&mut self, v: i32) {
            self.text.push_str(&v.to_string());
        }
        pub fn as_str(&self) -> &str {
            &self.text
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

pub mod time_budget {
    pub struct TimeBudget {
        deadline_ms: u64,
    }
    impl TimeBudget {
        pub fn new(budget_ms: u64) -> Self {
            Self {
                deadline_ms: crate::monotonic_ms().saturating_add(budget_ms),
            }
        }
        pub fn is_expired(&self) -> bool {
            crate::monotonic_ms() > self.deadline_ms
        }
        pub fn remaining_ms(&self) -> u64 {
            self.deadline_ms.saturating_sub(crate::monotonic_ms())
        }
    }
}

pub mod boot {
    pub mod console {
        pub fn serial_print(_s: &str) {}
        pub fn serial_print_dec(_v: u64) {}
    }
}

pub fn monotonic_ms() -> u64 {
    0
}

// ---------------------------------------------------------------------------
// Gerçek oyun modülleri (çekirdektekiyle aynı kaynak)
// ---------------------------------------------------------------------------

#[path = "preview_apps.rs"]
pub mod apps;

// ---------------------------------------------------------------------------
// Ana akış
// ---------------------------------------------------------------------------

use crate::apps::fps_game::FpsGameApp;
use crate::drivers::video::Framebuffer;

fn main() {
    let args: alloc::vec::Vec<String> = std::env::args().collect();
    let out_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| alloc::string::String::from("fps_scene.ppm"));
    let pitch_deg: f32 = args
        .get(2)
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0);
    let pos_x: f32 = args.get(3).and_then(|v| v.parse::<f32>().ok()).unwrap_or(3.5);
    let pos_y: f32 = args.get(4).and_then(|v| v.parse::<f32>().ok()).unwrap_or(3.5);
    let dir_deg: f32 = args.get(5).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);

    let (w, h) = (640usize, 400usize);
    let mut fb = Framebuffer::new(w, h);

    let mut app = FpsGameApp::default();
    app.pos_x = pos_x;
    app.pos_y = pos_y;
    let rad = dir_deg.to_radians();
    // Düzlemin *uzunluğu* kodun kendi görüş açısı ayarıdır; korunur.
    // Yalnızca yön, `dir` ile birlikte aynı açı kadar döndürülür.
    let plane_len = (app.plane_x * app.plane_x + app.plane_y * app.plane_y).sqrt();
    app.dir_x = rad.cos();
    app.dir_y = rad.sin();
    app.plane_x = -app.dir_y * plane_len;
    app.plane_y = app.dir_x * plane_len;
    app.pitch = pitch_deg.to_radians();

    app.render_3d_with_budget(&mut fb, 0, 0, w, h, u64::MAX);

    // PPM (P6) yaz.
    let mut out = std::io::BufWriter::new(std::fs::File::create(&out_path).expect("ppm"));
    write!(out, "P6\n{} {}\n255\n", w, h).unwrap();
    for &px in &fb.pixels {
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >> 8) & 0xFF) as u8;
        let b = (px & 0xFF) as u8;
        out.write_all(&[r, g, b]).unwrap();
    }
    out.flush().unwrap();
    eprintln!("{} yazildi ({}x{})", out_path, w, h);
}

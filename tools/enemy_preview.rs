//! 3B düşman modeli önizleme aracı (host / std).
//!
//! Model çizim kodunu (src/apps/fps_enemy_model.rs) doğrudan kullanarak
//! farklı açı ve mesafelerden düşman render'ı üretir ve PNG olarak kaydeder.
//!
//! Derleme:
//!   rustc --edition 2021 -O tools/enemy_preview.rs -o target/enemy_preview.exe
//! Çalıştırma:
//!   ./target/enemy_preview.exe target/enemy_preview.png
//!
//! Bu araç geliştirme içindir; çekirdek (no_std) ile birlikte derlenmez.

#[path = "../src/apps/fps_enemy_model.rs"]
mod fps_enemy_model;

use fps_enemy_model::{
    draw_enemy_model, Camera, EnemyModel, NoBudget, PixelSink, ENEMY_KINDS, MAX_DEPTH,
};
use std::f32::consts::PI;

// ---------------------------------------------------------------------------
// PNG yazıcı (sıkıştırmasız deflate blokları)
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn write_png(path: &str, w: usize, h: usize, pixels: &[u32]) {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0u8); // filtre: none
        for x in 0..w {
            let c = pixels[y * w + x];
            raw.push(((c >> 16) & 0xFF) as u8);
            raw.push(((c >> 8) & 0xFF) as u8);
            raw.push((c & 0xFF) as u8);
        }
    }

    let mut z = vec![0x78u8, 0x01];
    let mut i = 0usize;
    while i < raw.len() {
        let len = (raw.len() - i).min(65535);
        let last = if i + len >= raw.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend_from_slice(&(len as u16).to_le_bytes());
        z.extend_from_slice(&(!(len as u16)).to_le_bytes());
        z.extend_from_slice(&raw[i..i + len]);
        i += len;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &z);
    png_chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).expect("PNG yazılamadı");
}

// ---------------------------------------------------------------------------
// Sahne
// ---------------------------------------------------------------------------

struct BufSink<'a> {
    buf: &'a mut [u32],
    w: usize,
    h: usize,
}

impl PixelSink for BufSink<'_> {
    fn put(&mut self, x: usize, y: usize, color: u32) {
        if x < self.w && y < self.h {
            self.buf[y * self.w + x] = color;
        }
    }
}

fn draw_background(buf: &mut [u32], w: usize, h: usize) {
    let horizon = h / 2;
    for y in 0..h {
        for x in 0..w {
            let color = if y < horizon {
                // Gökyüzü gradyanı.
                let t = y as f32 / horizon.max(1) as f32;
                let r = (30.0 + 35.0 * t) as u32;
                let g = (50.0 + 25.0 * t) as u32;
                let b = (85.0 + 20.0 * t) as u32;
                (r << 16) | (g << 8) | b
            } else {
                // Zemin: perspektif ızgara.
                let t = (y - horizon) as f32 / horizon.max(1) as f32;
                let base = (
                    (150.0 - 70.0 * t) as u32,
                    (95.0 - 20.0 * t) as u32,
                    (55.0 - 10.0 * t) as u32,
                );
                // Basit ızgara çizgileri (dünya kareleri).
                let row_depth = (horizon + 1) as f32 / (y - horizon + 1) as f32 * 0.5;
                let cam_x = 2.0 * (x as f32 + 0.5) / w as f32 - 1.0;
                let wx = cam_x * row_depth;
                let grid = (wx.fract().abs() < 0.06) || (row_depth.fract() < 0.06);
                if grid {
                    (60u32 << 16) | (45u32 << 8) | 35u32
                } else {
                    (base.0 << 16) | (base.1 << 8) | base.2
                }
            };
            buf[y * w + x] = color;
        }
    }
}

/// Bir panel çizer: kamera (0,0) bakış yönü +x, düşman (ex, ey) konumunda.
/// `wall` verilirse (derinlik, yarı genişlik) bir duvar kolonu çizilir ve
/// düşmanın duvar arkasına gizlenmesi test edilir.
fn render_panel(
    w: usize,
    h: usize,
    ex: f32,
    ey: f32,
    facing: f32,
    kind: u8,
    walk: f32,
    wall: Option<(f32, f32)>,
) -> Vec<u32> {
    let mut buf = vec![0u32; w * h];
    draw_background(&mut buf, w, h);

    let cam = Camera {
        pos_x: 0.0,
        pos_y: 0.0,
        dir_x: 1.0,
        dir_y: 0.0,
        plane_x: 0.0,
        plane_y: 1.0,
    };
    let mut zbuf = vec![MAX_DEPTH * 4.0; w];

    // Duvar kolonu: ekran sütunları + derinlik tamponu.
    if let Some((depth, half)) = wall {
        let tan_half = half / depth;
        let x0 = (((1.0 - tan_half) * 0.5) * w as f32) as usize;
        let x1 = (((1.0 + tan_half) * 0.5) * w as f32) as usize;
        let line_h = (h as f32 / depth) as usize;
        let top = (h / 2).saturating_sub(line_h / 2);
        let bottom = (h / 2 + line_h / 2).min(h);
        for x in x0..x1.min(w) {
            zbuf[x] = depth;
            for y in top..bottom {
                buf[y * w + x] = 0x8B5A3A;
            }
        }
    }

    let enemy = EnemyModel {
        x: ex,
        y: ey,
        facing,
        walk_phase: walk,
        kind,
    };
    let mut sink = BufSink {
        buf: &mut buf,
        w,
        h,
    };
    draw_enemy_model(&mut sink, &cam, &zbuf, w, h, &enemy, &NoBudget);
    buf
}

fn main() {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/enemy_preview.png".to_string());

    let pw = 480usize;
    let ph = 400usize;
    // ex, ey, facing (radyan), tür, yürüyüş fazı, duvar (derinlik, yarı genişlik)
    let panels: Vec<(f32, f32, f32, u8, f32, Option<(f32, f32)>)> = vec![
        // Önden, çapraz, yandan, arkadan: gerçek hacim kontrolü.
        (3.0, 0.0, PI, 0, 0.0, None),
        (3.0, 0.0, PI * 0.75, 0, 0.9, None),
        (3.0, 0.0, PI * 0.5, 0, 1.6, None),
        (3.0, 0.0, 0.0, 0, 2.4, None),
        // Yakın görünüm ve farklı tür.
        (1.6, 0.0, PI, 0, 1.2, None),
        (3.0, 0.0, PI, 1, 0.6, None),
        // Duvar arkasında gizlenme (z-buffer) ve duvar önünde görünme.
        (3.0, 0.0, PI, 0, 0.0, Some((2.0, 0.5))),
        (1.0, 0.0, PI, 0, 0.0, Some((2.0, 0.5))),
    ];

    let cols = 4usize;
    let rows = (panels.len() + cols - 1) / cols;
    let total_w = pw * cols;
    let total_h = ph * rows;
    let mut canvas = vec![0u32; total_w * total_h];

    for (idx, &(ex, ey, facing, kind, walk, wall)) in panels.iter().enumerate() {
        let panel = render_panel(pw, ph, ex, ey, facing, kind, walk, wall);
        let px = (idx % cols) * pw;
        let py = (idx / cols) * ph;
        for y in 0..ph {
            for x in 0..pw {
                canvas[(py + y) * total_w + (px + x)] = panel[y * pw + x];
            }
        }
    }

    write_png(&out_path, total_w, total_h, &canvas);
    println!(
        "{} yazıldı ({}x{}), düşman türleri: {:?}",
        out_path,
        total_w,
        total_h,
        ENEMY_KINDS.len()
    );
}

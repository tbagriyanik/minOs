//! FPS doku atlasının host tarafında görsel önizlemesi.
//!
//! `src/apps/fps_textures.rs` modülünü `#[path]` ile içerip tek bir P6 PPM
//! dosyası üretir: üst satırda duvar dokuları, ortada zemin dokuları, altta
//! tavan dokuları (her doku 8x büyütülmüş, aralarında 4px boşluk).
//!
//! Kullanım:
//!   rustc --edition 2021 tools/fps_preview/preview_textures.rs -o target\fps_preview_textures.exe
//!   .\target\fps_preview_textures.exe target\fps_textures.ppm
//!   python tools\ppm2png.py target\fps_textures.ppm target\fps_textures.png --scale 2

extern crate alloc;

#[path = "../../src/apps/fps_textures.rs"]
mod fps_textures;

use std::fs::File;
use std::io::Write;

/// Her dokunun kaç kat büyütüleceği.
const SCALE: usize = 8;
/// Dokular arası boşluk (piksel).
const GAP: usize = 4;
/// Kaç doku sütunu (en uzun satır: duvarlar).
const COLS: usize = 8;

fn main() {
    let atlas = fps_textures::Atlas::new();
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("fps_textures.ppm"));

    let tex = fps_textures::TEX;
    let cell = tex * SCALE;
    let img_w = GAP + COLS * (cell + GAP);
    let img_h = GAP + 3 * (cell + GAP);

    // Arka plan (sheet rengi).
    let mut img = vec![18u8; img_w * img_h * 3];

    // Satır 0: duvarlar, satır 1: zeminler, satır 2: tavanlar.
    for i in 0..fps_textures::WALL_COUNT {
        let id = u8::try_from(i).unwrap_or(0);
        draw_texture(&mut img, img_w, cell, 0, i, |x, y| atlas.wall(id, x, y));
    }
    for i in 0..fps_textures::FLOOR_COUNT {
        let id = u8::try_from(i).unwrap_or(0);
        draw_texture(&mut img, img_w, cell, 1, i, |x, y| atlas.floor(id, x, y));
    }
    for i in 0..fps_textures::CEIL_COUNT {
        let id = u8::try_from(i).unwrap_or(0);
        draw_texture(&mut img, img_w, cell, 2, i, |x, y| atlas.ceil(id, x, y));
    }

    let mut out = File::create(&out_path).expect("ppm dosyasi olusturulamadi");
    write!(out, "P6\n{} {}\n255\n", img_w, img_h).expect("ppm basligi yazilamadi");
    out.write_all(&img).expect("ppm verisi yazilamadi");
    out.flush().expect("ppm yazilamadi");

    eprintln!(
        "{} yazildi ({}x{}) - {} duvar, {} zemin, {} tavan",
        out_path,
        img_w,
        img_h,
        fps_textures::WALL_COUNT,
        fps_textures::FLOOR_COUNT,
        fps_textures::CEIL_COUNT
    );
}

/// Tek bir dokuyu sheet'in (satir, sutun) konumuna buyuterek cizer.
fn draw_texture<F>(img: &mut [u8], img_w: usize, cell: usize, row: usize, col: usize, f: F)
where
    F: Fn(usize, usize) -> u32,
{
    let x0 = GAP + col * (cell + GAP);
    let y0 = GAP + row * (cell + GAP);
    let scale = SCALE;

    for sy in 0..cell {
        let ty = sy / scale;
        for sx in 0..cell {
            let tx = sx / scale;
            let color = f(tx, ty);
            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;
            let off = ((y0 + sy) * img_w + (x0 + sx)) * 3;
            img[off] = r;
            img[off + 1] = g;
            img[off + 2] = b;
        }
    }
}

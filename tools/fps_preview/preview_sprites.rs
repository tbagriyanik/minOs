//! FPS sprite onizleme araci (host / std).
//!
//! `src/apps/fps_sprites.rs` modulunu dogrudan kullanarak tum dusman
//! acilarini, olum karelerini, esyalari ve silahlari tek bir P6 PPM
//! sheet'ine dizer. Saydam pikseller koyu lacivert (0x101820) ile
//! doldurulur ki siluetler gorunsun.
//!
//! Derleme:
//!   rustc --edition 2021 tools/fps_preview/preview_sprites.rs -o target/fps_preview_sprites.exe
//! Calistirma:
//!   ./target/fps_preview_sprites.exe target/fps_sprites.ppm
//!
//! Bu arac gelistirme icindir; cekirdek (no_std) ile birlikte derlenmez.

extern crate alloc;

#[path = "../../src/apps/fps_sprites.rs"]
mod fps_sprites;

use fps_sprites::{
    Sprites, ALPHA, ENEMY_ANGLE_COUNT, ENEMY_DEATH_FRAMES, ENEMY_KIND_COUNT, ITEM_H,
    ITEM_KIND_COUNT, ITEM_W, SPR_H, SPR_W, WEAPON_H, WEAPON_KIND_COUNT, WEAPON_W,
};
use std::fs::File;
use std::io::Write;

/// Saydam piksellerin cizilecegi arka plan rengi.
const BG: (u8, u8, u8) = (0x10, 0x18, 0x20);

/// Hucreler arasindaki bosluk.
const GAP: usize = 4;
/// Sayfa kenar boslugu.
const MARGIN: usize = 8;

fn main() {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/fps_sprites.ppm".to_string());

    let s = Sprites::new();

    // Satirlar: dusmanlar / olum kareleri / esyalar / silahlar.
    let enemy_row_w = ENEMY_KIND_COUNT * ENEMY_ANGLE_COUNT * SPR_W;
    let death_row_w = ENEMY_KIND_COUNT * ENEMY_DEATH_FRAMES * SPR_W;
    let item_row_w = ITEM_KIND_COUNT * ITEM_W;
    let weapon_row_w = WEAPON_KIND_COUNT * WEAPON_W;

    let content_w = enemy_row_w
        .max(death_row_w)
        .max(item_row_w)
        .max(weapon_row_w);
    let content_w = content_w + GAP * (ENEMY_KIND_COUNT * ENEMY_ANGLE_COUNT - 1);

    let sheet_w = content_w + 2 * MARGIN;
    let row_heights = [SPR_H, SPR_H, ITEM_H, WEAPON_H];
    let mut sheet_h = 2 * MARGIN;
    for h in row_heights.iter() {
        sheet_h += h + GAP;
    }

    let mut sheet = vec![0xFFu8; sheet_w * sheet_h * 3];
    for px in sheet.chunks_exact_mut(3) {
        px[0] = BG.0;
        px[1] = BG.1;
        px[2] = BG.2;
    }

    let mut y = MARGIN;

    // 1) Dusmanlar: 3 tur x 3 aci.
    for kind in 0..ENEMY_KIND_COUNT {
        for angle in 0..ENEMY_ANGLE_COUNT {
            let x = MARGIN + (kind * ENEMY_ANGLE_COUNT + angle) * (SPR_W + GAP);
            blit(&mut sheet, sheet_w, x, y, SPR_W, SPR_H, |px, py| {
                s.enemy(kind as u8, angle as u8, px, py)
            });
        }
    }
    y += SPR_H + GAP;

    // 2) Olum kareleri: 3 tur x 3 kare.
    for kind in 0..ENEMY_KIND_COUNT {
        for frame in 0..ENEMY_DEATH_FRAMES {
            let x = MARGIN + (kind * ENEMY_DEATH_FRAMES + frame) * (SPR_W + GAP);
            blit(&mut sheet, sheet_w, x, y, SPR_W, SPR_H, |px, py| {
                s.death(kind as u8, frame as u8, px, py)
            });
        }
    }
    y += SPR_H + GAP;

    // 3) Esyalar.
    for kind in 0..ITEM_KIND_COUNT {
        let x = MARGIN + kind * (ITEM_W + GAP);
        blit(&mut sheet, sheet_w, x, y, ITEM_W, ITEM_H, |px, py| {
            s.item(kind as u8, px, py)
        });
    }
    y += ITEM_H + GAP;

    // 4) Silahlar.
    for kind in 0..WEAPON_KIND_COUNT {
        let x = MARGIN + kind * (WEAPON_W + GAP);
        blit(&mut sheet, sheet_w, x, y, WEAPON_W, WEAPON_H, |px, py| {
            s.weapon(kind as u8, px, py)
        });
    }

    let mut f = File::create(&out_path).expect("PPM dosyasi olusturulamadi");
    write!(f, "P6\n{} {}\n255\n", sheet_w, sheet_h).expect("PPM basligi yazilamadi");
    f.write_all(&sheet).expect("PPM verisi yazilamadi");
    drop(f);

    // Alfa kurali denetimi: saydam piksellerde bayrak set, opaklarda sifir.
    let mut transparent = 0usize;
    let mut opaque = 0usize;
    let mut bad = 0usize;
    for kind in 0..ENEMY_KIND_COUNT {
        for angle in 0..ENEMY_ANGLE_COUNT {
            for py in 0..SPR_H {
                for px in 0..SPR_W {
                    let v = s.enemy(kind as u8, angle as u8, px, py);
                    check(v, &mut transparent, &mut opaque, &mut bad);
                }
            }
        }
    }
    for kind in 0..ITEM_KIND_COUNT {
        for py in 0..ITEM_H {
            for px in 0..ITEM_W {
                let v = s.item(kind as u8, px, py);
                check(v, &mut transparent, &mut opaque, &mut bad);
            }
        }
    }
    for kind in 0..WEAPON_KIND_COUNT {
        for py in 0..WEAPON_H {
            for px in 0..WEAPON_W {
                let v = s.weapon(kind as u8, px, py);
                check(v, &mut transparent, &mut opaque, &mut bad);
            }
        }
    }

    println!(
        "{} yazildi ({}x{}), saydam={} opak={} alfa_kurali_hatasi={}",
        out_path, sheet_w, sheet_h, transparent, opaque, bad
    );

    // Sarma (x/y tasmasi) ve gecersiz girdi denetimi.
    let mut wrap_bad = 0usize;
    for kind in 0..ENEMY_KIND_COUNT {
        for angle in 0..ENEMY_ANGLE_COUNT {
            for (x, y) in [(0usize, 0usize), (5, 7), (31, 47)] {
                let base = s.enemy(kind as u8, angle as u8, x, y);
                if s.enemy(kind as u8, angle as u8, x + SPR_W, y) != base
                    || s.enemy(kind as u8, angle as u8, x, y + SPR_H) != base
                {
                    wrap_bad += 1;
                }
            }
        }
    }
    for kind in 0..ITEM_KIND_COUNT {
        let base = s.item(kind as u8, 3, 3);
        if s.item(kind as u8, 3 + ITEM_W, 3) != base || s.item(kind as u8, 3, 3 + ITEM_H) != base {
            wrap_bad += 1;
        }
    }
    for kind in 0..WEAPON_KIND_COUNT {
        let base = s.weapon(kind as u8, 9, 9);
        if s.weapon(kind as u8, 9 + WEAPON_W, 9) != base
            || s.weapon(kind as u8, 9, 9 + WEAPON_H) != base
        {
            wrap_bad += 1;
        }
    }
    // Gecersiz kind/angle/degerler guvenli sekilde (0,0) donmeli.
    let mut invalid_bad = 0usize;
    if s.enemy(ENEMY_KIND_COUNT as u8, 0, 0, 0) != 0
        || s.enemy(0, ENEMY_ANGLE_COUNT as u8, 0, 0) != 0
        || s.death(ENEMY_KIND_COUNT as u8, 0, 0, 0) != 0
        || s.death(0, ENEMY_DEATH_FRAMES as u8, 0, 0) != 0
        || s.item(ITEM_KIND_COUNT as u8, 0, 0) != 0
        || s.weapon(WEAPON_KIND_COUNT as u8, 0, 0) != 0
    {
        invalid_bad += 1;
    }

    println!(
        "sarma_hatasi={} gecersiz_girdi_hatasi={}",
        wrap_bad, invalid_bad
    );
}

/// Alfa bayragi kuralini denetler.
fn check(v: u32, transparent: &mut usize, opaque: &mut usize, bad: &mut usize) {
    let is_transparent = v & ALPHA != 0;
    if is_transparent {
        *transparent += 1;
        if v & 0x00FF_FFFF != 0 {
            *bad += 1;
        }
    } else {
        *opaque += 1;
    }
}

/// Bir sprite'i sayfaya basar (saydam pikseller arka planla doldurulur).
fn blit<F>(sheet: &mut [u8], sheet_w: usize, ox: usize, oy: usize, w: usize, h: usize, get: F)
where
    F: Fn(usize, usize) -> u32,
{
    for py in 0..h {
        for px in 0..w {
            let v = get(px, py);
            let (r, g, b) = if v & ALPHA != 0 {
                BG
            } else {
                (
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                )
            };
            let idx = ((oy + py) * sheet_w + (ox + px)) * 3;
            sheet[idx] = r;
            sheet[idx + 1] = g;
            sheet[idx + 2] = b;
        }
    }
}

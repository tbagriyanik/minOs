//! Wolfenstein 3D tarzı prosedürel doku atlası.
//!
//! Tüm duvar / zemin / tavan dokuları 32x32 piksel ve `0xRRGGBB` renktedir
//! (alfa biti yoktur; sprite'lar bu modülde yer almaz). Doku üretimi tamamen
//! deterministiktir: rastgelelik yerine küçük bir hash fonksiyonu (`hash2`)
//! kullanılır, böylece her açılışta aynı atlas üretilir.
//!
//! Dünya koordinatları `x`/`y` ile örneklenir; koordinatlar doku kenarı
//! dışına çıkarsa `x % TEX`, `y % TEX` ile sarmalanır.

use alloc::vec::Vec;

/// Doku kenar uzunluğu (piksel). Tüm duvar/zemin/tavan dokuları kare.
pub const TEX: usize = 32;

pub const WALL_COUNT: usize = 8;
pub const FLOOR_COUNT: usize = 5;
pub const CEIL_COUNT: usize = 4;

// Duvar doku kimlikleri
pub const WALL_BRICK: u8 = 0;
pub const WALL_STONE: u8 = 1;
pub const WALL_METAL: u8 = 2;
pub const WALL_PANEL: u8 = 3;
pub const WALL_DOOR: u8 = 4;
pub const WALL_PUSH: u8 = 5;
pub const WALL_SECRET: u8 = 6;
pub const WALL_TILE: u8 = 7;

// Zemin doku kimlikleri
pub const FLOOR_STONE: u8 = 0;
pub const FLOOR_CHECKER: u8 = 1;
pub const FLOOR_WOOD: u8 = 2;
pub const FLOOR_CARPET: u8 = 3;
pub const FLOOR_METAL: u8 = 4;

// Tavan doku kimlikleri
pub const CEIL_CONCRETE: u8 = 0;
pub const CEIL_LIGHT: u8 = 1;
pub const CEIL_DARK: u8 = 2;
pub const CEIL_TILE: u8 = 3;

/// Sıralı doku kimlikleri (atlas üretiminde kullanılır).
const WALL_IDS: [u8; WALL_COUNT] = [
    WALL_BRICK,
    WALL_STONE,
    WALL_METAL,
    WALL_PANEL,
    WALL_DOOR,
    WALL_PUSH,
    WALL_SECRET,
    WALL_TILE,
];
const FLOOR_IDS: [u8; FLOOR_COUNT] = [
    FLOOR_STONE,
    FLOOR_CHECKER,
    FLOOR_WOOD,
    FLOOR_CARPET,
    FLOOR_METAL,
];
const CEIL_IDS: [u8; CEIL_COUNT] = [CEIL_CONCRETE, CEIL_LIGHT, CEIL_DARK, CEIL_TILE];

// ---------------------------------------------------------------------------
// Renk yardımcıları
// ---------------------------------------------------------------------------

/// Deterministik 2B hash (xorshift tabanlı). `core::hash` kullanılmaz çünkü
/// hash'in çekirdek içinde sabit ve test edilebilir olması gerekir.
#[inline]
fn hash2(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x
        .wrapping_mul(0x27D4_EB2D)
        .wrapping_add(y.wrapping_mul(0x1656_67B1))
        .wrapping_add(seed.wrapping_mul(0x9E37_79B9));
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h
}

/// Piksel başına deterministik gürültü: -8..=8 aralığında tam sayı.
#[inline]
fn noise(x: usize, y: usize, seed: u32) -> i32 {
    (hash2(x as u32, y as u32, seed) & 0x0F) as i32 - 8
}

/// Rengi `delta` kadar açar/koyultur (kanal başına, taşma yok).
#[inline]
fn shade(c: u32, delta: i32) -> u32 {
    let r = (((c >> 16) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    let g = (((c >> 8) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    let b = ((c & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    (r << 16) | (g << 8) | b
}

/// İki rengi karıştırır (`t` = 0..255, 0 -> a, 255 -> b).
#[inline]
fn mix(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(255);
    let inv = 255 - t;
    let ch = |shift: u32| -> u32 {
        let ca = (a >> shift) & 0xFF;
        let cb = (b >> shift) & 0xFF;
        (ca * inv + cb * t) / 255
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Jitter'li Voronoi hücresi: (hücre kimliği, kenara uzaklık ölçüsü).
///
/// Kenara uzaklık ölçüsü küçükse piksel hücre sınırına (derz/çatlak) yakındır.
fn voronoi(x: u32, y: u32, cell: u32, seed: u32) -> (u32, u32) {
    let gx = x / cell;
    let gy = y / cell;
    let mut best = u32::MAX;
    let mut second = u32::MAX;
    let mut best_id = 0u32;

    for oy in -1i32..=1 {
        for ox in -1i32..=1 {
            let cx = gx as i32 + ox;
            let cy = gy as i32 + oy;
            let jx = (hash2(cx as u32, cy as u32, seed) & 0xFF) as i32 - 128;
            let jy = (hash2(cx as u32, cy as u32, seed ^ 0x9E37_79B9) & 0xFF) as i32 - 128;
            let half = (cell / 2) as i32;
            let px = cx * cell as i32 + half + (jx * cell as i32) / 384;
            let py = cy * cell as i32 + half + (jy * cell as i32) / 384;
            let dx = x as i32 - px;
            let dy = y as i32 - py;
            let d = (dx * dx + dy * dy) as u32;
            if d < best {
                second = best;
                best = d;
                best_id = hash2(cx as u32, cy as u32, seed ^ 0x1234_5678);
            } else if d < second {
                second = d;
            }
        }
    }

    (best_id, second - best)
}

// ---------------------------------------------------------------------------
// Duvar dokuları
// ---------------------------------------------------------------------------

/// Kırmızı-kahverengi tuğla; 8px sıralar, açık gri derz, tuğla başına
/// ton farkı ve ince gürültü.
fn brick_texel(x: usize, y: usize) -> u32 {
    const MORTAR: u32 = 0x8A8274;
    let row = y / 8;
    let ry = y % 8;
    let off = if row % 2 == 1 { 8 } else { 0 };
    let bx = (x + off) % 32;
    let brick = (bx / 16) as u32;

    // Yatay derz (her sıranın üstü) ve dikey derz (her 16px'te bir).
    if ry == 0 || bx % 16 == 0 {
        return shade(MORTAR, noise(x, y, 0x77));
    }

    let tone = (hash2(brick, row as u32, 0x51) & 0x1F) as i32 - 16;
    // Üst kenar biraz daha aydınlık, alt kenar gölgeli.
    let depth = -((ry as i32 - 4) * 2) / 3;
    let mut col = shade(0x7C4632, tone + depth);
    col = shade(col, noise(x, y, 0x99) / 2);

    // Arada küçük çatlak/çukur.
    if hash2(x as u32, y as u32, 0x33) & 0x3F == 0 {
        col = shade(col, -16);
    }
    col
}

/// Koyu gri, düzensiz taş blokları (Voronoi blok bölünmeleri + derz).
fn stone_texel(x: usize, y: usize) -> u32 {
    let (id, edge) = voronoi(x as u32, y as u32, 10, 0xA1);
    let tone = (hash2(id, 0, 0xB2) & 0x1F) as i32 - 16;
    let mut col = shade(0x6A6D67, tone);
    col = shade(col, noise(x, y, 0xC3));
    if edge < 10 {
        col = mix(col, 0x4B4D49, 215);
    }
    col
}

/// Yeşil-gri metal paneller: dikey panel çizgileri, perçinler, ton farkı.
fn metal_texel(x: usize, y: usize) -> u32 {
    let panel = (x / 8) as u32;
    let px = x % 8;
    let tone = (hash2(panel, 0, 0x21) & 0x1F) as i32 - 14;
    let mut col = shade(0x4F5B53, tone);

    // Panel arası dikey çizgi.
    if px == 0 {
        return shade(0x2C332F, noise(x, y, 0x22));
    }
    // Yatay panel bandı.
    if y % 16 == 0 {
        col = shade(0x39413C, noise(x, y, 0x24));
    }
    // Perçinler (üst parıltı + alt gölge), panel kenarlarına yakın.
    if px == 1 || px == 6 {
        if y % 16 == 4 {
            col = shade(0x748078, noise(x, y, 0x25));
        } else if y % 16 == 5 {
            col = shade(0x333A35, noise(x, y, 0x26));
        }
    }
    shade(col, noise(x, y, 0x23) / 2)
}

/// Açık taş/gri panel duvarı: dikey süpürgelikler, üst/alt kaplama, düz panel.
fn panel_texel(x: usize, y: usize) -> u32 {
    let mut col = 0x8E887A;

    // Üst/alt kaplama.
    if y < 5 {
        col = shade(0x7E7869, noise(x, y, 0x43));
        if y == 4 {
            col = 0x64604F;
        } else if y == 0 {
            col = shade(col, 9);
        }
    } else if y >= 27 {
        col = shade(0x7A7466, noise(x, y, 0x44));
        if y == 27 {
            col = 0x64604F;
        } else if y == 31 {
            col = shade(col, 9);
        }
    }

    // Dikey süpürgelikler (kenar kaplamaları) tüm yükseklik boyunca.
    if x < 4 {
        col = shade(0x6E6A5E, noise(x, y, 0x41));
        if x == 3 {
            col = 0x55524A;
        } else if x == 0 {
            col = shade(col, 10);
        }
    } else if x >= 28 {
        col = shade(0x767264, noise(x, y, 0x42));
        if x == 28 {
            col = 0x55524A;
        } else if x == 31 {
            col = shade(col, 10);
        }
    }

    // Orta panelde çok hafif dikey oluklar.
    if (4..28).contains(&x) && x % 8 == 4 {
        col = shade(col, -6);
    }
    shade(col, noise(x, y, 0x45) / 2)
}

/// Metal kapı: kalın çerçeve, dikey panel sıraları, orta şerit, tutamaç ve
/// üst kapı kasnağı. Duvar dokusundan belirgin şekilde farklı.
fn door_texel(x: usize, y: usize) -> u32 {
    let mut col;
    if !(3..29).contains(&x) || !(3..29).contains(&y) {
        col = shade(0x3C4247, noise(x, y, 0x51));
        // Köşe bağlantı parçaları.
        if (x == 3 || x == 28) && (y == 3 || y == 28) {
            col = 0x6E767C;
        }
    } else {
        let seg = ((x - 3) / 7) as u32;
        let spx = (x - 3) % 7;
        col = shade(0x5A6167, (hash2(seg, 0, 0x52) & 0x0F) as i32 - 8);
        // Dikey panel sıra çizgileri.
        if spx == 0 {
            col = 0x333A3F;
        }
        // Ortadaki dikey şerit.
        if (14..18).contains(&x) {
            col = 0x6E767E;
            if x == 14 || x == 17 {
                col = 0x3A4147;
            }
        }
        // Üst kapı kasnağı.
        if y < 8 {
            col = 0x474E54;
            if y == 7 {
                col = 0x2E343A;
            }
        }
        // Üstte küçük pencere.
        if (9..=14).contains(&y) && (14..18).contains(&x) {
            col = 0x1D2226;
            if x == 14 || x == 17 || y == 9 || y == 14 {
                col = 0x6E767E;
            }
        }
        // Tutamaç (kulp): koyu plaka üzerinde yatay çubuk.
        if (19..=26).contains(&x) && (20..=23).contains(&y) {
            col = 0x2E343A;
        }
        if (20..=25).contains(&x) && (21..=22).contains(&y) {
            col = 0xA9AEB1;
        }
        if (20..=25).contains(&x) && y == 21 {
            col = 0xC6CACD;
        }
    }
    shade(col, noise(x, y, 0x53) / 2)
}

/// İtilebilir gizli duvar: `WALL_PANEL` görünümünde ama ortada ince,
/// neredeyse fark edilmeyen bir dikiş var.
fn push_texel(x: usize, y: usize) -> u32 {
    let mut col = panel_texel(x, y);
    if x == 16 {
        col = shade(col, -13);
    } else if x == 15 || x == 17 {
        col = shade(col, -6);
    }
    col
}

/// Gizli kapı/geçit: koyu, sır dolu; ince dikey dikiş ve üstte küçük delik.
fn secret_texel(x: usize, y: usize) -> u32 {
    let mut col = shade(
        0x2F2B26,
        (hash2(x as u32, y as u32, 0x61) & 0x1F) as i32 - 8,
    );
    if x == 16 {
        col = shade(col, -11);
    }
    let dx = x as i32 - 16;
    let dy = y as i32 - 7;
    let d2 = dx * dx + dy * dy;
    if d2 <= 5 {
        col = 0x0B0908;
    } else if d2 <= 14 {
        col = shade(col, -18);
    }
    col
}

/// Fayans/seramik duvar: açık gri-krem, düzenli derzli.
fn tile_texel(x: usize, y: usize) -> u32 {
    let tx = x % 8;
    let ty = y % 8;
    let tile_id = ((x / 8) + (y / 8) * 4) as u32;
    let mut col = shade(0xB4AE9C, (hash2(tile_id, 7, 0x71) & 0x0F) as i32 - 6);
    if tx == 0 || ty == 0 {
        col = shade(0x8F897A, noise(x, y, 0x72) / 2);
    } else if tx == 1 || ty == 1 {
        col = shade(col, 9);
    }
    shade(col, noise(x, y, 0x73) / 2)
}

// ---------------------------------------------------------------------------
// Zemin dokuları
// ---------------------------------------------------------------------------

/// Kaba taş zemin: düzensiz lekeler ve ince çatlaklar, gri-kahverengi.
fn floor_stone_texel(x: usize, y: usize) -> u32 {
    let (id, edge) = voronoi(x as u32, y as u32, 9, 0x81);
    let tone = (hash2(id, 1, 0x82) & 0x1F) as i32 - 14;
    let mut col = shade(0x6B6154, tone);
    col = shade(col, noise(x, y, 0x83));
    if edge < 7 {
        col = mix(col, 0x4A433A, 205);
    }
    col
}

/// Dama tahtası: 16x16 kareler, hafif gürültü.
fn checker_texel(x: usize, y: usize) -> u32 {
    let sq = ((x / 16) + (y / 16)) % 2;
    let base = if sq == 0 { 0x9A9C98 } else { 0x2E3234 };
    shade(base, noise(x, y, 0x91) / 2)
}

/// Ahşap lambalar: dikey tahta şeritleri, damar çizgileri, kahverengi tonlar.
fn wood_texel(x: usize, y: usize) -> u32 {
    let plank = (x / 8) as u32;
    let px = x % 8;
    let tone = (hash2(plank, 0, 0xA1) & 0x1F) as i32 - 14;
    let mut col = shade(0x6B4B2C, tone);

    // Tahta ekleri (dikey derzler).
    if px == 0 {
        return shade(0x3A2A18, noise(x, y, 0xA4));
    }
    // Damar çizgileri: her tahtada iki sabit dikey şerit.
    let g1 = (hash2(plank, 11, 0xA2) % 8) as usize;
    let g2 = (hash2(plank, 12, 0xA2) % 8) as usize;
    if px == g1 || px == g2 {
        col = shade(col, -15);
    }
    // Enine kısa damar parçaları.
    if hash2(x as u32, (y / 2) as u32, 0xA3) & 0x1F == 0 {
        col = shade(col, -9);
    }
    shade(col, noise(x, y, 0xA5) / 2)
}

/// Koyu kırmızı-kahverengi halı: yoğun noktalama ve hafif desen.
fn carpet_texel(x: usize, y: usize) -> u32 {
    let mut col = shade(0x4E2B22, noise(x, y, 0xB1));
    let h = hash2(x as u32, y as u32, 0xB2);
    if h & 0x1F == 0 {
        col = shade(col, 15);
    } else if h & 0x1F == 1 {
        col = shade(col, -15);
    }
    // Hafif baklava deseni.
    let dx = (x % 8) as i32 - 4;
    let dy = (y % 8) as i32 - 4;
    if (dx + dy).abs() < 1 || (dx - dy).abs() < 1 {
        col = shade(col, 6);
    }
    col
}

/// Metal ızgara (grate): koyu arka plan + ızgara çizgileri.
fn metal_floor_texel(x: usize, y: usize) -> u32 {
    let gx = x % 8;
    let gy = y % 8;
    let mut col = 0x22262A;
    if gx == 0 || gy == 0 {
        col = shade(0x59636B, noise(x, y, 0xC1) / 2);
        if gx == 0 && gy == 0 {
            col = 0x6E7A83;
        }
    } else if gx == 1 || gy == 1 {
        col = shade(col, 10);
    }
    shade(col, noise(x, y, 0xC2))
}

// ---------------------------------------------------------------------------
// Tavan dokuları
// ---------------------------------------------------------------------------

/// Düz gri beton: hafif gürültü ve leke.
fn concrete_texel(x: usize, y: usize) -> u32 {
    let mut col = shade(0x565A5E, noise(x, y, 0xD1));
    if hash2((x / 4) as u32, (y / 4) as u32, 0xD2) & 0x3F == 0 {
        col = shade(col, -9);
    }
    col
}

/// Tavan paneli: açık çerçeve + orta bölümde parlak ışık bandı.
fn ceil_light_texel(x: usize, y: usize) -> u32 {
    if !(4..28).contains(&x) || !(4..28).contains(&y) {
        return shade(0x6A6F73, noise(x, y, 0xE1) / 2);
    }
    let mut col = 0x8C9298;
    if (13..19).contains(&x) && (9..23).contains(&y) {
        col = 0xF2EED6;
    } else if x == 12 || x == 19 {
        col = 0x9AA0A4;
    }
    shade(col, noise(x, y, 0xE2) / 3)
}

/// Koyu tavan: çok düşük parlaklık, hafif gürültü.
fn ceil_dark_texel(x: usize, y: usize) -> u32 {
    shade(0x1C2024, noise(x, y, 0xF1) / 2)
}

/// Açık fayans tavan: derzli ızgara.
fn ceil_tile_texel(x: usize, y: usize) -> u32 {
    let tx = x % 8;
    let ty = y % 8;
    let id = ((x / 8) + (y / 8) * 4) as u32;
    let mut col = shade(0xA8ADA6, (hash2(id, 3, 0xF2) & 0x0F) as i32 - 6);
    if tx == 0 || ty == 0 {
        col = 0x878C86;
    } else if tx == 1 || ty == 1 {
        col = shade(col, 8);
    }
    shade(col, noise(x, y, 0xF3) / 2)
}

// ---------------------------------------------------------------------------
// Doku seçiciler
// ---------------------------------------------------------------------------

#[inline]
fn wall_texel(id: u8, x: usize, y: usize) -> u32 {
    match id {
        WALL_BRICK => brick_texel(x, y),
        WALL_STONE => stone_texel(x, y),
        WALL_METAL => metal_texel(x, y),
        WALL_PANEL => panel_texel(x, y),
        WALL_DOOR => door_texel(x, y),
        WALL_PUSH => push_texel(x, y),
        WALL_SECRET => secret_texel(x, y),
        WALL_TILE => tile_texel(x, y),
        _ => brick_texel(x, y),
    }
}

#[inline]
fn floor_texel(id: u8, x: usize, y: usize) -> u32 {
    match id {
        FLOOR_STONE => floor_stone_texel(x, y),
        FLOOR_CHECKER => checker_texel(x, y),
        FLOOR_WOOD => wood_texel(x, y),
        FLOOR_CARPET => carpet_texel(x, y),
        FLOOR_METAL => metal_floor_texel(x, y),
        _ => floor_stone_texel(x, y),
    }
}

#[inline]
fn ceil_texel(id: u8, x: usize, y: usize) -> u32 {
    match id {
        CEIL_CONCRETE => concrete_texel(x, y),
        CEIL_LIGHT => ceil_light_texel(x, y),
        CEIL_DARK => ceil_dark_texel(x, y),
        CEIL_TILE => ceil_tile_texel(x, y),
        _ => concrete_texel(x, y),
    }
}

// ---------------------------------------------------------------------------
// Atlas
// ---------------------------------------------------------------------------

/// Tüm dokuları tek bir arabellekte tutan atlas.
pub struct Atlas {
    data: Vec<u32>,
}

impl Atlas {
    /// Atlası üretir (prosedürel çizim).
    pub fn new() -> Self {
        let mut data = Vec::with_capacity((WALL_COUNT + FLOOR_COUNT + CEIL_COUNT) * TEX * TEX);

        for &id in WALL_IDS.iter() {
            for y in 0..TEX {
                for x in 0..TEX {
                    data.push(wall_texel(id, x, y));
                }
            }
        }
        for &id in FLOOR_IDS.iter() {
            for y in 0..TEX {
                for x in 0..TEX {
                    data.push(floor_texel(id, x, y));
                }
            }
        }
        for &id in CEIL_IDS.iter() {
            for y in 0..TEX {
                for x in 0..TEX {
                    data.push(ceil_texel(id, x, y));
                }
            }
        }

        Atlas { data }
    }

    /// Duvar dokusu. `id` WALL_* sabitlerinden biri olmalı; geçersiz id
    /// güvenli bir şekilde WALL_BRICK'e düşer.
    #[inline(always)]
    pub fn wall(&self, id: u8, x: usize, y: usize) -> u32 {
        let idx = if usize::from(id) < WALL_COUNT {
            usize::from(id)
        } else {
            usize::from(WALL_BRICK)
        };
        self.texel(idx, x, y)
    }

    /// Zemin dokusu (FLOOR_*). Geçersiz id FLOOR_STONE'a düşer.
    #[inline(always)]
    pub fn floor(&self, id: u8, x: usize, y: usize) -> u32 {
        let idx = if usize::from(id) < FLOOR_COUNT {
            usize::from(id)
        } else {
            usize::from(FLOOR_STONE)
        };
        self.texel(WALL_COUNT + idx, x, y)
    }

    /// Tavan dokusu (CEIL_*). Geçersiz id CEIL_CONCRETE'e düşer.
    #[inline(always)]
    pub fn ceil(&self, id: u8, x: usize, y: usize) -> u32 {
        let idx = if usize::from(id) < CEIL_COUNT {
            usize::from(id)
        } else {
            usize::from(CEIL_CONCRETE)
        };
        self.texel(WALL_COUNT + FLOOR_COUNT + idx, x, y)
    }

    /// Tek piksel okuma (koordinatlar doku kenarında sarmalanır).
    #[inline(always)]
    fn texel(&self, index: usize, x: usize, y: usize) -> u32 {
        self.data[index * TEX * TEX + (y % TEX) * TEX + (x % TEX)]
    }
}

impl Default for Atlas {
    fn default() -> Self {
        Atlas::new()
    }
}

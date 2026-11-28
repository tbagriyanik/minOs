//! Kurt 3D tarzı sprite gorselleri (dusman, esya, silah).
//!
//! Modul, 2B billboard (yassı) sprite'ları prosedurel olarak uretir:
//!
//! * **Dusmanlar**: 32x48 piksel, 3 tur (asker / subay / muhafiz) ve
//!   3 acı (on / yan / arka).
//! * **Olum animasyonu**: 32x48 piksel, her tur icin 3 kare
//!   (dikilmis / dizlerden cokmus / yatmis).
//! * **Esyalar**: 16x16 piksel, 8 cesit.
//! * **Silahlar**: 64x48 piksel, ekranin altinda tutulan silah (3 cesit).
//!
//! Renkler `0xRRGGBB` formatindadir. Saydam (arka plan) piksellerde
//! [`ALPHA`] bayragi ayrica set edilir; opak piksellerde bu bit SIFIRDIR.
//!
//! Sprite verisi `no_std` + `alloc` uyumlu olarak, `Vec<u32>` tamponlarinda
//! tutulur. Sanat, ASCII tabanli sabit tablolardan (blit) ve prosedurel
//! silah cizimiyle uretilir.
//!
//! Bu modul genis bir public API sunar (olculer, cesit sayilari, esya
//! sabitleri); oyun entegrasyonu bunlarin hepsini kullanmayabilir. Bu
//! yuzden `dead_code` bu dosya icin kapalidir (proje genelindeki
//! `src/apps/filemanager.rs` vb. ile ayni uygulama).
#![allow(dead_code)]

use alloc::vec::Vec;

/// Dusman sprite olculeri (piksel).
pub const SPR_W: usize = 32;
pub const SPR_H: usize = 48;

/// Esya sprite olculeri.
pub const ITEM_W: usize = 16;
pub const ITEM_H: usize = 16;

/// Silah sprite olculeri (ekranda elde tutulan silah).
pub const WEAPON_W: usize = 64;
pub const WEAPON_H: usize = 48;

pub const ENEMY_KIND_COUNT: usize = 3;
/// 0 = oyuncuya donuk (on), 1 = yan, 2 = sirt (arka).
pub const ENEMY_ANGLE_COUNT: usize = 3;
pub const ENEMY_DEATH_FRAMES: usize = 3;
pub const ITEM_KIND_COUNT: usize = 8;
/// 0 = tabanca, 1 = pompoli, 2 = mitralyoz.
pub const WEAPON_KIND_COUNT: usize = 3;

/// Alfa bayragi: sprite pikseli saydam ise bu bit set edilir.
pub const ALPHA: u32 = 0x0100_0000;

pub const ITEM_AMMO: u8 = 0;
pub const ITEM_HEALTH: u8 = 1;
pub const ITEM_ARMOR: u8 = 2;
pub const ITEM_KEY_BLUE: u8 = 3;
pub const ITEM_KEY_YELLOW: u8 = 4;
pub const ITEM_KEY_RED: u8 = 5;
pub const ITEM_GOLD: u8 = 6;
pub const ITEM_SILVER: u8 = 7;

/// Tüm sprite verilerini iceren set.
pub struct Sprites {
    enemies: Vec<u32>, // ENEMY_KIND_COUNT * ENEMY_ANGLE_COUNT * SPR_W * SPR_H
    deaths: Vec<u32>,  // ENEMY_KIND_COUNT * ENEMY_DEATH_FRAMES * SPR_W * SPR_H
    items: Vec<u32>,   // ITEM_KIND_COUNT * ITEM_W * ITEM_H
    weapons: Vec<u32>, // WEAPON_KIND_COUNT * WEAPON_W * WEAPON_H
}

impl Sprites {
    /// Sprite'lari prosedurel olarak uretir.
    pub fn new() -> Self {
        let mut enemies = Vec::new();
        for kind in 0..ENEMY_KIND_COUNT {
            for angle in 0..ENEMY_ANGLE_COUNT {
                enemies.extend_from_slice(&build_enemy(kind as u8, angle as u8));
            }
        }

        let mut deaths = Vec::new();
        for kind in 0..ENEMY_KIND_COUNT {
            for frame in 0..ENEMY_DEATH_FRAMES {
                deaths.extend_from_slice(&build_death(kind as u8, frame as u8));
            }
        }

        let mut items = Vec::new();
        for kind in 0..ITEM_KIND_COUNT {
            items.extend_from_slice(&build_item(kind as u8));
        }

        let mut weapons = Vec::new();
        for kind in 0..WEAPON_KIND_COUNT {
            weapons.extend_from_slice(&build_weapon(kind as u8));
        }

        Sprites {
            enemies,
            deaths,
            items,
            weapons,
        }
    }

    /// Dusman sprite tek pikel. Gecersiz kind/angle degerlerinde guvenli sekilde
    /// (0,0) doner.
    #[inline(always)]
    pub fn enemy(&self, kind: u8, angle: u8, x: usize, y: usize) -> u32 {
        let k = kind as usize;
        let a = angle as usize;
        if k >= ENEMY_KIND_COUNT || a >= ENEMY_ANGLE_COUNT {
            return 0;
        }
        let idx = ((k * ENEMY_ANGLE_COUNT + a) * SPR_H + y % SPR_H) * SPR_W + x % SPR_W;
        self.enemies[idx]
    }

    /// Olum animasyonu karesi.
    #[inline(always)]
    pub fn death(&self, kind: u8, frame: u8, x: usize, y: usize) -> u32 {
        let k = kind as usize;
        let f = frame as usize;
        if k >= ENEMY_KIND_COUNT || f >= ENEMY_DEATH_FRAMES {
            return 0;
        }
        let idx = ((k * ENEMY_DEATH_FRAMES + f) * SPR_H + y % SPR_H) * SPR_W + x % SPR_W;
        self.deaths[idx]
    }

    /// Esya sprite tek pikel.
    #[inline(always)]
    pub fn item(&self, kind: u8, x: usize, y: usize) -> u32 {
        let k = kind as usize;
        if k >= ITEM_KIND_COUNT {
            return 0;
        }
        let idx = (k * ITEM_H + y % ITEM_H) * ITEM_W + x % ITEM_W;
        self.items[idx]
    }

    /// Silah sprite tek pikel.
    #[inline(always)]
    pub fn weapon(&self, kind: u8, x: usize, y: usize) -> u32 {
        let k = kind as usize;
        if k >= WEAPON_KIND_COUNT {
            return 0;
        }
        let idx = (k * WEAPON_H + y % WEAPON_H) * WEAPON_W + x % WEAPON_W;
        self.weapons[idx]
    }
}

/// Varsayilan yapici. Yalnizca `clippy::new_without_default` kuralini
/// saglamak icin vardir; oyun `Sprites::new()` ile kurar.
impl Default for Sprites {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Sabit sanat tablolari (ASCII) - derleme aninda boyutlari dogrulanir.
// ---------------------------------------------------------------------------

/// ASCII sanatini sabit boyutlu piksel tablosuna cevirir.
///
/// Derleme aninda (const eval) satir sayisi ve satir genisligi denetlenir;
/// hatali bir tablo derleme hatasi olarak ortaya cikar.
const fn parse_art<const ROWS: usize, const COLS: usize>(art: &str) -> [[u8; COLS]; ROWS] {
    let bytes = art.as_bytes();
    let mut out = [[b'.'; COLS]; ROWS];
    let mut row = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        // Satir sonundaki '\r' (CRLF dosya) yok sayilir.
        let mut end = i;
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        let len = end - start;
        if i < bytes.len() {
            i += 1;
        }
        // Bos satirlar (tablodaki bastaki/sonraki bosluk) atlanir.
        if len == 0 {
            continue;
        }
        assert!(
            row < ROWS,
            "fps_sprites: sanat tablosu satir sayisi cok fazla"
        );
        assert!(
            len == COLS,
            "fps_sprites: sanat tablosu satir genisligi hatali"
        );
        let mut col = 0usize;
        while col < COLS {
            out[row][col] = bytes[start + col];
            col += 1;
        }
        row += 1;
    }
    assert!(row == ROWS, "fps_sprites: sanat tablosu satir sayisi eksik");
    out
}

/// Dusman (on) art'i. 32x48.
///
/// Harfler: `.` saydam, `H` baslik, `h` baslik golgesi, `B` bant,
/// `S` ten, `s` ten golgesi, `E` goz, `U` uniform, `u` uniform golgesi,
/// `L` uniform isigi, `A` kol bandi, `P` pantolon, `p` pantolon golgesi,
/// `O` bot, `o` bot golgesi.
const FRONT_ART: &str = r#"
................................
................................
............HHHHHHHH............
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........BBBBBBBBBB...........
............sSSSSSSs............
............sESSSSEs............
............sSSSSSSs............
............sSSSsSSs............
..............ssssss............
...............ss...............
...........uUUUUUUUUL...........
...........uUUUUUUUUL...........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........uuuuUUUUUUUULLLL........
........AAuuUUUUUUUULAAL........
........AAuuUUUUUUUULAAL........
........uuuuUUUUUUUULLLL........
........SSsuUUUUUUUULSSs........
........SSsuUUUUUUUULSSs........
........SSsuUUUUUUUULSSs........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppppp.ppppp...........
..........ppppp.ppppp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
"#;

/// Dusman (yan / profil) art'i. 32x48.
const SIDE_ART: &str = r#"
................................
................................
.............HHHHHH.............
............HHHHHHHHHH..........
............HHHHHHHHHH..........
............HHHHHHHHHH..........
............HHHHHHHHHH..........
............BBBBBBBBBBB.........
............HSSSSSSSS...........
............HSSSSSESS...........
............HSSSSSSSSS..........
............HSSSSSSS............
.............ssssss.............
..............ssss..............
............uUUUUUUL............
............uUUUUUUL............
............uUUUUUUL............
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULUUL.........
............uUUUUUULSSs.........
............uUUUUUULSSs.........
............uUUUUUUL............
............uUUUUUUL............
............uUUUUUUL............
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.ppppp..........
...........ppppp.ppppp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
...........ppppp.PPPpp..........
..........oOOOOo.oOOOOo.........
..........oOOOOo.oOOOOo.........
..........oOOOOo.oOOOOo.........
..........oOOOOo.oOOOOo.........
..........oOOOOo.oOOOOo.........
"#;

/// Dusman (arka / sirt) art'i. 32x48.
const BACK_ART: &str = r#"
................................
................................
............HHHHHHHH............
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
...........HHHHHHHHHH...........
............hhhhhhhh............
...........uUUUUUUUUL...........
...........uUUUuuUUUL...........
...........uUUUuuUUUL...........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........uuuuUUUuuUUULLLL........
........AAuuUUUuuUUULAAL........
........AAuuUUUuuUUULAAL........
........uuuuUUUuuUUULLLL........
........SSsuUUUuuUUULSSs........
........SSsuUUUuuUUULSSs........
........SSsuUUUuuUUULSSs........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppppp.ppppp...........
..........ppppp.ppppp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
..........ppPPP.PPPpp...........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
.........oOOOOo.oOOOOo..........
"#;

/// Olum karesi 1: dizlerden cokmus (kuculmus, alcak pozisyon). 32x48.
const KNEEL_ART: &str = r#"
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
............HHHHHHHH............
...........HHHHHHHHHH...........
...........BBBBBBBBBB...........
............sSSSSSSs............
............sESSSSEs............
............sSSSSSSs............
.............ssssssss...........
.............ssssssss...........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
.........uUUUUUUUUUULL..........
..........PPPPPPPPPPPP..........
..........PPPPPPPPPPPP..........
..........PPPPPPPPPPPP..........
..........PPPP..PPPPPP..........
..........PPPP..PPPPPP..........
..........PPPP..PPPPPP..........
..........PPPP..PPPPPP..........
..........oooo..oooooo..........
..........oooo..oooooo..........
..........oooo..oooooo..........
................................
................................
................................
................................
"#;

/// Olum karesi 2: yatmis. 32x48.
const LIE_ART: &str = r#"
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
................................
...HHHHHHHH.....................
...HHHHHHHH.....................
...BBSSSSSS.....................
...SSSSSSSS.....................
....sssssss.....................
........UUU.....................
.........UUUU...................
.........UUUUUU.................
..........UUUUUU....PPPPPPPP....
..........UUUUUUU...PPPPPPPPP...
..........UUUUUUU...PPPPPPPPP...
..........ooooooo...ooooooooo...
..........ooooooo...ooooooooo...
..........ooooooo...ooooooooo...
................................
................................
"#;

/// Esya art'lari. 16x16.
///
/// Harfler: `.` saydam, `W` beyaz, `w` gri, `R` kirmizi, `G` yesil,
/// `v` yelek acik, `g` yelek koyu, `K` mavi, `k` koyu mavi, `Y` sari,
/// `y` koyu sari, `N` altin, `n` koyu altin, `S` gumus, `s` koyu gumus,
/// `M` metal, `m` koyu metal, `B` kahve, `H` zeytin, `h` koyu zeytin.
const AMMO_ART: &str = r#"
................
..YY..YY..YY....
..YY..YY..YY....
.HHHHHHHHHHHHHH.
.hHHHHHHHHHHHHh.
.hhMMMMMMMMMMhh.
.hhMMMMMMMMMMhh.
.hHHHHHHHHHHHHh.
.hHHHHHHHHHHHHh.
.hHHh.hhHHHHhHh.
.hHhh.hhHHHHhHh.
.hHHHHHHHHHHHHh.
.hHHHHHHHHHHHHh.
.hhHHHHHHHHHHhh.
..hhhhhhhhhhhh..
................
"#;

const HEALTH_ART: &str = r#"
................
................
.RRRRRRRRRRRRRR.
.RWWWWWWWWWWWWR.
.RWWWRRRRWWWWWR.
.RWWWRRRRWWWWWR.
.RWWWRRRRWWWWWR.
.WRRRRRRRRRRRWR.
.WRRRRRRRRRRRWR.
.RWWWRRRRWWWWWR.
.RWWWRRRRWWWWWR.
.RWWWRRRRWWWWWR.
.RWWWWWWWWWWWWR.
.RWWWWWWWWWWWWR.
.RRRRRRRRRRRRRR.
................
"#;

const ARMOR_ART: &str = r#"
................
................
....GG....GG....
....GG....GG....
....GG....GG....
...GGGGGGGGGG...
...GvvVVVVvvG...
...GvvVVVVvvG...
...GvvvvvvvvG...
...GvvVVVVvvG...
...GvvVVVVvvG...
...GGGGGGGGGG...
....gggggggg....
................
................
................
"#;

const KEY_BLUE_ART: &str = r#"
................
................
...KKKK.........
..K....K........
..K....K........
..K....K........
...KKKK.........
.....KK.........
.....KK.........
.....KK.........
.....KK.K.......
.....KK.KK......
.....KK.KK......
.....KK.........
.....KK.........
................
"#;

const KEY_YELLOW_ART: &str = r#"
................
................
...YYYY.........
..Y....Y........
..Y....Y........
..Y....Y........
...YYYY.........
.....YY.........
.....YY.........
.....YY.........
.....YY.Y.......
.....YY.YY......
.....YY.YY......
.....YY.........
.....YY.........
................
"#;

const KEY_RED_ART: &str = r#"
................
................
...RRRR.........
..R....R........
..R....R........
..R....R........
...RRRR.........
.....RR.........
.....RR.........
.....RR.........
.....RR.R.......
.....RR.RR......
.....RR.RR......
.....RR.........
.....RR.........
................
"#;

const GOLD_ART: &str = r#"
................
................
.....NNNN.......
...NNNNNNNN.....
...nNNNNNNn.....
.....NNNN.......
...NNNNNNNN.....
...nNNNNNNn.....
.....NNNN.......
...NNNNNNNN.....
...nNNNNNNn.....
.....NNNN.......
................
................
................
................
"#;

const SILVER_ART: &str = r#"
................
................
.....SSSS.......
...SSSSSSSS.....
...sSSSSSSs.....
.....SSSS.......
...SSSSSSSS.....
...sSSSSSSs.....
.....SSSS.......
...SSSSSSSS.....
...sSSSSSSs.....
.....SSSS.......
................
................
................
................
"#;

const FRONT: [[u8; SPR_W]; SPR_H] = parse_art(FRONT_ART);
const SIDE: [[u8; SPR_W]; SPR_H] = parse_art(SIDE_ART);
const BACK: [[u8; SPR_W]; SPR_H] = parse_art(BACK_ART);
const KNEEL: [[u8; SPR_W]; SPR_H] = parse_art(KNEEL_ART);
const LIE: [[u8; SPR_W]; SPR_H] = parse_art(LIE_ART);

const AMMO: [[u8; ITEM_W]; ITEM_H] = parse_art(AMMO_ART);
const HEALTH: [[u8; ITEM_W]; ITEM_H] = parse_art(HEALTH_ART);
const ARMOR: [[u8; ITEM_W]; ITEM_H] = parse_art(ARMOR_ART);
const KEY_BLUE: [[u8; ITEM_W]; ITEM_H] = parse_art(KEY_BLUE_ART);
const KEY_YELLOW: [[u8; ITEM_W]; ITEM_H] = parse_art(KEY_YELLOW_ART);
const KEY_RED: [[u8; ITEM_W]; ITEM_H] = parse_art(KEY_RED_ART);
const GOLD: [[u8; ITEM_W]; ITEM_H] = parse_art(GOLD_ART);
const SILVER: [[u8; ITEM_W]; ITEM_H] = parse_art(SILVER_ART);

// ---------------------------------------------------------------------------
// Paletler
// ---------------------------------------------------------------------------

/// Bir dusman turunun renk paleti.
struct Palette {
    head: u32,
    head_dark: u32,
    band: u32,
    skin: u32,
    skin_dark: u32,
    uniform: u32,
    uniform_dark: u32,
    uniform_light: u32,
    accent: u32,
    pants: u32,
    pants_dark: u32,
    boot: u32,
    boot_dark: u32,
    metal: u32,
    metal_dark: u32,
    wood: u32,
    wood_dark: u32,
}

/// Tur 0 "Asker": zeytin yesil uniform, koyu miğfer, kahverengi botlar.
const SOLDIER: Palette = Palette {
    head: 0x3C4436,
    head_dark: 0x242A20,
    band: 0x3C4436,
    skin: 0xC89A6A,
    skin_dark: 0x9A7346,
    uniform: 0x5E6D3F,
    uniform_dark: 0x3E4A28,
    uniform_light: 0x7A8850,
    accent: 0x5E6D3F,
    pants: 0x44512C,
    pants_dark: 0x2C3418,
    boot: 0x4C3624,
    boot_dark: 0x2F2117,
    metal: 0x767680,
    metal_dark: 0x2A2A31,
    wood: 0x7A5330,
    wood_dark: 0x4E3018,
};

/// Tur 1 "Subay": gri-mavi uniform, kep, altin apka bandi, kirmizi kol pahasi.
const OFFICER: Palette = Palette {
    head: 0x2E3947,
    head_dark: 0x1B2430,
    band: 0xD4A72C,
    skin: 0xD8AA7C,
    skin_dark: 0xA87E52,
    uniform: 0x737F91,
    uniform_dark: 0x4E5866,
    uniform_light: 0x93A0B2,
    accent: 0xA62B2B,
    pants: 0x535D6C,
    pants_dark: 0x3A4250,
    boot: 0x3C3024,
    boot_dark: 0x262017,
    metal: 0x767680,
    metal_dark: 0x2A2A31,
    wood: 0x7A5330,
    wood_dark: 0x4E3018,
};

/// Tur 2 "Muhafiz": kahve/tan uniform, siyah miğfer (bantli), koyu pantolon.
const GUARD: Palette = Palette {
    head: 0x26262C,
    head_dark: 0x16161A,
    band: 0x0C0C10,
    skin: 0xC08F5E,
    skin_dark: 0x8E6740,
    uniform: 0x8C7043,
    uniform_dark: 0x5E4A2A,
    uniform_light: 0xAA8A52,
    accent: 0x8C7043,
    pants: 0x2E2A24,
    pants_dark: 0x1E1A16,
    boot: 0x2E2418,
    boot_dark: 0x1B140D,
    metal: 0x767680,
    metal_dark: 0x2A2A31,
    wood: 0x7A5330,
    wood_dark: 0x4E3018,
};

fn palette(kind: u8) -> &'static Palette {
    match kind {
        1 => &OFFICER,
        2 => &GUARD,
        _ => &SOLDIER,
    }
}

// ---------------------------------------------------------------------------
// Cizim tuvali
// ---------------------------------------------------------------------------

/// Tam sayi karekok (`core`de `f32::sqrt` yoktur).
fn isqrt(n: u32) -> u32 {
    let mut rest = n;
    let mut root = 0u32;
    while rest > 2 * root {
        rest -= 2 * root + 1;
        root += 1;
    }
    root
}

/// Kucuk bir RGBA piksel tuvali (alfa bayragi ile saydamlik).
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u32>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        let mut px = Vec::new();
        px.resize(w * h, ALPHA);
        Canvas { w, h, px }
    }

    #[inline]
    fn set(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = color;
        }
    }

    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        let (xa, xb) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        let (ya, yb) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for y in ya..=yb {
            for x in xa..=xb {
                self.set(x, y, color);
            }
        }
    }

    /// Dolu daire (elips degil, daire).
    fn disc(&mut self, cx: i32, cy: i32, r: i32, color: u32) {
        if r <= 0 {
            return;
        }
        for y in (cy - r)..=(cy + r) {
            let dy = y - cy;
            let inside = r * r - dy * dy;
            if inside < 0 {
                continue;
            }
            let half = isqrt(inside as u32) as i32;
            for x in (cx - half)..=(cx + half) {
                self.set(x, y, color);
            }
        }
    }

    /// Kalin cizgi (kare kesitli).
    fn thick_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, thick: i32, color: u32) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = dx.abs().max(dy.abs()).max(1);
        let half = thick / 2;
        for s in 0..=steps {
            let x = x0 + dx * s / steps;
            let y = y0 + dy * s / steps;
            for oy in 0..thick {
                for ox in 0..thick {
                    self.set(x + ox - half, y + oy - half, color);
                }
            }
        }
    }

    /// Satirlari yatay olarak kaydirir (bosalan pikseller saydam olur).
    fn shift_rows(&mut self, y0: usize, y1: usize, dx: i32) {
        for y in y0..=y1.min(self.h.saturating_sub(1)) {
            let mut row: Vec<u32> = Vec::new();
            row.resize(self.w, ALPHA);
            for (x, slot) in row.iter_mut().enumerate() {
                let src = x as i32 - dx;
                *slot = if src >= 0 && (src as usize) < self.w {
                    self.px[y * self.w + src as usize]
                } else {
                    ALPHA
                };
            }
            self.px[y * self.w..(y + 1) * self.w].copy_from_slice(&row);
        }
    }

    /// Siluetin disina 1 piksellik koyu kontur cizer. Koyu arka plan
    /// uzerinde sekli okunabilir kilar (Wolf3D sprite stili).
    fn outline(&mut self, color: u32) {
        let w = self.w;
        let h = self.h;
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                // Yalnizca saydam pikseller aday olur; komsusu opak olan
                // saydam pikseller kontur rengiyle doldurulur.
                if self.px[i] & ALPHA == 0 {
                    continue;
                }
                let mut near = false;
                if x > 0 && self.px[i - 1] & ALPHA == 0 {
                    near = true;
                }
                if x + 1 < w && self.px[i + 1] & ALPHA == 0 {
                    near = true;
                }
                if y > 0 && self.px[i - w] & ALPHA == 0 {
                    near = true;
                }
                if y + 1 < h && self.px[i + w] & ALPHA == 0 {
                    near = true;
                }
                if near {
                    edges.push((x, y));
                }
            }
        }
        for (x, y) in edges {
            self.px[y * w + x] = color;
        }
    }

    fn into_vec(self) -> Vec<u32> {
        self.px
    }
}

// ---------------------------------------------------------------------------
// Sanat -> piksel cevirisi
// ---------------------------------------------------------------------------

/// Dusman art'inda bir karakterin rengini cozer.
fn enemy_color(ch: u8, p: &Palette) -> Option<u32> {
    Some(match ch {
        b'H' => p.head,
        b'h' => p.head_dark,
        b'B' => p.band,
        b'S' => p.skin,
        b's' => p.skin_dark,
        b'E' => 0x12121A,
        b'U' => p.uniform,
        b'u' => p.uniform_dark,
        b'L' => p.uniform_light,
        b'A' => p.accent,
        b'P' => p.pants,
        b'p' => p.pants_dark,
        b'O' => p.boot,
        b'o' => p.boot_dark,
        b'G' => p.metal,
        b'g' => p.metal_dark,
        b'W' => p.wood,
        b'w' => p.wood_dark,
        _ => return None,
    })
}

/// Esya art'inda bir karakterin rengini cozer.
fn item_color(ch: u8) -> Option<u32> {
    Some(match ch {
        b'W' => 0xE6E8EA,
        b'w' => 0xA8ADB2,
        b'R' => 0xC62828,
        b'r' => 0x7E1616,
        b'G' => 0x3F8A46,
        b'g' => 0x23512A,
        b'V' => 0x9AA88F,
        b'v' => 0x6E7A64,
        b'K' => 0x3C6ED2,
        b'k' => 0x1D3C86,
        b'Y' => 0xE4C530,
        b'y' => 0x8F7418,
        b'N' => 0xEFC13C,
        b'n' => 0xA17A1C,
        b'S' => 0xC4CCD4,
        b's' => 0x7E868E,
        b'M' => 0x8A9098,
        b'm' => 0x4C525A,
        b'B' => 0x6E4A26,
        b'b' => 0x40290F,
        b'H' => 0x6E7A46,
        b'h' => 0x465030,
        _ => return None,
    })
}

/// ASCII art tablosunu tuvale basar.
fn blit(c: &mut Canvas, art: &[[u8; SPR_W]; SPR_H], pal: &Palette) {
    for (row, line) in art.iter().enumerate() {
        for (col, &ch) in line.iter().enumerate() {
            if let Some(color) = enemy_color(ch, pal) {
                c.set(col as i32, row as i32, color);
            }
        }
    }
}

/// ASCII art tablosunu tuvale basar (16x16 esya).
fn blit_item(c: &mut Canvas, art: &[[u8; ITEM_W]; ITEM_H]) {
    for (row, line) in art.iter().enumerate() {
        for (col, &ch) in line.iter().enumerate() {
            if let Some(color) = item_color(ch) {
                c.set(col as i32, row as i32, color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Silah cizimi (prosedurel)
// ---------------------------------------------------------------------------

/// Elde tutulan deri el (3x2 blok).
fn draw_hand(c: &mut Canvas, x: i32, y: i32, p: &Palette) {
    c.set(x, y, p.skin);
    c.set(x + 1, y, p.skin);
    c.set(x + 2, y, p.skin_dark);
    c.set(x, y + 1, p.skin_dark);
    c.set(x + 1, y + 1, p.skin_dark);
    c.set(x + 2, y + 1, p.skin_dark);
}

/// On acida tufek: govde uzerinde capraz durur, dip alt solta, namlu ust saga.
fn draw_rifle_front(c: &mut Canvas, p: &Palette) {
    // Dip (ahsap): kalin blok, alt solta.
    c.thick_line(7, 33, 12, 27, 6, p.wood_dark);
    c.thick_line(7, 32, 12, 26, 4, p.wood);
    // Alici (metal).
    c.thick_line(12, 27, 16, 22, 6, p.metal);
    c.thick_line(13, 28, 17, 23, 2, p.metal_dark);
    // Namlu (metal): ust kenar isikli, alt kenar golgeli.
    c.thick_line(16, 22, 25, 12, 4, p.metal);
    c.thick_line(15, 21, 24, 11, 1, 0x9A9AA6);
    c.thick_line(17, 23, 26, 13, 1, p.metal_dark);
    // Arpacik.
    c.set(25, 11, p.metal);
    c.set(26, 11, p.metal_dark);
    // Eller: biri tetikte, biri namlu ustunde.
    draw_hand(c, 10, 29, p);
    draw_hand(c, 17, 21, p);
}

/// Yan acida tufek: yatay, saga dogru namlu.
fn draw_rifle_side(c: &mut Canvas, p: &Palette) {
    // Dip (ahsap): kuyrukta genis, namluya dogru daralan.
    c.fill_rect(3, 19, 5, 27, p.wood_dark);
    c.fill_rect(6, 19, 8, 26, p.wood_dark);
    c.fill_rect(9, 19, 11, 25, p.wood_dark);
    c.fill_rect(3, 19, 5, 20, p.wood);
    c.fill_rect(6, 19, 8, 20, p.wood);
    c.fill_rect(9, 19, 11, 20, p.wood);
    // Alici (metal).
    c.fill_rect(11, 18, 17, 25, p.metal);
    c.fill_rect(11, 18, 16, 19, 0x9A9AA6);
    c.fill_rect(16, 19, 17, 25, p.metal_dark);
    // Namlu.
    c.fill_rect(17, 19, 30, 23, p.metal);
    c.fill_rect(17, 19, 29, 20, 0x9A9AA6);
    c.fill_rect(17, 22, 30, 23, p.metal_dark);
    // Namlu ucu.
    c.fill_rect(30, 19, 31, 23, p.metal_dark);
    // Eller: tetik ve namlu tutucusu.
    draw_hand(c, 14, 21, p);
    draw_hand(c, 23, 21, p);
}

/// Arka acida tufek: omuzdan capraz gecer.
fn draw_rifle_back(c: &mut Canvas, p: &Palette) {
    // Dip (ahsap) alt sagda.
    c.thick_line(26, 33, 20, 27, 6, p.wood_dark);
    c.thick_line(26, 32, 20, 26, 4, p.wood);
    // Alici.
    c.thick_line(20, 27, 16, 22, 6, p.metal);
    c.thick_line(21, 28, 17, 23, 2, p.metal_dark);
    // Namlu.
    c.thick_line(16, 22, 6, 12, 4, p.metal);
    c.thick_line(15, 21, 5, 11, 1, 0x9A9AA6);
    c.thick_line(17, 23, 7, 13, 1, p.metal_dark);
    // Arpacik.
    c.set(5, 11, p.metal);
    c.set(4, 11, p.metal_dark);
    // Eller.
    draw_hand(c, 21, 28, p);
    draw_hand(c, 10, 21, p);
}

/// Elde tutulan silahlarin ortak renkleri.
mod weapon_color {
    pub const BODY: u32 = 0x3E3E48;
    pub const LIGHT: u32 = 0x7C7C88;
    pub const DARK: u32 = 0x1A1A20;
    pub const GRIP: u32 = 0x4A4238;
    pub const WOOD: u32 = 0x6E4A28;
    pub const WOOD_LIGHT: u32 = 0x8C6034;
    pub const SKIN: u32 = 0xC6925F;
    pub const SKIN_DARK: u32 = 0x9C6B41;
    pub const SLEEVE: u32 = 0x333D57;
    pub const SLEEVE_DARK: u32 = 0x28304A;
    pub const CUFF: u32 = 0x4E5A78;
}

/// Tabanca: koyu metal tabanca, namlu yukarida, elle gri tonlari.
fn draw_pistol(c: &mut Canvas) {
    use weapon_color as wc;

    // Kaydirici (namlu govdesi): dikey, hafif sola egik.
    c.fill_rect(26, 3, 38, 25, wc::BODY);
    c.fill_rect(26, 3, 28, 25, wc::LIGHT);
    c.fill_rect(37, 3, 38, 25, wc::DARK);
    c.fill_rect(29, 4, 36, 8, 0x4E4E58);
    // Arpacik ve namlu agzi.
    c.fill_rect(31, 0, 33, 4, wc::DARK);
    c.fill_rect(29, 3, 36, 5, 0x1A1A20);
    // Kaydirici tirnaklari (arka kisim).
    for y in 12..21 {
        if y % 2 == 0 {
            c.fill_rect(30, y, 35, y, 0x2A2A32);
        }
    }
    // Govde (frame).
    c.fill_rect(28, 25, 39, 30, 0x4A4A54);
    c.fill_rect(28, 25, 39, 26, 0x74747F);
    c.fill_rect(38, 26, 39, 30, wc::DARK);
    // Tetik korkulugu (sol tarafta "C" sekli) ve tetik.
    c.fill_rect(22, 27, 29, 29, wc::DARK);
    c.fill_rect(22, 27, 24, 35, wc::DARK);
    c.fill_rect(22, 33, 28, 35, wc::DARK);
    c.fill_rect(25, 29, 26, 32, wc::DARK);
    // Kabza (asagi dogru hafif saga egik).
    for y in 29..WEAPON_H as i32 {
        let x0 = 30 + (y - 29) / 4;
        c.fill_rect(x0, y, x0 + 11, y, 0x241F1A);
        c.fill_rect(x0 + 1, y, x0 + 10, y, wc::GRIP);
    }
    // Parmaklar kabzanin onunu sarar.
    for y in 33..WEAPON_H as i32 {
        let x0 = 30 + (y - 29) / 4;
        let color = if y % 3 == 0 { wc::SKIN_DARK } else { wc::SKIN };
        c.fill_rect(x0 - 8, y, x0 - 3, y, color);
    }
    // Bilek ve kol ceketi (alt sag kose).
    c.fill_rect(44, 40, 47, 47, wc::CUFF);
    c.fill_rect(46, 41, 63, 47, wc::SLEEVE);
    c.fill_rect(60, 41, 63, 47, wc::SLEEVE_DARK);
}

/// Pompoli: iki namlu, uzun, ahsap mahmaz ve tutamak tonlari.
fn draw_shotgun(c: &mut Canvas) {
    use weapon_color as wc;

    // Iki namlu yan yana.
    c.fill_rect(22, 2, 29, 28, wc::BODY);
    c.fill_rect(22, 2, 23, 28, wc::LIGHT);
    c.fill_rect(28, 2, 29, 28, wc::DARK);
    c.fill_rect(31, 2, 38, 28, wc::BODY);
    c.fill_rect(31, 2, 32, 28, wc::LIGHT);
    c.fill_rect(37, 2, 38, 28, wc::DARK);
    // Namlu ucu halkalari.
    c.fill_rect(21, 0, 30, 3, 0x4A4A54);
    c.fill_rect(30, 0, 39, 3, 0x4A4A54);
    // Namlu bandi.
    c.fill_rect(21, 24, 39, 28, 0x4A4A54);
    c.fill_rect(21, 24, 39, 25, 0x74747F);
    // Ahsap mahmaz (pump): oluklu.
    c.fill_rect(20, 29, 40, 38, wc::WOOD);
    c.fill_rect(20, 29, 21, 38, wc::WOOD_LIGHT);
    c.fill_rect(39, 29, 40, 38, 0x3E2814);
    for y in 31..37 {
        c.fill_rect(24, y, 36, y, 0x5E3E20);
    }
    // Mahmaz ustundeki el.
    for y in 30..38 {
        let color = if y % 3 == 0 { wc::SKIN_DARK } else { wc::SKIN };
        c.fill_rect(14, y, 20, y, color);
    }
    // Alici ve ahsap kabza.
    c.fill_rect(24, 38, 39, 42, 0x4A4A54);
    c.fill_rect(24, 38, 38, 39, 0x74747F);
    for y in 41..WEAPON_H as i32 {
        let x0 = 30 + (y - 41) / 2;
        c.fill_rect(x0, y, x0 + 11, y, wc::WOOD);
        c.fill_rect(x0, y, x0 + 1, y, wc::WOOD_LIGHT);
    }
    // Kabzayi tutan el ve kol ceketi.
    for y in 42..WEAPON_H as i32 {
        let x0 = 30 + (y - 41) / 2;
        let color = if y % 3 == 0 { wc::SKIN_DARK } else { wc::SKIN };
        c.fill_rect(x0 + 11, y, x0 + 15, y, color);
    }
    c.fill_rect(48, 40, 51, 47, wc::CUFF);
    c.fill_rect(50, 41, 63, 47, wc::SLEEVE);
    c.fill_rect(60, 41, 63, 47, wc::SLEEVE_DARK);
}

/// Mitralyoz: uzun namlu + sarici drum + ahsap tabanca, en genis.
fn draw_machinegun(c: &mut Canvas) {
    use weapon_color as wc;

    // Uzun namlu (delikli).
    c.fill_rect(28, 1, 35, 27, wc::BODY);
    c.fill_rect(28, 1, 29, 27, wc::LIGHT);
    c.fill_rect(34, 1, 35, 27, wc::DARK);
    for y in 6..24 {
        if y % 3 == 0 {
            c.fill_rect(30, y, 32, y + 1, 0x16161C);
        }
    }
    // Namlu ucu.
    c.fill_rect(27, 0, 36, 3, 0x4A4A54);
    // Alici.
    c.fill_rect(24, 26, 39, 36, 0x4A4A54);
    c.fill_rect(24, 26, 38, 27, 0x74747F);
    c.fill_rect(38, 27, 39, 36, wc::DARK);
    // Sarici drum.
    c.disc(31, 42, 9, 0x16161C);
    c.disc(31, 42, 7, 0x2E2E36);
    c.disc(31, 42, 5, 0x46464F);
    c.disc(31, 42, 2, wc::DARK);
    // Ahsap tabanca kabzasi.
    for y in 33..WEAPON_H as i32 {
        let x0 = 38 + (y - 33) / 3;
        c.fill_rect(x0, y, x0 + 9, y, 0x30200F);
        c.fill_rect(x0 + 1, y, x0 + 8, y, wc::WOOD);
        c.fill_rect(x0 + 1, y, x0 + 2, y, wc::WOOD_LIGHT);
    }
    // Kabzayi tutan el.
    for y in 37..WEAPON_H as i32 {
        let x0 = 38 + (y - 33) / 3;
        let color = if y % 3 == 0 { wc::SKIN_DARK } else { wc::SKIN };
        c.fill_rect(x0 + 9, y, x0 + 13, y, color);
    }
    // Kol ceketi.
    c.fill_rect(52, 40, 55, 47, wc::CUFF);
    c.fill_rect(54, 41, 63, 47, wc::SLEEVE);
    c.fill_rect(60, 41, 63, 47, wc::SLEEVE_DARK);
}

// ---------------------------------------------------------------------------
// Sprite uretimi
// ---------------------------------------------------------------------------

fn build_enemy(kind: u8, angle: u8) -> Vec<u32> {
    let pal = palette(kind);
    let mut c = Canvas::new(SPR_W, SPR_H);
    match angle {
        0 => {
            blit(&mut c, &FRONT, pal);
            c.outline(0x0E1018);
            draw_rifle_front(&mut c, pal);
        }
        1 => {
            blit(&mut c, &SIDE, pal);
            c.outline(0x0E1018);
            draw_rifle_side(&mut c, pal);
        }
        _ => {
            blit(&mut c, &BACK, pal);
            c.outline(0x0E1018);
            draw_rifle_back(&mut c, pal);
        }
    }
    // Subay kepi: on tarafta siperlik (visor).
    if kind == 1 && angle != 1 {
        c.fill_rect(19, 7, 23, 7, 0x1B2430);
    }
    c.into_vec()
}

fn build_death(kind: u8, frame: u8) -> Vec<u32> {
    let pal = palette(kind);
    let mut c = Canvas::new(SPR_W, SPR_H);
    match frame {
        0 => {
            // Vurulmus ani: ayakta ama hafif zik, beden saga egilmis.
            blit(&mut c, &FRONT, pal);
            c.shift_rows(0, 13, 1);
            c.shift_rows(14, 29, 1);
            c.shift_rows(30, SPR_H - 1, 2);
            c.outline(0x0E1018);
            // Kan izleri.
            c.set(14, 20, 0x9E1B1B);
            c.set(15, 20, 0x9E1B1B);
            c.set(16, 21, 0x7A1414);
            c.set(13, 19, 0x7A1414);
            c.set(17, 22, 0x7A1414);
            c.set(15, 9, 0x7A1414);
            draw_rifle_front(&mut c, pal);
        }
        1 => {
            // Dizlerden cokmus: kuculmus, alacak pozisyon.
            blit(&mut c, &KNEEL, pal);
            c.outline(0x0E1018);
            c.set(13, 22, 0x9E1B1B);
            c.set(14, 23, 0x7A1414);
            // Yere dusmus tufek (yanda).
            c.fill_rect(22, 41, 30, 43, pal.wood_dark);
            c.fill_rect(23, 41, 29, 41, pal.wood);
            c.fill_rect(22, 40, 25, 40, pal.metal_dark);
        }
        _ => {
            // Yatmis.
            blit(&mut c, &LIE, pal);
            c.outline(0x0E1018);
            // Yere dusmus silah.
            c.fill_rect(2, 45, 16, 46, 0x2A2A31);
            c.fill_rect(2, 45, 16, 45, 0x6E6E7A);
            c.fill_rect(16, 45, 20, 47, 0x4E3018);
        }
    }
    c.into_vec()
}

fn build_item(kind: u8) -> Vec<u32> {
    let mut c = Canvas::new(ITEM_W, ITEM_H);
    let art: &[[u8; ITEM_W]; ITEM_H] = match kind {
        ITEM_AMMO => &AMMO,
        ITEM_HEALTH => &HEALTH,
        ITEM_ARMOR => &ARMOR,
        ITEM_KEY_BLUE => &KEY_BLUE,
        ITEM_KEY_YELLOW => &KEY_YELLOW,
        ITEM_KEY_RED => &KEY_RED,
        ITEM_GOLD => &GOLD,
        _ => &SILVER,
    };
    blit_item(&mut c, art);
    c.into_vec()
}
fn build_weapon(kind: u8) -> Vec<u32> {
    let mut c = Canvas::new(WEAPON_W, WEAPON_H);
    match kind {
        0 => draw_pistol(&mut c),
        1 => draw_shotgun(&mut c),
        _ => draw_machinegun(&mut c),
    }
    // Koyu arka plan uzerinde silueti okunur kilmak icin kontur.
    c.outline(0x0E1018);
    c.into_vec()
}

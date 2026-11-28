//! Wolfenstein 3D tarzi seviye verisi.
//!
//! Seviyeler 32x32 karakterlik ASCII haritalardir; her karakter hucrenin
//! tipini ve zemin/tavan/dokularini birlikte belirler. Haritalar
//! `tools/gen_levels.py` ile uretilir ve dogrulanir (satir uzunlugu,
//! erisilebilirlik, cikis kontrolu).
//!
//! Karakterler:
//! - `#` tuğla duvar, `%` taş duvar, `&` metal duvar, `=` panel duvar
//! - `^` itilebilir gizli duvar
//! - `+` kapi (anahtarsiz), `1`/`2`/`3` mavi/sari/kirmizi anahtar kapisi
//! - `X` cikis (asansor), `S` gizli kapi (duvar gibi gorunur)
//! - `.` tas zemin, `,` dama zemin, `;` ahsap zemin, `:` hali zemin, `!` metal zemin
//! - `e`/`f`/`j` dusman (tur 0/1/2)
//! - `a` mermi, `h` can, `r` zirh, `k`/`y`/`d` anahtar, `g` altin, `v` gumus
//! - `@` oyuncu baslangici

use alloc::vec::Vec;

use crate::apps::fps_textures;

/// Hucre tipleri.
pub const TILE_EMPTY: u8 = 0;
pub const TILE_WALL: u8 = 1;
pub const TILE_DOOR: u8 = 2;
pub const TILE_PUSH: u8 = 3;
pub const TILE_EXIT: u8 = 4;
pub const TILE_SECRET: u8 = 5;

/// Kapinin gerektirdigi anahtar (0 = anahtarsiz).
pub const KEY_NONE: u8 = 0;
pub const KEY_BLUE: u8 = 1;
pub const KEY_YELLOW: u8 = 2;
pub const KEY_RED: u8 = 3;

/// Esya turleri (fps_sprites ile ayni kimlikler).
pub const ITEM_AMMO: u8 = 0;
pub const ITEM_HEALTH: u8 = 1;
pub const ITEM_ARMOR: u8 = 2;
pub const ITEM_KEY_BLUE: u8 = 3;
pub const ITEM_KEY_YELLOW: u8 = 4;
pub const ITEM_KEY_RED: u8 = 5;
pub const ITEM_GOLD: u8 = 6;
pub const ITEM_SILVER: u8 = 7;

/// Dusman turleri.
pub const ENEMY_SOLDIER: u8 = 0;
pub const ENEMY_OFFICER: u8 = 1;
pub const ENEMY_GUARD: u8 = 2;

pub const LEVEL_W: usize = 32;
pub const LEVEL_H: usize = 32;

/// Esya/dusman dogma noktasi.
#[derive(Clone, Copy)]
pub struct Spawn {
    pub x: f32,
    pub y: f32,
    /// ITEM_* veya ENEMY_* kimligi.
    pub kind: u8,
    /// true ise dusman, false ise esya.
    pub is_enemy: bool,
}

/// Calisma anindaki seviye durumu.
pub struct Level {
    pub w: usize,
    pub h: usize,
    pub tiles: Vec<u8>,
    pub wall_tex: Vec<u8>,
    pub floor_tex: Vec<u8>,
    pub ceil_tex: Vec<u8>,
    /// Hucre basina kapi anahtari (yalnizca TILE_DOOR icin anlamli).
    pub door_key: Vec<u8>,
    pub spawns: Vec<Spawn>,
    pub start_x: f32,
    pub start_y: f32,
}

impl Level {
    /// Hucre tipi (sinir disi TILE_WALL doner).
    #[inline(always)]
    pub fn tile(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return TILE_WALL;
        }
        self.tiles[y * self.w + x]
    }

    #[inline(always)]
    pub fn wall_tex(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return fps_textures::WALL_BRICK;
        }
        self.wall_tex[y * self.w + x]
    }

    #[inline(always)]
    pub fn floor_tex(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return fps_textures::FLOOR_STONE;
        }
        self.floor_tex[y * self.w + x]
    }

    #[inline(always)]
    pub fn ceil_tex(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return fps_textures::CEIL_CONCRETE;
        }
        self.ceil_tex[y * self.w + x]
    }

    #[inline(always)]
    pub fn door_key(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            return KEY_NONE;
        }
        self.door_key[y * self.w + x]
    }

    #[inline(always)]
    pub fn set_tile(&mut self, x: usize, y: usize, tile: u8) {
        if x < self.w && y < self.h {
            self.tiles[y * self.w + x] = tile;
        }
    }
}

/// ASCII haritayi ayristirip calisma ani seviyesine cevirir.
pub fn parse(rows: &[&str]) -> Level {
    let mut level = Level {
        w: LEVEL_W,
        h: LEVEL_H,
        tiles: alloc::vec![TILE_WALL; LEVEL_W * LEVEL_H],
        wall_tex: alloc::vec![fps_textures::WALL_BRICK; LEVEL_W * LEVEL_H],
        floor_tex: alloc::vec![fps_textures::FLOOR_STONE; LEVEL_W * LEVEL_H],
        ceil_tex: alloc::vec![fps_textures::CEIL_CONCRETE; LEVEL_W * LEVEL_H],
        door_key: alloc::vec![KEY_NONE; LEVEL_W * LEVEL_H],
        spawns: Vec::new(),
        start_x: 1.5,
        start_y: 1.5,
    };

    for (y, row) in rows.iter().enumerate() {
        if y >= LEVEL_H {
            break;
        }
        let bytes = row.as_bytes();
        for (x, &ch) in bytes.iter().enumerate() {
            if x >= LEVEL_W {
                break;
            }
            let idx = y * LEVEL_W + x;
            let (tile, wall, floor, ceil, key) = decode(ch);
            level.tiles[idx] = tile;
            level.wall_tex[idx] = wall;
            level.floor_tex[idx] = floor;
            level.ceil_tex[idx] = ceil;
            level.door_key[idx] = key;

            match ch {
                b'@' => {
                    level.start_x = x as f32 + 0.5;
                    level.start_y = y as f32 + 0.5;
                    level.tiles[idx] = TILE_EMPTY;
                }
                b'e' | b'f' | b'j' => {
                    let kind = match ch {
                        b'f' => ENEMY_OFFICER,
                        b'j' => ENEMY_GUARD,
                        _ => ENEMY_SOLDIER,
                    };
                    level.spawns.push(Spawn {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                        kind,
                        is_enemy: true,
                    });
                    level.tiles[idx] = TILE_EMPTY;
                }
                b'a' | b'h' | b'r' | b'k' | b'y' | b'd' | b'g' | b'v' => {
                    let kind = match ch {
                        b'h' => ITEM_HEALTH,
                        b'r' => ITEM_ARMOR,
                        b'k' => ITEM_KEY_BLUE,
                        b'y' => ITEM_KEY_YELLOW,
                        b'd' => ITEM_KEY_RED,
                        b'g' => ITEM_GOLD,
                        b'v' => ITEM_SILVER,
                        _ => ITEM_AMMO,
                    };
                    level.spawns.push(Spawn {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                        kind,
                        is_enemy: false,
                    });
                    level.tiles[idx] = TILE_EMPTY;
                }
                _ => {}
            }
        }
    }
    level
}

/// Karakteri (hucre tipi, duvar/zemin/tavan dokusu, kapi anahtari) cozer.
fn decode(ch: u8) -> (u8, u8, u8, u8, u8) {
    use fps_textures as t;
    match ch {
        b'#' => (
            TILE_WALL,
            t::WALL_BRICK,
            t::FLOOR_STONE,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'%' => (
            TILE_WALL,
            t::WALL_STONE,
            t::FLOOR_STONE,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'&' => (
            TILE_WALL,
            t::WALL_METAL,
            t::FLOOR_METAL,
            t::CEIL_TILE,
            KEY_NONE,
        ),
        b'=' => (
            TILE_WALL,
            t::WALL_PANEL,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'^' => (
            TILE_PUSH,
            t::WALL_PUSH,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'+' => (
            TILE_DOOR,
            t::WALL_DOOR,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'1' => (
            TILE_DOOR,
            t::WALL_DOOR,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_BLUE,
        ),
        b'2' => (
            TILE_DOOR,
            t::WALL_DOOR,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_YELLOW,
        ),
        b'3' => (
            TILE_DOOR,
            t::WALL_DOOR,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_RED,
        ),
        b'X' => (
            TILE_EXIT,
            t::WALL_METAL,
            t::FLOOR_METAL,
            t::CEIL_TILE,
            KEY_NONE,
        ),
        b'S' => (
            TILE_SECRET,
            t::WALL_SECRET,
            t::FLOOR_STONE,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b'.' => (
            TILE_EMPTY,
            t::WALL_BRICK,
            t::FLOOR_STONE,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b',' => (
            TILE_EMPTY,
            t::WALL_BRICK,
            t::FLOOR_CHECKER,
            t::CEIL_LIGHT,
            KEY_NONE,
        ),
        b';' => (
            TILE_EMPTY,
            t::WALL_BRICK,
            t::FLOOR_WOOD,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
        b':' => (
            TILE_EMPTY,
            t::WALL_BRICK,
            t::FLOOR_CARPET,
            t::CEIL_DARK,
            KEY_NONE,
        ),
        b'!' => (
            TILE_EMPTY,
            t::WALL_BRICK,
            t::FLOOR_METAL,
            t::CEIL_TILE,
            KEY_NONE,
        ),
        _ => (
            TILE_WALL,
            t::WALL_BRICK,
            t::FLOOR_STONE,
            t::CEIL_CONCRETE,
            KEY_NONE,
        ),
    }
}

// Uretilmis: python tools/gen_levels.py
pub const LEVEL_COUNT: usize = 3;

/// 1. seviye: Kışla
pub const LEVEL_1_NAME: &str = "Kışla";
pub const LEVEL_1_ROWS: [&str; 32] = [
    "################################",
    "##########%%%%%%%%%%############",
    "##.a......%,,,,,,,,,#;;;;k;;;;##",
    "##...e....+,,,,,f,,,+;;;;;;j;;##",
    "##........%,,,r,,,,,#;;;;;;;;;##",
    "##......h.%,,,,,,,,,#;;;;;;;;y##",
    "#####+#########+#########+######",
    "##............................##",
    "##............................##",
    "##.............@.............g##",
    "##............................##",
    "##d.................j.........##",
    "##..e.........................##",
    "##..........f...............e.##",
    "#####+####=====+====#####+######",
    "##;;;;;;;;=:::::::::#.........##",
    "##;;;;;;;;=:::::::::#.........##",
    "##;;;;;;;;=:::::::::#.........##",
    "##;;;;;;;;=::::::::a#.........##",
    "##;;f;;;;;=:::::::::#.......e.##",
    "##;;;;;;;;=:v:::j:::#....h....##",
    "#####+#########+#########+######",
    "##..........................k.##",
    "##.r..............j.........e.##",
    "##....f.....................X.##",
    "##............................##",
    "#&&&^&&&&#######################",
    "#&,,,,,,&#######################",
    "#&,y,,d,&#######################",
    "#&,,,,,,&#######################",
    "#&&&&&&&&#######################",
    "################################",
];

/// 2. seviye: Ambarlar
pub const LEVEL_2_NAME: &str = "Ambarlar";
pub const LEVEL_2_ROWS: [&str; 32] = [
    "################################",
    "##############%%%%%%%%%%%%%%%%%#",
    "##a,,,,,,,,,,,%......r........%#",
    "##,,,e,,,,,,,,%..........e....%#",
    "##,,,,,,,,,,,,+....j..........%#",
    "##,,,,,,,,,f,,%..............k%#",
    "##,,,,,,,,,,,,%...............%#",
    "##,,,,,,,,,,,,@.............f.%#",
    "##,,,,,,,,,,h,2...............%#",
    "##,,,,,,,,,,,,%...............%#",
    "####+###======+=====#####+######",
    "##......=;;;;;;;;;;;#:::::::::##",
    "##......=;;;g;;;;;;;#:::::::::##",
    "##......=;;;;;;;;;;;#:::::::::##",
    "##y.....=;;f;;;;;;;;#:::::::::##",
    "##......=;;;;;;;;;;;#:::::e:::##",
    "##..j...=;;;;;1;;;;;#:::::::::##",
    "##......=;;;;;;;;;;;#:::::::::##",
    "##......=;;;;;;;;;;^S::::::::v##",
    "##......=;;;;;;;;;;;#:::::::::##",
    "##.....e=d;;;;;;j;;;#:::::::f:##",
    "##......=;;;;;;;;;;;#:::::::::##",
    "####+#########+##########+######",
    "##............................##",
    "##............................##",
    "##....a.............e.........##",
    "##..j.....r...................##",
    "##..........j............f....##",
    "##......................h...X.##",
    "##............................##",
    "################################",
    "################################",
];

/// 3. seviye: Kanalizasyon
pub const LEVEL_3_NAME: &str = "Kanalizasyon";
pub const LEVEL_3_ROWS: [&str; 32] = [
    "################################",
    "#######################%%%%%%%%#",
    "##.......#::::::::::::#;;;;;;;%#",
    "##.e...h.#:::::e:::e::#;;;;;;;%#",
    "##.......+::::::::::::+;;;;;y;%#",
    "##.......#::j:::X:::::#;;;;;;;%#",
    "##a......#::::::::::k:#;;;f;;;%#",
    "##.......#::::::::::::#;;;;;;;%#",
    "##....f..#:r:::@::::::#;;;;;;;%#",
    "##.......+::::::r:::::+;;;;;j;%#",
    "##.......#::::::::::::#;;;;;;;%#",
    "##############1########%%%%%%%%#",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "##########............##########",
    "#################1#####========#",
    "##;;;;;;;#,,,,,,,,,,,,#.......=#",
    "##;d;;;;;#,,,j,,,,,,,,#.......=#",
    "##;e;;;;;#,,,,,,,,,,,,#...f...=#",
    "##;;;;;;;+,v,,,,,,,,,,+.......=#",
    "##;;;;;;;#,,,,,,,,,,,,#.......=#",
    "##;;;;;g;#,,,,,,,,e,,,#...h...=#",
    "##;;;;f;;#,,S^,,,,,a,,#.......=#",
    "##;;;;;;;+,,,,,,,,,,,,+.....j.=#",
    "##;;;;;;;#,,,,,,,,,,,,#.......=#",
    "#######################========#",
    "################################",
];

// Dogrulama: tum satirlar 32 karakter; oyuncuya tum esyalar ve cikis ulasilabilir.

/// Seviyelerin sirasi ve adlari.
pub const LEVEL_NAMES: [&str; LEVEL_COUNT] = [LEVEL_1_NAME, LEVEL_2_NAME, LEVEL_3_NAME];

/// Seviye `index` icin ASCII satirlarini dondurur.
pub fn rows(index: usize) -> &'static [&'static str; LEVEL_H] {
    match index {
        0 => &LEVEL_1_ROWS,
        1 => &LEVEL_2_ROWS,
        _ => &LEVEL_3_ROWS,
    }
}

/// `index` seviyesini ayristirir (girdi sinir disindaysa son seviye).
pub fn load(index: usize) -> Level {
    let idx = index.min(LEVEL_COUNT - 1);
    parse(rows(idx))
}

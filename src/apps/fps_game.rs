//! Kurt 3D tarzı birinci şahıs nişancı (FPS) oyunu.
//!
//! Oyun tamamen yazılım rasterizasyonu ile çizilir: duvarlar DDA ışın
//! izleme (raycasting) ile, zemin/tavan düzlem atma (plane casting) ile,
//! düşman ve eşyalar ise billboard sprite olarak çizilir.
//!
//! Göreli fare hareketi birikimli olarak, mutlak işaretçi hareketi ise
//! konum farkıyla bakış yönüne çevrilir.

use crate::apps::fps_level::{self, Level, KEY_BLUE, KEY_RED, KEY_YELLOW};
use crate::apps::fps_sprites::{self, Sprites};
use crate::apps::fps_textures::{self, Atlas};
use crate::apps::gfx3d::{
    fast_atan2, fast_ceil, fast_cos, fast_floor, fast_sin, fast_sqrt, plane_depth, screen_y,
    MAX_DEPTH,
};
use crate::drivers::audio::AudioDevice;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use crate::time_budget::TimeBudget;
use alloc::vec::Vec;
use core::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// Sabitler
// ---------------------------------------------------------------------------

/// Duvar yüksekliği (dünya birimi).
const WALL_H: f32 = 1.0;
/// Göz yüksekliği.
const EYE_Z: f32 = 0.5;
/// Oyuncunun yarı genişliği (çarpışma yarıçapı).
const PLAYER_RADIUS: f32 = 0.22;

/// Yürüyüş hızı (dünya birimi / 60fps kare).
const WALK_SPEED: f32 = 0.075;
/// Koşu hızı.
const SPRINT_SPEED: f32 = 0.12;
/// Ok tuşlarıyla dönüş hızı (radyan / 60fps kare).
const ROT_SPEED: f32 = 0.040;
/// Fare hassasiyeti (radyan / piksel).
const MOUSE_SENS: f32 = 0.0060;
/// Kenar sürükleme hızı (radyan/saniye). İmleç ekran kenarına
/// dayandığında bakış bu hızda dönmeye devam eder.
const EDGE_TURN_RATE: f32 = 1.8;
/// Kenar bölgesi (piksel).
const EDGE_ZONE: f32 = 48.0;

/// Silahlar.
const WEAPON_PISTOL: u8 = 0;
const WEAPON_SHOTGUN: u8 = 1;
const WEAPON_MACHINEGUN: u8 = 2;
const WEAPON_COUNT: usize = 3;

const PISTOL_AMMO_MAX: u32 = 50;
const SHOTGUN_AMMO_MAX: u32 = 25;
const MACHINEGUN_AMMO_MAX: u32 = 120;

const PISTOL_FIRE_DELAY: usize = 8;
const SHOTGUN_FIRE_DELAY: usize = 20;
const MACHINEGUN_FIRE_DELAY: usize = 4;

const SHOTGUN_PELLETS: usize = 5;
const RELOAD_TICKS: usize = 40;

const MAX_ENEMIES: usize = 24;
const MAX_PICKUPS: usize = 24;
const MAX_DOORS: usize = 16;
const MAX_PUSHWALLS: usize = 8;

/// Durum çubuğu yüksekliği (piksel).
const STATUS_BAR_H: usize = 58;
/// Üst araç çubuğu yüksekliği.
const TOOLBAR_H: usize = 26;

/// İç çözünürlük ölçeği (1 = pencere çözünürlüğü).
const RENDER_SCALE_MIN: usize = 1;
const RENDER_SCALE_MAX: usize = 3;

/// Z-buffer kapasitesi (iç sütun sayısı üst sınırı).
const ZBUF_MAX: usize = 1600;

/// Kapı animasyonu: tam açılma/kapama süresi (kare).
const DOOR_ANIM_TICKS: f32 = 12.0;

/// Düşman türleri için can değerleri.
const ENEMY_HP: [u8; 3] = [2, 3, 4];
/// Düşman türleri için hız.
const ENEMY_SPEED: [f32; 3] = [0.030, 0.026, 0.034];

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

/// Mesafeye göre 32 kademeli aydınlatma çarpanı tablosu.
///
/// Her kademe RGB çarpanı olarak paketlenir (`0xRRGGBB`, `0xFF` = değişmez).
const fn build_shade_lut() -> [u32; 32] {
    let mut lut = [0u32; 32];
    let mut i = 0;
    while i < 32 {
        // 0 = en yakın (parlak), 31 = en uzak (koyu).
        let f = (255 - (i * 143) / 31) as u32;
        lut[i] = (f << 16) | (f << 8) | f;
        i += 1;
    }
    lut
}

static SHADE_LUT: [u32; 32] = build_shade_lut();

/// Rengi aydınlatma çarpanıyla karartır.
#[inline(always)]
fn shade_color(color: u32, factor: u32) -> u32 {
    let r = ((((color >> 16) & 0xFF) * ((factor >> 16) & 0xFF)) >> 8) & 0xFF;
    let g = ((((color >> 8) & 0xFF) * ((factor >> 8) & 0xFF)) >> 8) & 0xFF;
    let b = (((color & 0xFF) * (factor & 0xFF)) >> 8) & 0xFF;
    (r << 16) | (g << 8) | b
}

/// Derinliğe karşılık gelen aydınlatma çarpanı.
#[inline(always)]
fn depth_factor(depth: f32) -> u32 {
    let idx = (depth * 2.2) as usize;
    if idx >= 32 {
        SHADE_LUT[31]
    } else {
        SHADE_LUT[idx]
    }
}

/// İki açı arasındaki farkı -PI..=PI aralığına normalize eder.
#[inline(always)]
fn angle_diff(a: f32, b: f32) -> f32 {
    let mut rel = a - b;
    while rel < -PI {
        rel += TAU;
    }
    while rel > PI {
        rel -= TAU;
    }
    rel
}

// ---------------------------------------------------------------------------
// Oyun nesneleri
// ---------------------------------------------------------------------------

/// Düşman.
#[derive(Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    alive: bool,
    hp: u8,
    /// Hasar sonrası kırmızı yanma süresi.
    hit_timer: usize,
    /// Ölüm animasyonu sayacı (0 = ölmedi).
    death_timer: usize,
    attack_timer: usize,
    /// Baktığı yön (radyan).
    facing: f32,
    /// Yürüyüş animasyonu fazı.
    walk_phase: f32,
    kind: u8,
    speed: f32,
    /// Oyuncuyu fark etti mi?
    alert: bool,
}

/// Toplanabilir eşya.
#[derive(Clone, Copy)]
struct Pickup {
    x: f32,
    y: f32,
    kind: u8,
    active: bool,
    /// Salınım fazı (sprite'ın hafifçe yukarı aşağı hareketi).
    bob: f32,
}

/// Kapı.
#[derive(Clone, Copy)]
struct Door {
    x: usize,
    y: usize,
    /// Açılma oranı 0..1.
    open: f32,
    /// Hedef açılma oranı.
    target: f32,
    /// Gereken anahtar (0 = anahtarsız).
    key: u8,
}

/// İtilebilir gizli duvar.
#[derive(Clone, Copy)]
struct PushWall {
    x: usize,
    y: usize,
    /// Kayma animasyonu 0..1 (1 = bitti).
    slide: f32,
    /// Kayma hedefi.
    to_x: usize,
    to_y: usize,
    /// Kayma başlangıcı.
    from_x: usize,
    from_y: usize,
    moving: bool,
}

/// Görünüm modu.
#[derive(Clone, Copy, PartialEq)]
pub enum RenderMode {
    Mode3D,
    TopDown2D,
}

/// Oyun sonucu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GameResult {
    Menu,
    Playing,
    Won,
    Lost,
}

/// Durum çubuğu/otomatik harita için bilgi.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HudView {
    Status,
    Automap,
}

// ---------------------------------------------------------------------------
// Oyun durumu
// ---------------------------------------------------------------------------

pub struct FpsGameApp {
    // Kamera
    pub pos_x: f32,
    pub pos_y: f32,
    pub dir_x: f32,
    pub dir_y: f32,
    pub plane_x: f32,
    pub plane_y: f32,
    /// Dikey bakış açısı (radyan, pozitif = aşağı).
    pub pitch: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    /// Fare hassasiyeti (radyan/piksel).
    pub mouse_sens: f32,

    // Oyuncu durumu
    pub health: u32,
    pub armor: u32,
    pub score: u32,
    pub ammo: u32,
    pub shots_fired: u32,
    pub hits: u32,
    pub shooting: bool,
    pub mouse_fire_held: bool,
    pub shoot_timer: usize,
    /// Toplanan anahtarlar (mavi, sarı, kırmızı).
    pub keys: [bool; 3],

    // Silah durumu
    pub weapon: u8,
    pub ammo_per_weapon: [u32; WEAPON_COUNT],
    pub is_reloading: bool,
    pub reload_timer: usize,

    // Görsel efektler
    pub weapon_bob: f32,
    pub muzzle_flash_timer: usize,
    pub damage_flash_timer: usize,

    // Dünya
    level: Level,
    level_index: usize,
    atlas: Atlas,
    sprites: Sprites,
    enemies: [Enemy; MAX_ENEMIES],
    enemy_count: usize,
    pickups: [Pickup; MAX_PICKUPS],
    pickup_count: usize,
    doors: [Door; MAX_DOORS],
    door_count: usize,
    push_walls: [PushWall; MAX_PUSHWALLS],
    push_count: usize,

    // Durum
    result: GameResult,
    /// Oyuncu bir seviyeyi bitirdiğinde gösterilen sayaç.
    level_clear_timer: usize,
    /// Son seviye bitti mi?
    pub final_level_cleared: bool,
    pub render_mode: RenderMode,
    hud_view: HudView,
    /// Kullanıcı "itme" modunda mı (gizli duvarı iter).
    pushing: bool,

    // Girdi
    pub key_mask: u16,
    /// Son bilinen mutlak fare konumu.
    pointer_x: f32,
    pointer_y: f32,
    pointer_valid: bool,
    last_pointer_update_ms: u64,
    /// Önceki kareden bu kareye geçen süre (ms).
    last_update_ms: u64,

    // Ses durumu
    pub last_shot_tick: usize,
    pub last_step_tick: usize,
    pub moving: bool,
    pub sound_pending: Option<(u32, usize)>,

    // Performans ölçümü
    pub fps: usize,
    pub min_fps: usize,
    pub frame_time_ms: u64,
    pub frame_interval_ms: u64,
    pub rendered_frames: u64,
    last_completed_frame_ms: u64,
    metrics_window_start_ms: u64,
    metrics_window_frames: u64,
    metrics_window_max_interval_ms: u64,
    pub mem_peak_used_kb: u64,
    pub mem_free_kb: u64,
    pub mem_heap_kb: u64,

    /// İç çözünürlük ölçeği (1 = tam çözünürlük).
    render_scale: usize,
    /// Sütun derinlik tamponu.
    zbuf: [f32; ZBUF_MAX],
    /// Sütun başına ışın yönü (x, y çiftleri).
    rays: [f32; ZBUF_MAX * 2],
}

// ---------------------------------------------------------------------------
// Tuş maskesi
// ---------------------------------------------------------------------------

const KEY_W: u16 = 1 << 0;
const KEY_S: u16 = 1 << 1;
const KEY_A: u16 = 1 << 2;
const KEY_D: u16 = 1 << 3;
const KEY_UP: u16 = 1 << 4;
const KEY_DOWN: u16 = 1 << 5;
const KEY_LEFT: u16 = 1 << 6;
const KEY_RIGHT: u16 = 1 << 7;
const KEY_SHIFT: u16 = 1 << 8;

// ---------------------------------------------------------------------------
// Uygulama
// ---------------------------------------------------------------------------

impl FpsGameApp {
    /// Yeni oyun: birinci seviye, oyuncu başlangıç noktasında.
    pub fn new() -> Self {
        let mut app = Self {
            pos_x: 1.5,
            pos_y: 1.5,
            dir_x: 1.0,
            dir_y: 0.0,
            plane_x: 0.0,
            plane_y: 1.0,
            pitch: 0.0,
            vel_x: 0.0,
            vel_y: 0.0,
            mouse_sens: MOUSE_SENS,
            health: 100,
            armor: 0,
            score: 0,
            ammo: PISTOL_AMMO_MAX,
            shots_fired: 0,
            hits: 0,
            shooting: false,
            mouse_fire_held: false,
            shoot_timer: 0,
            keys: [false; 3],
            weapon: WEAPON_PISTOL,
            ammo_per_weapon: [PISTOL_AMMO_MAX, SHOTGUN_AMMO_MAX, MACHINEGUN_AMMO_MAX],
            is_reloading: false,
            reload_timer: 0,
            weapon_bob: 0.0,
            muzzle_flash_timer: 0,
            damage_flash_timer: 0,
            level: fps_level::load(0),
            level_index: 0,
            atlas: Atlas::new(),
            sprites: Sprites::new(),
            enemies: [Enemy {
                x: 0.0,
                y: 0.0,
                alive: false,
                hp: 0,
                hit_timer: 0,
                death_timer: 0,
                attack_timer: 0,
                facing: 0.0,
                walk_phase: 0.0,
                kind: 0,
                speed: 0.0,
                alert: false,
            }; MAX_ENEMIES],
            enemy_count: 0,
            pickups: [Pickup {
                x: 0.0,
                y: 0.0,
                kind: 0,
                active: false,
                bob: 0.0,
            }; MAX_PICKUPS],
            pickup_count: 0,
            doors: [Door {
                x: 0,
                y: 0,
                open: 0.0,
                target: 0.0,
                key: 0,
            }; MAX_DOORS],
            door_count: 0,
            push_walls: [PushWall {
                x: 0,
                y: 0,
                slide: 1.0,
                to_x: 0,
                to_y: 0,
                from_x: 0,
                from_y: 0,
                moving: false,
            }; MAX_PUSHWALLS],
            push_count: 0,
            result: GameResult::Menu,
            level_clear_timer: 0,
            final_level_cleared: false,
            render_mode: RenderMode::Mode3D,
            hud_view: HudView::Status,
            pushing: false,
            key_mask: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            pointer_valid: false,
            last_pointer_update_ms: 0,
            last_update_ms: 0,
            last_shot_tick: 0,
            last_step_tick: 0,
            moving: false,
            sound_pending: None,
            fps: 0,
            min_fps: 0,
            frame_time_ms: 0,
            frame_interval_ms: 0,
            rendered_frames: 0,
            last_completed_frame_ms: 0,
            metrics_window_start_ms: 0,
            metrics_window_frames: 0,
            metrics_window_max_interval_ms: 0,
            mem_peak_used_kb: 0,
            mem_free_kb: 0,
            mem_heap_kb: 0,
            render_scale: RENDER_SCALE_MIN,
            zbuf: [0.0; ZBUF_MAX],
            rays: [0.0; ZBUF_MAX * 2],
        };
        app.load_level(0);
        app
    }

    // -----------------------------------------------------------------------
    // Seviye yükleme
    // -----------------------------------------------------------------------

    /// Seviyeyi yükler: düşman/eşya/kapı/gizli duvar listelerini kurar ve
    /// oyuncuyu başlangıç noktasına taşır.
    fn load_level(&mut self, index: usize) {
        self.level_index = index.min(fps_level::LEVEL_COUNT - 1);
        self.level = fps_level::load(self.level_index);

        // Düşmanlar.
        self.enemy_count = 0;
        for spawn in self.level.spawns.iter() {
            if !spawn.is_enemy || self.enemy_count >= MAX_ENEMIES {
                continue;
            }
            let kind = spawn.kind.min(2);
            self.enemies[self.enemy_count] = Enemy {
                x: spawn.x,
                y: spawn.y,
                alive: true,
                hp: ENEMY_HP[kind as usize],
                hit_timer: 0,
                death_timer: 0,
                attack_timer: 0,
                facing: 0.0,
                walk_phase: 0.0,
                kind,
                speed: ENEMY_SPEED[kind as usize],
                alert: false,
            };
            self.enemy_count += 1;
        }

        // Eşyalar.
        self.pickup_count = 0;
        for spawn in self.level.spawns.iter() {
            if spawn.is_enemy || self.pickup_count >= MAX_PICKUPS {
                continue;
            }
            self.pickups[self.pickup_count] = Pickup {
                x: spawn.x,
                y: spawn.y,
                kind: spawn.kind,
                active: true,
                bob: 0.0,
            };
            self.pickup_count += 1;
        }

        // Kapılar.
        self.door_count = 0;
        for y in 0..self.level.h {
            for x in 0..self.level.w {
                if self.level.tile(x, y) == fps_level::TILE_DOOR && self.door_count < MAX_DOORS {
                    self.doors[self.door_count] = Door {
                        x,
                        y,
                        open: 0.0,
                        target: 0.0,
                        key: self.level.door_key(x, y),
                    };
                    self.door_count += 1;
                }
            }
        }

        // İtilebilir gizli duvarlar.
        self.push_count = 0;
        for y in 0..self.level.h {
            for x in 0..self.level.w {
                if self.level.tile(x, y) == fps_level::TILE_PUSH && self.push_count < MAX_PUSHWALLS
                {
                    self.push_walls[self.push_count] = PushWall {
                        x,
                        y,
                        slide: 1.0,
                        to_x: x,
                        to_y: y,
                        from_x: x,
                        from_y: y,
                        moving: false,
                    };
                    self.push_count += 1;
                }
            }
        }

        // Oyuncu başlangıcı.
        self.pos_x = self.level.start_x;
        self.pos_y = self.level.start_y;
        self.dir_x = 1.0;
        self.dir_y = 0.0;
        self.plane_x = 0.0;
        self.plane_y = 1.0;
        self.pitch = 0.0;
        self.vel_x = 0.0;
        self.vel_y = 0.0;
        self.pushing = false;
        self.hud_view = HudView::Status;
        self.level_clear_timer = 0;
    }

    /// Sonraki seviyeye geçer (çıkış levhasına basıldığında).
    fn next_level(&mut self) {
        if self.level_index + 1 < fps_level::LEVEL_COUNT {
            self.load_level(self.level_index + 1);
        } else {
            self.final_level_cleared = true;
            self.result = GameResult::Won;
        }
    }

    // -----------------------------------------------------------------------
    // Çarpışma ve dünya sorguları
    // -----------------------------------------------------------------------

    /// Kapının açılma oranı (0 = kapalı, 1 = tam açık).
    fn door_open_at(&self, x: usize, y: usize) -> f32 {
        for d in self.doors.iter().take(self.door_count) {
            if d.x == x && d.y == y {
                return d.open;
            }
        }
        0.0
    }

    /// Verilen hücre oyuncunun geçemeyeceği bir engel mi?
    fn cell_blocks(&self, x: usize, y: usize) -> bool {
        let tile = self.level.tile(x, y);
        match tile {
            fps_level::TILE_WALL | fps_level::TILE_PUSH | fps_level::TILE_SECRET => true,
            fps_level::TILE_DOOR => self.door_open_at(x, y) < 0.98,
            _ => false,
        }
    }

    /// Oyuncunun dört köşesini duvar/kapalı kapıya karşı test eder.
    fn is_wall(&self, x: f32, y: f32) -> bool {
        let r = PLAYER_RADIUS;
        let corners = [
            (x - r, y - r),
            (x + r, y - r),
            (x - r, y + r),
            (x + r, y + r),
        ];
        for (cx, cy) in corners {
            if cx < 0.0 || cy < 0.0 {
                return true;
            }
            let mx = cx as usize;
            let my = cy as usize;
            if mx >= self.level.w || my >= self.level.h {
                return true;
            }
            if self.cell_blocks(mx, my) {
                return true;
            }
        }
        false
    }

    /// Düşman çarpışması (düşmanlar oyuncu radius'unda değil).
    fn enemy_blocked(&self, x: f32, y: f32) -> bool {
        let corners = [
            (x - 0.18, y - 0.18),
            (x + 0.18, y - 0.18),
            (x - 0.18, y + 0.18),
            (x + 0.18, y + 0.18),
        ];
        for (cx, cy) in corners {
            if cx < 0.0 || cy < 0.0 {
                return true;
            }
            let mx = cx as usize;
            let my = cy as usize;
            if mx >= self.level.w || my >= self.level.h {
                return true;
            }
            if self.cell_blocks(mx, my) {
                return true;
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Hareket
    // -----------------------------------------------------------------------

    /// Hareketi uygular (duvar boyunca kayma dahil).
    fn move_player(&mut self, dx: f32, dy: f32) {
        if !self.is_wall(self.pos_x + dx, self.pos_y + dy) {
            self.pos_x += dx;
            self.pos_y += dy;
            return;
        }
        if !self.is_wall(self.pos_x + dx, self.pos_y) {
            self.pos_x += dx;
            return;
        }
        if !self.is_wall(self.pos_x, self.pos_y + dy) {
            self.pos_y += dy;
        }
    }

    /// Yön vektörünü verilen açı kadar döndürür.
    fn rotate(&mut self, angle: f32) {
        let (sin_a, cos_a) = (fast_sin(angle), fast_cos(angle));
        let old_dir_x = self.dir_x;
        self.dir_x = self.dir_x * cos_a - self.dir_y * sin_a;
        self.dir_y = old_dir_x * sin_a + self.dir_y * cos_a;
        let old_plane_x = self.plane_x;
        self.plane_x = self.plane_x * cos_a - self.plane_y * sin_a;
        self.plane_y = old_plane_x * sin_a + self.plane_y * cos_a;
        let dir_len = fast_sqrt(self.dir_x * self.dir_x + self.dir_y * self.dir_y);
        if dir_len > 0.0 {
            self.dir_x /= dir_len;
            self.dir_y /= dir_len;
        }
        let plane_len = fast_sqrt(self.plane_x * self.plane_x + self.plane_y * self.plane_y);
        if plane_len > 0.0 {
            self.plane_x /= plane_len;
            self.plane_y /= plane_len;
        }
    }

    /// Pitch ile kaymış ufuk satırının ekran y koordinatı (iç çözünürlük).
    pub fn horizon_y(&self, view_h: usize) -> f32 {
        let vh = view_h as f32;
        (vh / 2.0 - self.pitch * vh * 0.5).clamp(vh * 0.25, vh * 0.75)
    }

    /// Fare ile bakış açısını günceller (dx = yatay, dy = dikey).
    pub fn look(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let bounded_dx = dx.clamp(-160.0, 160.0);
        let bounded_dy = dy.clamp(-120.0, 120.0);
        // Fare yönü ekran koordinatıdır: sağa hareket dx > 0, aşağı dy > 0.
        self.rotate(bounded_dx * self.mouse_sens);
        self.pitch = (self.pitch + bounded_dy * self.mouse_sens).clamp(-PI / 3.0, PI / 3.0);
    }

    /// Göreli fare hareketini, kareler arasında birikmiş olsa da kayıpsız uygular.
    pub fn handle_mouse_motion(&mut self, mut dx: i32, mut dy: i32) {
        if self.result != GameResult::Playing {
            return;
        }
        // Bozuk veya kenar-wraparound kaynaklı devasa deltalar döngüyü
        // uzun süre kilitler (her 160 px'lik adım look() çağırır); toplam
        // birikimi insanın algılayamayacağı bir sınıra kırp.
        dx = dx.clamp(-2048, 2048);
        dy = dy.clamp(-2048, 2048);
        while dx != 0 || dy != 0 {
            let step_x = dx.clamp(-160, 160);
            let step_y = dy.clamp(-120, 120);
            self.look(step_x as f32, step_y as f32);
            dx -= step_x;
            dy -= step_y;
        }
    }

    // -----------------------------------------------------------------------
    // Girdi
    // -----------------------------------------------------------------------

    /// Mutlak fare konumundan bakış yönünü günceller.
    ///
    /// Fare mutlak (absolute) konum bildirdiği için konum farkı kullanılır;
    /// bu, fareyi doğrudan 1:1 bir "fare hareketi" hissine yaklaştırır.
    /// İmleç ekran kenarına dayandığında hareket olayı gelmez; bu durumda
    /// "kenar sürükleme" ile bakış sabit bir hızda dönmeye devam eder.
    pub fn update_pointer(&mut self, px: f32, py: f32, screen_w: usize, screen_h: usize) {
        let now = crate::monotonic_ms();
        let dt = if self.last_pointer_update_ms == 0 {
            1.0 / 60.0
        } else {
            now.saturating_sub(self.last_pointer_update_ms).min(50) as f32 / 1000.0
        };
        self.last_pointer_update_ms = now;

        if self.result != GameResult::Playing {
            self.pointer_x = px;
            self.pointer_y = py;
            self.pointer_valid = true;
            return;
        }
        if !self.pointer_valid {
            self.pointer_x = px;
            self.pointer_y = py;
            self.pointer_valid = true;
            return;
        }

        let dx = px - self.pointer_x;
        let dy = py - self.pointer_y;
        self.pointer_x = px;
        self.pointer_y = py;

        // Göreceli hareket doğal 1:1 dönüş sağlar.
        let moved = dx != 0.0 || dy != 0.0;
        if moved {
            self.look(dx, dy);
        }

        // Kenar sürükleme: yalnızca imleç hareket ediyorken etkin.
        // Mutlak işaretçi (tablet) ekran kenarına dayandığında hareket
        // olayı üretmediği için bu yedek yol gerekir; ancak imleç
        // duruyken sürekli dönüşe yol açmamalı.
        if moved {
            let sw = screen_w as f32;
            let sh = screen_h as f32;
            let edge_w = EDGE_ZONE.min(sw * 0.15);
            let edge_h = EDGE_ZONE.min(sh * 0.15);
            let mut turn = 0.0f32;
            let mut pitch_turn = 0.0f32;
            if px <= edge_w {
                turn -= EDGE_TURN_RATE * dt * (1.0 - px / edge_w);
            } else if px >= sw - edge_w {
                turn += EDGE_TURN_RATE * dt * (1.0 - (sw - px) / edge_w);
            }
            if py <= edge_h {
                pitch_turn -= EDGE_TURN_RATE * dt * (1.0 - py / edge_h);
            } else if py >= sh - edge_h {
                pitch_turn += EDGE_TURN_RATE * dt * (1.0 - (sh - py) / edge_h);
            }
            if turn != 0.0 {
                self.rotate(turn);
            }
            if pitch_turn != 0.0 {
                self.pitch = (self.pitch + pitch_turn).clamp(-PI / 3.0, PI / 3.0);
            }
        }
    }

    /// Fare butonunun mevcut durumunu oyun döngüsüne aktarır.
    pub fn set_mouse_button(&mut self, pressed: bool) {
        self.mouse_fire_held = pressed;
    }

    /// Eski arayüzle uyumlu: doğrudan fare delta verilir.
    pub fn handle_mouse_look(&mut self, dx: f32, dy: f32) {
        if self.result == GameResult::Playing {
            self.look(dx.clamp(-96.0, 96.0), dy.clamp(-64.0, 64.0));
        }
    }

    /// Klavye girdisini işler. true dönerse tüketildi.
    pub fn handle_key(&mut self, ch: char, audio_dev: &mut AudioDevice) -> bool {
        match self.result {
            GameResult::Menu => match ch {
                ' ' | '\n' | '\r' => {
                    self.result = GameResult::Playing;
                    self.play_sound(audio_dev, SoundKind::Pickup);
                    return true;
                }
                't' | 'T' => {
                    self.render_mode = match self.render_mode {
                        RenderMode::Mode3D => RenderMode::TopDown2D,
                        RenderMode::TopDown2D => RenderMode::Mode3D,
                    };
                    return true;
                }
                _ => return false,
            },
            GameResult::Won | GameResult::Lost => {
                if ch == 'r' || ch == 'R' || ch == ' ' {
                    *self = Self::new();
                    self.result = GameResult::Playing;
                    return true;
                }
                return false;
            }
            GameResult::Playing => {}
        }

        // Esc: menüye dön ve basılı tuşları temizle.
        if ch == '\x1b' {
            self.result = GameResult::Menu;
            self.key_mask = 0;
            self.shooting = false;
            self.mouse_fire_held = false;
            self.pushing = false;
            return true;
        }
        if ch == '\n' || ch == '\r' {
            return false;
        }
        match ch {
            'w' | 'W' => {
                self.key_mask |= KEY_W;
                true
            }
            's' | 'S' => {
                self.key_mask |= KEY_S;
                true
            }
            'a' | 'A' => {
                self.key_mask |= KEY_A;
                true
            }
            'd' | 'D' => {
                self.key_mask |= KEY_D;
                true
            }
            ' ' => {
                // Kurt 3D'de boşluk tuşu "kullan" (kapı aç / duvar it);
                // ateş için sol fare tuşu kullanılır.
                self.use_action(audio_dev);
                true
            }
            'e' | 'E' => {
                self.use_action(audio_dev);
                true
            }
            '1' => {
                self.select_weapon(WEAPON_PISTOL);
                true
            }
            '2' => {
                self.select_weapon(WEAPON_SHOTGUN);
                true
            }
            '3' => {
                self.select_weapon(WEAPON_MACHINEGUN);
                true
            }
            'r' | 'R' => {
                self.start_reload();
                true
            }
            '\t' => {
                self.hud_view = match self.hud_view {
                    HudView::Status => HudView::Automap,
                    HudView::Automap => HudView::Status,
                };
                true
            }
            't' | 'T' => {
                self.render_mode = match self.render_mode {
                    RenderMode::Mode3D => RenderMode::TopDown2D,
                    RenderMode::TopDown2D => RenderMode::Mode3D,
                };
                true
            }
            _ => false,
        }
    }

    /// Silah seçimi.
    fn select_weapon(&mut self, weapon: u8) {
        if weapon >= WEAPON_COUNT as u8 {
            return;
        }
        if self.weapon == weapon {
            return;
        }
        self.weapon = weapon;
        self.ammo = self.ammo_per_weapon[weapon as usize];
        self.is_reloading = false;
        self.reload_timer = 0;
        self.shoot_timer = 0;
    }

    /// Tekerlekle silah değiştirir.
    pub fn cycle_weapon(&mut self, delta: i32) {
        if self.shoot_timer > 0 || self.is_reloading {
            return;
        }
        let count = WEAPON_COUNT as i32;
        let next = ((self.weapon as i32 + delta) % count + count) % count;
        self.select_weapon(next as u8);
    }

    /// Ok tuşu ile dönüş.
    pub fn handle_arrow(&mut self, left: bool, right: bool) {
        if self.result != GameResult::Playing {
            return;
        }
        if left {
            self.key_mask |= KEY_LEFT;
        }
        if right {
            self.key_mask |= KEY_RIGHT;
        }
    }

    /// Ok tuşu ile ileri/geri hareket.
    pub fn handle_arrow_up_down(&mut self, up: bool, down: bool) {
        if self.result != GameResult::Playing {
            return;
        }
        if up {
            self.key_mask |= KEY_UP;
        }
        if down {
            self.key_mask |= KEY_DOWN;
        }
    }

    /// Basılı tuşları günceller (WASD + oklar + Shift).
    pub fn sync_held_keys(&mut self, kbd: &crate::drivers::keyboard::KeyboardState) {
        if self.result != GameResult::Playing {
            return;
        }
        if kbd.is_held(0x11) {
            self.key_mask |= KEY_W;
        }
        if kbd.is_held(0x1F) {
            self.key_mask |= KEY_S;
        }
        if kbd.is_held(0x1E) {
            self.key_mask |= KEY_A;
        }
        if kbd.is_held(0x20) {
            self.key_mask |= KEY_D;
        }
        if kbd.is_held_extended(0x48) {
            self.key_mask |= KEY_UP;
        }
        if kbd.is_held_extended(0x50) {
            self.key_mask |= KEY_DOWN;
        }
        if kbd.is_held_extended(0x4B) {
            self.key_mask |= KEY_LEFT;
        }
        if kbd.is_held_extended(0x4D) {
            self.key_mask |= KEY_RIGHT;
        }
        if kbd.shift {
            self.key_mask |= KEY_SHIFT;
        }
    }

    /// "Kullan" eylemi: kapı aç/kapat, gizli kapı aç, itme moduna geç.
    ///
    /// Sağ fare tuşu ve `E` tuşu bu eylemi tetikler.
    pub fn handle_use(&mut self, audio_dev: &mut AudioDevice) {
        if self.result != GameResult::Playing {
            return;
        }
        self.use_action(audio_dev);
    }

    /// Fare tıklaması ile ateş (menüde oyunu başlatır).
    pub fn handle_mouse_click(&mut self, audio_dev: &mut AudioDevice) -> bool {
        match self.result {
            GameResult::Menu => {
                self.result = GameResult::Playing;
                self.play_sound(audio_dev, SoundKind::Pickup);
                return true;
            }
            GameResult::Won | GameResult::Lost => {
                *self = Self::new();
                self.result = GameResult::Playing;
                return true;
            }
            GameResult::Playing => {}
        }
        self.fire_weapon(audio_dev);
        true
    }

    // -----------------------------------------------------------------------
    // Silah
    // -----------------------------------------------------------------------

    /// Ateş eder (gecikme ve otomatik doldurma dahil).
    fn fire_weapon(&mut self, audio_dev: &mut AudioDevice) {
        if self.shoot_timer != 0 || self.is_reloading {
            return;
        }
        if self.ammo_per_weapon[self.weapon as usize] == 0 {
            self.start_reload();
            self.play_sound(audio_dev, SoundKind::Click);
            return;
        }
        let delay = match self.weapon {
            WEAPON_SHOTGUN => SHOTGUN_FIRE_DELAY,
            WEAPON_MACHINEGUN => MACHINEGUN_FIRE_DELAY,
            _ => PISTOL_FIRE_DELAY,
        };
        self.shoot_timer = delay;
        self.shooting = true;
        self.muzzle_flash_timer = 4;
        let hit = self.shoot();
        if hit {
            self.play_sound(audio_dev, SoundKind::Hit);
        } else {
            self.play_sound(audio_dev, SoundKind::Shoot);
        }
    }

    /// Silahla ateş eder; vurduysa true döner.
    fn shoot(&mut self) -> bool {
        let wp = self.weapon as usize;
        if self.ammo_per_weapon[wp] == 0 {
            return false;
        }
        self.shots_fired = self.shots_fired.saturating_add(1);
        self.ammo_per_weapon[wp] = self.ammo_per_weapon[wp].saturating_sub(1);
        self.ammo = self.ammo_per_weapon[wp];

        let pellet_count = if wp == WEAPON_SHOTGUN as usize {
            SHOTGUN_PELLETS
        } else {
            1
        };
        let spread = if wp == WEAPON_SHOTGUN as usize {
            0.12
        } else {
            0.0
        };
        let base_angle = fast_atan2(self.dir_y, self.dir_x);
        let mut any_hit = false;

        for pellet in 0..pellet_count {
            let mut aim = base_angle;
            if pellet_count > 1 {
                let center = (pellet_count - 1) as f32 / 2.0;
                aim += (pellet as f32 - center) * spread;
            }
            let mut best_dist = f32::MAX;
            let mut best_idx: Option<usize> = None;
            for (i, e) in self.enemies.iter().enumerate().take(self.enemy_count) {
                if !e.alive {
                    continue;
                }
                let dx = e.x - self.pos_x;
                let dy = e.y - self.pos_y;
                let dist = fast_sqrt(dx * dx + dy * dy);
                if dist <= 0.0 || !self.has_line_of_sight(e.x, e.y, dist) {
                    continue;
                }
                let angle = fast_atan2(dy, dx);
                let rel = angle_diff(angle, aim);
                // Menzile göre genişleyen nişan toleransı: yakında "otomatik
                // nişan alma" hissi verir, uzakta hassas olmayı korur.
                let tolerance = 0.045 + 0.26 / dist.max(0.5);
                if rel.abs() < tolerance && dist < best_dist {
                    best_dist = dist;
                    best_idx = Some(i);
                }
            }
            if let Some(idx) = best_idx {
                let enemy_angle = fast_atan2(
                    self.enemies[idx].y - self.pos_y,
                    self.enemies[idx].x - self.pos_x,
                );
                let headshot = angle_diff(enemy_angle, base_angle).abs() < 0.03;
                self.enemies[idx].hp = self.enemies[idx].hp.saturating_sub(1);
                self.enemies[idx].hit_timer = 8;
                self.hits = self.hits.saturating_add(1);
                if self.enemies[idx].hp == 0 {
                    self.enemies[idx].alive = false;
                    self.enemies[idx].death_timer = 24;
                    self.score = self.score.saturating_add(100);
                    if headshot {
                        self.score = self.score.saturating_add(50);
                    }
                }
                any_hit = true;
            }
        }
        any_hit
    }

    /// Yeniden doldurmayı başlatır.
    fn start_reload(&mut self) {
        if self.is_reloading {
            return;
        }
        let wp = self.weapon as usize;
        let max_ammo = self.weapon_max_ammo();
        if self.ammo_per_weapon[wp] >= max_ammo {
            return;
        }
        self.is_reloading = true;
        self.reload_timer = RELOAD_TICKS;
    }

    /// Silahın taşıdığı maksimum mermi.
    fn weapon_max_ammo(&self) -> u32 {
        match self.weapon {
            WEAPON_SHOTGUN => SHOTGUN_AMMO_MAX,
            WEAPON_MACHINEGUN => MACHINEGUN_AMMO_MAX,
            _ => PISTOL_AMMO_MAX,
        }
    }

    // -----------------------------------------------------------------------
    // Etkileşim (kapı / itilebilir duvar / gizli kapı)
    // -----------------------------------------------------------------------

    /// Oyuncunun baktığı yöndeki etkileşilebilir hücre.
    fn facing_interactable(&self) -> Option<(usize, usize)> {
        for dist in [0.6f32, 0.9, 1.2] {
            let fx = (self.pos_x + self.dir_x * dist) as usize;
            let fy = (self.pos_y + self.dir_y * dist) as usize;
            if fx < self.level.w && fy < self.level.h {
                let tile = self.level.tile(fx, fy);
                if tile == fps_level::TILE_DOOR
                    || tile == fps_level::TILE_PUSH
                    || tile == fps_level::TILE_SECRET
                {
                    return Some((fx, fy));
                }
            }
        }
        None
    }

    /// "Kullan" eylemi: kapı aç/kapat, gizli kapı aç, itme moduna geç.
    fn use_action(&mut self, audio_dev: &mut AudioDevice) {
        let Some((x, y)) = self.facing_interactable() else {
            return;
        };
        match self.level.tile(x, y) {
            fps_level::TILE_DOOR => {
                self.toggle_door(x, y, audio_dev);
            }
            fps_level::TILE_SECRET => {
                self.level.set_tile(x, y, fps_level::TILE_EMPTY);
                self.play_sound(audio_dev, SoundKind::Door);
            }
            fps_level::TILE_PUSH => {
                self.pushing = true;
                self.play_sound(audio_dev, SoundKind::Click);
            }
            _ => {}
        }
    }

    /// Kapıyı açar/kapatır. Anahtar gerekiyorsa açılmaz.
    fn toggle_door(&mut self, x: usize, y: usize, audio_dev: &mut AudioDevice) {
        for i in 0..self.door_count {
            if self.doors[i].x != x || self.doors[i].y != y {
                continue;
            }
            let key = self.doors[i].key;
            if key != 0 && self.door_open_at(x, y) < 0.5 {
                // Anahtar kontrolü.
                let idx = match key {
                    KEY_BLUE => 0,
                    KEY_YELLOW => 1,
                    KEY_RED => 2,
                    _ => usize::MAX,
                };
                if idx == usize::MAX || !self.keys[idx] {
                    self.play_sound(audio_dev, SoundKind::Locked);
                    return;
                }
            }
            if self.doors[i].open > 0.5 {
                self.doors[i].target = 0.0;
            } else {
                self.doors[i].target = 1.0;
            }
            self.play_sound(audio_dev, SoundKind::Door);
            return;
        }
    }

    /// İtilebilir duvarı bir hücre ileri iter.
    fn try_push_wall(&mut self, audio_dev: &mut AudioDevice) {
        if !self.pushing {
            return;
        }
        // Oyuncunun önündeki itilebilir duvarı bul.
        let fx = (self.pos_x + self.dir_x * 0.7) as usize;
        let fy = (self.pos_y + self.dir_y * 0.7) as usize;
        let Some(idx) = self
            .push_walls
            .iter()
            .position(|p| !p.moving && p.x == fx && p.y == fy)
        else {
            // İtilebilir duvar yoksa itme modundan çık.
            self.pushing = false;
            return;
        };
        // Hedef hücre: duvarın öte tarafındaki hücre (oyuncudan uzak yön).
        let step_x = self.dir_x.signum() as isize;
        let step_y = self.dir_y.signum() as isize;
        let tx = (self.push_walls[idx].x as isize + step_x) as usize;
        let ty = (self.push_walls[idx].y as isize + step_y) as usize;
        if tx >= self.level.w || ty >= self.level.h {
            return;
        }
        // Hedef normal bir duvar olmalı.
        if self.level.tile(tx, ty) != fps_level::TILE_WALL {
            return;
        }
        // Kaydır: eski hücre boşalır, hedef itilebilir duvar olur.
        self.level.set_tile(
            self.push_walls[idx].x,
            self.push_walls[idx].y,
            fps_level::TILE_EMPTY,
        );
        self.level.set_tile(tx, ty, fps_level::TILE_PUSH);
        self.push_walls[idx].from_x = self.push_walls[idx].x;
        self.push_walls[idx].from_y = self.push_walls[idx].y;
        self.push_walls[idx].x = tx;
        self.push_walls[idx].y = ty;
        self.push_walls[idx].to_x = tx;
        self.push_walls[idx].to_y = ty;
        self.push_walls[idx].slide = 0.0;
        self.push_walls[idx].moving = true;
        self.play_sound(audio_dev, SoundKind::Push);
        self.pushing = false;
    }

    // -----------------------------------------------------------------------
    // Eşya toplama
    // -----------------------------------------------------------------------

    fn collect_pickups(&mut self) {
        for i in 0..self.pickup_count {
            if !self.pickups[i].active {
                continue;
            }
            let dx = self.pickups[i].x - self.pos_x;
            let dy = self.pickups[i].y - self.pos_y;
            if dx * dx + dy * dy > 0.36 {
                continue;
            }
            self.pickups[i].active = false;
            let kind = self.pickups[i].kind;
            match kind {
                fps_level::ITEM_HEALTH => {
                    self.health = (self.health + 25).min(100);
                    self.score = self.score.saturating_add(50);
                }
                fps_level::ITEM_ARMOR => {
                    self.armor = (self.armor + 25).min(100);
                    self.score = self.score.saturating_add(50);
                }
                fps_level::ITEM_AMMO => {
                    for w in 0..WEAPON_COUNT {
                        let max = match w {
                            0 => PISTOL_AMMO_MAX,
                            1 => SHOTGUN_AMMO_MAX,
                            _ => MACHINEGUN_AMMO_MAX,
                        };
                        let add = if w == self.weapon as usize { 12 } else { 6 };
                        self.ammo_per_weapon[w] = (self.ammo_per_weapon[w] + add).min(max);
                    }
                    self.ammo = self.ammo_per_weapon[self.weapon as usize];
                    self.score = self.score.saturating_add(25);
                }
                fps_level::ITEM_KEY_BLUE => {
                    self.keys[0] = true;
                    self.score = self.score.saturating_add(100);
                }
                fps_level::ITEM_KEY_YELLOW => {
                    self.keys[1] = true;
                    self.score = self.score.saturating_add(100);
                }
                fps_level::ITEM_KEY_RED => {
                    self.keys[2] = true;
                    self.score = self.score.saturating_add(100);
                }
                fps_level::ITEM_GOLD => {
                    self.score = self.score.saturating_add(500);
                    self.health = (self.health + 10).min(100);
                }
                fps_level::ITEM_SILVER => {
                    self.score = self.score.saturating_add(100);
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Ses
    // -----------------------------------------------------------------------

    /// Ses efektlerini çalar.
    pub fn play_sound(&mut self, audio_dev: &mut AudioDevice, kind: SoundKind) {
        if audio_dev.is_muted || audio_dev.volume_left == 0 {
            return;
        }
        let (freq, dur) = match kind {
            SoundKind::Shoot => (1200, 50),
            SoundKind::Hit => (800, 50),
            SoundKind::Click => (180, 20),
            SoundKind::Reload => (440, 60),
            SoundKind::EnemyDamage => (600, 40),
            SoundKind::Step => (200, 30),
            SoundKind::Door => (620, 90),
            SoundKind::Pickup => (1000, 30),
            SoundKind::Locked => (140, 120),
            SoundKind::Push => (320, 120),
            SoundKind::Alert => (900, 120),
            SoundKind::LevelClear => (1200, 200),
        };
        audio_dev.set_pc_speaker_tone(freq);
        self.sound_pending = Some((freq, dur));
    }

    /// Bekleyen ses isteğini alır ve tüketir.
    pub fn take_sound(&mut self) -> Option<(u32, usize)> {
        self.sound_pending.take()
    }

    /// Ses isteğini işler (geçerliyse döndürür).
    pub fn sound_tone(req: Option<(u32, usize)>) -> Option<(u32, usize)> {
        req.filter(|(f, _)| *f > 0)
    }

    pub fn stop_sound(&mut self, audio_dev: &mut AudioDevice) {
        audio_dev.set_pc_speaker_tone(0);
    }

    // -----------------------------------------------------------------------
    // Görünüm çizimi
    // -----------------------------------------------------------------------

    /// Tamamlanan kareyi performans ölçümlerine kaydeder ve gerekirse iç
    /// çözünürlük ölçeğini ayarlar.
    pub fn record_completed_frame(&mut self, render_ms: u64) {
        const WARMUP_FRAMES: u64 = 30;
        const WINDOW_MS: u64 = 1000;

        let now = crate::monotonic_ms();
        self.frame_time_ms = render_ms;
        if self.last_completed_frame_ms != 0 {
            self.frame_interval_ms = now.saturating_sub(self.last_completed_frame_ms);
        }
        self.last_completed_frame_ms = now;
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        if self.rendered_frames <= WARMUP_FRAMES {
            return;
        }
        if self.metrics_window_start_ms == 0 {
            self.metrics_window_start_ms = now;
            self.metrics_window_frames = 0;
            self.metrics_window_max_interval_ms = 0;
        }
        self.metrics_window_frames = self.metrics_window_frames.saturating_add(1);
        self.metrics_window_max_interval_ms = self
            .metrics_window_max_interval_ms
            .max(self.frame_interval_ms);
        let elapsed = now.saturating_sub(self.metrics_window_start_ms);
        if elapsed < WINDOW_MS {
            return;
        }
        let window_fps = self.metrics_window_frames.max(1).saturating_mul(1000) / elapsed;
        self.fps = window_fps.min(u64::from(u32::MAX)) as usize;
        self.min_fps = if self.min_fps == 0 {
            self.fps
        } else {
            self.min_fps.min(self.fps)
        };

        // Uyarlanabilir iç çözünürlük: kare hızı düşükse ölçek büyür
        // (daha az piksel), yüksekse küçülür (tam çözünürlük hedeflenir).
        if self.fps < 50 && self.render_scale < RENDER_SCALE_MAX {
            self.render_scale += 1;
        } else if self.fps > 58 && self.render_scale > RENDER_SCALE_MIN {
            self.render_scale -= 1;
        }

        crate::boot::console::serial_print("[FPS3D-METRICS] fps=");
        crate::boot::console::serial_print_dec(window_fps);
        crate::boot::console::serial_print(" min_fps=");
        crate::boot::console::serial_print_dec(self.min_fps as u64);
        crate::boot::console::serial_print(" frame_ms=");
        crate::boot::console::serial_print_dec(render_ms);
        crate::boot::console::serial_print(" interval_ms=");
        crate::boot::console::serial_print_dec(self.frame_interval_ms);
        crate::boot::console::serial_print(" max_interval_ms=");
        crate::boot::console::serial_print_dec(self.metrics_window_max_interval_ms);
        crate::boot::console::serial_print(" frames=");
        crate::boot::console::serial_print_dec(self.rendered_frames);
        crate::boot::console::serial_print(" scale=");
        crate::boot::console::serial_print_dec(self.render_scale as u64);
        let stats = crate::mm::memory::mem_stats();
        let used_kb = stats.used_bytes / 1024;
        let free_kb = stats.free_bytes / 1024;
        self.mem_free_kb = free_kb;
        self.mem_heap_kb = stats.heap_committed_bytes / 1024;
        self.mem_peak_used_kb = self.mem_peak_used_kb.max(used_kb);
        crate::boot::console::serial_print(" used_kb=");
        crate::boot::console::serial_print_dec(used_kb);
        crate::boot::console::serial_print(" free_kb=");
        crate::boot::console::serial_print_dec(free_kb);
        crate::boot::console::serial_print(" heap_kb=");
        crate::boot::console::serial_print_dec(self.mem_heap_kb);
        crate::boot::console::serial_print(" peak_used_kb=");
        crate::boot::console::serial_print_dec(self.mem_peak_used_kb);
        crate::boot::console::serial_print("\n");
        self.metrics_window_start_ms = now;
        self.metrics_window_frames = 0;
        self.metrics_window_max_interval_ms = 0;
    }

    /// Yarım kalan karede çözünürlüğü hemen düşürerek sonraki denemeyi hızlandırır.
    pub fn record_incomplete_frame(&mut self) {
        self.render_scale = (self.render_scale + 1).min(RENDER_SCALE_MAX);
    }

    /// 3B sahneyi çizer (raycasting). `budget_ms` ile render süresi sınırlanır.
    pub fn render_3d_with_budget(
        &mut self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        budget_ms: u64,
    ) -> bool {
        let scale = self.render_scale.max(1);
        let iw = win_w.div_ceil(scale).min(ZBUF_MAX);
        let ih = win_h.div_ceil(scale);
        if iw < 8 || ih < 8 {
            return true;
        }

        // Ortak perspektif modeli: zemin (z = 0), tavan (z = WALL_H) ve duvarlar
        // (0 <= z <= WALL_H) tek bir izdüşümde hesaplanır:
        //     ekran_y = ufuk + (göz_yüksekliği - dünya_z) * ölçek / derinlik
        // Ufuk satırı dikey bakışla (pitch) kayar; ölçek ekran yüksekliğidir.
        let horizon_f = self.horizon_y(ih);
        let v_scale = ih as f32;
        // Odak uzaklığı dikey ölçekle aynı: izdüşüm izotropik olur ve 1x1
        // dünya hücreleri ekranda kare görünür.
        let focal = v_scale;

        // Kırpma ile kesişen iç (bölünmüş) koordinat aralıkları.
        let clip = fb.clip();
        let cx0 = clip.x0.max(win_x);
        let cx1 = clip.x1.min(win_x + win_w);
        let cy0 = clip.y0.max(win_y);
        let cy1 = clip.y1.min(win_y + win_h);
        if cx1 <= cx0 || cy1 <= cy0 {
            return true;
        }
        let col_start = (cx0 - win_x) / scale;
        let col_end = (cx1 - win_x).div_ceil(scale).min(iw);
        let row_start = (cy0 - win_y) / scale;
        let row_end = (cy1 - win_y).div_ceil(scale).min(ih);
        if col_end <= col_start || row_end <= row_start {
            return true;
        }

        // Sütun başına ışın dizisi (kare başına bir kez hesaplanır).
        let half_w = iw as f32 * 0.5;
        let inv_focal = 1.0 / focal;
        for x in 0..iw {
            let camera_x = (x as f32 + 0.5 - half_w) * inv_focal;
            self.rays[2 * x] = self.dir_x + self.plane_x * camera_x;
            self.rays[2 * x + 1] = self.dir_y + self.plane_y * camera_x;
        }
        // Duvar ışını henüz çizilmemiş sütunlarda sprite görünürlüğünü koru.
        for slot in self.zbuf[..iw].iter_mut() {
            *slot = MAX_DEPTH * 4.0;
        }

        // Arka plan: ufuk sis rengi (kaplanmayan pikseller için).
        fb.draw_rect(win_x, win_y, win_w, win_h, 0x141A22);

        // A slow plane pass must not starve walls and enemies. Give each
        // render layer its own share so later layers still get drawn when
        // an earlier one is incomplete.
        let __t0 = crate::monotonic_ms();
        let planes_budget_ms = budget_ms / 4;
        let walls_budget_ms = budget_ms / 2;
        let sprites_budget_ms = budget_ms.saturating_sub(planes_budget_ms + walls_budget_ms);
        let planes_complete = self.render_planes(
            fb,
            win_x,
            win_y,
            scale,
            iw,
            horizon_f,
            v_scale,
            col_start,
            col_end,
            row_start,
            row_end,
            &TimeBudget::new(planes_budget_ms),
        );
        let __m1 = crate::monotonic_ms();
        let walls_complete = self.render_walls(
            fb,
            win_x,
            win_y,
            scale,
            ih,
            horizon_f,
            v_scale,
            col_start,
            col_end,
            row_start,
            row_end,
            &TimeBudget::new(walls_budget_ms),
        );
        let __m2 = crate::monotonic_ms();
        let sprites_complete = self.render_sprites(
            fb,
            win_x,
            win_y,
            scale,
            iw,
            ih,
            horizon_f,
            v_scale,
            focal,
            col_start,
            col_end,
            row_start,
            row_end,
            &TimeBudget::new(sprites_budget_ms),
        );
        let __m3 = crate::monotonic_ms();
        let cancelled = !(planes_complete && walls_complete && sprites_complete);

        // FPS3D-PROFILE: asama surelerini seri porta yazar (donma ayiklama).
        let __tot = __m3.saturating_sub(__t0);
        static mut __LAST_PR: u64 = 0;
        if __tot > 60 {
            let __now = crate::monotonic_ms();
            unsafe {
                if __now.wrapping_sub(__LAST_PR) > 1500 {
                    __LAST_PR = __now;
                    crate::boot::console::serial_print("[FPS3D-PROFILE] planes=");
                    crate::boot::console::serial_print_dec(__m1.saturating_sub(__t0));
                    crate::boot::console::serial_print(" walls=");
                    crate::boot::console::serial_print_dec(__m2.saturating_sub(__m1));
                    crate::boot::console::serial_print(" sprites=");
                    crate::boot::console::serial_print_dec(__m3.saturating_sub(__m2));
                    crate::boot::console::serial_print(" total=");
                    crate::boot::console::serial_print_dec(__tot);
                    crate::boot::console::serial_print(" iw=");
                    crate::boot::console::serial_print_dec(iw as u64);
                    crate::boot::console::serial_print(" ih=");
                    crate::boot::console::serial_print_dec(ih as u64);
                    crate::boot::console::serial_print(" scale=");
                    crate::boot::console::serial_print_dec(scale as u64);
                    crate::boot::console::serial_print(" win=");
                    crate::boot::console::serial_print_dec(win_w as u64);
                    crate::boot::console::serial_print("x");
                    crate::boot::console::serial_print_dec(win_h as u64);
                    crate::boot::console::serial_print("\n");
                }
            }
        }

        if !cancelled {
            // Ateş/hasar ekran çerçevesi.
            if self.shooting && self.shoot_timer > 0 {
                let pulse = if self.shoot_timer > 5 {
                    0xFFD34E
                } else {
                    0xA62E2E
                };
                let edge = 3usize;
                fb.draw_rect(win_x, win_y, win_w, edge, pulse);
                fb.draw_rect(
                    win_x,
                    win_y + win_h.saturating_sub(edge),
                    win_w,
                    edge,
                    pulse,
                );
                fb.draw_rect(win_x, win_y, edge, win_h, pulse);
                fb.draw_rect(
                    win_x + win_w.saturating_sub(edge),
                    win_y,
                    edge,
                    win_h,
                    pulse,
                );
            }
        }

        !cancelled
    }

    /// Zemin ve tavanı düzlem atma (plane casting) ile çizer.
    ///
    /// Her satırın derinliği sabittir; dünya konumu `göz + ışın * derinlik`
    /// ile bulunur ve hücrenin zemin/tavan dokusundan örneklenir.
    #[allow(clippy::too_many_arguments)]
    fn render_planes(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        scale: usize,
        iw: usize,
        horizon_f: f32,
        v_scale: f32,
        col_start: usize,
        col_end: usize,
        row_start: usize,
        row_end: usize,
        budget: &TimeBudget,
    ) -> bool {
        const TEX: usize = fps_textures::TEX;
        const TEX_MASK: usize = TEX - 1;
        let horizon = horizon_f as usize;

        // Tavan (ufkun üstü) ve zemin (ufkun altı) ayrı döngülerde çizilir.
        for (plane_z, is_ceiling) in [(WALL_H, true), (0.0f32, false)] {
            let r0 = if is_ceiling {
                row_start
            } else {
                horizon.max(row_start)
            };
            let r1 = if is_ceiling {
                horizon.min(row_end)
            } else {
                row_end
            };
            if r1 <= r0 {
                continue;
            }
            for y in r0..r1 {
                if budget.is_expired() {
                    return false;
                }
                let depth = plane_depth(horizon_f, v_scale, EYE_Z, plane_z, y as f32 + 0.5);
                if !depth.is_finite() || depth <= 0.0 {
                    continue;
                }
                let factor = depth_factor(depth);
                for dy in 0..scale {
                    let fy = win_y + y * scale + dy;
                    let Some(row) = (unsafe { fb.scene_row_mut(fy, win_x, iw * scale) }) else {
                        continue;
                    };
                    for x in col_start..col_end {
                        let rx = self.rays[2 * x];
                        let ry = self.rays[2 * x + 1];
                        let world_x = self.pos_x + rx * depth;
                        let world_y = self.pos_y + ry * depth;
                        let cx = world_x as usize;
                        let cy = world_y as usize;
                        let color = if cx >= self.level.w || cy >= self.level.h {
                            0x1B212A
                        } else {
                            let u = (world_x * TEX as f32) as usize & TEX_MASK;
                            let v = (world_y * TEX as f32) as usize & TEX_MASK;
                            let texel = if is_ceiling {
                                let id = self.level.ceil_tex(cx, cy);
                                self.atlas.ceil(id, u, v)
                            } else {
                                let id = self.level.floor_tex(cx, cy);
                                self.atlas.floor(id, u, v)
                            };
                            shade_color(texel, factor)
                        };
                        let start = x * scale;
                        for dx in 0..scale {
                            row[start + dx] = color;
                        }
                    }
                }
            }
        }
        true
    }

    /// Duvarları DDA ışın izleme ile, dokulu olarak çizer.
    #[allow(clippy::too_many_arguments)]
    fn render_walls(
        &mut self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        scale: usize,
        ih: usize,
        horizon_f: f32,
        v_scale: f32,
        col_start: usize,
        col_end: usize,
        row_start: usize,
        row_end: usize,
        budget: &TimeBudget,
    ) -> bool {
        const TEX: usize = fps_textures::TEX;
        const TEX_MASK: usize = TEX - 1;

        for x in col_start..col_end {
            if budget.is_expired() {
                return false;
            }
            let ray_dir_x = self.rays[2 * x];
            let ray_dir_y = self.rays[2 * x + 1];

            // DDA: duvara kadar ilerle.
            let mut map_x = self.pos_x as usize;
            let mut map_y = self.pos_y as usize;
            let delta_dist_x = (1.0 / ray_dir_x.abs()).max(0.0001);
            let delta_dist_y = (1.0 / ray_dir_y.abs()).max(0.0001);
            let (step_x, mut side_dist_x) = if ray_dir_x < 0.0 {
                (-1isize, (self.pos_x - map_x as f32) * delta_dist_x)
            } else {
                (1isize, (map_x as f32 + 1.0 - self.pos_x) * delta_dist_x)
            };
            let (step_y, mut side_dist_y) = if ray_dir_y < 0.0 {
                (-1isize, (self.pos_y - map_y as f32) * delta_dist_y)
            } else {
                (1isize, (map_y as f32 + 1.0 - self.pos_y) * delta_dist_y)
            };

            let mut hit = false;
            let mut side = 0u8;
            let mut ray_steps = 0usize;
            while !hit && ray_steps < self.level.w + self.level.h + 4 {
                if budget.is_expired() {
                    return false;
                }
                ray_steps += 1;
                if side_dist_x < side_dist_y {
                    side_dist_x += delta_dist_x;
                    map_x = (map_x as isize + step_x) as usize;
                    side = 0;
                } else {
                    side_dist_y += delta_dist_y;
                    map_y = (map_y as isize + step_y) as usize;
                    side = 1;
                }
                if map_x >= self.level.w || map_y >= self.level.h {
                    hit = true;
                } else {
                    let tile = self.level.tile(map_x, map_y);
                    let blocked = match tile {
                        fps_level::TILE_WALL | fps_level::TILE_PUSH | fps_level::TILE_SECRET => {
                            true
                        }
                        fps_level::TILE_DOOR => self.door_open_at(map_x, map_y) < 0.98,
                        _ => false,
                    };
                    if blocked {
                        hit = true;
                    }
                }
            }

            // Duvarın gerçek (dik) mesafesi — "fisheye" düzeltmesi.
            let perp_wall_dist: f32 = if side == 0 {
                (map_x as f32 - self.pos_x + (1.0 - step_x as f32) / 2.0) / ray_dir_x
            } else {
                (map_y as f32 - self.pos_y + (1.0 - step_y as f32) / 2.0) / ray_dir_y
            };
            let perp_wall_dist = if perp_wall_dist.is_finite() {
                perp_wall_dist.abs().clamp(0.12, MAX_DEPTH * 4.0)
            } else {
                MAX_DEPTH
            };
            self.zbuf[x] = perp_wall_dist;

            // Duvarın dikey sınırları ortak perspektif modelinden gelir.
            let y_bottom = screen_y(horizon_f, v_scale, EYE_Z, 0.0, perp_wall_dist);
            let y_top = screen_y(horizon_f, v_scale, EYE_Z, WALL_H, perp_wall_dist);
            let draw_start = (y_top.max(0.0) as usize).max(row_start).min(ih);
            let draw_end = (y_bottom.max(0.0) as usize).min(row_end).min(ih);
            if draw_end <= draw_start {
                continue;
            }

            let in_bounds = map_x < self.level.w && map_y < self.level.h;
            let tile = if in_bounds {
                self.level.tile(map_x, map_y)
            } else {
                fps_level::TILE_WALL
            };
            let tex_id = if in_bounds {
                self.level.wall_tex(map_x, map_y)
            } else {
                fps_textures::WALL_BRICK
            };

            // Doku u koordinatı: duvara çarpan noktanın kesirli kısmı.
            let tex_x_raw = if side == 0 {
                perp_wall_dist * ray_dir_y
            } else {
                perp_wall_dist * ray_dir_x
            };
            let tex_u = ((tex_x_raw - fast_floor(tex_x_raw)) * TEX as f32) as usize & TEX_MASK;

            let mut factor = depth_factor(perp_wall_dist);
            if side == 1 {
                // Yan yüzeyler biraz daha koyu çizilir (klasik raycasting gölgesi).
                factor = (factor >> 1) + (factor >> 2);
            }

            // Kapı açılma animasyonu: kapı duvara doğru kayar.
            let door_open = if tile == fps_level::TILE_DOOR {
                self.door_open_at(map_x, map_y)
            } else {
                0.0
            };
            let shift = (door_open * TEX as f32) as usize;
            let hidden = shift > 0 && tex_u < shift;
            let sample_u = if shift > 0 {
                (tex_u + TEX - shift) & TEX_MASK
            } else {
                tex_u
            };

            // Doku satırının başlangıç değeri (ekran y'sine bağlı).
            let world_z_start = EYE_Z + (horizon_f - draw_start as f32) * perp_wall_dist / v_scale;
            let mut tex_v = (((1.0 - world_z_start / WALL_H) * TEX as f32) as usize) & TEX_MASK;

            let fb_x = win_x + x * scale;
            for y in draw_start..draw_end {
                if y & 15 == 0 && budget.is_expired() {
                    return false;
                }
                let color = if hidden {
                    0x14181E
                } else {
                    self.atlas.wall(tex_id, sample_u, tex_v)
                };
                let shaded = shade_color(color, factor);
                let fy = win_y + y * scale;
                for dy in 0..scale {
                    unsafe {
                        fb.fill_scene_span(fb_x, fy + dy, scale, shaded);
                    }
                }
                tex_v = (tex_v + 1) & TEX_MASK;
            }
        }
        true
    }

    /// Düşman ve eşya sprite'larını çizer (z-buffer ile kırpılır).
    #[allow(clippy::too_many_arguments)]
    fn render_sprites(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        scale: usize,
        iw: usize,
        ih: usize,
        horizon_f: f32,
        v_scale: f32,
        focal: f32,
        col_start: usize,
        col_end: usize,
        row_start: usize,
        row_end: usize,
        budget: &TimeBudget,
    ) -> bool {
        // Sprite'lar uzaktan yakına sıralanır (arkadan öne doğru çizilir).
        let mut order: [(f32, u8); MAX_ENEMIES + MAX_PICKUPS] =
            [(0.0, 0); MAX_ENEMIES + MAX_PICKUPS];
        let mut count = 0usize;
        for (i, e) in self.enemies.iter().enumerate().take(self.enemy_count) {
            if !e.alive && e.death_timer == 0 {
                continue;
            }
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            if count < order.len() {
                order[count] = (dx * dx + dy * dy, i as u8);
                count += 1;
            }
        }
        order[..count].sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(core::cmp::Ordering::Equal));

        for entry in order[..count].iter() {
            if budget.is_expired() {
                return false;
            }
            let e = self.enemies[entry.1 as usize];
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            if dist < 0.05 || dist > MAX_DEPTH {
                continue;
            }

            // Sprite'ın dünya ölçüleri.
            let (sprite_h, sprite_w, bottom_z) = if e.alive {
                (1.30f32, 0.85f32, 0.0f32)
            } else {
                (0.70f32, 1.00f32, 0.0f32)
            };

            // Kameraya göre dik derinlik (Wolf3D dönüşümü).
            let inv_det = 1.0 / (self.plane_x * self.dir_y - self.dir_x * self.plane_y);
            let transform_x = inv_det * (self.dir_y * dx - self.dir_x * dy);
            let transform_y = inv_det * (-self.plane_y * dx + self.plane_x * dy);
            if transform_y <= 0.05 {
                continue;
            }
            let depth = transform_y;
            let factor = depth_factor(depth);
            let screen_x = (iw as f32 * 0.5) * (1.0 + transform_x / transform_y);
            let half_w = sprite_w * 0.5;
            let sprite_px_w = half_w * focal / depth * 2.0;
            let start_x = fast_floor(screen_x - sprite_px_w * 0.5) as isize;
            let end_x = fast_ceil(screen_x + sprite_px_w * 0.5) as isize;

            let y_top = screen_y(horizon_f, v_scale, EYE_Z, bottom_z + sprite_h, depth);
            let y_bottom = screen_y(horizon_f, v_scale, EYE_Z, bottom_z, depth);
            let draw_start = (y_top.max(0.0) as usize).max(row_start).min(ih);
            let draw_end = (y_bottom.max(0.0) as usize).min(row_end).min(ih);
            if draw_end <= draw_start {
                continue;
            }
            let sprite_h_px = (y_bottom - y_top).max(1.0);

            // Düşman açısı: gövdenin oyuncuya göre yönü.
            let angle = if e.alive {
                let to_player = fast_atan2(self.pos_y - e.y, self.pos_x - e.x);
                let rel = angle_diff(e.facing, to_player).abs();
                if rel < 1.05 {
                    0u8
                } else if rel > 2.1 {
                    2u8
                } else {
                    1u8
                }
            } else {
                0
            };
            // Ölüm animasyonu karesi.
            let death_frame = if e.alive {
                0
            } else {
                match e.death_timer {
                    17..=24 => 0u8,
                    9..=16 => 1u8,
                    _ => 2u8,
                }
            };

            let lo = start_x.max(col_start as isize);
            let hi = end_x.min(col_end as isize);
            for sx in lo..hi {
                if (sx & 3) == 0 && budget.is_expired() {
                    return false;
                }
                let sx_u = sx as usize;
                if sx_u >= self.zbuf.len() {
                    break;
                }
                if self.zbuf[sx_u] < depth {
                    continue; // duvar arkasında kalıyor
                }
                // Doku u koordinatı.
                let lateral = (sx as f32 + 0.5 - screen_x) * depth / focal;
                let u = ((lateral + half_w) / sprite_w * fps_sprites::SPR_W as f32) as usize;
                if u >= fps_sprites::SPR_W {
                    continue;
                }
                let fb_x = win_x + sx_u * scale;
                for y in draw_start..draw_end {
                    if (y & 15) == 0 && budget.is_expired() {
                        return false;
                    }
                    let v = (((y as f32 + 0.5 - y_top) / sprite_h_px) * fps_sprites::SPR_H as f32)
                        as usize;
                    if v >= fps_sprites::SPR_H {
                        continue;
                    }
                    let color = if e.alive {
                        self.sprites.enemy(e.kind, angle, u, v)
                    } else {
                        self.sprites.death(e.kind, death_frame, u, v)
                    };
                    if color & fps_sprites::ALPHA != 0 {
                        continue;
                    }
                    let mut shaded = shade_color(color & 0x00FF_FFFF, factor);
                    if e.hit_timer > 0 {
                        shaded = 0xFF4040;
                    }
                    let fy = win_y + y * scale;
                    for dy in 0..scale {
                        unsafe {
                            fb.fill_scene_span(fb_x, fy + dy, scale, shaded);
                        }
                    }
                }
            }
        }

        // Eşyalar.
        for p in self.pickups.iter().take(self.pickup_count) {
            if !p.active {
                continue;
            }
            if budget.is_expired() {
                return false;
            }
            let dx = p.x - self.pos_x;
            let dy = p.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            if dist < 0.05 || dist > MAX_DEPTH {
                continue;
            }
            let inv_det = 1.0 / (self.plane_x * self.dir_y - self.dir_x * self.plane_y);
            let transform_x = inv_det * (self.dir_y * dx - self.dir_x * dy);
            let transform_y = inv_det * (-self.plane_y * dx + self.plane_x * dy);
            if transform_y <= 0.05 {
                continue;
            }
            let depth = transform_y;
            let factor = depth_factor(depth);
            let screen_x = (iw as f32 * 0.5) * (1.0 + transform_x / transform_y);
            let sprite_w = 0.42f32;
            let sprite_h = 0.42f32;
            let bob = fast_sin(p.bob) * 0.05;
            let y_top = screen_y(horizon_f, v_scale, EYE_Z, sprite_h + bob, depth);
            let y_bottom = screen_y(horizon_f, v_scale, EYE_Z, bob, depth);
            let sprite_px_w = sprite_w * focal / depth;
            let start_x = fast_floor(screen_x - sprite_px_w * 0.5) as isize;
            let end_x = fast_ceil(screen_x + sprite_px_w * 0.5) as isize;
            let draw_start = (y_top.max(0.0) as usize).max(row_start).min(ih);
            let draw_end = (y_bottom.max(0.0) as usize).min(row_end).min(ih);
            if draw_end <= draw_start {
                continue;
            }
            let sprite_h_px = (y_bottom - y_top).max(1.0);
            let lo = start_x.max(col_start as isize);
            let hi = end_x.min(col_end as isize);
            for sx in lo..hi {
                if (sx & 3) == 0 && budget.is_expired() {
                    return false;
                }
                let sx_u = sx as usize;
                if sx_u >= self.zbuf.len() {
                    break;
                }
                if self.zbuf[sx_u] < depth {
                    continue;
                }
                let lateral = (sx as f32 + 0.5 - screen_x) * depth / focal;
                let u =
                    ((lateral + sprite_w * 0.5) / sprite_w * fps_sprites::ITEM_W as f32) as usize;
                if u >= fps_sprites::ITEM_W {
                    continue;
                }
                let fb_x = win_x + sx_u * scale;
                for y in draw_start..draw_end {
                    if (y & 15) == 0 && budget.is_expired() {
                        return false;
                    }
                    let v = (((y as f32 + 0.5 - y_top) / sprite_h_px) * fps_sprites::ITEM_H as f32)
                        as usize;
                    if v >= fps_sprites::ITEM_H {
                        continue;
                    }
                    let color = self.sprites.item(p.kind, u, v);
                    if color & fps_sprites::ALPHA != 0 {
                        continue;
                    }
                    let shaded = shade_color(color & 0x00FF_FFFF, factor);
                    let fy = win_y + y * scale;
                    for dy in 0..scale {
                        unsafe {
                            fb.fill_scene_span(fb_x, fy + dy, scale, shaded);
                        }
                    }
                }
            }
        }
        true
    }

    /// Silahı (elde tutulan görsel) çizer.
    fn render_weapon(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        view_y: usize,
        win_w: usize,
        view_h: usize,
    ) {
        let bob_x = (fast_sin(self.weapon_bob) * 6.0) as isize;
        let bob_y = (fast_cos(self.weapon_bob * 2.0) * 4.0) as isize;
        let recoil = if self.shooting && self.shoot_timer > 0 {
            ((12 - self.shoot_timer.min(12)) as isize) * 2
        } else {
            0
        };

        // Silah sprite'ı görünüm alanının alt kısmına yerleştirilir.
        let target_h = (view_h as f32 * 0.42) as usize;
        let target_h = target_h.clamp(40, 240);
        let spr_w = fps_sprites::WEAPON_W;
        let spr_h = fps_sprites::WEAPON_H;
        let draw_h = target_h;
        let draw_w = (draw_h * spr_w) / spr_h;
        let fx = win_x + (win_w.saturating_sub(draw_w)) / 2 + bob_x as usize;
        let fy = view_y + view_h.saturating_sub(draw_h) + (bob_y + recoil) as usize;

        let sx_step = spr_w / draw_w.max(1);
        let sy_step = spr_h / draw_h.max(1);

        for dy in 0..draw_h {
            let v = (dy * sy_step).min(spr_h - 1);
            let fyy = fy + dy;
            for dx in 0..draw_w {
                let u = (dx * sx_step).min(spr_w - 1);
                let color = self.sprites.weapon(self.weapon, u, v);
                if color & fps_sprites::ALPHA != 0 {
                    continue;
                }
                unsafe {
                    fb.put_scene_pixel_raw(fx + dx, fyy, color & 0x00FF_FFFF);
                }
            }
        }

        // Namlu alevi.
        if self.muzzle_flash_timer > 0 {
            let mf = self.muzzle_flash_timer as f32 / 4.0;
            let size = ((draw_h as f32) * 0.28 * mf) as usize;
            let cx = fx + draw_w / 2;
            let cy = fy + (draw_h / 6).saturating_sub(size / 2);
            fb.draw_rect(
                cx.saturating_sub(size / 2),
                cy.saturating_sub(size / 2),
                size,
                size,
                0xFFF4A3,
            );
            fb.draw_rect(
                cx.saturating_sub(size / 4),
                cy.saturating_sub(size / 4),
                size / 2,
                size / 2,
                0xFFFFFF,
            );
        }

        // Mermi yok işareti.
        if self.ammo_per_weapon[self.weapon as usize] == 0 && !self.is_reloading {
            fb.draw_rect(
                fx + draw_w.saturating_sub(22),
                fy + 8,
                12,
                12,
                Color::ACCENT_RED,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Durum çubuğu
    // -----------------------------------------------------------------------

    /// Kurt 3D tarzı durum çubuğu: harita, can, zırh, anahtar, mermi.
    pub fn render_hud(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let bar_h = STATUS_BAR_H.min(win_h / 3);
        let bar_y = win_y + win_h.saturating_sub(bar_h);
        fb.draw_rect(win_x, bar_y, win_w, bar_h, 0x2A2F38);
        fb.draw_rect(win_x, bar_y, win_w, 1, 0x5A6472);

        // Sol: mini harita.
        let map_size = bar_h.saturating_sub(8).min(96);
        if map_size >= 16 {
            self.render_minimap(fb, win_x + 6, bar_y + 4, map_size);
        }

        // Sağ: sayaçlar.
        let mut x = win_x + 6 + map_size + 14;

        // Can çubuğu.
        let hp = self.health.min(100);
        fb.draw_rect(x, bar_y + 8, 90, 10, 0x1A1E24);
        fb.draw_rect(x, bar_y + 8, (90 * hp as usize) / 100, 10, 0xB03A2E);
        let mut hp_text = crate::gui::TextBuf::new();
        hp_text.push_str("CAN ");
        hp_text.push_u2(hp);
        MinGuiCompositor::draw_string(fb, x, bar_y + 24, hp_text.as_str(), Color::TEXT_MAIN);
        x += 96;

        // Zırh çubuğu.
        let ar = self.armor.min(100);
        fb.draw_rect(x, bar_y + 8, 90, 10, 0x1A1E24);
        fb.draw_rect(x, bar_y + 8, (90 * ar as usize) / 100, 10, 0x3B6EA5);
        let mut ar_text = crate::gui::TextBuf::new();
        ar_text.push_str("ZIRH ");
        ar_text.push_u2(ar);
        MinGuiCompositor::draw_string(fb, x, bar_y + 24, ar_text.as_str(), Color::TEXT_MAIN);
        x += 96;

        // Anahtarlar.
        for (i, key) in self.keys.iter().enumerate() {
            let color = match i {
                0 => 0x4C8FE0,
                1 => 0xE0C040,
                _ => 0xD04040,
            };
            fb.draw_rect(x, bar_y + 8, 12, 12, if *key { color } else { 0x2A2F38 });
            if *key {
                fb.draw_rect(x + 4, bar_y + 14, 4, 4, color);
            }
            x += 16;
        }
        x += 8;

        // Mermi (büyük rakam).
        let ammo = self.ammo_per_weapon[self.weapon as usize];
        let mut ammo_text = crate::gui::TextBuf::new();
        ammo_text.push_usize(ammo as usize);
        MinGuiCompositor::draw_string(fb, x, bar_y + 6, ammo_text.as_str(), Color::ACCENT_YELLOW);
        let mut wp_text = crate::gui::TextBuf::new();
        wp_text.push_str(match self.weapon {
            WEAPON_SHOTGUN => "POMPOLI",
            WEAPON_MACHINEGUN => "MITRALYOZ",
            _ => "TABANCA",
        });
        MinGuiCompositor::draw_string(fb, x, bar_y + 26, wp_text.as_str(), Color::TEXT_MUTED);
        x += 110;

        // Puan ve seviye.
        let mut sc = crate::gui::TextBuf::new();
        sc.push_str("PUAN ");
        sc.push_usize(self.score as usize);
        MinGuiCompositor::draw_string(fb, x, bar_y + 6, sc.as_str(), Color::TEXT_MAIN);
        let mut lv = crate::gui::TextBuf::new();
        lv.push_str("BOLUM ");
        lv.push_usize(self.level_index + 1);
        lv.push_str("/");
        lv.push_usize(fps_level::LEVEL_COUNT);
        MinGuiCompositor::draw_string(fb, x, bar_y + 26, lv.as_str(), Color::ACCENT_PINK);

        // Performans bilgisi (sağ alt).
        let mut fps_text = crate::gui::TextBuf::new();
        fps_text.push_str("FPS ");
        fps_text.push_usize(self.fps);
        if self.render_scale > 1 {
            fps_text.push_str(" x");
            fps_text.push_usize(self.render_scale);
        }
        MinGuiCompositor::draw_string(
            fb,
            win_x + win_w.saturating_sub(96),
            bar_y + 26,
            fps_text.as_str(),
            Color::TEXT_MUTED,
        );
    }

    /// Durum çubuğundaki mini harita.
    fn render_minimap(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let lw = self.level.w;
        let lh = self.level.h;
        let cell = (size / lw.max(lh)).max(1);
        let map_w = cell * lw;
        let map_h = cell * lh;
        let ox = x;
        let oy = y;
        fb.draw_rect(ox, oy, map_w.min(size), map_h.min(size), 0x14181E);
        for ty in 0..lh {
            for tx in 0..lw {
                let tile = self.level.tile(tx, ty);
                let color = match tile {
                    fps_level::TILE_WALL => 0x555F6B,
                    fps_level::TILE_PUSH => 0x6A5A3A,
                    fps_level::TILE_SECRET => 0x4A5360,
                    fps_level::TILE_DOOR => {
                        if self.door_open_at(tx, ty) > 0.5 {
                            0x3E7A4A
                        } else {
                            0xC8A030
                        }
                    }
                    fps_level::TILE_EXIT => 0x60C0E0,
                    _ => 0x1E242C,
                };
                fb.draw_rect(ox + tx * cell, oy + ty * cell, cell, cell, color);
            }
        }
        // Oyuncu oku.
        let px = ox + (self.pos_x as usize) * cell;
        let py = oy + (self.pos_y as usize) * cell;
        let dir_angle = fast_atan2(self.dir_y, self.dir_x);
        for i in 0..3 {
            let a = dir_angle + (i as f32 - 1.0) * 0.5;
            let sx = px as i32 + (fast_sin(a) * 3.0) as i32;
            let sy = py as i32 - (fast_cos(a) * 3.0) as i32;
            if sx >= 0 && sy >= 0 {
                fb.draw_rect(sx as usize, sy as usize, 1, 1, 0x60FF90);
            }
        }
    }

    /// Tam ekran otomatik harita (Tab).
    fn render_automap(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        fb.draw_rect(win_x, win_y, win_w, win_h, 0x0C1016);
        let lw = self.level.w;
        let lh = self.level.h;
        let cell = (win_w / lw).min(win_h / lh).max(1);
        let map_w = cell * lw;
        let map_h = cell * lh;
        let ox = win_x + (win_w.saturating_sub(map_w)) / 2;
        let oy = win_y + (win_h.saturating_sub(map_h)) / 2;
        for ty in 0..lh {
            for tx in 0..lw {
                let tile = self.level.tile(tx, ty);
                let color = match tile {
                    fps_level::TILE_WALL => 0x4A545F,
                    fps_level::TILE_PUSH => 0x8A7030,
                    fps_level::TILE_SECRET => 0x6A7480,
                    fps_level::TILE_DOOR => 0xC8A030,
                    fps_level::TILE_EXIT => 0x60C0E0,
                    _ => 0x161B22,
                };
                fb.draw_rect(ox + tx * cell, oy + ty * cell, cell, cell, color);
            }
        }
        // Eşyalar ve düşmanlar.
        for p in self.pickups.iter().take(self.pickup_count) {
            if !p.active {
                continue;
            }
            fb.draw_rect(
                ox + (p.x as usize) * cell,
                oy + (p.y as usize) * cell,
                cell.max(1),
                cell.max(1),
                0x50E070,
            );
        }
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            fb.draw_rect(
                ox + (e.x as usize) * cell,
                oy + (e.y as usize) * cell,
                cell.max(1),
                cell.max(1),
                0xE05050,
            );
        }
        let px = ox + (self.pos_x as usize) * cell;
        let py = oy + (self.pos_y as usize) * cell;
        let dir_angle = fast_atan2(self.dir_y, self.dir_x);
        for i in 0..5 {
            let a = dir_angle + (i as f32 - 2.0) * 0.35;
            let sx = px as i32 + (fast_sin(a) * 6.0) as i32;
            let sy = py as i32 - (fast_cos(a) * 6.0) as i32;
            if sx >= 0 && sy >= 0 {
                fb.draw_rect(sx as usize, sy as usize, 2, 2, 0x60FF90);
            }
        }
    }

    /// Eski uyumlu radar (artık durum çubuğundaki mini harita).
    pub fn render_radar(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize) {
        let size = 96.min(win_w / 4);
        self.render_minimap(fb, win_x + 8, win_y + 8, size);
    }

    /// 2D üstten bakış sahnesini çizer.
    pub fn render_2d_topdown(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let cell = (win_w / self.level.w).min(win_h / self.level.h).max(1);
        let map_w = cell * self.level.w;
        let map_h = cell * self.level.h;
        let offset_x = win_x + (win_w.saturating_sub(map_w)) / 2;
        let offset_y = win_y + (win_h.saturating_sub(map_h)) / 2;

        for ty in 0..self.level.h {
            for tx in 0..self.level.w {
                let tile = self.level.tile(tx, ty);
                let color = match tile {
                    fps_level::TILE_WALL => 0x6B7A8C,
                    fps_level::TILE_DOOR => {
                        if self.door_open_at(tx, ty) > 0.5 {
                            0x45C766
                        } else {
                            0xE0A92E
                        }
                    }
                    fps_level::TILE_PUSH => 0xC09040,
                    fps_level::TILE_EXIT => 0x60C0E0,
                    _ => 0x151A22,
                };
                fb.draw_rect(
                    offset_x + tx * cell,
                    offset_y + ty * cell,
                    cell,
                    cell,
                    color,
                );
            }
        }
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            fb.draw_rect(
                offset_x + (e.x as usize) * cell,
                offset_y + (e.y as usize) * cell,
                (cell / 2).max(2),
                (cell / 2).max(2),
                0xFF4040,
            );
        }
        let px = offset_x + (self.pos_x as usize) * cell;
        let py = offset_y + (self.pos_y as usize) * cell;
        fb.draw_rect(px, py, cell.max(2), cell.max(2), 0x60FF90);
    }

    // -----------------------------------------------------------------------
    // Ana çizim girişi
    // -----------------------------------------------------------------------

    /// İçerik çizimini zaman bütçesiyle çalıştırır.
    pub fn render_content_with_budget(
        &mut self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        render_ms: u64,
    ) -> bool {
        let budget = TimeBudget::new(render_ms);

        // Üst araç çubuğu.
        let mode_label = match self.render_mode {
            RenderMode::Mode3D => crate::gui::ui_text("FPS 3D — Kurt tarzı", "FPS 3D — Kurt style"),
            RenderMode::TopDown2D => crate::gui::ui_text("FPS 2D Üstten", "FPS 2D Top-Down"),
        };
        fb.draw_rect(
            win_x + 4,
            win_y + 4,
            win_w - 8,
            TOOLBAR_H - 4,
            Color::TITLE_BAR,
        );
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 8, mode_label, Color::TEXT_MAIN);

        // Seviye adı ve kontrol ipucu.
        let mut info = crate::gui::TextBuf::new();
        info.push_str("SEVIYE: ");
        info.push_str(fps_level::LEVEL_NAMES[self.level_index]);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 260,
            win_y + 8,
            info.as_str(),
            Color::ACCENT_GREEN,
        );

        if self.result == GameResult::Playing {
            let hint = crate::gui::ui_text(
                "Sol tık: ateş  |  E: kullan  |  1-3: silah  |  Tab: harita  |  Esc: menü",
                "LMB: fire  |  E: use  |  1-3: weapon  |  Tab: map  |  Esc: menu",
            );
            let tw = MinGuiCompositor::text_width(hint, 1);
            MinGuiCompositor::draw_string(
                fb,
                win_x + win_w.saturating_sub(tw + 12),
                win_y + 8,
                hint,
                Color::TEXT_MUTED,
            );
        }

        // Sahne alanı: araç çubuğu ile durum çubuğu arası.
        let view_y = win_y + TOOLBAR_H;
        let bar_h = STATUS_BAR_H.min(win_h / 3);
        let view_h = win_h.saturating_sub(TOOLBAR_H + bar_h);
        if view_h < 8 || win_w < 8 {
            return true;
        }

        if self.result == GameResult::Menu {
            fb.draw_rect(win_x + 2, view_y, win_w.saturating_sub(4), view_h, 0x101722);
            self.render_menu(fb, win_x, view_y, view_h, win_w);
            return true;
        }

        let mut completed = true;
        match self.render_mode {
            RenderMode::Mode3D => {
                if budget.is_expired() {
                    completed = false;
                } else {
                    completed = self.render_3d_with_budget(
                        fb,
                        win_x,
                        view_y,
                        win_w,
                        view_h,
                        budget.remaining_ms(),
                    );
                }
                // Nişangâh.
                let cx = win_x + win_w / 2;
                let cy = view_y + view_h / 2;
                fb.draw_rect(cx.saturating_sub(1), cy.saturating_sub(8), 2, 5, 0xD8F4FF);
                fb.draw_rect(cx.saturating_sub(1), cy + 3, 2, 5, 0xD8F4FF);
                fb.draw_rect(cx.saturating_sub(8), cy.saturating_sub(1), 5, 2, 0xD8F4FF);
                fb.draw_rect(cx + 3, cy.saturating_sub(1), 5, 2, 0xD8F4FF);
                fb.draw_rect(cx, cy, 1, 1, 0xFFFFFF);

                // Elde tutulan silah.
                self.render_weapon(fb, win_x, view_y, win_w, view_h);

                // Hasar efekti.
                if self.damage_flash_timer > 0 {
                    fb.draw_rect(win_x + 2, view_y + 2, win_w - 4, 4, Color::ACCENT_RED);
                    fb.draw_rect(
                        win_x + 2,
                        view_y + view_h.saturating_sub(6),
                        win_w - 4,
                        4,
                        Color::ACCENT_RED,
                    );
                }
            }
            RenderMode::TopDown2D => {
                if budget.is_expired() {
                    completed = false;
                } else {
                    self.render_2d_topdown(fb, win_x, view_y, win_w, view_h);
                }
            }
        }

        // Durum çubuğu.
        if self.result == GameResult::Playing {
            self.render_hud(fb, win_x, win_y, win_w, win_h);
        }

        // Otomatik harita (Tab) tam ekran kaplar.
        if self.hud_view == HudView::Automap && self.result == GameResult::Playing {
            self.render_automap(fb, win_x, view_y, win_w, view_h);
        }

        // Seviye tamamlandı / oyun sonu mesajları.
        if self.level_clear_timer > 0 {
            let msg = crate::gui::ui_text("BOLUM TAMAMLANDI!", "LEVEL COMPLETE!");
            let tw = MinGuiCompositor::text_width(msg, 1);
            MinGuiCompositor::draw_string(
                fb,
                win_x + (win_w.saturating_sub(tw)) / 2,
                view_y + view_h / 3,
                msg,
                Color::ACCENT_GREEN,
            );
        }
        if completed && self.result != GameResult::Playing {
            self.render_menu(fb, win_x, view_y, view_h, win_w);
        }

        completed
    }

    /// Bütçesiz çizim (geriye uyum).
    pub fn render_content(
        &mut self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let _ = self.render_content_with_budget(fb, win_x, win_y, win_w, win_h, u64::MAX);
    }

    /// Bütçesiz 3B render (geriye uyum).
    pub fn render_3d(
        &mut self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let _ = self.render_3d_with_budget(fb, win_x, win_y, win_w, win_h, u64::MAX);
    }

    /// Menü / oyun sonu ekranı.
    fn render_menu(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        view_y: usize,
        view_h: usize,
        win_w: usize,
    ) {
        let is_menu = self.result == GameResult::Menu;
        let is_won = self.result == GameResult::Won;
        let accent = if is_won {
            Color::ACCENT_GREEN
        } else {
            Color::ACCENT_RED
        };
        fb.draw_rect(win_x, view_y, win_w, view_h, 0x0B1118);

        let horizon = view_y + view_h * 2 / 3;
        for line in 0..6 {
            let y = horizon + line * 10;
            if y < view_y + view_h {
                fb.draw_rect(win_x + 12, y, win_w.saturating_sub(24), 1, 0x18212D);
            }
        }

        let card_w = win_w.saturating_sub(32).min(620);
        let card_h = view_h.saturating_sub(24).min(370);
        let compact_height = card_h < 340;
        let card_x = win_x + win_w.saturating_sub(card_w) / 2;
        let card_y = view_y + view_h.saturating_sub(card_h) / 2;
        fb.draw_rounded_rect(card_x, card_y, card_w, card_h, 10, 0x1A2430);
        fb.draw_rounded_rect_outline(card_x, card_y, card_w, card_h, 10, 0x3B4A5C);
        fb.draw_rect(card_x + 1, card_y + 1, card_w.saturating_sub(2), 3, accent);

        let brand = crate::gui::ui_text("MINOS // FPS GOREVI", "MINOS // FPS MISSION");
        MinGuiCompositor::draw_string(fb, card_x + 22, card_y + 19, brand, Color::ACCENT_BLUE);

        let title = match self.result {
            GameResult::Menu => "Kurt 3D",
            GameResult::Won => "ZAFER!",
            GameResult::Lost => "OLDUNUZ",
            GameResult::Playing => "",
        };
        let title = crate::gui::ui_text(title, title);
        let title_w = MinGuiCompositor::text_width(title, 2);
        MinGuiCompositor::draw_string_scaled(
            fb,
            card_x + card_w.saturating_sub(title_w) / 2,
            card_y + 43,
            title,
            accent,
            2,
        );

        let subtitle = match self.result {
            GameResult::Menu => {
                crate::gui::ui_text("KARANLIKTA YOLUNU BUL", "FIND YOUR WAY THROUGH THE DARK")
            }
            GameResult::Won => crate::gui::ui_text("BOLUM TAMAMLANDI", "MISSION COMPLETE"),
            GameResult::Lost => crate::gui::ui_text("GOREV BASARISIZ", "MISSION FAILED"),
            GameResult::Playing => "",
        };
        let subtitle_w = MinGuiCompositor::text_width(subtitle, 1);
        MinGuiCompositor::draw_string(
            fb,
            card_x + card_w.saturating_sub(subtitle_w) / 2,
            card_y + 79,
            subtitle,
            Color::TEXT_MUTED,
        );

        let button_w = card_w.saturating_sub(48).min(240);
        let button_h = 38;
        let button_x = card_x + card_w.saturating_sub(button_w) / 2;
        let button_y = card_y + if compact_height { 92 } else { 104 };
        fb.draw_rounded_rect(button_x, button_y, button_w, button_h, 7, 0x50302F);
        fb.draw_rounded_rect_outline(button_x, button_y, button_w, button_h, 7, accent);
        let action = if is_menu {
            crate::gui::ui_text("OYUNA BASLA", "START GAME")
        } else {
            crate::gui::ui_text("TEKRAR OYNA", "PLAY AGAIN")
        };
        let action_w = MinGuiCompositor::text_width(action, 1);
        MinGuiCompositor::draw_string(
            fb,
            button_x + button_w.saturating_sub(action_w) / 2,
            button_y + 10,
            action,
            Color::TEXT_MAIN,
        );

        if !is_menu {
            let mut score = crate::gui::TextBuf::new();
            score.push_str(crate::gui::ui_text("PUAN  ", "SCORE  "));
            score.push_usize(self.score as usize);
            let score_w = MinGuiCompositor::text_width(score.as_str(), 1);
            MinGuiCompositor::draw_string(
                fb,
                card_x + card_w.saturating_sub(score_w) / 2,
                button_y + button_h + 5,
                score.as_str(),
                Color::ACCENT_YELLOW,
            );
        } else {
            let start_hint = crate::gui::ui_text(
                "BASLATMAK ICIN ENTER / BOSLUK / FARE",
                "PRESS ENTER / SPACE / CLICK TO START",
            );
            let hint_w = MinGuiCompositor::text_width(start_hint, 1);
            MinGuiCompositor::draw_string(
                fb,
                card_x + card_w.saturating_sub(hint_w) / 2,
                button_y + button_h + 5,
                start_hint,
                Color::TEXT_MUTED,
            );
        }

        let controls_y = if compact_height {
            (button_y + button_h + 22).max(card_y + card_h.saturating_sub(115))
        } else {
            card_y + 174
        };
        fb.draw_rect(
            card_x + 20,
            controls_y,
            card_w.saturating_sub(40),
            1,
            0x3B4A5C,
        );
        let controls_title = crate::gui::ui_text("KONTROLLER", "CONTROLS");
        MinGuiCompositor::draw_string(
            fb,
            card_x + 22,
            controls_y + 9,
            controls_title,
            Color::ACCENT_BLUE,
        );

        if compact_height {
            let controls = [
                crate::gui::ui_text("WASD: HAREKET / FARE: BAKIS", "WASD: MOVE / MOUSE: LOOK"),
                crate::gui::ui_text("SOL TIK: ATES / E: KULLAN", "CLICK: FIRE / E: USE"),
                crate::gui::ui_text("1-3: SILAH / R: DOLDUR", "1-3: WEAPON / R: RELOAD"),
                crate::gui::ui_text("TAB: HARITA / T: GORUNUM", "TAB: MAP / T: VIEW"),
            ];
            for (index, line) in controls.iter().enumerate() {
                let width = MinGuiCompositor::text_width(line, 1);
                MinGuiCompositor::draw_string(
                    fb,
                    card_x + card_w.saturating_sub(width) / 2,
                    controls_y + 31 + index * 12,
                    line,
                    Color::TEXT_MAIN,
                );
            }
        } else if card_w >= 450 {
            let controls = [
                ("W A S D", crate::gui::ui_text("HAREKET", "MOVE")),
                ("FARE", crate::gui::ui_text("BAKIS", "LOOK")),
                ("SOL TIK", crate::gui::ui_text("ATES", "FIRE")),
                (
                    "E / BOSLUK",
                    crate::gui::ui_text("KULLAN / KAPI", "USE / DOOR"),
                ),
                (
                    "1-3 / R / TAB",
                    crate::gui::ui_text("SILAH / DOLDUR / HARITA", "WEAPON / RELOAD / MAP"),
                ),
            ];
            let row_y = controls_y + 31;
            for (index, (key, action)) in controls.iter().enumerate() {
                let y = row_y + index * 19;
                fb.draw_rounded_rect(card_x + 22, y, 132, 17, 3, 0x273341);
                MinGuiCompositor::draw_string(fb, card_x + 29, y + 3, key, Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, card_x + 168, y + 3, action, Color::TEXT_MUTED);
            }
        } else {
            let controls = [
                "WASD: HAREKET",
                "FARE: BAKIS / SOL TIK: ATES",
                "E: KULLAN / KAPI",
                "1-3: SILAH / R: DOLDUR",
                "TAB: HARITA / T: GORUNUM",
            ];
            for (index, line) in controls.iter().enumerate() {
                let width = MinGuiCompositor::text_width(line, 1);
                MinGuiCompositor::draw_string(
                    fb,
                    card_x + card_w.saturating_sub(width) / 2,
                    controls_y + 31 + index * 18,
                    line,
                    Color::TEXT_MAIN,
                );
            }
        }

        if is_menu && !compact_height {
            let mode = match self.render_mode {
                RenderMode::Mode3D => "3D",
                RenderMode::TopDown2D => "2D",
            };
            let mut footer = crate::gui::TextBuf::new();
            footer.push_str(crate::gui::ui_text(
                "T: GORUNUM  |  MOD: ",
                "T: VIEW  |  MODE: ",
            ));
            footer.push_str(mode);
            let width = MinGuiCompositor::text_width(footer.as_str(), 1);
            MinGuiCompositor::draw_string(
                fb,
                card_x + card_w.saturating_sub(width) / 2,
                card_y + card_h.saturating_sub(23),
                footer.as_str(),
                Color::ACCENT_GREEN,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Güncelleme
    // -----------------------------------------------------------------------

    pub fn is_playing(&self) -> bool {
        self.result == GameResult::Playing
    }

    /// Oyun mantığını zaman bütçesiyle günceller.
    pub fn update_with_budget(&mut self, audio_dev: &mut AudioDevice, budget_ms: u64) -> bool {
        let budget = TimeBudget::new(budget_ms);
        let mut cancelled = false;

        let key_mask = core::mem::take(&mut self.key_mask);
        if self.result != GameResult::Playing {
            return true;
        }

        // Kare süresine göre ölçek: düşük kare hızında da dünya hızı sabit kalsın.
        let now = crate::monotonic_ms();
        let dt_ms = if self.last_update_ms == 0 {
            16
        } else {
            now.saturating_sub(self.last_update_ms).clamp(8, 60)
        };
        self.last_update_ms = now;
        let dt = dt_ms as f32 / 16.6667;
        let ticks = ((dt + 0.5) as usize).max(1);

        // Ateş: basılı tutulduğu sürece ateş eder.
        if self.mouse_fire_held && self.shoot_timer == 0 && !self.is_reloading {
            self.fire_weapon(audio_dev);
        }

        // Hareket.
        if key_mask != 0 {
            if budget.is_expired() {
                cancelled = true;
            } else {
                let speed = if key_mask & KEY_SHIFT != 0 {
                    SPRINT_SPEED
                } else {
                    WALK_SPEED
                } * dt;
                let mut target_dx = 0.0f32;
                let mut target_dy = 0.0f32;
                if key_mask & KEY_W != 0 {
                    target_dx += self.dir_x;
                    target_dy += self.dir_y;
                }
                if key_mask & KEY_S != 0 {
                    target_dx -= self.dir_x;
                    target_dy -= self.dir_y;
                }
                if key_mask & KEY_A != 0 {
                    target_dx += self.dir_y;
                    target_dy -= self.dir_x;
                }
                if key_mask & KEY_D != 0 {
                    target_dx -= self.dir_y;
                    target_dy += self.dir_x;
                }
                let mut forward = 0.0f32;
                if key_mask & KEY_UP != 0 {
                    forward += 1.0;
                }
                if key_mask & KEY_DOWN != 0 {
                    forward -= 1.0;
                }
                target_dx += forward * self.dir_x;
                target_dy += forward * self.dir_y;

                let len_sq = target_dx * target_dx + target_dy * target_dy;
                if len_sq > 1.0 {
                    let inv = 1.0 / fast_sqrt(len_sq);
                    target_dx *= inv;
                    target_dy *= inv;
                }
                self.vel_x = target_dx * speed;
                self.vel_y = target_dy * speed;

                if self.vel_x != 0.0 || self.vel_y != 0.0 {
                    self.move_player(self.vel_x, self.vel_y);
                }
                self.moving = self.vel_x != 0.0 || self.vel_y != 0.0;

                if key_mask & KEY_LEFT != 0 {
                    self.rotate(-ROT_SPEED * dt);
                }
                if key_mask & KEY_RIGHT != 0 {
                    self.rotate(ROT_SPEED * dt);
                }

                // İtilebilir duvara doğru yürüyen oyuncu duvarı iter.
                self.try_push_wall(audio_dev);
            }
        } else {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.vel_x = 0.0;
                self.vel_y = 0.0;
                self.moving = false;
                self.pushing = false;
            }
        }

        // Ateş sayacı.
        if !cancelled && self.shoot_timer > 0 {
            self.shoot_timer = self.shoot_timer.saturating_sub(ticks);
            if self.shoot_timer == 0 {
                self.shooting = false;
                self.stop_sound(audio_dev);
            }
        }

        // Adım sesi.
        if !cancelled && self.moving {
            self.last_step_tick = self.last_step_tick.saturating_add(ticks);
            if self.last_step_tick % 15 == 0 {
                self.play_sound(audio_dev, SoundKind::Step);
            }
        } else if !cancelled {
            self.last_step_tick = 0;
        }
        self.moving = false;

        // Kapı animasyonları.
        if !cancelled {
            for i in 0..self.door_count {
                if budget.is_expired() {
                    cancelled = true;
                    break;
                }
                let d = self.doors[i];
                if d.open != d.target {
                    let step = DOOR_ANIM_TICKS.recip() * ticks as f32;
                    let open = if d.target > d.open {
                        (d.open + step).min(1.0)
                    } else {
                        (d.open - step).max(0.0)
                    };
                    self.doors[i].open = open;
                }
            }
        }

        // Gizli duvar kayma animasyonu.
        if !cancelled {
            for i in 0..self.push_count {
                if self.push_walls[i].moving {
                    self.push_walls[i].slide += 0.08 * ticks as f32;
                    if self.push_walls[i].slide >= 1.0 {
                        self.push_walls[i].slide = 1.0;
                        self.push_walls[i].moving = false;
                    }
                }
            }
        }

        // Eşya toplama.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.collect_pickups();
            }
        }

        // Düşmanlar.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.update_enemies(audio_dev, ticks);
            }
        }

        // Zamanlayıcılar ve efektler.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                if self.is_reloading {
                    self.reload_timer = self.reload_timer.saturating_sub(ticks);
                    if self.reload_timer == 0 {
                        let max_ammo = self.weapon_max_ammo();
                        self.ammo_per_weapon[self.weapon as usize] = max_ammo;
                        self.ammo = max_ammo;
                        self.is_reloading = false;
                        self.play_sound(audio_dev, SoundKind::Reload);
                    }
                }
                if self.muzzle_flash_timer > 0 {
                    self.muzzle_flash_timer = self.muzzle_flash_timer.saturating_sub(ticks);
                }
                if self.damage_flash_timer > 0 {
                    self.damage_flash_timer = self.damage_flash_timer.saturating_sub(ticks);
                }
                if self.level_clear_timer > 0 {
                    self.level_clear_timer = self.level_clear_timer.saturating_sub(ticks);
                    if self.level_clear_timer == 0 {
                        self.next_level();
                    }
                }
                // Silah sallanması.
                if self.vel_x != 0.0 || self.vel_y != 0.0 {
                    self.weapon_bob = (self.weapon_bob + 0.18 * dt).clamp(0.0, TAU);
                } else {
                    self.weapon_bob = (self.weapon_bob * 0.85).max(0.0);
                }
                for p in self.pickups.iter_mut().take(self.pickup_count) {
                    if p.active {
                        p.bob += 0.08 * dt;
                    }
                }
            }
        }

        // Çıkış levhası: üzerine basınca seviye tamamlanır.
        if !cancelled && self.level_clear_timer == 0 {
            let px = self.pos_x as usize;
            let py = self.pos_y as usize;
            if self.level.tile(px, py) == fps_level::TILE_EXIT {
                self.level_clear_timer = 45;
                self.play_sound(audio_dev, SoundKind::LevelClear);
            }
        }

        // Oyun sonu.
        if !cancelled && self.health == 0 {
            crate::boot::console::serial_print("[FPS-GAME] Oyun sonu: can tukendi (Lost)\n");
            self.result = GameResult::Lost;
        }

        !cancelled
    }

    /// Düşman yapay zekâsı.
    fn update_enemies(&mut self, audio_dev: &mut AudioDevice, ticks: usize) {
        for i in 0..self.enemy_count {
            let mut e = self.enemies[i];
            if e.attack_timer > 0 {
                e.attack_timer = e.attack_timer.saturating_sub(ticks);
            }
            if e.hit_timer > 0 {
                e.hit_timer = e.hit_timer.saturating_sub(ticks);
            }
            if !e.alive {
                if e.death_timer > 0 {
                    e.death_timer = e.death_timer.saturating_sub(ticks);
                }
                self.enemies[i] = e;
                continue;
            }

            let dx = self.pos_x - e.x;
            let dy = self.pos_y - e.y;
            let dist = fast_sqrt(dx * dx + dy * dy);

            if !e.alert && dist < 9.0 && self.has_line_of_sight(e.x, e.y, dist) {
                e.alert = true;
                self.play_sound(audio_dev, SoundKind::Alert);
            }

            let mut should_attack = false;
            if e.alert && dist < 9.0 {
                // Oyuncuya doğru yürü.
                e.facing = fast_atan2(dy, dx);
                if dist > 1.05 {
                    let step = e.speed * ticks as f32;
                    let nx = e.x + (dx / dist) * step;
                    let ny = e.y + (dy / dist) * step;
                    if !self.enemy_blocked(nx, e.y) {
                        e.x = nx;
                    } else if !self.enemy_blocked(e.x, ny) {
                        e.y = ny;
                    } else if !self.enemy_blocked(nx, ny) {
                        e.x = nx;
                        e.y = ny;
                    }
                } else if e.attack_timer == 0 {
                    should_attack = true;
                }
            }

            if e.x != self.enemies[i].x || e.y != self.enemies[i].y {
                e.walk_phase = (e.walk_phase + 0.30 * ticks as f32) % TAU;
            }

            self.enemies[i] = e;

            if should_attack {
                self.enemies[i].attack_timer = 50;
                let dmg = 12u32 + (e.kind as u32) * 4;
                let mut armor_dmg = (dmg as f32 * 0.5) as u32;
                armor_dmg = armor_dmg.min(self.armor);
                let health_dmg = dmg.saturating_sub(armor_dmg);
                self.armor = self.armor.saturating_sub(armor_dmg);
                self.health = self.health.saturating_sub(health_dmg);
                self.damage_flash_timer = 8;
                self.play_sound(audio_dev, SoundKind::EnemyDamage);
            }
        }
    }

    /// İki nokta arasında görüş hattı var mı?
    fn has_line_of_sight(&self, target_x: f32, target_y: f32, dist: f32) -> bool {
        if dist <= 0.001 {
            return true;
        }
        let ray_x = (target_x - self.pos_x) / dist;
        let ray_y = (target_y - self.pos_y) / dist;
        let mut map_x = self.pos_x as usize;
        let mut map_y = self.pos_y as usize;
        let delta_x = if ray_x.abs() > 0.0001 {
            1.0 / ray_x.abs()
        } else {
            f32::MAX
        };
        let delta_y = if ray_y.abs() > 0.0001 {
            1.0 / ray_y.abs()
        } else {
            f32::MAX
        };
        let step_x = if ray_x < 0.0 { -1isize } else { 1 };
        let step_y = if ray_y < 0.0 { -1isize } else { 1 };
        let mut side_x = if ray_x < 0.0 {
            (self.pos_x - map_x as f32) * delta_x
        } else {
            (map_x as f32 + 1.0 - self.pos_x) * delta_x
        };
        let mut side_y = if ray_y < 0.0 {
            (self.pos_y - map_y as f32) * delta_y
        } else {
            (map_y as f32 + 1.0 - self.pos_y) * delta_y
        };

        for _ in 0..(self.level.w + self.level.h) {
            let hit_dist = if side_x < side_y {
                let d = side_x;
                side_x += delta_x;
                map_x = (map_x as isize + step_x) as usize;
                d
            } else {
                let d = side_y;
                side_y += delta_y;
                map_y = (map_y as isize + step_y) as usize;
                d
            };
            if hit_dist >= dist {
                return true;
            }
            if map_x >= self.level.w || map_y >= self.level.h {
                return false;
            }
            if self.cell_blocks(map_x, map_y) {
                return false;
            }
        }
        false
    }

    /// Bütçesiz güncelleme (geriye uyum).
    pub fn update(&mut self, audio_dev: &mut AudioDevice) {
        let _ = self.update_with_budget(audio_dev, u64::MAX);
    }

    // -----------------------------------------------------------------------
    // Test / önizleme yardımcıları
    // -----------------------------------------------------------------------

    /// Önizleme ve host testleri için: oyunu oynanış durumuna alır.
    #[doc(hidden)]
    pub fn set_result_playing_for_test(&mut self) {
        self.result = GameResult::Playing;
    }

    /// Önizleme ve host testleri için: seviye yükler.
    #[doc(hidden)]
    pub fn load_level_for_test(&mut self, index: usize) {
        self.load_level(index);
    }

    /// Önizleme ve host testleri için: kamera konumunu ve bakış yönünü ayarlar.
    #[doc(hidden)]
    pub fn set_view_for_test(&mut self, x: f32, y: f32, angle: f32) {
        self.pos_x = x;
        self.pos_y = y;
        let (sin_a, cos_a) = (fast_sin(angle), fast_cos(angle));
        self.dir_x = cos_a;
        self.dir_y = sin_a;
        self.plane_x = -sin_a;
        self.plane_y = cos_a;
        self.pitch = 0.0;
    }

    /// Önizleme ve host testleri için: düşman listesine erişim.
    #[doc(hidden)]
    pub fn enemy_positions_for_test(&self) -> Vec<(f32, f32)> {
        self.enemies
            .iter()
            .take(self.enemy_count)
            .filter(|e| e.alive)
            .map(|e| (e.x, e.y))
            .collect()
    }
}

impl Default for FpsGameApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Ses türleri.
#[derive(Clone, Copy)]
pub enum SoundKind {
    Shoot,
    Hit,
    Click,
    Reload,
    EnemyDamage,
    Step,
    Door,
    Pickup,
    Locked,
    Push,
    Alert,
    LevelClear,
}

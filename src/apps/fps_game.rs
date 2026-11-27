//! FPS tarzli performans testi oyunu.
//! 3D ışın izleme (raycasting) motoru, fare ile bakis, WASD hareket,
//! uzaklasma gürültüsüyle gölgelendirme, düşman hedefleri ve ses efektleri.
//! Düşmanlar 3B kutulardan oluşan humanoid modeller olarak çizilir
//! (bkz. `fps_enemy_model`).
//! no_std + alloc ortamında çalışır, çekirdek audio cihazı üzerinden ses üretir.

use crate::drivers::audio::AudioDevice;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use crate::time_budget::TimeBudget;
use core::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// Harita / döşeme sabitleri (fps_game kendi haritasını kullanır)
// ---------------------------------------------------------------------------
const TILE_WALL: u8 = 1;
const TILE_DOOR: u8 = 2;
/// Pickup döşemesi — statik haritada kullanılmaz, yalnızca eşleme kollarında
/// yer tutucu olarak bulunur (pickuplar dinamik olarak yerleştirilir).
const TILE_PICKUP: u8 = 3;

// ---------------------------------------------------------------------------
// Tekstür sabitleri (8×8 prosedürel dokular)
// ---------------------------------------------------------------------------
const TEX_W: usize = 8;
const TEX_H: usize = 8;
const TEX_BRICK: u8 = 0;
const TEX_METAL: u8 = 1;
const TEX_STONE: u8 = 2;

// ---------------------------------------------------------------------------
// Silah sabitleri
// ---------------------------------------------------------------------------
const WEAPON_PISTOL: u8 = 0;
const WEAPON_SHOTGUN: u8 = 1;
const WEAPON_COUNT: usize = 2;

const PISTOL_AMMO_MAX: u32 = 50;
const SHOTGUN_AMMO_MAX: u32 = 20;
const SHOTGUN_PELLETS: usize = 5;

const PISTOL_FIRE_DELAY: usize = 8;
const SHOTGUN_FIRE_DELAY: usize = 20;
const RELOAD_TICKS: usize = 40;

// ---------------------------------------------------------------------------
// Oyuncu ve hareket sabitleri
// ---------------------------------------------------------------------------
const PLAYER_RADIUS: f32 = 0.25;
const WALK_SPEED: f32 = 0.085;
const SPRINT_SPEED: f32 = 0.135;
const ROT_SPEED: f32 = 0.055;
const MOUSE_SENS: f32 = 0.0055;

// ---------------------------------------------------------------------------
// Görüş alanı ve render sabitleri
// ---------------------------------------------------------------------------
// Not: 3B çizimde görüş alanı kameranın `plane` vektöründen türetilir; duvar
// ve düşman modelleri aynı izdüşümü paylaşır. `MAX_DEPTH` model modülünden
// gelir (duvar sisi ve düşman kırpma paylaşır).
const ENEMIES_PER_FRAME: usize = 8;

// ---------------------------------------------------------------------------
// Ortak 3B geometri (zemin / tavan / duvar)
//
// ---------------------------------------------------------------------------
// Bütün yüzeyler tek bir perspektif modelinde hesaplanır:
//
//     ekran_y = ufuk + (göz_yüksekliği - dünya_z) * ölçek / derinlik
//
// `göz yüksekliği` duvar ortasındadır (duvarlar 1 dünya birimi yükseklikte),
// `ölçek` ise ekran yüksekliğidir (odak uzaklığı = ekran yüksekliği).
// Ufuk satırı dikey bakışla (pitch) kaydığı için zemin, tavan, duvarlar ve
// düşman modelleri aynı anda hareket eder; aralarında boşluk veya kayma
// oluşmaz.
/// Göz yüksekliği (dünya birimi).
const EYE_Z: f32 = 0.5;
/// Duvar ve tavan yüksekliği (dünya birimi).
const WALL_H: f32 = 1.0;
/// Zemin düzlemi rengi (1 dünya birimlik karo).
const FLOOR_BASE: u32 = 0x9C5F35;
/// Dama deseninde ikinci zemin tonu.
const FLOOR_ALT: u32 = 0x8B5330;
/// Zemin ızgara çizgisi rengi.
const FLOOR_GRID: u32 = 0x4A3626;
/// Tavan düzlemi rengi (1 dünya birimlik karo).
const CEIL_BASE: u32 = 0x3E5064;
/// Dama deseninde ikinci tavan tonu.
const CEIL_ALT: u32 = 0x35455A;
/// Tavan ızgara çizgisi rengi.
const CEIL_GRID: u32 = 0x28313E;
/// Uzaklıkta sis rengine karışan arka plan tonu.
const FOG_COLOR: u32 = 0x1E2836;

// ---------------------------------------------------------------------------
// Görsel efekt sabitleri
// ---------------------------------------------------------------------------
const MUZZLE_FLASH_TICKS: usize = 4;
const DAMAGE_FLASH_TICKS: usize = 8;

// ---------------------------------------------------------------------------
// Zırh ve savaş sabitleri
// ---------------------------------------------------------------------------
const MAX_ARMOR: u32 = 100;
const ENEMY_ATTACK_DIST: f32 = 1.5;
const ENEMY_CHASE_SPEED: f32 = 0.03;
const ENEMY_ATTACK_DELAY: usize = 60;
const ENEMY_ATTACK_DAMAGE: u32 = 15;
// Düşmanların oyuncuyu fark edip takip etmeye başladığı mesafe.
const ENEMY_CHASE_RADIUS: f32 = 9.0;
// Düşmanların oyuncunun başlangıç konumuna bu mesafeden daha yakın doğmamasını sağlar.
const ENEMY_MIN_SPAWN_DIST: f32 = 6.0;
const ENEMY_MIN_SPAWN_DIST_SQ: f32 = ENEMY_MIN_SPAWN_DIST * ENEMY_MIN_SPAWN_DIST;

// ---------------------------------------------------------------------------
// Klavye bit maskeleri (key_mask alanı için)
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
// Matematik ve 3B düşman modeli yardımcıları
// ---------------------------------------------------------------------------
//
// Hızlı trigonometri ve düşman modeli çizimi bağımsız bir modülde toplandı;
// böylece aynı model kodu hem çekirdekte hem de host testlerinde çalışır.

use crate::apps::fps_enemy_model::{EnemyModel, ENEMY_KINDS};
use crate::apps::gfx3d::{
    fast_atan2, fast_cos, fast_floor, fast_sin, fast_sqrt, plane_depth, screen_y, Camera3D,
    PixelSink, RenderBudget, MAX_DEPTH,
};

/// Framebuffer üzerine yazan `PixelSink` uygulaması.
struct FramebufferSink<'a> {
    fb: &'a mut Framebuffer,
    origin_x: usize,
    origin_y: usize,
}

impl PixelSink for FramebufferSink<'_> {
    fn put(&mut self, x: usize, y: usize, color: u32) {
        self.fb
            .put_pixel(self.origin_x + x, self.origin_y + y, color);
    }
}

/// Zaman bütçesini model çizimine bağlar.
impl RenderBudget for TimeBudget {
    fn expired(&self) -> bool {
        self.is_expired()
    }
}

// ---------------------------------------------------------------------------
// Duvar tekstürleri: 8x8 procedural dokular. Hücre türüne göre palet,
// duvar vuruş offset'ine (tx) göre yatay örnekleme yapılır.
// ---------------------------------------------------------------------------

/// Tek pikseli tema-tonuna göre koyulaştırır (mesafe sis etkisi için).
#[allow(dead_code)]
#[inline(always)]
fn shade_color(color: u32, delta: i32) -> u32 {
    let r = (((color >> 16) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    let g = (((color >> 8) & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    let b = ((color & 0xFF) as i32 + delta).clamp(0, 255) as u32;
    (r << 16) | (g << 8) | b
}

/// Hücre türüne göre 8x8 duvar tekstürü pikselini döner. `tx` duvara yapılan
/// vuruşın yatay konumudur (0.0..1.0), `ty` dikey konumudur.
#[inline(always)]
fn wall_texel(cell: u8, tx: f32, ty: f32) -> u32 {
    let mut px = (tx * TEX_W as f32) as usize;
    let mut py = (ty * TEX_H as f32) as usize;
    px = px.min(TEX_W - 1);
    py = py.min(TEX_H - 1);
    let base = match cell {
        TILE_WALL => TEX_BRICK,
        TILE_DOOR => TEX_METAL,
        _ => TEX_STONE,
    };
    // Hücreye göre farklı tekstür paleti.
    match base {
        TEX_BRICK => brick_texel(px, py),
        TEX_STONE => stone_texel(px, py),
        _ => metal_texel(px, py),
    }
}

#[inline(always)]
fn brick_texel(x: usize, y: usize) -> u32 {
    let brick = if (y % 4) < 2 { 0xA97142 } else { 0x8B5A3A };
    if x % 4 == 0 || y % 4 == 0 {
        0x4A3A2E
    } else {
        brick
    }
}

#[inline(always)]
fn stone_texel(x: usize, y: usize) -> u32 {
    let shade = if (x + y) % 3 == 0 { 0x6B7280 } else { 0x4B5563 };
    if (x == 3 && y % 2 == 0) || (y == 3 && x % 2 == 0) {
        0x2E3440
    } else {
        shade
    }
}

#[inline(always)]
fn metal_texel(x: usize, y: usize) -> u32 {
    let panel = if (x / 4 + y / 4) % 2 == 0 {
        0x6E7A8C
    } else {
        0x52606D
    };
    if x % 4 == 0 && y % 4 == 0 {
        0xB8B8B8
    } else {
        panel
    }
}

// ---------------------------------------------------------------------------
// Ortak geometri yardımcıları
// ---------------------------------------------------------------------------

/// Rengi verilen çarpanla karartır (sis/gölge).
#[inline(always)]
fn scale_color(color: u32, factor: f32) -> u32 {
    let r = (((color >> 16) & 0xFF) as f32 * factor).min(255.0) as u32;
    let g = (((color >> 8) & 0xFF) as f32 * factor).min(255.0) as u32;
    let b = ((color & 0xFF) as f32 * factor).min(255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Mesafeye bağlı karartma çarpanı. Duvarlar, zemin ve tavan aynı sis
/// eğrisini paylaşır; böylece derinlik ipuçları birbiriyle tutarlıdır.
#[inline(always)]
fn depth_shade(depth: f32) -> f32 {
    (0.5 + 0.5 * (1.0 - (depth / MAX_DEPTH).clamp(0.0, 1.0))).min(1.0)
}

/// Zemin düzlemi (z = 0) dokusu: 1 dünya birimlik ızgara çizgileri ve dama
/// deseni. Dünya koordinatlarından türetildiği için ızgara çizgileri duvar
/// dipleriyle tam hizada çizilir.
#[inline(always)]
fn floor_texel(world_x: f32, world_y: f32, shade: f32) -> u32 {
    let fx = fast_floor(world_x);
    let fy = fast_floor(world_y);
    let tx = world_x - fx;
    let ty = world_y - fy;
    let on_grid = tx < 0.035 || tx > 0.965 || ty < 0.035 || ty > 0.965;
    let base = if on_grid {
        FLOOR_GRID
    } else if ((fx as i32) + (fy as i32)) & 1 == 0 {
        FLOOR_BASE
    } else {
        FLOOR_ALT
    };
    scale_color(base, shade)
}

/// Tavan düzlemi (z = WALL_H) dokusu — zeminle aynı ızgara mantığı.
#[inline(always)]
fn ceiling_texel(world_x: f32, world_y: f32, shade: f32) -> u32 {
    let fx = fast_floor(world_x);
    let fy = fast_floor(world_y);
    let tx = world_x - fx;
    let ty = world_y - fy;
    let on_grid = tx < 0.035 || tx > 0.965 || ty < 0.035 || ty > 0.965;
    let base = if on_grid {
        CEIL_GRID
    } else if ((fx as i32) + (fy as i32)) & 1 == 0 {
        CEIL_BASE
    } else {
        CEIL_ALT
    };
    scale_color(base, shade)
}

// ---------------------------------------------------------------------------
// Sabitler ve yardımcı matematik
// ---------------------------------------------------------------------------

const MAP_W: usize = 20;
const MAP_H: usize = 20;
/// Harita: room/corridor tabanli 20x20 bolum (# duvar, . acil, + kapı).
/// Sol üst, sağ üst, sağ oda, sol orta, orta ve alt odalar koridorlarla baglanir.
static MAP: [[u8; MAP_W]; MAP_H] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
];

/// Düşman: konum, yaşam durumu ve 3B model animasyon durumu.
#[derive(Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    alive: bool,
    hit_timer: usize,
    attack_timer: usize,
    /// Modelin baktığı yön (radyan, oyuncuya doğru).
    facing: f32,
    /// Yürüyüş animasyonu fazı.
    walk_phase: f32,
    /// Düşman türü (palet ve ölçek seçer).
    kind: u8,
    /// Takip hızı (dalga zorluğuna göre ayarlanır).
    speed: f32,
    /// Boss düşman bayrağı (daha fazla can verir).
    #[allow(dead_code)]
    is_boss: bool,
}

const MAX_ENEMIES: usize = 8;

/// Pickup: (x, y, tür, aktif) — 1 = can, 2 = puan, 3 = zırh.
#[derive(Clone, Copy)]
struct Pickup {
    x: f32,
    y: f32,
    kind: u8,
    active: bool,
}

const MAX_PICKUPS: usize = 6;

/// Kapı durumu: 0 = kapalı, 1..=10 = açılıyor/kapalıyor, 11 = açık.
#[derive(Clone, Copy)]
struct Door {
    x: usize,
    y: usize,
    state: u8, // 0 = kapalı, 11 = açık
}

/// Görünüm modu: 3D raycasting veya 2D üstten bakış.
#[derive(Clone, Copy, PartialEq)]
pub enum RenderMode {
    Mode3D,
    TopDown2D,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameResult {
    Menu,
    Playing,
    Won,
    Lost,
}

/// Oyun durumu.
pub struct FpsGameApp {
    // Oyuncu durumu
    pub pos_x: f32,
    pub pos_y: f32,
    pub dir_x: f32,
    pub dir_y: f32,
    pub plane_x: f32,
    pub plane_y: f32,
    /// Dikey bakış açısı (pitch): radyan, pozitif = aşağı, negatif = yukarı.
    /// Clamp edilmiştir: -PI/3 ..= PI/3.
    pub pitch: f32,
    /// Hareket hızı vektörü (ivmeli hareket için).
    pub vel_x: f32,
    pub vel_y: f32,
    /// Fare hassasiyeti (radyan/piksel).
    pub mouse_sens: f32,
    pub health: u32,
    pub score: u32,
    pub ammo: u32,  // aktif silahın mermi sayısı
    pub armor: u32, // zırh (hasarı azaltır)
    pub shots_fired: u32,
    pub hits: u32,
    pub shooting: bool,
    /// Sol fare basılı tutulurken otomatik ateş (Wolfenstein tarzı).
    pub mouse_fire_held: bool,
    pub shoot_timer: usize,
    result: GameResult,

    // Silah / mermi durumu
    pub weapon: u8,                           // WEAPON_PISTOL / WEAPON_SHOTGUN
    pub ammo_per_weapon: [u32; WEAPON_COUNT], // silah başına mermi
    pub is_reloading: bool,
    pub reload_timer: usize,
    pub last_fire_tick: usize,

    // Görsel efektler
    pub weapon_bob: f32, // silah sallanma fazı
    pub muzzle_flash_timer: usize,
    pub damage_flash_timer: usize,

    // Dalga / olgunlaşma
    pub wave: usize,
    pub wave_timer: usize,

    // Görünüm modu
    pub render_mode: RenderMode,

    // Düşmanlar
    enemies: [Enemy; MAX_ENEMIES],
    enemy_count: usize,

    // Pickuplar
    pickups: [Pickup; MAX_PICKUPS],
    pickup_count: usize,

    // Kapılar
    doors: [Door; 8],
    door_count: usize,

    // Zamanlayıcılar / FPS
    pub fps: usize,
    pub min_fps: usize,
    pub frame_time_ms: u64,
    pub frame_interval_ms: u64,
    pub rendered_frames: u64,
    last_completed_frame_ms: u64,
    metrics_window_start_ms: u64,
    metrics_window_frames: u64,
    metrics_window_max_interval_ms: u64,
    /// Bellek ölçümü: oyun çalışırken gözlenen tepe sistem kullanımı (KB)
    /// ve son örneklenen boş bellek (KB). Her metrik penceresinde seri
    /// konsola `[FPS3D-METRICS]` satırıyla birlikte yazılır.
    pub mem_peak_used_kb: u64,
    pub mem_free_kb: u64,
    pub mem_heap_kb: u64,

    // Ses durumu
    pub last_shot_tick: usize,
    pub last_step_tick: usize,
    pub moving: bool,
    pub key_mask: u16,
    pub sound_pending: Option<(u32, usize)>,
}

impl FpsGameApp {
    pub fn new() -> Self {
        let mut enemies = [Enemy {
            x: 0.0,
            y: 0.0,
            alive: false,
            hit_timer: 0,
            attack_timer: 0,
            facing: 0.0,
            walk_phase: 0.0,
            kind: 0,
            speed: ENEMY_CHASE_SPEED,
            is_boss: false,
        }; MAX_ENEMIES];

        // Başlangıç düşmanlarını harita üzerindeki boş hücrelere yerleştir.
        // Oyuncunun başlangıç konumuna (3.5, 3.5) çok yakın doğmalarını engelle.
        let spawn_px = 3.5_f32;
        let spawn_py = 3.5_f32;
        let mut n = 0;
        for (ty, row) in MAP.iter().enumerate().skip(2).take(MAP_H.saturating_sub(3)) {
            for (tx, &cell) in row.iter().enumerate().skip(2).take(MAP_W.saturating_sub(3)) {
                if cell == 0 && n < MAX_ENEMIES && (tx + ty) % 3 == 0 {
                    let ex = tx as f32 + 0.5;
                    let ey = ty as f32 + 0.5;
                    let ddx = ex - spawn_px;
                    let ddy = ey - spawn_py;
                    if ddx * ddx + ddy * ddy < ENEMY_MIN_SPAWN_DIST_SQ {
                        continue;
                    }
                    enemies[n] = Enemy {
                        x: ex,
                        y: ey,
                        alive: true,
                        hit_timer: 0,
                        attack_timer: 0,
                        facing: fast_atan2(spawn_py - ey, spawn_px - ex),
                        walk_phase: 0.0,
                        kind: (n % ENEMY_KINDS.len()) as u8,
                        speed: ENEMY_CHASE_SPEED,
                        is_boss: false,
                    };
                    n += 1;
                }
            }
        }

        // Pickupları boş hücrelere yerleştir.
        let mut pickups = [Pickup {
            x: 0.0,
            y: 0.0,
            kind: 0,
            active: false,
        }; MAX_PICKUPS];
        let mut pcount = 0;
        for (ty, row) in MAP.iter().enumerate().skip(2).take(MAP_H.saturating_sub(3)) {
            for (tx, &cell) in row.iter().enumerate().skip(2).take(MAP_W.saturating_sub(3)) {
                if cell == 0 && pcount < MAX_PICKUPS && (tx * 2 + ty) % 5 == 0 {
                    let kind = match (tx + ty) % 3 {
                        0 => 1, // can
                        1 => 2, // puan
                        _ => 3, // zırh
                    };
                    pickups[pcount] = Pickup {
                        x: tx as f32 + 0.5,
                        y: ty as f32 + 0.5,
                        kind,
                        active: true,
                    };
                    pcount += 1;
                }
            }
        }

        // Kapıları harita konumlarından kaydet.
        let mut doors = [Door {
            x: 0,
            y: 0,
            state: 0,
        }; 8];
        let mut dcount = 0;
        for (ty, row) in MAP.iter().enumerate() {
            for (tx, &cell) in row.iter().enumerate() {
                if cell == TILE_DOOR && dcount < doors.len() {
                    doors[dcount] = Door {
                        x: tx,
                        y: ty,
                        state: 0,
                    };
                    dcount += 1;
                }
            }
        }

        Self {
            pos_x: 3.5,
            pos_y: 3.5,
            dir_x: 1.0,
            dir_y: 0.0,
            // Bakış düzlemi: `dir`'e dik BİRİM vektör (ekranın sağ yönü).
            // Uzunluğu görüş açısını belirlemez; açı odak uzaklığından
            // (`render_h`) türetilir, böylece yatay ve dikey ölçek eşitlenir
            // ve 1x1 dünya hücreleri ekranda kare görünür.
            plane_x: 0.0,
            plane_y: 1.0,
            pitch: 0.0,
            vel_x: 0.0,
            vel_y: 0.0,
            mouse_sens: MOUSE_SENS,
            health: 100,
            armor: 0,
            score: 0,
            ammo: 50,
            shots_fired: 0,
            hits: 0,
            shooting: false,
            mouse_fire_held: false,
            shoot_timer: 0,
            result: GameResult::Menu,
            render_mode: RenderMode::Mode3D,
            enemies,
            enemy_count: n,
            pickups,
            pickup_count: pcount,
            doors,
            door_count: dcount,
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
            last_shot_tick: 0,
            last_step_tick: 0,
            moving: false,
            key_mask: 0,
            sound_pending: None,
            // Silah / mermi durumu
            weapon: WEAPON_PISTOL,
            ammo_per_weapon: [PISTOL_AMMO_MAX, SHOTGUN_AMMO_MAX],
            is_reloading: false,
            reload_timer: 0,
            last_fire_tick: 0,
            // Görsel efektler
            weapon_bob: 0.0,
            muzzle_flash_timer: 0,
            damage_flash_timer: 0,
            // Dalga
            wave: 1,
            wave_timer: 0,
        }
    }

    /// Harita duvarı kontrolü (duvar veya kapalı kapı engeller).
    #[inline(always)]
    /// Kapı açık mı?
    fn door_is_open(&self, mx: usize, my: usize) -> bool {
        for d in self.doors.iter().take(self.door_count) {
            if d.x == mx && d.y == my {
                return d.state >= 11;
            }
        }
        false
    }

    /// Alt-hücre çarpışma kontrolü: oyuncunun dört köşesini duvar/kapalı kapıya karşı test eder.
    fn is_wall(&self, x: f32, y: f32) -> bool {
        let r = PLAYER_RADIUS;
        let corners = [
            (x - r, y - r),
            (x + r, y - r),
            (x - r, y + r),
            (x + r, y + r),
        ];
        for (cx, cy) in corners {
            // Negatif koordinat usize cast'te panik yapar; duvar say.
            if cx < 0.0 || cy < 0.0 {
                return true;
            }
            let mx = cx as usize;
            let my = cy as usize;
            if mx >= MAP_W || my >= MAP_H {
                return true;
            }
            let t = MAP[my][mx];
            if t == TILE_WALL {
                return true;
            }
            if t == TILE_DOOR && !self.door_is_open(mx, my) {
                return true;
            }
        }
        false
    }

    /// Açı farkını -PI..=PI aralığınanormalize eder.
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

    /// Silahla ateş eder. Pistol tek mermi, flinti yayılmalı birden çok pellet.
    /// Vurduysa true döner. Mermi bitişikte otomatik yeniden doldurma başlar.
    fn shoot(&mut self) -> bool {
        let wp = self.weapon as usize;
        let ammo = &mut self.ammo_per_weapon[wp];
        if *ammo == 0 {
            if !self.is_reloading {
                self.start_reload();
            }
            return false;
        }
        self.shots_fired = self.shots_fired.saturating_add(1);
        *ammo = ammo.saturating_sub(1);
        self.ammo = *ammo; // HUD eşitlemesi (aktif silah mermi)

        let pellet_count = if wp == WEAPON_SHOTGUN as usize {
            SHOTGUN_PELLETS
        } else {
            1
        };
        let spread = if wp == WEAPON_SHOTGUN as usize {
            0.13
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
                let rel = Self::angle_diff(angle, aim);
                let tolerance = 0.05 + 0.28 / dist;
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
                let headshot = Self::angle_diff(enemy_angle, base_angle).abs() < 0.025;
                self.enemies[idx].alive = false;
                self.enemies[idx].hit_timer = 12;
                self.hits = self.hits.saturating_add(1);
                if headshot {
                    self.score = self.score.saturating_add(150); // baş vuruş bonusu
                } else {
                    self.score = self.score.saturating_add(if wp == WEAPON_SHOTGUN as usize {
                        40
                    } else {
                        100
                    });
                }
                any_hit = true;
            }
        }
        any_hit
    }

    /// Yeniden doldurmayı başlatır (mermi dolusu veya boş olduğunda).
    fn start_reload(&mut self) {
        if self.is_reloading {
            return;
        }
        let wp = self.weapon as usize;
        let max_ammo = if wp == WEAPON_SHOTGUN as usize {
            SHOTGUN_AMMO_MAX
        } else {
            PISTOL_AMMO_MAX
        };
        if self.ammo_per_weapon[wp] >= max_ammo {
            return;
        }
        self.is_reloading = true;
        self.reload_timer = RELOAD_TICKS;
    }

    /// Yeniden doldurmayı tamamlar.
    fn finish_reload(&mut self) {
        let wp = self.weapon as usize;
        let max_ammo = if wp == WEAPON_SHOTGUN as usize {
            SHOTGUN_AMMO_MAX
        } else {
            PISTOL_AMMO_MAX
        };
        self.ammo_per_weapon[wp] = max_ammo;
        self.ammo = max_ammo;
        self.is_reloading = false;
        self.reload_timer = 0;
    }

    fn has_line_of_sight(&self, target_x: f32, target_y: f32, dist: f32) -> bool {
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

        for _ in 0..(MAP_W + MAP_H) {
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
            if map_x >= MAP_W || map_y >= MAP_H {
                return false;
            }
            let tile = MAP[map_y][map_x];
            if tile == TILE_WALL || (tile == TILE_DOOR && !self.door_is_open(map_x, map_y)) {
                return false;
            }
        }
        false
    }

    /// Oyuncunun baktığı yönde, erişim mesafesindeki kapı karesi.
    fn facing_door(&self) -> Option<(usize, usize)> {
        // Oyuncu duvara 0.25 birim yaklaşabildiği için 0.7..1.3 aralığı
        // hem yakın hem de bir kare uzaktaki kapıyı yakalar.
        for dist in [0.7f32, 1.0, 1.3] {
            let fx = (self.pos_x + self.dir_x * dist) as usize;
            let fy = (self.pos_y + self.dir_y * dist) as usize;
            if fx < MAP_W && fy < MAP_H && MAP[fy][fx] == TILE_DOOR {
                return Some((fx, fy));
            }
        }
        None
    }

    /// Kapıyı aç/kapat (oyuncu karşısındaysa). Durum değiştiyse `true`.
    fn toggle_door(&mut self) -> bool {
        let Some((front_x, front_y)) = self.facing_door() else {
            return false;
        };
        // Zaten listedeki kapıyı bul.
        for d in self.doors.iter_mut().take(self.door_count) {
            if d.x == front_x && d.y == front_y {
                if d.state == 0 {
                    d.state = 1; // açılmaya başla
                    return true;
                } else if d.state >= 11 {
                    d.state = 21; // kapanmaya başla (21 = 10 kare kapanma animasyonu)
                    return true;
                }
                return false;
            }
        }
        // Yeni kapı kaydı ekle.
        if self.door_count < self.doors.len() {
            self.doors[self.door_count] = Door {
                x: front_x,
                y: front_y,
                state: 1,
            };
            self.door_count += 1;
            return true;
        }
        false
    }

    /// Pickup'ı topla (üstüne gelindiğinde).
    fn collect_pickups(&mut self) {
        for p in self.pickups.iter_mut().take(self.pickup_count) {
            if !p.active {
                continue;
            }
            let dx = p.x - self.pos_x;
            let dy = p.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            if dist < 0.6 {
                p.active = false;
                match p.kind {
                    1 => {
                        self.health = (self.health + 25).min(200);
                        self.score += 50;
                    }
                    2 => {
                        self.ammo = self.ammo.saturating_add(20);
                        self.score += 25;
                    }
                    3 => {
                        // Zırh: hasarı azaltır (maks %100).
                        self.armor = (self.armor + 25).min(MAX_ARMOR);
                        self.score += 50;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Düşman vurulma zamanlayıcılarını ilerlet.
    fn respawn_enemies(&mut self) {
        for e in self.enemies.iter_mut().take(self.enemy_count) {
            if e.hit_timer > 0 {
                e.hit_timer -= 1;
            }
        }
    }

    /// Hareketi işle (duvar çarpması kontrolü ile, duvar boyunca kayma).
    fn move_player(&mut self, dx: f32, dy: f32) {
        // Tam hareketi dene (köşegen hareket de dahil).
        if !self.is_wall(self.pos_x + dx, self.pos_y + dy) {
            self.pos_x += dx;
            self.pos_y += dy;
            return;
        }
        // X ekseniyle kaymaya çalış (duvara çarptığında yatay kayma).
        if !self.is_wall(self.pos_x + dx, self.pos_y) {
            self.pos_x += dx;
            return;
        }
        // Y ekseniyle kaymaya çalış (dikey kayma).
        if !self.is_wall(self.pos_x, self.pos_y + dy) {
            self.pos_y += dy;
        }
    }

    /// Döndür.
    fn rotate(&mut self, angle: f32) {
        let (sin_a, cos_a) = (fast_sin(angle), fast_cos(angle));
        let old_dir_x = self.dir_x;
        self.dir_x = self.dir_x * cos_a - self.dir_y * sin_a;
        self.dir_y = old_dir_x * sin_a + self.dir_y * cos_a;
        let old_plane_x = self.plane_x;
        self.plane_x = self.plane_x * cos_a - self.plane_y * sin_a;
        self.plane_y = old_plane_x * sin_a + self.plane_y * cos_a;
        // Sürekli dönüşte kayan nokta sapmasını önlemek için vektörleri
        // yeniden normalize et (boy 1, birbirine dik kalır).
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

    /// Pitch (dikey bakış) ile kaymış ufuk satırının ekran y koordinatı.
    ///
    /// Zemin, tavan, duvarlar ve düşman modelleri bu değeri paylaşır; hepsi
    /// aynı anda kaydığı için dikey bakışta sahne bütünlüğü bozulmaz.
    pub fn horizon_y(&self, view_h: usize) -> f32 {
        (view_h as f32 / 2.0 - self.pitch * view_h as f32 * 0.5)
            .clamp(view_h as f32 * 0.25, view_h as f32 * 0.75)
    }

    /// Fare ile bakış açısını günceller. dx = yatay (yaw), dy = dikey (pitch).
    pub fn look(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let bounded_dx = dx.clamp(-128.0, 128.0);
        let bounded_dy = dy.clamp(-128.0, 128.0);
        // Fare yönü ekran koordinatıdır: sağa hareket dx > 0, aşağı dy > 0.
        // `rotate(+a)` bakışı sağa çevirir (ok tuşlarıyla aynı yön),
        // pozitif pitch ise aşağı bakar; işaretler bu iki yönle uyumlu.
        self.rotate(bounded_dx * self.mouse_sens);
        // Dikey bakış (pitch): -PI/3 ..= PI/3 arasında clamp'lenir.
        // Render sırasında ufuk satırı bu açıya göre yukarı/aşağı kaydırılır.
        self.pitch = (self.pitch + bounded_dy * self.mouse_sens).clamp(-PI / 3.0, PI / 3.0);
    }

    /// Ses efektleri: ateş, adım, vurma.
    pub fn play_sound(&mut self, audio_dev: &mut AudioDevice, kind: SoundKind) {
        if audio_dev.is_muted || audio_dev.volume_left == 0 {
            return;
        }
        match kind {
            SoundKind::Shoot => {
                audio_dev.set_pc_speaker_tone(1200);
                self.sound_pending = Some((1200, 50));
            }
            SoundKind::Hit => {
                audio_dev.set_pc_speaker_tone(800);
                self.sound_pending = Some((800, 50));
            }
            SoundKind::Click => {
                audio_dev.set_pc_speaker_tone(180);
                self.sound_pending = Some((180, 20));
            }
            SoundKind::Reload => {
                audio_dev.set_pc_speaker_tone(440);
                self.sound_pending = Some((440, 60));
            }
            SoundKind::EnemyDamage => {
                audio_dev.set_pc_speaker_tone(600);
                self.sound_pending = Some((600, 40));
            }
            SoundKind::Step => {
                audio_dev.set_pc_speaker_tone(200);
                self.sound_pending = Some((200, 30));
            }
            SoundKind::Door => {
                audio_dev.set_pc_speaker_tone(620);
                self.sound_pending = Some((620, 90));
            }
            SoundKind::Pickup => {
                audio_dev.set_pc_speaker_tone(1000);
                self.sound_pending = Some((1000, 30));
            }
        }
    }

    /// Bekleyen ses isteğini alır ve tüketen.
    pub fn take_sound(&mut self) -> Option<(u32, usize)> {
        self.sound_pending.take()
    }

    /// Ses isteğini işler (geçerliyse döndürür, aksi halde None).
    pub fn sound_tone(req: Option<(u32, usize)>) -> Option<(u32, usize)> {
        req.filter(|(f, _)| *f > 0)
    }

    pub fn stop_sound(&mut self, audio_dev: &mut AudioDevice) {
        audio_dev.set_pc_speaker_tone(0);
    }

    /// Tamamlanan bir içerik karesini gerçek zaman ölçümleriyle kaydeder.
    /// `render_ms` uygulama içeriğinin çizim süresidir; FPS ve frame-time
    /// penceresi tamamlanan karelerin `monotonic_ms` damgalarından hesaplanır.
    /// İlk 30 kare ısınma (warm-up) sayılmaz.
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
        // Bellek ölçümü: pencere başına bir kez sistem belleği örneklenir;
        // tepe kullanım ve o anki boş/heap değerleri raporlanır.
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

    /// 3D sahneyi çizer (raycasting). `budget_ms` ile render süresi sınırlanır;
    /// bütçe aşımı veya iptal durumunda render erken durur ve `false` döner.
    pub fn render_3d_with_budget(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        budget_ms: u64,
    ) -> bool {
        let budget = TimeBudget::new(budget_ms);
        let mut cancelled = false;

        // Görünüm alanı render_content tarafından araç çubuğu ile HUD
        // arasında hesaplanıp verilir.
        let render_y = win_y;
        let render_h = win_h;
        let render_x = win_x;
        let render_w = win_w;
        if render_w < 8 || render_h < 8 {
            return true;
        }

        // Z-buffer: her sütun için mesafe.
        let mut z_buffer: [f32; 1024] = [0.0; 1024];
        let w = render_w.min(1024);

        // ------------------------------------------------------------------
        // Ortak perspektif modeli
        //
        // Zemin (z = 0), tavan (z = WALL_H) ve duvarlar (0 <= z <= WALL_H)
        // tek bir izdüşümde hesaplanır:
        //
        //     ekran_y = ufuk + (göz_yüksekliği - dünya_z) * ölçek / derinlik
        //
        // Ufuk satırı dikey bakışla (pitch) kayar; ölçek ekran yüksekliğidir.
        // Böylece zemin ızgarası, duvar tabanları, tavan ve düşman modelleri
        // aynı geometride buluşur — pitch değiştiğinde hepsi birlikte
        // hareket eder, aralarında boşluk veya kayma kalmaz.
        // ------------------------------------------------------------------
        let horizon_f = self.horizon_y(render_h);
        let horizon = horizon_f as usize;
        let v_scale = render_h as f32;
        // Odak uzaklığı dikey ölçekle aynıdır; yatay ve dikey dünya ölçeği
        // böylece eşitlenir (izotropik izdüşüm, kare hücreler).
        let focal = v_scale;
        // Bir sütunun ışın parametresi: ekranın merkezinden odak uzaklığı
        // kadar uzaklık. `plane` birim vektör olduğu için bu parametre doğrudan
        // `lateral/depth` değerine karşılık gelir.
        let column_cam_x =
            |sx: f32, step: f32| -> f32 { (sx + step * 0.5 - w as f32 * 0.5) / focal };
        // Zemin örnekleme adımı duvar sütun adımıyla aynıdır: aynı sütunda
        // zemin ve duvar aynı ışını kullanır, böylece zemin ızgara çizgileri
        // duvar kenarlarıyla tam hizalı çizilir. Çok geniş sahnelerde adım
        // büyütülerek ışın/örnek sayısı azaltılır (retro-piksel görünümü
        // zaten bu ölçekte hedefleniyor).
        let column_step = if w >= 850 {
            3
        } else if w >= 500 {
            2
        } else {
            1
        };
        let floor_step = column_step;
        // Tavan dokusu dekoratif olduğu için daha kaba örneklenir.
        let ceiling_step = if w >= 500 { 4 } else { 2 };

        // Arka plan: ufuk sis rengi (kaplanmayan pikseller için).
        fb.draw_rect(render_x, render_y, render_w, render_h, FOG_COLOR);

        // Kırpma: kirli-dikdörtgen çiziminde sahnenin yalnızca değişen bölümü
        // ekrana gider. Piksel başına kırpma hesabı yapmak yerine tüm sahne
        // geçişleri burada bir kez hesaplanan dikdörtgene kırpılır; bu, yoğun
        // zemin/tavan/duvar döngülerinde kare başına yüz binlerce kırpma
        // hesabını ortadan kaldırır.
        let clip = fb.clip();
        let scene_x0 = render_x.max(clip.x0);
        let scene_y0 = render_y.max(clip.y0);
        let scene_x1 = (render_x + w).min(clip.x1);
        let scene_y1 = (render_y + render_h).min(clip.y1);
        // Görünüm koordinatlarındaki (0..render_h) geçerli satır aralığı.
        let row_y0 = scene_y0.saturating_sub(render_y).min(render_h);
        let row_y1 = scene_y1.saturating_sub(render_y).min(render_h);

        // Tavan düzlemi (z = WALL_H): gözün 0,5 birim üstünde olduğu için
        // ufuk satırının üstünde kalan satırlar tavanı gösterir.
        // Aynı renkli bitişik sütun örnekleri tek yatay span'e birleştirilir
        // (run-length): uzak tavan büyük tek renk blokları oluşturur ve
        // kare başına çizim çağrısı sayısı dramatik düşer. Piksel çıktısı
        // birebir aynıdır.
        for y in row_y0..horizon.min(row_y1) {
            if budget.is_expired() {
                cancelled = true;
                break;
            }
            let depth = plane_depth(horizon_f, v_scale, EYE_Z, WALL_H, y as f32 + 0.5);
            if !depth.is_finite() || depth <= 0.0 {
                continue;
            }
            let shade = depth_shade(depth);
            let mut run_x = 0usize;
            let mut run_cols = 0usize;
            let mut run_color = 0u32;
            for x in (0..w).step_by(ceiling_step) {
                if x & 63 == 0 && budget.is_expired() {
                    cancelled = true;
                    break;
                }
                let camera_x = column_cam_x(x as f32, ceiling_step as f32);
                let ray_x = self.dir_x + self.plane_x * camera_x;
                let ray_y = self.dir_y + self.plane_y * camera_x;
                let world_x = self.pos_x + ray_x * depth;
                let world_y = self.pos_y + ray_y * depth;
                let color = if depth > MAX_DEPTH {
                    scale_color(CEIL_BASE, shade)
                } else {
                    ceiling_texel(world_x, world_y, shade)
                };
                if run_cols > 0 && color == run_color {
                    run_cols += 1;
                } else {
                    if run_cols > 0 {
                        let px0 = (render_x + run_x).max(scene_x0);
                        let px1 = (render_x + run_x + run_cols * ceiling_step)
                            .min(render_x + w)
                            .min(scene_x1);
                        if px1 > px0 {
                            unsafe {
                                fb.fill_scene_span(px0, render_y + y, px1 - px0, run_color);
                            }
                        }
                    }
                    run_x = x;
                    run_cols = 1;
                    run_color = color;
                }
            }
            if !cancelled && run_cols > 0 {
                let px0 = (render_x + run_x).max(scene_x0);
                let px1 = (render_x + run_x + run_cols * ceiling_step)
                    .min(render_x + w)
                    .min(scene_x1);
                if px1 > px0 {
                    unsafe {
                        fb.fill_scene_span(px0, render_y + y, px1 - px0, run_color);
                    }
                }
            }
            if cancelled {
                break;
            }
        }

        if !cancelled {
            // Zemin düzlemi (z = 0): her satır, ekrandaki derinliğine karşılık
            // gelen dünya noktasına yansıtılır. Tavanla aynı run-length
            // birleştirme uygulanır.
            for y in horizon.max(row_y0)..row_y1 {
                if budget.is_expired() {
                    cancelled = true;
                    break;
                }
                let depth = plane_depth(horizon_f, v_scale, EYE_Z, 0.0, y as f32 + 0.5);
                if !depth.is_finite() || depth <= 0.0 {
                    continue;
                }
                let shade = depth_shade(depth);
                let mut run_x = 0usize;
                let mut run_cols = 0usize;
                let mut run_color = 0u32;
                for x in (0..w).step_by(floor_step) {
                    if x & 63 == 0 && budget.is_expired() {
                        cancelled = true;
                        break;
                    }
                    let camera_x = column_cam_x(x as f32, floor_step as f32);
                    let ray_x = self.dir_x + self.plane_x * camera_x;
                    let ray_y = self.dir_y + self.plane_y * camera_x;
                    let world_x = self.pos_x + ray_x * depth;
                    let world_y = self.pos_y + ray_y * depth;
                    let color = if depth > MAX_DEPTH {
                        scale_color(FLOOR_BASE, shade)
                    } else {
                        floor_texel(world_x, world_y, shade)
                    };
                    if run_cols > 0 && color == run_color {
                        run_cols += 1;
                    } else {
                        if run_cols > 0 {
                            let px0 = (render_x + run_x).max(scene_x0);
                            let px1 = (render_x + run_x + run_cols * floor_step)
                                .min(render_x + w)
                                .min(scene_x1);
                            if px1 > px0 {
                                unsafe {
                                    fb.fill_scene_span(px0, render_y + y, px1 - px0, run_color);
                                }
                            }
                        }
                        run_x = x;
                        run_cols = 1;
                        run_color = color;
                    }
                }
                if !cancelled && run_cols > 0 {
                    let px0 = (render_x + run_x).max(scene_x0);
                    let px1 = (render_x + run_x + run_cols * floor_step)
                        .min(render_x + w)
                        .min(scene_x1);
                    if px1 > px0 {
                        unsafe {
                            fb.fill_scene_span(px0, render_y + y, px1 - px0, run_color);
                        }
                    }
                }
                if cancelled {
                    break;
                }
            }
        }
        if !cancelled {
            // Perspektif çizgi: açık tur-möjik bant
            fb.draw_rect(render_x, render_y + horizon, render_w, 3, 0xF0B040);
        }

        // Geniş pencerelerde her ikinci sütun aynı duvar geometrisini paylaşır.
        // Bu, DDA/f32 maliyetini yaklaşık yarıya indirirken küçük ekranlarda
        // tam çözünürlüklü görünümü korur.
        for x in (0..w).step_by(column_step) {
            if budget.is_expired() {
                cancelled = true;
                break;
            }

            // Sütunun ışın parametresi (odak uzaklığına göre ölçekli). Sütunun
            // ortası esas alınır; zemin ve tavan örnekleri de aynı ışını
            // kullandığından yüzeyler birbirine tam oturur.
            let camera_x = column_cam_x(x as f32, column_step as f32);
            let ray_dir_x = self.dir_x + self.plane_x * camera_x;
            let ray_dir_y = self.dir_y + self.plane_y * camera_x;

            // Grid hücresi.
            let mut map_x = self.pos_x as usize;
            let mut map_y = self.pos_y as usize;

            // Mesafe adımları.
            let delta_dist_x = (1.0 / ray_dir_x.abs()).max(0.0001);
            let delta_dist_y = (1.0 / ray_dir_y.abs()).max(0.0001);

            // Yön ve ilk kenar mesafesi.
            let step_x: isize;
            let step_y: isize;
            let mut side_dist_x: f32;
            let mut side_dist_y: f32;

            if ray_dir_x < 0.0 {
                step_x = -1;
                side_dist_x = (self.pos_x - map_x as f32) * delta_dist_x;
            } else {
                step_x = 1;
                side_dist_x = (map_x as f32 + 1.0 - self.pos_x) * delta_dist_x;
            }
            if ray_dir_y < 0.0 {
                step_y = -1;
                side_dist_y = (self.pos_y - map_y as f32) * delta_dist_y;
            } else {
                step_y = 1;
                side_dist_y = (map_y as f32 + 1.0 - self.pos_y) * delta_dist_y;
            }

            // DDA: duvara kadar ilerle.
            let mut hit = false;
            let mut side = 0;

            // Harita bozuk olsa bile tek ray ana döngüyü kilitlemesin.
            // Bütçe kontrolü ile DDA döngüsünün sonsuzlaşması engellenir.
            let mut ray_steps = 0usize;
            while !hit && ray_steps < MAP_W + MAP_H + 4 {
                if budget.is_expired() {
                    cancelled = true;
                    break;
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
                if map_x >= MAP_W || map_y >= MAP_H {
                    hit = true;
                } else {
                    let t = MAP[map_y][map_x];
                    if t == TILE_WALL || (t == TILE_DOOR && !self.door_is_open(map_x, map_y)) {
                        hit = true;
                    }
                }
            }
            if cancelled {
                break;
            }

            // Duvara gerçek mesafe (fisheye düzeltmesi).
            let perp_wall_dist: f32 = if side == 0 {
                (map_x as f32 - self.pos_x + (1.0 - step_x as f32) / 2.0) / ray_dir_x
            } else {
                (map_y as f32 - self.pos_y + (1.0 - step_y as f32) / 2.0) / ray_dir_y
            };
            // Bozuk bir açı/mesafe hiçbir zaman dev bir çizim döngüsüne
            // dönüşmesin.
            let perp_wall_dist = if perp_wall_dist.is_finite() {
                perp_wall_dist.abs().clamp(0.15, MAX_DEPTH * 4.0)
            } else {
                MAX_DEPTH
            };

            for slot in z_buffer[x..(x + column_step).min(w)].iter_mut() {
                *slot = perp_wall_dist;
            }

            // Duvarın dikey sınırları ortak perspektif modelinden gelir:
            // taban (z = 0) ve tepe (z = WALL_H) aynı formülle hesaplanır.
            // Böylece duvar dibi zemindeki ızgara çizgileriyle aynı derinlikte
            // buluşur ve duvar pitch ile birlikte (zeminden ayrılmadan) kayar.
            let y_bottom = screen_y(horizon_f, v_scale, EYE_Z, 0.0, perp_wall_dist);
            let y_top = screen_y(horizon_f, v_scale, EYE_Z, WALL_H, perp_wall_dist);
            // Kırpma dikdörtgeniyle kesiştir: yalnızca gerçekten görünen
            // satırlar ve sütunlar yazılır.
            let draw_start = (y_top.max(0.0) as usize).max(row_y0).min(render_h);
            let draw_end = (y_bottom.max(0.0) as usize).min(row_y1).min(render_h);
            // Bu sütunun ekrandaki piksel aralığı (kırpılmış).
            let col_px0 = (render_x + x).max(scene_x0);
            let col_px1 = ((render_x + x + column_step).min(render_x + w)).min(scene_x1);
            let col_visible = col_px1 > col_px0 && draw_end > draw_start;

            // Duvar tekstürü (Wolfenstein 3D tarzı desenli duvarlar).
            // Ray harita sınırından çıktıysa son örneği dışarı taşırma.
            let tile = if map_x < MAP_W && map_y < MAP_H {
                MAP[map_y][map_x]
            } else {
                TILE_WALL
            };
            // Tekstür x koordinatı: duvara çarpan noktanın fractional kısmı.
            let tex_x_raw = if side == 0 {
                perp_wall_dist * ray_dir_y
            } else {
                perp_wall_dist * ray_dir_x
            };
            // Negatif koordinatlarda da doğru frac değeri (tabana doğru yuvarlama).
            let tex_x = tex_x_raw - fast_floor(tex_x_raw);
            // Duvar tekstürü daha küçük, kapı daha büyük görünsün diye ölçek ayarı
            let tex_scale = if tile == TILE_DOOR { 0.5 } else { 64.0 };
            let tx = (tex_x * tex_scale) as usize % 8;

            // Duvar rengi (mesafeye göre gölgeli).
            // Yakındaki duvarlar canlı, uzaktakiler hâlâ ayırt edilebilir.
            let shade = depth_shade(perp_wall_dist);
            // Kapı açıklığı sütun başına bir kez sorgulanır (eski kod her
            // piksel satırında kapı listesinde doğrusal arama yapıyordu).
            let door_open = tile == TILE_DOOR && self.door_is_open(map_x, map_y);

            // Dikey çizgi olarak duvarı çiz (tekstürlü). Pikseller kırpma
            // garantili `fill_scene_span` ile yazılır; satır başına zaman
            // bütçesi denetimi 16 satırda bire düşürülerek `rdtsc` maliyeti
            // azaltılır.
            if col_visible {
                let span_w = col_px1 - col_px0;
                for yy in draw_start..draw_end {
                    if yy & 15 == 0 && budget.is_expired() {
                        cancelled = true;
                        break;
                    }
                    // Tekstür y koordinatı (0.0 = üst, 1.0 = alt) dünya
                    // yüksekliğinden türetilir; duvar ekran kenarından taşsa
                    // bile doku esnemez, yalnızca kırpılır.
                    let world_z = EYE_Z + (horizon_f - yy as f32) * perp_wall_dist / v_scale;
                    let ty_f = (1.0 - world_z / WALL_H).clamp(0.0, 1.0);
                    // Duvar metni: hücre türüne göre procedural doku, vuruş
                    // konumundan (tex_x, ty_f) örnekleme yapılır.
                    let base_color = wall_texel(tile, tex_x, ty_f);
                    let wall_color = match tile {
                        TILE_DOOR if door_open => {
                            // Açık kapı: parlak yeşil geçiş + koyu çerçeve.
                            if tx == 0 || tx == 7 || ty_f < 0.06 || ty_f > 0.94 {
                                scale_color(0x307040, shade)
                            } else {
                                scale_color(0x40C060, shade)
                            }
                        }
                        _ => scale_color(base_color, shade),
                    };
                    // Yan yüzeyi kanal bazlı karart; packed RGB değerini bölmek
                    // renk kanallarını bozuyordu ve duvarı görünmez yapıyordu.
                    let wall_color = if side == 1 {
                        let r = (((wall_color >> 16) & 0xFF) * 3) / 4;
                        let g = (((wall_color >> 8) & 0xFF) * 3) / 4;
                        let b = (wall_color & 0xFF) * 3 / 4;
                        (r << 16) | (g << 8) | b
                    } else {
                        wall_color
                    };
                    unsafe {
                        fb.fill_scene_span(col_px0, render_y + yy, span_w, wall_color);
                    }
                }
            }
            if cancelled {
                break;
            }
        }

        if !cancelled {
            // Düşmanlar gerçek 3B modeller olarak çizilir: kutulardan oluşan
            // humanoid gövde, perspektif izdüşüm ve duvar derinlik tamponu ile.
            let mut rendered_models: usize = 0;
            for e in self.enemies.iter().take(self.enemy_count) {
                if budget.is_expired() {
                    cancelled = true;
                    break;
                }
                if !e.alive {
                    continue;
                }
                if rendered_models >= ENEMIES_PER_FRAME {
                    continue;
                }
                rendered_models += 1;
                if !self.draw_enemy_3d(
                    fb,
                    render_x,
                    render_y,
                    w,
                    render_h,
                    &z_buffer[..w],
                    e,
                    &budget,
                ) {
                    cancelled = true;
                    break;
                }
            }
        }

        if !cancelled {
            // Vurma efekti: tüm görüntüyü piksel piksel yeniden okumak pahalıydı.
            // Bunun yerine ekran kenarda küçük bir hasar/ateş çerçevesi çizilir.
            if self.shooting && self.shoot_timer > 0 {
                let pulse = if self.shoot_timer > 5 {
                    0xFFD34E
                } else {
                    0xA62E2E
                };
                let edge = 3usize;
                fb.draw_rect(render_x, render_y, render_w, edge, pulse);
                fb.draw_rect(
                    render_x,
                    render_y + render_h.saturating_sub(edge),
                    render_w,
                    edge,
                    pulse,
                );
                fb.draw_rect(render_x, render_y, edge, render_h, pulse);
                fb.draw_rect(
                    render_x + render_w.saturating_sub(edge),
                    render_y,
                    edge,
                    render_h,
                    pulse,
                );
            }
        }

        !cancelled
    }

    // -----------------------------------------------------------------------
    // 3B düşman modeli çizimi
    // -----------------------------------------------------------------------

    /// Bir düşmanın 3B modelini çizer.
    ///
    /// Model çizimi `fps_enemy_model` modülünde yaşar; burada yalnızca
    /// framebuffer ile model modülü arasındaki köprü kurulur. Bütçe aşılırsa
    /// `false` döner.
    fn draw_enemy_3d(
        &self,
        fb: &mut Framebuffer,
        render_x: usize,
        render_y: usize,
        view_w: usize,
        view_h: usize,
        zbuf: &[f32],
        e: &Enemy,
        budget: &TimeBudget,
    ) -> bool {
        if view_w < 8 || view_h < 8 || zbuf.len() < view_w {
            return true;
        }
        let cam = Camera3D {
            pos_x: self.pos_x,
            pos_y: self.pos_y,
            dir_x: self.dir_x,
            dir_y: self.dir_y,
            plane_x: self.plane_x,
            plane_y: self.plane_y,
            eye_z: 0.5,
            v_scale: view_h as f32,
            // Yatay odak dikey odakla aynı: izdüşüm izotropik olur ve model
            // duvar/zemin ile aynı ışında buluşur.
            focal_x: view_h as f32,
            // Dikey bakış (pitch) düşman modeli için de horizon'u kaydırır.
            // Aynı yardımcı kullanıldığı için model ile duvar/zemin hep aynı
            // ufukta buluşur.
            horizon_y: self.horizon_y(view_h),
            view_w: view_w as f32,
        };
        let model = EnemyModel {
            x: e.x,
            y: e.y,
            facing: e.facing,
            walk_phase: e.walk_phase,
            kind: e.kind,
        };
        let mut sink = FramebufferSink {
            fb,
            origin_x: render_x,
            origin_y: render_y,
        };
        crate::apps::fps_enemy_model::draw_enemy_model(
            &mut sink, &cam, zbuf, view_w, view_h, &model, budget,
        )
    }

    /// Geriye uyumlu: bütçe kontrolü olmadan 3D render yapar.
    pub fn render_3d(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let _ = self.render_3d_with_budget(fb, win_x, win_y, win_w, win_h, u64::MAX);
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
        // Kare hücreler: harita en-boy oranı bozulmadan ortalanır.
        let cell = (win_w / MAP_W).min(win_h / MAP_H).max(1);
        let map_w = cell * MAP_W;
        let map_h = cell * MAP_H;
        let offset_x = win_x + win_w.saturating_sub(map_w) / 2;
        let offset_y = win_y + win_h.saturating_sub(map_h) / 2;

        // Harita karelerini çiz.
        for (ty, row) in MAP.iter().enumerate() {
            for (tx, &t) in row.iter().enumerate() {
                let x = offset_x + tx * cell;
                let y = offset_y + ty * cell;
                let color = match t {
                    TILE_WALL => 0x6B7A8C,
                    TILE_DOOR => {
                        if self.door_is_open(tx, ty) {
                            0x45C766
                        } else {
                            0xE0A92E
                        }
                    }
                    TILE_PICKUP => 0x40E040,
                    _ => 0x151A22,
                };
                fb.draw_rect(
                    x + 1,
                    y + 1,
                    cell.saturating_sub(2),
                    cell.saturating_sub(2),
                    color,
                );
                // Hücre kenarlığı.
                fb.draw_rect(x, y, cell, 1, 0x2A3340);
                fb.draw_rect(x, y, 1, cell, 0x2A3340);
            }
        }

        // Düşmanları çiz (kırmızı kare).
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let ex = offset_x + (e.x as usize).min(MAP_W - 1) * cell + cell / 4;
            let ey = offset_y + (e.y as usize).min(MAP_H - 1) * cell + cell / 4;
            let size = (cell / 2).max(2);
            fb.draw_rect(ex, ey, size, size, 0xFF4040);
        }

        // Pickupları çiz (yeşil küçük kare).
        for p in self.pickups.iter().take(self.pickup_count) {
            if !p.active {
                continue;
            }
            let px = offset_x + (p.x as usize).min(MAP_W - 1) * cell + cell / 3;
            let py = offset_y + (p.y as usize).min(MAP_H - 1) * cell + cell / 3;
            let size = (cell / 3).max(2);
            let color = if p.kind == 1 {
                0x40FF40 // can (yeşil)
            } else if p.kind == 2 {
                0xFFFF40 // mermi (sarı)
            } else {
                0x40A0FF // zırh (mavi)
            }; /* kind 1: health(green), kind 2: ammo(yellow), kind 3: armor(blue) */
            fb.draw_rect(px, py, size, size, color);
        }

        // Oyuncuyu çiz (mavi kare + yön işareti).
        let px = offset_x + (self.pos_x as usize).min(MAP_W - 1) * cell + cell / 4;
        let py = offset_y + (self.pos_y as usize).min(MAP_H - 1) * cell + cell / 4;
        let size = (cell / 2).max(2);
        fb.draw_rect(px, py, size, size, 0x4040FF);

        // Yön işareti (oyuncunun baktığı yön).
        let dir_len = (cell / 2).max(4);
        let dx = (self.dir_x * dir_len as f32) as isize;
        let dy = (self.dir_y * dir_len as f32) as isize;
        let cx = px + size / 2;
        let cy = py + size / 2;
        let end_x = (cx as isize + dx).max(0) as usize;
        let end_y = (cy as isize + dy).max(0) as usize;
        fb.draw_rect(cx.saturating_sub(1), cy.saturating_sub(1), 3, 3, 0xFFFFFF);
        // Basit çizgi: yön noktasını işaretle.
        fb.draw_rect(
            end_x.saturating_sub(1),
            end_y.saturating_sub(1),
            3,
            3,
            0xFFFF00,
        );

        // Mod bilgisi.
        let mut info = crate::gui::TextBuf::new();
        info.push_str(crate::gui::ui_text("2D Üstten Bakış", "2D Top-Down View"));
        MinGuiCompositor::draw_string(
            fb,
            offset_x + 4,
            offset_y.saturating_add(win_h).saturating_sub(16),
            info.as_str(),
            Color::ACCENT_YELLOW,
        );
    }

    /// Radar / küçük harita: duvarlar, kapılar ve düşmanlar.
    pub fn render_radar(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize) {
        let radar_w = 96usize;
        let radar_h = 72usize;
        let rx = win_x + win_w.saturating_sub(radar_w).saturating_sub(10);
        let ry = win_y + 8;

        fb.draw_rect(rx, ry, radar_w, radar_h, 0x05070A);
        let cw = (radar_w / MAP_W).max(1);
        let ch = (radar_h / MAP_H).max(1);

        for (ty, row) in MAP.iter().enumerate() {
            for (tx, &t) in row.iter().enumerate() {
                let color = match t {
                    TILE_WALL => 0x4A5568,
                    TILE_DOOR => {
                        if self.door_is_open(tx, ty) {
                            0x45C766
                        } else {
                            0xE0A92E
                        }
                    }
                    TILE_PICKUP => 0x40E040,
                    _ => 0x0C1018,
                };
                fb.draw_rect(rx + tx * cw, ry + ty * ch, cw, ch, color);
            }
        }
        fb.draw_rect_outline(rx, ry, radar_w, radar_h, Color::ACCENT_GREEN);

        // Oyuncu noktası ve bakış yönü.
        let px = rx + (self.pos_x / MAP_W as f32 * radar_w as f32) as usize;
        let py = ry + (self.pos_y / MAP_H as f32 * radar_h as f32) as usize;
        if px > rx && px < rx + radar_w && py > ry && py < ry + radar_h {
            fb.draw_rect(
                px.saturating_sub(1),
                py.saturating_sub(1),
                3,
                3,
                Color::ACCENT_BLUE,
            );
            let dx = (self.dir_x * 5.0) as isize;
            let dy = (self.dir_y * 5.0) as isize;
            let ex = (px as isize + dx).max(0) as usize;
            let ey = (py as isize + dy).max(0) as usize;
            fb.draw_rect(ex, ey, 1, 1, 0xFFFFFF);
        }

        // Düşmanlar.
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let ex = rx + (e.x / MAP_W as f32 * radar_w as f32) as usize;
            let ey = ry + (e.y / MAP_H as f32 * radar_h as f32) as usize;
            if ex > rx && ex < rx + radar_w && ey > ry && ey < ry + radar_h {
                fb.draw_rect(ex, ey, 2, 2, Color::ACCENT_RED);
            }
        }
    }

    /// HUD (sayaçlar, can, puan, FPS) çizer.
    pub fn render_hud(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let hud_y = win_y + win_h.saturating_sub(24);
        fb.draw_rect(win_x + 6, hud_y, win_w - 12, 18, Color::TASKBAR_BG);

        // Can.
        let mut hp_text = crate::gui::TextBuf::new();
        hp_text.push_str(crate::gui::ui_text("Can: ", "HP: "));
        hp_text.push_u2(self.health);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 12,
            hud_y + 4,
            hp_text.as_str(),
            Color::ACCENT_GREEN,
        );

        // Zırh.
        let mut ar_text = crate::gui::TextBuf::new();
        ar_text.push_str(crate::gui::ui_text("Zırh: ", "Armor: "));
        ar_text.push_u2(self.armor);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 96,
            hud_y + 4,
            ar_text.as_str(),
            Color::ACCENT_BLUE,
        );

        // Silah.
        let wp_name = if self.weapon == WEAPON_SHOTGUN {
            crate::gui::ui_text("Flinti", "Shotgun")
        } else {
            crate::gui::ui_text("Pistol", "Pistol")
        };
        let mut wp_text = crate::gui::TextBuf::new();
        wp_text.push_str(crate::gui::ui_text("Silah: ", "Weapon: "));
        wp_text.push_str(wp_name);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 150,
            hud_y + 4,
            wp_text.as_str(),
            Color::ACCENT_PINK,
        );

        // Puan.
        let mut score_text = crate::gui::TextBuf::new();
        score_text.push_str(crate::gui::ui_text("Puan: ", "Score: "));
        score_text.push_u2(self.score);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 300,
            hud_y + 4,
            score_text.as_str(),
            Color::TEXT_MAIN,
        );

        // Mermi.
        let mut ammo_text = crate::gui::TextBuf::new();
        ammo_text.push_str(crate::gui::ui_text("Mermi: ", "Ammo: "));
        ammo_text.push_u2(self.ammo);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 230,
            hud_y + 4,
            ammo_text.as_str(),
            Color::ACCENT_YELLOW,
        );

        // FPS / minimum FPS / kare süresi.
        let mut fps_text = crate::gui::TextBuf::new();
        fps_text.push_str(crate::gui::ui_text("FPS: ", "FPS: "));
        fps_text.push_usize(self.fps);
        fps_text.push_str(crate::gui::ui_text(" (min ", " (min "));
        fps_text.push_usize(self.min_fps);
        fps_text.push_str(") ");
        fps_text.push_usize(self.frame_time_ms as usize);
        fps_text.push_str("ms");
        // Bellek: metrik penceresinde saniyede bir kez örneklenen boş
        // bellek MB cinsinden gösterilir.
        fps_text.push_str(crate::gui::ui_text(" bellek:", " mem:"));
        fps_text.push_usize((self.mem_free_kb / 1024) as usize);
        fps_text.push_str("MB");
        MinGuiCompositor::draw_string(
            fb,
            win_x + 380,
            hud_y + 4,
            fps_text.as_str(),
            Color::ACCENT_YELLOW,
        );

        // Vuruş oranı.
        let acc = if self.shots_fired > 0 {
            (self.hits as f32 / self.shots_fired as f32 * 100.0) as u32
        } else {
            0
        };
        let mut acc_text = crate::gui::TextBuf::new();
        acc_text.push_str(crate::gui::ui_text("Dogru: ", "Acc: "));
        acc_text.push_u2(acc);
        acc_text.push_str("%");
        MinGuiCompositor::draw_string(
            fb,
            win_x + 470,
            hud_y + 4,
            acc_text.as_str(),
            Color::TEXT_MUTED,
        );

        // Düşman sayısı.
        let alive = self
            .enemies
            .iter()
            .take(self.enemy_count)
            .filter(|e| e.alive)
            .count();
        let mut en_text = crate::gui::TextBuf::new();
        en_text.push_str(crate::gui::ui_text("Dusman: ", "Enemies: "));
        en_text.push_u2(alive as u32);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 570,
            hud_y + 4,
            en_text.as_str(),
            Color::TEXT_MUTED,
        );

        // Dalga.
        let mut wave_text = crate::gui::TextBuf::new();
        wave_text.push_str(crate::gui::ui_text("Dalga: ", "Wave: "));
        wave_text.push_u2(self.wave as u32);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 650,
            hud_y + 4,
            wave_text.as_str(),
            Color::ACCENT_PINK,
        );
    }

    /// Ana çizim girişi.
    /// İçerik çizimini zaman bütçesiyle çalıştır. Render bütçesi aşılırsa
    /// erken durur ve `false` döner. `render_ms` render bütçesi (ms).
    pub fn render_content_with_budget(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        render_ms: u64,
    ) -> bool {
        let budget = TimeBudget::new(render_ms);

        // Araç çubuğu — mod başlığı.
        let mode_label = match self.render_mode {
            RenderMode::Mode3D => {
                crate::gui::ui_text("FPS 3D (T=2D Üstten)", "FPS 3D (T=Top-Down 2D)")
            }
            RenderMode::TopDown2D => {
                crate::gui::ui_text("FPS 2D Üstten (T=3D)", "FPS 2D Top-Down (T=3D)")
            }
        };
        if budget.is_expired() {
            return false;
        }
        fb.draw_rect(
            win_x + 6,
            win_y + 30,
            win_w - 12,
            crate::gui::sp(24),
            Color::TITLE_BAR,
        );
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 36, mode_label, Color::TEXT_MAIN);

        // Karşıda bir kapı varsa kullanım ipucu göster.
        if self.render_mode == RenderMode::Mode3D {
            if budget.is_expired() {
                return false;
            }
            if let Some((door_x, door_y)) = self.facing_door() {
                let label = if self.door_is_open(door_x, door_y) {
                    crate::gui::ui_text("E: Kapıyı kapat", "E: Close door")
                } else {
                    crate::gui::ui_text("E: Kapıyı aç", "E: Open door")
                };
                let tw = MinGuiCompositor::text_width(label, 1);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + win_w.saturating_sub(tw + 14),
                    win_y + 36,
                    label,
                    Color::ACCENT_YELLOW,
                );
            }
        }

        // Sahne alanı: araç çubuğu ile HUD arasında kalan bölge.
        let view_y = win_y + 30 + crate::gui::sp(24) + 2;
        let view_h = win_h.saturating_sub((view_y - win_y) + 26);

        if self.result == GameResult::Menu {
            if budget.is_expired() {
                return false;
            }
            fb.draw_rect(win_x + 2, view_y, win_w.saturating_sub(4), view_h, 0x101722);
            self.render_result_menu(fb, win_x, view_y, view_h, win_w);
            return true;
        }

        // Görünüm moduna göre sahne çiz.
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
                if completed && !budget.is_expired() {
                    self.render_radar(fb, win_x, view_y, win_w);
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

        // HUD (bütçe varsa).
        if completed && !budget.is_expired() {
            self.render_hud(fb, win_x, win_y, win_w, win_h);
        }

        // 3D modunda crosshair ve silah çiz (bütçe varsa).
        if completed && !budget.is_expired() && self.render_mode == RenderMode::Mode3D {
            // Merkez işaretçi (crosshair).
            let cx = win_x + win_w / 2;
            let cy = view_y + view_h / 2;
            fb.draw_rect(cx.saturating_sub(1), cy.saturating_sub(7), 2, 5, 0xD8F4FF);
            fb.draw_rect(cx.saturating_sub(1), cy + 2, 2, 5, 0xD8F4FF);
            fb.draw_rect(cx.saturating_sub(7), cy.saturating_sub(1), 5, 2, 0xD8F4FF);
            fb.draw_rect(cx + 2, cy.saturating_sub(1), 5, 2, 0xD8F4FF);
            fb.draw_rect(cx, cy, 1, 1, 0xFFFFFF);

            // Silah silüeti: sallanma (bobbing), geri tepme ve muzzle flaş.
            let bob_offset = (fast_sin(self.weapon_bob) * 3.0) as isize;
            let bob_offset = bob_offset.clamp(-4, 4);
            let gun_w = if self.weapon == WEAPON_SHOTGUN {
                110
            } else {
                92
            };
            let gun_h = if self.weapon == WEAPON_SHOTGUN {
                66
            } else {
                58
            };
            let gun_x = win_x + win_w / 2 - gun_w / 2 + bob_offset as usize;
            let gun_y = win_y + win_h.saturating_sub(gun_h + 10 - bob_offset.unsigned_abs());
            let recoil = if self.shooting && self.shoot_timer > 0 {
                (10 - self.shoot_timer) * 2
            } else {
                0
            };
            let gy = gun_y.saturating_add(recoil);
            let gun_body = if self.weapon == WEAPON_SHOTGUN {
                0x2E3A46
            } else {
                0x252A33
            };
            let gun_trim = if self.weapon == WEAPON_SHOTGUN {
                0x9FB0BC
            } else {
                0x77808B
            };
            fb.draw_rect(gun_x + 20, gy + 8, gun_w - 38, 25, gun_body);
            fb.draw_rect(gun_x + 25, gy + 5, gun_w - 48, 6, gun_trim);
            fb.draw_rect(gun_x + gun_w / 2 - 5, gy, 10, 10, 0xAEB8C2);
            fb.draw_rect(gun_x + 30, gy + 28, 28, 25, 0x4B3030);
            fb.draw_rect(gun_x + 34, gy + 32, 20, 21, 0x6A3B36);
            fb.draw_rect(gun_x + 18, gy + 30, 12, 7, 0x171A20);
            // Mermi yok işareti.
            if self.ammo_per_weapon[self.weapon as usize] == 0 && !self.is_reloading {
                fb.draw_rect(gun_x + gun_w - 16, gy + 6, 12, 12, Color::ACCENT_RED);
            } else if self.ammo > 0 {
                fb.draw_rect(gun_x + gun_w - 15, gy + 5, 10, 10, 0xFFD700);
            }
            // Muzzle flaş (ateş etme anında kısa parlaklık).
            if self.muzzle_flash_timer > 0 {
                let mf = self.muzzle_flash_timer as f32 / MUZZLE_FLASH_TICKS as f32;
                let size = (14.0 * mf) as usize;
                let cx2 = gun_x + gun_w / 2;
                let cy2 = gy.saturating_sub(4);
                fb.draw_rect(
                    cx2.saturating_sub(size / 2),
                    cy2.saturating_sub(size / 2),
                    size,
                    size,
                    0xFFF4A3,
                );
                fb.draw_rect(
                    cx2.saturating_sub(size / 4),
                    cy2.saturating_sub(size / 4),
                    size / 2,
                    size / 2,
                    0xFFFFFF,
                );
            }
            if self.shooting && self.shoot_timer > 5 {
                fb.draw_rect(
                    gun_x + gun_w / 2 - 5,
                    gy.saturating_sub(8),
                    10,
                    10,
                    0xFFF4A3,
                );
                fb.draw_rect(gun_x + gun_w / 2 - 2, gy.saturating_sub(14), 4, 6, 0xFFFFFF);
            }

            // Vuruş flaşı.
            if self.shooting && self.shoot_timer > 0 {
                fb.draw_rect(
                    win_x + win_w.saturating_sub(60),
                    win_y + win_h.saturating_sub(80),
                    50,
                    30,
                    Color::ACCENT_RED,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + win_w.saturating_sub(52),
                    win_y + win_h.saturating_sub(74),
                    crate::gui::ui_text("VURDU!", "HIT!"),
                    Color::ACCENT_RED,
                );
            }

            // Hasar vigneti: düşman vurduğunda kenarlarda kırık kırmızı flaş.
            if self.damage_flash_timer > 0 {
                let t = self.damage_flash_timer as f32 / DAMAGE_FLASH_TICKS as f32;
                let alpha = (t * 0.5) as usize;
                let flash = Color::ACCENT_RED;
                let top = win_y + view_y.saturating_sub(2);
                let bottom = win_y + win_h.saturating_sub(2);
                fb.draw_rect(win_x + 2, top, win_w - 4, 4.max(alpha), flash);
                fb.draw_rect(
                    win_x + 2,
                    bottom.saturating_sub(4.max(alpha)),
                    win_w - 4,
                    4.max(alpha),
                    flash,
                );
                fb.draw_rect(
                    win_x + 2,
                    top,
                    4.max(alpha),
                    view_h.saturating_sub(4),
                    flash,
                );
                fb.draw_rect(
                    bottom.saturating_sub(4.max(alpha)),
                    top,
                    4.max(alpha),
                    view_h.saturating_sub(4),
                    flash,
                );
            }
        }

        if completed && self.result != GameResult::Playing {
            self.render_result_menu(fb, win_x, view_y, view_h, win_w);
        }

        completed
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let _ = self.render_content_with_budget(fb, win_x, win_y, win_w, win_h, u64::MAX);
    }

    fn render_result_menu(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        view_y: usize,
        view_h: usize,
        win_w: usize,
    ) {
        if self.result == GameResult::Playing {
            return;
        }
        let panel_w = win_w.min(360);
        let panel_h = if self.result == GameResult::Menu {
            154
        } else {
            104
        };
        let panel_x = win_x + win_w.saturating_sub(panel_w) / 2;
        let panel_y = view_y + view_h.saturating_sub(panel_h) / 2;
        fb.draw_rounded_rect_alpha(panel_x, panel_y, panel_w, panel_h, 8, 0x080B12, 238);
        let (title, color) = match self.result {
            GameResult::Menu => (
                crate::gui::ui_text("FPS 3D OYUN", "FPS 3D GAME"),
                Color::ACCENT_YELLOW,
            ),
            GameResult::Won => (
                crate::gui::ui_text("KAZANDIN!", "YOU WIN!"),
                Color::ACCENT_GREEN,
            ),
            GameResult::Lost => (
                crate::gui::ui_text("GAME OVER!", "GAME OVER!"),
                Color::ACCENT_RED,
            ),
            GameResult::Playing => ("", Color::TEXT_MAIN),
        };
        let title_x = panel_x + panel_w.saturating_sub(MinGuiCompositor::text_width(title, 1)) / 2;
        MinGuiCompositor::draw_string(fb, title_x, panel_y + 18, title, color);
        match self.result {
            GameResult::Menu => {
                let controls = [
                    crate::gui::ui_text(
                        "WASD: Hareket  |  Fare: Bakis",
                        "WASD: Move  |  Mouse: Look",
                    ),
                    crate::gui::ui_text("Sol tik / BOS: Ates", "Left click / SPACE: Shoot"),
                    crate::gui::ui_text("T: 2D / 3D gorunum", "T: Toggle 2D / 3D view"),
                    crate::gui::ui_text("BOS/ENTER/Tikla: Basla", "SPACE/ENTER/Click: Start"),
                ];
                for (i, text) in controls.iter().enumerate() {
                    let text_x =
                        panel_x + panel_w.saturating_sub(MinGuiCompositor::text_width(text, 1)) / 2;
                    MinGuiCompositor::draw_string(
                        fb,
                        text_x,
                        panel_y + 46 + i * 22,
                        text,
                        if i == controls.len() - 1 {
                            Color::ACCENT_YELLOW
                        } else {
                            Color::TEXT_MAIN
                        },
                    );
                }
            }
            GameResult::Won | GameResult::Lost => {
                let prompt = crate::gui::ui_text(
                    "R, BOS veya tikla: Menuye don",
                    "R, SPACE or click: Main menu",
                );
                let prompt_x =
                    panel_x + panel_w.saturating_sub(MinGuiCompositor::text_width(prompt, 1)) / 2;
                MinGuiCompositor::draw_string(fb, prompt_x, panel_y + 58, prompt, Color::TEXT_MAIN);
            }
            GameResult::Playing => {}
        }
    }

    /// Oyun şu anda oynanıyor mu (menüde değil)?
    pub fn is_playing(&self) -> bool {
        self.result == GameResult::Playing
    }

    /// Klavye girdisini işle. true dönerse tüketildi.
    pub fn handle_key(&mut self, ch: char, audio_dev: &mut AudioDevice) -> bool {
        if self.result == GameResult::Menu {
            match ch {
                ' ' | '\n' | '\r' => {
                    self.result = GameResult::Playing;
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
            }
        }
        if self.result != GameResult::Playing {
            if ch == 'r' || ch == 'R' || ch == ' ' {
                *self = Self::new();
                return true;
            }
            return false;
        }
        // Esc: ana menüye dön ve basılı tuşları temizle.
        if ch == '\x1b' {
            self.result = GameResult::Menu;
            self.key_mask = 0;
            self.shooting = false;
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
                // Ateş et (silah gecikmesi sayesinde hız sınırlı).
                if self.shoot_timer == 0 && !self.is_reloading {
                    let delay = if self.weapon == WEAPON_SHOTGUN {
                        SHOTGUN_FIRE_DELAY
                    } else {
                        PISTOL_FIRE_DELAY
                    };
                    self.shoot_timer = delay;
                    self.shooting = true;
                    self.muzzle_flash_timer = MUZZLE_FLASH_TICKS;
                    let hit = self.shoot();
                    if hit {
                        self.play_sound(audio_dev, SoundKind::Hit);
                    } else if self.ammo_per_weapon[self.weapon as usize] == 0 {
                        // Mermi bitti: otomatik yeniden doldurma.
                        self.start_reload();
                        self.play_sound(audio_dev, SoundKind::Click);
                    } else {
                        self.play_sound(audio_dev, SoundKind::Shoot);
                    }
                }
                true
            }
            '1' => {
                if self.shoot_timer == 0 && !self.is_reloading {
                    self.weapon = WEAPON_PISTOL;
                    self.ammo = self.ammo_per_weapon[WEAPON_PISTOL as usize];
                }
                true
            }
            '2' => {
                if self.shoot_timer == 0 && !self.is_reloading {
                    self.weapon = WEAPON_SHOTGUN;
                    self.ammo = self.ammo_per_weapon[WEAPON_SHOTGUN as usize];
                }
                true
            }
            'r' | 'R' => {
                // Yeniden doldurma.
                self.start_reload();
                true
            }
            'e' | 'E' => {
                // Kapı aç/kapat.
                // Sadece kapı tamamen kapalıysa (state 0) açılma sesi çal.
                // Açıkken basma kapatma işlemi yapar ama ses tekrar çalmaz.
                let was_closed = self
                    .doors
                    .iter()
                    .take(self.door_count)
                    .all(|d| d.state == 0);
                let toggled = self.toggle_door();
                if toggled && was_closed {
                    self.play_sound(audio_dev, SoundKind::Door);
                }
                true
            }
            't' | 'T' => {
                // Görünüm modunu değiştir: 3D <-> 2D üstten bakış.
                self.render_mode = match self.render_mode {
                    RenderMode::Mode3D => RenderMode::TopDown2D,
                    RenderMode::TopDown2D => RenderMode::Mode3D,
                };
                true
            }
            _ => false,
        }
    }

    /// Ok tuşu ile dönüş (tuş basımı kaydı, gerçek dönüş update() içinde).
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

    /// Ok tuşu ile ileri/geri hareket (tuş basımı kaydı, gerçek hareket update() içinde).
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

    /// Basılı tutulan klavye tuşlarını (`KeyboardState`) her karede senkronize eder.
    /// Böylece W/A/S/D ve ok tuşları tek tek basmak gerekmeden akıcı hareket sağlar.
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

    /// Fare hareketi ile bakış. dx = yatay, dy = dikey.
    pub fn handle_mouse_look(&mut self, dx: f32, dy: f32) {
        if self.result == GameResult::Playing {
            // Biriken büyük imleç sıçramaları oyunu çevirmesin; gerçek FPS
            // mouse-look hissi için tek karelik deltayı sınırlıyoruz.
            self.look(dx.clamp(-96.0, 96.0), dy.clamp(-64.0, 64.0));
        }
    }

    /// Fare butonunun mevcut durumunu oyun döngüsüne aktarır.
    pub fn set_mouse_button(&mut self, pressed: bool) {
        self.mouse_fire_held = pressed;
    }

    /// Fare tıklaması ile ateş.
    pub fn handle_mouse_click(&mut self, audio_dev: &mut AudioDevice) -> bool {
        if self.result == GameResult::Menu {
            self.result = GameResult::Playing;
            return true;
        }
        if self.result != GameResult::Playing {
            *self = Self::new();
            return true;
        }
        if self.shoot_timer == 0 && !self.is_reloading {
            let delay = if self.weapon == WEAPON_SHOTGUN {
                SHOTGUN_FIRE_DELAY
            } else {
                PISTOL_FIRE_DELAY
            };
            self.shoot_timer = delay;
            self.shooting = true;
            self.muzzle_flash_timer = MUZZLE_FLASH_TICKS;
            let hit = self.shoot();
            if hit {
                self.play_sound(audio_dev, SoundKind::Hit);
            } else if self.ammo_per_weapon[self.weapon as usize] == 0 {
                self.start_reload();
                self.play_sound(audio_dev, SoundKind::Click);
            } else {
                self.play_sound(audio_dev, SoundKind::Shoot);
            }
        }
        true
    }

    /// Her frame'de çağrılan güncelleme.
    /// Güncelleme işlemini zaman bütçesiyle çalıştır. Bütçe aşımı veya iptal
    /// durumunda `false` döner; ana döngü bunu görebilir ve işlemi erken sonlandırabilir.
    pub fn update_with_budget(&mut self, audio_dev: &mut AudioDevice, budget_ms: u64) -> bool {
        let budget = TimeBudget::new(budget_ms);
        let mut cancelled = false;

        // Tuş maskesi her karede tek seferde tüketilir. Önceki sürümde
        // `key_mask = 0` yalnızca bütçe dolmadığında çalışıyordu; iptal
        // durumunda basılı bitler sonraki karelere "takılı" kalıyor ve
        // oyuncu tuşu bırakmasına rağmen hareket etmeye devam ediyordu.
        let key_mask = core::mem::take(&mut self.key_mask);

        if self.result != GameResult::Playing {
            return true;
        }

        // Ateş tuşunu her fizik karesinde değerlendir. Böylece oyuncu
        // tıklamayı sürdürürken silah gecikmesi kadar hızlı ve kesintisiz ateş
        // eder; tek tıklama davranışı da korunur.
        if self.mouse_fire_held && self.shoot_timer == 0 && !self.is_reloading {
            let delay = if self.weapon == WEAPON_SHOTGUN {
                SHOTGUN_FIRE_DELAY
            } else {
                PISTOL_FIRE_DELAY
            };
            self.shoot_timer = delay;
            self.shooting = true;
            self.muzzle_flash_timer = MUZZLE_FLASH_TICKS;
            let hit = self.shoot();
            if hit {
                self.play_sound(audio_dev, SoundKind::Hit);
            } else if self.ammo_per_weapon[self.weapon as usize] == 0 {
                self.start_reload();
                self.play_sound(audio_dev, SoundKind::Click);
            } else {
                self.play_sound(audio_dev, SoundKind::Shoot);
            }
        }

        // --- Çok tuşlu hareket ve dönüş ---
        if key_mask != 0 {
            if budget.is_expired() {
                cancelled = true;
            } else {
                let move_speed = if key_mask & KEY_SHIFT != 0 {
                    SPRINT_SPEED
                } else {
                    WALK_SPEED
                };
                // İstenen hareket yönünü hesapla.
                let mut target_dx = 0.0f32;
                let mut target_dy = 0.0f32;

                if key_mask & KEY_W != 0 {
                    target_dx += self.dir_x * move_speed;
                    target_dy += self.dir_y * move_speed;
                }
                if key_mask & KEY_S != 0 {
                    target_dx -= self.dir_x * move_speed;
                    target_dy -= self.dir_y * move_speed;
                }
                if key_mask & KEY_A != 0 {
                    target_dx += self.dir_y * move_speed;
                    target_dy -= self.dir_x * move_speed;
                }
                if key_mask & KEY_D != 0 {
                    target_dx -= self.dir_y * move_speed;
                    target_dy += self.dir_x * move_speed;
                }

                let mut forward = 0.0f32;
                if key_mask & KEY_UP != 0 {
                    forward += move_speed;
                }
                if key_mask & KEY_DOWN != 0 {
                    forward -= move_speed;
                }
                target_dx += forward * self.dir_x;
                target_dy += forward * self.dir_y;

                // Hız vektörünü büyüklüğüne göre sınırla. Eski kontrol
                // `dx != 0 && dy != 0` koşuluyla köşegen normalize
                // ediyordu; bu yüzden çapraz bir yöne bakarken düz ileri
                // hareket ~%29 yavaşlıyordu ("ilerleyemiyor" hissi).
                // Artık bileşen sayısından bağımsız olarak hız asla
                // move_speed'i aşmaz.
                let len_sq = target_dx * target_dx + target_dy * target_dy;
                if len_sq > move_speed * move_speed {
                    let inv = move_speed / fast_sqrt(len_sq);
                    target_dx *= inv;
                    target_dy *= inv;
                }

                // Standart FPS kontrolü: hız hedefe anında eşitlenir.
                // Eski ivme+sürtünme modeli (~0.4 sn ivmelenme, ~0.5 sn
                // kayma) "tuş bırakınca hâlâ hareket" ve "ağır his"
                // şikayetinin kaynağıydı.
                self.vel_x = target_dx;
                self.vel_y = target_dy;

                // Hareketi uygula (çarpışma kontrolü ile).
                if self.vel_x != 0.0 || self.vel_y != 0.0 {
                    self.move_player(self.vel_x, self.vel_y);
                }
                self.moving = self.vel_x != 0.0 || self.vel_y != 0.0;

                if key_mask & KEY_LEFT != 0 {
                    self.rotate(-ROT_SPEED);
                }
                if key_mask & KEY_RIGHT != 0 {
                    self.rotate(ROT_SPEED);
                }
            }
        } else {
            // Tuş basılı değilse hız anında sıfırlanır; artık kayma yok.
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.vel_x = 0.0;
                self.vel_y = 0.0;
                self.moving = false;
            }
        }

        // Ateş sayacı.
        if !cancelled && self.shoot_timer > 0 {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.shoot_timer -= 1;
                if self.shoot_timer == 0 {
                    self.shooting = false;
                    self.stop_sound(audio_dev);
                }
            }
        }

        // Adım sesi.
        if !cancelled && self.moving {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.last_step_tick = self.last_step_tick.saturating_add(1);
                if self.last_step_tick % 15 == 0 {
                    self.play_sound(audio_dev, SoundKind::Step);
                }
            }
        } else if !cancelled {
            self.last_step_tick = 0;
        }
        self.moving = false;

        // Kapı animasyonları.
        if !cancelled {
            for d in self.doors.iter_mut().take(self.door_count) {
                if budget.is_expired() {
                    cancelled = true;
                    break;
                }
                if d.state > 0 && d.state < 11 {
                    d.state = d.state.saturating_add(1);
                    if d.state >= 11 {
                        d.state = 11;
                    }
                } else if d.state > 11 {
                    d.state = d.state.saturating_sub(1);
                    if d.state <= 11 {
                        d.state = 0;
                    }
                }
            }
        }

        // Pickup toplama.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.collect_pickups();
            }
        }

        // Düşman AI'sı / dalga yönetimi (aşağıdaki blokta).
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                self.respawn_enemies();
            }
        }

        // Yeniden doldurma ve efekt zamanlayıcıları.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                if self.is_reloading {
                    self.reload_timer = self.reload_timer.saturating_sub(1);
                    if self.reload_timer == 0 {
                        self.finish_reload();
                        self.play_sound(audio_dev, SoundKind::Reload);
                    }
                }
                if self.muzzle_flash_timer > 0 {
                    self.muzzle_flash_timer -= 1;
                }
                if self.damage_flash_timer > 0 {
                    self.damage_flash_timer -= 1;
                }
                // Silah sallanma (bobbing) — harekette artar, duruşta gevşer.
                if self.moving {
                    self.weapon_bob = (self.weapon_bob + 0.18).clamp(0.0, TAU);
                } else {
                    self.weapon_bob = (self.weapon_bob * 0.85).max(0.0);
                }
            }
        }

        // Düşman yapay zekası: oyuncuyu takip eder, mesafeye göre saldırır.
        if !cancelled {
            if budget.is_expired() {
                cancelled = true;
            } else {
                for i in 0..self.enemy_count {
                    if budget.is_expired() {
                        cancelled = true;
                        break;
                    }
                    // Kopyala (Enemy: Copy), böylece self.enemies üzerinde
                    // uzun süreli mutable borrow tutulmaz.
                    let mut e = self.enemies[i];

                    if e.attack_timer > 0 {
                        e.attack_timer -= 1;
                    }

                    if !e.alive {
                        continue;
                    }

                    let dx = self.pos_x - e.x;
                    let dy = self.pos_y - e.y;
                    let dist = fast_sqrt(dx * dx + dy * dy);

                    let mut should_attack = false;

                    // 3B model yönü: düşman oyuncuya doğru bakar.
                    e.facing = fast_atan2(dy, dx);

                    let prev_x = e.x;
                    let prev_y = e.y;
                    if dist > ENEMY_ATTACK_DIST && dist < ENEMY_CHASE_RADIUS {
                        let step = e.speed;
                        let nx = e.x + (dx / dist) * step;
                        let ny = e.y + (dy / dist) * step;

                        // Düşman duvarı aşmaya çalışır: önce doğrudan hareket,
                        // engel varsa eksen bazlı kayma (duvar boyunca sürünme).
                        let mut moved = false;
                        if !self.is_wall(nx, e.y) {
                            e.x = nx;
                            moved = true;
                        } else if !self.is_wall(e.x, ny) {
                            // X ekseni engelliyse Y ekseninde dene (duvar boyunca kayma).
                            e.y = ny;
                            moved = true;
                        } else if !self.is_wall(nx, ny) {
                            // İkisi de engelse köşegen dene.
                            e.x = nx;
                            e.y = ny;
                            moved = true;
                        }
                        // Hiçbiri mümkün değilse düşman yerinde bekler (duvarın arkasında takılır).
                        let _ = moved;
                    } else if dist <= ENEMY_ATTACK_DIST && e.attack_timer == 0 {
                        should_attack = true;
                    }

                    // Yürüyüş animasyonu fazı: hareket ettikçe ilerler.
                    if e.x != prev_x || e.y != prev_y {
                        e.walk_phase = (e.walk_phase + 0.28) % TAU;
                    }

                    // Değişiklikleri geri yaz.
                    self.enemies[i] = e;

                    if should_attack {
                        self.enemies[i].attack_timer = ENEMY_ATTACK_DELAY;
                        let dmg = ENEMY_ATTACK_DAMAGE;
                        let mut armor_dmg = (dmg as f32 * 0.5) as u32;
                        armor_dmg = armor_dmg.min(self.armor);
                        let health_dmg = dmg.saturating_sub(armor_dmg);
                        self.armor = self.armor.saturating_sub(armor_dmg);
                        self.health = self.health.saturating_sub(health_dmg);
                        self.damage_flash_timer = DAMAGE_FLASH_TICKS;
                        self.play_sound(audio_dev, SoundKind::EnemyDamage);
                    }
                }
            }
        }

        // Dalga sistemi: tüm düşmanlar öldüyse bir sonraki dalgayı başlat.
        if !cancelled {
            let all_dead = self
                .enemies
                .iter()
                .take(self.enemy_count)
                .all(|enemy| !enemy.alive);
            if all_dead {
                if self.wave_timer == 0 {
                    self.wave = self.wave.saturating_add(1);
                    self.wave_timer = 180;

                    // Dalga zorluğu: ilerleyen dallarda daha fazla ve daha hızlı düşman.
                    let wave = self.wave;
                    let enemy_speed = ENEMY_CHASE_SPEED + (wave as f32 * 0.005).clamp(0.0, 0.02);
                    let spawn_count = (8 + wave * 2).clamp(8, MAX_ENEMIES);

                    let spawn_points = [
                        (2.5, 2.5),
                        (17.5, 2.5),
                        (17.5, 5.5),
                        (2.5, 17.5),
                        (9.5, 17.5),
                        (15.5, 17.5),
                        (2.5, 9.5),
                        (9.5, 14.5),
                        (14.5, 9.5),
                        (5.5, 14.5),
                        (14.5, 5.5),
                        (5.5, 5.5),
                    ];
                    for (i, e) in self.enemies.iter_mut().take(self.enemy_count).enumerate() {
                        if !e.alive && i < spawn_count {
                            // Oyuncuya en uzak doğma noktasını seç ki üstüne binmesinler.
                            let mut best = spawn_points[i % spawn_points.len()];
                            let mut best_d = -1.0_f32;
                            for &(cx, cy) in spawn_points.iter() {
                                let ddx = cx - self.pos_x;
                                let ddy = cy - self.pos_y;
                                let d = ddx * ddx + ddy * ddy;
                                if d > best_d {
                                    best_d = d;
                                    best = (cx, cy);
                                }
                            }
                            let (sx, sy) = best;
                            // Her 5. dalgada "boss" düşman (tür 2 = iri, daha fazla can).
                            let is_boss = wave % 5 == 0 && i == 0;
                            *e = Enemy {
                                x: sx,
                                y: sy,
                                alive: true,
                                hit_timer: 0,
                                attack_timer: 0,
                                facing: fast_atan2(self.pos_y - sy, self.pos_x - sx),
                                walk_phase: 0.0,
                                kind: if is_boss {
                                    2 // iri boss düşman
                                } else {
                                    ((wave + i) % ENEMY_KINDS.len()) as u8
                                },
                                speed: enemy_speed,
                                is_boss,
                            };
                            if is_boss {
                                // walk_phase üzerinden boss bayrağı saklayabiliriz,
                                // ama şimdilik sadece görsel olarak farklıdır (tür 2).
                            }
                        }
                    }
                } else {
                    self.wave_timer -= 1;
                }
            }
        }

        if !cancelled {
            if self.health == 0 {
                crate::boot::console::serial_print("[FPS-GAME] Oyun sonu: Can tükendi (Lost)\n");
                self.result = GameResult::Lost;
            } else if self.wave >= 15 {
                crate::boot::console::serial_print(
                    "[FPS-GAME] Oyun sonu: 15 dalga geçildi (Won)\n",
                );
                self.result = GameResult::Won;
            }
        }

        !cancelled
    }

    /// Geriye uyumlu: bütçe kontrolü olmadan güncelleme yapar.
    pub fn update(&mut self, audio_dev: &mut AudioDevice) {
        let _ = self.update_with_budget(audio_dev, u64::MAX);
    }
}

impl Default for FpsGameApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Ses efekt türleri.
#[derive(Clone, Copy, PartialEq)]
pub enum SoundKind {
    Shoot,
    Hit,
    Click,
    Reload,
    Step,
    Door,
    Pickup,
    EnemyDamage,
}

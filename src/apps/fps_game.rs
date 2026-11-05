//! FPS tarzli performans testi oyunu.
//! 3D ışın izleme (raycasting) motoru, fare ile bakis, WASD hareket,
//! uzaklasma gürültüsüyle gölgelendirme, düşman hedefleri ve ses efektleri.
//! no_std + alloc ortamında çalışır, çekirdek audio cihazı üzerinden ses üretir.

use crate::drivers::audio::AudioDevice;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use core::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// no_std için hızlı trigonometrik ve karekök fonksiyonları
// ---------------------------------------------------------------------------

const HALF_PI: f32 = PI / 2.0;

/// Taylor serisi ile sinüs (viewer3d ile aynı).
#[inline(always)]
fn fast_sin(mut x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    // Açı normalizasyonu mutlaka sonlu kalsın. Bozuk/taşmış bir değer için
    // `x -= TAU` sonsuz döngüye girebilirdi (özellikle uzun süreli dönüşte).
    for _ in 0..8 {
        if x <= PI {
            break;
        }
        x -= TAU;
    }
    for _ in 0..8 {
        if x >= -PI {
            break;
        }
        x += TAU;
    }
    let x2 = x * x;
    let x3 = x * x2;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
}

#[inline(always)]
fn fast_cos(x: f32) -> f32 {
    fast_sin(x + HALF_PI)
}

/// Newton-Raphson karekök.
#[inline(always)]
fn fast_sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
    for _ in 0..20 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// atan2 yaklaşımı.
#[inline(always)]
fn fast_atan2(y: f32, x: f32) -> f32 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    if x == 0.0 {
        if y > 0.0 {
            return HALF_PI;
        }
        if y < 0.0 {
            return -HALF_PI;
        }
        return 0.0;
    }
    let mut atan = (y / x).atan_approx();
    if x < 0.0 {
        if y >= 0.0 {
            atan += PI;
        } else {
            atan -= PI;
        }
    }
    atan
}

/// f32 için atan yakınlığı (polinom).
trait AtanApprox {
    fn atan_approx(self) -> f32;
}
impl AtanApprox for f32 {
    fn atan_approx(self) -> f32 {
        let x = self.abs();
        let x2 = x * x;
        // x/(1+x^2) * (1 + x^2/3 + x^4/5 + ...) kısaltılmış seri
        let result = x / (1.0 + x2) * (1.0 + x2 * (0.3333333 + x2 * (0.2 + x2 * 0.1428571)));
        if self < 0.0 {
            -result
        } else {
            result
        }
    }
}

// ---------------------------------------------------------------------------
// Sabitler ve yardımcı matematik
// ---------------------------------------------------------------------------

const MAP_W: usize = 16;
const MAP_H: usize = 16;
const FOV: f32 = PI / 3.0; // 60 derece
const MAX_DEPTH: f32 = 16.0;
const MOVE_SPEED: f32 = 0.14;
const ROT_SPEED: f32 = 0.10;
// Ana döngü fare delta'sını yarım ölçekle gönderiyor; bu değer 1:1'e yakın,
// daha kontrollü bir FPS dönüşü için ayarlandı.
const MOUSE_SENS: f32 = 0.0045;
const PLAYER_RADIUS: f32 = 0.25;

// Tuş durumu maske bayrakları (handle_key / handle_arrow ile ayarlanır,
// update() içinde tek seferde uygulanır — çok tuşlu hareket destekler).
const KEY_W: u16 = 1 << 0;
const KEY_S: u16 = 1 << 1;
const KEY_A: u16 = 1 << 2;
const KEY_D: u16 = 1 << 3;
const KEY_LEFT: u16 = 1 << 4;
const KEY_RIGHT: u16 = 1 << 5;

/// Harita kare türleri: 0 = boş, 1 = duvar, 2 = kapı, 3 = canlı/pickup.
const TILE_WALL: u8 = 1;
const TILE_DOOR: u8 = 2;
const TILE_PICKUP: u8 = 3;

/// Harita: Wolfenstein 3D tarzı bölüm.
static MAP: [[u8; MAP_W]; MAP_H] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 1, 2, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1],
    [1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 0, 2, 1, 0, 2, 1, 1, 1, 0, 1, 0, 0, 1],
    [1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
    [1, 0, 1, 0, 2, 0, 1, 1, 2, 0, 1, 1, 1, 1, 0, 1],
    [1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 2, 1, 0, 1, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1],
    [1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 0, 0, 2, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1],
    [1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
];

/// Düşman: (x, y, can, aktif)
#[derive(Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    alive: bool,
    hit_timer: usize,
}

const MAX_ENEMIES: usize = 8;

/// Pickup: (x, y, tür, aktif) — 1 = can, 2 = puan.
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
    Playing,
    Won,
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
    pub health: u32,
    pub score: u32,
    pub ammo: u32,
    pub shots_fired: u32,
    pub hits: u32,
    pub shooting: bool,
    pub shoot_timer: usize,
    result: GameResult,

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
    pub frame_count: usize,
    pub last_fps_tick: usize,
    pub fps: usize,

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
        }; MAX_ENEMIES];

        // Başlangıç düşmanlarını harita üzerindeki boş hücrelere yerleştir.
        let mut n = 0;
        for ty in 2..MAP_H - 1 {
            for tx in 2..MAP_W - 1 {
                if MAP[ty][tx] == 0 && n < MAX_ENEMIES && (tx + ty) % 3 == 0 {
                    enemies[n] = Enemy {
                        x: tx as f32 + 0.5,
                        y: ty as f32 + 0.5,
                        alive: true,
                        hit_timer: 0,
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
        for ty in 2..MAP_H - 1 {
            for tx in 2..MAP_W - 1 {
                if MAP[ty][tx] == 0 && pcount < MAX_PICKUPS && (tx * 2 + ty) % 5 == 0 {
                    pickups[pcount] = Pickup {
                        x: tx as f32 + 0.5,
                        y: ty as f32 + 0.5,
                        kind: if (tx + ty) % 2 == 0 { 1 } else { 2 },
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
        for ty in 0..MAP_H {
            for tx in 0..MAP_W {
                if MAP[ty][tx] == TILE_DOOR && dcount < doors.len() {
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
            plane_x: 0.0,
            plane_y: 0.66,
            health: 100,
            score: 0,
            ammo: 50,
            shots_fired: 0,
            hits: 0,
            shooting: false,
            shoot_timer: 0,
            result: GameResult::Playing,
            render_mode: RenderMode::Mode3D,
            enemies,
            enemy_count: n,
            pickups,
            pickup_count: pcount,
            doors,
            door_count: dcount,
            frame_count: 0,
            last_fps_tick: 0,
            fps: 0,
            last_shot_tick: 0,
            last_step_tick: 0,
            moving: false,
            key_mask: 0,
            sound_pending: None,
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

    /// Düşmana ateş et, mermi varsa en yakın canlı düşmanı vursa true döner.
    fn shoot(&mut self) -> bool {
        if self.ammo == 0 {
            return false;
        }
        self.ammo = self.ammo.saturating_sub(1);
        self.shots_fired += 1;
        let mut best_dist = f32::MAX;
        let mut best_idx: Option<usize> = None;

        for (i, e) in self.enemies.iter().enumerate().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            // Düşmanı görmek için görüş açısı içinde olmalı (basit kontrol).
            let angle = fast_atan2(dy, dx);
            let mut rel = angle - fast_atan2(self.dir_y, self.dir_x);
            while rel < -PI {
                rel += TAU;
            }
            while rel > PI {
                rel -= TAU;
            }
            if rel.abs() < FOV / 2.0 + 0.3 && dist < best_dist {
                best_dist = dist;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            self.enemies[idx].alive = false;
            self.enemies[idx].hit_timer = 10;
            self.hits += 1;
            self.score += 100;
            true
        } else {
            false
        }
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
    }

    /// Fare ile bakış açısını güncelle.
    fn look(&mut self, dx: f32) {
        self.rotate(-dx * MOUSE_SENS);
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
            SoundKind::Step => {
                audio_dev.set_pc_speaker_tone(200);
                self.sound_pending = Some((200, 30));
            }
            SoundKind::Door => {
                audio_dev.set_pc_speaker_tone(620);
                self.sound_pending = Some((620, 90));
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

    /// FPS sayacını güncelle.
    pub fn tick_fps(&mut self, tick: usize) {
        self.frame_count += 1;
        if tick.saturating_sub(self.last_fps_tick) >= 60 {
            // Yaklaşık saniyede bir (60 tick varsayımıyla).
            self.fps = self.frame_count.saturating_sub(self.last_fps_tick);
            self.last_fps_tick = tick;
            self.frame_count = 0;
        }
    }

    /// 3D sahneyi çizer (raycasting).
    pub fn render_3d(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        // Görünüm alanı render_content tarafından araç çubuğu ile HUD
        // arasında hesaplanıp verilir.
        let render_y = win_y;
        let render_h = win_h;
        let render_x = win_x;
        let render_w = win_w;
        if render_w < 8 || render_h < 8 {
            return;
        }

        // Z-buffer: her sütun için mesafe.
        let mut z_buffer: [f32; 1024] = [0.0; 1024];
        let w = render_w.min(1024);

        // Wolfenstein tarzı net iki bölge: tavan ve zemin birbirinden belirgin
        // olmalı; çok koyu arka plan duvarları ve perspektifi yutuyordu.
        let horizon = render_h / 2;
        for y in 0..horizon {
            let t = y as f32 / horizon.max(1) as f32;
            let r = (18.0 + 18.0 * t) as u32;
            let g = (28.0 + 18.0 * t) as u32;
            let b = (52.0 + 22.0 * t) as u32;
            fb.draw_rect(
                render_x,
                render_y + y,
                render_w,
                1,
                (r << 16) | (g << 8) | b,
            );
        }
        // Zemin: açık gri-kahverengi, yatay perspektif bantlarıyla okunaklı.
        for y in horizon..render_h {
            let t = (y - horizon) as f32 / horizon.max(1) as f32;
            let r = (92.0 - 42.0 * t) as u32;
            let g = (76.0 - 34.0 * t) as u32;
            let b = (58.0 - 26.0 * t) as u32;
            let color = (r << 16) | (g << 8) | b;
            fb.draw_rect(render_x, render_y + y, render_w, 1, color);
        }
        fb.draw_rect(render_x, render_y + horizon, render_w, 3, 0xD0A060);

        // Geniş pencerelerde her ikinci sütun aynı duvar geometrisini paylaşır.
        // Bu, DDA/f32 maliyetini yaklaşık yarıya indirirken küçük ekranlarda
        // tam çözünürlüklü görünümü korur.
        let column_step = if w >= 800 { 2 } else { 1 };
        for x in (0..w).step_by(column_step) {
            // Kamera x koordinatı (-1 ile 1 arası).
            let camera_x = 2.0 * (x as f32) / (w as f32) - 1.0;
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
            let perp_wall_dist: f32;

            // Harita bozuk olsa bile tek ray ana döngüyü kilitlemesin.
            let mut ray_steps = 0usize;
            while !hit && ray_steps < MAP_W + MAP_H + 4 {
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
                    // Kapılar açıkken de çizilir: koyu bir geçit + parlak
                    // yeşil kasayla "açık kapı" olarak okunur.
                    // Açık kapı artık ray için boş geçit: oyuncu çarpışması
                    // ile render aynı durumu paylaşmalı, aksi halde kapıdan
                    // geçilse bile karşı odanın duvarı çizilmeye devam eder.
                    if t == TILE_WALL || (t == TILE_DOOR && !self.door_is_open(map_x, map_y)) {
                        hit = true;
                    }
                }
            }

            // Duvara gerçek mesafe (fisheye düzeltmesi).
            if side == 0 {
                perp_wall_dist =
                    (map_x as f32 - self.pos_x + (1.0 - step_x as f32) / 2.0) / ray_dir_x;
            } else {
                perp_wall_dist =
                    (map_y as f32 - self.pos_y + (1.0 - step_y as f32) / 2.0) / ray_dir_y;
            }
            // Bozuk bir açı/mesafe hiçbir zaman dev bir çizim döngüsüne
            // dönüşmesin.
            let perp_wall_dist = if perp_wall_dist.is_finite() {
                perp_wall_dist.abs().max(0.15).min(MAX_DEPTH * 4.0)
            } else {
                MAX_DEPTH
            };

            for z in x..(x + column_step).min(w) {
                z_buffer[z] = perp_wall_dist;
            }

            // Duvar yüksekliği: standart raycasting projeksiyonu. Ekran
            // yüksekliği / mesafe; böylece yakın duvar büyük, uzak duvar
            // küçük görünür ve tavan/zemin görünür kalır.
            let line_h =
                (render_h as f32 / perp_wall_dist.max(0.15)).min(render_h as f32 * 4.0) as usize;
            let draw_start = (render_h / 2).saturating_sub(line_h / 2);
            let draw_end = (render_h / 2).saturating_add(line_h / 2).min(render_h);

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
            let int_part = tex_x_raw as i32 as f32;
            let mut tex_x = tex_x_raw - int_part;
            if tex_x < 0.0 {
                tex_x += 1.0;
            }
            let tx = (tex_x * 8.0) as usize % 8;

            // Duvar rengi (mesafeye göre gölgeli).
            // Yakındaki duvarlar canlı, uzaktakiler hâlâ ayırt edilebilir.
            let shade = (0.5 + 0.5 * (1.0 - (perp_wall_dist / MAX_DEPTH).clamp(0.0, 1.0))).min(1.0);
            let apply_shade = |color: u32| -> u32 {
                let r = (((color >> 16) & 0xFF) as f32 * shade) as u32;
                let g = (((color >> 8) & 0xFF) as f32 * shade) as u32;
                let b = ((color & 0xFF) as f32 * shade) as u32;
                (r << 16) | (g << 8) | b
            };

            // Dikey çizgi olarak duvarı çiz (tekstürlü).
            if draw_start < draw_end && draw_end > 0 && draw_start < render_h {
                let ds = draw_start.max(0).min(render_h);
                let de = draw_end.max(0).min(render_h);
                for yy in ds..de {
                    // Tekstür y koordinatı.
                    let ty = ((yy - ds) as f32 / (de - ds).max(1) as f32 * 8.0) as usize % 8;
                    let wall_color = match tile {
                        TILE_WALL => {
                            // Tuğla duvar: iki sıra, kaydırılmış dikey derzler
                            // ve üst kenarda ışık vurgusu ile net okunur.
                            let row = ty / 4;
                            let brick = match ty % 4 {
                                0 => 0x6F2A22,
                                1 => 0xC25443,
                                2 => 0xA8402F,
                                _ => 0x8C352A,
                            };
                            let joint_x = if row == 0 { 0 } else { 4 };
                            let color = if ty % 4 == 0 || tx == joint_x {
                                0x3D1714 // harç
                            } else {
                                brick
                            };
                            apply_shade(color)
                        }
                        TILE_DOOR => {
                            if self.door_is_open(map_x, map_y) {
                                // Açık kapı: koyu geçit + parlak yeşil kasa.
                                if tx == 0 || tx == 7 || ty == 0 || ty == 7 {
                                    apply_shade(0x45C766)
                                } else {
                                    apply_shade(0x07110E)
                                }
                            } else {
                                // Kapalı kapı: metalik camgöbeği panel,
                                // sarı tehlike şeritleri ve pirinç kol.
                                let color = if tx == 0 || tx == 7 || ty == 0 || ty == 7 {
                                    0x16323A // kasa
                                } else if tx == 4 && (ty == 3 || ty == 4) {
                                    0xFFD24A // kol
                                } else if (ty == 2 || ty == 4) && (tx == 2 || tx == 5) {
                                    0xE0A92E // tehlike şeridi
                                } else if tx % 2 == 0 {
                                    0x2E6B78
                                } else {
                                    0x377C8B
                                };
                                apply_shade(color)
                            }
                        }
                        TILE_PICKUP => apply_shade(0x40E040),
                        _ => apply_shade(0x604040),
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
                    fb.draw_rect(
                        render_x + x,
                        render_y + yy,
                        column_step.min(w - x),
                        1,
                        wall_color,
                    );
                }
            }

            // Düşman sprite'ları (basit kare, kamera yönüne göre).
            // Bu basit sürümde düşmanlar 3D olarak çizilmez, yalnızca radar üzerinden gösterilir.
        }

        // Düşman sprite'larını çiz (basit billboard).
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            if dist > MAX_DEPTH || dist < 0.5 {
                continue;
            }
            let angle = fast_atan2(dy, dx);
            let mut rel = angle - fast_atan2(self.dir_y, self.dir_x);
            while rel < -PI {
                rel += TAU;
            }
            while rel > PI {
                rel -= TAU;
            }
            if rel.abs() > FOV / 2.0 + 0.5 {
                continue;
            }

            // Ekran x pozisyonu.
            let screen_x = (rel / (FOV / 2.0) + 1.0) / 2.0 * (w as f32);
            let sx = screen_x as usize;
            if sx >= w {
                continue;
            }

            // Mesafeye göre boyut: duvar yüksekliğinin yaklaşık yarısı
            // kadar bir insan silueti.
            let sprite_h = (render_h as f32 / dist.max(0.3) * 0.45) as usize;
            let sprite_w = (sprite_h / 2).clamp(8, 96);
            let draw_start_y = (render_h / 2).saturating_sub(sprite_h / 2);
            let draw_end_y = (render_h / 2).saturating_add(sprite_h / 2).min(render_h);

            // Z-buffer testi: duvarın önündeyse çiz. Sprite'ı tek sütun yerine
            // gerçek bir billboard olarak çizmek düşmanları okunabilir kılar.
            if dist < z_buffer[sx] {
                let color = if e.hit_timer > 0 {
                    Color::ACCENT_RED
                } else {
                    Color::ACCENT_PINK
                };
                let cx = render_x + sx;
                let cy0 = render_y + draw_start_y;
                let cy1 = render_y + draw_end_y;
                let sw = sprite_w;
                for yy in cy0..cy1 {
                    for xx in 0..sw {
                        let px = cx.saturating_sub(sw / 2) + xx;
                        if px < render_x + w {
                            let rel_y = yy.saturating_sub(cy0) as f32
                                / (cy1.saturating_sub(cy0).max(1)) as f32;
                            let is_head = rel_y < 0.28;
                            let edge = xx < 2 || xx + 2 >= sw;
                            let pixel_color = if edge {
                                0x190F22
                            } else if is_head {
                                0xF08A7C
                            } else {
                                color
                            };
                            fb.put_pixel(px, yy, pixel_color);
                        }
                    }
                }
                // Kontrastlı gözler/nişan alınabilir merkez.
                let eye_y = cy0 + (cy1.saturating_sub(cy0) / 5);
                fb.draw_rect(cx.saturating_sub(sw / 4), eye_y, 3, 3, 0xFFF1C1);
                fb.draw_rect(cx + sw / 4, eye_y, 3, 3, 0xFFF1C1);
            }
        }

        // Vurma efekti: tüm görüntüyü piksel piksel yeniden okumak pahalıydı.
        // Bunun yerine ekran kenarında küçük bir hasar/ateş çerçevesi çizilir.
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
        for ty in 0..MAP_H {
            for tx in 0..MAP_W {
                let x = offset_x + tx * cell;
                let y = offset_y + ty * cell;
                let t = MAP[ty][tx];
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
            let color = if p.kind == 1 { 0x40FF40 } else { 0xFFFF40 };
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

        for ty in 0..MAP_H {
            for tx in 0..MAP_W {
                let t = MAP[ty][tx];
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

        // Puan.
        let mut score_text = crate::gui::TextBuf::new();
        score_text.push_str(crate::gui::ui_text("Puan: ", "Score: "));
        score_text.push_u2(self.score);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 120,
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
            win_x + 160,
            hud_y + 4,
            ammo_text.as_str(),
            Color::ACCENT_YELLOW,
        );

        // FPS.
        let mut fps_text = crate::gui::TextBuf::new();
        fps_text.push_str(crate::gui::ui_text("FPS: ", "FPS: "));
        fps_text.push_u2(self.fps as u32);
        MinGuiCompositor::draw_string(
            fb,
            win_x + 220,
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
            win_x + 300,
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
            win_x + 380,
            hud_y + 4,
            en_text.as_str(),
            Color::TEXT_MUTED,
        );
    }

    /// Ana çizim girişi.
    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        // Araç çubuğu — mod başlığı.
        let mode_label = match self.render_mode {
            RenderMode::Mode3D => {
                crate::gui::ui_text("FPS 3D (T=2D Üstten)", "FPS 3D (T=Top-Down 2D)")
            }
            RenderMode::TopDown2D => {
                crate::gui::ui_text("FPS 2D Üstten (T=3D)", "FPS 2D Top-Down (T=3D)")
            }
        };
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

        // Görünüm moduna göre sahne çiz.
        match self.render_mode {
            RenderMode::Mode3D => {
                self.render_3d(fb, win_x, view_y, win_w, view_h);
                self.render_radar(fb, win_x, view_y, win_w);
            }
            RenderMode::TopDown2D => {
                self.render_2d_topdown(fb, win_x, view_y, win_w, view_h);
            }
        }

        // HUD.
        self.render_hud(fb, win_x, win_y, win_w, win_h);

        // 3D modunda yalnızca crosshair ve silah çiz.
        if self.render_mode == RenderMode::Mode3D {
            // Merkez işaretçi (crosshair) — büyük ve net.
            let cx = win_x + win_w / 2;
            let cy = view_y + view_h / 2;
            // Dikey çizgi.
            fb.draw_rect(cx.saturating_sub(1), cy.saturating_sub(6), 3, 13, 0xFFFF00);
            // Yatay çizgi.
            fb.draw_rect(cx.saturating_sub(6), cy.saturating_sub(1), 13, 3, 0xFFFF00);
            // Merkez nokta.
            fb.draw_rect(cx.saturating_sub(1), cy.saturating_sub(1), 3, 3, 0xFFFFFF);

            // Daha okunaklı bir silah silüeti: gövde, sürgü, namlu ve kabza.
            let gun_w = 92;
            let gun_h = 58;
            let gun_x = win_x + win_w / 2 - gun_w / 2;
            let gun_y = win_y + win_h.saturating_sub(gun_h + 10);
            let recoil = if self.shooting && self.shoot_timer > 0 {
                (10 - self.shoot_timer) * 2
            } else {
                0
            };
            let gy = gun_y.saturating_add(recoil);
            // Silah gövdesi.
            fb.draw_rect(gun_x + 20, gy + 8, gun_w - 38, 25, 0x252A33);
            fb.draw_rect(gun_x + 25, gy + 5, gun_w - 48, 6, 0x77808B);
            fb.draw_rect(gun_x + gun_w / 2 - 5, gy, 10, 10, 0xAEB8C2);
            // Silah kabzası.
            fb.draw_rect(gun_x + 30, gy + 28, 28, 25, 0x4B3030);
            fb.draw_rect(gun_x + 34, gy + 32, 20, 21, 0x6A3B36);
            fb.draw_rect(gun_x + 18, gy + 30, 12, 7, 0x171A20);
            // Mermi sayacı (silah üzerinde).
            if self.ammo > 0 {
                fb.draw_rect(gun_x + gun_w - 15, gy + 5, 10, 10, 0xFFD700);
            }
            // Ateş flaşı.
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
                "ATES!",
                Color::WHITE,
            );
        }
        self.render_result_menu(fb, win_x, view_y, view_h, win_w);
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
        let panel_h = 92;
        let panel_x = win_x + win_w.saturating_sub(panel_w) / 2;
        let panel_y = view_y + view_h.saturating_sub(panel_h) / 2;
        fb.draw_rounded_rect_alpha(panel_x, panel_y, panel_w, panel_h, 8, 0x080B12, 238);
        let (title, color) = match self.result {
            GameResult::Won => (
                crate::gui::ui_text("KAZANDIN!", "YOU WIN!"),
                Color::ACCENT_GREEN,
            ),
            GameResult::Playing => ("", Color::TEXT_MAIN),
        };
        MinGuiCompositor::draw_string(fb, panel_x + 58, panel_y + 20, title, color);
        MinGuiCompositor::draw_string(
            fb,
            panel_x + 58,
            panel_y + 52,
            crate::gui::ui_text("R veya BOS: Yeniden baslat", "R or SPACE: Restart"),
            Color::TEXT_MAIN,
        );
    }

    /// Klavye girdisini işle. true dönerse tüketildi.
    pub fn handle_key(&mut self, ch: char, audio_dev: &mut AudioDevice) -> bool {
        if self.result != GameResult::Playing {
            if ch == 'r' || ch == 'R' || ch == ' ' {
                *self = Self::new();
                return true;
            }
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
                // Ateş et.
                if self.shoot_timer == 0 {
                    self.shoot_timer = 10;
                    self.shooting = true;
                    let hit = self.shoot();
                    if hit {
                        self.play_sound(audio_dev, SoundKind::Hit);
                    } else {
                        self.play_sound(audio_dev, SoundKind::Shoot);
                    }
                }
                true
            }
            'r' | 'R' => {
                self.respawn_enemies();
                true
            }
            'e' | 'E' => {
                // Kapı aç/kapat.
                if self.toggle_door() {
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
        if left {
            self.key_mask |= KEY_LEFT;
        }
        if right {
            self.key_mask |= KEY_RIGHT;
        }
    }

    /// Fare hareketi ile bakış.
    pub fn handle_mouse_look(&mut self, dx: f32) {
        self.look(dx);
    }

    /// Fare tıklaması ile ateş.
    pub fn handle_mouse_click(&mut self, audio_dev: &mut AudioDevice) -> bool {
        if self.shoot_timer == 0 {
            self.shoot_timer = 10;
            self.shooting = true;
            let hit = self.shoot();
            if hit {
                self.play_sound(audio_dev, SoundKind::Hit);
            } else {
                self.play_sound(audio_dev, SoundKind::Shoot);
            }
        }
        true
    }

    /// Her frame'de çağrılan güncelleme.
    pub fn update(&mut self, audio_dev: &mut AudioDevice) {
        if self.result != GameResult::Playing {
            return;
        }

        // --- Çok tuşlu hareket ve dönüş (update içinde tek seferde uygulanır) ---
        if self.key_mask != 0 {
            let mut dx = 0.0f32;
            let mut dy = 0.0f32;

            if self.key_mask & KEY_W != 0 {
                dx += self.dir_x * MOVE_SPEED;
                dy += self.dir_y * MOVE_SPEED;
            }
            if self.key_mask & KEY_S != 0 {
                dx -= self.dir_x * MOVE_SPEED;
                dy -= self.dir_y * MOVE_SPEED;
            }
            if self.key_mask & KEY_A != 0 {
                // Sola strafe: sağ vektör (-dir_y, dir_x)nin tersi.
                dx += self.dir_y * MOVE_SPEED;
                dy -= self.dir_x * MOVE_SPEED;
            }
            if self.key_mask & KEY_D != 0 {
                // Sağa strafe.
                dx -= self.dir_y * MOVE_SPEED;
                dy += self.dir_x * MOVE_SPEED;
            }

            // Köşegen hareketi normalize et (√2 hız artışını önle).
            if dx != 0.0 && dy != 0.0 {
                let inv_sqrt2 = 0.70710678;
                dx *= inv_sqrt2;
                dy *= inv_sqrt2;
            }

            self.move_player(dx, dy);
            self.moving = dx != 0.0 || dy != 0.0;

            // Dönüş (sola / sağa) — hareketle aynı anda çalışabilir.
            if self.key_mask & KEY_LEFT != 0 {
                self.rotate(-ROT_SPEED);
            }
            if self.key_mask & KEY_RIGHT != 0 {
                self.rotate(ROT_SPEED);
            }

            self.key_mask = 0;
        }

        // Ateş sayacı.
        if self.shoot_timer > 0 {
            self.shoot_timer -= 1;
            if self.shoot_timer == 0 {
                self.shooting = false;
                self.stop_sound(audio_dev);
            }
        }

        // Adım sesi (hareket sırasında periyodik).
        if self.moving {
            self.last_step_tick = self.last_step_tick.saturating_add(1);
            if self.last_step_tick % 15 == 0 {
                self.play_sound(audio_dev, SoundKind::Step);
            }
        } else {
            self.last_step_tick = 0;
        }
        self.moving = false;

        // Kapı animasyonları (açılıyor/kapalıyor).
        for d in self.doors.iter_mut().take(self.door_count) {
            if d.state > 0 && d.state < 11 {
                d.state = d.state.saturating_add(1);
                if d.state >= 11 {
                    d.state = 11; // tamamen açık
                }
            } else if d.state > 11 {
                d.state = d.state.saturating_sub(1);
                if d.state <= 11 {
                    d.state = 0; // tamamen kapalı
                }
            }
        }

        // Pickup toplama.
        self.collect_pickups();

        // Düşmanları yenile.
        self.respawn_enemies();

        if self.health == 0 {
            // Can bittiğinde oyunu kilitleme: oyuncuyu yeniden dogur ve
            // oyuna devam et.
            self.respawn_player();
        } else if self
            .enemies
            .iter()
            .take(self.enemy_count)
            .all(|enemy| !enemy.alive)
        {
            self.result = GameResult::Won;
        }
    }

    /// Can bittiğinde oyuncuyu yeniden dogur; oyunu kilitlemez.
    fn respawn_player(&mut self) {
        self.health = 100;
        self.pos_x = 3.5;
        self.pos_y = 3.5;
        self.dir_x = 1.0;
        self.dir_y = 0.0;
        self.plane_x = 0.0;
        self.plane_y = 0.66;
        self.shooting = false;
        self.shoot_timer = 0;
        self.result = GameResult::Playing;
    }
}

/// Ses efekt türleri.
#[derive(Clone, Copy, PartialEq)]
pub enum SoundKind {
    Shoot,
    Hit,
    Step,
    Door,
}

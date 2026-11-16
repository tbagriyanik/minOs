//! 3D şehir sürüş oyunu.
//!
//! Raycasting tabanlı şehir motoru: ızgara düzeninde cadde, kaldırım, park ve
//! farklı yükseklikte binalar; direksiyon/fram/hız fiziği; trafik araçları,
//! toplanabilir hedefler, mini harita, hız göstergesi ve motor sesi.
//!
//! `no_std` + `alloc` ortamında çalışır; çizim framebuffer üzerine yapılır,
//! ses çekirdek ses cihazı (PC speaker / HDA) üzerinden üretilir.

use alloc::vec::Vec;

use crate::drivers::audio::AudioDevice;
use crate::drivers::keyboard::KeyAction;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use crate::time_budget::TimeBudget;

// ---------------------------------------------------------------------------
// Sabitler
// ---------------------------------------------------------------------------

/// Şehir ızgara genişliği (hücre).
pub const CITY_W: usize = 64;
/// Şehir ızgara yüksekliği (hücre).
pub const CITY_H: usize = 64;

/// Cadde periyodu: her 8 hücrede bir cadde koridoru.
const ROAD_PERIOD: usize = 8;
/// Cadde koridorunun genişliği (hücre).
const ROAD_HALF: usize = 2;
/// Bir binanın kat yüksekliği (dünya birimi).
const STOREY_H: f32 = 2.6;
/// Sürücü gözü / kamera yüksekliği (dünya birimi).
const CAM_H: f32 = 1.25;
/// Aracın çarpışma yarıçapı.
const CAR_RADIUS: f32 = 0.42;
/// Azami ileri hız (dünya birimi/s).
const MAX_SPEED: f32 = 24.0;
/// Azami geri hız (dünya birimi/s).
const MAX_REVERSE: f32 = 7.0;
/// Gaz ivmesi.
const ACCEL: f32 = 8.5;
/// Fren ivmesi.
const BRAKE: f32 = 14.0;
/// Doğrusal sürtünme katsayısı.
const DRAG: f32 = 0.9;
/// Azami direksiyon açısı hızı (rad/s).
const TURN_RATE: f32 = 1.9;
/// Görüş uzaklığı (dünya birimi).
const MAX_VIEW: f32 = 70.0;
/// Kamera düzlem uzunluğu (görüş açısı).
const PLANE_LEN: f32 = 0.72;

/// Trafik aracı sayısı.
const MAX_TRAFFIC: usize = 10;
/// Toplanabilir hedef (kubbe) sayısı.
const MAX_ORBS: usize = 10;
/// Render bütçesi (ms).
const RENDER_BUDGET_MS: u64 = 80;

// Hücre türleri.
const T_ROAD: u8 = 0;
const T_SIDEWALK: u8 = 1;
const T_BUILDING: u8 = 2;
const T_PARK: u8 = 3;

// Zemin renkleri.
const ROAD_COLOR: u32 = 0x3B4046;
const SIDEWALK_COLOR: u32 = 0x8C9199;
const PARK_COLOR: u32 = 0x2F6B36;
const LANE_COLOR: u32 = 0xE8C55A;
const WINDOW_COLOR: u32 = 0x8FC4DC;
const WINDOW_LIT_COLOR: u32 = 0xF0DFA0;

/// Bina tonları (gölgelendirilerek kullanılır).
const BUILDING_TONES: [u32; 6] = [0x8A8F99, 0x6E7A8C, 0x9A8E7C, 0x7E8C7A, 0x8C7E8E, 0x9EA3AC];

// Tuş maske bayrakları.
const KEY_UP: u16 = 1 << 0;
const KEY_DOWN: u16 = 1 << 1;
const KEY_LEFT: u16 = 1 << 2;
const KEY_RIGHT: u16 = 1 << 3;
const KEY_HAND: u16 = 1 << 4;

// PS/2 set 1 tarama kodları (basılı tutma takibi için).
const SCAN_W: u8 = 0x11;
const SCAN_A: u8 = 0x1E;
const SCAN_S: u8 = 0x1F;
const SCAN_D: u8 = 0x20;
const SCAN_SPACE: u8 = 0x39;
// E0 ön ekli gezinme tuşları.
const SCAN_UP: u8 = 0x48;
const SCAN_DOWN: u8 = 0x50;
const SCAN_LEFT: u8 = 0x4B;
const SCAN_RIGHT: u8 = 0x4D;

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

#[inline(always)]
fn sin(x: f32) -> f32 {
    crate::apps::viewer3d::sin(x)
}

#[inline(always)]
fn cos(x: f32) -> f32 {
    crate::apps::viewer3d::cos(x)
}

#[inline(always)]
fn absf(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// no_std çekirdekte `f32::floor` yok; elle taban değer.
#[inline(always)]
fn floorf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if x < 0.0 && t != x {
        t - 1.0
    } else {
        t
    }
}

/// Kesirli kısım (0..1).
#[inline(always)]
fn fractf(x: f32) -> f32 {
    x - floorf(x)
}

/// Pozitif modülo (kayan nokta, negatif sonuç üretmez).
#[inline(always)]
fn modf(x: f32, m: f32) -> f32 {
    let mut r = x - m * floorf(x / m);
    if r < 0.0 {
        r += m;
    }
    r
}

/// Rengi verilen çarpanla gölgelendirir (0 = siyah, 1 = kendisi).
#[inline(always)]
fn shade(color: u32, factor: f32) -> u32 {
    let f = factor.clamp(0.0, 1.2);
    let r = (((color >> 16) & 0xFF) as f32 * f) as u32;
    let g = (((color >> 8) & 0xFF) as f32 * f) as u32;
    let b = ((color & 0xFF) as f32 * f) as u32;
    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

/// Deterministik karma: şehir düzeni her açılışta aynıdır.
fn hash4(a: usize, b: usize, c: usize, d: usize) -> u32 {
    let mut h = (a as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((b as u32).wrapping_mul(0x85EB_CA77))
        ^ (c as u32).wrapping_mul(0xC2B2_AE3D)
        ^ (d as u32).wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    h
}

// ---------------------------------------------------------------------------
// Şehir haritası
// ---------------------------------------------------------------------------

/// Şehir ızgarası: hücre türü, bina yüksekliği (kat) ve renk tonu.
pub struct CityMap {
    pub kind: Vec<u8>,
    pub height: Vec<u8>,
    pub tone: Vec<u8>,
}

impl CityMap {
    /// Şehri üretir: her 8x8 blokta 2 hücre geniş cadde, kaldırım çemberi ve
    /// 2x2 hücrelik binalar; blokların bir kısmı park olarak bırakılır.
    pub fn generate() -> Self {
        let n = CITY_W * CITY_H;
        let mut map = CityMap {
            kind: alloc::vec![T_ROAD; n],
            height: alloc::vec![0; n],
            tone: alloc::vec![0; n],
        };
        for by in 0..(CITY_H / ROAD_PERIOD) {
            for bx in 0..(CITY_W / ROAD_PERIOD) {
                let park = hash4(bx, by, 7, 11) % 100 < 14;
                for y in 0..(ROAD_PERIOD - ROAD_HALF) {
                    for x in 0..(ROAD_PERIOD - ROAD_HALF) {
                        let cx = bx * ROAD_PERIOD + ROAD_HALF + x;
                        let cy = by * ROAD_PERIOD + ROAD_HALF + y;
                        if cx >= CITY_W || cy >= CITY_H {
                            continue;
                        }
                        let idx = cy * CITY_W + cx;
                        if park {
                            map.kind[idx] = T_PARK;
                            map.height[idx] = 0;
                            map.tone[idx] = 0;
                            continue;
                        }
                        // Bloğun dış çemberi kaldırımdır.
                        let edge = x == 0
                            || y == 0
                            || x == ROAD_PERIOD - ROAD_HALF - 1
                            || y == ROAD_PERIOD - ROAD_HALF - 1;
                        if edge {
                            map.kind[idx] = T_SIDEWALK;
                            map.height[idx] = 0;
                            map.tone[idx] = 0;
                            continue;
                        }
                        // İç alan 2x2 hücrelik binalara bölünür.
                        let gx = (x - 1) / 2;
                        let gy = (y - 1) / 2;
                        let h = hash4(bx, by, gx, gy);
                        let storeys = 1 + (h % 6) as u8;
                        let tone = ((h >> 8) % BUILDING_TONES.len() as u32) as u8;
                        map.kind[idx] = T_BUILDING;
                        map.height[idx] = storeys;
                        map.tone[idx] = tone;
                    }
                }
            }
        }
        map
    }

    /// Verilen hücre katı (bina) mı?
    #[inline(always)]
    pub fn is_solid(&self, cx: i32, cy: i32) -> bool {
        if cx < 0 || cy < 0 || cx >= CITY_W as i32 || cy >= CITY_H as i32 {
            return true;
        }
        self.kind[cy as usize * CITY_W + cx as usize] == T_BUILDING
    }

    /// Hücre türü (sınır dışı: yol).
    #[inline(always)]
    pub fn tile(&self, cx: i32, cy: i32) -> u8 {
        if cx < 0 || cy < 0 || cx >= CITY_W as i32 || cy >= CITY_H as i32 {
            return T_ROAD;
        }
        self.kind[cy as usize * CITY_W + cx as usize]
    }
}

// ---------------------------------------------------------------------------
// Trafik ve hedefler
// ---------------------------------------------------------------------------

/// Cadde üzerinde ilerleyen trafik aracı.
#[derive(Clone, Copy)]
pub struct TrafficCar {
    pub x: f32,
    pub y: f32,
    pub dir_x: f32,
    pub dir_y: f32,
    pub speed: f32,
    pub color: u32,
}

/// Toplanabilir hedef.
#[derive(Clone, Copy)]
pub struct Orb {
    pub x: f32,
    pub y: f32,
    pub active: bool,
}

/// Oyun durumu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CityState {
    /// Başlangıç ekranı.
    Menu,
    /// Oyun sürüyor.
    Driving,
    /// Duraklatıldı.
    Paused,
}

// ---------------------------------------------------------------------------
// Uygulama
// ---------------------------------------------------------------------------

/// 3D şehir sürüş oyunu.
pub struct CityDriveApp {
    pub map: CityMap,
    // Araç durumu
    pub car_x: f32,
    pub car_y: f32,
    pub heading: f32,
    pub speed: f32,
    pub cam_dir: f32,
    // Girdi
    pub key_mask: u16,
    // Dünya
    pub traffic: [TrafficCar; MAX_TRAFFIC],
    pub orbs: [Orb; MAX_ORBS],
    // İstatistik
    pub score: u32,
    pub collected: u32,
    pub distance: f32,
    pub crashes: u32,
    pub crash_flash: u32,
    pub state: CityState,
    pub engine_freq: u32,
}

impl CityDriveApp {
    pub fn new() -> Self {
        let mut app = CityDriveApp {
            map: CityMap::generate(),
            car_x: 1.5,
            car_y: 9.5,
            heading: 0.0,
            speed: 0.0,
            cam_dir: 0.0,
            key_mask: 0,
            traffic: [TrafficCar {
                x: 0.0,
                y: 0.0,
                dir_x: 1.0,
                dir_y: 0.0,
                speed: 0.0,
                color: 0,
            }; MAX_TRAFFIC],
            orbs: [Orb {
                x: 0.0,
                y: 0.0,
                active: false,
            }; MAX_ORBS],
            score: 0,
            collected: 0,
            distance: 0.0,
            crashes: 0,
            crash_flash: 0,
            state: CityState::Menu,
            engine_freq: 0,
        };
        app.reset_world();
        app
    }

    /// Araç, trafik ve hedefleri başlangıç durumuna döndürür.
    pub fn reset_world(&mut self) {
        self.car_x = 1.5;
        self.car_y = 9.5;
        self.heading = 0.0;
        self.cam_dir = 0.0;
        self.speed = 0.0;
        self.key_mask = 0;
        self.distance = 0.0;
        self.crashes = 0;
        self.crash_flash = 0;

        // Trafik araçları cadde şeritlerine yerleştirilir.
        for (i, traffic) in self.traffic.iter_mut().enumerate().take(MAX_TRAFFIC) {
            let h = hash4(i, 3, 5, 17);
            let corridor = (h % (CITY_W / ROAD_PERIOD) as u32) as usize;
            let lane = ((h >> 8) & 1) as usize;
            let horizontal = ((h >> 12) & 1) == 0;
            let start = ((h >> 16) % (CITY_W as u32 - 4)) as f32 + 2.0;
            let speed = 3.0 + ((h >> 24) % 40) as f32 * 0.1;
            let color = TRAFFIC_COLORS[(h % TRAFFIC_COLORS.len() as u32) as usize];
            let (x, y, dx, dy) = if horizontal {
                (
                    start,
                    (corridor * ROAD_PERIOD) as f32 + 0.5 + lane as f32,
                    if lane == 0 { 1.0 } else { -1.0 },
                    0.0,
                )
            } else {
                (
                    (corridor * ROAD_PERIOD) as f32 + 0.5 + lane as f32,
                    start,
                    0.0,
                    if lane == 0 { -1.0 } else { 1.0 },
                )
            };
            *traffic = TrafficCar {
                x,
                y,
                dir_x: dx,
                dir_y: dy,
                speed,
                color,
            };
        }

        // Hedefler cadde üzerine dağıtılır.
        for (i, orb) in self.orbs.iter_mut().enumerate().take(MAX_ORBS) {
            let h = hash4(i, 29, 31, 37);
            let corridor = (h % (CITY_W / ROAD_PERIOD) as u32) as usize;
            let along = ((h >> 8) % (CITY_W as u32 - 4)) as f32 + 2.0;
            let horizontal = ((h >> 16) & 1) == 0;
            let (x, y) = if horizontal {
                (along, (corridor * ROAD_PERIOD) as f32 + 1.0)
            } else {
                ((corridor * ROAD_PERIOD) as f32 + 1.0, along)
            };
            *orb = Orb { x, y, active: true };
        }
    }

    /// Klavyeden gelen karakteri işler; tüketildiyse `true` döner.
    pub fn handle_key(&mut self, ch: char) -> bool {
        match ch {
            'w' | 'W' => {
                self.key_mask |= KEY_UP;
                true
            }
            's' | 'S' => {
                self.key_mask |= KEY_DOWN;
                true
            }
            'a' | 'A' => {
                self.key_mask |= KEY_LEFT;
                true
            }
            'd' | 'D' => {
                self.key_mask |= KEY_RIGHT;
                true
            }
            ' ' => {
                self.key_mask |= KEY_HAND;
                true
            }
            '\n' | '\r' => {
                match self.state {
                    CityState::Menu => {
                        self.state = CityState::Driving;
                        self.reset_world();
                    }
                    CityState::Paused => self.state = CityState::Driving,
                    CityState::Driving => {}
                }
                true
            }
            '\x1b' => {
                self.state = match self.state {
                    CityState::Driving => CityState::Paused,
                    CityState::Paused => CityState::Driving,
                    CityState::Menu => CityState::Menu,
                };
                true
            }
            'r' | 'R' => {
                self.reset_world();
                self.state = CityState::Driving;
                true
            }
            _ => false,
        }
    }

    /// Klavyeden gelen eylemi işler (ok tuşları).
    pub fn handle_key_action(&mut self, action: KeyAction) -> bool {
        match action {
            KeyAction::ArrowUp => {
                self.key_mask |= KEY_UP;
                true
            }
            KeyAction::ArrowDown => {
                self.key_mask |= KEY_DOWN;
                true
            }
            KeyAction::ArrowLeft => {
                self.key_mask |= KEY_LEFT;
                true
            }
            KeyAction::ArrowRight => {
                self.key_mask |= KEY_RIGHT;
                true
            }
            _ => false,
        }
    }

    /// Tuş bırakıldığında maskeyi temizler.
    pub fn clear_key(&mut self, ch: char) {
        match ch {
            'w' | 'W' => self.key_mask &= !KEY_UP,
            's' | 'S' => self.key_mask &= !KEY_DOWN,
            'a' | 'A' => self.key_mask &= !KEY_LEFT,
            'd' | 'D' => self.key_mask &= !KEY_RIGHT,
            ' ' => self.key_mask &= !KEY_HAND,
            _ => {}
        }
    }

    /// Fizik ve dünya güncellemesi.
    ///
    /// `kbd` üzerinden basılı tutulan tuşlar okunur; böylece gaz tuşu basılı
    /// tutulduğu sürece araç hızlanır (bas-bırak değil).
    pub fn update(&mut self, dt_ms: u64, kbd: &crate::drivers::keyboard::KeyboardState) {
        let dt = (dt_ms as f32 / 1000.0).min(0.05);
        if self.state != CityState::Driving {
            self.speed = 0.0;
            self.engine_freq = 0;
            return;
        }

        // Basılı tutulan fizik tuşları (W/A/S/D, ok tuşları, Space).
        // Tuş yalnızca basılı tutulduğu sürece etkilidir; tek basışla
        // hızlanıp sürekli ilerlemeye devam etmemesi için `key_mask`
        // her karede sıfırlanır ve sadece şu an basılı olan tuşlardan
        // yeniden oluşturulur.
        self.key_mask = 0;
        if kbd.is_held(SCAN_W) || kbd.is_held_extended(SCAN_UP) {
            self.key_mask |= KEY_UP;
        }
        if kbd.is_held(SCAN_S) || kbd.is_held_extended(SCAN_DOWN) {
            self.key_mask |= KEY_DOWN;
        }
        if kbd.is_held(SCAN_A) || kbd.is_held_extended(SCAN_LEFT) {
            self.key_mask |= KEY_LEFT;
        }
        if kbd.is_held(SCAN_D) || kbd.is_held_extended(SCAN_RIGHT) {
            self.key_mask |= KEY_RIGHT;
        }
        if kbd.is_held(SCAN_SPACE) {
            self.key_mask |= KEY_HAND;
        }

        let throttle =
            (self.key_mask & KEY_UP != 0) as i32 - (self.key_mask & KEY_DOWN != 0) as i32;
        if throttle > 0 {
            if self.speed < 0.0 {
                self.speed += BRAKE * dt;
            } else {
                self.speed += ACCEL * dt;
            }
        } else if throttle < 0 {
            if self.speed > 0.0 {
                self.speed -= BRAKE * dt;
            } else {
                self.speed -= ACCEL * 0.55 * dt;
            }
        }

        // Sürtünme ve el freni.
        self.speed -= self.speed * DRAG * dt;
        if self.key_mask & KEY_HAND != 0 {
            self.speed -= self.speed * 3.2 * dt;
        }
        self.speed = self.speed.clamp(-MAX_REVERSE, MAX_SPEED);

        // Direksiyon: yalnızca araç hareket ederken etkilidir.
        let steer =
            (self.key_mask & KEY_RIGHT != 0) as i32 - (self.key_mask & KEY_LEFT != 0) as i32;
        if steer != 0 {
            let factor = ((absf(self.speed) + 0.10) / MAX_SPEED).min(1.0);
            let dir = if self.speed < 0.0 { -1.0 } else { 1.0 };
            self.heading += steer as f32 * TURN_RATE * factor * dir * dt;
            if self.heading > core::f32::consts::PI {
                self.heading -= core::f32::consts::TAU;
            } else if self.heading < -core::f32::consts::PI {
                self.heading += core::f32::consts::TAU;
            }
        }

        // İlerleme ve çarpışma.
        let fx = cos(self.heading);
        let fy = sin(self.heading);
        let nx = self.car_x + fx * self.speed * dt;
        let ny = self.car_y + fy * self.speed * dt;
        if self.collides(nx, ny) {
            if absf(self.speed) > 2.0 {
                self.crashes += 1;
                self.crash_flash = 14;
            }
            self.speed *= -0.22;
        } else {
            self.car_x = nx;
            self.car_y = ny;
        }
        self.car_x = self.car_x.clamp(0.4, CITY_W as f32 - 0.4);
        self.car_y = self.car_y.clamp(0.4, CITY_H as f32 - 0.4);
        self.distance += absf(self.speed) * dt;

        // Kamera yönü araca hafif gecikmeyle takip eder.
        let mut diff = self.heading - self.cam_dir;
        while diff > core::f32::consts::PI {
            diff -= core::f32::consts::TAU;
        }
        while diff < -core::f32::consts::PI {
            diff += core::f32::consts::TAU;
        }
        self.cam_dir += diff * (7.0 * dt).min(1.0);

        // Trafik hareketi ve oyuncuyla çarpışma.
        for car in self.traffic.iter_mut() {
            car.x += car.dir_x * car.speed * dt;
            car.y += car.dir_y * car.speed * dt;
            if car.x < 0.0 {
                car.x += CITY_W as f32;
            } else if car.x >= CITY_W as f32 {
                car.x -= CITY_W as f32;
            }
            if car.y < 0.0 {
                car.y += CITY_H as f32;
            } else if car.y >= CITY_H as f32 {
                car.y -= CITY_H as f32;
            }
            let dx = car.x - self.car_x;
            let dy = car.y - self.car_y;
            if dx * dx + dy * dy < 0.85 {
                self.speed *= 0.25;
                self.crash_flash = 10;
                // Çarpan araç biraz geri itilir.
                car.x -= car.dir_x * 0.05;
                car.y -= car.dir_y * 0.05;
            }
        }

        // Hedef toplama.
        let mut remaining = 0u32;
        for orb in self.orbs.iter_mut() {
            if !orb.active {
                remaining += 1;
                continue;
            }
            let dx = orb.x - self.car_x;
            let dy = orb.y - self.car_y;
            if dx * dx + dy * dy < 1.6 {
                orb.active = false;
                self.score += 25;
                self.collected += 1;
            } else {
                remaining += 1;
            }
        }
        if remaining == MAX_ORBS as u32 {
            // Tüm hedefler toplandı: yeni hedefler belirir.
            for (i, orb) in self.orbs.iter_mut().enumerate() {
                let h = hash4(i, self.collected as usize, 41, 43);
                let corridor = (h % (CITY_W / ROAD_PERIOD) as u32) as usize;
                let along = ((h >> 8) % (CITY_W as u32 - 4)) as f32 + 2.0;
                if ((h >> 16) & 1) == 0 {
                    orb.x = along;
                    orb.y = (corridor * ROAD_PERIOD) as f32 + 1.0;
                } else {
                    orb.x = (corridor * ROAD_PERIOD) as f32 + 1.0;
                    orb.y = along;
                }
                orb.active = true;
            }
        }

        if self.crash_flash > 0 {
            self.crash_flash -= 1;
        }

        // Motor sesi: hıza göre frekans.
        let ratio = (absf(self.speed) / MAX_SPEED).min(1.0);
        self.engine_freq = 40 + (ratio * 150.0) as u32;
    }

    /// Motor sesini üretir (çağıran ses cihazına yazar).
    pub fn play_engine_sound(&mut self, audio_dev: &mut AudioDevice) {
        if self.state != CityState::Driving {
            audio_dev.set_pc_speaker_tone(0);
            return;
        }
        if audio_dev.is_muted || audio_dev.volume_left == 0 {
            return;
        }
        audio_dev.set_pc_speaker_tone(self.engine_freq);
    }

    /// Motor sesini durdurur.
    pub fn stop_sound(&mut self, audio_dev: &mut AudioDevice) {
        audio_dev.set_pc_speaker_tone(0);
        self.engine_freq = 0;
    }

    /// Aracın verilen konumda bir binaya çarpışıp çarpışmayacağını kontrol eder.
    fn collides(&self, x: f32, y: f32) -> bool {
        let r = CAR_RADIUS;
        let min_x = floorf(x - r) as i32;
        let max_x = floorf(x + r) as i32;
        let min_y = floorf(y - r) as i32;
        let max_y = floorf(y + r) as i32;
        for cy in min_y..=max_y {
            for cx in min_x..=max_x {
                if self.map.is_solid(cx, cy) {
                    // Hücrenin merkezine olan mesafeyi de kontrol et (köşe geçişi).
                    let nearest_x = (cx as f32 + 0.5).clamp(x - r, x + r);
                    let nearest_y = (cy as f32 + 0.5).clamp(y - r, y + r);
                    let dx = nearest_x - x;
                    let dy = nearest_y - y;
                    if dx * dx + dy * dy < r * r {
                        return true;
                    }
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Çizim
    // -----------------------------------------------------------------------

    /// Ana çizim girişi (zaman bütçeli).
    pub fn render_content_with_budget(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        budget_ms: u64,
    ) -> bool {
        let budget = TimeBudget::new(budget_ms);
        if win_w < 64 || win_h < 64 {
            return true;
        }

        // İçerik alanı pencere başlık çubuğunun altında kalır.
        let top = win_y + crate::gui::titlebar_h();
        let view_h = win_h.saturating_sub(crate::gui::titlebar_h() + 1);
        if view_h < 64 {
            return true;
        }
        let horizon = top + view_h / 2;
        let dir_x = cos(self.cam_dir);
        let dir_y = sin(self.cam_dir);
        let plane_x = -dir_y * PLANE_LEN;
        let plane_y = dir_x * PLANE_LEN;
        let focal = view_h as f32 * 0.5;

        // 1) Gökyüzü: dikey gradyan ve güneş.
        for row in 0..(horizon - top) {
            if budget.is_expired() {
                return false;
            }
            let t = row as f32 / (horizon - top).max(1) as f32;
            let r = (58.0 + 120.0 * t) as u32;
            let g = (110.0 + 95.0 * t) as u32;
            let b = (185.0 + 45.0 * t) as u32;
            fb.draw_rect(win_x, top + row, win_w, 1, (r << 16) | (g << 8) | b);
        }
        let sun_x = win_x + (win_w as f32 * 0.74) as usize;
        let sun_y = top + ((horizon - top) as f32 * 0.24) as usize;
        for dy in -9i32..=9 {
            for dx in -9i32..=9 {
                let d2 = dx * dx + dy * dy;
                if d2 <= 72 {
                    let color = if d2 <= 30 { 0xFFF6C8 } else { 0xFFE9A0 };
                    fb.draw_rect(
                        (sun_x as i32 + dx).max(0) as usize,
                        (sun_y as i32 + dy).max(0) as usize,
                        1,
                        1,
                        color,
                    );
                }
            }
        }

        // 2) Zemin: ızgara izdüşümü (asfalt, kaldırım, park, şerit).
        for row in (horizon..top + view_h).step_by(2) {
            if budget.is_expired() {
                return false;
            }
            let dy = (row as f32 + 1.0) - horizon as f32;
            if dy <= 0.0 {
                continue;
            }
            let dist = CAM_H * focal / dy;
            if dist > MAX_VIEW {
                continue;
            }
            for col in (0..win_w).step_by(4) {
                let cxf = 2.0 * col as f32 / win_w as f32 - 1.0;
                let rx = dir_x + plane_x * cxf;
                let ry = dir_y + plane_y * cxf;
                let fx = self.car_x + rx * dist;
                let fy = self.car_y + ry * dist;
                let color = self.floor_color(fx, fy, dist);
                fb.draw_rect(win_x + col, row, 4.min(win_w - col), 2, color);
            }
        }

        // 3) Binalar: raycasting duvar sütunları.
        // Geniş pencerelerde sprite derinlik testi de tam görünürlük alanını
        // kapsasın; 512 sütunluk tampon yüksek çözünürlükte araçları kırpıyordu.
        let mut zbuf = [MAX_VIEW; 1024];
        let col_step = if win_w >= 900 { 2 } else { 1 };
        for col in (0..win_w.min(1024)).step_by(col_step) {
            if budget.is_expired() {
                return false;
            }
            let cxf = 2.0 * col as f32 / win_w as f32 - 1.0;
            let rx = dir_x + plane_x * cxf;
            let ry = dir_y + plane_y * cxf;

            let mut map_x = self.car_x as i32;
            let mut map_y = self.car_y as i32;
            let delta_x = (1.0 / absf(rx)).max(0.0001);
            let delta_y = (1.0 / absf(ry)).max(0.0001);
            let step_x: i32 = if rx < 0.0 { -1 } else { 1 };
            let step_y: i32 = if ry < 0.0 { -1 } else { 1 };
            let mut side_x = if rx < 0.0 {
                (self.car_x - map_x as f32) * delta_x
            } else {
                (map_x as f32 + 1.0 - self.car_x) * delta_x
            };
            let mut side_y = if ry < 0.0 {
                (self.car_y - map_y as f32) * delta_y
            } else {
                (map_y as f32 + 1.0 - self.car_y) * delta_y
            };

            let mut hit = false;
            let mut side = 0;
            let mut hit_x = 0i32;
            let mut hit_y = 0i32;
            let mut steps = 0;
            while !hit && steps < 96 {
                steps += 1;
                if side_x < side_y {
                    side_x += delta_x;
                    map_x += step_x;
                    side = 0;
                } else {
                    side_y += delta_y;
                    map_y += step_y;
                    side = 1;
                }
                if map_x < 0 || map_y < 0 || map_x >= CITY_W as i32 || map_y >= CITY_H as i32 {
                    break;
                }
                if self.map.is_solid(map_x, map_y) {
                    hit = true;
                    hit_x = map_x;
                    hit_y = map_y;
                }
            }
            if !hit {
                continue;
            }

            let perp = if side == 0 {
                (hit_x as f32 - self.car_x + (1.0 - step_x as f32) * 0.5) / rx
            } else {
                (hit_y as f32 - self.car_y + (1.0 - step_y as f32) * 0.5) / ry
            };
            let perp = if perp.is_finite() {
                absf(perp).clamp(0.2, MAX_VIEW)
            } else {
                MAX_VIEW
            };
            if perp > MAX_VIEW {
                continue;
            }
            for slot in zbuf[col..(col + col_step).min(win_w.min(1024))].iter_mut() {
                *slot = perp;
            }

            let idx = hit_y as usize * CITY_W + hit_x as usize;
            let storeys = self.map.height[idx] as f32;
            let wall_h = storeys * STOREY_H;
            let scale = focal / perp;
            let y_bottom = horizon as f32 + CAM_H * scale;
            let y_top = horizon as f32 - (wall_h - CAM_H) * scale;
            // Dünya projeksiyonu mutlak ekran koordinatındadır. Burada
            // `win_h` yerine görünüm alanının [top, top + view_h) sınırlarını
            // kullanmak, başlık çubuğu altındaki duvarların yanlışlıkla
            // tamamen atlanmasını ve yarım çizilmesini önler.
            if y_bottom <= top as f32 || y_top >= (top + view_h) as f32 {
                continue;
            }

            let tone = BUILDING_TONES[self.map.tone[idx] as usize % BUILDING_TONES.len()];
            let dist_shade = 0.42 + 0.58 * (1.0 - (perp / MAX_VIEW).clamp(0.0, 1.0));
            let side_shade = if side == 0 { 1.0 } else { 0.74 };
            let factor = dist_shade * side_shade;
            let wall_color = shade(tone, factor);
            let win_color = shade(WINDOW_COLOR, factor);
            let lit_color = shade(WINDOW_LIT_COLOR, factor);

            // Yatay doku koordinatı (sütun başına sabit).
            let wall_u = if side == 0 {
                fractf(perp * ry)
            } else {
                fractf(perp * rx)
            };
            let window_x = wall_u > 0.16 && wall_u < 0.84;

            let row_start = (y_top.max(top as f32) as usize).min(top + view_h);
            let row_end = (y_bottom.max(top as f32) as usize).min(top + view_h);
            if row_start >= row_end {
                continue;
            }
            for row in row_start..row_end {
                let z = CAM_H + (horizon as f32 - row as f32) / scale;
                let storey_f = fractf(z / STOREY_H);
                let band = storey_f < 0.62;
                let color = if window_x && band {
                    // Pencerelerin bir kısmı yanıyor.
                    if ((hit_x * 31 + hit_y * 17 + (row / 7) as i32) % 5) == 0 {
                        lit_color
                    } else {
                        win_color
                    }
                } else {
                    wall_color
                };
                fb.draw_rect(win_x + col, row, col_step, 1, color);
            }
        }

        // 4) Sprite'lar: hedef kubbeleri ve trafik araçları.
        for orb in self.orbs.iter() {
            if !orb.active {
                continue;
            }
            self.draw_sprite(
                fb, top, win_w, horizon, focal, &zbuf, orb.x, orb.y, 0.55, 0xF5D442, true,
            );
        }
        for car in self.traffic.iter() {
            self.draw_sprite(
                fb, top, win_w, horizon, focal, &zbuf, car.x, car.y, 1.0, car.color, false,
            );
        }

        // 5) HUD.
        self.render_hud(fb, win_x, top, win_w, view_h);
        true
    }

    /// Bütçesiz çizim (uyumluluk).
    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let _ = self.render_content_with_budget(fb, win_x, win_y, win_w, win_h, RENDER_BUDGET_MS);
    }

    /// Zemin rengini hesaplar (asfalt, kaldırım, park, yol şeridi).
    fn floor_color(&self, fx: f32, fy: f32, dist: f32) -> u32 {
        let cx = fx as i32;
        let cy = fy as i32;
        let kind = self.map.tile(cx, cy);
        let shade_f = 0.55 + 0.45 * (1.0 - (dist / MAX_VIEW).clamp(0.0, 1.0));
        let base = match kind {
            T_SIDEWALK => SIDEWALK_COLOR,
            T_PARK => PARK_COLOR,
            _ => ROAD_COLOR,
        };
        // Yol şeridi: koridorun ortasında kesikli sarı çizgi.
        let lx = modf(fx, ROAD_PERIOD as f32);
        let ly = modf(fy, ROAD_PERIOD as f32);
        let dashed_x = modf(fx, 3.0) < 1.7;
        let dashed_y = modf(fy, 3.0) < 1.7;
        if ly < ROAD_HALF as f32 && absf(ly - 1.0) < 0.16 && dashed_x {
            return shade(LANE_COLOR, shade_f);
        }
        if lx < ROAD_HALF as f32 && absf(lx - 1.0) < 0.16 && dashed_y {
            return shade(LANE_COLOR, shade_f);
        }
        // Hücreye bağlı az karartma: düz zemini parçalı bir doku gibi gösterir,
        // ek çizim maliyeti yok.
        let mut color = shade(base, shade_f);
        if (cx + cy) & 1 == 0 {
            color = shade(color, 0.88);
        }
        color
    }

    /// Dünya koordinatındaki bir nesneyi ekrana bilboard olarak çizer.
    #[allow(clippy::too_many_arguments)]
    fn draw_sprite(
        &self,
        fb: &mut Framebuffer,
        win_y: usize,
        win_w: usize,
        horizon: usize,
        focal: f32,
        zbuf: &[f32; 1024],
        world_x: f32,
        world_y: f32,
        world_h: f32,
        color: u32,
        round: bool,
    ) {
        let dir_x = cos(self.cam_dir);
        let dir_y = sin(self.cam_dir);
        let plane_x = -dir_y * PLANE_LEN;
        let plane_y = dir_x * PLANE_LEN;
        let rel_x = world_x - self.car_x;
        let rel_y = world_y - self.car_y;
        let depth = rel_x * dir_x + rel_y * dir_y;
        if depth <= 0.25 {
            return;
        }
        let lateral = rel_x * plane_x + rel_y * plane_y;
        let cam_x = lateral / depth;
        if cam_x < -1.0 || cam_x > 1.0 {
            return;
        }
        let screen_x = ((cam_x + 1.0) * 0.5 * win_w as f32) as i32;
        if screen_x < 0 || screen_x as usize >= win_w.min(1024) {
            return;
        }
        let depth = depth.min(MAX_VIEW);
        if depth >= zbuf[screen_x as usize] {
            return;
        }
        let scale = focal / depth;
        let size_y = (world_h * scale).max(2.0) as i32;
        let size_x = if round {
            size_y
        } else {
            (world_h * 1.9 * scale).max(3.0) as i32
        };
        let center_y = horizon as f32 + (CAM_H - world_h * 0.5) * scale;
        let top = (center_y - size_y as f32 * 0.5).max(0.0) as i32;
        let left = screen_x - size_x / 2;
        let bottom = top + size_y;

        if round {
            // Kubbe: parlak merkezli halka.
            for row in top..bottom {
                let dy = row - (top + bottom) / 2;
                let half = (size_x / 2) as f32;
                let dx = half - absf(dy as f32);
                let w = (dx * 2.0) as i32;
                if w <= 0 {
                    continue;
                }
                let edge = dx < half * 0.35;
                let c = if edge { 0xFFF0A0 } else { color };
                fb.draw_rect(
                    (left + half as i32 - w / 2).max(0) as usize,
                    (win_y as i32 + row).max(0) as usize,
                    w as usize,
                    1,
                    c,
                );
            }
        } else {
            // Trafik aracı: gövde + cam bandı.
            fb.draw_rect(
                left.max(0) as usize,
                (win_y as i32 + top).max(0) as usize,
                size_x.max(1) as usize,
                size_y.max(1) as usize,
                color,
            );
            let glass_h = (size_y / 4).max(1) as usize;
            fb.draw_rect(
                (left + size_x / 6).max(0) as usize,
                (win_y as i32 + top + size_y / 4).max(0) as usize,
                (size_x * 2 / 3).max(1) as usize,
                glass_h,
                0x9FD4E8,
            );
            fb.draw_rect(
                left.max(0) as usize,
                (win_y as i32 + bottom - size_y / 5).max(0) as usize,
                size_x.max(1) as usize,
                (size_y / 5).max(1) as usize,
                0x1A1C20,
            );
        }
    }

    /// HUD: hız göstergesi, skor, mini harita, kontrol ipuçları.
    fn render_hud(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        // Hız göstergesi (sol alt).
        let panel_w = 190.min(win_w.saturating_sub(20));
        let panel_h = 54;
        let px = win_x + 10;
        let py = win_y + win_h.saturating_sub(panel_h + 10);
        fb.draw_rect(px, py, panel_w, panel_h, 0x141922);
        fb.draw_rect_outline(px, py, panel_w, panel_h, 0x2E3A4A);

        let speed_kmh = (absf(self.speed) * 7.2) as u32;
        let mut label = crate::gui::TextBuf::new();
        label.push_usize(speed_kmh as usize);
        label.push_str(" km/s");
        MinGuiCompositor::draw_string(fb, px + 10, py + 8, label.as_str(), Color::TEXT_MAIN);

        // Hız çubuğu.
        let bar_x = px + 10;
        let bar_y = py + 28;
        let bar_w = panel_w - 20;
        let bar_h = 12;
        fb.draw_rect(bar_x, bar_y, bar_w, bar_h, 0x232A35);
        let ratio = (absf(self.speed) / MAX_SPEED).min(1.0);
        let fill_w = ((bar_w as f32) * ratio) as usize;
        let fill_color = if ratio > 0.8 {
            0xFF6B6B
        } else if ratio > 0.5 {
            0xF0B040
        } else {
            0x62D6A8
        };
        fb.draw_rect(bar_x, bar_y, fill_w, bar_h, fill_color);

        // Skor ve mesafe (sol üst).
        let mut score_buf = crate::gui::TextBuf::new();
        score_buf.push_str("Skor: ");
        score_buf.push_usize(self.score as usize);
        score_buf.push_str("  Mesafe: ");
        score_buf.push_usize((self.distance * 10.0) as usize);
        score_buf.push_str(" m");
        MinGuiCompositor::draw_string(
            fb,
            win_x + 10,
            win_y + 8,
            score_buf.as_str(),
            Color::TEXT_MUTED,
        );

        // Mini harita (sağ üst).
        let map_size = 110.min(win_w / 4).max(48);
        let mx = win_x + win_w.saturating_sub(map_size + 10);
        let my = win_y + 10;
        self.render_minimap(fb, mx, my, map_size);

        // Kontrol ipuçları (alt orta).
        let hint = crate::gui::ui_text(
            "W/S gaz-fren  A/D direksiyon  Space el freni  R sifirla  Esc duraklat",
            "W/S throttle-brake  A/D steer  Space handbrake  R reset  Esc pause",
        );
        MinGuiCompositor::draw_string(
            fb,
            win_x + (win_w.saturating_sub(MinGuiCompositor::text_width(hint, 1))) / 2,
            win_y + win_h.saturating_sub(18),
            hint,
            Color::TEXT_MUTED,
        );

        // Çarpışma efekti.
        if self.crash_flash > 0 {
            let alpha = (self.crash_flash as f32 / 14.0).min(1.0);
            let c = shade(0xFF4444, alpha * 0.55);
            fb.draw_rect_outline(win_x, win_y, win_w, win_h, c);
        }

        // Menü / duraklatma katmanı.
        match self.state {
            CityState::Menu => {
                let title = crate::gui::ui_text("ŞEHİR SÜRÜŞÜ", "CITY DRIVE");
                let sub = crate::gui::ui_text(
                    "[Enter] ile sürüşe başla",
                    "Press [Enter] to start driving",
                );
                let controls = crate::gui::ui_text(
                    "W/S gaz-fren · A/D direksiyon · Space el freni",
                    "W/S throttle-brake · A/D steer · Space handbrake",
                );
                let goal = crate::gui::ui_text(
                    "Sarı kubbeleri topla, trafikten kaç!",
                    "Collect yellow orbs, dodge traffic!",
                );
                let cx = win_x + win_w / 2;
                let cy = win_y + win_h / 2;
                let bw = 320.min(win_w.saturating_sub(20));
                let bh = 150.min(win_h.saturating_sub(20));
                fb.draw_rect(cx - bw / 2, cy - bh / 2, bw, bh, 0x0E1218);
                fb.draw_rect_outline(cx - bw / 2, cy - bh / 2, bw, bh, 0x4DABF7);
                MinGuiCompositor::draw_string(
                    fb,
                    cx - MinGuiCompositor::text_width(title, 2) / 2,
                    cy - bh / 2 + 18,
                    title,
                    Color::ACCENT_BLUE,
                );
                for (i, line) in [sub, controls, goal].iter().enumerate() {
                    MinGuiCompositor::draw_string(
                        fb,
                        cx - MinGuiCompositor::text_width(line, 1) / 2,
                        cy - bh / 2 + 58 + i * 22,
                        line,
                        Color::TEXT_MAIN,
                    );
                }
            }
            CityState::Paused => {
                let title = crate::gui::ui_text("DURAKLATILDI", "PAUSED");
                let sub =
                    crate::gui::ui_text("[Enter] devam · R sıfırla", "[Enter] resume · R reset");
                let cx = win_x + win_w / 2;
                let cy = win_y + win_h / 2;
                let bw = 280.min(win_w.saturating_sub(20));
                let bh = 96.min(win_h.saturating_sub(20));
                fb.draw_rect(cx - bw / 2, cy - bh / 2, bw, bh, 0x0E1218);
                fb.draw_rect_outline(cx - bw / 2, cy - bh / 2, bw, bh, 0x4DABF7);
                MinGuiCompositor::draw_string(
                    fb,
                    cx - MinGuiCompositor::text_width(title, 2) / 2,
                    cy - bh / 2 + 16,
                    title,
                    Color::ACCENT_BLUE,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    cx - MinGuiCompositor::text_width(sub, 1) / 2,
                    cy - bh / 2 + 54,
                    sub,
                    Color::TEXT_MAIN,
                );
            }
            CityState::Driving => {}
        }
    }

    /// Üstten bakış mini harita.
    fn render_minimap(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        fb.draw_rect(x, y, size, size, 0x10141A);
        fb.draw_rect_outline(x, y, size, size, 0x2E3A4A);
        let view_cells = 26.0f32;
        let scale = size as f32 / view_cells;
        let half = view_cells * 0.5;

        // Caddeler.
        for i in 0..(CITY_W / ROAD_PERIOD) {
            let road = (i * ROAD_PERIOD) as f32;
            let vx = x as f32 + (road - self.car_x) * scale;
            if vx >= x as f32 && vx < (x + size) as f32 {
                fb.draw_rect(vx as usize, y, 3, size, 0x39414D);
            }
            let vy = y as f32 + (road - self.car_y) * scale;
            if vy >= y as f32 && vy < (y + size) as f32 {
                fb.draw_rect(x, vy as usize, size, 3, 0x39414D);
            }
        }

        // Hedefler.
        for orb in self.orbs.iter() {
            if !orb.active {
                continue;
            }
            let px = x as f32 + (orb.x - self.car_x) * scale;
            let py = y as f32 + (orb.y - self.car_y) * scale;
            if px >= x as f32 && px < (x + size) as f32 && py >= y as f32 && py < (y + size) as f32
            {
                fb.draw_rect(px as usize, py as usize, 3, 3, 0xF5D442);
            }
        }

        // Trafik.
        for car in self.traffic.iter() {
            let px = x as f32 + (car.x - self.car_x) * scale;
            let py = y as f32 + (car.y - self.car_y) * scale;
            if px >= x as f32 && px < (x + size) as f32 && py >= y as f32 && py < (y + size) as f32
            {
                fb.draw_rect(px as usize, py as usize, 3, 3, car.color);
            }
        }

        // Oyuncu: merkezde yön oku.
        let cx = x + size / 2;
        let cy = y + size / 2;
        fb.draw_rect(cx - 2, cy - 2, 5, 5, 0xFF6B6B);
        let fx = cos(self.heading);
        let fy = sin(self.heading);
        for step in 1..6 {
            let px = (cx as f32 + fx * step as f32 * 1.6) as usize;
            let py = (cy as f32 + fy * step as f32 * 1.6) as usize;
            fb.draw_rect(px, py, 2, 2, 0xFFD0D0);
        }
        let _ = half;
    }
}

/// Trafik araçları için renk paleti.
const TRAFFIC_COLORS: [u32; 6] = [0xD94F4F, 0x4F8FD9, 0xE0C04F, 0x5FD98F, 0xD9D9D9, 0x9A5FD9];

impl Default for CityDriveApp {
    fn default() -> Self {
        Self::new()
    }
}

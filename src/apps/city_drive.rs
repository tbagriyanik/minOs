//! 3D şehir sürüş oyunu.
//!
//! Raycasting tabanlı şehir motoru: ızgara düzeninde cadde, kaldırım, park ve
//! farklı yükseklikte binalar; bisiklet modeline dayalı direksiyon/fram/hız
//! fiziği; sağdan trafik kuralına uyan, kavşaklarda dönen, birbirini takip
//! eden trafik araçları; toplanabilir ödüller, mini harita, hız göstergesi
//! ve motor sesi.
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
pub const CITY_W: usize = 60;
/// Şehir ızgara yüksekliği (hücre).
pub const CITY_H: usize = 60;

/// Cadde periyodu: her 12 hücrede bir cadde koridoru.
const ROAD_PERIOD: usize = 12;
/// Cadde koridorunun genişliği (hücre) — 6 m'lik iki şeritli yol.
const ROAD_HALF: usize = 6;
/// Yol ekseninin koridor içindeki konumu (hücre).
const ROAD_AXIS_OFF: f32 = 3.0;
/// Şerit merkezinin yol eksenine uzaklığı (metre) — 3 m'lik şeritler.
const LANE_OFF: f32 = 1.5;
/// Bir binanın kat yüksekliği (dünya birimi).
const STOREY_H: f32 = 2.6;
/// Sürücü gözü / kamera yüksekliği (dünya birimi).
const CAM_H: f32 = 1.25;
/// Aracın çarpışma yarıçapı.
const CAR_RADIUS: f32 = 0.42;
/// Azami ileri hız (metre/s).
const MAX_SPEED: f32 = 28.0;
/// Azami geri hız (metre/s).
const MAX_REVERSE: f32 = 6.0;
/// Gaz ivmesi (m/s²).
const ACCEL: f32 = 7.5;
/// Fren ivmesi (m/s²).
const BRAKE: f32 = 13.5;
/// El freni ivmesi (m/s²).
const HANDBRAKE: f32 = 11.0;
/// Rölanti direnci (m/s²).
const DRAG_BASE: f32 = 0.32;
/// Hava direncinin karesel katsayısı.
const DRAG_K: f32 = 0.0022;
/// Bisiklet modeli: dingil mesafesi (metre).
const WHEELBASE: f32 = 2.72;
/// Tam kilitte, düşük hızda minimum dönüş yarıçapı (metre).
const TURN_R_MIN: f32 = 4.2;
/// Hıza bağlı dönüş yarıçapı artış katsayısı (yüksek hızda geniş yay).
const TURN_R_SPEED_K: f32 = 0.030;
/// Direksiyon kolunun dönme hızı (komut birimi/s).
const STEER_RATE: f32 = 2.8;
/// Kavşak dönüş yayı yarıçapı (metre).
const TURN_ARC_R: f32 = 2.5;
/// Kavşakta dönüş hızı (metre/s).
const TURN_SPEED: f32 = 7.0;
/// Görüş uzaklığı (dünya birimi).
const MAX_VIEW: f32 = 70.0;
/// Kamera düzlem uzunluğu (görüş açısı).
const PLANE_LEN: f32 = 0.72;

/// Trafik aracı sayısı.
const MAX_TRAFFIC: usize = 16;
/// Toplanabilir ödül sayısı.
const MAX_REWARDS: usize = 14;
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
const EDGE_LINE_COLOR: u32 = 0xC6D0D8;
const WINDOW_COLOR: u32 = 0x8FC4DC;
const WINDOW_LIT_COLOR: u32 = 0xF0DFA0;

/// Bina tonları (gölgelendirilerek kullanılır).
const BUILDING_TONES: [u32; 6] = [0x8A8F99, 0x6E7A8C, 0x9A8E7C, 0x7E8C7A, 0x8C7E8E, 0x9EA3AC];

/// Gerçekçi araç boyaları (gümüş, antrasit, beyaz, mavi, kırmızı, ...).
const CAR_PAINT: [u32; 12] = [
    0xB8BDC4, // gümüş
    0x2B3038, // antrasit
    0xE8EAED, // beyaz
    0x1B3A6B, // koyu mavi
    0x8C1F26, // koyu kırmızı
    0x1F5138, // koyu yeşil
    0xC9C3B6, // bej
    0x4A4E56, // gri
    0x9C6B1E, // bronz
    0x14171C, // siyah
    0x2E6E8E, // metal mavi
    0x7A3E12, // bakır
];

/// Taksi sarısı.
const TAXI_COLOR: u32 = 0xE3B32B;
///polis aracı gövde rengi.
const POLICE_COLOR: u32 = 0xE6E8EC;

/// Gece gökyüzü üst rengi.
const NIGHT_SKY_TOP: u32 = 0x02040A;
/// Gece gökyüzü alt rengi.
const NIGHT_SKY_HORIZON: u32 = 0x1A2438;
/// Gün gökyüzü üst rengi.
const DAY_SKY_TOP: u32 = 0x3A6EBC;
/// Gün gökyüzü alt rengi.
const DAY_SKY_HORIZON: u32 = 0x6EB3FA;

/// Gün/döngü saati (0.0 = gece yarısı, 0.25 = sabah, 0.5 = öğle, 0.75 = akşam).
const DAY_CYCLE_SPEED: f32 = 0.015; // saniye başına döngü ilerleme
/// Gece başlangıç eşiği (0.0..1.0).
const NIGHT_START: f32 = 0.72;
/// Geçiş yumuşaklığı (0.0..1.0).
const NIGHT_FADE: f32 = 0.18;

// Parçacık sistemi (egzoz dumanı, çarpışma gnisteri, drift tozu).
const MAX_PARTICLES: usize = 48;
/// Yakıt tankı kapasitesi (litre).
const FUEL_CAPACITY: f32 = 60.0;
/// Yakıt tüketimi (litre/km, başlangıç).
const FUEL_CONSUMPTION_BASE: f32 = 0.07;
/// Yakıt uyarı eşiği (litrede).
const FUEL_WARN: f32 = 8.0;

// Drift durumu.
const DRIFT_SMOKE_INTERVAL: f32 = 0.06;

// Tuş maska bayrakları.
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

/// no_std çekirdekte `f32::sqrt` yok; Newton yöntemiyle yaklaşık değer.
#[inline(always)]
fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x > 1.0 { x * 0.5 } else { x };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
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

/// İki renk arasında lineer interpolasyon (t: 0..1).
#[inline(always)]
fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let r = (((a >> 16) & 0xFF) as f32 * (1.0 - t) + ((b >> 16) & 0xFF) as f32 * t) as u32;
    let g = (((a >> 8) & 0xFF) as f32 * (1.0 - t) + ((b >> 8) & 0xFF) as f32 * t) as u32;
    let bl = ((a & 0xFF) as f32 * (1.0 - t) + (b & 0xFF) as f32 * t) as u32;
    (r << 16) | (g << 8) | bl
}

/// İki 8-bit kanal değeri arasında interpolasyon.
#[inline(always)]
fn lerp_u8(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    ((a as f32 * (1.0 - t) + b as f32 * t) as u32).min(255)
}

/// Gökyüzü rengi için interpolasyon (gece/gün geçişi).
#[inline(always)]
fn lerp_sky(a: u32, b: u32, t: f32) -> u32 {
    lerp_color(a, b, t)
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
    /// Şehri üretir: her 10x10 blokta 4 hücre geniş cadde, kaldırım çemberi ve
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
// Trafik ve ödüller
// ---------------------------------------------------------------------------

/// Trafik aracının hareket ekseni.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LaneAxis {
    /// Yatay cadde (x ekseni).
    Horizontal,
    /// Dikey cadde (y ekseni).
    Vertical,
}

/// Kavşak dönüş yayı — araç yayın çevresinde döner.
#[derive(Clone, Copy)]
pub struct TurnArc {
    /// Yay merkezi.
    pub cx: f32,
    pub cy: f32,
    /// Yay yarıçapı.
    pub r: f32,
    /// Merkezden araca olan vektör.
    pub rx: f32,
    pub ry: f32,
    /// Dönülen toplam açı (rad).
    pub swept: f32,
    /// Dönüş yönü: +1 saat yönünün tersi, -1 saat yönü.
    pub rot: f32,
    /// Yayın bittiği kesin nokta (yeni şerit üzerinde).
    pub exit_x: f32,
    pub exit_y: f32,
    /// Yay sonundaki kesin yön (şerit yönü).
    pub end_dir_x: f32,
    pub end_dir_y: f32,
    /// Dönüş sonundaki şerit.
    pub new_axis: LaneAxis,
    pub new_road: usize,
    pub new_lane_dir: f32,
}

/// Trafik aracı tipi — gerçekçi gövde ölçüleri ve karakteristik davranış.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VehicleKind {
    /// Şehir kompakti.
    Compact,
    /// Sedan.
    Sedan,
    /// SUV / arazi aracı.
    Suv,
    /// Panelvan.
    Van,
    /// Pickup.
    Pickup,
    /// Taksi.
    Taxi,
    /// Polis aracı.
    Police,
}

impl VehicleKind {
    /// Gövde ölçüleri: (uzunluk, genişlik, yükseklik) — metre.
    pub fn dims(self) -> (f32, f32, f32) {
        match self {
            VehicleKind::Compact => (3.80, 1.68, 1.48),
            VehicleKind::Sedan => (4.62, 1.78, 1.45),
            VehicleKind::Suv => (4.72, 1.86, 1.70),
            // 3D rendering boyutları için eklendi
            VehicleKind::Van => (5.20, 1.94, 2.15),
            VehicleKind::Pickup => (5.30, 1.90, 1.82),
            VehicleKind::Taxi => (4.70, 1.80, 1.50),
            VehicleKind::Police => (4.90, 1.84, 1.52),
        }
    }

    /// Sürüş hızı (metre/s).
    pub fn cruise(self) -> f32 {
        match self {
            VehicleKind::Compact => 11.0,
            VehicleKind::Sedan => 13.5,
            VehicleKind::Suv => 12.5,
            VehicleKind::Van => 10.5,
            VehicleKind::Pickup => 11.0,
            VehicleKind::Taxi => 14.5,
            VehicleKind::Police => 16.0,
        }
    }

    /// İvme (m/s²).
    pub fn accel(self) -> f32 {
        match self {
            VehicleKind::Compact => 2.6,
            VehicleKind::Sedan => 2.8,
            VehicleKind::Suv => 2.6,
            VehicleKind::Van => 1.9,
            VehicleKind::Pickup => 2.2,
            VehicleKind::Taxi => 3.1,
            VehicleKind::Police => 3.6,
        }
    }

    /// Gövde rengi.
    pub fn paint(self, h: u32) -> u32 {
        match self {
            VehicleKind::Taxi => TAXI_COLOR,
            VehicleKind::Police => POLICE_COLOR,
            _ => CAR_PAINT[(h % CAR_PAINT.len() as u32) as usize],
        }
    }

    /// Araç tipini hash'ten seçer (trafik karışımı).
    pub fn pick(h: u32) -> Self {
        match h % 100 {
            0..=17 => VehicleKind::Compact,
            18..=44 => VehicleKind::Sedan,
            45..=60 => VehicleKind::Suv,
            61..=72 => VehicleKind::Van,
            73..=82 => VehicleKind::Pickup,
            83..=93 => VehicleKind::Taxi,
            _ => VehicleKind::Police,
        }
    }
}

/// Cadde üzerinde ilerleyen trafik aracı.
#[derive(Clone, Copy)]
pub struct TrafficCar {
    pub x: f32,
    pub y: f32,
    /// Yön birim vektörü.
    pub dir_x: f32,
    pub dir_y: f32,
    pub speed: f32,
    pub target_speed: f32,
    pub color: u32,
    pub kind: VehicleKind,
    /// Gövde ölçüleri (metre).
    pub length: f32,
    pub width: f32,
    pub height: f32,
    /// Fren lambası yoğunluğu (0..1).
    pub brake: f32,
    /// Hareket ekseni.
    pub axis: LaneAxis,
    /// Yol indeksi.
    pub road: usize,
    /// Şerit yönü (+1 / -1).
    pub lane_dir: f32,
    /// Aktif kavşak dönüşü.
    pub turn: Option<TurnArc>,
}

impl TrafficCar {
    /// Aracın şerit üzerindeki ileri konumu (yön işaretli).
    #[inline(always)]
    pub fn along(&self) -> f32 {
        match self.axis {
            LaneAxis::Horizontal => self.x * self.lane_dir,
            LaneAxis::Vertical => self.y * self.lane_dir,
        }
    }
}

/// Toplanabilir ödül türü.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RewardKind {
    /// Madeni para.
    Coin,
    /// Banknot.
    Banknote,
    /// Yakıt bidonu.
    Fuel,
    /// Madalya.
    Medal,
}

impl RewardKind {
    /// Puan değeri.
    pub fn score(self) -> u32 {
        match self {
            RewardKind::Coin => 25,
            RewardKind::Banknote => 50,
            RewardKind::Fuel => 75,
            RewardKind::Medal => 100,
        }
    }

    /// Görünür boyut: (yükseklik, genişlik) — metre.
    pub fn dims(self) -> (f32, f32) {
        match self {
            RewardKind::Coin => (0.44, 0.44),
            RewardKind::Banknote => (0.32, 0.66),
            RewardKind::Fuel => (0.62, 0.34),
            RewardKind::Medal => (0.68, 0.38),
        }
    }

    /// Harita / HUD rengi.
    pub fn tint(self) -> u32 {
        match self {
            RewardKind::Coin => 0xF2C53D,
            RewardKind::Banknote => 0x62C97A,
            RewardKind::Fuel => 0xE05B4A,
            RewardKind::Medal => 0xFFD24A,
        }
    }

    /// Ödül türünü hash'ten seçer.
    pub fn pick(h: u32) -> Self {
        match h % 100 {
            0..=54 => RewardKind::Coin,
            55..=79 => RewardKind::Banknote,
            80..=91 => RewardKind::Fuel,
            _ => RewardKind::Medal,
        }
    }
}

/// Toplanabilir ödül.
#[derive(Clone, Copy)]
pub struct Reward {
    pub x: f32,
    pub y: f32,
    pub active: bool,
    pub kind: RewardKind,
    /// Animasyon fazı.
    pub phase: f32,
}

/// Parçacık (egzoz dumanı, çarpışma gnisteri, drift tozu).
#[derive(Clone, Copy)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32, // kalan yaşam (saniye)
    pub max_life: f32,
    pub size: f32,
    pub color: u32,
    pub kind: ParticleKind,
}

/// Parçacık türü.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ParticleKind {
    /// Egzoz dumanı.
    Exhaust,
    /// Çarpışma gnisteri.
    Spark,
    /// Drift tozu.
    Dust,
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
// Yol ağı yardımcıları
// ---------------------------------------------------------------------------

/// Şeridin yol eksenine göre merkez noktası (x, y).
fn lane_point(axis: LaneAxis, road: usize, lane_dir: f32) -> (f32, f32) {
    let center = road as f32 * ROAD_PERIOD as f32 + ROAD_AXIS_OFF;
    match axis {
        // Yatay cadde: şerit merkezi eksenin altında (sağdan trafik).
        LaneAxis::Horizontal => (0.0, center - lane_dir * LANE_OFF),
        // Dikey cadde: şerit merkezi eksenin sağında.
        LaneAxis::Vertical => (center + lane_dir * LANE_OFF, 0.0),
    }
}

/// Şerit yön birim vektörü.
fn lane_dir_vec(axis: LaneAxis, lane_dir: f32) -> (f32, f32) {
    match axis {
        LaneAxis::Horizontal => (lane_dir, 0.0),
        LaneAxis::Vertical => (0.0, lane_dir),
    }
}

/// Yönün sağ vektörü (dik yönde, saat yönünün tersi).
#[inline(always)]
fn right_vec(dx: f32, dy: f32) -> (f32, f32) {
    (dy, -dx)
}

/// Verilen konumdan sonraki kavşak merkezinin koordinatı.
fn next_intersection(pos: f32, lane_dir: f32) -> f32 {
    let period = ROAD_PERIOD as f32;
    let base = floorf((pos - ROAD_AXIS_OFF) / period) * period + ROAD_AXIS_OFF;
    if lane_dir > 0.0 {
        if base > pos {
            base
        } else {
            base + period
        }
    } else if base < pos {
        base
    } else {
        base - period
    }
}

/// Kavşak merkezinin yol indeksi.
fn intersection_index(center: f32) -> usize {
    (((center - ROAD_AXIS_OFF) / ROAD_PERIOD as f32) + 0.5) as usize
}

/// Sprite'ın duvar arkasında kalmayan sütun aralıklarını hesaplar.
///
/// Bir bilboard sprite'ın bütün sütunları aynı derinlikte olduğundan duvar
/// tamponu (`zbuf`) sütun bazında kırpılabilir. Böylece araç önündeki
/// binanın arkasında kalır, binanın üstüne binmez — yolda kalan araç.
fn visible_runs(
    zbuf: &[f32; 1024],
    depth: f32,
    left: i32,
    right: i32,
) -> ([(usize, usize); 8], usize) {
    let mut runs = [(0usize, 0usize); 8];
    let mut count = 0usize;
    let mut col = left.max(0) as usize;
    let end = right.max(0) as usize;
    while col < end && col < zbuf.len() {
        if zbuf[col] > depth {
            let start = col;
            while col < end && col < zbuf.len() && zbuf[col] > depth {
                col += 1;
            }
            if count < runs.len() {
                runs[count] = (start, col);
                count += 1;
            }
        } else {
            col += 1;
        }
    }
    (runs, count)
}

/// Dikdörtgeni yalnızca sprite'ın görünür sütunlarına çizer.
#[allow(clippy::too_many_arguments)]
fn draw_clipped(
    fb: &mut Framebuffer,
    runs: &[(usize, usize); 8],
    count: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: u32,
) {
    if count == 0 {
        return;
    }
    let x_end = x.saturating_add(w);
    for run in runs.iter().take(count) {
        let (rx0, rx1) = *run;
        let a = x.max(rx0);
        let b = x_end.min(rx1);
        if b > a {
            fb.draw_rect(a, y, b - a, h, color);
        }
    }
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
    /// Direksiyon kolu konumu (-1..1): yumuşatılmış direksiyon komutu.
    pub steer_cmd: f32,
    // Girdi
    pub key_mask: u16,
    // Dünya
    pub traffic: [TrafficCar; MAX_TRAFFIC],
    pub rewards: [Reward; MAX_REWARDS],
    // Animasyon saati (s).
    pub anim_t: f32,
    // İstatistik
    pub score: u32,
    pub collected: u32,
    pub distance: f32,
    pub crashes: u32,
    pub crash_flash: u32,
    pub state: CityState,
    pub engine_freq: u32,
    // Gün/gece döngüsü (0.0..1.0).
    pub time_of_day: f32,
    // Yakıt (litre).
    pub fuel: f32,
    /// Drift açısı (rad, pozitif sağa, negatif sola).
    pub drift_angle: f32,
    /// Drift puanı.
    pub drift_score: u32,
    /// Drift dumanı sayacı.
    pub drift_smoke_timer: f32,
    // Parçacık sistemi.
    pub particles: [Particle; MAX_PARTICLES],
    // Zamanlayıcı (saniye).
    pub lap_timer: f32,
    /// Geçtiği son kontrol noktası indeksi.
    pub checkpoint: usize,
    /// En iyi tur zamanı (saniye).
    pub best_lap: f32,
    /// Kontrol noktaları (düz sabit bir döngü).
    pub checkpoints: [(f32, f32); 8],
}

impl CityDriveApp {
    pub fn new() -> Self {
        let mut app = CityDriveApp {
            map: CityMap::generate(),
            car_x: 20.5,
            car_y: 13.5,
            heading: 0.0,
            speed: 0.0,
            cam_dir: 0.0,
            steer_cmd: 0.0,
            key_mask: 0,
            traffic: [TrafficCar {
                x: 0.0,
                y: 0.0,
                dir_x: 1.0,
                dir_y: 0.0,
                speed: 0.0,
                target_speed: 0.0,
                color: 0,
                kind: VehicleKind::Sedan,
                length: 4.62,
                width: 1.78,
                height: 1.45,
                brake: 0.0,
                axis: LaneAxis::Horizontal,
                road: 0,
                lane_dir: 1.0,
                turn: None,
            }; MAX_TRAFFIC],
            rewards: [Reward {
                x: 0.0,
                y: 0.0,
                active: false,
                kind: RewardKind::Coin,
                phase: 0.0,
            }; MAX_REWARDS],
            anim_t: 0.0,
            score: 0,
            collected: 0,
            distance: 0.0,
            crashes: 0,
            crash_flash: 0,
            state: CityState::Menu,
            engine_freq: 0,
            time_of_day: 0.5,
            fuel: FUEL_CAPACITY,
            drift_angle: 0.0,
            drift_score: 0,
            drift_smoke_timer: 0.0,
            particles: [Particle {
                x: 0.0,
                y: 0.0,
                vx: 0.0,
                vy: 0.0,
                life: 0.0,
                max_life: 0.0,
                size: 0.0,
                color: 0,
                kind: ParticleKind::Exhaust,
            }; MAX_PARTICLES],
            lap_timer: 0.0,
            checkpoint: 0,
            best_lap: 0.0,
            checkpoints: [(0.0, 0.0); 8],
        };
        app.reset_world();
        app
    }

    /// Araç, trafik ve ödülleri başlangıç durumuna döndürür.
    pub fn reset_world(&mut self) {
        self.car_x = 20.5;
        self.car_y = 13.5;
        self.heading = 0.0;
        self.cam_dir = 0.0;
        self.speed = 0.0;
        self.steer_cmd = 0.0;
        self.key_mask = 0;
        self.anim_t = 0.0;
        self.distance = 0.0;
        self.crashes = 0;
        self.crash_flash = 0;
        self.time_of_day = 0.5;
        self.fuel = FUEL_CAPACITY;
        self.drift_angle = 0.0;
        self.drift_score = 0;
        self.drift_smoke_timer = 0.0;
        self.lap_timer = 0.0;
        self.checkpoint = 0;

        // Kontrol noktalarını şehrin dört köşesine ve orta noktalarına yerleştir.
        let cp: [(f32, f32); 8] = [
            (10.0, 10.0),
            (50.0, 10.0),
            (50.0, 50.0),
            (10.0, 50.0),
            (30.0, 10.0),
            (50.0, 30.0),
            (30.0, 50.0),
            (10.0, 30.0),
        ];
        self.checkpoints = cp;

        for p in self.particles.iter_mut() {
            p.life = 0.0;
        }

        self.spawn_traffic();
        self.spawn_rewards();
    }

    /// Trafik araçlarını şeritlere yerleştirir.
    ///
    /// Araçlar sağdan trafik kuralına uyar: şerit merkezi yol ekseninin
    /// sağındadır ve her araç kendi tipine uygun hızda sürer.
    fn spawn_traffic(&mut self) {
        // Yerleştirilen araçların konumları (çakışmayı önlemek için).
        let mut placed = [(0.0f32, 0.0f32); MAX_TRAFFIC];
        let mut placed_count = 0usize;

        for (i, traffic) in self.traffic.iter_mut().enumerate() {
            let h = hash4(i, 3, 5, 17);
            let kind = VehicleKind::pick(h);
            let (length, width, height) = kind.dims();
            let color = kind.paint(h >> 5);

            let axis = if (h >> 12) & 1 == 0 {
                LaneAxis::Horizontal
            } else {
                LaneAxis::Vertical
            };
            let lane_dir = if (h >> 8) & 1 == 0 { 1.0 } else { -1.0 };
            let road_count = match axis {
                LaneAxis::Horizontal => CITY_H / ROAD_PERIOD,
                LaneAxis::Vertical => CITY_W / ROAD_PERIOD,
            };
            let road = ((h >> 4) % road_count as u32) as usize;
            let (dx, dy) = lane_dir_vec(axis, lane_dir);
            let (rx, ry) = right_vec(dx, dy);
            let center = road as f32 * ROAD_PERIOD as f32 + ROAD_AXIS_OFF;
            // Şerit merkezi: yol ekseninin sağ tarafı.
            let (lane_x, lane_y) = match axis {
                LaneAxis::Horizontal => (0.0, center + ry * LANE_OFF),
                LaneAxis::Vertical => (center + rx * LANE_OFF, 0.0),
            };

            // Cadde boyunca oyuncuya ve diğer araçlara uzak bir nokta seç.
            let limit = match axis {
                LaneAxis::Horizontal => CITY_W,
                LaneAxis::Vertical => CITY_H,
            };
            let mut pos = 4.0;
            let mut found = false;
            for attempt in 0..12usize {
                let t = hash4(i, attempt, 23, 29);
                let cand = 3.0 + (t % (limit as u32 - 6)) as f32;
                let (cx, cy) = match axis {
                    LaneAxis::Horizontal => (cand, lane_y),
                    LaneAxis::Vertical => (lane_x, cand),
                };
                if (cx - self.car_x).abs() < 9.0 && (cy - self.car_y).abs() < 9.0 {
                    continue;
                }
                let mut ok = true;
                for p in placed.iter().take(placed_count) {
                    if (cx - p.0).abs() < 8.0 && (cy - p.1).abs() < 8.0 {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    pos = cand;
                    found = true;
                    break;
                }
            }
            let _ = found;
            let (x, y) = match axis {
                LaneAxis::Horizontal => (pos, lane_y),
                LaneAxis::Vertical => (lane_x, pos),
            };
            placed[placed_count] = (x, y);
            placed_count += 1;

            *traffic = TrafficCar {
                x,
                y,
                dir_x: dx,
                dir_y: dy,
                speed: kind.cruise() * 0.6,
                target_speed: kind.cruise(),
                color,
                kind,
                length,
                width,
                height,
                brake: 0.0,
                axis,
                road,
                lane_dir,
                turn: None,
            };
        }
    }

    /// Ödülleri cadde üzerine dağıtır.
    fn spawn_rewards(&mut self) {
        let round = self.collected as usize;
        for (i, reward) in self.rewards.iter_mut().enumerate() {
            let h = hash4(i, 29, 31, 37 + round.wrapping_mul(7));
            let kind = RewardKind::pick(h);
            let road = ((h >> 4) % (CITY_W / ROAD_PERIOD) as u32) as usize;
            let along = ((h >> 8) % (CITY_W as u32 - 6)) as f32 + 3.0;
            let horizontal = ((h >> 16) & 1) == 0;
            let center = road as f32 * ROAD_PERIOD as f32 + ROAD_AXIS_OFF;
            let (x, y) = if horizontal {
                (along, center + 0.45)
            } else {
                (center - 0.45, along)
            };
            *reward = Reward {
                x,
                y,
                active: true,
                kind,
                phase: (h % 628) as f32 * 0.01,
            };
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
        self.anim_t += dt;

        // --- Gün/gece döngüsü ---
        self.time_of_day += DAY_CYCLE_SPEED * dt;
        if self.time_of_day >= 1.0 {
            self.time_of_day -= 1.0;
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

        // --- Gaz ve fren ---
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

        // --- Rölanti ve hava direnci (karesel) ---
        let v_abs = absf(self.speed);

        // --- Yakıt tüketimi ---
        if v_abs > 0.5 {
            let consumption = FUEL_CONSUMPTION_BASE * (1.0 + v_abs * 0.15) * dt;
            self.fuel = (self.fuel - consumption).max(0.0);
            if self.fuel == 0.0 && self.speed != 0.0 {
                self.speed *= 0.5;
            }
        }

        if v_abs > 0.01 {
            let sgn = if self.speed > 0.0 { 1.0 } else { -1.0 };
            let coast = DRAG_BASE + DRAG_K * v_abs * v_abs;
            // Gaz basılıyken direncin yalnızca küçük bir kısmı uygulanır.
            let factor = if throttle == 0 { 1.0 } else { 0.22 };
            self.speed -= sgn * coast * factor * dt;
        }

        // --- El freni ---
        let handbrake = self.key_mask & KEY_HAND != 0 && v_abs > 0.05;
        if handbrake {
            let sgn = if self.speed > 0.0 { 1.0 } else { -1.0 };
            self.speed -= sgn * HANDBRAKE * dt;
        }
        if absf(self.speed) < 0.05 {
            self.speed = 0.0;
        }
        self.speed = self.speed.clamp(-MAX_REVERSE, MAX_SPEED);

        // --- Drift: el freni ile arka tekerlek traksiyonu kaybolur, araç sarkar ---
        let steer_input =
            (self.key_mask & KEY_RIGHT != 0) as i32 - (self.key_mask & KEY_LEFT != 0) as i32;
        let steer_target = steer_input as f32;
        let max_delta = STEER_RATE * dt;
        self.steer_cmd += (steer_target - self.steer_cmd).clamp(-max_delta, max_delta);

        let drift_active = handbrake && absf(self.steer_cmd) > 0.25 && v_abs > 3.0;
        if drift_active {
            // Drift açısı: direksiyon yönüne göre arka çıkış, sınırlı.
            let drift_target = self.steer_cmd * 1.6;
            self.drift_angle += (drift_target - self.drift_angle).clamp(-4.0 * dt, 4.0 * dt);
            // Drift'te ivme azalır, yanal sürüklenme oluşur.
            let fx = cos(self.heading + self.drift_angle * 0.5);
            let fy = sin(self.heading + self.drift_angle * 0.5);
            let nx = self.car_x + fx * self.speed * dt * 0.85;
            let ny = self.car_y + fy * self.speed * dt * 0.85;
            if !self.collides(nx, ny) {
                self.car_x = nx;
                self.car_y = ny;
            }
            self.heading += self.drift_angle * 0.5 * dt;
            self.drift_smoke_timer += dt;
            if self.drift_smoke_timer >= DRIFT_SMOKE_INTERVAL {
                self.drift_smoke_timer = 0.0;
                self.spawn_particle(ParticleKind::Dust, self.car_x, self.car_y, 0.0, 0.0);
                self.drift_score = self.drift_score.saturating_add(1);
            }
        } else {
            self.drift_angle *= 1.0 - 6.0 * dt;
            if absf(self.drift_angle) < 0.01 {
                self.drift_angle = 0.0;
            }
        }

        // --- Direksiyon: bisiklet modeli ---
        // Direksiyon kolu zaten drift bloğu içinde güncellendi; burada yalnızca
        // yaw (yaw hızı) hesaplanır.
        // Sarma yarıçapı hıza bağlıdır: yüksek hızda direksiyon açısı
        // daralır, dönüş çapı büyür (gerçek araç davranışı).
        let speed_abs = absf(self.speed);
        let r_min = TURN_R_MIN + TURN_R_SPEED_K * speed_abs * speed_abs;
        let tan_max = WHEELBASE / r_min; // tan(δ_max)
                                         // Yaw hızı: ω = v · tan(δ) / dingil mesafesi.
        let yaw = self.speed * self.steer_cmd * tan_max / WHEELBASE;
        self.heading += yaw * dt;
        if self.heading > core::f32::consts::PI {
            self.heading -= core::f32::consts::TAU;
        } else if self.heading < -core::f32::consts::PI {
            self.heading += core::f32::consts::TAU;
        }

        // --- İlerleme ve çarpışma (drift aktifse zaten drift bloğu içinde hareket edildi) ---
        if !drift_active {
            let fx = cos(self.heading);
            let fy = sin(self.heading);
            let nx = self.car_x + fx * self.speed * dt;
            let ny = self.car_y + fy * self.speed * dt;
            if self.collides(nx, ny) {
                if absf(self.speed) > 2.0 {
                    self.crashes += 1;
                    self.crash_flash = 14;
                    self.spawn_particle(
                        ParticleKind::Spark,
                        self.car_x,
                        self.car_y,
                        -fx * 2.0,
                        -fy * 2.0,
                    );
                }
                self.speed *= -0.18;
            } else {
                self.car_x = nx;
                self.car_y = ny;
            }
            self.car_x = self.car_x.clamp(0.4, CITY_W as f32 - 0.4);
            self.car_y = self.car_y.clamp(0.4, CITY_H as f32 - 0.4);
            self.distance += absf(self.speed) * dt;
        }

        // Yakıt bittiyse aracı durdur.
        if self.fuel <= 0.0 {
            self.speed = 0.0;
        }

        // --- Kamera yönü araca hafif gecikmeyle takip eder ---
        let mut diff = self.heading - self.cam_dir;
        while diff > core::f32::consts::PI {
            diff -= core::f32::consts::TAU;
        }
        while diff < -core::f32::consts::PI {
            diff += core::f32::consts::TAU;
        }
        self.cam_dir += diff * (8.0 * dt).min(1.0);

        // --- Parçacık sistemi güncellemesi ---
        self.update_particles(dt);

        // Egzoz dumanı (hızlıyken arka tekerleklerden).
        if v_abs > 4.0 {
            let exhaust_timer = self.anim_t;
            if (exhaust_timer * 30.0) as u32 % 4 == 0 {
                let back_x = self.car_x - cos(self.heading) * 1.2;
                let back_y = self.car_y - sin(self.heading) * 1.2;
                let smoke_vx = -cos(self.heading) * 0.5 + (self.anim_t % 1.0 - 0.5) * 0.2;
                let smoke_vy = -sin(self.heading) * 0.5 + (self.anim_t % 1.0 - 0.5) * 0.2;
                self.spawn_particle(ParticleKind::Exhaust, back_x, back_y, smoke_vx, smoke_vy);
            }
        }

        // --- Trafik ---
        self.update_traffic(dt);
        for i in 0..MAX_TRAFFIC {
            let car = self.traffic[i];
            let dx = car.x - self.car_x;
            let dy = car.y - self.car_y;
            let r = CAR_RADIUS + car.width * 0.5;
            if dx * dx + dy * dy < r * r {
                if self.crash_flash == 0 && absf(self.speed) > 2.0 {
                    self.crashes += 1;
                }
                self.speed *= 0.25;
                self.crash_flash = 10;
                // Çarpan araç biraz geri itilir.
                let mut c = car;
                c.x -= c.dir_x * 0.08;
                c.y -= c.dir_y * 0.08;
                self.traffic[i] = c;
            }
        }

        // --- Ödül toplama ---
        let mut remaining = 0u32;
        for i in 0..MAX_REWARDS {
            let reward = self.rewards[i];
            if !reward.active {
                remaining += 1;
                continue;
            }
            let dx = reward.x - self.car_x;
            let dy = reward.y - self.car_y;
            if dx * dx + dy * dy < 1.7 {
                let mut r = reward;
                r.active = false;
                self.rewards[i] = r;
                self.score += reward.kind.score();
                self.collected += 1;
            } else {
                remaining += 1;
            }
        }
        if remaining == MAX_REWARDS as u32 {
            // Tüm ödüller toplandı: yenileri belirir.
            self.spawn_rewards();
        }

        // --- Kontrol noktaları (zaman koşusu) ---
        if self.state == CityState::Driving {
            let cp = self.checkpoints[self.checkpoint];
            let dx = cp.0 - self.car_x;
            let dy = cp.1 - self.car_y;
            if dx * dx + dy * dy < 4.0 {
                self.checkpoint = (self.checkpoint + 1) % self.checkpoints.len();
                if self.checkpoint == 0 {
                    // Tur tamamlandı.
                    if self.best_lap == 0.0 || self.lap_timer < self.best_lap {
                        self.best_lap = self.lap_timer;
                    }
                    self.lap_timer = 0.0;
                    self.score = self.score.saturating_add(500);
                } else {
                    self.score = self.score.saturating_add(100);
                }
            }
            self.lap_timer += dt;
        }

        if self.crash_flash > 0 {
            self.crash_flash -= 1;
        }

        // --- Motor sesi: hıza göre frekans ---
        let ratio = (absf(self.speed) / MAX_SPEED).min(1.0);
        self.engine_freq = 40 + (ratio * 150.0) as u32;
    }

    // -----------------------------------------------------------------------
    // Trafik simülasyonu
    // -----------------------------------------------------------------------

    /// Trafik akışını günceller: şerit sürüşü, kavşak dönüşleri ve takip.
    fn update_traffic(&mut self, dt: f32) {
        for i in 0..MAX_TRAFFIC {
            let mut car = self.traffic[i];
            // Bu karede harcanacak süre (yaya geçişte kalan süreye düşer).
            let mut move_dt = dt;

            if car.turn.is_none() {
                // Düz hatta sürüş ve kavşak planı.
                if let Some((entry_along, arc)) = self.plan_turn(i, &car) {
                    let remaining = entry_along - car.along();
                    if remaining > 0.0 {
                        let t_to_entry = remaining / car.speed.max(0.5);
                        if t_to_entry <= dt {
                            // Kavşağa kadar düz gidip kalan süreyi yaya harcar.
                            car.x += car.dir_x * car.speed * t_to_entry;
                            car.y += car.dir_y * car.speed * t_to_entry;
                            car.turn = Some(arc);
                            move_dt = dt - t_to_entry;
                        }
                    }
                }
                if car.turn.is_none() {
                    car.x += car.dir_x * car.speed * move_dt;
                    car.y += car.dir_y * car.speed * move_dt;
                }
            }

            // Yay üzerindeki hareket.
            if let Some(mut arc) = car.turn {
                let step = car.speed * move_dt / arc.r;
                let (s, c) = (sin(step), cos(step));
                let (rx, ry) = (arc.rx, arc.ry);
                if arc.rot > 0.0 {
                    // Saat yönünün tersi.
                    arc.rx = rx * c - ry * s;
                    arc.ry = rx * s + ry * c;
                } else {
                    // Saat yönü.
                    arc.rx = rx * c + ry * s;
                    arc.ry = -rx * s + ry * c;
                }
                arc.swept += step;
                car.x = arc.cx + arc.rx;
                car.y = arc.cy + arc.ry;
                // Yön: yaya teğet.
                let (tx, ty) = if arc.rot > 0.0 {
                    (-arc.ry, arc.rx)
                } else {
                    (arc.ry, -arc.rx)
                };
                let inv = 1.0 / arc.r;
                car.dir_x = tx * inv;
                car.dir_y = ty * inv;
                if arc.swept >= core::f32::consts::FRAC_PI_2 {
                    // Dönüş bitti: araç kesin çıkış noktasına oturur ve
                    // yeni şerit yönüne tam hizalanır (kayma oluşmaz).
                    car.x = arc.exit_x;
                    car.y = arc.exit_y;
                    car.dir_x = arc.end_dir_x;
                    car.dir_y = arc.end_dir_y;
                    car.axis = arc.new_axis;
                    car.road = arc.new_road;
                    car.lane_dir = arc.new_lane_dir;
                    car.turn = None;
                } else {
                    car.turn = Some(arc);
                }
            }

            // Şehir sınırı: yol devam eder, araç karşı tarafa sarar.
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

            self.traffic[i] = car;
        }

        self.update_traffic_speed(dt);
    }

    /// Araçların hızını takip mesafesine ve hedef hıza göre ayarlar.
    fn update_traffic_speed(&mut self, dt: f32) {
        for i in 0..MAX_TRAFFIC {
            let car = self.traffic[i];
            let mut c = car;

            if c.turn.is_some() {
                // Kavşakta yavaşla, direksiyon düzelt.
                c.speed = c.speed.min(TURN_SPEED);
                c.brake = 0.6;
                self.traffic[i] = c;
                continue;
            }

            // Aynı şeritte öndeki araç.
            let mut gap = 60.0f32;
            let mut lead_speed = 30.0f32;
            for (j, other) in self.traffic.iter().enumerate() {
                if i == j {
                    continue;
                }
                if other.axis != c.axis || other.road != c.road || other.lane_dir != c.lane_dir {
                    continue;
                }
                let d = (other.along() - c.along()) * c.lane_dir;
                if d > 0.0 && d < gap {
                    gap = d;
                    lead_speed = other.speed;
                }
            }

            // Oyuncu da öndeki engeldir.
            let player_lateral = match c.axis {
                LaneAxis::Horizontal => self.car_y,
                LaneAxis::Vertical => self.car_x,
            };
            let (_, lane_y) = lane_point(c.axis, c.road, c.lane_dir);
            let lane_center = match c.axis {
                LaneAxis::Horizontal => lane_y,
                LaneAxis::Vertical => lane_point(c.axis, c.road, c.lane_dir).0,
            };
            if absf(player_lateral - lane_center) < 1.6 {
                let player_along = match c.axis {
                    LaneAxis::Horizontal => self.car_x * c.lane_dir,
                    LaneAxis::Vertical => self.car_y * c.lane_dir,
                };
                let d = (player_along - c.along()) * c.lane_dir;
                if d > 0.0 && d < gap {
                    gap = d;
                    lead_speed = 0.0;
                }
            }

            // Takip mesafesine göre hız.
            let safe = 5.0 + c.speed * 0.9;
            if gap < safe {
                c.speed -= 8.0 * dt;
                c.brake = 1.0;
            } else if gap < safe * 2.5 && c.speed > lead_speed {
                c.speed -= 4.0 * dt;
                c.brake = 0.55;
            } else {
                c.speed += c.kind.accel() * dt;
                c.brake = 0.0;
            }
            c.speed = c.speed.clamp(0.0, c.target_speed);

            // Şerit değiştirme: yavaş araçın arkasında kalmak yerine boş şerite geç.
            if gap < safe && c.speed < c.target_speed * 0.7 {
                let other_lane_dir = -c.lane_dir;
                let (_, other_lane_y) = lane_point(c.axis, c.road, other_lane_dir);
                let my_lateral = match c.axis {
                    LaneAxis::Horizontal => c.y,
                    LaneAxis::Vertical => c.x,
                };
                let lateral_gap = absf(my_lateral - other_lane_y);
                // Diğer şeritte aynı bölgede araç var mı?
                let mut other_lane_clear = true;
                for (j, other) in self.traffic.iter().enumerate() {
                    if i == j || other.turn.is_some() {
                        continue;
                    }
                    if other.axis != c.axis
                        || other.road != c.road
                        || other.lane_dir != other_lane_dir
                    {
                        continue;
                    }
                    let d = (other.along() - c.along()) * c.lane_dir;
                    if d > -12.0 && d < 12.0 {
                        other_lane_clear = false;
                        break;
                    }
                }
                if other_lane_clear && lateral_gap > 1.0 {
                    // Şerit değiştir: yönü ters çevir, yanal konumu ayarla.
                    c.lane_dir = other_lane_dir;
                    let (dx, dy) = lane_dir_vec(c.axis, c.lane_dir);
                    let (rtx, rty) = right_vec(dx, dy);
                    let center = c.road as f32 * ROAD_PERIOD as f32 + ROAD_AXIS_OFF;
                    let (lane_x, lane_y) = match c.axis {
                        LaneAxis::Horizontal => (0.0, center + rty * LANE_OFF),
                        LaneAxis::Vertical => (center + rtx * LANE_OFF, 0.0),
                    };
                    c.x = lane_x;
                    c.y = lane_y;
                    c.brake = 0.3;
                }
            }

            self.traffic[i] = c;
        }
    }

    /// Bir aracın sonraki kavşakta yapacağı manevrayı planlar.
    ///
    /// Deterministik bir seçimle (düz / sağa / sola) kavşak dönüş yayını
    /// üretir; giriş noktası aracın önündeyse `Some` döner.
    fn plan_turn(&self, index: usize, car: &TrafficCar) -> Option<(f32, TurnArc)> {
        let pos = match car.axis {
            LaneAxis::Horizontal => car.x,
            LaneAxis::Vertical => car.y,
        };
        let center = next_intersection(pos, car.lane_dir);
        let dist = (center - pos) * car.lane_dir;
        if dist > 20.0 || dist < -2.0 {
            return None;
        }
        let inter = intersection_index(center);
        let road_count = match car.axis {
            LaneAxis::Horizontal => CITY_W / ROAD_PERIOD,
            LaneAxis::Vertical => CITY_H / ROAD_PERIOD,
        };
        if inter >= road_count {
            return None;
        }
        let roll = hash4(index, car.road, inter, 91) % 100;
        let right_turn = if roll < 64 {
            return None; // Düz git
        } else {
            roll < 82
        };
        let arc = self.build_turn(car, inter, right_turn)?;

        // Giriş noktası araçtan ileride olmalı.
        let entry_x = arc.cx + arc.rx;
        let entry_y = arc.cy + arc.ry;
        let entry_along = match car.axis {
            LaneAxis::Horizontal => entry_x * car.lane_dir,
            LaneAxis::Vertical => entry_y * car.lane_dir,
        };
        if entry_along < car.along() - 0.5 {
            return None;
        }
        Some((entry_along, arc))
    }

    /// Kavşak dönüş yayını üretir.
    ///
    /// Mevcut şerit ile hedef şeritin kesiştiği köşeye, dönüş yarıçapı kadar
    /// uzaklıkta bir yay merkezi yerleştirilir.
    fn build_turn(&self, car: &TrafficCar, inter: usize, right_turn: bool) -> Option<TurnArc> {
        let (ax, ay) = lane_point(car.axis, car.road, car.lane_dir);
        let (dx, dy) = lane_dir_vec(car.axis, car.lane_dir);
        let (rtx, rty) = right_vec(dx, dy);

        let new_axis = match car.axis {
            LaneAxis::Horizontal => LaneAxis::Vertical,
            LaneAxis::Vertical => LaneAxis::Horizontal,
        };
        // Yeni yön: sağa dönüşte sağ vektörü, sola dönüşte onun tersi.
        let (ndx, ndy) = if right_turn { (rtx, rty) } else { (-rtx, -rty) };
        let new_lane_dir = match new_axis {
            LaneAxis::Horizontal => ndx,
            LaneAxis::Vertical => ndy,
        };
        let (bx, by) = lane_point(new_axis, inter, new_lane_dir);
        let (nrx, nry) = right_vec(ndx, ndy);

        // Giriş noktası: iki şerit arasındaki açıklığı kapatacak konum.
        let r = TURN_ARC_R;
        let (mut ex, mut ey) = (bx - ax, by - ay);
        if right_turn {
            ex += r * (nrx - rtx);
            ey += r * (nry - rty);
        } else {
            ex += r * (rtx - nrx);
            ey += r * (rty - nry);
        }
        let t = ex * dx + ey * dy;
        let entry_x = ax + dx * t;
        let entry_y = ay + dy * t;

        // Yayın çıkış noktası: yeni şerit üzerindeki kesin konum.
        let s = -(ex * ndx + ey * ndy);
        let exit_x = bx + ndx * s;
        let exit_y = by + ndy * s;

        // Yay merkezi ve başlangıç yarıçap vektörü.
        let (cx, cy, rx, ry, rot) = if right_turn {
            (
                entry_x + rtx * r,
                entry_y + rty * r,
                -rtx * r,
                -rty * r,
                -1.0,
            )
        } else {
            (entry_x - rtx * r, entry_y - rty * r, rtx * r, rty * r, 1.0)
        };

        Some(TurnArc {
            cx,
            cy,
            r,
            rx,
            ry,
            swept: 0.0,
            rot,
            exit_x,
            exit_y,
            end_dir_x: ndx,
            end_dir_y: ndy,
            new_axis,
            new_road: inter,
            new_lane_dir,
        })
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

    /// Yeni bir parçacık oluşturur (boş slot varsa).
    fn spawn_particle(&mut self, kind: ParticleKind, x: f32, y: f32, vx: f32, vy: f32) {
        let (life, size, color) = match kind {
            ParticleKind::Exhaust => (0.8, 3.0, 0x6B7280),
            ParticleKind::Spark => (0.4, 2.0, 0xFFE08C),
            ParticleKind::Dust => (0.6, 2.5, 0x9CA3AF),
        };
        for p in self.particles.iter_mut() {
            if p.life <= 0.0 {
                *p = Particle {
                    x,
                    y,
                    vx,
                    vy,
                    life,
                    max_life: life,
                    size,
                    color,
                    kind,
                };
                return;
            }
        }
    }

    /// Tüm parçacıkları günceller ve süresi dolanları temizler.
    fn update_particles(&mut self, dt: f32) {
        for p in self.particles.iter_mut() {
            if p.life <= 0.0 {
                continue;
            }
            p.life -= dt;
            if p.life > 0.0 {
                p.x += p.vx * dt;
                p.y += p.vy * dt;
                p.vx *= 1.0 - 4.0 * dt;
                p.vy *= 1.0 - 4.0 * dt;
                p.size *= p.life / p.max_life;
            }
        }
    }

    /// Parçacığı ekranda çizer (zemin izdüşümüyle).
    fn draw_particles(
        &self,
        fb: &mut Framebuffer,
        _top: usize,
        win_w: usize,
        horizon: usize,
        focal: f32,
        zbuf: &[f32; 1024],
    ) {
        for p in self.particles.iter() {
            if p.life <= 0.0 {
                continue;
            }
            let (screen_x, _depth, scale) = match self.project(p.x, p.y, win_w, focal, zbuf) {
                Some(v) => v,
                None => continue,
            };
            let t = p.life / p.max_life;
            let alpha = t.clamp(0.0, 1.0);
            let size = (p.size * scale * 0.5).max(1.0) as usize;
            let color = shade(p.color, alpha);
            let y0 = (horizon as f32 + CAM_H * scale - size as f32 * 0.5) as usize;
            let x0 = screen_x.saturating_sub(size as i32 / 2) as usize;
            fb.draw_rect(x0, y0, size, size, color);
        }
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
        let top = win_y + crate::gui::scaled_titlebar_h();
        let view_h = win_h.saturating_sub(crate::gui::scaled_titlebar_h() + 1);
        if view_h < 64 {
            return true;
        }
        let horizon = top + view_h / 2;
        let dir_x = cos(self.cam_dir);
        let dir_y = sin(self.cam_dir);
        let plane_x = -dir_y * PLANE_LEN;
        let plane_y = dir_x * PLANE_LEN;
        let focal = view_h as f32 * 0.5;

        // 1) Gökyüzü: gün/gece döngüsüne bağlı dikey gradyan ve güneş/ay.
        let is_night = self.time_of_day > NIGHT_START || self.time_of_day < 0.28;
        let night_t = if self.time_of_day > NIGHT_START {
            ((self.time_of_day - NIGHT_START) / NIGHT_FADE).clamp(0.0, 1.0)
        } else if self.time_of_day < 0.28 {
            ((0.28 - self.time_of_day) / NIGHT_FADE).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sky_top = if is_night {
            let a = NIGHT_SKY_TOP;
            let b = DAY_SKY_TOP;
            lerp_sky(a, b, night_t)
        } else {
            let a = DAY_SKY_TOP;
            let b = NIGHT_SKY_TOP;
            lerp_sky(a, b, night_t)
        };
        let sky_horizon = if is_night {
            lerp_sky(NIGHT_SKY_HORIZON, DAY_SKY_HORIZON, night_t)
        } else {
            lerp_sky(DAY_SKY_HORIZON, NIGHT_SKY_HORIZON, night_t)
        };
        for row in 0..(horizon - top) {
            if budget.is_expired() {
                return false;
            }
            let t = row as f32 / (horizon - top).max(1) as f32;
            let r = lerp_u8((sky_top >> 16) & 0xFF, (sky_horizon >> 16) & 0xFF, t);
            let g = lerp_u8((sky_top >> 8) & 0xFF, (sky_horizon >> 8) & 0xFF, t);
            let b = lerp_u8(sky_top & 0xFF, sky_horizon & 0xFF, t);
            fb.draw_rect(win_x, top + row, win_w, 1, (r << 16) | (g << 8) | b);
        }
        if is_night {
            // Ay: küçük, beyaz-mavi.
            let moon_x = win_x + (win_w as f32 * 0.22) as usize;
            let moon_y = top + ((horizon - top) as f32 * 0.30) as usize;
            for dy in -5i32..=5 {
                for dx in -5i32..=5 {
                    let d2 = dx * dx + dy * dy;
                    if d2 <= 20 {
                        let color = if d2 <= 8 { 0xE8EEFF } else { 0xC8D4F0 };
                        fb.draw_rect(
                            (moon_x as i32 + dx).max(0) as usize,
                            (moon_y as i32 + dy).max(0) as usize,
                            1,
                            1,
                            color,
                        );
                    }
                }
            }
        } else {
            // Güneş.
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
                let mut color = self.floor_color(fx, fy, dist);
                // Gece gölgelemesi.
                if is_night {
                    let night_f = 0.35 + 0.35 * night_t;
                    color = shade(color, night_f);
                }
                fb.draw_rect(win_x + col, row, 4.min(win_w - col), 2, color);
            }
        }

        // 3) Binalar: raycasting duvar sütunları.
        // Geniş pencerelerde sprite derinlik testi de tam görünürlük alanını
        // kapsasın; 512 sütunluk tampon yüksek çözünürlükte araçları kırpıyordu.
        let mut zbuf = [MAX_VIEW; 1024];
        let col_step = if win_w >= 500 { 2 } else { 1 };
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
            let roof_color = shade(tone, factor * 0.55);
            let win_color = shade(WINDOW_COLOR, factor);
            let lit_color = shade(WINDOW_LIT_COLOR, factor);
            let cornice = shade(tone, factor * 0.82);

            // Yatay doku koordinatı (sütun başına sabit).
            let wall_u = if side == 0 {
                fractf(perp * ry)
            } else {
                fractf(perp * rx)
            };
            let window_x = wall_u > 0.12 && wall_u < 0.88;

            let row_start = (y_top.max(top as f32) as usize).min(top + view_h);
            let row_end = (y_bottom.max(top as f32) as usize).min(top + view_h);
            if row_start >= row_end {
                continue;
            }
            for row in row_start..row_end {
                let z = CAM_H + (horizon as f32 - row as f32) / scale;
                let storey_f = fractf(z / STOREY_H);
                let storey_idx = floorf(z / STOREY_H) as i32;
                let is_top_row = row == row_start;

                // Tavan/çatı çizgısı (en üst satır).
                if is_top_row && storeys > 0.0 {
                    fb.draw_rect(win_x + col, row, col_step, 1, roof_color);
                    continue;
                }
                // Körnice: her kat arası ince finta çizgi.
                if storey_idx > 0 && storey_f < 0.03 {
                    fb.draw_rect(win_x + col, row, col_step, 1, cornice);
                    continue;
                }
                // Zemin katı kapısı (en altta, orta bölgede).
                if storey_idx == 0 && storey_f > 0.78 && storey_f < 0.92 && window_x {
                    fb.draw_rect(win_x + col, row, col_step, 1, shade(0x3A3A3A, factor));
                    continue;
                }
                // Pencereler: kat başına iki pencere, gece yanık.
                let in_window_band = storey_f > 0.12 && storey_f < 0.55;
                let window_col = wall_u > 0.22 && wall_u < 0.42 || wall_u > 0.58 && wall_u < 0.78;
                let lit = is_night
                    && ((hit_x * 13 + hit_y * 7 + row as i32 * 3 + storey_idx * 11) % 7) == 0;
                if in_window_band && window_col && window_x {
                    let color = if lit { lit_color } else { win_color };
                    fb.draw_rect(win_x + col, row, col_step, 1, color);
                    continue;
                }
                fb.draw_rect(win_x + col, row, col_step, 1, wall_color);
            }
        }

        // 4) Sprite'lar: ödüller, trafik araçları ve parçacıklar.
        for reward in self.rewards.iter() {
            if !reward.active {
                continue;
            }
            self.draw_reward(fb, top, win_w, horizon, focal, &zbuf, reward);
        }
        self.draw_traffic_3d(
            fb, win_x, top, win_w, view_h, horizon, focal, &zbuf, &budget,
        );
        self.draw_particles(fb, top, win_w, horizon, focal, &zbuf);

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
        let ly = modf(fy, ROAD_PERIOD as f32);
        let lx = modf(fx, ROAD_PERIOD as f32);

        // Kavşakta yaya geçişi (beyaz kalın çizgiler).
        let inter_x = absf(ly - ROAD_AXIS_OFF) < 0.12;
        let inter_y = absf(lx - ROAD_AXIS_OFF) < 0.12;
        if kind == T_ROAD && (inter_x || inter_y) {
            let cross = if inter_x {
                modf(fx, 1.6) < 0.8
            } else {
                modf(fy, 1.6) < 0.8
            };
            if cross {
                return shade(0xF0F0F0, shade_f);
            }
        }

        // Yol kenar çizgileri (iki yönü ayıran kesikli sarı çizgi).
        if kind == T_ROAD
            && (absf(ly) < 0.11
                || absf(ly - ROAD_HALF as f32) < 0.11
                || absf(lx) < 0.11
                || absf(lx - ROAD_HALF as f32) < 0.11)
        {
            return shade(EDGE_LINE_COLOR, shade_f);
        }
        // Şerit işaretleri (kırık beyaz çizgi).
        if absf(ly - ROAD_AXIS_OFF) < 0.15 && modf(fx, 3.2) < 1.9 {
            return shade(LANE_COLOR, shade_f);
        }
        if absf(lx - ROAD_AXIS_OFF) < 0.15 && modf(fy, 3.2) < 1.9 {
            return shade(LANE_COLOR, shade_f);
        }
        // Kıvırcık (curb): cadde ile kaldırım sınırı.
        if kind == T_SIDEWALK {
            let curb_x = absf(lx) < 0.06 || absf(lx - ROAD_HALF as f32) < 0.06;
            let curb_y = absf(ly) < 0.06 || absf(ly - ROAD_HALF as f32) < 0.06;
            if curb_x || curb_y {
                return shade(0xB8B8B8, shade_f);
            }
        }
        let roughness = if kind == T_ROAD {
            0.94 + (hash4(cx as usize, cy as usize, 53, 17) % 7) as f32 * 0.015
        } else {
            1.0
        };
        let mut color = shade(base, shade_f * roughness);
        // Kaldırım taş dokusu (1 m plaka); asfalt düz kalır.
        if kind == T_SIDEWALK && (cx + cy) & 1 == 0 {
            color = shade(color, 0.90);
        }
        color
    }

    /// Bir dünya noktasının ekran projeksiyonunu hesaplar (derinlik testi ile).
    fn project(
        &self,
        world_x: f32,
        world_y: f32,
        win_w: usize,
        focal: f32,
        zbuf: &[f32; 1024],
    ) -> Option<(i32, f32, f32)> {
        let dir_x = cos(self.cam_dir);
        let dir_y = sin(self.cam_dir);
        let plane_x = -dir_y * PLANE_LEN;
        let plane_y = dir_x * PLANE_LEN;
        let rel_x = world_x - self.car_x;
        let rel_y = world_y - self.car_y;
        let depth = rel_x * dir_x + rel_y * dir_y;
        if depth <= 0.25 {
            return None;
        }
        let lateral = rel_x * plane_x + rel_y * plane_y;
        // Duvar izdüşümüyle aynı kamera x'i: ışın `plane * PLANE_LEN * cxf`
        // kullandığından yatay koordinat PLANE_LEN'e bölünür. Bu olmadan
        // ödüller ve işaretçiler duvarların ~PLANE_LEN² oranında
        // sıkışmış/kenara kaymış görünürdü.
        let cam_x = lateral / (depth * PLANE_LEN);
        if cam_x < -1.0 || cam_x > 1.0 {
            return None;
        }
        let screen_x = ((cam_x + 1.0) * 0.5 * win_w as f32) as i32;
        if screen_x < 0 || screen_x as usize >= win_w.min(1024) {
            return None;
        }
        let depth = depth.min(MAX_VIEW);
        if depth >= zbuf[screen_x as usize] {
            return None;
        }
        Some((screen_x, depth, focal / depth))
    }

    /// Trafik aracını bilboard olarak çizer.
    ///
    /// Araç yönü ile görüş yönüne göre ön / arka / yan görünüş seçilir:
    /// önde farlar, arkada stop lambaları, yanda tekerlekler görünür.
    fn draw_traffic_3d(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        top: usize,
        win_w: usize,
        view_h: usize,
        horizon: usize,
        focal: f32,
        zbuf: &[f32; 1024],
        budget: &TimeBudget,
    ) {
        use crate::apps::gfx3d::{self, Box3D, Camera3D, Face3D, PixelSink, ProjVertex, Shading};

        struct FbSink<'a> {
            fb: &'a mut Framebuffer,
            win_x: usize,
            top: usize,
        }
        impl<'a> PixelSink for FbSink<'a> {
            fn put(&mut self, x: usize, y: usize, color: u32) {
                self.fb.put_pixel(self.win_x + x, self.top + y, color);
            }
        }

        let dir_x = cos(self.cam_dir);
        let dir_y = sin(self.cam_dir);
        let cam = Camera3D {
            pos_x: self.car_x,
            pos_y: self.car_y,
            dir_x,
            dir_y,
            // `plane` birim dik vektördür; görüş açısı `focal_x` ile
            // belirlenir. Bu, araçların şehir ızgarasıyla aynı geometride
            // buluşmasını sağlar (önceki eşleme birebir korunmuştur).
            plane_x: -dir_y,
            plane_y: dir_x,
            // Kamera yüksekliği duvar izdüşümüyle aynı olmalı (CAM_H);
            // aksi halde araçlar yerden hafif yukarıda "süzülür".
            eye_z: CAM_H,
            horizon_y: horizon as f32,
            v_scale: focal,
            // Duvar ışınları `plane` vektörünü PLANE_LEN ile ölçekler
            // (`ray = dir + plane_unit * PLANE_LEN * cxf`). Sprite
            // izdüşümünün aynı ekran x'ini üretmesi için odak uzaklığı
            // PLANE_LEN'e BÖLÜNMELİ; çarpılması sprite'ları yatayda
            // merkeze doğru ~2 kat sıkıştırıyordu.
            focal_x: win_w as f32 * 0.5 / PLANE_LEN,
            view_w: win_w as f32,
        };
        let is_night = self.time_of_day > NIGHT_START || self.time_of_day < 0.28;
        let shading = Shading {
            light: (0.4, 0.3, 0.85),
            ambient: if is_night { 0.35 } else { 0.55 },
            diffuse: if is_night { 0.35 } else { 0.40 },
            fill: 0.15,
            fog_distance: MAX_VIEW,
            fog_floor: 0.20,
        };

        // Her araç için gövde + kabin + far/stop + 4 tekerlek = en fazla 8 kutu -> 48 yüz.
        // Uzaklığa göre en yakın 8 aracı tam 3B çiz.
        let mut faces = [Face3D {
            depth: 0.0,
            color: 0,
            v: [ProjVertex {
                x: 0.0,
                y: 0.0,
                depth: 0.0,
            }; 4],
        }; 384];
        let mut face_count = 0usize;

        for car in self.traffic.iter() {
            let dx = car.x - self.car_x;
            let dy = car.y - self.car_y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > MAX_VIEW * MAX_VIEW || dist_sq < 0.25 {
                continue;
            }
            if face_count + 48 > faces.len() {
                break;
            }

            let (length, width, height) = car.kind.dims();
            let fwd = (car.dir_x, car.dir_y, 0.0);
            let right = (-car.dir_y, car.dir_x, 0.0);
            let up = (0.0, 0.0, 1.0);
            let axes = [fwd, right, up];

            let body_h = height * 0.52;
            let wheel_r = (height * 0.18).max(0.22);
            let body_z = wheel_r * 0.5 + body_h * 0.5;
            let body_colors = [car.color; 6];
            gfx3d::push_box(
                &mut faces,
                &mut face_count,
                &cam,
                &Box3D {
                    center: (car.x, car.y, body_z),
                    half: (length * 0.48, width * 0.48, body_h * 0.5),
                    axes,
                    colors: body_colors,
                    skip: 0b000010,
                    emissive: false,
                },
                &shading,
            );

            // Kabin / camlı üst bölüm
            let cabin_h = height * 0.40;
            let cabin_z = wheel_r * 0.5 + body_h + cabin_h * 0.5;
            let cabin_shift = -length * 0.06;
            let glass = 0x243448;
            let roof = shade(car.color, 0.88);
            gfx3d::push_box(
                &mut faces,
                &mut face_count,
                &cam,
                &Box3D {
                    center: (
                        car.x + fwd.0 * cabin_shift,
                        car.y + fwd.1 * cabin_shift,
                        cabin_z,
                    ),
                    half: (length * 0.26, width * 0.41, cabin_h * 0.5),
                    axes,
                    colors: [roof, roof, glass, glass, glass, glass],
                    skip: 0b000010,
                    emissive: false,
                },
                &shading,
            );

            // 4 tekerlek
            let wx = length * 0.31;
            let wy = width * 0.47;
            let wheel_colors = [0x1A1C20; 6];
            for &(sx, sy) in &[(-wx, -wy), (-wx, wy), (wx, -wy), (wx, wy)] {
                gfx3d::push_box(
                    &mut faces,
                    &mut face_count,
                    &cam,
                    &Box3D {
                        center: (
                            car.x + fwd.0 * sx + right.0 * sy,
                            car.y + fwd.1 * sx + right.1 * sy,
                            wheel_r * 0.5,
                        ),
                        half: (wheel_r * 0.9, width * 0.08, wheel_r * 0.5),
                        axes,
                        colors: wheel_colors,
                        skip: 0b000010,
                        emissive: false,
                    },
                    &shading,
                );
            }

            // Ön farlar ve arka stop lambaları
            let front_off = length * 0.485;
            let rear_off = -length * 0.485;
            let light_y = width * 0.30;
            let light_z = body_z;
            for &sy in &[-light_y, light_y] {
                gfx3d::push_box(
                    &mut faces,
                    &mut face_count,
                    &cam,
                    &Box3D {
                        center: (
                            car.x + fwd.0 * front_off + right.0 * sy,
                            car.y + fwd.1 * front_off + right.1 * sy,
                            light_z,
                        ),
                        half: (0.04, width * 0.10, body_h * 0.16),
                        axes,
                        colors: [0xFFF3C4; 6],
                        skip: 0b000010,
                        emissive: is_night,
                    },
                    &shading,
                );
                gfx3d::push_box(
                    &mut faces,
                    &mut face_count,
                    &cam,
                    &Box3D {
                        center: (
                            car.x + fwd.0 * rear_off + right.0 * sy,
                            car.y + fwd.1 * rear_off + right.1 * sy,
                            light_z,
                        ),
                        half: (0.04, width * 0.10, body_h * 0.16),
                        axes,
                        colors: [0xFF2B2B; 6],
                        skip: 0b000010,
                        emissive: true,
                    },
                    &shading,
                );
            }
        }

        if face_count > 0 {
            let mut sink = FbSink { fb, win_x, top };
            gfx3d::draw_faces(
                &mut sink,
                win_w,
                view_h,
                &zbuf[..win_w.min(1024)],
                &mut faces,
                face_count,
                budget,
            );
        }
    }

    fn draw_reward(
        &self,
        fb: &mut Framebuffer,
        win_y: usize,
        win_w: usize,
        horizon: usize,
        focal: f32,
        zbuf: &[f32; 1024],
        reward: &Reward,
    ) {
        let (screen_x, depth, scale) = match self.project(reward.x, reward.y, win_w, focal, zbuf) {
            Some(v) => v,
            None => return,
        };
        let (h_world, w_world) = reward.kind.dims();
        // Havada hafif salınım.
        let bob = sin(self.anim_t * 2.0 + reward.phase) * 0.07;
        let size_y = (h_world * scale).max(4.0) as usize;
        let size_x = (w_world * scale).max(4.0) as usize;
        let bottom = horizon as f32 + (CAM_H - (0.32 + bob)) * scale;
        let top = bottom - size_y as f32;
        let y0 = (win_y as f32 + top).max(0.0) as usize;
        let left = screen_x - (size_x / 2) as i32;
        let lx = left.max(0) as usize;
        let lw = size_x.max(1);
        let factor = 0.55 + 0.45 * (1.0 - (depth / MAX_VIEW).clamp(0.0, 1.0));

        // Duvar arkasında kalan sütunları kırp.
        let (runs, run_count) = visible_runs(zbuf, depth, left, left + size_x as i32);
        if run_count == 0 {
            return;
        }

        match reward.kind {
            RewardKind::Coin => {
                // Madeni para: altın disk, kalın kenar ve parlak merkez.
                let r = size_y as f32 * 0.5;
                let base = shade(0xD9A62B, factor);
                let rim = shade(0xF7DE8C, factor);
                let shine = shade(0xFFF4C8, factor);
                for row in 0..size_y {
                    let dy = row as f32 + 0.5 - r;
                    let hw = sqrtf((r * r - dy * dy).max(0.0));
                    if hw < 0.5 {
                        continue;
                    }
                    let w = (hw * 2.0) as usize;
                    let x0 = screen_x.saturating_sub((w as i32) / 2).max(0) as usize;
                    let color = if hw > r * 0.80 {
                        rim
                    } else if dy < -r * 0.25 && hw < r * 0.45 {
                        shine
                    } else {
                        base
                    };
                    draw_clipped(fb, &runs, run_count, x0, y0 + row, w, 1, color);
                }
            }
            RewardKind::Banknote => {
                // Banknot: yeşil kâğıt, açık kenar ve orta oval.
                let body = shade(0x5FBF78, factor);
                let edge = shade(0xA8E6B6, factor);
                let inner = shade(0x3E9E58, factor);
                draw_clipped(fb, &runs, run_count, lx, y0, lw, size_y, body);
                draw_clipped(fb, &runs, run_count, lx, y0, lw, 1, edge);
                draw_clipped(fb, &runs, run_count, lx, y0 + size_y - 1, lw, 1, edge);
                draw_clipped(fb, &runs, run_count, lx + 1, y0, 1, size_y, edge);
                draw_clipped(fb, &runs, run_count, lx + lw - 2, y0, 1, size_y, edge);
                let iw = (lw * 44 / 100).max(1);
                let ih = (size_y * 44 / 100).max(1);
                draw_clipped(
                    fb,
                    &runs,
                    run_count,
                    screen_x.saturating_sub((iw as i32) / 2).max(0) as usize,
                    y0 + (size_y - ih) / 2,
                    iw,
                    ih,
                    inner,
                );
            }
            RewardKind::Fuel => {
                // Yakıt bidonu: kırmızı gövde, koyu kapak ve tutamak.
                let body = shade(0xD2503F, factor);
                let edge = shade(0x9C3629, factor);
                let cap = shade(0x2A2E34, factor);
                draw_clipped(fb, &runs, run_count, lx, y0, lw, size_y, body);
                draw_clipped(fb, &runs, run_count, lx, y0 + size_y - 1, lw, 1, edge);
                draw_clipped(fb, &runs, run_count, lx, y0, 1, size_y, edge);
                draw_clipped(fb, &runs, run_count, lx + lw - 2, y0, 1, size_y, edge);
                // Kapak.
                draw_clipped(
                    fb,
                    &runs,
                    run_count,
                    screen_x.saturating_sub(lw as i32 * 15 / 100).max(0) as usize,
                    y0.saturating_sub(2),
                    (lw * 30 / 100).max(2),
                    2,
                    cap,
                );
                // Tutamak.
                let handle = (size_y * 18 / 100).max(2);
                draw_clipped(
                    fb,
                    &runs,
                    run_count,
                    lx + (lw * 32 / 100),
                    y0.saturating_sub(handle),
                    (lw * 30 / 100).max(2),
                    handle,
                    cap,
                );
            }
            RewardKind::Medal => {
                // Madalya: kurşun şeritler ve altın disk.
                let ribbon = shade(0xD0402E, factor);
                let ribbon2 = shade(0x2C5FA8, factor);
                let base = shade(0xD9A62B, factor);
                let rim = shade(0xF7DE8C, factor);
                let r = size_y as f32 * 0.36;
                let disc_top = y0 + (size_y as f32 * 0.30) as usize;
                // Şeritler.
                draw_clipped(
                    fb,
                    &runs,
                    run_count,
                    lx + (lw * 14 / 100),
                    y0,
                    (lw * 14 / 100).max(2),
                    disc_top.saturating_sub(y0),
                    ribbon,
                );
                draw_clipped(
                    fb,
                    &runs,
                    run_count,
                    lx + (lw * 72 / 100),
                    y0,
                    (lw * 14 / 100).max(2),
                    disc_top.saturating_sub(y0),
                    ribbon2,
                );
                // Disk.
                for row in 0..((r * 2.0) as usize) {
                    let dy = row as f32 + 0.5 - r;
                    let hw = sqrtf((r * r - dy * dy).max(0.0));
                    if hw < 0.5 {
                        continue;
                    }
                    let w = (hw * 2.0) as usize;
                    let x0 = screen_x.saturating_sub((w as i32) / 2).max(0) as usize;
                    let color = if hw > r * 0.78 { rim } else { base };
                    draw_clipped(fb, &runs, run_count, x0, disc_top + row, w, 1, color);
                }
            }
        }

        // 3D hacim: ödülün üst yüzü ve sağ yan yüzü.
        let r_top_h = (size_y * 8 / 100).max(1);
        draw_clipped(
            fb,
            &runs,
            run_count,
            lx,
            y0.saturating_sub(r_top_h),
            lw,
            r_top_h,
            shade(0xE8E8EC, factor.min(1.2)),
        );
        let r_side_w = (lw * 14 / 100).max(1);
        draw_clipped(
            fb,
            &runs,
            run_count,
            lx + lw.saturating_sub(r_side_w),
            y0,
            r_side_w,
            size_y,
            shade(0x555555, factor),
        );
    }

    /// HUD: hız göstergesi, dönüş yarıçapı, skor, mini harita, kontrol ipuçları.
    fn render_hud(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        // Hız / RPM / yakıt / drift paneli (sol alt).
        let panel_w = 260.min(win_w.saturating_sub(20));
        let panel_h = 104;
        let px = win_x + 10;
        let py = win_y + win_h.saturating_sub(panel_h + 10);
        fb.draw_rect(px, py, panel_w, panel_h, 0x141922);
        fb.draw_rect_outline(px, py, panel_w, panel_h, 0x2E3A4A);

        // Hız.
        let speed_kmh = (absf(self.speed) * 3.6) as u32;
        let mut speed_buf = crate::gui::TextBuf::new();
        speed_buf.push_usize(speed_kmh as usize);
        speed_buf.push_str(" ");
        speed_buf.push_str(crate::gui::ui_text("km/sa", "km/h"));
        MinGuiCompositor::draw_string(fb, px + 10, py + 8, speed_buf.as_str(), Color::TEXT_MAIN);

        // Hız çubuğu.
        let bar_x = px + 10;
        let bar_y = py + 22;
        let bar_w = panel_w - 20;
        let bar_h = 10;
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

        // RPM (sanal: hıza ve direksiyona bağlı).
        let rpm =
            ((absf(self.speed) / MAX_SPEED) * 8000.0 * (1.0 + absf(self.steer_cmd) * 0.3)) as u32;
        let mut rpm_buf = crate::gui::TextBuf::new();
        rpm_buf.push_str(crate::gui::ui_text("RPM: ", "RPM: "));
        rpm_buf.push_usize(rpm as usize);
        MinGuiCompositor::draw_string(fb, px + 10, py + 36, rpm_buf.as_str(), Color::TEXT_MUTED);

        // Vites (sanal: hıza bağlı).
        let gear = if self.speed < -0.5 {
            crate::gui::ui_text("R", "R")
        } else if self.speed < 2.0 {
            crate::gui::ui_text("N", "N")
        } else if self.speed < 8.0 {
            crate::gui::ui_text("1", "1")
        } else if self.speed < 14.0 {
            crate::gui::ui_text("2", "2")
        } else if self.speed < 20.0 {
            crate::gui::ui_text("3", "3")
        } else {
            crate::gui::ui_text("4", "4")
        };
        let mut gear_buf = crate::gui::TextBuf::new();
        gear_buf.push_str(crate::gui::ui_text("Vit: ", "Gear: "));
        gear_buf.push_str(gear);
        MinGuiCompositor::draw_string(fb, px + 90, py + 36, gear_buf.as_str(), Color::TEXT_MUTED);

        // Yakıt çubuğu.
        let fuel_bar_h = 8;
        let fuel_label = crate::gui::ui_text("Yakıt: ", "Fuel: ");
        MinGuiCompositor::draw_string(fb, px + 10, py + 48, fuel_label, Color::TEXT_MUTED);
        let fuel_ratio = (self.fuel / FUEL_CAPACITY).clamp(0.0, 1.0);
        let fuel_fill_w = ((panel_w - 20 - 40) as f32 * fuel_ratio) as usize;
        let fuel_color = if fuel_ratio < 0.15 {
            0xFF6B6B
        } else if fuel_ratio < 0.30 {
            0xF0B040
        } else {
            0x62D6A8
        };
        fb.draw_rect(px + 50, py + 58, panel_w - 60, fuel_bar_h, 0x232A35);
        fb.draw_rect(px + 50, py + 58, fuel_fill_w, fuel_bar_h, fuel_color);
        if fuel_ratio < 0.15 {
            let mut warn_buf = crate::gui::TextBuf::new();
            warn_buf.push_str(crate::gui::ui_text("Düşük yakıt!", "Low fuel!"));
            MinGuiCompositor::draw_string(fb, px + 10, py + 68, warn_buf.as_str(), 0xFF6B6B);
        }

        // Drift puanı.
        if self.drift_score > 0 {
            let mut drift_buf = crate::gui::TextBuf::new();
            drift_buf.push_str(crate::gui::ui_text("Drift: ", "Drift: "));
            drift_buf.push_usize(self.drift_score as usize);
            MinGuiCompositor::draw_string(fb, px + 10, py + 80, drift_buf.as_str(), 0xFFD700);
        }

        // Dönüş yarıçapı göstergesi (bisiklet modeli).
        let mut radius_buf = crate::gui::TextBuf::new();
        radius_buf.push_str(crate::gui::ui_text("Dönüş yarıçapı: ", "Turn radius: "));
        if absf(self.steer_cmd) > 0.08 {
            let speed_abs = absf(self.speed);
            let r_min = TURN_R_MIN + TURN_R_SPEED_K * speed_abs * speed_abs;
            radius_buf.push_usize((r_min / absf(self.steer_cmd)) as usize);
            radius_buf.push_str(" m");
        } else {
            radius_buf.push_str(crate::gui::ui_text("düz", "straight"));
        }
        MinGuiCompositor::draw_string(fb, px + 10, py + 92, radius_buf.as_str(), Color::TEXT_MUTED);

        // Skor, mesafe, ödül ve yakıt (sol üst).
        let mut score_buf = crate::gui::TextBuf::new();
        score_buf.push_str(crate::gui::ui_text("Skor: ", "Score: "));
        score_buf.push_usize(self.score as usize);
        score_buf.push_str("  ");
        score_buf.push_str(crate::gui::ui_text("Mesafe: ", "Distance: "));
        score_buf.push_usize((self.distance * 10.0) as usize);
        score_buf.push_str(" m  ");
        score_buf.push_str(crate::gui::ui_text("Ödül: ", "Prizes: "));
        score_buf.push_usize(self.collected as usize);
        score_buf.push_str("  ");
        score_buf.push_str(crate::gui::ui_text("Yakıt: ", "Fuel: "));
        score_buf.push_usize((self.fuel * 10.0) as usize);
        score_buf.push_str(" L");
        MinGuiCompositor::draw_string(
            fb,
            win_x + 10,
            win_y + 8,
            score_buf.as_str(),
            if self.fuel < FUEL_WARN {
                0xFF6B6B
            } else {
                Color::TEXT_MUTED
            },
        );

        // Zaman / tur (sağ üst, mini haritanın altında).
        let mut time_buf = crate::gui::TextBuf::new();
        let hour = (self.time_of_day * 24.0) as u32;
        let minute = ((self.time_of_day * 24.0 * 60.0) % 60.0) as u32;
        time_buf.push_str(crate::gui::ui_text("Saat: ", "Time: "));
        time_buf.push_usize(hour as usize);
        time_buf.push_str(":");
        if minute < 10 {
            time_buf.push_str("0");
        }
        time_buf.push_usize(minute as usize);
        time_buf.push_str("  ");
        time_buf.push_str(crate::gui::ui_text("Tur: ", "Lap: "));
        time_buf.push_usize(self.lap_timer as usize);
        time_buf.push_str("s");
        MinGuiCompositor::draw_string(
            fb,
            win_x + win_w.saturating_sub(MinGuiCompositor::text_width(time_buf.as_str(), 1)) - 10,
            win_y + 8,
            time_buf.as_str(),
            Color::TEXT_MUTED,
        );

        // Mini harita (sağ üst).
        let map_size = 110.min(win_w / 4).max(48);
        let mx = win_x + win_w.saturating_sub(map_size + 10);
        let my = win_y + 10;
        self.render_minimap(fb, mx, my, map_size);

        // Kontrol ipuçları (alt orta).
        let hint = crate::gui::ui_text(
            "W/S gaz-fren  A/D direksiyon  Space el freni (drift)  R sifirla  Esc duraklat",
            "W/S throttle-brake  A/D steer  Space handbrake (drift)  R reset  Esc pause",
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
                    "Ödülleri topla, yakıtını koru, drift yap, tur tamamla!",
                    "Collect prizes, manage fuel, drift, complete laps!",
                );
                let cx = win_x + win_w / 2;
                let cy = win_y + win_h / 2;
                let bw = 340.min(win_w.saturating_sub(20));
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
        let is_night = self.time_of_day > NIGHT_START || self.time_of_day < 0.28;
        let bg = if is_night { 0x05070A } else { 0x10141A };
        let outline = if is_night { 0x1A2438 } else { 0x2E3A4A };
        fb.draw_rect(x, y, size, size, bg);
        fb.draw_rect_outline(x, y, size, size, outline);
        let view_cells = 26.0f32;
        let scale = size as f32 / view_cells;

        // Caddeler.
        for i in 0..(CITY_W / ROAD_PERIOD) {
            let road = (i * ROAD_PERIOD) as f32 + ROAD_AXIS_OFF;
            let vx = x as f32 + (road - self.car_x) * scale;
            if vx >= x as f32 && vx < (x + size) as f32 {
                fb.draw_rect(vx as usize, y, 3, size, 0x39414D);
            }
            let vy = y as f32 + (road - self.car_y) * scale;
            if vy >= y as f32 && vy < (y + size) as f32 {
                fb.draw_rect(x, vy as usize, size, 3, 0x39414D);
            }
        }

        // Ödüller.
        for reward in self.rewards.iter() {
            if !reward.active {
                continue;
            }
            let px = x as f32 + (reward.x - self.car_x) * scale;
            let py = y as f32 + (reward.y - self.car_y) * scale;
            if px >= x as f32 && px < (x + size) as f32 && py >= y as f32 && py < (y + size) as f32
            {
                fb.draw_rect(px as usize, py as usize, 3, 3, reward.kind.tint());
            }
        }

        // Trafik.
        for car in self.traffic.iter() {
            let px = x as f32 + (car.x - self.car_x) * scale;
            let py = y as f32 + (car.y - self.car_y) * scale;
            if px >= x as f32 && px < (x + size) as f32 && py >= y as f32 && py < (x + size) as f32
            {
                fb.draw_rect(px as usize, py as usize, 3, 3, car.color);
            }
        }

        // Kontrol noktaları.
        for (i, cp) in self.checkpoints.iter().enumerate() {
            let px = x as f32 + (cp.0 - self.car_x) * scale;
            let py = y as f32 + (cp.1 - self.car_y) * scale;
            if px >= x as f32 && px < (x + size) as f32 && py >= y as f32 && py < (y + size) as f32
            {
                let color = if i == self.checkpoint {
                    0xFFD700
                } else {
                    0x4A4A4A
                };
                fb.draw_rect(px as usize, py as usize, 4, 4, color);
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
    }
}

impl Default for CityDriveApp {
    fn default() -> Self {
        Self::new()
    }
}

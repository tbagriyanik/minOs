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
    while x > PI {
        x -= TAU;
    }
    while x < -PI {
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
const WALL_HEIGHT_SCALE: f32 = 400.0;
const MOVE_SPEED: f32 = 0.08;
const ROT_SPEED: f32 = 0.06;
const MOUSE_SENS: f32 = 0.0015;

/// Harita: 1 = duvar, 0 = boş. Sınırlar duvarlı.
static MAP: [[u8; MAP_W]; MAP_H] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1],
    [1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1],
    [1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
    [1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1],
    [1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1],
    [1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
    [1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1],
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
    pub shots_fired: u32,
    pub hits: u32,
    pub shooting: bool,
    pub shoot_timer: usize,

    // Düşmanlar
    enemies: [Enemy; MAX_ENEMIES],
    enemy_count: usize,

    // Zamanlayıcılar / FPS
    pub frame_count: usize,
    pub last_fps_tick: usize,
    pub fps: usize,

    // Ses durumu
    pub last_shot_tick: usize,
    pub last_step_tick: usize,
    pub moving: bool,
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

        Self {
            pos_x: 3.5,
            pos_y: 3.5,
            dir_x: 1.0,
            dir_y: 0.0,
            plane_x: 0.0,
            plane_y: 0.66,
            health: 100,
            score: 0,
            shots_fired: 0,
            hits: 0,
            shooting: false,
            shoot_timer: 0,
            enemies,
            enemy_count: n,
            frame_count: 0,
            last_fps_tick: 0,
            fps: 0,
            last_shot_tick: 0,
            last_step_tick: 0,
            moving: false,
            sound_pending: None,
        }
    }

    /// Harita duvarı kontrolü.
    #[inline(always)]
    fn is_wall(mx: usize, my: usize) -> bool {
        mx < MAP_W && my < MAP_H && MAP[my][mx] != 0
    }

    /// Düşmana ateş et, en yakın canlı düşmanı vursa true döner.
    fn shoot(&mut self) -> bool {
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

    /// Düşmanları yeniden canlandır (canlı olmayanları).
    fn respawn_enemies(&mut self) {
        for e in self.enemies.iter_mut().take(self.enemy_count) {
            if !e.alive && e.hit_timer == 0 {
                // Basit yeniden doğurma: rastgele bir boş hücreye.
                let tx = (self.frame_count as usize * 7 + e.x as usize * 13) % (MAP_W - 2) + 1;
                let ty = (self.frame_count as usize * 11 + e.y as usize * 17) % (MAP_H - 2) + 1;
                if MAP[ty][tx] == 0 {
                    e.x = tx as f32 + 0.5;
                    e.y = ty as f32 + 0.5;
                    e.alive = true;
                }
            }
            if e.hit_timer > 0 {
                e.hit_timer -= 1;
            }
        }
    }

    /// Hareketi işle (duvar çarpması kontrolü ile).
    fn move_player(&mut self, dx: f32, dy: f32) {
        let new_x = self.pos_x + dx;
        let new_y = self.pos_y + dy;

        // X ekseni çarpışma
        if !Self::is_wall(new_x as usize, self.pos_y as usize) {
            self.pos_x = new_x;
        }
        // Y ekseni çarpışma
        if !Self::is_wall(self.pos_x as usize, new_y as usize) {
            self.pos_y = new_y;
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
        let render_y = win_y + 30; // Araç çubuğu altı
        let render_h = win_h.saturating_sub(54); // Araç çubuğu + durum çubuğu
        let render_x = win_x;
        let render_w = win_w;

        // Z-buffer: her sütun için mesafe.
        let mut z_buffer: [f32; 1024] = [0.0; 1024];
        let w = render_w.min(1024);

        // Siyah tavan / zemin.
        fb.draw_rect(render_x, render_y, render_w, render_h / 2, 0x101010);
        fb.draw_rect(
            render_x,
            render_y + render_h / 2,
            render_w,
            render_h / 2,
            0x0a0a0a,
        );

        for x in 0..w {
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

            while !hit {
                if side_dist_x < side_dist_y {
                    side_dist_x += delta_dist_x;
                    map_x = (map_x as isize + step_x) as usize;
                    side = 0;
                } else {
                    side_dist_y += delta_dist_y;
                    map_y = (map_y as isize + step_y) as usize;
                    side = 1;
                }
                if map_x >= MAP_W || map_y >= MAP_H || Self::is_wall(map_x, map_y) {
                    hit = true;
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

            z_buffer[x] = perp_wall_dist;

            // Duvar yüksekliği.
            let line_h =
                (render_h as f32 / perp_wall_dist.max(0.1) * WALL_HEIGHT_SCALE * 0.25) as usize;
            let draw_start = (render_h / 2).saturating_sub(line_h / 2);
            let draw_end = (render_h / 2).saturating_add(line_h / 2).min(render_h);

            // Duvar rengi (mesafeye göre gölgeli).
            let base = match MAP[map_y][map_x] {
                1 => 0x404040,
                _ => 0x606060,
            };
            let shade = (1.0 - (perp_wall_dist / MAX_DEPTH).clamp(0.0, 1.0)) * 0.6 + 0.4;
            let r = (((base >> 16) & 0xFF) as f32 * shade) as u32;
            let g = (((base >> 8) & 0xFF) as f32 * shade) as u32;
            let b = ((base & 0xFF) as f32 * shade) as u32;
            let wall_color = (r << 16) | (g << 8) | b;

            // Kenar duvarları daha koyu.
            let wall_color = if side == 1 {
                wall_color / 2
            } else {
                wall_color
            };

            // Dikey çizgi olarak duvarı çiz.
            if draw_start < draw_end && draw_end > 0 && draw_start < render_h {
                let ds = draw_start.max(0).min(render_h);
                let de = draw_end.max(0).min(render_h);
                for yy in ds..de {
                    fb.put_pixel(render_x + x, render_y + yy, wall_color);
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

            // Mesafeye göre boyut.
            let sprite_h = (render_h as f32 / dist.max(0.3) * 150.0) as usize;
            let sprite_w = sprite_h;
            let draw_start_y = (render_h / 2).saturating_sub(sprite_h / 2);
            let draw_end_y = (render_h / 2).saturating_add(sprite_h / 2).min(render_h);

            // Z-buffer testi: duvarın önündeyse çiz.
            if dist < z_buffer[sx] {
                let color = if e.hit_timer > 0 {
                    Color::ACCENT_RED
                } else {
                    Color::ACCENT_PINK
                };
                let cx = render_x + sx;
                let cy0 = render_y + draw_start_y;
                let cy1 = render_y + draw_end_y;
                let sw = sprite_w.min(4);
                for yy in cy0..cy1 {
                    for xx in 0..sw {
                        let px = cx.saturating_sub(sw / 2) + xx;
                        if px < render_x + w {
                            fb.put_pixel(px, yy, color);
                        }
                    }
                }
            }
        }

        // Vurma efekti (kırmızı flaş).
        if self.shooting && self.shoot_timer > 0 {
            let flash_alpha = (self.shoot_timer as f32 / 10.0).clamp(0.0, 1.0);
            let overlay = (0xFF0000 as f32 * flash_alpha * 0.3) as u32;
            for yy in render_y..render_y + render_h {
                for xx in render_x..render_x + render_w {
                    let cur = fb.get_pixel(xx, yy);
                    let r = ((cur >> 16) & 0xFF) as u32;
                    let g = ((cur >> 8) & 0xFF) as u32;
                    let b = (cur & 0xFF) as u32;
                    let nr = (r.saturating_add(overlay)).min(0xFF);
                    let ng = g.saturating_sub(overlay / 2).max(0);
                    let nb = b.saturating_sub(overlay / 2).max(0);
                    fb.put_pixel(xx, yy, (nr << 16) | (ng << 8) | nb);
                }
            }
        }
    }

    /// Radar / küçük haritayı çizer.
    pub fn render_radar(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize) {
        let radar_w = 80usize;
        let radar_h = 60usize;
        let rx = win_x + win_w.saturating_sub(radar_w).saturating_sub(10);
        let ry = win_y + 40;

        fb.draw_rect(rx, ry, radar_w, radar_h, 0x000000);
        fb.draw_rect_outline(rx, ry, radar_w, radar_h, Color::ACCENT_GREEN);

        // Oyuncu noktası.
        let px = rx + (self.pos_x / MAP_W as f32 * radar_w as f32) as usize;
        let py = ry + (self.pos_y / MAP_H as f32 * radar_h as f32) as usize;
        if px > rx && px < rx + radar_w && py > ry && py < ry + radar_h {
            fb.draw_rect(px, py, 2, 2, Color::ACCENT_BLUE);
        }

        // Düşmanlar.
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let ex = rx + (e.x / MAP_W as f32 * radar_w as f32) as usize;
            let ey = ry + (e.y / MAP_H as f32 * radar_h as f32) as usize;
            if ex > rx && ex < rx + radar_w && ey > ry && ey < ry + radar_h {
                fb.draw_rect(ex, ey, 1, 1, Color::ACCENT_RED);
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
        // Araç çubuğu.
        fb.draw_rect(
            win_x + 6,
            win_y + 30,
            win_w - 12,
            crate::gui::sp(24),
            Color::TITLE_BAR,
        );
        MinGuiCompositor::draw_string(
            fb,
            win_x + 12,
            win_y + 36,
            crate::gui::ui_text("FPS Performans Testi", "FPS Performance Test"),
            Color::TEXT_MAIN,
        );

        // 3D sahne.
        self.render_3d(fb, win_x, win_y + 54, win_w, win_h);

        // Radar.
        self.render_radar(fb, win_x, win_y, win_w);

        // HUD.
        self.render_hud(fb, win_x, win_y, win_w, win_h);

        // Merkez işaretçi (crosshair).
        let cx = win_x + win_w / 2;
        let cy = win_y + 54 + (win_h.saturating_sub(54)) / 2;
        fb.draw_rect(cx.saturating_sub(2), cy, 4, 1, Color::WHITE);
        fb.draw_rect(cx, cy.saturating_sub(2), 1, 4, Color::WHITE);

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
    }

    /// Klavye girdisini işle. true dönerse tüketildi.
    pub fn handle_key(&mut self, ch: char, audio_dev: &mut AudioDevice) -> bool {
        match ch {
            'w' | 'W' => {
                self.move_player(self.dir_x * MOVE_SPEED, self.dir_y * MOVE_SPEED);
                self.moving = true;
                true
            }
            's' | 'S' => {
                self.move_player(-self.dir_x * MOVE_SPEED, -self.dir_y * MOVE_SPEED);
                self.moving = true;
                true
            }
            'a' | 'A' => {
                // Sola strafe.
                self.move_player(-self.dir_y * MOVE_SPEED, self.dir_x * MOVE_SPEED);
                self.moving = true;
                true
            }
            'd' | 'D' => {
                // Sağa strafe.
                self.move_player(self.dir_y * MOVE_SPEED, -self.dir_x * MOVE_SPEED);
                self.moving = true;
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
            _ => false,
        }
    }

    /// Ok tuşu ile dönüş.
    pub fn handle_arrow(&mut self, left: bool, right: bool) {
        if left {
            self.rotate(-ROT_SPEED);
        }
        if right {
            self.rotate(ROT_SPEED);
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

        // Düşmanları yenile.
        self.respawn_enemies();
    }
}

/// Ses efekt türleri.
#[derive(Clone, Copy, PartialEq)]
pub enum SoundKind {
    Shoot,
    Hit,
    Step,
}

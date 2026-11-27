//! 3B düşman modeli host testleri.
//!
//! Model çizimi çekirdekten bağımsız bir modülde olduğu için (src/apps/
//! fps_enemy_model.rs) burada doğrudan çalıştırılabilir: model bir düz
//! tampona çizilir ve silüet, derinlik kırpma ve animasyon davranışı
//! doğrulanır.

#[path = "host_fps_enemy_apps.rs"]
pub mod apps;

use alloc::vec;
use alloc::vec::Vec;
use apps::fps_enemy_model::{draw_enemy_model, EnemyModel};
use apps::gfx3d::{Camera3D, NoBudget, PixelSink, MAX_DEPTH};
use core::f32::consts::PI;

struct Sink {
    buf: Vec<u32>,
    w: usize,
    h: usize,
}

impl PixelSink for Sink {
    fn put(&mut self, x: usize, y: usize, color: u32) {
        if x < self.w && y < self.h {
            self.buf[y * self.w + x] = color;
        }
    }
}

fn camera() -> Camera3D {
    Camera3D {
        pos_x: 0.0,
        pos_y: 0.0,
        dir_x: 1.0,
        dir_y: 0.0,
        plane_x: 0.0,
        plane_y: 1.0,
        eye_z: 0.5,
        v_scale: 240.0,
        focal_x: 160.0,
        horizon_y: 120.0,
        view_w: 320.0,
    }
}

fn enemy_at(x: f32, y: f32, facing: f32, walk: f32, kind: u8) -> EnemyModel {
    EnemyModel {
        x,
        y,
        facing,
        walk_phase: walk,
        kind,
    }
}

fn render(w: usize, h: usize, enemy: &EnemyModel, zbuf: &[f32]) -> Vec<u32> {
    let mut sink = Sink {
        buf: vec![0u32; w * h],
        w,
        h,
    };
    draw_enemy_model(&mut sink, &camera(), zbuf, w, h, enemy, &NoBudget);
    sink.buf
}

fn open_zbuf(w: usize) -> Vec<f32> {
    vec![MAX_DEPTH * 4.0; w]
}

/// Çizilen piksel sayısı (arka plan 0).
fn drawn_pixels(buf: &[u32]) -> usize {
    buf.iter().filter(|&&c| c != 0).count()
}

/// Çizilen piksellerin sınır kutusu.
fn bounds(buf: &[u32], w: usize) -> (usize, usize, usize, usize) {
    let (mut min_x, mut min_y) = (usize::MAX, usize::MAX);
    let (mut max_x, mut max_y) = (0usize, 0usize);
    let mut any = false;
    for (i, &c) in buf.iter().enumerate() {
        if c == 0 {
            continue;
        }
        any = true;
        let x = i % w;
        let y = i / w;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if !any {
        return (0, 0, 0, 0);
    }
    (min_x, min_y, max_x, max_y)
}

#[test]
fn model_renders_humanoid_silhouette() {
    let (w, h) = (320usize, 240usize);
    let buf = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 0), &open_zbuf(w));

    let pixels = drawn_pixels(&buf);
    assert!(
        pixels > 150,
        "3B model beklenen sayıda piksel çizmeli, çizildi: {}",
        pixels
    );

    let (min_x, min_y, max_x, max_y) = bounds(&buf, w);
    let height = max_y - min_y + 1;
    let width = max_x - min_x + 1;

    // Model 0.70 birim boyunda; 3 birim mesafede ekran yüksekliği
    // 0.70 * 240 / 3 = 56 piksel civarı olmalı.
    assert!(
        (40..=75).contains(&height),
        "model boyu beklenen aralıkta değil: {} piksel",
        height
    );
    // Genişlik gövde + kollar: yaklaşık 20 piksel.
    assert!(
        (8..=34).contains(&width),
        "model genişliği beklenen aralıkta değil: {} piksel",
        width
    );
    // Model merkezde olmalı (ekran ortası = 160).
    let center = (min_x + max_x) / 2;
    assert!(
        (140..=180).contains(&center),
        "model ekran merkezine hizalı değil: {}",
        center
    );
}

#[test]
fn model_rotation_changes_pixels() {
    let (w, h) = (320usize, 240usize);
    let zbuf = open_zbuf(w);
    let front = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 0), &zbuf);
    let side = render(w, h, &enemy_at(3.0, 0.0, PI * 0.5, 0.0, 0), &zbuf);
    let back = render(w, h, &enemy_at(3.0, 0.0, 0.0, 0.0, 0), &zbuf);

    assert_ne!(front, side, "yöne çevrilen model aynı görünmemeli");
    assert_ne!(front, back, "arkadan görünüm önden görünümle aynı olmamalı");
    // Yandan görünümde model daha dar olmalı (billboard değil, gerçek hacim).
    let (_, _, _, fw) = bounds(&front, w);
    let (_, _, _, sw) = bounds(&side, w);
    assert!(
        sw < fw,
        "yandan görünüm daha dar olmalı: ön {} px, yan {} px",
        fw,
        sw
    );
}

#[test]
fn walk_animation_changes_pose() {
    let (w, h) = (320usize, 240usize);
    let zbuf = open_zbuf(w);
    let a = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 0), &zbuf);
    let b = render(w, h, &enemy_at(3.0, 0.0, PI, 1.6, 0), &zbuf);
    assert_ne!(a, b, "yürüyüş animasyonu modeli değiştirmeli");
}

#[test]
fn enemy_is_occluded_by_wall_in_front() {
    let (w, h) = (320usize, 240usize);
    // Duvvar x=2'de, y in [-0.5, 0.5] aralığında: ekran sütunları ~120..200.
    let mut zbuf = open_zbuf(w);
    for x in 120..200 {
        zbuf[x] = 2.0;
    }

    // Düşman duvarın arkasında (derinlik 3) — hiç görünmemeli.
    let hidden = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 0), &zbuf);
    assert_eq!(
        drawn_pixels(&hidden),
        0,
        "duvar önündeki düşman kırpılmalı, çizildi: {} piksel",
        drawn_pixels(&hidden)
    );

    // Aynı düşman duvarın önünde (derinlik 1) — görünmeli.
    let visible = render(w, h, &enemy_at(1.0, 0.0, PI, 0.0, 0), &zbuf);
    assert!(
        drawn_pixels(&visible) > 100,
        "duvar önündeki düşman görünmeli, çizildi: {} piksel",
        drawn_pixels(&visible)
    );
}

#[test]
fn eyes_are_emissive_only_from_front() {
    let (w, h) = (320usize, 240usize);
    let zbuf = open_zbuf(w);
    let front = render(w, h, &enemy_at(2.0, 0.0, PI, 0.0, 0), &zbuf);
    let back = render(w, h, &enemy_at(2.0, 0.0, 0.0, 0.0, 0), &zbuf);

    // Gözler emissif (kendi ışığında) oldukları için parlak sarı-beyaz
    // tonda kalır; cilt tonu (g ≈ 176) belirgin şekilde daha koyudur.
    let eye = |buf: &[u32]| {
        buf.iter()
            .filter(|&&c| {
                let r = (c >> 16) & 0xFF;
                let g = (c >> 8) & 0xFF;
                let b = c & 0xFF;
                r > 245 && g > 225 && b > 140
            })
            .count()
    };
    assert!(eye(&front) > 0, "önden görünümde parlayan gözler olmalı");
    assert_eq!(eye(&back), 0, "arkadan görünümde gözler görünmemeli");
}

#[test]
fn enemy_kinds_have_different_appearance() {
    let (w, h) = (320usize, 240usize);
    let zbuf = open_zbuf(w);
    let grunt = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 0), &zbuf);
    let brute = render(w, h, &enemy_at(3.0, 0.0, PI, 0.0, 2), &zbuf);
    assert_ne!(grunt, brute, "farklı düşman türleri farklı görünmeli");

    // İri düşman daha büyük olmalı.
    let (_, _, _, gh) = bounds(&grunt, w);
    let (_, _, _, bh) = bounds(&brute, w);
    assert!(bh > gh, "iri düşman daha büyük olmalı: {} vs {}", gh, bh);
}

#[test]
fn enemy_outside_view_is_not_drawn() {
    let (w, h) = (320usize, 240usize);
    let zbuf = open_zbuf(w);
    // Arkada kalan düşman çizilmemeli.
    let behind = render(w, h, &enemy_at(-3.0, 0.0, PI, 0.0, 0), &zbuf);
    assert_eq!(drawn_pixels(&behind), 0, "arkadaki düşman çizilmemeli");

    // Görüş alanı dışındaki düşman çizilmemeli.
    let side = render(w, h, &enemy_at(3.0, 12.0, PI, 0.0, 0), &zbuf);
    assert_eq!(drawn_pixels(&side), 0, "görüş alanı dışı çizilmemeli");
}

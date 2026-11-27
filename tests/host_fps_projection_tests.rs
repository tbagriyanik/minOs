//! FPS 3B sahnesinin izdüşüm tutarlılığı testleri.
//!
//! Zemin (z = 0), tavan (z = WALL_H), duvarlar (0 <= z <= WALL_H) ve düşman
//! modelleri tek bir perspektif modelini paylaşmalıdır:
//!
//!     ekran_y = ufuk + (göz_yüksekliği - dünya_z) * ölçek / derinlik
//!     ekran_x = w/2 + (lateral / derinlik) * odak
//!
//! Işın parametreleştirmesi `plane` (birim dik vektör) ile yapılır:
//!
//!     ışın = dir + plane * ((x + 0.5 - w/2) / odak)
//!
//! Bu testler, düzlem derinliği (`plane_depth`) ile ekran y'si (`screen_y`)
//! arasındaki gidiş-dönüşün tutarlı olduğunu, duvar diplerinin zemindeki
//! ızgara çizgileriyle aynı noktada buluştuğunu, düşman modellerinin
//! duvarlarla aynı sütuna düştüğünü ve dikey bakışın (pitch) tüm yüzeyleri
//! birlikte kaydırdığını doğrular.

#[path = "host_fps_enemy_apps.rs"]
pub mod apps;

use apps::gfx3d::{plane_depth, project_point, screen_y, Camera3D};

/// Göz yüksekliği (dünya birimi) — duvarlar 1 birim yükseklikte.
const EYE_Z: f32 = 0.5;
/// Duvar/tavan yüksekliği (dünya birimi).
const WALL_H: f32 = 1.0;

/// Görüntü alanı: 640x400 (16:10).
const VIEW_W: f32 = 640.0;
const VIEW_H: f32 = 400.0;
/// Odak uzaklığı = ekran yüksekliği → izotropik izdüşüm.
const FOCAL: f32 = VIEW_H;

/// Ufuk satırı ve kamera (odak tabanlı yatay ölçek).
fn camera(horizon_y: f32) -> Camera3D {
    Camera3D {
        pos_x: 0.0,
        pos_y: 0.0,
        dir_x: 1.0,
        dir_y: 0.0,
        plane_x: 0.0,
        plane_y: 1.0,
        eye_z: EYE_Z,
        v_scale: VIEW_H,
        focal_x: FOCAL,
        horizon_y,
        view_w: VIEW_W,
    }
}

#[test]
fn floor_row_round_trips_to_same_depth() {
    let horizon = VIEW_H / 2.0;
    // Ufuk satırının hemen altından ekranın altına kadar birkaç satır.
    for row in (horizon as usize + 1)..(VIEW_H as usize) {
        let depth = plane_depth(horizon, VIEW_H, EYE_Z, 0.0, row as f32 + 0.5);
        assert!(
            depth.is_finite() && depth > 0.0,
            "zemin satırı {} geçerli derinlik üretmeli: {}",
            row,
            depth
        );
        // Aynı satıra geri dönüş: zemin noktası tam o satıra düşmeli.
        let y = screen_y(horizon, VIEW_H, EYE_Z, 0.0, depth);
        assert!(
            (y - (row as f32 + 0.5)).abs() < 0.01,
            "zemin satırı {} geri dönmeliydi, {}",
            row,
            y
        );
    }
}

#[test]
fn wall_base_meets_floor_at_same_depth() {
    let horizon = VIEW_H / 2.0;
    for depth in [0.5f32, 1.0, 2.5, 7.0, 19.0] {
        // Duvarın dibi (z = 0) ve zemindeki aynı derinlikli nokta aynı
        // ekran y'sinde olmalı — aksi halde duvar zeminden kopar.
        let wall_base = screen_y(horizon, VIEW_H, EYE_Z, 0.0, depth);
        let floor_row = plane_depth(horizon, VIEW_H, EYE_Z, 0.0, wall_base);
        assert!(
            (floor_row - depth).abs() < 0.001,
            "duvar dibi derinliği {} idi, zemin {:.3} verdi",
            depth,
            floor_row
        );
    }
}

#[test]
fn wall_span_is_symmetric_around_horizon() {
    let horizon = VIEW_H / 2.0;
    for depth in [0.5f32, 1.0, 3.0, 12.0] {
        let top = screen_y(horizon, VIEW_H, EYE_Z, WALL_H, depth);
        let bottom = screen_y(horizon, VIEW_H, EYE_Z, 0.0, depth);
        let above = horizon - top;
        let below = bottom - horizon;
        assert!(
            (above - below).abs() < 0.001,
            "duvar ufka göre simetrik olmalı: üst {:.3} alt {:.3}",
            above,
            below
        );
    }
}

#[test]
fn ceiling_and_floor_are_symmetric() {
    // Göz, duvar ortasındadır (0.5 = 1.0 - 0.5); bu yüzden tavan ve zemin
    // ufuk satırına eşit uzaklıkta ve aynı derinlikte karşılık gelir.
    let horizon = VIEW_H / 2.0;
    for offset in 1..100usize {
        let floor_depth = plane_depth(horizon, VIEW_H, EYE_Z, 0.0, horizon + offset as f32);
        let ceiling_depth = plane_depth(horizon, VIEW_H, EYE_Z, WALL_H, horizon - offset as f32);
        assert!(
            (floor_depth - ceiling_depth).abs() < 0.001,
            "tavan ve zemin eşit derinlikte olmalı: {} / {}",
            floor_depth,
            ceiling_depth
        );
    }
}

#[test]
fn enemy_model_shares_the_same_projection() {
    // Düşman modelleri `project_point` kullanır; duvarlar ise `screen_y`
    // yardımcısını. İkisi de aynı sonucu vermeli, aksi halde düşman duvarın
    // içine gömülür veya yerden yükselir.
    let horizon = VIEW_H / 2.0;
    let cam = camera(horizon);
    for depth in [1.0f32, 2.0, 4.0, 9.0] {
        // Düşmanın ayakları zeminde (z = 0).
        let foot = project_point(&cam, depth, 0.0, 0.0).expect("ayak noktası projelenmeli");
        let wall_base = screen_y(horizon, VIEW_H, EYE_Z, 0.0, depth);
        assert!(
            (foot.y - wall_base).abs() < 0.001,
            "düşman ayağı ({}) duvar dibiyle ({}) aynı hizada olmalı",
            foot.y,
            wall_base
        );
        // Düşmanın başı (z = 0.7) da aynı ölçekte olmalı.
        let head = project_point(&cam, depth, 0.0, 0.7).expect("baş noktası projelenmeli");
        let expected = screen_y(horizon, VIEW_H, EYE_Z, 0.7, depth);
        assert!(
            (head.y - expected).abs() < 0.001,
            "düşman başı ({}) beklenen y'yi ({}) vermeli",
            head.y,
            expected
        );
    }
}

#[test]
fn pitch_shifts_every_surface_together() {
    // Farklı ufuk (pitch) değerlerinde, aynı derinlikteki duvar dibi ile
    // zemin noktası arasındaki fark sabit kalmalı; yüzeyler birlikte kayar.
    let depth = 3.0f32;
    let mut prev_delta = f32::MAX;
    for horizon in [100.0f32, 150.0, 200.0, 250.0, 300.0] {
        let wall_base = screen_y(horizon, VIEW_H, EYE_Z, 0.0, depth);
        let wall_top = screen_y(horizon, VIEW_H, EYE_Z, WALL_H, depth);
        let floor_depth = plane_depth(horizon, VIEW_H, EYE_Z, 0.0, wall_base);
        assert!(
            (floor_depth - depth).abs() < 0.001,
            "pitch {} altında zemin derinliği bozuldu: {}",
            horizon,
            floor_depth
        );
        // Duvar hâlâ ufka göre simetrik olmalı.
        assert!((wall_top - (2.0 * horizon - wall_base)).abs() < 0.001);
        let delta = wall_base - horizon;
        assert!(
            (delta - prev_delta).abs() < 0.001 || prev_delta == f32::MAX,
            "ufuk kaydıkça duvar-zemin farkı değişmemeli"
        );
        prev_delta = delta;
    }
}

#[test]
fn column_ray_lands_on_its_own_screen_column() {
    // Sütun -> ışın parametresi dönüşümü, zeminde ve duvarda aynı ışını
    // seçmeli: `dir + plane * ((x + 0.5 - w/2) / focal)` ışını, dünya
    // noktası `project_point` ile aynı sütuna düşmeli.
    let horizon = VIEW_H / 2.0;
    let cam = camera(horizon);
    for x in [0usize, 1, 10, 100, 319, 639] {
        let camera_x = (x as f32 + 0.5 - VIEW_W * 0.5) / FOCAL;
        let ray_x = cam.dir_x + cam.plane_x * camera_x;
        let ray_y = cam.dir_y + cam.plane_y * camera_x;
        let depth = 4.0f32;
        let point = project_point(&cam, ray_x * depth, ray_y * depth, 0.0)
            .expect("zemin noktası projelenmeli");
        assert!(
            (point.x - (x as f32 + 0.5)).abs() < 0.01,
            "sütun {} ışını ekran x'ine denk gelmeli: {}",
            x,
            point.x
        );
    }
}

#[test]
fn world_cells_are_square() {
    // İzotropik izdüşüm: 1 dünya birimi genişlik ve 1 dünya birimi yükseklik
    // aynı derinlikte ekranda eşit uzunlukta olmalı.
    let horizon = VIEW_H / 2.0;
    let cam = camera(horizon);
    let depth = 5.0f32;
    // Yanal 1 birim (dünya y ekseni) ve dikey 1 birim (z ekseni).
    let side_a = project_point(&cam, depth, 0.0, 0.0).expect("nokta projelenmeli");
    let side_b = project_point(&cam, depth, 1.0, 0.0).expect("nokta projelenmeli");
    let width = (side_b.x - side_a.x).abs();
    let top = project_point(&cam, depth, 0.0, 1.0).expect("nokta projelenmeli");
    let height = (top.y - side_a.y).abs();
    assert!(
        (width - height).abs() < 0.01,
        "hücre kare olmalı: genişlik {:.3} yükseklik {:.3}",
        width,
        height
    );
}

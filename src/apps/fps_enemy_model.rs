//! FPS oyunu için 3B düşman modeli.
//!
//! Düşmanlar 2B billboard sprite olarak değil, kutulardan (kafa, gövde, iki
//! kol, iki bacak, iki göz) oluşan düşük poli bir **3B humanoid** olarak
//! modellenir. Model düşmanın baktığı yöne göre döndürülür, perspektif
//! izdüşümüyle ekrana yansıtılır, düz gölgelendirme ile aydınlatılır ve
//! duvar derinlik tamponuna (z-buffer) göre kırpılır. Böylece düşman duvar
//! arkasına doğru şekilde gizlenir ve dönüş açısına göre hacim kazanır.
//!
//! Çizim altyapısı ortak `gfx3d` modülündedir; bu modül yalnızca modelin
//! tanımını ve kutu listesini üretir.

use super::gfx3d::{
    blend_toward, fast_cos, fast_sin, fast_sqrt, Box3D, Camera3D, Face3D, PixelSink, ProjVertex,
    RenderBudget, Shading, MAX_DEPTH,
};

// ---------------------------------------------------------------------------
// Model tanımları
// ---------------------------------------------------------------------------

/// Model parçasının animasyon rolü.
pub const ROLE_TORSO: u8 = 0;
pub const ROLE_HEAD: u8 = 1;
pub const ROLE_ARM_L: u8 = 2;
pub const ROLE_ARM_R: u8 = 3;
pub const ROLE_LEG_L: u8 = 4;
pub const ROLE_LEG_R: u8 = 5;
pub const ROLE_EYE: u8 = 6;

/// Modeldeki bir kutunun yerel tanımı.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct ModelPart {
    /// Yerel merkez: x = sağ/sol, y = ileri/geri, z = yukarı.
    pub cx: f32,
    pub cy: f32,
    pub cz: f32,
    /// Yarı boyutlar.
    pub hx: f32,
    pub hy: f32,
    pub hz: f32,
    /// Animasyon rolü (renk paletten türetilir).
    pub role: u8,
    /// Uzuv pivotu (omuz / kalça); sallanma bu yükseklikteki eksen etrafında olur.
    pub pivot_z: f32,
}

/// Düşman türleri: (gövde rengi, kafa rengi, uzuv rengi, ölçek).
#[allow(dead_code)]
pub const ENEMY_KINDS: [(u32, u32, u32, f32); 3] = [
    (0xB03A6A, 0xF2B09A, 0x501C30, 1.00), // 0: sıradan düşman
    (0x2E7D32, 0xE8C79A, 0x1B5E20, 0.92), // 1: hızlı düşman
    (0x6B3FA0, 0xC9A27A, 0x3A2450, 1.22), // 2: iri düşman
];

/// Humanoid model (yaklaşık 0.70 birim boy, 1.0 birimlik duvarın altında).
#[allow(dead_code)]
pub static ENEMY_MODEL: [ModelPart; 8] = [
    // Gövde
    ModelPart {
        cx: 0.0,
        cy: 0.0,
        cz: 0.40,
        hx: 0.115,
        hy: 0.075,
        hz: 0.13,
        role: ROLE_TORSO,
        pivot_z: 0.0,
    },
    // Kafa
    ModelPart {
        cx: 0.0,
        cy: 0.0,
        cz: 0.60,
        hx: 0.085,
        hy: 0.085,
        hz: 0.09,
        role: ROLE_HEAD,
        pivot_z: 0.0,
    },
    // Sol kol
    ModelPart {
        cx: -0.16,
        cy: 0.0,
        cz: 0.40,
        hx: 0.045,
        hy: 0.055,
        hz: 0.115,
        role: ROLE_ARM_L,
        pivot_z: 0.515,
    },
    // Sağ kol
    ModelPart {
        cx: 0.16,
        cy: 0.0,
        cz: 0.40,
        hx: 0.045,
        hy: 0.055,
        hz: 0.115,
        role: ROLE_ARM_R,
        pivot_z: 0.515,
    },
    // Sol bacak
    ModelPart {
        cx: -0.055,
        cy: 0.0,
        cz: 0.14,
        hx: 0.05,
        hy: 0.055,
        hz: 0.13,
        role: ROLE_LEG_L,
        pivot_z: 0.27,
    },
    // Sağ bacak
    ModelPart {
        cx: 0.055,
        cy: 0.0,
        cz: 0.14,
        hx: 0.05,
        hy: 0.055,
        hz: 0.13,
        role: ROLE_LEG_R,
        pivot_z: 0.27,
    },
    // Sol göz
    ModelPart {
        cx: -0.035,
        cy: 0.093,
        cz: 0.615,
        hx: 0.022,
        hy: 0.014,
        hz: 0.018,
        role: ROLE_EYE,
        pivot_z: 0.0,
    },
    // Sağ göz
    ModelPart {
        cx: 0.035,
        cy: 0.093,
        cz: 0.615,
        hx: 0.022,
        hy: 0.014,
        hz: 0.018,
        role: ROLE_EYE,
        pivot_z: 0.0,
    },
];

/// Azami yüzey sayısı (8 parça × 6 yüzey).
#[allow(dead_code)]
const MAX_MODEL_FACES: usize = 64;

/// Düşman modeli için gölgelendirme parametreleri.
#[allow(dead_code)]
pub const ENEMY_SHADING: Shading = Shading::new((0.35, -0.30, 0.88), 0.30, 0.58, 0.22, 0.45, 20.0);

// ---------------------------------------------------------------------------
// Çizim arayüzü
// ---------------------------------------------------------------------------

/// Çizilecek düşman modeli.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct EnemyModel {
    /// Dünya konumu.
    pub x: f32,
    pub y: f32,
    /// Modelin baktığı yön (radyan).
    pub facing: f32,
    /// Yürüyüş animasyonu fazı.
    pub walk_phase: f32,
    /// Düşman türü (palet + ölçek).
    pub kind: u8,
}

/// Bir düşmanın 3B modelini çizer.
///
/// Model kutuları üretilir, `gfx3d` ile perspektif izdüşüme taşınır, düz
/// gölgelendirme ile aydınlatılır ve derinlik tamponuna göre kırpılır.
/// Bütçe aşılırsa `false` döner.
#[allow(dead_code)]
pub fn draw_enemy_model(
    sink: &mut dyn PixelSink,
    cam: &Camera3D,
    zbuf: &[f32],
    view_w: usize,
    view_h: usize,
    enemy: &EnemyModel,
    budget: &dyn RenderBudget,
) -> bool {
    if view_w < 8 || view_h < 8 || zbuf.len() < view_w {
        return true;
    }

    // Düşman merkezinin kamera derinliği ve yanal ofseti.
    let rel_x = enemy.x - cam.pos_x;
    let rel_y = enemy.y - cam.pos_y;
    let center_depth = rel_x * cam.dir_x + rel_y * cam.dir_y;
    if center_depth < 0.35 || center_depth > MAX_DEPTH {
        return true;
    }
    let lateral = rel_x * cam.plane_x + rel_y * cam.plane_y;
    // Duvar projeksiyonuyla aynı "tan" ölçüsü: görüş alanı dışını ele.
    if (lateral / center_depth).abs() > 1.15 {
        return true;
    }

    let (body_col, head_col, leg_col, scale) =
        ENEMY_KINDS[(enemy.kind as usize) % ENEMY_KINDS.len()];

    // Düşmanın baktığı yön ve ona dik yanal vektör.
    let yaw = enemy.facing;
    let fwd_x = fast_cos(yaw);
    let fwd_y = fast_sin(yaw);
    let right = (fwd_y, -fwd_x, 0.0);
    let fwd = (fwd_x, fwd_y, 0.0);
    let up = (0.0, 0.0, 1.0);

    // Yürüyüş animasyonu.
    let swing = fast_sin(enemy.walk_phase);

    let mut faces = [Face3D {
        depth: 0.0,
        color: 0,
        v: [
            ProjVertex {
                x: 0.0,
                y: 0.0,
                depth: 0.0,
            },
            ProjVertex {
                x: 0.0,
                y: 0.0,
                depth: 0.0,
            },
            ProjVertex {
                x: 0.0,
                y: 0.0,
                depth: 0.0,
            },
            ProjVertex {
                x: 0.0,
                y: 0.0,
                depth: 0.0,
            },
        ],
    }; MAX_MODEL_FACES];
    let mut face_count = 0usize;

    for part in ENEMY_MODEL.iter() {
        // Uzuv sallanması: kollar ve bacaklar omuz/kalça ekseninde döner.
        let pivot_z = part.pivot_z * scale;
        let (swing_angle, swingable) = match part.role {
            ROLE_ARM_L => (swing * 0.55, true),
            ROLE_ARM_R => (-swing * 0.55, true),
            ROLE_LEG_L => (-swing * 0.55, true),
            ROLE_LEG_R => (swing * 0.55, true),
            _ => (0.0, false),
        };
        let (sin_a, cos_a) = if swingable {
            (fast_sin(swing_angle), fast_cos(swing_angle))
        } else {
            (0.0, 1.0)
        };

        // Parçanın taban rengi.
        let base_color = match part.role {
            ROLE_HEAD => head_col,
            ROLE_LEG_L | ROLE_LEG_R => leg_col,
            ROLE_EYE => 0xFFE79A,
            ROLE_ARM_L | ROLE_ARM_R => blend_toward(body_col, 0x000000, 0.38),
            _ => body_col,
        };
        let emissive = part.role == ROLE_EYE;

        // Kutu: yerel eksenler sallanmayla döndürülür.
        let v = (
            fwd.0 * cos_a + up.0 * sin_a,
            fwd.1 * cos_a + up.1 * sin_a,
            fwd.2 * cos_a + up.2 * sin_a,
        );
        let w = (
            -fwd.0 * sin_a + up.0 * cos_a,
            -fwd.1 * sin_a + up.1 * cos_a,
            -fwd.2 * sin_a + up.2 * cos_a,
        );

        // Merkez de pivot etrafında döner.
        let lx = part.cx * scale;
        let dy = part.cy * scale;
        let dz = part.cz * scale - pivot_z;
        let ly = dy * cos_a - dz * sin_a;
        let lz = pivot_z + dy * sin_a + dz * cos_a;

        let center = (
            enemy.x + right.0 * lx + v.0 * ly + w.0 * lz,
            enemy.y + right.1 * lx + v.1 * ly + w.1 * lz,
            right.2 * lx + v.2 * ly + w.2 * lz,
        );

        let b = Box3D {
            center,
            axes: [right, v, w],
            half: (part.hx * scale, part.hy * scale, part.hz * scale),
            colors: [base_color; 6],
            // Alt yüzeyler hiç görünmez (kamera göz seviyesinin üstünde).
            skip: 0b000010,
            emissive,
        };
        super::gfx3d::push_box(&mut faces, &mut face_count, cam, &b, &ENEMY_SHADING);
    }

    if face_count == 0 {
        return true;
    }

    // Taban gölgesi: modelin ayaklarının altındaki zemin elipsi.
    // Elips, düşmanın baktığı yöndeki dünya noktalarının izdüşümüyle
    // hesaplanır (yakın kenar ekranda daha aşağıda görünür).
    let shadow_half = 0.20 * scale;
    let near_p = super::gfx3d::project_point(
        cam,
        enemy.x + fwd_x * shadow_half,
        enemy.y + fwd_y * shadow_half,
        0.0,
    );
    let far_p = super::gfx3d::project_point(
        cam,
        enemy.x - fwd_x * shadow_half,
        enemy.y - fwd_y * shadow_half,
        0.0,
    );
    let side_p = super::gfx3d::project_point(
        cam,
        enemy.x + right.0 * shadow_half,
        enemy.y + right.1 * shadow_half,
        0.0,
    );
    let center_p = super::gfx3d::project_point(cam, enemy.x, enemy.y, 0.0);
    if let (Some(near_p), Some(far_p), Some(side_p), Some(center_p)) =
        (near_p, far_p, side_p, center_p)
    {
        let y_top = far_p.y.min(near_p.y);
        let y_bottom = far_p.y.max(near_p.y);
        let half_w = (side_p.x - center_p.x).abs();
        let half_h = ((y_bottom - y_top) * 0.5).max(0.5);
        let mid_y = (y_top + y_bottom) * 0.5;
        if half_w >= 1.0 {
            for py in (y_top as i64).max(0) as usize..=(y_bottom as i64).max(0) as usize {
                if py >= view_h {
                    break;
                }
                let norm = ((py as f32 + 0.5 - mid_y) / half_h).abs();
                if norm > 1.0 {
                    continue;
                }
                let row_half = (half_w * fast_sqrt((1.0 - norm * norm).max(0.0))) as usize;
                let cx = center_p.x as usize;
                for px_off in 0..row_half * 2 {
                    let px = cx.saturating_sub(row_half) + px_off;
                    if px >= view_w {
                        break;
                    }
                    if center_depth < zbuf[px] {
                        sink.put(px, py, 0x0A0710);
                    }
                }
            }
        }
    }

    super::gfx3d::draw_faces(sink, view_w, view_h, zbuf, &mut faces, face_count, budget)
}

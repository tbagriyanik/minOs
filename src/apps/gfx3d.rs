//! Ortak 3B ağ (mesh) rasterizasyonu.
//!
//! Hem FPS oyunundaki düşman modelleri hem de şehir sürüşü oyunundaki trafik
//! araçları bu modülü kullanır. Dünya uzayındaki kutular (döndürülmüş eksen
//! hizalı kutular) perspektif izdüşümle ekrana taşınır, düz gölgelendirme
//! ile aydınlatılır, yüzeyler uzaktan yakına sıralanır (painter's algorithm)
//! ve sütun başına tutulan derinlik tamponuna göre kırpılır.
//!
//! Modül çekirdek bağımlılığından arındırılmıştır: piksel çıktısı
//! [`PixelSink`], zaman kontrolü ise [`RenderBudget`] trait'i ile sağlanır.
//! Böylece aynı çizim kodu çekirdekte ve host testlerinde çalışabilir.

use core::f32::consts::PI;

// ---------------------------------------------------------------------------
// no_std matematik yardımcıları
// ---------------------------------------------------------------------------

/// Taylor serisi ile sinüs.
#[inline(always)]
pub fn fast_sin(mut x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    // Açı normalizasyonu mutlaka sonlu kalsın. Bozuk/taşmış bir değer için
    // `x -= TAU` sonsuz döngüye girebilirdi (özellikle uzun süreli dönüşte).
    for _ in 0..8 {
        if x <= PI {
            break;
        }
        x -= core::f32::consts::TAU;
    }
    for _ in 0..8 {
        if x >= -PI {
            break;
        }
        x += core::f32::consts::TAU;
    }
    let x2 = x * x;
    let x3 = x * x2;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
}

/// Kosinüs.
#[inline(always)]
pub fn fast_cos(x: f32) -> f32 {
    fast_sin(x + PI / 2.0)
}

/// Newton-Raphson karekök.
#[inline(always)]
pub fn fast_sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
    for _ in 0..6 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// atan2 yaklaşımı.
#[allow(dead_code)]
#[inline(always)]
pub fn fast_atan2(y: f32, x: f32) -> f32 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    if x == 0.0 {
        if y > 0.0 {
            return PI / 2.0;
        }
        if y < 0.0 {
            return -PI / 2.0;
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

/// atan yaklaşımı (polinom).
#[allow(dead_code)]
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

/// no_std ortamında `f32::floor` yoktur; matematiği kendimiz yaparız.
#[inline(always)]
pub fn fast_floor(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let t = x as i32 as f32;
    if x < 0.0 && t != x {
        t - 1.0
    } else {
        t
    }
}

/// no_std ortamında `f32::ceil` yoktur; matematiği kendimiz yaparız.
#[inline(always)]
pub fn fast_ceil(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let t = x as i32 as f32;
    if x > 0.0 && t != x {
        t + 1.0
    } else {
        t
    }
}

// ---------------------------------------------------------------------------
// Arayüzler
// ---------------------------------------------------------------------------

/// Görüş/derinlik sabiti (duvar sis mesafesi).
pub const MAX_DEPTH: f32 = 20.0;

/// Piksel hedefi. Çekirdekte framebuffer, host testlerinde düz bir tampon
/// bu trait'i uygular.
pub trait PixelSink {
    /// Görüntü alanı koordinatında (0..view_w, 0..view_h) piksel yazar.
    fn put(&mut self, x: usize, y: usize, color: u32);
}

/// Zaman bütçesi denetimi.
pub trait RenderBudget {
    /// Bütçe aşıldıysa `true`.
    fn expired(&self) -> bool;
}

/// Bütçe aşımını hiç takip etmeyen yardımcı (testler için).
pub struct NoBudget;

impl RenderBudget for NoBudget {
    fn expired(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Kamera ve geometri
// ---------------------------------------------------------------------------

/// Kamera durumu. İki oyun da aynı izdüşümü paylaşır: yanal konum `tan`
/// cinsinden, dikey konum ise göz yüksekliğine göre ölçeklenir.
#[derive(Clone, Copy)]
pub struct Camera3D {
    pub pos_x: f32,
    pub pos_y: f32,
    /// Bakış yönü (birim vektör).
    pub dir_x: f32,
    pub dir_y: f32,
    /// Bakış düzlemi (dir'e dik; uzunluğu görüş açısını belirler).
    pub plane_x: f32,
    pub plane_y: f32,
    /// Göz yüksekliği (dünya birimi).
    pub eye_z: f32,
    /// Dikey ölçek: bir birim yükseklik, derinlik başına bu kadar piksel.
    pub v_scale: f32,
    /// Ufuk satırı (ekran y, mutlak).
    pub horizon_y: f32,
    /// Görüntü alanı genişliği (ekran x ölçeklendirmesi).
    pub view_w: f32,
}

/// Ekrana izdüşüm edilmiş köşe.
#[derive(Clone, Copy)]
pub struct ProjVertex {
    /// Ekran x koordinatı.
    pub x: f32,
    /// Ekran y koordinatı.
    pub y: f32,
    /// Kamera derinliği (z-buffer değerleriyle aynı birimde).
    pub depth: f32,
}

/// Ekrana hazır yüzey (dört köşe + düz gölge rengi).
#[derive(Clone, Copy)]
pub struct Face3D {
    /// Merkez derinliği — uzaktan yakına sıralama için.
    pub depth: f32,
    pub color: u32,
    pub v: [ProjVertex; 4],
}

/// Dünya uzayında eş konumlu, yerel eksende hizalı bir kutu.
///
/// Yerel eksenler `(sağ, ileri, yukarı)` sırasıyla `axes` içinde tutulur ve
/// dünya uzayında birim vektörlerdir; böylece araçlar araç ekseninde,
/// insan gövdesi uzuvsal açıyla döndürülebilir.
#[derive(Clone, Copy)]
pub struct Box3D {
    /// Merkez (dünya uzayı).
    pub center: (f32, f32, f32),
    /// Yerel eksenler: (sağ, ileri, yukarı).
    pub axes: [(f32, f32, f32); 3],
    /// Yarı boyutlar (yerel eksenlerde).
    pub half: (f32, f32, f32),
    /// Yüz renkleri: `[+z, -z, +x, -x, +y, -y]`.
    pub colors: [u32; 6],
    /// Atlanacak yüzler (bit maskesi): bit0=+z … bit5=-y.
    pub skip: u8,
    /// Emissif yüzeyler (kendi ışığında, gölgelendirilmez).
    pub emissive: bool,
}

/// Gölgelendirme parametreleri.
#[derive(Clone, Copy)]
pub struct Shading {
    /// Dünya ışık yönü.
    pub light: (f32, f32, f32),
    /// Ortam ışığı çarpanı.
    pub ambient: f32,
    /// Difüz ışık çarpanı.
    pub diffuse: f32,
    /// Kamera dolgu ışığı çarpanı.
    pub fill: f32,
    /// Uzaktaki yüzeylere uygulanan minimum çarpan (sis).
    pub fog_floor: f32,
    /// Sis mesafesi (dünya birimi).
    pub fog_distance: f32,
}

impl Shading {
    /// Yeni gölgelendirme tanımı.
    pub const fn new(
        light: (f32, f32, f32),
        ambient: f32,
        diffuse: f32,
        fill: f32,
        fog_floor: f32,
        fog_distance: f32,
    ) -> Self {
        Self {
            light,
            ambient,
            diffuse,
            fill,
            fog_floor,
            fog_distance,
        }
    }
}

/// Bir kutunun altı yüzü (köşe indeksleri, dışa bakan sarım sırasıyla).
/// Köşe indeksi bit düzeni: bit0 = x (sağ), bit1 = y (ileri), bit2 = z (yukarı).
pub const BOX_FACES: [[usize; 4]; 6] = [
    [4, 5, 7, 6], // +z (üst)
    [0, 2, 3, 1], // -z (alt)
    [1, 3, 7, 5], // +x (sağ)
    [0, 4, 6, 2], // -x (sol)
    [2, 6, 7, 3], // +y (ön)
    [0, 1, 5, 4], // -y (arka)
];

/// İki rengi belirli oranda karıştırır.
#[inline(always)]
pub fn blend_toward(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ar = ((a >> 16) & 0xFF) as f32;
    let ag = ((a >> 8) & 0xFF) as f32;
    let ab = (a & 0xFF) as f32;
    let br = ((b >> 16) & 0xFF) as f32;
    let bg = ((b >> 8) & 0xFF) as f32;
    let bb = (b & 0xFF) as f32;
    let r = (ar + (br - ar) * t) as u32;
    let g = (ag + (bg - ag) * t) as u32;
    let bl = (ab + (bb - ab) * t) as u32;
    (r << 16) | (g << 8) | bl
}

// ---------------------------------------------------------------------------
// Çizim
// ---------------------------------------------------------------------------

/// Dünya koordinatındaki bir noktayı ekrana perspektif izdüşümüyle taşır.
#[inline(always)]
pub fn project_point(cam: &Camera3D, wx: f32, wy: f32, wz: f32) -> Option<ProjVertex> {
    let rel_x = wx - cam.pos_x;
    let rel_y = wy - cam.pos_y;
    let depth = rel_x * cam.dir_x + rel_y * cam.dir_y;
    if depth < 0.08 {
        // Yakın düzlem arkasında — kırpma yapılmadan yüzey atlanır.
        return None;
    }
    let lateral = rel_x * cam.plane_x + rel_y * cam.plane_y;
    let cam_x = lateral / depth;
    let x = (cam_x + 1.0) * 0.5 * cam.view_w;
    let y = cam.horizon_y + (cam.eye_z - wz) * cam.v_scale / depth;
    Some(ProjVertex { x, y, depth })
}

/// Üçgeni derinlik tamponuna göre doldurur. Derinlik, üçgenin köşeleri
/// arasında barycentric olarak doğrusal interpolasyonla hesaplanır.
/// Bütçe aşılırsa `false` döner.
pub fn raster_triangle(
    sink: &mut dyn PixelSink,
    view_w: usize,
    view_h: usize,
    zbuf: &[f32],
    a: ProjVertex,
    b: ProjVertex,
    c: ProjVertex,
    color: u32,
    budget: &dyn RenderBudget,
) -> bool {
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.abs() < 0.0001 {
        return true;
    }
    let min_x = fast_floor(a.x.min(b.x).min(c.x)).max(0.0) as usize;
    let max_x = (fast_ceil(a.x.max(b.x).max(c.x)) as i64).clamp(0, view_w as i64 - 1) as usize;
    let min_y = fast_floor(a.y.min(b.y).min(c.y)).max(0.0) as usize;
    let max_y = (fast_ceil(a.y.max(b.y).max(c.y)) as i64).clamp(0, view_h as i64 - 1) as usize;
    if min_x > max_x || min_y > max_y {
        return true;
    }
    let inv_area = 1.0 / area;
    for py in min_y..=max_y {
        if (py & 15) == 0 && budget.expired() {
            return false;
        }
        let sy = py as f32 + 0.5;
        for (px, &z) in zbuf.iter().enumerate().take(max_x + 1).skip(min_x) {
            let sx = px as f32 + 0.5;
            // Barycentric ağırlıklar (kenar fonksiyonları).
            let w0 = ((c.x - b.x) * (sy - b.y) - (c.y - b.y) * (sx - b.x)) * inv_area;
            if w0 < 0.0 {
                continue;
            }
            let w1 = ((a.x - c.x) * (sy - c.y) - (a.y - c.y) * (sx - c.x)) * inv_area;
            if w1 < 0.0 {
                continue;
            }
            let w2 = ((b.x - a.x) * (sy - a.y) - (b.y - a.y) * (sx - a.x)) * inv_area;
            if w2 < 0.0 {
                continue;
            }
            let depth = w0 * a.depth + w1 * b.depth + w2 * c.depth;
            if depth >= z {
                // Öndeki bir yüzey var — bu piksel kırpılır.
                continue;
            }
            sink.put(px, py, color);
        }
    }
    true
}

/// Bir kutunun görünür yüzlerini projeksiyonla hazırlar ve yüzey listesine
/// ekler. `skip` maskesiyle kapatılan yüzler çizilmez.
pub fn push_box(
    faces: &mut [Face3D],
    count: &mut usize,
    cam: &Camera3D,
    b: &Box3D,
    shading: &Shading,
) {
    if *count + 6 > faces.len() {
        return;
    }
    // Işık yönünü normalize et.
    let l_len = fast_sqrt(
        shading.light.0 * shading.light.0
            + shading.light.1 * shading.light.1
            + shading.light.2 * shading.light.2,
    );
    let lx = shading.light.0 / l_len;
    let ly = shading.light.1 / l_len;
    let lz = shading.light.2 / l_len;

    // Kutu köşeleri (dünya uzayı).
    let mut corners = [(0.0f32, 0.0f32, 0.0f32); 8];
    for (i, corner) in corners.iter_mut().enumerate() {
        let sx = if i & 1 == 0 { -b.half.0 } else { b.half.0 };
        let sy = if i & 2 == 0 { -b.half.1 } else { b.half.1 };
        let sz = if i & 4 == 0 { -b.half.2 } else { b.half.2 };
        let u = b.axes[0];
        let v = b.axes[1];
        let w = b.axes[2];
        *corner = (
            b.center.0 + u.0 * sx + v.0 * sy + w.0 * sz,
            b.center.1 + u.1 * sx + v.1 * sy + w.1 * sz,
            b.center.2 + u.2 * sx + v.2 * sy + w.2 * sz,
        );
    }

    for (face_idx, idx) in BOX_FACES.iter().enumerate() {
        if b.skip & (1 << face_idx) != 0 {
            continue;
        }
        let v0 = corners[idx[0]];
        let v1 = corners[idx[1]];
        let v2 = corners[idx[2]];
        let v3 = corners[idx[3]];

        // Yüzey normali (ilk üç köşeden).
        let e1 = (v1.0 - v0.0, v1.1 - v0.1, v1.2 - v0.2);
        let e2 = (v2.0 - v0.0, v2.1 - v0.1, v2.2 - v0.2);
        let mut n = (
            e1.1 * e2.2 - e1.2 * e2.1,
            e1.2 * e2.0 - e1.0 * e2.2,
            e1.0 * e2.1 - e1.1 * e2.0,
        );
        let n_len = fast_sqrt(n.0 * n.0 + n.1 * n.1 + n.2 * n.2);
        if n_len < 0.000001 {
            continue;
        }
        n.0 /= n_len;
        n.1 /= n_len;
        n.2 /= n_len;

        // Yüzey merkezi.
        let fc = (
            (v0.0 + v1.0 + v2.0 + v3.0) / 4.0,
            (v0.1 + v1.1 + v2.1 + v3.1) / 4.0,
            (v0.2 + v1.2 + v2.2 + v3.2) / 4.0,
        );

        // Normal kutu dışına baksın.
        let outward =
            n.0 * (fc.0 - b.center.0) + n.1 * (fc.1 - b.center.1) + n.2 * (fc.2 - b.center.2);
        if outward < 0.0 {
            n.0 = -n.0;
            n.1 = -n.1;
            n.2 = -n.2;
        }

        // Arka yüzeyleri ele (kameraya bakmayan yüzeyler çizilmez).
        let to_cam = (cam.pos_x - fc.0, cam.pos_y - fc.1, cam.eye_z - fc.2);
        if n.0 * to_cam.0 + n.1 * to_cam.1 + n.2 * to_cam.2 <= 0.0 {
            continue;
        }

        // Düz gölgelendirme: dünya ışığı + kamera dolgusu.
        let base = b.colors[face_idx];
        let color = if b.emissive {
            base
        } else {
            let mut diffuse = n.0 * lx + n.1 * ly + n.2 * lz;
            if diffuse < 0.0 {
                diffuse = 0.0;
            }
            let mut fill = n.0 * to_cam.0 + n.1 * to_cam.1 + n.2 * to_cam.2;
            if fill < 0.0 {
                fill = 0.0;
            }
            let intensity = shading.ambient + shading.diffuse * diffuse + shading.fill * fill;
            // Mesafeye bağlı karartma (sis).
            let ddx = fc.0 - cam.pos_x;
            let ddy = fc.1 - cam.pos_y;
            let face_dist = fast_sqrt(ddx * ddx + ddy * ddy);
            let fog = if shading.fog_distance > 0.0 {
                let t = (face_dist / shading.fog_distance).clamp(0.0, 1.0);
                shading.fog_floor + (1.0 - shading.fog_floor) * (1.0 - t)
            } else {
                1.0
            };
            let shade = intensity * fog;
            let r = (((base >> 16) & 0xFF) as f32 * shade).min(255.0) as u32;
            let g = (((base >> 8) & 0xFF) as f32 * shade).min(255.0) as u32;
            let bl = ((base & 0xFF) as f32 * shade).min(255.0) as u32;
            (r << 16) | (g << 8) | bl
        };

        // Köşeleri ekrana izdüşümle.
        let mut pv = [ProjVertex {
            x: 0.0,
            y: 0.0,
            depth: 0.0,
        }; 4];
        let mut ok = true;
        for k in 0..4 {
            let c = corners[idx[k]];
            match project_point(cam, c.0, c.1, c.2) {
                Some(p) => pv[k] = p,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }

        // Çok küçük yüzeyleri atla (perfans).
        let area_px = ((pv[1].x - pv[0].x) * (pv[2].y - pv[0].y)
            - (pv[1].y - pv[0].y) * (pv[2].x - pv[0].x))
            .abs()
            * 0.5;
        if area_px < 1.0 {
            continue;
        }

        faces[*count] = Face3D {
            depth: (pv[0].depth + pv[1].depth + pv[2].depth + pv[3].depth) / 4.0,
            color,
            v: pv,
        };
        *count += 1;
    }
}

/// Yüzey listesini uzaktan yakına sıralayıp ekrana basar.
///
/// `zbuf` sütun başına dünya derinliği tutar (duvar/bina çiziminden gelir).
/// Bütçe aşılırsa `false` döner.
pub fn draw_faces(
    sink: &mut dyn PixelSink,
    view_w: usize,
    view_h: usize,
    zbuf: &[f32],
    faces: &mut [Face3D],
    count: usize,
    budget: &dyn RenderBudget,
) -> bool {
    let n = count.min(faces.len());
    // Yüzeyleri uzaktan yakına sırala (painter's algorithm).
    for i in 1..n {
        let key = faces[i];
        let mut j = i;
        while j > 0 && faces[j - 1].depth < key.depth {
            faces[j] = faces[j - 1];
            j -= 1;
        }
        faces[j] = key;
    }

    for f in faces.iter().take(n) {
        if budget.expired() {
            return false;
        }
        if !raster_triangle(
            sink, view_w, view_h, zbuf, f.v[0], f.v[1], f.v[2], f.color, budget,
        ) {
            return false;
        }
        if !raster_triangle(
            sink, view_w, view_h, zbuf, f.v[0], f.v[2], f.v[3], f.color, budget,
        ) {
            return false;
        }
    }
    true
}

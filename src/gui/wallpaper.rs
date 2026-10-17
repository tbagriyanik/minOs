//! Masaüstü arka planı (duvar kağıdı) motoru.
//!
//! Ayarlar uygulamasından seçilebilir: tek renk veya
//! prosedürel örnek desenler ve mini masaüstü resimleri.
//! PNG çözücüye ihtiyaç duymaz; her karede hızlı çizilecek
//! şekilde tasarlanmıştır.

use crate::drivers::video::Framebuffer;
use crate::gui::Color;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Duvar kağıdı kimliği.
/// 0 = tema rengi (varsayılan gradyan), 1..=6 düz renkler,
/// 7..=9 mini masaüstü resimleri.
pub const COUNT: usize = 10;

static CURRENT: AtomicU8 = AtomicU8::new(0);
static SOLID: AtomicU32 = AtomicU32::new(0x1E3A5F);

pub const SOLID_COLORS: [u32; 6] = [
    0x1E3A5F, // koyu mavi
    0x1F5F3A, // yeşil
    0x5F1F2E, // bordo
    0x3A1F5F, // mor
    0x7A4A1F, // turuncu-kahve
    0x22262E, // koyu gri
];

pub fn count() -> usize { COUNT }

pub fn get() -> u8 { CURRENT.load(Ordering::Relaxed).min((COUNT - 1) as u8) }

pub fn set(id: u8) { CURRENT.store(id.min((COUNT - 1) as u8), Ordering::Relaxed); }

pub fn solid_color() -> u32 { SOLID.load(Ordering::Relaxed) }

pub fn set_solid(color: u32) { SOLID.store(color & 0xFFFFFF, Ordering::Relaxed); }

pub fn apply(id: u8, color: u32) {
    set(id);
    if id >= 1 && id <= 6 {
        set_solid(SOLID_COLORS[(id as usize - 1).min(5)]);
    } else if color != 0 {
        set_solid(color);
    }
}

pub fn name_tr(id: u8) -> &'static str {
    match id {
        0 => "Tema Rengi",
        1 => "Koyu Mavi",
        2 => "Yeşil",
        3 => "Bordo",
        4 => "Mor",
        5 => "Kahve",
        6 => "Koyu Gri",
        7 => "Yıldızlı Gece",
        8 => "Gün Batımı",
        9 => "Okyanus Dalgası",
        _ => "Tema",
    }
}

pub fn name_en(id: u8) -> &'static str {
    match id {
        0 => "Theme",
        1 => "Navy",
        2 => "Green",
        3 => "Maroon",
        4 => "Purple",
        5 => "Brown",
        6 => "Dark Gray",
        7 => "Starry Night",
        8 => "Sunset Glow",
        9 => "Ocean Waves",
        _ => "Theme",
    }
}

pub fn name(id: u8, english: bool) -> &'static str {
    if english { name_en(id) } else { name_tr(id) }
}

fn shade(rgb: u32, delta: i16) -> u32 {
    let r = ((rgb >> 16) & 0xFF) as i16;
    let g = ((rgb >> 8) & 0xFF) as i16;
    let b = (rgb & 0xFF) as i16;
    let cl = |v: i16| v.clamp(0, 255) as u32;
    (cl(r + delta) << 16) | (cl(g + delta) << 8) | cl(b + delta)
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

pub fn draw(fb: &mut Framebuffer) {
    let (w, h) = (fb.width, fb.height);
    match get() {
        0 => {
            let top = Color::DARK_BG;
            let bottom = crate::gui::MinGuiCompositor::shade(Color::DARK_BG, -12);
            fb.draw_vgradient(0, 0, w, h, top, bottom);
        }
        1..=6 => {
            let c = SOLID_COLORS[(get() as usize - 1).min(5)];
            fb.draw_vgradient(0, 0, w, h, c, shade(c, -14));
        }
        7 => draw_starry(fb, w, h),
        8 => draw_sunset(fb, w, h),
        9 => draw_ocean(fb, w, h),
        _ => draw_geometric(fb, w, h),
    }
}

#[allow(dead_code)]
fn draw_starry(fb: &mut Framebuffer, w: usize, h: usize) {
    fb.draw_vgradient(0, 0, w, h, 0x070B1A, 0x101B3A);
    let mx = w.saturating_sub(w / 5);
    let my = h / 6;
    let mr = (w.min(h) / 14).max(18);
    let mut y = my.saturating_sub(mr);
    let y_end = (my + mr).min(h);
    while y < y_end {
        let dy = y as isize - my as isize;
        let dx = int_sqrt((mr * mr).saturating_sub((dy * dy) as usize)) as usize;
        let x0 = mx.saturating_sub(dx);
        let x1 = (mx + dx).min(w);
        if x1 > x0 { fb.draw_rect(x0, y, x1 - x0, 1, 0xE8EDF7); }
        y += 1;
    }
    let n = (w * h / 9000).clamp(120, 260);
    for i in 0..n {
        let v = hash(i as u32 * 2654435761 + 0x9E3779B9);
        let x = (v as usize) % w.max(1);
        let y = (v.rotate_right(13) as usize) % h.max(1);
        let b = 0x9AA8CC + (v & 0x202020);
        fb.put_pixel(x, y, b & 0xFFFFFF);
        if v & 7 == 0 && x + 1 < w { fb.put_pixel(x + 1, y, b & 0xFFFFFF); }
    }
    let hb = h / 6;
    fb.draw_rect(0, h.saturating_sub(hb), w, hb, 0x05070F);
}

#[allow(dead_code)]
fn draw_sunset(fb: &mut Framebuffer, w: usize, h: usize) {
    let sky_h = h * 2 / 3;
    // 1. Gökyüzü: İki parçalı dikey gradyan (mor -> kızıl -> turuncu)
    fb.draw_vgradient(0, 0, w, sky_h / 2, 0x2B1B4E, 0xB44A6B);
    fb.draw_vgradient(0, sky_h / 2, w, sky_h - sky_h / 2, 0xB44A6B, 0xF5A25D);

    // 2. Güneş: Yatay satır satır hızlı çizim (çember)
    let sx = w / 2;
    let sy = sky_h.saturating_sub(sky_h / 6);
    let sr = (w.min(h) / 12).max(20);
    let mut y = sy.saturating_sub(sr);
    let y_end = (sy + sr).min(sky_h);
    while y < y_end {
        let dy = y as isize - sy as isize;
        let dx = int_sqrt((sr * sr).saturating_sub((dy * dy) as usize)) as usize;
        let x0 = sx.saturating_sub(dx);
        let x1 = (sx + dx).min(w);
        if x1 > x0 { fb.draw_rect(x0, y, x1 - x0, 1, 0xFFE6A3); }
        y += 1;
    }

    // 3. Arka Dağ Silüeti (0x3A2140): 4 piksellik blok adımlarıyla hızlı çizim (1280 yerine 320 çağrı)
    let base = sky_h;
    let step = 4;
    let mut x = 0;
    while x < w {
        let bw = step.min(w - x);
        let h1 = base + pseudo(x, 7, h / 8);
        let y1 = h1.min(h);
        if y1 < h { fb.draw_rect(x, y1, bw, h - y1, 0x3A2140); }
        x += step;
    }

    // 4. Ön Dağ Silüeti (0x241426): 4 piksellik blok adımlarıyla hızlı çizim
    let mut x = 0;
    while x < w {
        let bw = step.min(w - x);
        let h2 = base + pseudo(x + 3917, 13, h / 5);
        let y2 = h2.min(h);
        if y2 < h { fb.draw_rect(x, y2, bw, h - y2, 0x241426); }
        x += step;
    }

    // 5. Taban zemin dolgusu: ufuk altındaki en alt kısmı tek bir yatay blokla mühürle
    let ground_y = (base + h / 5).min(h);
    if ground_y < h {
        fb.draw_rect(0, ground_y, w, h - ground_y, 0x241426);
    }
}

#[allow(dead_code)]
fn draw_ocean(fb: &mut Framebuffer, w: usize, h: usize) {
    fb.draw_vgradient(0, 0, w, h, 0x0A2E4E, 0x0E5A7A);
    let bands = 4;
    let step = 4;
    for b in 0..bands {
        let y = h * (b + 1) / (bands + 1);
        let amp = 6 + b * 3;
        let col = shade(0x2EA8C8, (b as i16 - 2) * 8);
        let mut x = 0;
        while x < w {
            let bw = step.min(w - x);
            let off = pseudo(x + b * 733, 5, amp * 2) as isize - amp as isize;
            let yy = (y as isize + off).clamp(0, h as isize - 1) as usize;
            fb.draw_rect(x, yy, bw, 2, col);
            x += step;
        }
    }
}

fn draw_geometric(fb: &mut Framebuffer, w: usize, h: usize) {
    fb.draw_vgradient(0, 0, w, h, 0x14161E, 0x232733);
    let step = 56usize;
    let mut k = 0usize;
    while k * step < w + h {
        let c = if (k & 1) == 0 { 0x2E3B55 } else { 0x39466A };
        let mut y = 0usize;
        while y < h {
            let x = k * step + y / 2;
            if x < w {
                let len = 10.min(w - x);
                fb.draw_rect(x, y, len, 1, c);
            }
            y += 3;
        }
        k += 1;
    }
    let cx = w / 2;
    let cy = h / 2;
    let r = (w.min(h) / 6).max(30);
    for dy in 0..=r * 2 {
        let y = cy.saturating_sub(r) + dy;
        if y >= h { continue; }
        let d = dy as isize - r as isize;
        let dx = int_sqrt((r * r).saturating_sub((d * d) as usize)) as usize;
        let x0 = cx.saturating_sub(dx);
        let x1 = (cx + dx).min(w.saturating_sub(1));
        if x1 > x0 {
            fb.put_pixel(x0, y, 0x89B4FA);
            fb.put_pixel(x1, y, 0x89B4FA);
        }
    }
}

pub fn draw_preview(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, id: u8) {
    if w < 4 || h < 4 { return; }
    match id {
        0 => {
            let (bg, _) = crate::gui::theme::preview_colors(crate::gui::theme::current());
            fb.draw_rect(x, y, w, h, bg);
            fb.draw_rect(x, y, w, h / 2, shade(bg, 10));
        }
        1..=6 => {
            let c = SOLID_COLORS[(id as usize - 1).min(5)];
            fb.draw_rect(x, y, w, h, c);
        }
        7 => {
            // Yıldızlı Gece önizleme
            fb.draw_vgradient(x, y, w, h, 0x070B1A, 0x101B3A);
            fb.draw_rect(x + w * 3 / 4, y + h / 5, 4, 4, 0xE8EDF7);
            fb.draw_rect(x, y + h * 5 / 6, w, h / 6, 0x05070F);
        }
        8 => {
            // Gün Batımı önizleme
            fb.draw_vgradient(x, y, w, h / 2, 0x2B1B4E, 0xB44A6B);
            fb.draw_vgradient(x, y + h / 2, w, h - h / 2, 0xB44A6B, 0xF5A25D);
            fb.draw_rect(x + w / 2 - 3, y + h / 2 - 3, 6, 6, 0xFFE6A3);
            fb.draw_rect(x, y + h * 2 / 3, w, h / 3, 0x241426);
        }
        9 => {
            // Okyanus Dalgası önizleme
            fb.draw_vgradient(x, y, w, h, 0x0A2E4E, 0x0E5A7A);
            fb.draw_rect(x, y + h / 3, w, 2, 0x2EA8C8);
            fb.draw_rect(x, y + h * 2 / 3, w, 2, 0x3ED8E8);
        }
        _ => {
            fb.draw_rect(x, y, w, h, 0x1A1E2A);
        }
    }
}

#[inline(always)]
fn int_sqrt(n: usize) -> usize {
    let mut x = n;
    let mut y = 0usize;
    let mut bit = 1usize << ((usize::BITS - n.leading_zeros().max(1)) & !1);
    while bit != 0 {
        let t = y + bit;
        y >>= 1;
        if x >= t {
            x -= t;
            y += bit;
        }
        bit >>= 2;
    }
    y
}

#[allow(dead_code)]
#[inline(always)]
fn pseudo(x: usize, mul: usize, range: usize) -> usize {
    if range == 0 { return 0; }
    let v = hash((x.wrapping_mul(mul).wrapping_add(0x51F3)) as u32);
    (v as usize) % range
}
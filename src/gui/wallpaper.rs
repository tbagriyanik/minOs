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

// Duvar kağıdı önbelleği (prosedürel resimlerin ve gradyanların 60 FPS'de
// her karede yeniden matematiksel olarak hesaplanıp çizilmesini önler; tek seferde
// çizilir ve her karede doğrudan hafızadan blit edilir).
static mut WALLPAPER_CACHE: [u32; crate::drivers::video::BACKBUFFER_LEN] = [0; crate::drivers::video::BACKBUFFER_LEN];
static mut CACHED_ID: u8 = 255;
static mut CACHED_SOLID: u32 = 0;
static mut CACHED_THEME: u8 = 255;
static mut CACHED_W: usize = 0;
static mut CACHED_H: usize = 0;
static mut CACHE_VALID: bool = false;

pub fn invalidate_cache() {
    unsafe {
        CACHE_VALID = false;
        CACHED_ID = 255;
    }
}

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

pub fn set(id: u8) {
    let old = CURRENT.swap(id.min((COUNT - 1) as u8), Ordering::Relaxed);
    if old != id {
        invalidate_cache();
    }
}

pub fn solid_color() -> u32 { SOLID.load(Ordering::Relaxed) }

pub fn set_solid(color: u32) {
    let col = color & 0xFFFFFF;
    let old = SOLID.swap(col, Ordering::Relaxed);
    if old != col {
        invalidate_cache();
    }
}

pub fn apply(id: u8, color: u32) {
    set(id);
    if id >= 1 && id <= 6 {
        set_solid(SOLID_COLORS[(id as usize - 1).min(5)]);
    } else if color != 0 {
        set_solid(color);
    }
    invalidate_cache();
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
        8 => "Petek Dokusu",
        9 => "Karbon Doku",
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
        8 => "Hexagon Matrix",
        9 => "Carbon Fiber",
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
    let cur_id = get();
    let cur_solid = solid_color();
    let cur_theme = crate::gui::theme::current().index();

    let needs_render = unsafe {
        !CACHE_VALID
            || CACHED_ID != cur_id
            || CACHED_SOLID != cur_solid
            || CACHED_THEME != cur_theme
            || CACHED_W != w
            || CACHED_H != h
    };

    if needs_render {
        // Önbellek yenile: Arka planı backbuffer'a prosedürel olarak çiz
        match cur_id {
            0 => {
                let top = Color::DARK_BG;
                let bottom = crate::gui::MinGuiCompositor::shade(Color::DARK_BG, -12);
                fb.draw_vgradient(0, 0, w, h, top, bottom);
            }
            1..=6 => {
                let c = SOLID_COLORS[(cur_id as usize - 1).min(5)];
                fb.draw_vgradient(0, 0, w, h, c, shade(c, -14));
            }
            7 => draw_starry(fb, w, h),
            8 => draw_hex_pattern(fb, w, h),
            9 => draw_carbon_fiber(fb, w, h),
            _ => draw_geometric(fb, w, h),
        }

        // Çizilen pikselleri önbelleğe al
        unsafe {
            fb.copy_backbuffer_to_raw(core::ptr::addr_of_mut!(WALLPAPER_CACHE) as *mut u32, crate::drivers::video::BACKBUFFER_LEN);
            CACHED_ID = cur_id;
            CACHED_SOLID = cur_solid;
            CACHED_THEME = cur_theme;
            CACHED_W = w;
            CACHED_H = h;
            CACHE_VALID = true;
        }
    } else {
        // Önbellek geçerli: Tek bir ultra hızlı memcpy ile backbuffer'ı doldur
        unsafe {
            fb.copy_backbuffer_from_raw(core::ptr::addr_of!(WALLPAPER_CACHE) as *const u32, crate::drivers::video::BACKBUFFER_LEN);
        }
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

/// Petek Dokusu (Hexagon Matrix) - Modern, teknolojik petek ızgara deseni
#[allow(dead_code)]
fn draw_hex_pattern(fb: &mut Framebuffer, w: usize, h: usize) {
    fb.draw_vgradient(0, 0, w, h, 0x0E1420, 0x182438);

    let cell_w = 40usize;
    let cell_h = 24usize;

    let mut row = 0usize;
    let mut y = 0usize;
    while y < h {
        let x_offset = if (row & 1) == 1 { cell_w / 2 } else { 0 };
        let mut x = x_offset;
        while x < w {
            // Petek merkez çekirdeği (küçük parlayan blok)
            let core_x = x + cell_w / 2;
            let core_y = y + cell_h / 2;
            if core_x + 3 < w && core_y + 3 < h {
                let col = if (row + (x / cell_w)) % 5 == 0 {
                    0x3882B8 // Vurgulu petek
                } else {
                    0x1B2C44 // Normal petek çizgisi
                };
                fb.draw_rect(core_x, core_y, 4, 4, col);
            }
            // Yatay üst ve alt kenar çizgileri
            if x + 16 < w && y < h {
                fb.draw_rect(x + 4, y, 12, 1, 0x223652);
            }
            // Dikey bağlantı noktası
            if x < w && y + 6 < h {
                fb.draw_rect(x, y + 2, 1, 6, 0x1A2A40);
            }
            x += cell_w;
        }
        y += cell_h;
        row += 1;
    }
}

/// Karbon Doku (Carbon Fiber) - Şık, 3B dokulu örgü karbon fiber deseni (Twill Weave)
#[allow(dead_code)]
fn draw_carbon_fiber(fb: &mut Framebuffer, w: usize, h: usize) {
    // Koyu antrasit zemin
    fb.clear(0x0C0D0F);

    // 8x8 piksellik örgülü karbon lifi deseni
    // Her 8x8 hücre 4 adet 4x4'lük bloktan oluşur (2x2 twill örgü simülasyonu)
    let block = 4usize;
    let cell = block * 2; // 8

    let mut cy = 0usize;
    while cy < h {
        let block_row = cy / block;
        let mut cx = 0usize;
        while cx < w {
            let block_col = cx / block;
            // Çapraz dama / örgü yönü:
            // Bir blok yatay lifleri (üstten aydınlatmalı), diğeri dikey lifleri temsil eder
            let is_horizontal = ((block_row + block_col) & 1) == 0;

            if is_horizontal {
                // Yatay karbon ipliği: 4 satır, üst açık/parlak gri, alt gölgeli koyu gri
                let bw = block.min(w - cx);
                if cy < h { fb.draw_rect(cx, cy, bw, 1, 0x2A2C32); }
                if cy + 1 < h { fb.draw_rect(cx, cy + 1, bw, 1, 0x222428); }
                if cy + 2 < h { fb.draw_rect(cx, cy + 2, bw, 1, 0x1A1B1E); }
                if cy + 3 < h { fb.draw_rect(cx, cy + 3, bw, 1, 0x121316); }
            } else {
                // Dikey karbon ipliği: sol kenar aydınlık, sağ kenar gölgeli
                let bh = block.min(h - cy);
                if cx < w { fb.draw_rect(cx, cy, 1, bh, 0x282A30); }
                if cx + 1 < w { fb.draw_rect(cx + 1, cy, 1, bh, 0x202226); }
                if cx + 2 < w { fb.draw_rect(cx + 2, cy, 1, bh, 0x18191C); }
                if cx + 3 < w { fb.draw_rect(cx + 3, cy, 1, bh, 0x101114); }
            }

            cx += block;
        }
        cy += block;
    }

    // Karbon dikiş kesişim noktaları ve derinlik gölgeleri:
    // Her 8 pikselde bir hafif koyu ayrım çizgisi
    let mut grid_y = 0usize;
    while grid_y < h {
        fb.draw_rect(0, grid_y, w, 1, 0x08080A);
        grid_y += cell;
    }
    let mut grid_x = 0usize;
    while grid_x < w {
        fb.draw_rect(grid_x, 0, 1, h, 0x08080A);
        grid_x += cell;
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
            fb.draw_rect(x + w / 4, y + h / 3, 2, 2, 0xB0C4DE);
            fb.draw_rect(x + w / 2, y + h / 4, 2, 2, 0xD0E0FF);
            fb.draw_rect(x, y + h * 5 / 6, w, h / 6, 0x05070F);
        }
        8 => {
            // Petek Dokusu önizleme (Hexagon Matrix)
            fb.draw_vgradient(x, y, w, h, 0x0E1420, 0x182438);
            fb.draw_rect(x + 4, y + 4, 6, 1, 0x3882B8);
            fb.draw_rect(x + 14, y + 8, 6, 1, 0x223652);
            fb.draw_rect(x + 6, y + 14, 6, 1, 0x3882B8);
            fb.draw_rect(x + 16, y + 18, 6, 1, 0x223652);
        }
        9 => {
            // Karbon Doku önizleme (Carbon Fiber)
            fb.draw_rect(x, y, w, h, 0x0C0D0F);
            let mut cy = y;
            while cy < y + h {
                let row = (cy - y) / 3;
                let mut cx = x;
                while cx < x + w {
                    let col = (cx - x) / 3;
                    let is_horiz = ((row + col) & 1) == 0;
                    let c = if is_horiz { 0x24262C } else { 0x16171A };
                    let bw = 3.min(x + w - cx);
                    let bh = 3.min(y + h - cy);
                    fb.draw_rect(cx, cy, bw, bh, c);
                    cx += 3;
                }
                cy += 3;
            }
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
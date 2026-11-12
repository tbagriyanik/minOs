//! Masaüstü arka planı (duvar kağıdı) motoru.
//!
//! Ayarlar uygulamasından seçilebilir: tek renk veya
//! prosedürel örnek desenler ve mini masaüstü resimleri.
//! PNG çözücüye ihtiyaç duymaz; her karede hızlı çizilecek
//! şekilde tasarlanmıştır.

#![allow(static_mut_refs)]

use crate::drivers::video::Framebuffer;
use crate::gui::Color;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Duvar kağıdı kimliği.
/// 0 = tema rengi (varsayılan gradyan), 1..=6 düz renkler,
/// 7..=9 mini masaüstü resimleri.
pub const COUNT: usize = 10;

static CURRENT: AtomicU8 = AtomicU8::new(0);
static SOLID: AtomicU32 = AtomicU32::new(0x1E3A5F);
static mut CUSTOM_RGB: Option<Arc<[u8]>> = None;
static mut CUSTOM_W: usize = 0;
static mut CUSTOM_H: usize = 0;

pub fn set_custom_image(rgb: Arc<[u8]>, width: usize, height: usize) {
    unsafe {
        CUSTOM_RGB = Some(rgb);
        CUSTOM_W = width;
        CUSTOM_H = height;
    }
    invalidate_cache();
}

// Duvar kağıdı önbelleği (prosedürel resimlerin ve gradyanların 60 FPS'de
// her karede yeniden matematiksel olarak hesaplanıp çizilmesini önler; tek seferde
// çizilir ve her karede doğrudan hafızadan blit edilir).
static mut WALLPAPER_CACHE: [u32; crate::drivers::video::BACKBUFFER_LEN] =
    [0; crate::drivers::video::BACKBUFFER_LEN];
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

pub fn count() -> usize {
    COUNT
}

pub fn get() -> u8 {
    CURRENT.load(Ordering::Relaxed).min((COUNT - 1) as u8)
}

pub fn set(id: u8) {
    let old = CURRENT.swap(id.min((COUNT - 1) as u8), Ordering::Relaxed);
    if old != id {
        invalidate_cache();
    }
}

pub fn solid_color() -> u32 {
    SOLID.load(Ordering::Relaxed)
}

pub fn set_solid(color: u32) {
    let col = color & 0xFFFFFF;
    let old = SOLID.swap(col, Ordering::Relaxed);
    if old != col {
        invalidate_cache();
    }
}

pub fn apply(id: u8, color: u32) {
    unsafe {
        CUSTOM_RGB = None;
        CUSTOM_W = 0;
        CUSTOM_H = 0;
    }
    set(id);
    if (1..=6).contains(&id) {
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
        7 => "Masaüstü 1",
        8 => "Masaüstü 2",
        9 => "Masaüstü 3",
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
        7 => "Desktop 1",
        8 => "Desktop 2",
        9 => "Desktop 3",
        _ => "Theme",
    }
}

pub fn name(id: u8, english: bool) -> &'static str {
    if english {
        name_en(id)
    } else {
        name_tr(id)
    }
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

#[allow(static_mut_refs)]
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
        // Önbellek yenilemesi tüm ekranı kaplar; kirli bölge kırpmasını geçici
        // olarak kaldırıp arka planı tüm ekrana çizip önbelleğe al.
        let saved = fb.base_clip();
        fb.clear_frame_clip();
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
            7 => draw_png_tile(fb, w, h, include_bytes!("../../public/desktop1.png")),
            8 => draw_png_tile(fb, w, h, include_bytes!("../../public/desktop2.png")),
            9 => draw_png_tile(fb, w, h, include_bytes!("../../public/desktop3.png")),
            _ => draw_geometric(fb, w, h),
        }
        unsafe {
            let custom_rgb = CUSTOM_RGB.clone();
            if let (Some(rgb), true) = (custom_rgb, CUSTOM_W > 0 && CUSTOM_H > 0) {
                for y in 0..h {
                    for x in 0..w {
                        let i = ((y % CUSTOM_H) * CUSTOM_W + (x % CUSTOM_W)) * 3;
                        if i + 2 < rgb.len() {
                            fb.put_pixel(
                                x,
                                y,
                                ((rgb[i] as u32) << 16)
                                    | ((rgb[i + 1] as u32) << 8)
                                    | rgb[i + 2] as u32,
                            );
                        }
                    }
                }
            }
        }

        // Çizilen pikselleri önbelleğe al
        unsafe {
            fb.copy_backbuffer_to_raw(
                core::ptr::addr_of_mut!(WALLPAPER_CACHE) as *mut u32,
                crate::drivers::video::BACKBUFFER_LEN,
            );
            CACHED_ID = cur_id;
            CACHED_SOLID = cur_solid;
            CACHED_THEME = cur_theme;
            CACHED_W = w;
            CACHED_H = h;
            CACHE_VALID = true;
        }
        // Kırpmayı (varsa kirli dikdörtgeni) geri yükle.
        fb.set_frame_clip(Some((
            saved.x0,
            saved.y0,
            saved.x1 - saved.x0,
            saved.y1 - saved.y0,
        )));
    } else {
        // Önbellek geçerli: yalnızca çerçeve (kirli) bölgesini tek memcpy ile geri yükle.
        let region = fb.base_clip();
        unsafe {
            fb.copy_region_from_raw(core::ptr::addr_of!(WALLPAPER_CACHE) as *const u32, region);
        }
    }
}

/// Ayarlar penceresindeki PNG duvar kağıdı önizlemeleri için küçük resim
/// ölçüsü. Tam boy PNG'yi her karede çözmek pahalı olduğundan bir kez
/// küçültülüp önbelleğe alınır.
const THUMB_W: usize = 128;
const THUMB_H: usize = 48;

static mut THUMB_PIXELS: [[u32; THUMB_W * THUMB_H]; 3] = [[0; THUMB_W * THUMB_H]; 3];
static mut THUMB_READY: [bool; 3] = [false; 3];

fn thumb_source(idx: usize) -> &'static [u8] {
    match idx {
        0 => include_bytes!("../../public/desktop1.png"),
        1 => include_bytes!("../../public/desktop2.png"),
        _ => include_bytes!("../../public/desktop3.png"),
    }
}

/// 7..=9 numaralı PNG duvar kağıtı için önizleme piksellerini bir kez hazırlar.
/// Tam boy PNG'yi çözüp 128x48 lik küçük resime indirir. Bu işlem pahalıdır;
/// çağrıcı bunu ekran ayarları açılışında değil, önyükleme sırasında tek seferlik
/// yapmalıdır.
#[allow(static_mut_refs)]
pub fn ensure_thumb(idx: usize) {
    unsafe {
        if THUMB_READY[idx] {
            return;
        }
        let Some(decoded) = crate::apps::imgdecode::decode_png(thumb_source(idx)) else {
            // Çözme başarısız olsa bile işaretleyip tekrar denemeyi engelle.
            THUMB_READY[idx] = true;
            return;
        };
        let (sw, sh) = (decoded.width, decoded.height);
        if sw == 0 || sh == 0 {
            THUMB_READY[idx] = true;
            return;
        }
        // Kutu oranına uydurmak için kaynağı ortadan kırp; böylece küçük
        // resimlerde de duvar kağıdının tamamı değil, merkezi görünür.
        let (crop_w, crop_h, x0, y0) = if sw * THUMB_H > sh * THUMB_W {
            let cw = (sh * THUMB_W / THUMB_H).max(1);
            (cw, sh, (sw - cw) / 2, 0)
        } else {
            let ch = (sw * THUMB_H / THUMB_W).max(1);
            (sw, ch, 0, (sh - ch) / 2)
        };
        for ty in 0..THUMB_H {
            let sy = y0 + ty * crop_h / THUMB_H;
            for tx in 0..THUMB_W {
                let sx = x0 + tx * crop_w / THUMB_W;
                THUMB_PIXELS[idx][ty * THUMB_W + tx] = decoded.pixel(sx, sy);
            }
        }
        THUMB_READY[idx] = true;
    }
}

/// Tüm PNG duvar kağıdı küçük resimlerini (7, 8, 9) önceden çözer.
/// Önyükleme ekranında tek seferlik çağrılmalıdır; böylece ekran ayarları
/// açıldığında fare takılmaz.
pub fn ensure_all_thumbs() {
    unsafe {
        for idx in 0..THUMB_READY.len() {
            if !thumb_ready(idx) {
                ensure_thumb(idx);
            }
        }
    }
}

/// Tüm PNG küçük resimleri hazır mı?
pub fn thumbs_ready() -> bool {
    unsafe { THUMB_READY[0] && THUMB_READY[1] && THUMB_READY[2] }
}

/// Belirli bir PNG küçük resmi hazır mı?
pub fn thumb_ready(idx: usize) -> bool {
    unsafe { THUMB_READY.get(idx).copied().unwrap_or(false) }
}

/// PNG duvar kağıdının önbelleğe alınmış küçük resmini kutuya sığdırır.
#[allow(static_mut_refs)]
fn draw_thumb_preview(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, idx: usize) {
    ensure_thumb(idx);
    unsafe {
        let px = &THUMB_PIXELS[idx];
        for ty in 0..h {
            let sy = ty * THUMB_H / h;
            for tx in 0..w {
                let sx = tx * THUMB_W / w;
                fb.put_pixel(x + tx, y + ty, px[sy * THUMB_W + sx]);
            }
        }
    }
}

fn draw_png_tile(fb: &mut Framebuffer, w: usize, h: usize, data: &[u8]) {
    if let Some(decoded) = crate::apps::imgdecode::decode_png(data) {
        let (iw, ih) = (decoded.width, decoded.height);
        if iw == 0 || ih == 0 {
            return;
        }
        for y in 0..h {
            for x in 0..w {
                let i = ((y % ih) * iw + (x % iw)) * 3;
                if i + 2 < decoded.rgb.len() {
                    fb.put_pixel(
                        x,
                        y,
                        ((decoded.rgb[i] as u32) << 16)
                            | ((decoded.rgb[i + 1] as u32) << 8)
                            | decoded.rgb[i + 2] as u32,
                    );
                }
            }
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
        let dx = int_sqrt((mr * mr).saturating_sub((dy * dy) as usize));
        let x0 = mx.saturating_sub(dx);
        let x1 = (mx + dx).min(w);
        if x1 > x0 {
            fb.draw_rect(x0, y, x1 - x0, 1, 0xE8EDF7);
        }
        y += 1;
    }
    let n = (w * h / 9000).clamp(120, 260);
    for i in 0..n {
        let v = hash(i as u32 * 2654435761 + 0x9E3779B9);
        let x = (v as usize) % w.max(1);
        let y = (v.rotate_right(13) as usize) % h.max(1);
        let b = 0x9AA8CC + (v & 0x202020);
        fb.put_pixel(x, y, b & 0xFFFFFF);
        if v & 7 == 0 && x + 1 < w {
            fb.put_pixel(x + 1, y, b & 0xFFFFFF);
        }
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
                if cy < h {
                    fb.draw_rect(cx, cy, bw, 1, 0x2A2C32);
                }
                if cy + 1 < h {
                    fb.draw_rect(cx, cy + 1, bw, 1, 0x222428);
                }
                if cy + 2 < h {
                    fb.draw_rect(cx, cy + 2, bw, 1, 0x1A1B1E);
                }
                if cy + 3 < h {
                    fb.draw_rect(cx, cy + 3, bw, 1, 0x121316);
                }
            } else {
                // Dikey karbon ipliği: sol kenar aydınlık, sağ kenar gölgeli
                let bh = block.min(h - cy);
                if cx < w {
                    fb.draw_rect(cx, cy, 1, bh, 0x282A30);
                }
                if cx + 1 < w {
                    fb.draw_rect(cx + 1, cy, 1, bh, 0x202226);
                }
                if cx + 2 < w {
                    fb.draw_rect(cx + 2, cy, 1, bh, 0x18191C);
                }
                if cx + 3 < w {
                    fb.draw_rect(cx + 3, cy, 1, bh, 0x101114);
                }
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
        if y >= h {
            continue;
        }
        let d = dy as isize - r as isize;
        let dx = int_sqrt((r * r).saturating_sub((d * d) as usize));
        let x0 = cx.saturating_sub(dx);
        let x1 = (cx + dx).min(w.saturating_sub(1));
        if x1 > x0 {
            fb.put_pixel(x0, y, 0x89B4FA);
            fb.put_pixel(x1, y, 0x89B4FA);
        }
    }
}

pub fn draw_preview(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, id: u8) {
    if w < 4 || h < 4 {
        return;
    }
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
        7..=9 => {
            // Kutuda, seçildiğinde uygulanacak PNG duvar kağıdının kendisi
            // görünür; başka bir masaüstü resmi önizlenmez.
            draw_thumb_preview(fb, x, y, w, h, (id - 7) as usize);
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
    if range == 0 {
        return 0;
    }
    let v = hash((x.wrapping_mul(mul).wrapping_add(0x51F3)) as u32);
    (v as usize) % range
}

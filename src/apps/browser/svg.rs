//! SVG alt kümesi: statik vektör şekilleri tarayıcı bitmap'ine rasterize eder (no_std).
//!
//! Desteklenenler: `<svg>` kökü (width/height/viewBox), `<g>`/iç içe `<svg>`
//! transform'ları (translate/scale/rotate/matrix), `rect`, `circle`, `ellipse`,
//! `line`, `polyline`, `polygon`, `path` (M/L/H/V/C/S/Q/T/Z + A→doğru parçası),
//! `fill`/`stroke`/`stroke-width`/`opacity` sunum nitelikleri ve `style` özniteliği.
//! `<defs>`/`<title>`/`<desc>`/`<metadata>`/`<script>`/`text`/`image`/`use` atlanır.
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::types::html_color;

// ── no_std float yardımcıları ─────────────────────────────────────

const FPI: f32 = core::f32::consts::PI;

fn fsin(mut x: f32) -> f32 {
    while x > FPI {
        x -= core::f32::consts::TAU;
    }
    while x < -FPI {
        x += core::f32::consts::TAU;
    }
    let x2 = x * x;
    let x3 = x * x2;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0
}

fn fcos(x: f32) -> f32 {
    fsin(x + core::f32::consts::FRAC_PI_2)
}

fn fsqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
    for _ in 0..20 {
        g = 0.5 * (g + x / g);
    }
    g
}

fn ffloor(x: f32) -> f32 {
    let i = x as i32;
    let f = i as f32;
    if f > x {
        f - 1.0
    } else {
        f
    }
}

fn fceil(x: f32) -> f32 {
    let i = x as i32;
    let f = i as f32;
    if f < x {
        f + 1.0
    } else {
        f
    }
}

fn fround(x: f32) -> f32 {
    ffloor(x + 0.5)
}

/// `f32` üzerinde libm metot adlarıyla aynı arayüzü sunan uzatma; no_std
/// çekirdekte `sin/floor/...` yerleşik olmadığı için yardımcı fonksiyonlara
/// bağlanır. `min`/`max`/`clamp`/`abs` gibi core'da var olanlar dokunulmaz.
trait SvgFloatExt {
    fn sin(self) -> f32;
    fn cos(self) -> f32;
    fn sqrt(self) -> f32;
    fn floor(self) -> f32;
    fn ceil(self) -> f32;
    fn round(self) -> f32;
}

impl SvgFloatExt for f32 {
    fn sin(self) -> f32 {
        fsin(self)
    }
    fn cos(self) -> f32 {
        fcos(self)
    }
    fn sqrt(self) -> f32 {
        fsqrt(self)
    }
    fn floor(self) -> f32 {
        ffloor(self)
    }
    fn ceil(self) -> f32 {
        fceil(self)
    }
    fn round(self) -> f32 {
        fround(self)
    }
}

#[derive(Clone, Copy, Default)]
struct Mat {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat {
    fn identity() -> Self {
        Mat {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }
    /// self * other (other önce uygulanır, sonra self).
    fn mul(self, o: Mat) -> Mat {
        Mat {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

#[derive(Clone, Copy, Default)]
struct Pt {
    x: f32,
    y: f32,
}

struct Shape {
    /// Doldurma rengi (None = none/şeffaf).
    fill: Option<(u8, u8, u8, u8)>,
    /// Çizgi rengi.
    stroke: Option<(u8, u8, u8, u8)>,
    stroke_w: f32,
    opacity: f32,
    /// Yerel koordinatlar; rasterizasyon öncesi CTM uygulanır.
    kind: ShapeKind,
}

enum ShapeKind {
    Poly { pts: Vec<Pt>, closed: bool },
}

struct SvgParser<'a> {
    src: &'a [u8],
    pos: usize,
    width: usize,
    height: usize,
    /// CTM yığını; ilk eleman kök viewport dönüşümüdür.
    mats: Vec<Mat>,
    shapes: Vec<Shape>,
    /// İşlenmeyen alt ağaç derinliği (defs/title/desc/metadata/script).
    skip: usize,
}

fn parse_attrs(raw: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < raw.len() {
        while i < raw.len() && (raw[i].is_ascii_whitespace() || raw[i] == b'/') {
            i += 1;
        }
        let ks = i;
        while i < raw.len()
            && (raw[i].is_ascii_alphanumeric()
                || raw[i] == b'-'
                || raw[i] == b'_'
                || raw[i] == b':')
        {
            i += 1;
        }
        if i == ks {
            i += 1;
            continue;
        }
        let key = String::from(core::str::from_utf8(&raw[ks..i]).unwrap_or(""));
        while i < raw.len() && raw[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= raw.len() || raw[i] != b'=' {
            out.push((key, String::new()));
            continue;
        }
        i += 1;
        while i < raw.len() && raw[i].is_ascii_whitespace() {
            i += 1;
        }
        let val = if i < raw.len() && (raw[i] == b'"' || raw[i] == b'\'') {
            let q = raw[i];
            i += 1;
            let vs = i;
            while i < raw.len() && raw[i] != q {
                i += 1;
            }
            let v = &raw[vs..i];
            i = (i + 1).min(raw.len());
            v
        } else {
            let vs = i;
            while i < raw.len() && !raw[i].is_ascii_whitespace() && raw[i] != b'>' {
                i += 1;
            }
            &raw[vs..i]
        };
        out.push((key, String::from(core::str::from_utf8(val).unwrap_or(""))));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `style="fill:#f00;stroke-width:2"` içindeki bildirimi bulur.
fn style_decl<'a>(attrs: &'a [(String, String)], prop: &str) -> Option<&'a str> {
    let style = attr(attrs, "style")?;
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            if k.trim().eq_ignore_ascii_case(prop) {
                return Some(v.trim());
            }
        }
    }
    None
}

fn paint(attrs: &[(String, String)], prop: &str) -> Option<String> {
    style_decl(attrs, prop)
        .map(String::from)
        .or_else(|| attr(attrs, prop).map(String::from))
}

fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    if s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") {
        return None;
    }
    html_color(s.as_bytes()).map(|c| {
        (
            ((c >> 16) & 0xff) as u8,
            ((c >> 8) & 0xff) as u8,
            (c & 0xff) as u8,
        )
    })
}

fn parse_num(s: &str) -> f32 {
    s.trim().parse::<f32>().unwrap_or(0.0)
}

/// "10 20", "10,20", "10-20" biçimlerindeki sayı listesi.
fn parse_num_list(s: &str) -> Vec<f32> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let start = i;
        if b[i] == b'+' || b[i] == b'-' {
            i += 1;
        }
        let mut seen_digit = false;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            seen_digit = true;
            i += 1;
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_digit() {
                i = j;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if seen_digit {
            if let Ok(v) = core::str::from_utf8(&b[start..i])
                .unwrap_or("")
                .parse::<f32>()
            {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn parse_transform(s: &str) -> Mat {
    let mut m = Mat::identity();
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        while i < b.len() && !(b[i].is_ascii_alphabetic()) {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let name_start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let name = &s[name_start..i];
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() || b[i] != b'(' {
            continue;
        }
        i += 1;
        let arg_start = i;
        let mut depth = 1i32;
        while i < b.len() && depth > 0 {
            if b[i] == b'(' {
                depth += 1;
            } else if b[i] == b')' {
                depth -= 1;
            }
            i += 1;
        }
        let args = parse_num_list(&s[arg_start..i.saturating_sub(1)]);
        let t = match name {
            "translate" => Mat {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: args.first().copied().unwrap_or(0.0),
                f: args.get(1).copied().unwrap_or(0.0),
            },
            "scale" => {
                let sx = args.first().copied().unwrap_or(1.0);
                let sy = args.get(1).copied().unwrap_or(sx);
                Mat {
                    a: sx,
                    b: 0.0,
                    c: 0.0,
                    d: sy,
                    e: 0.0,
                    f: 0.0,
                }
            }
            "rotate" => {
                let ang = args.first().copied().unwrap_or(0.0) * (FPI / 180.0);
                let (sin, cos) = (fsin(ang), fcos(ang));
                let mut r = Mat {
                    a: cos,
                    b: sin,
                    c: -sin,
                    d: cos,
                    e: 0.0,
                    f: 0.0,
                };
                if args.len() >= 3 {
                    let cx = args[1];
                    let cy = args[2];
                    r = Mat {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: cx,
                        f: cy,
                    }
                    .mul(r)
                    .mul(Mat {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: -cx,
                        f: -cy,
                    });
                }
                r
            }
            "matrix" if args.len() >= 6 => Mat {
                a: args[0],
                b: args[1],
                c: args[2],
                d: args[3],
                e: args[4],
                f: args[5],
            },
            _ => Mat::identity(),
        };
        m = m.mul(t);
    }
    m
}

/// viewBox dönüşümü: minx/miny/w/h → hedef w/h (meet, ortalı).
fn viewbox_mat(minx: f32, miny: f32, vw: f32, vh: f32, w: usize, h: usize) -> Mat {
    if vw <= 0.0 || vh <= 0.0 {
        return Mat::identity();
    }
    let s = (w as f32 / vw).min(h as f32 / vh);
    let ox = (w as f32 - vw * s) / 2.0 - minx * s;
    let oy = (h as f32 - vh * s) / 2.0 - miny * s;
    Mat {
        a: s,
        b: 0.0,
        c: 0.0,
        d: s,
        e: ox,
        f: oy,
    }
}

fn len_or_pct(s: &str, base: f32, default: f32) -> f32 {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return parse_num(p) / 100.0 * base;
    }
    if s.is_empty() {
        default
    } else {
        parse_num(s)
    }
}

// ── Path düzleştirme ──────────────────────────────────────────────

fn path_tokens(d: &str) -> Vec<(u8, f32)> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() {
            // Büyük/küçük harf korunur — küçük harf göreli komut demektir.
            out.push((c, 0.0));
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        let start = i;
        if b[i] == b'+' || b[i] == b'-' {
            i += 1;
        }
        let mut seen = false;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            seen = true;
            i += 1;
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_digit() {
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        if seen {
            if let Ok(v) = core::str::from_utf8(&b[start..i])
                .unwrap_or("")
                .parse::<f32>()
            {
                // Küçük harf komutlar için işaretçi: negatif değer -1 işareti yerine
                // komut bayrağı kullanılır; göreli komutlar ayrı çözülür.
                out.push((0, v));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Path `d` özniteliğini alt yollara düzleştirir (eğriler doğru parçalarına indirgenir).
fn flatten_path(d: &str) -> Vec<Vec<Pt>> {
    let toks = path_tokens(d);
    let mut subs: Vec<Vec<Pt>> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut i = 0usize;
    let mut cmd = 0u8;
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut sx = 0.0f32; // alt yol başlangıcı
    let mut sy = 0.0f32;
    let mut px = 0.0f32; // önceki kontrol noktası (S/T için)
    let mut py = 0.0f32;
    let mut pcmd = 0u8;

    // Yalnızca sayı token'ı okur; komut token'ı görünce None döner
    // (böylece komutlar argüman sanılmaz).
    let num = |toks: &[(u8, f32)], i: &mut usize| -> Option<f32> {
        if *i < toks.len() && toks[*i].0 == 0 {
            let v = toks[*i].1;
            *i += 1;
            Some(v)
        } else {
            None
        }
    };

    while i < toks.len() {
        if toks[i].0 != 0 {
            cmd = toks[i].0;
            i += 1;
            if cmd.eq_ignore_ascii_case(&b'z') {
                if !cur.is_empty() {
                    cur.push(Pt { x: sx, y: sy });
                    subs.push(core::mem::take(&mut cur));
                }
                continue;
            }
        } else if cmd == 0 {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let c = cmd.to_ascii_uppercase();
        // Komut argüman sayısı; M/L/T=2, H/V=1, S/Q=4, C=6, A=7
        let need = match c {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'S' | b'Q' => 4,
            b'C' => 6,
            b'A' => 7,
            _ => 0,
        };
        if need == 0 {
            break;
        }
        loop {
            let mut args = [0.0f32; 7];
            let mut ok = true;
            for slot in args.iter_mut().take(need) {
                match num(&toks, &mut i) {
                    Some(v) => *slot = v,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            // Göreli değerleri mutlak coords'a çevir (H/V hariç — eksen bazlı)
            // Uç nokta hesabı: A komutunda (x,y) args[5..7]'dedir.
            let (mut x, mut y) = match c {
                b'H' => {
                    let nx = args[0] + if rel { cx } else { 0.0 };
                    (nx, cy)
                }
                b'V' => {
                    let ny = args[0] + if rel { cy } else { 0.0 };
                    (cx, ny)
                }
                b'A' => (
                    args[5] + if rel { cx } else { 0.0 },
                    args[6] + if rel { cy } else { 0.0 },
                ),
                _ => (
                    args[0] + if rel { cx } else { 0.0 },
                    args[1] + if rel { cy } else { 0.0 },
                ),
            };
            match c {
                b'M' => {
                    // Yeni alt yol (M sonrası tekrarlar L sayılır — aşağıda cmd güncellenir)
                    if !cur.is_empty() {
                        subs.push(core::mem::take(&mut cur));
                    }
                    cur.push(Pt { x, y });
                    sx = x;
                    sy = y;
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'L' => {
                    cur.push(Pt { x, y });
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'H' | b'V' => {
                    cur.push(Pt { x, y });
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'C' => {
                    let x1 = args[0] + if rel { cx } else { 0.0 };
                    let y1 = args[1] + if rel { cy } else { 0.0 };
                    let x2 = args[2] + if rel { cx } else { 0.0 };
                    let y2 = args[3] + if rel { cy } else { 0.0 };
                    (x, y) = (
                        args[4] + if rel { cx } else { 0.0 },
                        args[5] + if rel { cy } else { 0.0 },
                    );
                    for k in 1..=16 {
                        let t = k as f32 / 16.0;
                        let mt = 1.0 - t;
                        cur.push(Pt {
                            x: mt * mt * mt * cx
                                + 3.0 * mt * mt * t * x1
                                + 3.0 * mt * t * t * x2
                                + t * t * t * x,
                            y: mt * mt * mt * cy
                                + 3.0 * mt * mt * t * y1
                                + 3.0 * mt * t * t * y2
                                + t * t * t * y,
                        });
                    }
                    px = x2;
                    py = y2;
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'S' => {
                    let (x1, y1) = if pcmd == b'C' || pcmd == b'S' {
                        (2.0 * cx - px, 2.0 * cy - py)
                    } else {
                        (cx, cy)
                    };
                    let x2 = args[0] + if rel { cx } else { 0.0 };
                    let y2 = args[1] + if rel { cy } else { 0.0 };
                    (x, y) = (
                        args[2] + if rel { cx } else { 0.0 },
                        args[3] + if rel { cy } else { 0.0 },
                    );
                    for k in 1..=16 {
                        let t = k as f32 / 16.0;
                        let mt = 1.0 - t;
                        cur.push(Pt {
                            x: mt * mt * mt * cx
                                + 3.0 * mt * mt * t * x1
                                + 3.0 * mt * t * t * x2
                                + t * t * t * x,
                            y: mt * mt * mt * cy
                                + 3.0 * mt * mt * t * y1
                                + 3.0 * mt * t * t * y2
                                + t * t * t * y,
                        });
                    }
                    px = x2;
                    py = y2;
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'Q' => {
                    let x1 = args[0] + if rel { cx } else { 0.0 };
                    let y1 = args[1] + if rel { cy } else { 0.0 };
                    (x, y) = (
                        args[2] + if rel { cx } else { 0.0 },
                        args[3] + if rel { cy } else { 0.0 },
                    );
                    for k in 1..=12 {
                        let t = k as f32 / 12.0;
                        let mt = 1.0 - t;
                        cur.push(Pt {
                            x: mt * mt * cx + 2.0 * mt * t * x1 + t * t * x,
                            y: mt * mt * cy + 2.0 * mt * t * y1 + t * t * y,
                        });
                    }
                    px = x1;
                    py = y1;
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'T' => {
                    let (x1, y1) = if pcmd == b'Q' || pcmd == b'T' {
                        (2.0 * cx - px, 2.0 * cy - py)
                    } else {
                        (cx, cy)
                    };
                    for k in 1..=12 {
                        let t = k as f32 / 12.0;
                        let mt = 1.0 - t;
                        cur.push(Pt {
                            x: mt * mt * cx + 2.0 * mt * t * x1 + t * t * x,
                            y: mt * mt * cy + 2.0 * mt * t * y1 + t * t * y,
                        });
                    }
                    px = x1;
                    py = y1;
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                b'A' => {
                    // Yay yaklaşımı: yayın son noktasına doğru parça (basit ama güvenli).
                    cur.push(Pt { x, y });
                    cx = x;
                    cy = y;
                    pcmd = c;
                }
                _ => break,
            }
            // M sonrası tekrarlar L sayılır (SVG kuralı)
            if c == b'M' {
                cmd = if rel { b'l' } else { b'L' };
            }
            // Bir sonraki token komutsa iç döngü biter; sayıysa aynı komut
            // argümanları tekrarlanır.
            if i >= toks.len() || toks[i].0 != 0 {
                break;
            }
        }
    }
    if !cur.is_empty() {
        subs.push(cur);
    }
    subs
}

// ── Rasterizasyon ─────────────────────────────────────────────────

struct Raster<'a> {
    w: usize,
    h: usize,
    px: &'a mut [u8],
}

impl<'a> Raster<'a> {
    fn blend(&mut self, x: i32, y: i32, c: (u8, u8, u8), a: u8) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = ((y as usize) * self.w + x as usize) * 3;
        let af = a as u32;
        let inv = 255 - af;
        self.px[i] = ((c.0 as u32 * af + self.px[i] as u32 * inv + 127) / 255) as u8;
        self.px[i + 1] = ((c.1 as u32 * af + self.px[i + 1] as u32 * inv + 127) / 255) as u8;
        self.px[i + 2] = ((c.2 as u32 * af + self.px[i + 2] as u32 * inv + 127) / 255) as u8;
    }

    /// Even-odd tarama çizgisi ile çokgen doldurma.
    fn fill_poly(&mut self, pts: &[Pt], c: (u8, u8, u8), a: u8) {
        if pts.len() < 3 {
            return;
        }
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for p in pts {
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
        }
        let y0 = (ffloor(min_y) as i32).max(0);
        let y1 = (fceil(max_y) as i32).min(self.h as i32 - 1);
        let mut xs: Vec<f32> = Vec::new();
        for y in y0..=y1 {
            let sy = y as f32 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let p = pts[i];
                let q = pts[(i + 1) % pts.len()];
                if (p.y <= sy && q.y > sy) || (q.y <= sy && p.y > sy) {
                    let t = (sy - p.y) / (q.y - p.y);
                    xs.push(p.x + t * (q.x - p.x));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            for pair in xs.as_chunks::<2>().0 {
                let xa = fceil(pair[0]) as i32;
                let xb = ffloor(pair[1]) as i32;
                for x in xa..=xb {
                    self.blend(x, y, c, a);
                }
            }
        }
        let _ = (min_x, max_x);
    }

    /// Kalın doğru parçası (stroke).
    fn stroke_seg(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, w: f32, c: (u8, u8, u8), a: u8) {
        let half = (w / 2.0).max(0.5);
        let min_x = ffloor(x1.min(x2) - half - 1.0) as i32;
        let max_x = fceil(x1.max(x2) + half + 1.0) as i32;
        let min_y = ffloor(y1.min(y2) - half - 1.0) as i32;
        let max_y = fceil(y1.max(y2) + half + 1.0) as i32;
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len2 = dx * dx + dy * dy;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let t = if len2 > 0.0 {
                    (((px - x1) * dx + (py - y1) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let ddx = px - (x1 + t * dx);
                let ddy = py - (y1 + t * dy);
                if fsqrt(ddx * ddx + ddy * ddy) <= half {
                    self.blend(x, y, c, a);
                }
            }
        }
    }

    fn stroke_poly(&mut self, pts: &[Pt], w: f32, c: (u8, u8, u8), a: u8, closed: bool) {
        if pts.len() < 2 {
            return;
        }
        let n = pts.len() - if closed { 0 } else { 1 };
        for i in 0..n {
            let p = pts[i];
            let q = pts[(i + 1) % pts.len()];
            self.stroke_seg(p.x, p.y, q.x, q.y, w, c, a);
        }
    }
}

// ── Belge işleme ──────────────────────────────────────────────────

/// `<svg ...>...</svg>` parçasını (açılış ve kapanış etiketleri dahil) rasterize eder.
/// Dönen değer: (genişlik, yükseklik, RGB pikseller).
pub fn render_svg(src: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut p = SvgParser {
        src,
        pos: 0,
        width: 0,
        height: 0,
        mats: Vec::new(),
        shapes: Vec::new(),
        skip: 0,
    };
    // İlk <svg> açılış etiketini bul
    let svg_pos = find_tag(src, b"svg", false)?;
    p.pos = svg_pos;
    let (name, attrs_raw, self_close, next) = p.next_tag()?;
    if !name.eq_ignore_ascii_case("svg") {
        return None;
    }
    let attrs = parse_attrs(attrs_raw);
    p.width = {
        let w = attr(&attrs, "width")
            .map(|s| len_or_pct(s, 300.0, 300.0))
            .unwrap_or(300.0);
        (fround(w) as usize).clamp(1, 1024)
    };
    p.height = {
        let h = attr(&attrs, "height")
            .map(|s| len_or_pct(s, 150.0, 150.0))
            .unwrap_or(150.0);
        (fround(h) as usize).clamp(1, 1024)
    };
    let mut root = Mat::identity();
    if let Some(vb) = attr(&attrs, "viewBox") {
        let n = parse_num_list(vb);
        if n.len() >= 4 {
            root = viewbox_mat(n[0], n[1], n[2], n[3], p.width, p.height);
        }
    }
    p.mats.push(root);
    p.pos = next;
    if self_close {
        // Boş svg — beyaz bitmap döndür
        let buf = alloc::vec![255u8; p.width * p.height * 3];
        return Some((p.width, p.height, buf));
    }
    p.parse_body(0);

    // Rasterize — şekil noktaları üretim anında CTM ile cihaz uzayına
    // taşındığı için burada doğrudan çizilir.
    let mut buf = alloc::vec![255u8; p.width * p.height * 3];
    {
        let mut ras = Raster {
            w: p.width,
            h: p.height,
            px: &mut buf,
        };
        raster_shapes(&p.shapes, &mut ras);
    }
    Some((p.width, p.height, buf))
}

fn raster_shapes(shapes: &[Shape], ras: &mut Raster) {
    // Shape içinde CTM'yi saklamak için Shape'a mat alanı eklenmedi;
    // bu fonksiyon shapes'ın zaten cihaz uzayında noktaları içerdiğini varsayar.
    // Aşağıda shapes üretimi bu şekilde yapılır.
    for s in shapes {
        let op = s.opacity.clamp(0.0, 1.0);
        match &s.kind {
            ShapeKind::Poly { pts, closed } => {
                if let Some(fc) = s.fill {
                    let a = (fc.3 as f32 * op) as u8;
                    ras.fill_poly(pts, (fc.0, fc.1, fc.2), a);
                }
                if let Some(sc) = s.stroke {
                    let a = (sc.3 as f32 * op) as u8;
                    ras.stroke_poly(pts, s.stroke_w.max(0.1), (sc.0, sc.1, sc.2), a, *closed);
                }
            }
        }
    }
}

/// `<svg` konumunu bulur (büyük/küçük harf duyarsız).
fn find_tag(src: &[u8], name: &[u8], closing: bool) -> Option<usize> {
    let pat: Vec<u8> = if closing {
        let mut v = Vec::with_capacity(name.len() + 3);
        v.extend_from_slice(b"</");
        v.extend_from_slice(name);
        v
    } else {
        let mut v = Vec::with_capacity(name.len() + 2);
        v.push(b'<');
        v.extend_from_slice(name);
        v
    };
    src.windows(pat.len())
        .position(|w| w.eq_ignore_ascii_case(&pat))
}

/// Bir sonraki etiketi ayrıştırır: (ad, öznitelik baytları, kendinden kapanır, sonraki konum).
fn parse_tag_at(src: &[u8], lt: usize) -> Option<(String, &[u8], bool, usize)> {
    if lt >= src.len() || src[lt] != b'<' {
        return None;
    }
    let mut i = lt + 1;
    let mut closing = false;
    if i < src.len() && src[i] == b'/' {
        closing = true;
        i += 1;
    }
    let ns = i;
    while i < src.len() && (src[i].is_ascii_alphabetic()) {
        i += 1;
    }
    if i == ns {
        return None;
    }
    let name = String::from(core::str::from_utf8(&src[ns..i]).unwrap_or(""));
    // '>' bul (tırnak içi '>' atlanır)
    let mut quote = 0u8;
    let mut gt = i;
    while gt < src.len() {
        let c = src[gt];
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'>' {
            break;
        }
        gt += 1;
    }
    if gt >= src.len() {
        return None;
    }
    let attrs_end = gt;
    // Kendinden kapanır: '>' öncesi son anlamlı bayt '/'
    let mut j = attrs_end;
    let mut self_close = false;
    while j > i {
        j -= 1;
        if src[j].is_ascii_whitespace() {
            continue;
        }
        if src[j] == b'/' {
            self_close = true;
        }
        break;
    }
    Some((
        if closing { format!("/{}", name) } else { name },
        &src[i..attrs_end],
        self_close && !closing,
        gt + 1,
    ))
}

impl<'a> SvgParser<'a> {
    /// src içinde lt konumundaki '<' için sonraki etiketi döndürür.
    fn next_tag(&mut self) -> Option<(String, &'a [u8], bool, usize)> {
        // Yorum/declarations atla
        loop {
            let lt = self.src[self.pos..]
                .iter()
                .position(|&b| b == b'<')
                .map(|o| self.pos + o)?;
            if self.src[lt..].starts_with(b"<!--") {
                if let Some(e) = find_bytes(&self.src[lt + 4..], b"-->") {
                    self.pos = lt + 4 + e + 3;
                    continue;
                }
                return None;
            }
            if self.src[lt..].starts_with(b"<!") || self.src[lt..].starts_with(b"<?") {
                if let Some(e) = self.src[lt..].iter().position(|&b| b == b'>') {
                    self.pos = lt + e + 1;
                    continue;
                }
                return None;
            }
            self.pos = lt;
            return parse_tag_at(self.src, lt);
        }
    }

    fn parse_body(&mut self, depth: usize) {
        let _ = depth;
        while self.pos < self.src.len() {
            let before = self.pos;
            let Some((name, attrs_raw, self_close, next)) = self.next_tag() else {
                break;
            };
            let lname = name.trim_start_matches('/').to_ascii_lowercase();
            if lname == "svg" && name.starts_with('/') {
                // Kök </svg> — işimiz bitti (iç içe svg'ler matrisle yönetilir,
                // bu yüzden yalnızca derinlik 0 kapanışı durdurur; basitlik için
                // ilk kapanışı da durdurmayız çünkü kök parse_body'yi çağıran
                // zaten kök seviyesinde. İç svg'ler grup gibi ele alınır.)
                // Kök kapanışını tanımak için: depth==0'da svg kapanışı sonlandırır.
                // Burada her </svg> kök kapanışı sayılır — iç içe svg destklenmez.
                break;
            }
            let closing = name.starts_with('/');
            let attrs = parse_attrs(attrs_raw);
            if self.skip > 0 {
                if closing {
                    // skip konteynerinin kapanışı — hangi etiket olduğunu izlemeyiz;
                    // basit yaklaşım: skip yalnızca tek etiketli konteynerler için.
                    self.skip -= 1;
                } else if !self_close {
                    self.skip += 1;
                }
                self.pos = next;
                continue;
            }
            if closing {
                // </g> vb. — matris pop
                if lname == "g" || lname == "svg" || lname == "a" || lname == "switch" {
                    self.mats.pop();
                }
                self.pos = next;
                continue;
            }
            match lname.as_str() {
                "defs" | "title" | "desc" | "metadata" | "script" | "style" | "text" | "image"
                | "use" | "symbol" | "marker" | "pattern" | "clipPath" | "mask" | "filter"
                | "linearGradient" | "radialGradient" | "stop" => {
                    if !self_close {
                        self.skip = 1;
                    }
                }
                "g" | "a" | "switch" => {
                    let mut m = *self.mats.last().unwrap_or(&Mat::identity());
                    if let Some(t) = attr(&attrs, "transform") {
                        m = m.mul(parse_transform(t));
                    }
                    self.mats.push(m);
                }
                "svg" => {
                    // İç içe svg: viewBox + x/y varsa viewport dönüşümü uygula.
                    let mut m = *self.mats.last().unwrap_or(&Mat::identity());
                    if let Some(t) = attr(&attrs, "transform") {
                        m = m.mul(parse_transform(t));
                    }
                    let x = attr(&attrs, "x").map(parse_num).unwrap_or(0.0);
                    let y = attr(&attrs, "y").map(parse_num).unwrap_or(0.0);
                    if x != 0.0 || y != 0.0 {
                        m = m.mul(Mat {
                            a: 1.0,
                            b: 0.0,
                            c: 0.0,
                            d: 1.0,
                            e: x,
                            f: y,
                        });
                    }
                    if let Some(vb) = attr(&attrs, "viewBox") {
                        let n = parse_num_list(vb);
                        if n.len() >= 4 {
                            // İç svg boyutu: width/height attrs veya %100
                            let w = attr(&attrs, "width")
                                .map(|s| len_or_pct(s, self.width as f32, self.width as f32))
                                .unwrap_or(self.width as f32)
                                as usize;
                            let h = attr(&attrs, "height")
                                .map(|s| len_or_pct(s, self.height as f32, self.height as f32))
                                .unwrap_or(self.height as f32)
                                as usize;
                            m = m.mul(viewbox_mat(n[0], n[1], n[2], n[3], w.max(1), h.max(1)));
                        }
                    }
                    self.mats.push(m);
                }
                "rect" => {
                    let x = attr(&attrs, "x").map(parse_num).unwrap_or(0.0);
                    let y = attr(&attrs, "y").map(parse_num).unwrap_or(0.0);
                    let w = attr(&attrs, "width").map(parse_num).unwrap_or(0.0);
                    let h = attr(&attrs, "height").map(parse_num).unwrap_or(0.0);
                    if w > 0.0 && h > 0.0 {
                        let pts = [
                            Pt { x, y },
                            Pt { x: x + w, y },
                            Pt { x: x + w, y: y + h },
                            Pt { x, y: y + h },
                        ];
                        self.push_shape(
                            ShapeKind::Poly {
                                pts: self.xform_pts(&pts),
                                closed: true,
                            },
                            &attrs,
                        );
                    }
                }
                "circle" => {
                    let cx = attr(&attrs, "cx").map(parse_num).unwrap_or(0.0);
                    let cy = attr(&attrs, "cy").map(parse_num).unwrap_or(0.0);
                    let r = attr(&attrs, "r").map(parse_num).unwrap_or(0.0);
                    if r > 0.0 {
                        // Daireyi CTM ile elipse dönüştürmek için örnekle:
                        // basit yaklaşım — 48 nokta, CTM uygulanır.
                        let mut pts: Vec<Pt> = Vec::with_capacity(48);
                        for k in 0..48 {
                            let t = k as f32 / 48.0 * core::f32::consts::TAU;
                            pts.push(Pt {
                                x: cx + r * fcos(t),
                                y: cy + r * fsin(t),
                            });
                        }
                        self.push_shape(
                            ShapeKind::Poly {
                                pts: self.xform_pts(&pts),
                                closed: true,
                            },
                            &attrs,
                        );
                    }
                }
                "ellipse" => {
                    let cx = attr(&attrs, "cx").map(parse_num).unwrap_or(0.0);
                    let cy = attr(&attrs, "cy").map(parse_num).unwrap_or(0.0);
                    let rx = attr(&attrs, "rx").map(parse_num).unwrap_or(0.0);
                    let ry = attr(&attrs, "ry").map(parse_num).unwrap_or(0.0);
                    if rx > 0.0 && ry > 0.0 {
                        // CTM altında elips → örneklenmiş poligon (dönüşüm güvenli)
                        let mut pts: Vec<Pt> = Vec::with_capacity(48);
                        for k in 0..48 {
                            let t = k as f32 / 48.0 * core::f32::consts::TAU;
                            pts.push(Pt {
                                x: cx + rx * fcos(t),
                                y: cy + ry * fsin(t),
                            });
                        }
                        self.push_shape(
                            ShapeKind::Poly {
                                pts: self.xform_pts(&pts),
                                closed: true,
                            },
                            &attrs,
                        );
                    }
                }
                "line" => {
                    let x1 = attr(&attrs, "x1").map(parse_num).unwrap_or(0.0);
                    let y1 = attr(&attrs, "y1").map(parse_num).unwrap_or(0.0);
                    let x2 = attr(&attrs, "x2").map(parse_num).unwrap_or(0.0);
                    let y2 = attr(&attrs, "y2").map(parse_num).unwrap_or(0.0);
                    let pts = [Pt { x: x1, y: y1 }, Pt { x: x2, y: y2 }];
                    self.push_shape(
                        ShapeKind::Poly {
                            pts: self.xform_pts(&pts),
                            closed: false,
                        },
                        &attrs,
                    );
                }
                "polyline" | "polygon" => {
                    if let Some(pts_s) = attr(&attrs, "points") {
                        let nums = parse_num_list(pts_s);
                        let mut pts: Vec<Pt> = Vec::new();
                        for pair in nums.as_chunks::<2>().0 {
                            pts.push(Pt {
                                x: pair[0],
                                y: pair[1],
                            });
                        }
                        if pts.len() >= 2 {
                            self.push_shape(
                                ShapeKind::Poly {
                                    pts: self.xform_pts(&pts),
                                    closed: lname == "polygon",
                                },
                                &attrs,
                            );
                        }
                    }
                }
                "path" => {
                    if let Some(d) = attr(&attrs, "d") {
                        let subs = flatten_path(d);
                        // Tüm alt yolları tek shape olarak birleştir (fill even-odd
                        // her alt yolu ayrı doldurur — delikler için yeterli yaklaşım).
                        for sub in subs {
                            if sub.len() >= 2 {
                                let closed = sub
                                    .first()
                                    .map(|f| {
                                        (f.x - sub.last().unwrap().x).abs() < 0.01
                                            && (f.y - sub.last().unwrap().y).abs() < 0.01
                                    })
                                    .unwrap_or(false);
                                self.push_shape(
                                    ShapeKind::Poly {
                                        pts: self.xform_pts(&sub),
                                        closed,
                                    },
                                    &attrs,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
            self.pos = next;
            if before == self.pos {
                break;
            }
        }
    }

    fn xform_pts(&self, pts: &[Pt]) -> Vec<Pt> {
        let m = self.mats.last().copied().unwrap_or(Mat::identity());
        pts.iter()
            .map(|p| {
                let (x, y) = m.apply(p.x, p.y);
                Pt { x, y }
            })
            .collect()
    }

    fn push_shape(&mut self, kind: ShapeKind, attrs: &[(String, String)]) {
        // `transform` özniteliği şekle de uygulanabilir (grup dışında)
        let mut pts_kind = kind;
        if let Some(t) = attr(attrs, "transform") {
            let tm = parse_transform(t);
            // Shape zaten xform_pts ile cihaz uzayında — transform'u noktalar
            // üretiminde uygulamak için burada tekrar uygulamak gerekir; bu
            // yüzden transform'lu şekiller için kind'ı yeniden üretmek yerine
            // nokta listesine doğrudan uyguluyoruz.
            let ShapeKind::Poly { pts, closed } = pts_kind;
            pts_kind = ShapeKind::Poly {
                pts: pts
                    .iter()
                    .map(|p| {
                        let (x, y) = tm.apply(p.x, p.y);
                        Pt { x, y }
                    })
                    .collect(),
                closed,
            };
        }
        let op = attr(attrs, "opacity").map(parse_num).unwrap_or(1.0);
        let fill = paint(attrs, "fill")
            .and_then(|s| parse_color(&s))
            .map(|(r, g, b)| {
                let a = paint(attrs, "fill-opacity")
                    .map(|s| parse_num(&s))
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0);
                (r, g, b, (a * 255.0) as u8)
            });
        let stroke = paint(attrs, "stroke")
            .and_then(|s| parse_color(&s))
            .map(|(r, g, b)| {
                let a = paint(attrs, "stroke-opacity")
                    .map(|s| parse_num(&s))
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0);
                (r, g, b, (a * 255.0) as u8)
            });
        let stroke_w = paint(attrs, "stroke-width")
            .or_else(|| attr(attrs, "stroke-width").map(String::from))
            .map(|s| parse_num(&s))
            .unwrap_or(1.0);
        // SVG varsayılanı: fill siyah, stroke yok
        let fill = if fill.is_none() && paint(attrs, "fill").is_none() {
            // fill belirtilmemiş → siyah
            let a = paint(attrs, "fill-opacity")
                .map(|s| parse_num(&s))
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            Some((0u8, 0u8, 0u8, (a * 255.0) as u8))
        } else {
            fill
        };
        self.shapes.push(Shape {
            fill,
            stroke,
            stroke_w,
            opacity: op,
            kind: pts_kind,
        });
    }
}

fn find_bytes(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    h.windows(n.len()).position(|w| w == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_rect_renders_pixels() {
        let src = b"<svg width='10' height='10'><rect x='2' y='2' width='4' height='4' fill='#ff0000'/></svg>";
        let (w, h, px) = render_svg(src).unwrap();
        assert_eq!((w, h), (10, 10));
        // (4,4) kırmızı olmalı
        let i = (4 * w + 4) * 3;
        assert_eq!(&px[i..i + 3], &[255, 0, 0]);
        // (0,0) beyaz (arka plan)
        assert_eq!(&px[0..3], &[255, 255, 255]);
    }

    #[test]
    fn svg_circle_and_stroke() {
        let src = b"<svg width='20' height='20' viewBox='0 0 20 20'><circle cx='10' cy='10' r='8' fill='none' stroke='#00ff00' stroke-width='2'/></svg>";
        let (w, h, px) = render_svg(src).unwrap();
        assert_eq!((w, h), (20, 20));
        // Merkez dolu değil (fill none), kenar yeşil
        let ci = (10 * w + 10) * 3;
        assert_eq!(&px[ci..ci + 3], &[255, 255, 255]);
        let ei = (10 * w + 18) * 3; // sağ kenar
        assert_eq!(&px[ei..ei + 3], &[0, 255, 0]);
    }

    #[test]
    fn svg_transform_translate() {
        let src = b"<svg width='10' height='10'><g transform='translate(5,0)'><rect x='0' y='0' width='2' height='2' fill='#0000ff'/></g></svg>";
        let (w, _, px) = render_svg(src).unwrap();
        let at5 = (1 * w + 6) * 3;
        assert_eq!(&px[at5..at5 + 3], &[0, 0, 255]);
        let at1 = (1 * w + 1) * 3;
        assert_eq!(&px[at1..at1 + 3], &[255, 255, 255]);
    }

    #[test]
    fn svg_path_polygon() {
        let src =
            b"<svg width='10' height='10'><path d='M1,1 L8,1 L8,8 L1,8 Z' fill='#123456'/></svg>";
        let (w, _, px) = render_svg(src).unwrap();
        let i = (4 * w + 4) * 3;
        assert_eq!(&px[i..i + 3], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn svg_defs_skipped() {
        let src = b"<svg width='10' height='10'><defs><rect x='0' y='0' width='9' height='9' fill='#ff0000'/></defs><rect x='0' y='0' width='2' height='2' fill='#00ff00'/></svg>";
        let (_, _, px) = render_svg(src).unwrap();
        let i = (5 * 10 + 5) * 3;
        // defs'teki kırmızı render edilmemeli → beyaz
        assert_eq!(&px[i..i + 3], &[255, 255, 255]);
    }
}

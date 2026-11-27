//! CSS ayrıştırıcı ve stil eşleştirici (no_std).
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use super::types::{html_color, BackgroundSize, ElementStyle, GridTrack, TextAlign};

/// Tek bir CSS bildirimi: özellik adı, değer ve önem (`!important`) bayrağı.
///
/// Bildirimler kısa gösterimlerden longhand'lara açıldıktan sonra saklanır;
/// böylece cascade özellik bazlı (per-property) hesaplanabilir.
#[derive(Clone, Debug)]
pub struct CssDecl {
    /// Longhand özellik adı (ör. "margin-top").
    pub prop: String,
    /// Değer metni (`!important` işareti temizlenmiş).
    pub value: String,
    /// `!important` işareti taşıyor mu?
    pub important: bool,
}

#[derive(Clone)]
pub struct CssRule {
    pub selector: String,
    /// Kuralın birleşik stili; cascade artık `decls` üzerinden özellik bazlı
    /// hesaplandığından bu alan geriye dönük uyum için doldurulur, okunmaz.
    #[allow(dead_code)]
    pub style: ElementStyle,
    /// İsteğe bağlı @media koşulu (örnek: "screen and (min-width: 600px)").
    /// None ise kural her zaman etkindir.
    #[allow(dead_code)]
    pub media: Option<String>,
    /// Kuralın bildirim listesi (özellik bazlı cascade için).
    pub decls: Vec<CssDecl>,
    /// Katman sırası (`@layer`). `None` = katmansız (unlayered) kural.
    pub layer: Option<usize>,
}

/// Tek bir @keyframes adımı: yüzde konumu ve stili.
/// `percent`, zaman tabanlı animasyon eklenene kadar okunmaz (statik render
/// ilk kareyi uygular); bu yüzden dead_code bastırılır.
/// A single @keyframes step: percent position and style. `percent` is not read
/// until time-based animation lands (static render applies the first frame),
/// hence the dead_code allowance.
#[derive(Clone)]
pub struct Keyframe {
    #[allow(dead_code)]
    pub percent: f32,
    pub style: ElementStyle,
}

/// İsimli @keyframes kümesi (isim + kare listesi).
/// Named @keyframes set (name + frame list).
#[derive(Clone)]
pub struct Keyframes {
    pub name: String,
    pub frames: Vec<Keyframe>,
}

/// @font-face kuralı: font-family adı ve uzak kaynağı (URL).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FontFace {
    pub font_family: String,
    pub src_url: Option<String>,
}

/// @font-face kuralları deposu (global, tek iş parçacığı).
pub static mut FONT_FACES: alloc::vec::Vec<FontFace> = alloc::vec::Vec::new();

/// Yeni bir @font-face kuralı ekler.
#[allow(static_mut_refs)]
pub fn register_font_face(face: FontFace) {
    unsafe {
        FONT_FACES.push(face);
    }
}

/// Belirtilen font-family için kayıtlı @font-face kaynağını döndürür.
#[allow(dead_code, static_mut_refs)]
pub fn find_font_face(font_family: &str) -> Option<String> {
    unsafe {
        FONT_FACES
            .iter()
            .find(|f| f.font_family == font_family)
            .and_then(|f| f.src_url.clone())
    }
}

fn strip_css_comments(css: &str) -> String {
    let b = css.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut p = 0;
    while p < b.len() {
        if p + 1 < b.len() && b[p] == b'/' && b[p + 1] == b'*' {
            match b[p + 2..].windows(2).position(|w| w == b"*/") {
                Some(rel) => {
                    p = p + 2 + rel + 2;
                    continue;
                }
                None => break,
            }
        }
        out.push(b[p]);
        p += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// CSS özel değişkenlerini (`--ad: değer`) toplar.
///
/// Gerçek CSS'te değişkenler seçici kapsamına göre miras alınır; bu basit
/// motor tüm sayfa için geçerli tek bir tablo tutar. `:root` ve `body`
/// değişkenleri pratikte en yaygın kullanım olduğu için bu yaklaşım renk
/// destekli sitelerin büyük bölümünü çalıştırır.
fn collect_css_variables(css: &str, vars: &mut Vec<(String, String)>) {
    let b = css.as_bytes();
    let mut p = 0usize;
    while p + 1 < b.len() {
        if b[p] == b'-' && b[p + 1] == b'-' {
            let name_start = p;
            p += 2;
            while p < b.len() && (b[p].is_ascii_alphanumeric() || b[p] == b'-' || b[p] == b'_') {
                p += 1;
            }
            let name = &css[name_start..p];
            let mut q = p;
            while q < b.len() && b[q].is_ascii_whitespace() {
                q += 1;
            }
            if q < b.len() && b[q] == b':' {
                q += 1;
                let val_start = q;
                let mut depth = 0i32;
                while q < b.len() {
                    match b[q] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        b';' | b'}' if depth <= 0 => break,
                        _ => {}
                    }
                    q += 1;
                }
                let val = css[val_start..q].trim();
                if !name.is_empty() && !val.is_empty() && val.len() <= 256 {
                    if let Some(entry) = vars.iter_mut().find(|(n, _)| n == name) {
                        entry.1 = String::from(val);
                    } else if vars.len() < 256 {
                        vars.push((String::from(name), String::from(val)));
                    }
                }
                p = q;
                continue;
            }
        }
        p += 1;
    }
}

/// `var(--ad[, yedek])` çağrılarını toplanan değerlerle değiştirir.
fn substitute_css_variables(css: &str, vars: &[(String, String)]) -> String {
    let mut current = String::from(css);
    for _ in 0..4 {
        let bytes = current.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        let mut changed = false;
        while i < bytes.len() {
            if i + 4 <= bytes.len() && bytes[i..i + 4].eq_ignore_ascii_case(b"var(") {
                let mut depth = 1i32;
                let mut j = i + 4;
                while j < bytes.len() && depth > 0 {
                    if bytes[j] == b'(' {
                        depth += 1;
                    } else if bytes[j] == b')' {
                        depth -= 1;
                    }
                    j += 1;
                }
                if depth == 0 {
                    let inner = &current[i + 4..j - 1];
                    let (name, fallback) = match inner.find(',') {
                        Some(pos) => (inner[..pos].trim(), Some(inner[pos + 1..].trim())),
                        None => (inner.trim(), None),
                    };
                    if name.starts_with("--") {
                        if let Some((_, value)) = vars.iter().find(|(n, _)| n == name) {
                            out.extend_from_slice(value.as_bytes());
                            changed = true;
                            i = j;
                            continue;
                        }
                        if let Some(value) = fallback {
                            out.extend_from_slice(value.as_bytes());
                            changed = true;
                            i = j;
                            continue;
                        }
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        current = String::from_utf8(out)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
        if !changed {
            break;
        }
    }
    current
}

/// Kısa gösterimdeki ilk geçerli rengi bulur (ör. `1px solid #f00`).
fn shorthand_color(value: &str) -> Option<u32> {
    html_color(value.as_bytes())
}

/// `border: 1px solid #f00` kısa gösterimini çözer.
fn apply_border_shorthand(style: &mut ElementStyle, value: &str) {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("hidden") {
        style.border_width = 0;
        style.border_style = 0;
        return;
    }
    for part in v.split_whitespace() {
        if part.eq_ignore_ascii_case("solid") {
            style.border_style = 1;
        } else if part.eq_ignore_ascii_case("dashed") {
            style.border_style = 2;
        } else if part.eq_ignore_ascii_case("dotted") {
            style.border_style = 3;
        } else if part.eq_ignore_ascii_case("double") {
            style.border_style = 1;
        } else if let Some(px) = parse_px_value(part, None, None, None, None) {
            style.border_width = px;
        }
    }
    if let Some(c) = shorthand_color(v) {
        style.border_color = Some(c);
    }
}

/// Piksel, em, rem, %, vh, vw birimlerini piksel cinsinden çözer.
///
/// # Arguments
///
/// * `val` - Değer dizgesi (örnek: "16px", "2rem", "50%", "100vh")
/// * `viewport_width` - Görünüm alanı genişliği piksel cinsinden (opsiyonel, `None` için varsayılan kullanılır)
/// * `viewport_height` - Görünüm alanı yüksekliği piksel cinsinden (opsiyonel, `None` için varsayılan kullanılır)
/// * `parent_width` - Vanem elementi genişliği yüzdesi için hesaplanacak genişlik (opsiyonel, `None` için %100 varsayılır)
/// * `parent_height` - Vanem elementi yüksekliği yüzdesi için hesaplanacak yükseklik (opsiyonel, `None` için %100 varsayılır)
fn parse_px_value(
    val: &str,
    viewport_width: Option<usize>,
    viewport_height: Option<usize>,
    parent_width: Option<usize>,
    parent_height: Option<usize>,
) -> Option<usize> {
    let t = val.trim().to_ascii_lowercase();
    let (num_str, unit) = if let Some(n) = t.strip_suffix("px") {
        (n, "px")
    } else if let Some(n) = t.strip_suffix("em") {
        (n, "em")
    } else if let Some(n) = t.strip_suffix("rem") {
        (n, "rem")
    } else if let Some(n) = t.strip_suffix("%") {
        (n, "%")
    } else if let Some(n) = t.strip_suffix("vh") {
        (n, "vh")
    } else if let Some(n) = t.strip_suffix("vw") {
        (n, "vw")
    } else if let Some(n) = t.strip_suffix("pt") {
        (n, "pt")
    } else {
        (t.as_str(), "")
    };
    let n: f32 = num_str.trim().parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }

    match unit {
        "px" => Some(n as usize),
        "em" => Some((n * 16.0f32) as usize), // 1em = 16px varsayımı
        "rem" => Some((n * 16.0f32) as usize), // 1rem = 16px varsayımı
        "%" => {
            let parent_w = parent_width.unwrap_or(100);
            let _parent_h = parent_height.unwrap_or(100);
            // Yüzde değeri parent genişliğine/güksekliğine göre hesapla
            // Bu fonksiyon çağrısında parent_boyutları sağlanmazsa None döner
            if parent_w > 0 {
                Some((n * parent_w as f32 / 100.0) as usize)
            } else {
                None
            }
        }
        "vh" => {
            let h = viewport_height.unwrap_or(768); // Varsayılan: 768p height
            Some((n * h as f32 / 100.0) as usize)
        }
        "vw" => {
            let w = viewport_width.unwrap_or(1024); // Varsayılan: 1024p width
            Some((n * w as f32 / 100.0) as usize)
        }
        "pt" => Some((n * 1.25f32) as usize), // 1pt ≈ 1.25px
        // Birimsiz sıfır geçerli bir uzunluktur (`margin-top: 0`).
        "" => {
            if n == 0.0 {
                Some(0)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn font_scale_from_size(val: &str) -> usize {
    let l = val.to_ascii_lowercase();
    if l.contains("xxx-large")
        || l.contains("xx-large")
        || l.contains("x-large")
        || l.contains("large")
    {
        return 2;
    }
    if l.contains("small") {
        return 1;
    }
    // Varsayılan viewport ve parent boyutları ile px değerini hesapla
    match parse_px_value(&l, None, None, None, None) {
        Some(px) if px >= 20 => 2,
        _ => 1,
    }
}

fn is_bold_weight(val: &str) -> bool {
    let l = val.trim().to_ascii_lowercase();
    if l == "bold" || l == "bolder" || l == "bolder " {
        return true;
    }
    l.parse::<u32>().is_ok_and(|w| w >= 600)
}

/// `;` ile ayrılmış bildirimleri böler; `url(data:...;base64,...)` gibi
/// parantez içindeki ayraçları ve tırnaklı metinleri bölmez.
fn split_css_declarations(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut quote = None::<u8>;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => {
                quote = Some(b);
                i += 1;
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            b';' if depth <= 0 => {
                out.push(&input[start..i]);
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    out.push(&input[start..]);
    out
}

/// Bildirim değerinden `!important` işaretini ayırır.
///
/// Dönen değer: (temizlenmiş değer, önem bayrağı).
fn split_important(value: &str) -> (String, bool) {
    let v = value.trim();
    // `!important` ASCII olduğu için son 10 bayt kontrolü yeterlidir; ancak
    // çok baytlı karakterin ortasında bölmemek için `get` kullanılır.
    if let Some(tail) = v.get(v.len().saturating_sub(10)..) {
        if tail.eq_ignore_ascii_case("!important") {
            let head = &v[..v.len() - 10];
            return (String::from(head.trim()), true);
        }
    }
    (String::from(v), false)
}

/// CSS kısa gösterimini longhand (uzun) özelliklere açar.
///
/// Özellik bazlı cascade için kısa gösterimler ayrıştırma anında longhand
/// bildirimlere bölünür; aksi halde `margin: 10px` ile `margin-top: 0`
/// çakışması çözülemez.
fn expand_css_declaration(prop: &str, value: &str) -> Vec<(String, String)> {
    match prop {
        "margin" | "padding" => {
            let parts: Vec<&str> = value.split_whitespace().collect();
            if parts.is_empty() {
                return Vec::new();
            }
            let (top, right, bottom, left) = match parts.len() {
                1 => (parts[0], parts[0], parts[0], parts[0]),
                2 => (parts[0], parts[1], parts[0], parts[1]),
                3 => (parts[0], parts[1], parts[2], parts[1]),
                _ => (parts[0], parts[1], parts[2], parts[3]),
            };
            let prefix = if prop == "margin" {
                "margin"
            } else {
                "padding"
            };
            vec![
                (format!("{}-top", prefix), String::from(top)),
                (format!("{}-right", prefix), String::from(right)),
                (format!("{}-bottom", prefix), String::from(bottom)),
                (format!("{}-left", prefix), String::from(left)),
            ]
        }
        "gap" => vec![
            (String::from("row-gap"), String::from(value.trim())),
            (String::from("column-gap"), String::from(value.trim())),
        ],
        "place-items" | "place-content" | "place-self" => {
            let names: [&str; 2] = match prop {
                "place-items" => ["align-items", "justify-content"],
                "place-content" => ["align-content", "justify-content"],
                _ => ["align-self", "justify-self"],
            };
            let parts: Vec<&str> = value.split_whitespace().collect();
            match parts.len() {
                0 => Vec::new(),
                1 => vec![
                    (String::from(names[0]), String::from(parts[0])),
                    (String::from(names[1]), String::from(parts[0])),
                ],
                _ => vec![
                    (String::from(names[0]), String::from(parts[0])),
                    (String::from(names[1]), String::from(parts[1])),
                ],
            }
        }
        "flex" => {
            let parts: Vec<&str> = value.split_whitespace().collect();
            match parts.len() {
                0 => Vec::new(),
                1 => {
                    let p = parts[0].trim();
                    let (grow, shrink, basis) = if p.eq_ignore_ascii_case("none") {
                        ("0", "0", "auto")
                    } else if p.eq_ignore_ascii_case("auto") {
                        ("1", "1", "auto")
                    } else if p.eq_ignore_ascii_case("initial") {
                        ("0", "1", "auto")
                    } else {
                        (p, "1", "0%")
                    };
                    vec![
                        (String::from("flex-grow"), String::from(grow)),
                        (String::from("flex-shrink"), String::from(shrink)),
                        (String::from("flex-basis"), String::from(basis)),
                    ]
                }
                _ => {
                    let basis = parts.get(2).copied().unwrap_or("0%");
                    vec![
                        (String::from("flex-grow"), String::from(parts[0].trim())),
                        (String::from("flex-shrink"), String::from(parts[1].trim())),
                        (String::from("flex-basis"), String::from(basis.trim())),
                    ]
                }
            }
        }
        _ => vec![(String::from(prop), String::from(value.trim()))],
    }
}

/// Tırnaklı bölümleri ayırır: `"a b" "c d"` → ["a b", "c d"].
fn split_quoted_segments(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in value.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    out.push(core::mem::take(&mut current));
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                }
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `grid-template-areas` değerini satır → sütun → alan adı tablosuna çevirir.
///
/// `"header header" "side main"` → [["header","header"],["side","main"]].
/// `"."` boş hücreyi belirtir; satırlar dikdörtgen olacak şekilde tamamlanır.
fn parse_grid_template_areas(value: &str) -> Vec<Vec<String>> {
    let segments = split_quoted_segments(value);
    let mut rows: Vec<Vec<String>> = Vec::new();
    if segments.is_empty() {
        // Tırnaksız kullanım: tüm değer tek satır sayılır.
        let cells: Vec<String> = value.split_whitespace().map(String::from).collect();
        if !cells.is_empty() {
            rows.push(cells);
        }
    } else {
        for seg in segments {
            let cells: Vec<String> = seg.split_whitespace().map(String::from).collect();
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
    }
    // Satırları aynı genişliğe getir (eksik hücreler boş).
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for row in rows.iter_mut() {
        while row.len() < width {
            row.push(String::from("."));
        }
    }
    rows
}

/// `grid-template-areas` tablosundaki adlandırılmış bir alanın hücre aralığı.
///
/// Satır ve sütun indeksleri 0 tabanlıdır; aralıklar iki uç dahildir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridAreaRect {
    /// Alan adı (`.` ve boş adlar hariç tutulur).
    pub name: String,
    /// Başlangıç satırı.
    pub row_start: usize,
    /// Başlangıç sütunu.
    pub col_start: usize,
    /// Bitiş satırı (dahil).
    pub row_end: usize,
    /// Bitiş sütunu (dahil).
    pub col_end: usize,
}

/// Grid'e yerleştirilen tek bir öğenin hücresi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSlot {
    /// Satır indeksi.
    pub row: usize,
    /// Sütun indeksi (adlandırılmış alanda alanın başlangıç sütunu).
    pub col: usize,
    /// Öğenin kapladığı sütun sayısı (tek hücrede 1).
    pub col_span: usize,
}

/// CSS Grid yerleşim hesabı: `grid-template-areas` alanları + otomatik akış.
///
/// Saf bir durum makinesidir; `html.rs` metin-akışı çizicisi hücre başına
/// metin yazarken bu hesabı kullanır.
///
/// Kurallar:
/// - `grid-area` adı şablonda varsa öğe o alanın dikdörtgenine yerleşir.
/// - Aynı alan adını taşıyan ikinci öğe otomatik akışa düşer.
/// - Şablonda olmayan alan adı da otomatik akışa düşer.
/// - Şablon hücrelerinin tamamı (adlandırılmış alanlar ve `.` boş hücreler)
///   otomatik akışa kapalıdır; otomatik akış yalnızca şablon dışı hücrelere
///   (örtülü sütunlar / örtük satırlar) yerleşir. Aynı alan adını taşıyan
///   ikinci öğe de bu yüzden şablonun altındaki satıra iner.
pub struct GridPlacer {
    /// Şablondaki adlandırılmış alanların dikdörtgenleri.
    rects: Vec<GridAreaRect>,
    /// Hücrelerin rezerve olup olmadığı (satır → sütun).
    reserved: Vec<Vec<bool>>,
    /// Izgaranın açık sütun sayısı (şablondan geniş olabilir).
    col_count: usize,
    /// Otomatik akış imleci (satır).
    auto_row: usize,
    /// Otomatik akış imleci (sütun).
    auto_col: usize,
    /// Kullanılmış alan adları.
    claimed: Vec<String>,
    /// Yerleştirilen alanların dikdörtgenleri (render için).
    placed: Vec<GridAreaRect>,
}

impl GridPlacer {
    /// Yeni bir grid yerleşim durumu başlatır.
    ///
    /// `template`: `grid-template-areas` tablosu (satır → hücre adları).
    /// `col_count`: ızgaranın açık sütun sayısı.
    pub fn new(template: &[Vec<String>], col_count: usize) -> Self {
        let mut rects: Vec<GridAreaRect> = Vec::new();
        let mut template_rows = 0usize;
        let mut template_cols = 0usize;
        for (r, row) in template.iter().enumerate() {
            template_rows = template_rows.max(r + 1);
            template_cols = template_cols.max(row.len());
            for (c, cell) in row.iter().enumerate() {
                let name = cell.trim();
                if name.is_empty() || name == "." {
                    continue;
                }
                match rects.iter_mut().find(|x| x.name == name) {
                    Some(existing) => {
                        // Alan birden çok hücreye yayılıyor: dikdörtgeni büyüt.
                        existing.row_start = existing.row_start.min(r);
                        existing.col_start = existing.col_start.min(c);
                        existing.row_end = existing.row_end.max(r);
                        existing.col_end = existing.col_end.max(c);
                    }
                    None => rects.push(GridAreaRect {
                        name: String::from(name),
                        row_start: r,
                        col_start: c,
                        row_end: r,
                        col_end: c,
                    }),
                }
            }
        }
        // Şablon hücrelerinin tamamı rezervedir (adlandırılmış alanlar ve "."
        // boş hücreler otomatik akışa kapalıdır: auto_row zaten şablonun
        // altından başlar ve sütun, şablondan daha geniş olabilir).
        let mut reserved: Vec<Vec<bool>> = Vec::new();
        for _ in 0..template_rows {
            reserved.push(vec![true; template_cols]);
        }
        GridPlacer {
            rects,
            reserved,
            col_count: col_count.max(1),
            auto_row: template_rows,
            auto_col: 0,
            claimed: Vec::new(),
            placed: Vec::new(),
        }
    }

    /// Bir grid çocuğunu yerleştirir ve hücresini döner.
    ///
    /// `area`: çocuğun `grid-area` değeri (alan adı). Şablonda karşılığı yoksa
    /// veya alan daha önce kullanıldıysa öğe otomatik akışa düşer.
    pub fn place(&mut self, area: Option<&str>) -> GridSlot {
        if let Some(name) = area {
            let name = name.trim();
            if !name.is_empty() && !self.claimed.iter().any(|c| c == name) {
                if let Some(rect) = self.rects.iter().find(|r| r.name == name).cloned() {
                    self.claimed.push(String::from(name));
                    for r in rect.row_start..=rect.row_end {
                        for c in rect.col_start..=rect.col_end {
                            self.mark_occupied(r, c);
                        }
                    }
                    self.placed.push(rect.clone());
                    return GridSlot {
                        row: rect.row_start,
                        col: rect.col_start,
                        col_span: rect.col_end - rect.col_start + 1,
                    };
                }
            }
        }
        // Otomatik akış: sıradaki boş hücre.
        let (row, col) = self.next_auto_cell();
        GridSlot {
            row,
            col,
            col_span: 1,
        }
    }

    /// Otomatik akışta sıradaki boş hücreyi bulur, işaretler ve döner.
    ///
    /// Konteynerın kendi metni (bir grid öğesine ait olmayan) bu yolla
    /// hücrelere yazılır.
    pub fn next_auto_cell(&mut self) -> (usize, usize) {
        let (row, col) = self.next_free_cell();
        self.mark_occupied(row, col);
        self.auto_col = col + 1;
        if self.auto_col >= self.col_count {
            self.auto_col = 0;
            self.auto_row = row + 1;
        }
        (row, col)
    }

    /// Yerleştirilen alanların dikdörtgenleri (render için).
    pub fn placed_rects(&self) -> &[GridAreaRect] {
        &self.placed
    }

    /// İmleçten sonraki boş hücreyi bulur (rezerve hücreler atlanır).
    fn next_free_cell(&self) -> (usize, usize) {
        let mut row = self.auto_row;
        let mut col = self.auto_col;
        loop {
            if self.is_reserved(row, col) {
                col += 1;
                if col >= self.col_count {
                    col = 0;
                    row += 1;
                }
                continue;
            }
            break;
        }
        (row, col)
    }

    /// Hücre rezerve mi? (şablon hücreleri ve dolu hücreler rezervedir)
    fn is_reserved(&self, row: usize, col: usize) -> bool {
        self.reserved
            .get(row)
            .and_then(|cells| cells.get(col))
            .copied()
            .unwrap_or(false)
    }

    /// Hücreyi dolu (rezerve) olarak işaretler.
    fn mark_occupied(&mut self, row: usize, col: usize) {
        while self.reserved.len() <= row {
            self.reserved.push(Vec::new());
        }
        let cells = &mut self.reserved[row];
        while cells.len() <= col {
            cells.push(false);
        }
        cells[col] = true;
    }
}

/// `url(...)` içindeki ilk adresi döndürür (tırnaklar temizlenir).
fn extract_css_url(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut start = 0usize;
    while start + 4 <= bytes.len() {
        let rel = match bytes[start..].windows(4).position(|w| w == b"url(") {
            Some(pos) => start + pos,
            None => break,
        };
        let inner_start = rel + 4;
        let mut depth = 1i32;
        let mut i = inner_start;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            break;
        }
        let inner = value[inner_start..i - 1].trim();
        let inner = inner
            .strip_prefix('"')
            .or_else(|| inner.strip_prefix('\''))
            .unwrap_or(inner);
        let inner = inner
            .strip_suffix('"')
            .or_else(|| inner.strip_suffix('\''))
            .unwrap_or(inner)
            .trim();
        if !inner.is_empty() {
            return Some(String::from(inner));
        }
        start = i;
    }
    None
}

/// Renk ayrıştırmasını bozmamak için `url(...)` ve gradyan/image-set
/// işlevlerini değer metninden çıkarır.
fn strip_css_image_functions(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &value[i..];
        let lower = rest.to_ascii_lowercase();
        let mut func_len = None;
        for name in [
            "url(",
            "image-set(",
            "linear-gradient(",
            "radial-gradient(",
            "conic-gradient(",
        ] {
            if lower.starts_with(name) {
                func_len = Some(name.len());
                break;
            }
        }
        if let Some(len) = func_len {
            let mut depth = 1i32;
            let mut j = i + len;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            i = j.min(bytes.len());
            out.push(' ');
            continue;
        }
        let ch_len = rest.chars().next().map_or(1, |c| c.len_utf8());
        out.push_str(&rest[..ch_len]);
        i += ch_len;
    }
    out
}

fn parse_background_size(val: &str) -> Option<BackgroundSize> {
    let l = val.trim().to_ascii_lowercase();
    match l.as_str() {
        "cover" => Some(BackgroundSize::Cover),
        "contain" => Some(BackgroundSize::Contain),
        "stretch" | "100%" | "100% 100%" => Some(BackgroundSize::Stretch),
        "auto" => Some(BackgroundSize::Auto),
        _ if l.contains('%') => Some(BackgroundSize::Stretch),
        _ => None,
    }
}

/// `background` kısa gösterimindeki tekrar/boyut anahtarlarını ayıklar.
fn apply_background_tokens(style: &mut ElementStyle, value: &str) {
    let masked = strip_css_image_functions(value);
    for token in masked.split(|c: char| c.is_ascii_whitespace() || c == '/') {
        let t = token.trim().to_ascii_lowercase();
        if t.is_empty() {
            continue;
        }
        match t.as_str() {
            "no-repeat" => style.background_repeat = Some(false),
            "repeat" | "repeat-x" | "repeat-y" | "space" | "round" => {
                style.background_repeat = Some(true)
            }
            _ => {
                // Kısa gösterimde `%` belirteci çoğunlukla konumdur; boyut
                // olarak yalnızca cover/contain/auto/stretch gibi açık
                // anahtarları ve `background-size` özelliği kullanılır.
                if !t.contains('%') {
                    if let Some(size) = parse_background_size(&t) {
                        style.background_size = Some(size);
                    }
                }
            }
        }
    }
}

/// Tek bir `prop: value` bildirimini stile uygular.
#[allow(unreachable_patterns)]
fn apply_css_decl(style: &mut ElementStyle, prop: &str, raw_val: &str) {
    let mut val = raw_val.trim();
    if let Some(tail) = val.get(val.len().saturating_sub(10)..) {
        if tail.eq_ignore_ascii_case("!important") {
            val = val[..val.len() - 10].trim();
        }
    }
    match prop {
        "color" => {
            if !val.eq_ignore_ascii_case("inherit")
                && !val.eq_ignore_ascii_case("initial")
                && !val.eq_ignore_ascii_case("unset")
                && !val.eq_ignore_ascii_case("currentcolor")
            {
                if let Some(c) = html_color(val.as_bytes()) {
                    style.color = Some(c);
                }
            }
        }
        "background-color" => {
            // `transparent` / `none`: arka plan rengi yok.
            if !val.eq_ignore_ascii_case("transparent")
                && !val.eq_ignore_ascii_case("none")
                && !val.eq_ignore_ascii_case("inherit")
            {
                if let Some(c) = html_color(val.as_bytes()) {
                    style.bg_color = Some(c);
                }
            }
        }
        "background" => {
            // Kısa gösterim: renk + url(...) + tekrar/boyut anahtarları.
            // url yoksa shorthand arka plan görselini sıfırlar (CSS cascade).
            if let Some(url) = extract_css_url(val) {
                style.background_image = Some(url);
            } else if !val.eq_ignore_ascii_case("inherit")
                && !val.eq_ignore_ascii_case("initial")
                && !val.eq_ignore_ascii_case("unset")
            {
                style.background_image = Some(String::new());
            }
            style.background_repeat = Some(true);
            style.background_size = Some(BackgroundSize::Auto);
            apply_background_tokens(style, val);
            let color_part = strip_css_image_functions(val);
            if !color_part.trim().eq_ignore_ascii_case("transparent")
                && !color_part.trim().eq_ignore_ascii_case("none")
                && !color_part.trim().eq_ignore_ascii_case("inherit")
            {
                if let Some(c) = html_color(color_part.as_bytes()) {
                    style.bg_color = Some(c);
                }
            }
        }
        "background-image" => {
            if val.eq_ignore_ascii_case("none") {
                style.background_image = Some(String::new());
            } else if let Some(url) = extract_css_url(val) {
                style.background_image = Some(url);
            } else {
                let lv = val.to_ascii_lowercase();
                if lv.contains("gradient(") || lv.contains("image-set(") {
                    // Gradyan/image-set henüz desteklenmiyor; önceki url'i ezme.
                    style.background_image = Some(String::new());
                }
            }
        }
        "background-size" => {
            if let Some(size) = parse_background_size(val) {
                style.background_size = Some(size);
            }
        }
        "background-repeat" => {
            if val.contains("no-repeat") {
                style.background_repeat = Some(false);
            } else if val.contains("repeat") || val.contains("space") || val.contains("round") {
                style.background_repeat = Some(true);
            }
        }
        "text-align" => {
            style.align = if val.eq_ignore_ascii_case("center") {
                TextAlign::Center
            } else if val.eq_ignore_ascii_case("right") {
                TextAlign::Right
            } else {
                TextAlign::Left
            };
        }
        "font-size" => style.font_scale = font_scale_from_size(val),
        "font-family" => {
            // font-family: "CustomFont", sans-serif — ilk isim alınır
            let name = val
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'');
            if !name.is_empty() {
                style.font_family = Some(String::from(name));
            }
        }
        "margin-left" | "padding-left" | "text-indent" => {
            // Varsayılan: viewport ve parent bilgis yok, direkt px/em/rem parse et
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.indent_px = px;
            }
        }
        "margin-top" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.margin_top = px;
            }
        }
        "margin" | "padding" => {
            // Kısa gösterim: soldan itibaren ikinci değer sol kenar boşluğudur.
            let parts: Vec<&str> = val.split_whitespace().collect();
            let left = match parts.len() {
                1 => parts.first().copied(),
                2 => parts.get(1).copied(),
                3 => parts.get(1).copied(),
                _ => parts.get(3).copied(),
            };
            if let Some(v) = left {
                // Margin/padding için varsayılan: tam genişlik/yükseklik hesaplamak
                // için viewport ve parent bilgisi lateri eklenebilir.
                if let Some(px) = parse_px_value(v, None, None, None, None) {
                    style.indent_px = px;
                }
            }
        }
        "font-weight" => style.bold = is_bold_weight(val),
        "text-decoration" | "text-decoration-line" => {
            if val.contains("underline") {
                style.underline = true;
            }
            if val.contains("line-through") || val.contains("overline") {
                style.line_through = true;
            }
        }
        "vertical-align" => {
            if val.eq_ignore_ascii_case("baseline") {
                style.vertical_align = false;
            } else if val.eq_ignore_ascii_case("middle") {
                style.vertical_align = true;
            } else if val.eq_ignore_ascii_case("top") {
                style.vertical_align = false;
            } else if val.eq_ignore_ascii_case("bottom") {
                style.vertical_align = true;
            }
        }
        "line-height" => {
            // Basit bir çözüm: bir sayı veya "normal" kabul et
            if val.eq_ignore_ascii_case("normal") {
                style.line_height = 1.2; // Sabit çarpan
            } else if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.line_height = px as f32 / 16.0; // 16px varsayılan font size
            }
        }
        "cursor" => {
            // Cursor stilleri sadece tanıtım amacı; burada doğrudan etkisi yok
        }
        "white-space" => {
            // white-space stilleri
            if val.eq_ignore_ascii_case("nowrap") {
                style.no_wrap = true;
            } else if val.eq_ignore_ascii_case("pre") {
                style.pre_formatted = true;
            }
        }
        "border" | "outline" => {
            apply_border_shorthand(style, val);
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" | "outline-color" => {
            apply_border_shorthand(style, val);
        }
        "border-width" => {
            // Çoklu değerlerde (`1px 2px`) ilk genişlik kullanılır.
            let first = val.split_whitespace().next().unwrap_or(val);
            if let Some(px) = parse_px_value(first, None, None, None, None) {
                style.border_width = px;
            }
        }
        "border-color" => {
            if let Some(c) = html_color(val.as_bytes()) {
                style.border_color = Some(c);
            }
        }
        "border-style" => {
            // Sınır stili (solid, dashed vb.)
            if val.eq_ignore_ascii_case("solid") {
                style.border_style = 1; // solid
            } else if val.eq_ignore_ascii_case("dashed") {
                style.border_style = 2; // dashed
            } else if val.eq_ignore_ascii_case("dotted") {
                style.border_style = 3; // dotted
            }
        }
        "cursor" => {
            // Cursor stilleri sadece tanıtım amacı; burada doğrudan etkisi yok
            // ancak tanıtıp bilgi tutarız
        }
        "border-radius" => {
            // Sınır radius'i (basit uygulama)
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.border_radius = px;
            }
        }
        "box-shadow" => {
            // Box shadow - basit uygulama, sadece renk ayrıştırılıyor
            if let Some(c) = html_color(val.as_bytes()) {
                style.box_shadow_color = Some(c);
            }
        }
        "text-shadow" => {
            // Text shadow - basit uygulama
            if let Some(c) = html_color(val.as_bytes()) {
                style.text_shadow_color = Some(c);
            }
        }
        "display" => {
            if val.eq_ignore_ascii_case("flex") || val.eq_ignore_ascii_case("inline-flex") {
                style.display = Some(1); // flex modu
            } else if val.eq_ignore_ascii_case("grid") || val.eq_ignore_ascii_case("inline-grid") {
                style.display = Some(2); // grid modu
            } else if val.eq_ignore_ascii_case("block") {
                style.display = Some(3); // block
            } else if val.eq_ignore_ascii_case("inline") {
                style.display = Some(4); // inline
            } else if val.eq_ignore_ascii_case("none") {
                style.display_none = true;
            } else if val.eq_ignore_ascii_case("inline-block") {
                style.display = Some(5); // inline-block
            }
        }
        "position" => {
            if val.eq_ignore_ascii_case("static") {
                style.position = 0; // static
            } else if val.eq_ignore_ascii_case("relative") {
                style.position = 1; // relative
            } else if val.eq_ignore_ascii_case("absolute") {
                style.position = 2; // absolute
            } else if val.eq_ignore_ascii_case("fixed") {
                style.position = 3; // fixed
            } else if val.eq_ignore_ascii_case("sticky") {
                style.position = 4; // sticky
            }
        }
        "z-index" => {
            if let Ok(n) = val.parse::<i32>() {
                style.z_index = n;
            }
        }
        "overflow" => {
            if val.eq_ignore_ascii_case("visible") {
                style.overflow = 0;
            } else if val.eq_ignore_ascii_case("hidden") {
                style.overflow = 1;
            } else if val.eq_ignore_ascii_case("scroll") {
                style.overflow = 2;
            }
        }
        "width" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.width = px;
            }
        }
        "height" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.height = px;
            }
        }
        "max-width" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.max_width = px;
            }
        }
        "max-height" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.max_height = px;
            }
        }
        "min-width" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.min_width = px;
            }
        }
        "min-height" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.min_height = px;
            }
        }
        "animation" => {
            // CSS animation: animation-name <duration> <iteration-count> [easing]
            // Örnek: "pulse 2s infinite" veya "pulse 2s ease-in-out infinite"
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(kf_name) = parts.first() {
                if !kf_name.is_empty() && !kf_name.starts_with('@') {
                    style.animation_name = Some(String::from(*kf_name));
                }
            }
            for p in parts.iter().skip(1) {
                if let Some((num_str, _unit)) = p.split_once("s") {
                    if !num_str.is_empty() {
                        style.animation_duration = num_str.trim().parse::<f32>().unwrap_or(1.0);
                    }
                }
            }
            for p in parts.iter().skip(1) {
                if *p == "infinite"
                    || *p == "infinite "
                    || *p == "loop"
                    || *p == "linear"
                    || *p == "ease"
                    || *p == "ease-in"
                    || *p == "ease-out"
                    || *p == "ease-in-out"
                {
                    style.animation_iterations = -1;
                    break;
                }
                if let Ok(n) = p.parse::<f32>() {
                    style.animation_iterations = if n >= 1.0 { n as i32 } else { -1 };
                    break;
                }
            }
        }
        "transition" => {
            // CSS transition: transition-property <duration> <timing-function>
            // Örnek: "all 0.3s ease" veya "opacity 0.2s"
            let parts: Vec<&str> = val.split_whitespace().collect();
            let tp = parts.first().unwrap_or(&"");
            if !tp.is_empty() && !tp.starts_with('@') {
                style.transition_property = Some(String::from(*tp));
            }
            for p in parts.iter().skip(1) {
                if let Some((num_str, _unit)) = p.split_once("s") {
                    if !num_str.is_empty() {
                        style.transition_duration = num_str.trim().parse::<f32>().unwrap_or(0.0);
                        break;
                    }
                }
            }
        }
        "opacity" => {
            // CSS opacity: 0.0 - 1.0
            if let Ok(n) = val.trim().parse::<f32>() {
                style.opacity = n.clamp(0.0, 1.0);
            }
        }
        "filter" => {
            // CSS filter: blur(), brightness(), contrast(), saturate(),
            // sepia(), hue-rotate(), invert()
            style.filter = Some(val.to_string());
        }
        "aspect-ratio" => {
            // CSS aspect-ratio: birimli sayı veya percentages
            style.aspect_ratio = Some(val.to_string());
        }
        "mask-image" => {
            // CSS mask-image: url() veya svg()
            style.mask_image = Some(val.to_string());
        }
        "clip-path" => {
            // CSS clip-path: function() with shapes
            style.clip_path = Some(val.to_string());
        }
        "list-style-type" => {
            if val.eq_ignore_ascii_case("none") {
                style.list_style_type = 0;
            } else if val.eq_ignore_ascii_case("disc") {
                style.list_style_type = 1;
            } else if val.eq_ignore_ascii_case("circle") {
                style.list_style_type = 2;
            } else if val.eq_ignore_ascii_case("square") {
                style.list_style_type = 3;
            }
        }
        "float" => {
            if val.eq_ignore_ascii_case("left") {
                style.float_left = true;
            } else if val.eq_ignore_ascii_case("right") {
                style.float_right = true;
            }
        }
        "clear" => {
            if val.eq_ignore_ascii_case("both") {
                style.clear_both = true;
            }
        }
        // CSS Grid ozellikleri
        "grid-template-columns" => {
            style.grid_template_columns = parse_grid_tracks(val);
        }
        "grid-template-rows" => {
            style.grid_template_rows = parse_grid_tracks(val);
        }
        "grid-gap" | "gap" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.grid_gap = px;
                style.gap = px;
            }
        }
        "grid-template-areas" => {
            style.grid_template_areas = parse_grid_template_areas(val);
        }
        "grid-area" => {
            // grid-area: <isim> → adlandırılmış alan ataması.
            // grid-area: row / column biçimi sayısal; şu an yalnızca alan
            // adı desteklenir.
            let v = val.trim();
            if !v.is_empty() && !v.contains('/') && !v.starts_with(|c: char| c.is_ascii_digit()) {
                style.grid_area = Some(String::from(v));
            }
        }
        "grid-column" => {
            // grid-column: start / end
        }
        "grid-row" => {
            // grid-row: start / end
        }
        // CSS Flexbox özellikleri
        "flex-direction" => {
            style.flex_direction = match val.to_ascii_lowercase().trim() {
                "row" => 0,
                "row-reverse" => 1,
                "column" => 2,
                "column-reverse" => 3,
                _ => 0,
            };
        }
        "flex-wrap" => {
            style.flex_wrap = match val.to_ascii_lowercase().trim() {
                "nowrap" => 0,
                "wrap" => 1,
                "wrap-reverse" => 2,
                _ => 0,
            };
        }
        "justify-content" => {
            style.justify_content = match val.to_ascii_lowercase().trim() {
                "flex-start" | "start" => 0,
                "flex-end" | "end" => 1,
                "center" => 2,
                "space-between" => 3,
                "space-around" => 4,
                "space-evenly" => 5,
                _ => 0,
            };
        }
        "align-items" => {
            style.align_items = match val.to_ascii_lowercase().trim() {
                "stretch" | "normal" => 0,
                "flex-start" | "start" => 1,
                "flex-end" | "end" => 2,
                "center" => 3,
                "baseline" => 4,
                _ => 0,
            };
        }
        "align-content" => {
            style.align_content = match val.to_ascii_lowercase().trim() {
                "stretch" | "normal" => 0,
                "flex-start" | "start" => 1,
                "flex-end" | "end" => 2,
                "center" => 3,
                "space-between" => 4,
                "space-around" => 5,
                _ => 0,
            };
        }
        "align-self" => {
            // align-self, align-items ile aynı kodlamayı kullanır.
            style.align_self = match val.to_ascii_lowercase().trim() {
                "stretch" | "normal" | "auto" => 0,
                "flex-start" | "start" => 1,
                "flex-end" | "end" => 2,
                "center" => 3,
                "baseline" => 4,
                _ => 0,
            };
        }
        "justify-self" => {
            style.justify_self = match val.to_ascii_lowercase().trim() {
                "stretch" | "normal" | "auto" => 0,
                "flex-start" | "start" | "left" => 1,
                "flex-end" | "end" | "right" => 2,
                "center" => 3,
                _ => 0,
            };
        }
        "order" => {
            if let Ok(n) = val.trim().parse::<i32>() {
                style.order = n;
            }
        }
        "flex-grow" => {
            if let Ok(n) = val.trim().parse::<u32>() {
                style.flex_grow = n;
            }
        }
        "flex-shrink" => {
            if let Ok(n) = val.trim().parse::<u32>() {
                style.flex_shrink = n;
            }
        }
        "flex-basis" => {
            let v = val.trim().to_ascii_lowercase();
            if v == "auto" {
                style.flex_basis = 0;
            } else if let Some(px) = parse_px_value(&v, None, None, None, None) {
                style.flex_basis = px;
            }
        }
        "flex" => {
            // Kısa gösterim: flex-grow [flex-shrink] [flex-basis]
            let parts: Vec<&str> = val.split_whitespace().collect();
            if parts.len() == 1 {
                let p = parts[0].trim();
                if p.eq_ignore_ascii_case("none") {
                    style.flex_grow = 0;
                    style.flex_shrink = 0;
                    style.flex_basis = 0;
                } else if p.eq_ignore_ascii_case("auto") {
                    style.flex_grow = 1;
                    style.flex_shrink = 1;
                    style.flex_basis = 0;
                } else if let Ok(n) = p.parse::<u32>() {
                    style.flex_grow = n;
                    style.flex_shrink = 1;
                    style.flex_basis = 0;
                }
            } else if parts.len() >= 2 {
                if let Ok(g) = parts[0].trim().parse::<u32>() {
                    style.flex_grow = g;
                }
                if let Ok(s) = parts[1].trim().parse::<u32>() {
                    style.flex_shrink = s;
                }
                if parts.len() >= 3 {
                    let b = parts[2].trim().to_ascii_lowercase();
                    if b == "auto" {
                        style.flex_basis = 0;
                    } else if let Some(px) = parse_px_value(&b, None, None, None, None) {
                        style.flex_basis = px;
                    }
                }
            }
        }
        "row-gap" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.row_gap = px;
                if style.gap == 0 {
                    style.gap = px;
                }
                if style.grid_gap == 0 {
                    style.grid_gap = px;
                }
            }
        }
        "column-gap" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.column_gap = px;
                if style.gap == 0 {
                    style.gap = px;
                }
                if style.grid_gap == 0 {
                    style.grid_gap = px;
                }
            }
        }
        _ => {}
    }
}

/// Grid iz listesini ayristir: "100px 1fr 2fr" veya "repeat(3, 1fr)"
fn parse_grid_tracks(val: &str) -> Vec<GridTrack> {
    let mut tracks = Vec::new();
    let t = val.trim();

    // repeat(n, track) duzeni
    if let Some(inner) = t.strip_prefix("repeat(") {
        if let Some(close) = inner.find(')') {
            let args = &inner[..close];
            let comma = args.find(',').unwrap_or(args.len());
            let count: u32 = args[..comma].trim().parse().unwrap_or(1);
            let track_str = args[comma + 1..].trim();
            let track = parse_single_grid_track(track_str);
            for _ in 0..count {
                tracks.push(track.clone());
            }
            return tracks;
        }
    }

    // Boşlukla ayrılmış izler
    for part in t.split_whitespace() {
        tracks.push(parse_single_grid_track(part));
    }
    tracks
}

/// Tek bir grid izini ayristir.
fn parse_single_grid_track(part: &str) -> GridTrack {
    let p = part.trim();
    if p.eq_ignore_ascii_case("auto") {
        return GridTrack::Auto;
    }
    if let Some(n) = p.strip_suffix("fr") {
        if let Ok(fr) = n.trim().parse::<u32>() {
            return GridTrack::Fraction(fr);
        }
    }
    if let Some(px) = parse_px_value(p, None, None, None, None) {
        return GridTrack::Fixed(px);
    }
    GridTrack::Auto
}

/// Katman adının sırasını döndürür; ad ilk kez görülüyorsa sona eklenir.
///
/// Katman önceliği ilk bildirilen katman en düşük, sonraki katmanlar daha
/// yüksek önceliklidir.
fn layer_index(layers: &mut Vec<String>, name: &str) -> usize {
    if let Some(idx) = layers.iter().position(|l| l == name) {
        return idx;
    }
    layers.push(String::from(name));
    layers.len() - 1
}

/// Seçicileri ve bildirim bloklarını ayrıştırır; `@media` gibi iç içe bloklar recursive işlenir.
///
/// `layers` katman adlarının bildirim sırasını tutar; `current_layer` ise
/// içinde bulunulan `@layer` bloğunun sırasını (katmansız kural için `None`).
fn parse_css_rules(
    bytes: &[u8],
    rules: &mut Vec<CssRule>,
    keyframes: &mut Vec<Keyframes>,
    layers: &mut Vec<String>,
    current_media: Option<&str>,
    current_layer: Option<usize>,
) {
    let mut p = 0;
    while p < bytes.len() {
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= bytes.len() {
            break;
        }
        let sel_start = p;
        while p < bytes.len()
            && bytes[p] != b'{'
            && bytes[p] != b'<'
            && bytes[p] != b';'
            && bytes[p] != b'}'
        {
            p += 1;
        }
        if p >= bytes.len() {
            break;
        }
        // Bloksuz at-kuralı (ör. `@layer base, components;`).
        if bytes[p] == b';' {
            let stmt = core::str::from_utf8(&bytes[sel_start..p])
                .unwrap_or("")
                .trim();
            p += 1;
            if stmt.len() > 6 && stmt[..6].eq_ignore_ascii_case("@layer") {
                for name in stmt[6..].split(',') {
                    let name = name.trim();
                    if !name.is_empty() {
                        layer_index(layers, name);
                    }
                }
            }
            continue;
        }
        // Kapanış parantezi: blok dışına çık.
        if bytes[p] == b'}' {
            p += 1;
            continue;
        }
        if bytes[p] != b'{' {
            break;
        }
        let selector = core::str::from_utf8(&bytes[sel_start..p])
            .unwrap_or("")
            .trim();

        // Eşleşen süslü parantezi bul (iç içe bloklar için).
        let mut depth = 0usize;
        let mut block_end = bytes.len();
        let mut i = p;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                depth += 1;
            } else if bytes[i] == b'}' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    block_end = i;
                    break;
                }
            }
            i += 1;
        }
        let block = core::str::from_utf8(&bytes[p + 1..block_end]).unwrap_or("");
        p = if block_end < bytes.len() {
            block_end + 1
        } else {
            bytes.len()
        };

        if selector.starts_with('@') {
            // @media / @supports / @layer / @font-face: içindeki kuralları uygula.
            let at = selector.split_whitespace().next().unwrap_or("");
            if at == "@media" {
                // Media koşulunu ayıkla: "@media screen and (min-width: 600px)"
                let media_cond = selector
                    .split_whitespace()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(" ");
                let combined = match (current_media, media_cond.is_empty()) {
                    (Some(parent), false) => Some(format!("{} and {}", parent, media_cond)),
                    (Some(parent), true) => Some(parent.to_string()),
                    (None, false) => Some(media_cond),
                    (None, true) => None,
                };
                parse_css_rules(
                    block.as_bytes(),
                    rules,
                    keyframes,
                    layers,
                    combined.as_deref(),
                    current_layer,
                );
            } else if at == "@layer" {
                // `@layer ad { ... }`: blok içindeki kurallar katmana aittir.
                // Katmansız (isimsiz) `@layer { ... }` katman önceliğine girmez.
                let layer_name = selector
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let layer = if layer_name.is_empty() {
                    current_layer
                } else {
                    Some(layer_index(layers, &layer_name))
                };
                parse_css_rules(
                    block.as_bytes(),
                    rules,
                    keyframes,
                    layers,
                    current_media,
                    layer,
                );
            } else if at == "@supports" || at == "@container" {
                parse_css_rules(
                    block.as_bytes(),
                    rules,
                    keyframes,
                    layers,
                    current_media,
                    current_layer,
                );
            } else if at == "@keyframes" {
                // @keyframes NAME { 0% { opacity: 1 } 50% { opacity: 0.5 } }
                let kf_name = selector
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !kf_name.is_empty() && keyframes.len() < 64 {
                    let mut frames: Vec<Keyframe> = Vec::new();
                    // Keyframes block: "0% { opacity: 1 } 50% { opacity: 0.5 }"
                    let b = block.as_bytes();
                    let mut i: usize = 0;
                    let mut frame_sel_start: usize = 0;
                    while i < b.len() {
                        if b[i] == b'{' {
                            let sel_part = core::str::from_utf8(&b[frame_sel_start..i])
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let mut style = ElementStyle::new();
                            let mut depth: usize = 1;
                            let mut j = i + 1;
                            let mut close: Option<usize> = None;
                            while j < b.len() {
                                if b[j] == b'{' {
                                    depth += 1;
                                } else if b[j] == b'}' {
                                    depth -= 1;
                                    if depth == 0 {
                                        close = Some(j);
                                        break;
                                    }
                                }
                                j += 1;
                            }
                            let block_content =
                                core::str::from_utf8(&b[i + 1..close.unwrap_or(b.len())])
                                    .unwrap_or("");
                            for decl in split_css_declarations(block_content) {
                                if let Some((prop, val)) = decl.split_once(':') {
                                    let prop = prop.trim().to_ascii_lowercase();
                                    apply_css_decl(&mut style, &prop, val);
                                }
                            }
                            let percent = if sel_part.ends_with('%') {
                                sel_part[..sel_part.len() - 1].parse::<f32>().unwrap_or(0.0)
                            } else {
                                100.0
                            };
                            frames.push(Keyframe { percent, style });
                            if let Some(c) = close {
                                i = c + 1;
                                frame_sel_start = i;
                                while i < b.len() && b[i] != b'{' && b[i] != b'}' && b[i] != b';' {
                                    i += 1;
                                }
                            }
                        } else {
                            i += 1;
                        }
                    }
                    if !frames.is_empty() {
                        keyframes.push(Keyframes {
                            name: kf_name,
                            frames,
                        });
                    }
                } else if at == "@font-face" {
                    // @font-face { font-family: "CustomFont"; src: url("https://..."); }
                    let mut font_family = String::new();
                    let mut src_url = String::new();
                    for decl in split_css_declarations(block) {
                        if let Some((prop, val)) = decl.split_once(':') {
                            let prop = prop.trim().to_ascii_lowercase();
                            let val = val.trim();
                            match prop.as_str() {
                                "font-family" => {
                                    font_family =
                                        val.trim_matches(|c| c == '"' || c == '\'').to_string();
                                }
                                "src" => {
                                    // url("...") veya url('...') formatını ayıkla
                                    if let Some(inner) =
                                        val.strip_prefix("url(").and_then(|s| s.strip_suffix(')'))
                                    {
                                        src_url = inner
                                            .trim_matches(|c| c == '"' || c == '\'')
                                            .to_string();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if !font_family.is_empty() {
                        register_font_face(FontFace {
                            font_family,
                            src_url: if src_url.is_empty() {
                                None
                            } else {
                                Some(src_url)
                            },
                        });
                    }
                }
            }
            continue;
        }

        // Bildirimleri longhand'lara açarak sakla; cascade özellik bazlı
        // hesaplanır. `rule.style` ise geriye dönük uyum için doldurulur.
        let mut decls: Vec<CssDecl> = Vec::new();
        let mut style = ElementStyle::new();
        for decl in split_css_declarations(block) {
            if let Some((prop, val)) = decl.split_once(':') {
                let prop = prop.trim().to_ascii_lowercase();
                if prop.is_empty() {
                    continue;
                }
                let (value, important) = split_important(val);
                for (longhand, longhand_value) in expand_css_declaration(&prop, &value) {
                    apply_css_decl(&mut style, &longhand, &longhand_value);
                    decls.push(CssDecl {
                        prop: longhand,
                        value: longhand_value,
                        important,
                    });
                }
            }
        }
        // Virgülle ayrılmış seçici listesi — :is()/.not() argümanlarındaki
        // virgülleri bölmemek için üst düzey ayrıştırıcı kullanılır.
        for s in split_top_level(selector, ',') {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                rules.push(CssRule {
                    selector: String::from(trimmed),
                    style: style.clone(),
                    media: current_media.map(String::from),
                    decls: decls.clone(),
                    layer: current_layer,
                });
            }
        }
    }
}

#[derive(Clone)]
pub struct ParsedCss {
    pub rules: Vec<CssRule>,
    pub keyframes: Vec<Keyframes>,
}

pub fn parse_css(css_code: &str) -> ParsedCss {
    let mut rules = Vec::new();
    let mut keyframes = Vec::new();
    let cleaned = strip_css_comments(css_code);
    // CSS değişkenleri renk değerlerinde çok yaygın; parse etmeden önce
    // `var(--ad)` çağrılarını çözerek renk kaybını önle.
    let mut variables: Vec<(String, String)> = Vec::new();
    collect_css_variables(&cleaned, &mut variables);
    let expanded = if variables.is_empty() {
        cleaned
    } else {
        substitute_css_variables(&cleaned, &variables)
    };
    let mut layers: Vec<String> = Vec::new();
    parse_css_rules(
        expanded.as_bytes(),
        &mut rules,
        &mut keyframes,
        &mut layers,
        None,
        None,
    );
    ParsedCss { rules, keyframes }
}

/// CSS özgüllük (specificity) skoru: (ID sayısı, sınıf/özellik sayısı, etiket sayısı).
/// W3C spesifikasyonuna göre hesaplanır; yüksek skor kazanıcı.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub a: u32, // ID seçicileri
    pub b: u32, // Sınıf, özellik, sözde-sınıf seçicileri
    pub c: u32, // Etiket ve sözde-element seçicileri
}

impl Specificity {
    pub const fn new(a: u32, b: u32, c: u32) -> Self {
        Self { a, b, c }
    }
}

/// Seçici adı karakterlerini (harf, rakam, '-', '_') atlar.
fn skip_name_chars(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
    {
        i += 1;
    }
    i
}

/// `(` konumundan başlayarak parantez içeriğini ve kapanıştan sonraki indeksi
/// döndürür. İç içe parantezler desteklenir.
fn extract_paren_content(text: &str, open: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (String::from(&text[open + 1..i]), i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (String::from(&text[open + 1..]), bytes.len())
}

/// Parantez içinde olmayan virgüllerden böler.
fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ => {}
        }
        if bytes[i] == sep as u8 && depth == 0 {
            out.push(&text[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

/// Bir CSS seçicisinin özgüllük skorunu hesaplar.
///
/// `:is()`, `:not()` ve `:has()` fonksiyonel sözde-sınıfları argümanlarındaki
/// en yüksek özgüllüğü alır; `:where()` hiçbir katkı yapmaz (0). Diğer
/// sözde-sınıflar (`:hover`, `:nth-child` ...) birer sınıf (b) sayılır.
fn selector_specificity(sel: &str) -> Specificity {
    let s = sel.trim();
    let bytes = s.as_bytes();
    let mut a = 0u32;
    let mut b = 0u32;
    let mut c = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => {
                a += 1;
                i = skip_name_chars(bytes, i + 1);
            }
            b'.' => {
                b += 1;
                i = skip_name_chars(bytes, i + 1);
            }
            b'[' => {
                // Öznitelik seçicisi → sınıf (b) sayılır.
                b += 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b':' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b':' {
                    // Sözde-element: ::before, ::after vb. → c++
                    c += 1;
                    i = skip_name_chars(bytes, i + 1);
                    continue;
                }
                let name_start = i;
                i = skip_name_chars(bytes, i);
                let name = s[name_start..i].to_ascii_lowercase();
                if i < bytes.len() && bytes[i] == b'(' {
                    let (args, next) = extract_paren_content(s, i);
                    i = next;
                    match name.as_str() {
                        "is" | "not" | "has" => {
                            // Argümanlardaki en yüksek özgüllük alınır.
                            let mut best = Specificity::new(0, 0, 0);
                            for arg in split_top_level(&args, ',') {
                                let sp = selector_specificity(arg);
                                if sp > best {
                                    best = sp;
                                }
                            }
                            a += best.a;
                            b += best.b;
                            c += best.c;
                        }
                        // :where() her zaman 0 özgüllük katkısı yapar.
                        "where" => {}
                        _ => b += 1,
                    }
                } else {
                    b += 1;
                }
            }
            b'*' => {
                i += 1; // Evrensel seçici, 0 etki
            }
            _ if bytes[i].is_ascii_alphabetic() => {
                c += 1;
                i = skip_name_chars(bytes, i + 1);
            }
            _ => {
                i += 1;
            }
        }
    }
    Specificity::new(a, b, c)
}

/// Cascade sıralamasına giren tek bir bildirim.
///
/// Sıralama anahtarı: (önem, satır içi mi, katman, özgüllük, kaynak sırası).
struct CascadeEntry {
    /// `@layer` sırası (`None` = katmansız).
    layer: Option<usize>,
    /// Seçicinin özgüllüğü.
    specificity: Specificity,
    /// Kaynak sırası (kurallar vektöründeki indeks).
    source: usize,
    /// Satır içi (`style="..."`) bildirim mi?
    inline_rule: bool,
    /// `!important` işareti taşıyor mu?
    important: bool,
    /// Longhand özellik adı.
    prop: String,
    /// Değer metni.
    value: String,
}

/// Bir @media koşulunu değerlendirir.
/// Desteklenen: screen, (min-width: Npx), (max-width: Npx), (min-height: Npx), (max-height: Npx), and
fn media_matches(cond: &str, viewport_w: usize, viewport_h: usize) -> bool {
    let c = cond.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("screen") || c.eq_ignore_ascii_case("all") {
        return true;
    }

    // "and" ile ayrılmış parçalar. `landscape` gibi kelimelerin içindeki
    // "and" dizisini ayraç sanmamak için kelime sınırı aranır.
    let mut start = 0usize;
    let mut i = 0usize;
    let bytes = c.as_bytes();
    while i < bytes.len() {
        let boundary = i + 3 <= bytes.len()
            && bytes[i..i + 3].eq_ignore_ascii_case(b"and")
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            && (i + 3 == bytes.len() || !bytes[i + 3].is_ascii_alphanumeric());
        if boundary {
            if !media_part_matches(&c[start..i], viewport_w, viewport_h) {
                return false;
            }
            i += 3;
            start = i;
        } else {
            i += 1;
        }
    }
    media_part_matches(&c[start..], viewport_w, viewport_h)
}

/// Tek bir @media koşul parçasını değerlendirir (`screen`, `(min-width: 600px)`).
fn media_part_matches(part: &str, viewport_w: usize, viewport_h: usize) -> bool {
    let part = part.trim();
    if part.is_empty() || part.eq_ignore_ascii_case("screen") || part.eq_ignore_ascii_case("all") {
        return true;
    }
    // (min-width: 600px) veya (max-width: 800px) vb.
    let inner = part
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(part);
    let inner = inner.trim();

    if let Some((prop, val)) = inner.split_once(':') {
        let prop = prop.trim().to_ascii_lowercase();
        let val = val.trim();
        let px = val
            .strip_suffix("px")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);

        match prop.as_str() {
            "min-width" => viewport_w >= px,
            "max-width" => viewport_w <= px,
            "min-height" => viewport_h >= px,
            "max-height" => viewport_h <= px,
            // Tarayıcı açık (light) temayla çizilir; koyu tema kuralları
            // sayfanın varsayılan beyaz zeminini ezmemeli.
            "prefers-color-scheme" => val.eq_ignore_ascii_case("light"),
            "orientation" => {
                if val.eq_ignore_ascii_case("portrait") {
                    viewport_w < viewport_h
                } else {
                    viewport_w >= viewport_h
                }
            }
            _ => true,
        }
    } else {
        true
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn class_list_contains(class: Option<&str>, want: &str) -> bool {
    class.is_some_and(|c| {
        c.split_whitespace()
            .any(|part| part.eq_ignore_ascii_case(want))
    })
}

// ---------------------------------------------------------------------------
// Bağlam-duyarlı seçici eşleştirme
//
// Tek geçişli HTML çizimleyici tam bir DOM kurmaz; yine de kardeş/ata
// bağlamını izlemek için her açılan öğe için kompakt bir `NodeBrief`,
// ebeveynin çocuk listesi `NodeBrief` olarak ve ata zinciri `NodeInfo`
// olarak tutulur. `:nth-child`, `[attr]`, `+`/`~` ve ata birleştiricileri
// bu bağlam üzerinden doğru değerlendirilir.
// ---------------------------------------------------------------------------

/// Bir öğenin seçici bağlamındaki kompakt kopyası (kardeş listelerinde
/// ve özne düğümünde kullanılır).
#[derive(Clone)]
pub struct NodeBrief {
    pub tag: String,
    pub class: Option<String>,
    pub id: Option<String>,
    /// Ham öznitelik metni (`[attr]` seçicileri için, ~768 bayt üst sınır).
    pub attrs: Vec<u8>,
    /// Ebeveyn içindeki 1-tabanlı element sırası.
    pub index: usize,
    /// Aynı etiketli önceki kardeş sayısı + 1 (`:nth-of-type` için).
    pub type_index: usize,
}

/// Açık öğe çerçevesi: düğüm + şimdiye kadarki element çocuklarının
/// kompakt kopyaları (kardeş seçicileri ve `:nth-*` için).
#[derive(Clone)]
pub struct NodeInfo {
    pub base: NodeBrief,
    /// Son ~384 element çocuğun kopyası (seçici bağlamı).
    pub children: Vec<NodeBrief>,
    /// Görülen toplam element çocuğu sayısı (children dolsa bile
    /// `:nth-child` sırası doğru kalır).
    pub child_count: usize,
    /// Etiket → çocuk sayısı (`:nth-of-type` doğruluğu için, ~32 etiket).
    pub tag_counts: Vec<(String, usize)>,
}

impl NodeInfo {
    /// Yeni boş çerçeve.
    pub fn new(base: NodeBrief) -> Self {
        Self {
            base,
            children: Vec::new(),
            child_count: 0,
            tag_counts: Vec::new(),
        }
    }

    /// Çocuk sayacı + etiket sayacını günceller; çocuk kopyasını
    /// liste doluysa eklemez (bağlam yaklaşık kalır).
    pub fn note_child(&mut self, brief: &NodeBrief) {
        self.child_count += 1;
        if let Some((_, n)) = self
            .tag_counts
            .iter_mut()
            .find(|(t, _)| t.eq_ignore_ascii_case(&brief.tag))
        {
            *n += 1;
        } else if self.tag_counts.len() < 32 {
            self.tag_counts.push((brief.tag.clone(), 1));
        }
        if self.children.len() < 384 {
            self.children.push(brief.clone());
        }
    }

    /// Aynı etiketli çocuk sayısı + 1 (:nth-of-type dizini).
    pub fn type_index_of(&self, tag: &str) -> usize {
        self.tag_counts
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(tag))
            .map_or(1, |(_, n)| n + 1)
    }
}

/// Seçici eşleştirmesi için özne öğe bağlamı.
pub struct ElemCtx<'a> {
    /// Özne düğüm (henüz ebeveynin çocuk listesine eklenmemiş olabilir).
    pub node: &'a NodeBrief,
    /// Kökten doğrudan ebeveyne ata zinciri (`NodeInfo` dilimleri).
    pub ancestors: &'a [&'a NodeInfo],
    /// Öğe ile aynı ebeveyndeki önceki element kardeşler.
    pub prev_sibs: &'a [NodeBrief],
}

/// Eşleştiricideki bir düğüm referansı.
#[derive(Clone, Copy)]
enum NodeRef<'a> {
    Brief(&'a NodeBrief),
    Info(&'a NodeInfo),
}

impl<'a> NodeRef<'a> {
    fn tag(&self) -> &'a str {
        match self {
            NodeRef::Brief(b) => &b.tag,
            NodeRef::Info(n) => &n.base.tag,
        }
    }
    fn class(&self) -> Option<&'a str> {
        match self {
            NodeRef::Brief(b) => b.class.as_deref(),
            NodeRef::Info(n) => n.base.class.as_deref(),
        }
    }
    fn id(&self) -> Option<&'a str> {
        match self {
            NodeRef::Brief(b) => b.id.as_deref(),
            NodeRef::Info(n) => n.base.id.as_deref(),
        }
    }
    fn attrs(&self) -> &'a [u8] {
        match self {
            NodeRef::Brief(b) => &b.attrs,
            NodeRef::Info(n) => &n.base.attrs,
        }
    }
    fn index(&self) -> usize {
        match self {
            NodeRef::Brief(b) => b.index,
            NodeRef::Info(n) => n.base.index,
        }
    }
    fn type_index(&self) -> usize {
        match self {
            NodeRef::Brief(b) => b.type_index,
            NodeRef::Info(n) => n.base.type_index,
        }
    }
}

/// Eşleştirme sırasında bir düğüm + onun bağlamı.
struct View<'a> {
    node: NodeRef<'a>,
    /// Düğümün ataları, kökten ebeveyne doğru.
    ancestors: &'a [&'a NodeInfo],
    /// Düğümden önceki element kardeşler.
    prev_sibs: &'a [NodeBrief],
}

impl<'a> View<'a> {
    fn subject(ctx: &'a ElemCtx<'a>) -> Self {
        View {
            node: NodeRef::Brief(ctx.node),
            ancestors: ctx.ancestors,
            prev_sibs: ctx.prev_sibs,
        }
    }
    /// Doğrudan ebeveyn görünümü.
    fn parent(&self) -> Option<View<'a>> {
        let (parent, rest) = self.ancestors.split_last()?;
        Some(View {
            node: NodeRef::Info(parent),
            ancestors: rest,
            prev_sibs: &parent.children,
        })
    }
    /// Ata zincirindeki `idx` numaralı (kök=0) ata görünümü.
    fn ancestor_at(&self, idx: usize) -> Option<View<'a>> {
        let node = self.ancestors.get(idx)?;
        Some(View {
            node: NodeRef::Info(node),
            ancestors: &self.ancestors[..idx],
            prev_sibs: &node.children,
        })
    }
    /// `list` içindeki `idx` numaralı önceki kardeşin görünümü.
    fn sibling_at(&self, list: &'a [NodeBrief], idx: usize) -> View<'a> {
        View {
            node: NodeRef::Brief(&list[idx]),
            ancestors: self.ancestors,
            prev_sibs: &list[..idx],
        }
    }
}

/// Karmaşık seçiciyi (birleştirici, bileşik) çiftlerine böler.
/// Birleştirici, kendinden SONRAKİ bileşiğe aittir.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Comb {
    Descendant,
    Child,
    Adjacent,
    General,
}

/// Üst düzeydeki (parantez/köşeli ayraç dışındaki) birleştiricilere göre
/// seçiciyi böler; ilk elemanın birleştiricisi `None`'dır.
fn split_complex(sel: &str) -> Vec<(Option<Comb>, String)> {
    let bytes = sel.as_bytes();
    let mut out: Vec<(Option<Comb>, String)> = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut pending: Option<Comb> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'>' | b'+' | b'~' if depth == 0 => {
                let comp = sel[start..i].trim();
                if !comp.is_empty() {
                    out.push((pending.take(), comp.to_string()));
                }
                pending = Some(match bytes[i] {
                    b'>' => Comb::Child,
                    b'+' => Comb::Adjacent,
                    _ => Comb::General,
                });
                i += 1;
                start = i;
                continue;
            }
            b' ' | b'\t' | b'\n' | b'\r' if depth == 0 => {
                // Ardışık boşlukları tek torun birleştiricisine indir;
                // açık bir birleştirici bekliyorsa boşluğu atla.
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let comp = sel[start..i].trim();
                if !comp.is_empty() {
                    out.push((pending.take(), comp.to_string()));
                    pending = Some(Comb::Descendant);
                }
                i = j;
                start = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    let comp = sel[start..].trim();
    if !comp.is_empty() {
        out.push((pending.take(), comp.to_string()));
    }
    out
}

/// `an+b` biçimindeki :nth-* argümanını (a, b) olarak çözer.
/// "odd"→(2,1), "even"→(2,0), "3"→(0,3), "2n+1"→(2,1), "-n+3"→(-1,3).
fn parse_anb(s: &str) -> Option<(i64, i64)> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if !s.contains('n') {
        return s.parse::<i64>().ok().map(|n| (0, n));
    }
    let (a_part, b_part) = s.split_once('n')?;
    let a = match a_part.trim() {
        "" | "+" => 1,
        "-" => -1,
        v => v.parse::<i64>().ok()?,
    };
    let b_str = b_part.trim();
    let b = if b_str.is_empty() {
        0
    } else {
        // "n+3" → b_part = "+3"; "n-3" → "-3"; "2n3" geçersiz
        if b_str.starts_with('+') || b_str.starts_with('-') {
            b_str.parse::<i64>().ok()?
        } else {
            return None;
        }
    };
    Some((a, b))
}

/// 1-tabanlı `index`'in `an+b` kalıbına uyup uymadığını denetler.
fn nth_matches(index: usize, a: i64, b: i64) -> bool {
    if a == 0 {
        return index as i64 == b;
    }
    let d = index as i64 - b;
    d % a == 0 && d / a >= 0
}

/// Ham öznitelik metninde `name` özniteliğinin değerini döndürür.
/// Değer tırnakları ayıklanır; öznitelik değersizse boş dilim döner.
fn attr_lookup<'a>(attrs: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let name_b = name.as_bytes();
    let mut i = 0usize;
    while i < attrs.len() {
        // Öznitelik adı
        while i < attrs.len() && (attrs[i].is_ascii_whitespace() || attrs[i] == b'/') {
            i += 1;
        }
        let k_start = i;
        while i < attrs.len()
            && (attrs[i].is_ascii_alphanumeric() || attrs[i] == b'-' || attrs[i] == b'_')
        {
            i += 1;
        }
        if i == k_start {
            i += 1;
            continue;
        }
        let cur = &attrs[k_start..i];
        while i < attrs.len() && attrs[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < attrs.len() && attrs[i] == b'=' {
            i += 1;
            while i < attrs.len() && attrs[i].is_ascii_whitespace() {
                i += 1;
            }
            let (v_start, quote) = if i < attrs.len() && (attrs[i] == b'\'' || attrs[i] == b'"') {
                let q = attrs[i];
                i += 1;
                (i, q)
            } else {
                (i, 0)
            };
            while i < attrs.len()
                && ((quote != 0 && attrs[i] != quote)
                    || (quote == 0 && !attrs[i].is_ascii_whitespace() && attrs[i] != b'>'))
            {
                i += 1;
            }
            let val = &attrs[v_start..i];
            if quote != 0 && i < attrs.len() {
                i += 1; // kapanış tırnağı
            }
            if cur.eq_ignore_ascii_case(name_b) {
                return Some(val);
            }
        } else if cur.eq_ignore_ascii_case(name_b) {
            return Some(b"");
        }
    }
    None
}

/// `[name op value]` öznitelik seçicisini değerlendirir.
fn attr_selector_matches(attrs: &[u8], content: &str) -> bool {
    let c = content.trim();
    if c.is_empty() {
        return false;
    }
    // İşleç: iki karakterli (~=, |=, ^=, $=, *=) veya =.
    let bytes = c.as_bytes();
    let mut op_pos = None;
    let mut op_len = 0usize;
    for i in 0..bytes.len() {
        if bytes[i] == b'=' {
            op_pos = Some(i);
            op_len = if i > 0 && matches!(bytes[i - 1], b'~' | b'|' | b'^' | b'$' | b'*') {
                2
            } else {
                1
            };
            break;
        }
        if bytes[i] == b']' {
            break;
        }
    }
    let ci = c.to_ascii_lowercase().ends_with(" i");
    let body = if ci {
        let t = c.trim_end();
        t[..t.len() - 2].trim_end()
    } else {
        c
    };
    let (name, op, raw_val) = match op_pos {
        Some(pos) => {
            let name_end = if op_len == 2 { pos - 1 } else { pos };
            let op = &body[name_end..pos + 1];
            let mut v = body[pos + 1..].trim();
            // Tırnakları ayıkla
            if v.len() >= 2
                && ((v.starts_with('"') && v.ends_with('"'))
                    || (v.starts_with('\'') && v.ends_with('\'')))
            {
                v = &v[1..v.len() - 1];
            }
            (body[..name_end].trim(), op, v)
        }
        None => (body, "", ""),
    };
    if name.is_empty() {
        return false;
    }
    let Some(val) = attr_lookup(attrs, name) else {
        return false;
    };
    let val_str = core::str::from_utf8(val).unwrap_or("");
    let want = raw_val;
    let eq = |a: &str, b: &str| {
        if ci {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    match op {
        "" => true, // [attr] — varlık
        "=" => eq(val_str, want),
        "~=" => val_str.split_whitespace().any(|w| eq(w, want)),
        "|=" => {
            eq(val_str, want)
                || (val_str.len() > want.len()
                    && val_str.starts_with(want)
                    && val_str.as_bytes()[want.len()] == b'-')
        }
        "^=" => !want.is_empty() && val_str.starts_with(want),
        "$=" => !want.is_empty() && val_str.ends_with(want),
        "*=" => !want.is_empty() && val_str.contains(want),
        _ => false,
    }
}

/// Sözde-sınıfı bağlam üzerinde değerlendirir.
fn pseudo_matches(name: &str, args: Option<&str>, view: &View) -> bool {
    match name {
        "first-child" => view.node.index() == 1,
        "nth-child" => {
            let Some(a) = args.and_then(parse_anb) else {
                return false;
            };
            nth_matches(view.node.index(), a.0, a.1)
        }
        "nth-of-type" => {
            let Some(a) = args.and_then(parse_anb) else {
                return false;
            };
            nth_matches(view.node.type_index(), a.0, a.1)
        }
        // :root — html öğesi; ata zincirinde yalnızca sanal #document
        // kök çerçevesi bulunabilir.
        "root" => {
            view.node.tag().eq_ignore_ascii_case("html")
                && view.ancestors.iter().all(|n| n.base.tag == "#document")
        }
        "is" | "where" => {
            let list = args.unwrap_or_default();
            split_top_level(list, ',').iter().any(|arg| {
                let parts = split_complex(arg);
                !parts.is_empty() && complex_view_matches(&parts, parts.len() - 1, view)
            })
        }
        "not" => {
            let list = args.unwrap_or_default();
            !split_top_level(list, ',').iter().any(|arg| {
                let parts = split_complex(arg);
                !parts.is_empty() && complex_view_matches(&parts, parts.len() - 1, view)
            })
        }
        // Dinamik/durum sözde-sınıfları ve desteklenmeyenler eşleşmez.
        _ => false,
    }
}

/// Karmaşık seçicinin `i` numaralı (sağdan) bileşiğini `view` üzerinde
/// eşleştirir ve birleştiricileri geriye doğru izler.
fn complex_view_matches(parts: &[(Option<Comb>, String)], i: usize, view: &View) -> bool {
    let Some((_, compound)) = parts.get(i) else {
        return false;
    };
    if !compound_view_matches(compound, view) {
        return false;
    }
    if i == 0 {
        return true;
    }
    match parts[i].0.unwrap_or(Comb::Descendant) {
        Comb::Child => view
            .parent()
            .is_some_and(|v| complex_view_matches(parts, i - 1, &v)),
        Comb::Descendant => {
            // En yakın atadan köke doğru dene.
            let mut idx = view.ancestors.len();
            while idx > 0 {
                idx -= 1;
                if let Some(v) = view.ancestor_at(idx) {
                    if complex_view_matches(parts, i - 1, &v) {
                        return true;
                    }
                }
            }
            false
        }
        Comb::Adjacent => view.prev_sibs.last().is_some_and(|_| {
            let k = view.prev_sibs.len() - 1;
            let v = view.sibling_at(view.prev_sibs, k);
            complex_view_matches(parts, i - 1, &v)
        }),
        Comb::General => {
            for k in (0..view.prev_sibs.len()).rev() {
                let v = view.sibling_at(view.prev_sibs, k);
                if complex_view_matches(parts, i - 1, &v) {
                    return true;
                }
            }
            false
        }
    }
}

/// Bir bileşik seçiciyi (`div.card`, `.a.b`, `#id`, `*`, `[attr=x]`,
/// `:nth-child(2)`) bağlam üzerinde eşleştirir.
fn compound_view_matches(compound: &str, view: &View) -> bool {
    let s = compound.trim();
    if s.is_empty() {
        return false;
    }
    let base_end = s.find([':', '[']).unwrap_or(s.len());
    let base = s[..base_end].trim();

    if !base.is_empty() && base != "*" {
        let (tag_part, mut rest) = match base.find(['#', '.']) {
            Some(pos) => (&base[..pos], &base[pos..]),
            None => (base, ""),
        };
        if !tag_part.is_empty()
            && tag_part != "*"
            && !tag_part.eq_ignore_ascii_case(view.node.tag())
        {
            return false;
        }
        while let Some(ch) = rest.chars().next() {
            if ch == '#' {
                rest = &rest[1..];
                let end = rest.find(['#', '.']).unwrap_or(rest.len());
                let want = &rest[..end];
                if !view.node.id().is_some_and(|v| v.eq_ignore_ascii_case(want)) {
                    return false;
                }
                rest = &rest[end..];
            } else if ch == '.' {
                rest = &rest[1..];
                let end = rest.find(['#', '.']).unwrap_or(rest.len());
                let want = &rest[..end];
                if !class_list_contains(view.node.class(), want) {
                    return false;
                }
                rest = &rest[end..];
            } else {
                rest = &rest[1..];
            }
        }
    } else if base.is_empty() {
        // Yalnızca öznitelik/sözde seçicisi — geçerli, devam et.
    }

    // Kalan bölümü öznitelik ve sözde-sınıf koşullarına göre tara.
    let bytes = s.as_bytes();
    let mut i = base_end;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                let mut depth = 1i32;
                let mut j = i + 1;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if depth != 0 {
                    return false;
                }
                let content = &s[i + 1..j - 1];
                if !attr_selector_matches(view.node.attrs(), content) {
                    return false;
                }
                i = j;
            }
            b':' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b':' {
                    // Sözde-elementler statik çizimde desteklenmez.
                    return false;
                }
                let start = i;
                i = skip_name_chars(bytes, i);
                let name = s[start..i].to_ascii_lowercase();
                let mut args = None;
                if i < bytes.len() && bytes[i] == b'(' {
                    let (content, next) = extract_paren_content(s, i);
                    args = Some(content);
                    i = next;
                }
                if !pseudo_matches(&name, args.as_deref(), view) {
                    return false;
                }
            }
            _ => i += 1,
        }
    }
    true
}

/// Seçici eşleştirme (bağlamlı): virgülle ayrılmış parçalar,
/// torun/çocuk/kardeş birleştiricileri ve bileşik seçiciler desteklenir.
/// Dinamik sözde-sınıflar (`:hover`) ve sözde-elementler statik çizimde
/// uygulanmaz.
fn selector_matches_ctx(sel: &str, view: &View) -> bool {
    let sel = sel.trim();
    if sel.is_empty() {
        return false;
    }
    // Virgüllü gruplarda yalnızca dinamik/sözde-element içeren parçayı atla;
    // `a, a:visited { color: red }` gibi yaygın grupların statik kısmı çalışsın.
    for part in split_top_level(sel, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if contains_ci(part, "::")
            || contains_ci(part, ":hover")
            || contains_ci(part, ":focus")
            || contains_ci(part, ":active")
            || contains_ci(part, ":visited")
            || contains_ci(part, ":link")
        {
            continue;
        }
        let parts = split_complex(part);
        if parts.is_empty() {
            continue;
        }
        if complex_view_matches(&parts, parts.len() - 1, view) {
            return true;
        }
    }
    false
}

/// CSS Cascade: kuralları katman → özgüllük → kaynak sırası ile sıralar ve
/// bildirimleri özellik bazlı çözer.
///
/// `media` ile filtreleme yapılır; eşleşmeyen @media kuralları atlanır.
/// Aynı özgüllükteyse sonra gelen kural kazanır (son kazanır kuralı).
/// `!important` bildirimler normal bildirimlerin üzerinde uygulanır.
fn apply_css_cascade_ctx(
    rules: &[CssRule],
    ctx: &ElemCtx,
    inline_style: Option<&str>,
    viewport_w: usize,
    viewport_h: usize,
) -> ElementStyle {
    let view = View::subject(ctx);
    let mut entries: Vec<CascadeEntry> = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        // @media filtresi
        if let Some(ref media) = rule.media {
            if !media_matches(media, viewport_w, viewport_h) {
                continue;
            }
        }

        // Virgülle ayrılmış her seçici kendi özgüllüğüyle değerlendirilir.
        for part in split_top_level(&rule.selector, ',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if !selector_matches_ctx(part, &view) {
                continue;
            }
            let specificity = selector_specificity(part);
            for decl in &rule.decls {
                entries.push(CascadeEntry {
                    layer: rule.layer,
                    specificity,
                    source: index,
                    inline_rule: false,
                    important: decl.important,
                    prop: decl.prop.clone(),
                    value: decl.value.clone(),
                });
            }
        }
    }

    // Satır içi bildirimler: normal kuralların üstünde, `!important` kurallarının
    // altında yer alır.
    if let Some(inline) = inline_style {
        for decl in split_css_declarations(inline) {
            if let Some((prop, val)) = decl.split_once(':') {
                let prop = prop.trim().to_ascii_lowercase();
                if prop.is_empty() {
                    continue;
                }
                let (value, important) = split_important(val);
                for (longhand, longhand_value) in expand_css_declaration(&prop, &value) {
                    entries.push(CascadeEntry {
                        layer: None,
                        specificity: Specificity::new(0, 0, 0),
                        source: usize::MAX,
                        inline_rule: true,
                        important,
                        prop: longhand,
                        value: longhand_value,
                    });
                }
            }
        }
    }

    // Çözümleme sırası: önce normal bildirimler (katman → özgüllük → kaynak
    // sırası), sonra `!important` bildirimler (aynı sıralama).
    entries.sort_by(|x, y| {
        let kx = (
            x.important,
            x.inline_rule,
            x.layer.unwrap_or(usize::MAX),
            x.specificity,
            x.source,
        );
        let ky = (
            y.important,
            y.inline_rule,
            y.layer.unwrap_or(usize::MAX),
            y.specificity,
            y.source,
        );
        kx.cmp(&ky)
    });

    // Bildirimler sırayla uygulanır; son uygulanan kazanır.
    let mut resolved = ElementStyle::new();
    for entry in &entries {
        apply_css_decl(&mut resolved, &entry.prop, &entry.value);
    }
    resolved
}

pub fn match_css_style(
    rules: &[CssRule],
    tag: &str,
    class: Option<&str>,
    id: Option<&str>,
    inline_style: Option<&str>,
    viewport_w: usize,
    viewport_h: usize,
) -> ElementStyle {
    // Bağlamsız çağrılar (html/body kökü, testler): ata/kardeş
    // birleştiricileri eşleşmez; basit seçiciler çalışmaya devam eder.
    let node = NodeBrief {
        tag: tag.to_string(),
        class: class.map(|c| c.to_string()),
        id: id.map(|i| i.to_string()),
        attrs: Vec::new(),
        index: 1,
        type_index: 1,
    };
    let ctx = ElemCtx {
        node: &node,
        ancestors: &[],
        prev_sibs: &[],
    };
    apply_css_cascade_ctx(rules, &ctx, inline_style, viewport_w, viewport_h)
}

/// Tam bağlamlı eşleştirme: `ctx` öğenin ata/kardeş/öznitelik bilgisini
/// taşır; `:nth-child`, `[attr]`, `+`/`~` ve ata birleştiricileri
/// doğru değerlendirilir.
pub fn match_css_style_ctx(
    rules: &[CssRule],
    ctx: &ElemCtx,
    inline_style: Option<&str>,
    viewport_w: usize,
    viewport_h: usize,
) -> ElementStyle {
    apply_css_cascade_ctx(rules, ctx, inline_style, viewport_w, viewport_h)
}

/// Animasyon ismi ile keyframes kaydını bulur.
pub fn find_keyframes<'a>(keyframes: &'a [Keyframes], name: &str) -> Option<&'a Keyframes> {
    keyframes.iter().find(|k| k.name == name)
}

/// CSS animasyonu için geçerli kare değerini hesaplar.
/// `elapsed_ms` animasyonun başlangıcından geçen süre, `duration_ms` animasyon süresi,
/// `iterations` tekrar sayısı (-1 = sonsuz). Döndürülen değer 0.0..=1.0 arası ilerleme.
pub fn animation_progress(elapsed_ms: u64, duration_ms: f32, iterations: i32) -> Option<f32> {
    if duration_ms <= 0.0 {
        return None;
    }
    let duration_us = (duration_ms * 1000.0) as u64;
    let elapsed_us = elapsed_ms * 1000;

    // Tek döngü içi ilerleme (0.0..=1.0)
    let cycle = (elapsed_us % duration_us) as f32 / duration_us as f32;

    // Tekrar sayısı kontrolü
    if iterations > 0 {
        let total_cycles = elapsed_us / duration_us;
        if total_cycles >= iterations as u64 {
            return Some(1.0); // Animasyon bitti, son kare
        }
    }

    Some(cycle)
}

/// Keyframes arası lineer interpolasyon yapar.
/// `progress` 0.0..=1.0 arası bir değer. İlk ve son kare arasında interpolasyon yapar.
pub fn interpolate_keyframes(kf: &Keyframes, progress: f32) -> ElementStyle {
    if kf.frames.is_empty() {
        return ElementStyle::new();
    }
    if kf.frames.len() == 1 {
        return kf.frames[0].style.clone();
    }

    let p = progress.clamp(0.0, 1.0) * 100.0; // 0..=100

    // İki kareyi bul: p'nin hangi iki kare arasında olduğunu
    for i in 0..kf.frames.len() - 1 {
        let f0 = &kf.frames[i];
        let f1 = &kf.frames[i + 1];
        if p >= f0.percent && p <= f1.percent {
            let t = if f1.percent > f0.percent {
                (p - f0.percent) / (f1.percent - f0.percent)
            } else {
                0.0
            };
            return blend_styles(&f0.style, &f1.style, t);
        }
    }

    // p son karenin üzerindeyse son kareyi döndür. Liste beklenmedik biçimde
    // boşaldıysa (ör. çalışma zamanında mutasyon) boş stil güvenli düşüştür.
    kf.frames
        .last()
        .map(|frame| frame.style.clone())
        .unwrap_or_else(ElementStyle::new)
}

/// İki stil arasında lineer interpolasyon (opacity, renk, arka plan rengi).
pub fn blend_styles(a: &ElementStyle, b: &ElementStyle, t: f32) -> ElementStyle {
    let mut result = a.clone();
    result.opacity = a.opacity + (b.opacity - a.opacity) * t;
    // Renk interpolasyonu
    if let (Some(ca), Some(cb)) = (a.color, b.color) {
        let ra = ((ca >> 16) & 0xFF) as f32;
        let ga = ((ca >> 8) & 0xFF) as f32;
        let ba = (ca & 0xFF) as f32;
        let rb = ((cb >> 16) & 0xFF) as f32;
        let gb = ((cb >> 8) & 0xFF) as f32;
        let bb = (cb & 0xFF) as f32;
        let r = (ra + (rb - ra) * t) as u32;
        let g = (ga + (gb - ga) * t) as u32;
        let b = (ba + (bb - ba) * t) as u32;
        result.color = Some((r << 16) | (g << 8) | b);
    }
    // Arka plan rengi interpolasyonu
    if let (Some(ca), Some(cb)) = (a.bg_color, b.bg_color) {
        let ra = ((ca >> 16) & 0xFF) as f32;
        let ga = ((ca >> 8) & 0xFF) as f32;
        let ba = (ca & 0xFF) as f32;
        let rb = ((cb >> 16) & 0xFF) as f32;
        let gb = ((cb >> 8) & 0xFF) as f32;
        let bb = (cb & 0xFF) as f32;
        let r = (ra + (rb - ra) * t) as u32;
        let g = (ga + (gb - ga) * t) as u32;
        let b = (ba + (bb - ba) * t) as u32;
        result.bg_color = Some((r << 16) | (g << 8) | b);
    }
    result
}

/// CSS geçiş (transition) ilerlemesini hesaplar.
/// `elapsed_ms` geçişin başlangıcından geçen süre, `duration_ms` geçiş süresi.
/// Döndürülen değer 0.0..=1.0 arası ilerleme.
pub fn transition_progress(elapsed_ms: u64, duration_ms: f32) -> Option<f32> {
    if duration_ms <= 0.0 {
        return None;
    }
    let duration_us = (duration_ms * 1000.0) as u64;
    let elapsed_us = elapsed_ms * 1000;
    if elapsed_us >= duration_us {
        return Some(1.0);
    }
    Some(elapsed_us as f32 / duration_us as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_placer_assigns_named_areas_and_spans() {
        // "header header" / "side main" şablonu.
        let parsed = parse_css(".g { grid-template-areas: \"header header\" \"side main\"; }");
        let style = match_css_style(&parsed.rules, "div", Some("g"), None, None, 600, 768);
        assert_eq!(style.grid_template_areas.len(), 2);
        assert_eq!(
            style.grid_template_areas[1],
            vec![String::from("side"), String::from("main")]
        );

        let mut placer = GridPlacer::new(&style.grid_template_areas, 2);
        let header = placer.place(Some("header"));
        assert_eq!((header.row, header.col, header.col_span), (0, 0, 2));
        // Aynı alan ikinci kez istenirse otomatik akışa düşer (şablon hücreleri
        // rezerve olduğu için şablon dışındaki satıra geçer).
        let again = placer.place(Some("header"));
        assert!(again.row >= 2);
        let side = placer.place(Some("side"));
        assert_eq!((side.row, side.col), (1, 0));
        let main = placer.place(Some("main"));
        assert_eq!((main.row, main.col), (1, 1));
        // Yerleşen alanlar çizim için kaydedilir.
        assert_eq!(placer.placed_rects().len(), 3);
    }

    #[test]
    fn resolves_css_variables_and_color_formats() {
        let parsed = parse_css(
            ":root { --brand: #123456; --accent: rgb(0 128 255); }\n\
             body { background-color: var(--brand); color: rgb(255 0 0 / 50%); }\n\
             .card { background: #abc url(bg.png); border: 1px solid hsl(120, 100%, 50%); }\n\
             .link { color: var(--accent); }",
        );

        let body = match_css_style(&parsed.rules, "body", None, None, None, 600, 768);
        assert_eq!(body.bg_color, Some(0x123456));
        assert_eq!(body.color, Some(0xff0000));

        let card = match_css_style(&parsed.rules, "div", Some("card"), None, None, 600, 768);
        assert_eq!(card.bg_color, Some(0xaabbcc));
        assert_eq!(card.border_color, Some(0x00ff00));
        assert_eq!(card.background_image.as_deref(), Some("bg.png"));
        assert_eq!(card.background_repeat, Some(true));
        assert_eq!(card.background_size, Some(BackgroundSize::Auto));

        let link = match_css_style(&parsed.rules, "span", Some("link"), None, None, 600, 768);
        assert_eq!(link.color, Some(0x0080ff));
    }

    #[test]
    fn selector_groups_keep_static_parts() {
        let parsed = parse_css("a, a:visited { color: #112233; } body.dark { background: white; }");
        let link = match_css_style(&parsed.rules, "a", None, None, None, 600, 768);
        assert_eq!(link.color, Some(0x112233));

        let dark = match_css_style(&parsed.rules, "body", Some("dark"), None, None, 600, 768);
        assert_eq!(dark.bg_color, Some(0xffffff));
        let light = match_css_style(&parsed.rules, "body", None, None, None, 600, 768);
        assert_eq!(light.bg_color, None);
    }

    #[test]
    fn inline_style_color_overrides_rules() {
        let parsed = parse_css(".card { color: red; }");
        let card = match_css_style(
            &parsed.rules,
            "div",
            Some("card"),
            None,
            Some("color: blue"),
            600,
            768,
        );
        assert_eq!(card.color, Some(0x0000ff));
    }

    #[test]
    fn light_color_scheme_wins_over_dark() {
        let parsed = parse_css(
            "@media (prefers-color-scheme: dark) { body { background: black; color: white; } }\n\
             @media (prefers-color-scheme: light) { body { background: white; color: black; } }",
        );
        let body = match_css_style(&parsed.rules, "body", None, None, None, 600, 768);
        assert_eq!(body.bg_color, Some(0xffffff));
        assert_eq!(body.color, Some(0x000000));
    }

    #[test]
    fn display_property_activates_flex_grid_and_block() {
        let parsed = parse_css(
            ".flex { display: flex; }\n\
             .grid { display: grid; }\n\
             .block { display: block; }\n\
             .inline { display: inline; }\n\
             .hidden { display: none; }\n\
             .ib { display: inline-block; }",
        );
        let flex = match_css_style(&parsed.rules, "div", Some("flex"), None, None, 600, 768);
        assert_eq!(flex.display, Some(1));
        assert!(!flex.display_none);

        let grid = match_css_style(&parsed.rules, "div", Some("grid"), None, None, 600, 768);
        assert_eq!(grid.display, Some(2));
        assert!(!grid.display_none);

        let block = match_css_style(&parsed.rules, "div", Some("block"), None, None, 600, 768);
        assert_eq!(block.display, Some(3));
        assert!(!block.display_none);

        let inline = match_css_style(&parsed.rules, "span", Some("inline"), None, None, 600, 768);
        assert_eq!(inline.display, Some(4));
        assert!(!inline.display_none);

        let hidden = match_css_style(&parsed.rules, "div", Some("hidden"), None, None, 600, 768);
        assert!(hidden.display_none);
        assert_eq!(hidden.display, None);

        let ib = match_css_style(&parsed.rules, "span", Some("ib"), None, None, 600, 768);
        assert_eq!(ib.display, Some(5));
        assert!(!ib.display_none);
    }

    #[test]
    fn overflow_property_sets_overflow_mode() {
        let parsed = parse_css(
            ".vis { overflow: visible; }\n\
             .hid { overflow: hidden; }\n\
             .sc { overflow: scroll; }",
        );
        let vis = match_css_style(&parsed.rules, "div", Some("vis"), None, None, 600, 768);
        assert_eq!(vis.overflow, 0);
        let hid = match_css_style(&parsed.rules, "div", Some("hid"), None, None, 600, 768);
        assert_eq!(hid.overflow, 1);
        let sc = match_css_style(&parsed.rules, "div", Some("sc"), None, None, 600, 768);
        assert_eq!(sc.overflow, 2);
    }

    // --- Bağlamlı seçici motoru testleri ---

    fn brief(
        tag: &str,
        class: Option<&str>,
        id: Option<&str>,
        attrs: &str,
        index: usize,
    ) -> NodeBrief {
        NodeBrief {
            tag: tag.to_string(),
            class: class.map(|c| c.to_string()),
            id: id.map(|i| i.to_string()),
            attrs: attrs.as_bytes().to_vec(),
            index,
            type_index: index,
        }
    }

    /// `ul > li:nth-child(2)` gibi seçicileri test etmek için
    /// basit bir ata/kardeş bağlamı kurar.
    fn ctx<'a>(
        node: &'a NodeBrief,
        ancestors: &'a [&'a NodeInfo],
        prev_sibs: &'a [NodeBrief],
    ) -> ElemCtx<'a> {
        ElemCtx {
            node,
            ancestors,
            prev_sibs,
        }
    }

    fn has_color(rules: &[CssRule], c: &ElemCtx) -> bool {
        match_css_style_ctx(rules, c, None, 600, 768)
            .color
            .is_some()
    }

    #[test]
    fn attr_selectors_match() {
        let parsed = parse_css(
            "[data-x] { color: red; }\n\
             [data-x=\"1\"] { color: red; }\n\
             [data-x='2'] { color: red; }\n\
             [data-x~=tok] { color: red; }\n\
             [data-x|=pre] { color: red; }\n\
             [data-x^=ab] { color: red; }\n\
             [data-x$=yz] { color: red; }\n\
             [data-x*=mid] { color: red; }\n\
             [data-x=NO] { color: red; }\n\
             [data-x=no i] { color: red; }",
        );
        let node = brief("div", None, None, "data-x=\"1\" id=\"a\"", 1);
        let c = ctx(&node, &[], &[]);
        // [data-x], [data-x="1"], [data-x^=ab]? hayır — değer "1".
        // Eşleşenler: [data-x], [data-x="1"], [data-x=no i] değil.
        assert!(has_color(&parsed.rules, &c)); // en az [data-x] eşleşir
                                               // Değer "1" için ~=, |=, ^=, $=, *= ve i bayraksız =NO eşleşmez;
                                               // kesin doğrulama için ayrı kurallar:
        let p2 = parse_css("[data-x=\"1\"] { color: red; }");
        assert!(has_color(&p2.rules, &c));
        let p3 = parse_css("[data-x=\"2\"] { color: red; }");
        assert!(!has_color(&p3.rules, &c));
        let p4 = parse_css("[data-x=no i] { color: red; }");
        let node2 = brief("div", None, None, "data-x=\"NO\"", 1);
        assert!(has_color(&p4.rules, &ctx(&node2, &[], &[])));
        let p5 = parse_css("[data-x=no] { color: red; }");
        assert!(!has_color(&p5.rules, &ctx(&node2, &[], &[])));
        let p6 = parse_css("[data-x~=tok] { color: red; }");
        let node3 = brief("div", None, None, "data-x=\"aa tok bb\"", 1);
        assert!(has_color(&p6.rules, &ctx(&node3, &[], &[])));
        let p7 = parse_css("[data-x|=pre] { color: red; }");
        let node4 = brief("div", None, None, "data-x=\"pre-fix\"", 1);
        assert!(has_color(&p7.rules, &ctx(&node4, &[], &[])));
        let node5 = brief("div", None, None, "data-x=\"prefix\"", 1);
        assert!(!has_color(&p7.rules, &ctx(&node5, &[], &[])));
        let p8 = parse_css("[disabled] { color: red; }");
        let node6 = brief("input", None, None, "disabled type=\"text\"", 1);
        assert!(has_color(&p8.rules, &ctx(&node6, &[], &[])));
    }

    #[test]
    fn nth_child_and_first_child() {
        let parsed = parse_css(
            "li:nth-child(2) { color: red; }\n\
             li:nth-child(odd) { color: red; }\n\
             li:nth-child(2n+1) { color: red; }\n\
             li:first-child { color: red; }",
        );
        let second = brief("li", None, None, "", 2);
        assert!(has_color(&parsed.rules, &ctx(&second, &[], &[])));
        let third = brief("li", None, None, "", 3);
        // odd/2n+1 → 3 eşleşir
        assert!(has_color(&parsed.rules, &ctx(&third, &[], &[])));
        let fourth = brief("li", None, None, "", 4);
        assert!(!has_color(&parsed.rules, &ctx(&fourth, &[], &[])));
        let first = brief("li", None, None, "", 1);
        assert!(has_color(&parsed.rules, &ctx(&first, &[], &[])));
        // nth-of-type: type_index ayrı
        let p2 = parse_css("p:nth-of-type(2) { color: red; }");
        let mut n = brief("p", None, None, "", 5);
        n.type_index = 2;
        assert!(has_color(&p2.rules, &ctx(&n, &[], &[])));
        n.type_index = 3;
        assert!(!has_color(&p2.rules, &ctx(&n, &[], &[])));
    }

    #[test]
    fn sibling_and_ancestor_combinators() {
        // div > p kuralı: ebeveyn div olmalı
        let parsed = parse_css("div > p { color: red; }");
        let parent_div = NodeInfo::new(brief("div", None, None, "", 1));
        let ancestors: Vec<&NodeInfo> = vec![&parent_div];
        let p = brief("p", None, None, "", 1);
        assert!(has_color(&parsed.rules, &ctx(&p, &ancestors, &[])));
        let parent_span = NodeInfo::new(brief("span", None, None, "", 1));
        let ancestors2: Vec<&NodeInfo> = vec![&parent_span];
        assert!(!has_color(&parsed.rules, &ctx(&p, &ancestors2, &[])));

        // div p (torun): doğrudan ebeveyn span olsa bile ata div ise eşleşir
        let parsed2 = parse_css("div p { color: red; }");
        let gp = NodeInfo::new(brief("div", None, None, "", 1));
        let mid = NodeInfo::new(brief("span", None, None, "", 1));
        let ancestors3: Vec<&NodeInfo> = vec![&gp, &mid];
        assert!(has_color(&parsed2.rules, &ctx(&p, &ancestors3, &[])));
        let ancestors4: Vec<&NodeInfo> = vec![&mid];
        assert!(!has_color(&parsed2.rules, &ctx(&p, &ancestors4, &[])));

        // h1 + p: hemen önceki kardeş h1 olmalı
        let parsed3 = parse_css("h1 + p { color: red; }");
        let h1 = brief("h1", None, None, "", 1);
        let prevs = vec![h1];
        assert!(has_color(&parsed3.rules, &ctx(&p, &[], &prevs)));
        let span_prev = brief("span", None, None, "", 1);
        let prevs2 = vec![span_prev];
        assert!(!has_color(&parsed3.rules, &ctx(&p, &[], &prevs2)));

        // h1 ~ p: önceki HERHANGİ bir kardeş h1 yeterli
        let parsed4 = parse_css("h1 ~ p { color: red; }");
        let h1b = brief("h1", None, None, "", 1);
        let mid_sib = brief("em", None, None, "", 2);
        let prevs3 = vec![h1b, mid_sib];
        assert!(has_color(&parsed4.rules, &ctx(&p, &[], &prevs3)));
        let prevs4 = vec![
            brief("em", None, None, "", 1),
            brief("b", None, None, "", 2),
        ];
        assert!(!has_color(&parsed4.rules, &ctx(&p, &[], &prevs4)));
    }

    #[test]
    fn not_is_where_nested() {
        let parsed = parse_css("p:not(.lead) { color: red; }");
        let plain = brief("p", Some("body"), None, "", 1);
        assert!(has_color(&parsed.rules, &ctx(&plain, &[], &[])));
        let lead = brief("p", Some("lead"), None, "", 1);
        assert!(!has_color(&parsed.rules, &ctx(&lead, &[], &[])));

        let parsed2 = parse_css("p:is(.a, .b) { color: red; }");
        assert_eq!(parsed2.rules.len(), 1);
        assert_eq!(parsed2.rules[0].selector, "p:is(.a, .b)");
        let parsed2b = parse_css("p:is(.b) { color: red; }");
        assert!(has_color(
            &parsed2b.rules,
            &ctx(&brief("p", Some("b"), None, "", 1), &[], &[])
        ));
        assert!(has_color(
            &parsed2.rules,
            &ctx(&brief("p", Some("b"), None, "", 1), &[], &[])
        ));
        assert!(!has_color(
            &parsed2.rules,
            &ctx(&brief("p", Some("c"), None, "", 1), &[], &[])
        ));

        // :not içinde karmaşık seçici: ata bağlamı da değerlendirilir
        let parent = NodeInfo::new(brief("div", None, None, "", 1));
        let ancestors: Vec<&NodeInfo> = vec![&parent];
        let parsed3 = parse_css("p:not(div span) { color: red; }");
        let p = brief("p", None, None, "", 1);
        // div ata altında p → "div span" eşleşmez (p span değil) → :not true
        assert!(has_color(&parsed3.rules, &ctx(&p, &ancestors, &[])));
        let parsed4 = parse_css("p:not(div p) { color: red; }");
        // div ata + p özne → "div p" eşleşir → :not false
        assert!(!has_color(&parsed4.rules, &ctx(&p, &ancestors, &[])));
    }

    #[test]
    fn root_and_universal() {
        let parsed = parse_css(":root { color: red; }");
        let doc = NodeInfo::new(brief("#document", None, None, "", 0));
        let ancestors: Vec<&NodeInfo> = vec![&doc];
        let html = brief("html", None, None, "", 1);
        assert!(has_color(&parsed.rules, &ctx(&html, &ancestors, &[])));
        let body = brief("body", None, None, "", 1);
        assert!(!has_color(&parsed.rules, &ctx(&body, &ancestors, &[])));

        let parsed2 = parse_css("* { color: red; }");
        assert!(has_color(
            &parsed2.rules,
            &ctx(&brief("any", None, None, "", 1), &[], &[])
        ));
    }
}

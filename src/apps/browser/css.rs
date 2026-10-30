//! CSS ayrıştırıcı ve stil eşleştirici (no_std).
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use super::types::{html_color, ElementStyle, GridTrack, TextAlign};

#[derive(Clone)]
pub struct CssRule {
    pub selector: String,
    pub style: ElementStyle,
    /// İsteğe bağlı @media koşulu (örnek: "screen and (min-width: 600px)").
    /// None ise kural her zaman etkindir.
    #[allow(dead_code)]
    pub media: Option<String>,
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
        Some(px) => {
            if px >= 20 {
                2
            } else {
                1
            }
        }
        None => 1,
    }
}

fn is_bold_weight(val: &str) -> bool {
    let l = val.trim().to_ascii_lowercase();
    if l == "bold" || l == "bolder" || l == "bolder " {
        return true;
    }
    l.parse::<u32>().map_or(false, |w| w >= 600)
}

/// Tek bir `prop: value` bildirimini stile uygular.
#[allow(unreachable_patterns)]
fn apply_css_decl(style: &mut ElementStyle, prop: &str, raw_val: &str) {
    let mut val = raw_val.trim();
    if let Some(stripped) = val.strip_suffix("!important") {
        val = stripped.trim();
    }
    match prop {
        "color" => {
            if let Some(c) = html_color(val.as_bytes()) {
                style.color = Some(c);
            }
        }
        "background-color" | "background" => {
            if let Some(c) = html_color(val.as_bytes()) {
                style.bg_color = Some(c);
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
        "display" => {
            if val.eq_ignore_ascii_case("none") {
                style.display_none = true;
            }
        }
        "margin-left" | "padding-left" | "text-indent" => {
            // Varsayılan: viewport ve parent bilgis yok, direkt px/em/rem parse et
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.indent_px = px;
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
        "overflow" => {
            //overflow stilleri basitçe göz ardı ediliyor veya varsayılan ayarlanıyor
        }
        "white-space" => {
            // white-space stilleri
            if val.eq_ignore_ascii_case("nowrap") {
                style.no_wrap = true;
            } else if val.eq_ignore_ascii_case("pre") {
                style.pre_formatted = true;
            }
        }
        "border" => {
            // Sınır stilleri basitçe işleniyor
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {}
        "border-width" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
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
        "background-image" => {
            // Background image - basit tanımla
        }
        "background-gradient" => {
            // Background gradient - gelecek geliştirme
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
            }
        }
        "grid-area" => {
            // grid-area: row-start / column-start / row-end / column-end
            // Basit implementasyon: sadece ilk iki degeri kaydet (satir, sutun baslangici)
            let parts: Vec<&str> = val.split('/').collect();
            if let Some(first) = parts.first() {
                if let Ok(_n) = first.trim().parse::<usize>() {
                    // grid-area basit tutuluyor, html.rs'te kullanilacak
                }
            }
        }
        "grid-column" => {
            // grid-column: start / end
        }
        "grid-row" => {
            // grid-row: start / end
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

/// Seçicileri ve bildirim bloklarını ayrıştırır; `@media` gibi iç içe bloklar recursive işlenir.
fn parse_css_rules(
    bytes: &[u8],
    rules: &mut Vec<CssRule>,
    keyframes: &mut Vec<Keyframes>,
    current_media: Option<&str>,
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
        while p < bytes.len() && bytes[p] != b'{' && bytes[p] != b'<' {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'{' {
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
            // @media / @supports / @layer: içindeki kuralları uygula.
            // @font-face, @import: atlanır.
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
                parse_css_rules(block.as_bytes(), rules, keyframes, combined.as_deref());
            } else if at == "@supports" || at == "@layer" || at == "@container" {
                parse_css_rules(block.as_bytes(), rules, keyframes, current_media);
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
                            for decl in block_content.split(';') {
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
                }
            }
            continue;
        }

        let mut style = ElementStyle::new();
        for decl in block.split(';') {
            if let Some((prop, val)) = decl.split_once(':') {
                let prop = prop.trim().to_ascii_lowercase();
                apply_css_decl(&mut style, &prop, val);
            }
        }
        for s in selector.split(',') {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                rules.push(CssRule {
                    selector: String::from(trimmed),
                    style: style.clone(),
                    media: None,
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
    parse_css_rules(
        strip_css_comments(css_code).as_bytes(),
        &mut rules,
        &mut keyframes,
        None,
    );
    ParsedCss { rules, keyframes }
}

/// CSS özgüllük (specificity) skoru: (ID sayısı, sınıf/özellik sayısı, etiket sayısı).
/// W3C spesifikasyonuna göre hesaplanır; yüksek skor kazanıcı.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

/// Bir CSS seçicisinin özgüllük skorunu hesaplar.
fn selector_specificity(sel: &str) -> Specificity {
    let mut a = 0u32;
    let mut b = 0u32;
    let mut c = 0u32;
    let s = sel.trim();
    // Virgülle ayrılmış çoklu seçiciler: her birinin en yüksek skoru alınır.
    for part in s.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let mut pa = 0u32;
        let mut pb = 0u32;
        let mut pc = 0u32;
        let mut chars = p.chars().peekable();
        while let Some(&ch) = chars.peek() {
            match ch {
                '#' => {
                    pa += 1;
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2.is_ascii_alphanumeric() || c2 == '-' || c2 == '_' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                '.' => {
                    pb += 1;
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2.is_ascii_alphanumeric() || c2 == '-' || c2 == '_' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                '[' => {
                    pb += 1;
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2 == ']' {
                            chars.next();
                            break;
                        } else {
                            chars.next();
                        }
                    }
                }
                ':' => {
                    chars.next();
                    if let Some(&c2) = chars.peek() {
                        if c2 == ':' {
                            chars.next();
                            // Sözde-element: ::before, ::after vb. → c++
                            pc += 1;
                        } else {
                            // Sözde-sınıf: :hover, :nth-child() vb. → b++
                            pb += 1;
                        }
                        while let Some(&c3) = chars.peek() {
                            if c3.is_ascii_alphanumeric()
                                || c3 == '-'
                                || c3 == '_'
                                || c3 == '('
                                || c3 == ')'
                            {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                '*' => {
                    chars.next(); // Evrensel seçici, 0 etki
                }
                _ if ch.is_ascii_alphabetic() => {
                    pc += 1;
                    chars.next();
                    while let Some(&c2) = chars.peek() {
                        if c2.is_ascii_alphanumeric() || c2 == '-' || c2 == '_' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        }
        a = a.max(pa);
        b = b.max(pb);
        c = c.max(pc);
    }
    Specificity::new(a, b, c)
}

/// CSS kuralı ile özgüllük skoru.
struct ScoredRule {
    specificity: Specificity,
    style: ElementStyle,
}

/// Bir @media koşulunu değerlendirir.
/// Desteklenen: screen, (min-width: Npx), (max-width: Npx), (min-height: Npx), (max-height: Npx), and
fn media_matches(cond: &str, viewport_w: usize, viewport_h: usize) -> bool {
    let c = cond.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("screen") || c.eq_ignore_ascii_case("all") {
        return true;
    }

    // "and" ile ayrılmış parçalar
    for part in c.split("and") {
        let part = part.trim();
        if part.is_empty()
            || part.eq_ignore_ascii_case("screen")
            || part.eq_ignore_ascii_case("all")
        {
            continue;
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

            let ok = match prop.as_str() {
                "min-width" => viewport_w >= px,
                "max-width" => viewport_w <= px,
                "min-height" => viewport_h >= px,
                "max-height" => viewport_h <= px,
                _ => true,
            };
            if !ok {
                return false;
            }
        }
    }
    true
}

/// CSS Cascade: kuralları özgüllüğe göre sıralar ve en yüksek skoru uygular.
/// `media` ile filtreleme yapılır; eşleşmeyen @media kuralları atlanır.
/// Aynı özgüllükteyse sonra gelen kural kazanır (son kazanır kuralı).
fn apply_css_cascade(
    rules: &[CssRule],
    tag: &str,
    class: Option<&str>,
    id: Option<&str>,
    viewport_w: usize,
    viewport_h: usize,
) -> ElementStyle {
    let mut scored: Vec<ScoredRule> = Vec::new();
    for rule in rules {
        // @media filtresi
        if let Some(ref media) = rule.media {
            if !media_matches(media, viewport_w, viewport_h) {
                continue;
            }
        }

        let sel = rule.selector.as_str();
        let matches = if sel.starts_with('.') {
            class.map_or(false, |c| {
                c.split_whitespace()
                    .any(|part| part.eq_ignore_ascii_case(&sel[1..]))
            })
        } else if sel.starts_with('#') {
            id.map_or(false, |i| i.eq_ignore_ascii_case(&sel[1..]))
        } else {
            sel.eq_ignore_ascii_case(tag)
        };
        if matches {
            scored.push(ScoredRule {
                specificity: selector_specificity(sel),
                style: rule.style.clone(),
            });
        }
    }

    // Özgüllüğe göre sırala (aşşağıdan yukarıya); eşit skorlarda orijinal sırayı koru (stabil).
    scored.sort_by(|a, b| a.specificity.cmp(&b.specificity));

    let mut resolved = ElementStyle::new();
    for sr in scored {
        resolved.merge_from(&sr.style);
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
    let mut resolved = apply_css_cascade(rules, tag, class, id, viewport_w, viewport_h);
    if let Some(inline) = inline_style {
        for decl in inline.split(';') {
            if let Some((prop, val)) = decl.split_once(':') {
                let prop = prop.trim().to_ascii_lowercase();
                apply_css_decl(&mut resolved, &prop, val);
            }
        }
    }
    resolved
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

    // p son karenin üzerindeyse son kareyi döndür
    kf.frames.last().unwrap().style.clone()
}

/// İki stil arasında lineer interpolasyon (sadece opacity için şimdilik).
fn blend_styles(a: &ElementStyle, b: &ElementStyle, t: f32) -> ElementStyle {
    let mut result = a.clone();
    result.opacity = a.opacity + (b.opacity - a.opacity) * t;
    result
}

/// CSS geçiş (transition) ilerlemesini hesaplar.
/// `elapsed_ms` geçişin başlangıcından geçen süre, `duration_ms` geçiş süresi.
/// Döndürülen değer 0.0..=1.0 arası ilerleme.
#[allow(dead_code)]
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

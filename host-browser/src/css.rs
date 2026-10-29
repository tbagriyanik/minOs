//! CSS ayrıştırıcı ve stil eşleştirici (no_std/alloc).
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::types::{html_color, ElementStyle, GridTrack, TextAlign};

#[derive(Clone)]
pub struct CssRule {
    pub selector: String,
    pub style: ElementStyle,
}

#[derive(Clone)]
pub struct Keyframe {
    pub percent: f32,
    pub style: ElementStyle,
}

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
        "em" => Some((n * 16.0f32) as usize),
        "rem" => Some((n * 16.0f32) as usize),
        "%" => {
            let parent_w = parent_width.unwrap_or(100);
            if parent_w > 0 {
                Some((n * parent_w as f32 / 100.0) as usize)
            } else {
                None
            }
        }
        "vh" => {
            let h = viewport_height.unwrap_or(768);
            Some((n * h as f32 / 100.0) as usize)
        }
        "vw" => {
            let w = viewport_width.unwrap_or(1024);
            Some((n * w as f32 / 100.0) as usize)
        }
        "pt" => Some((n * 1.25f32) as usize),
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
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.indent_px = px;
            }
        }
        "margin" | "padding" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            let left = match parts.len() {
                1 => parts.first().copied(),
                2 => parts.get(1).copied(),
                3 => parts.get(1).copied(),
                _ => parts.get(3).copied(),
            };
            if let Some(v) = left {
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
            if val.eq_ignore_ascii_case("normal") {
                style.line_height = 1.2;
            } else if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.line_height = px as f32 / 16.0;
            }
        }
        "overflow" => {}
        "white-space" => {
            if val.eq_ignore_ascii_case("nowrap") {
                style.no_wrap = true;
            } else if val.eq_ignore_ascii_case("pre") {
                style.pre_formatted = true;
            }
        }
        "border" => {}
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
            if val.eq_ignore_ascii_case("solid") {
                style.border_style = 1;
            } else if val.eq_ignore_ascii_case("dashed") {
                style.border_style = 2;
            } else if val.eq_ignore_ascii_case("dotted") {
                style.border_style = 3;
            }
        }
        "border-radius" => {
            if let Some(px) = parse_px_value(val, None, None, None, None) {
                style.border_radius = px;
            }
        }
        "box-shadow" => {
            if let Some(c) = html_color(val.as_bytes()) {
                style.box_shadow_color = Some(c);
            }
        }
        "text-shadow" => {
            if let Some(c) = html_color(val.as_bytes()) {
                style.text_shadow_color = Some(c);
            }
        }
        "display" => {
            if val.eq_ignore_ascii_case("flex") || val.eq_ignore_ascii_case("inline-flex") {
                style.display = Some(1);
            } else if val.eq_ignore_ascii_case("grid") || val.eq_ignore_ascii_case("inline-grid") {
                style.display = Some(2);
            } else if val.eq_ignore_ascii_case("block") {
                style.display = Some(3);
            } else if val.eq_ignore_ascii_case("inline") {
                style.display = Some(4);
            } else if val.eq_ignore_ascii_case("none") {
                style.display_none = true;
            } else if val.eq_ignore_ascii_case("inline-block") {
                style.display = Some(5);
            }
        }
        "position" => {
            if val.eq_ignore_ascii_case("static") {
                style.position = 0;
            } else if val.eq_ignore_ascii_case("relative") {
                style.position = 1;
            } else if val.eq_ignore_ascii_case("absolute") {
                style.position = 2;
            } else if val.eq_ignore_ascii_case("fixed") {
                style.position = 3;
            } else if val.eq_ignore_ascii_case("sticky") {
                style.position = 4;
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
        "background-image" => {}
        "background-gradient" => {}
        "animation" => {
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
                if *p == "infinite" || *p == "loop" || *p == "linear" || *p == "ease" {
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
        "grid-area" => {}
        "grid-column" => {}
        "grid-row" => {}
        _ => {}
    }
}

fn parse_grid_tracks(val: &str) -> Vec<GridTrack> {
    let mut tracks = Vec::new();
    let t = val.trim();

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

    for part in t.split_whitespace() {
        tracks.push(parse_single_grid_track(part));
    }
    tracks
}

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

fn parse_css_rules(bytes: &[u8], rules: &mut Vec<CssRule>, keyframes: &mut Vec<Keyframes>) {
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
            let at = selector.split_whitespace().next().unwrap_or("");
            if at == "@media" || at == "@supports" || at == "@layer" || at == "@container" {
                parse_css_rules(block.as_bytes(), rules, keyframes);
            } else if at == "@keyframes" {
                let kf_name = selector
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !kf_name.is_empty() && keyframes.len() < 64 {
                    let mut frames: Vec<Keyframe> = Vec::new();
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
    );
    ParsedCss { rules, keyframes }
}

pub fn match_css_style(
    rules: &[CssRule],
    tag: &str,
    class: Option<&str>,
    id: Option<&str>,
    inline_style: Option<&str>,
) -> ElementStyle {
    let mut resolved = ElementStyle::new();
    for rule in rules {
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
            resolved.merge_from(&rule.style);
        }
    }
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

pub fn find_keyframes<'a>(keyframes: &'a [Keyframes], name: &str) -> Option<&'a Keyframes> {
    keyframes.iter().find(|k| k.name == name)
}

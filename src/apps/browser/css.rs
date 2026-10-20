//! CSS ayrıştırıcı ve stil eşleştirici (no_std).
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use super::types::{html_color, ElementStyle, TextAlign};

#[derive(Clone)]
pub struct CssRule {
    pub selector: String,
    pub style: ElementStyle,
}

fn strip_css_comments(css: &str) -> String {
    let b = css.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut p = 0;
    while p < b.len() {
        if p + 1 < b.len() && b[p] == b'/' && b[p + 1] == b'*' {
            match b[p + 2..].windows(2).position(|w| w == b"*/") {
                Some(rel) => { p = p + 2 + rel + 2; continue; }
                None => break,
            }
        }
        out.push(b[p]);
        p += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// "16px" / "1.5em" / "24" gibi değerleri piksel cinsinden çözer.
fn parse_px_value(val: &str) -> Option<usize> {
    let t = val.trim().to_ascii_lowercase();
    let (num_str, mul) = if let Some(r) = t.strip_suffix("px") { (r, 1.0f32) }
        else if let Some(r) = t.strip_suffix("em") { (r, 16.0f32) }
        else if let Some(r) = t.strip_suffix("rem") { (r, 16.0f32) }
        else if t.ends_with('%') || t.ends_with("pt") || t.ends_with("vh") || t.ends_with("vw") { return None; }
        else { (t.as_str(), 1.0f32) };
    let n: f32 = num_str.trim().parse().ok()?;
    if !n.is_finite() || n < 0.0 { return None; }
    Some((n * mul) as usize)
}

fn font_scale_from_size(val: &str) -> usize {
    let l = val.to_ascii_lowercase();
    if l.contains("xxx-large") || l.contains("xx-large") || l.contains("x-large") || l.contains("large") {
        return 2;
    }
    if l.contains("small") { return 1; }
    match parse_px_value(&l) {
        Some(px) => if px >= 20 { 2 } else { 1 },
        None => 1,
    }
}

fn is_bold_weight(val: &str) -> bool {
    let l = val.trim().to_ascii_lowercase();
    if l == "bold" || l == "bolder" || l == "bolder " { return true; }
    l.parse::<u32>().map_or(false, |w| w >= 600)
}

/// Tek bir `prop: value` bildirimini stile uygular.
fn apply_css_decl(style: &mut ElementStyle, prop: &str, raw_val: &str) {
    let mut val = raw_val.trim();
    if let Some(stripped) = val.strip_suffix("!important") {
        val = stripped.trim();
    }
    match prop {
        "color" => { if let Some(c) = html_color(val.as_bytes()) { style.color = Some(c); } }
        "background-color" | "background" => {
            if let Some(c) = html_color(val.as_bytes()) { style.bg_color = Some(c); }
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
        "display" => { if val.eq_ignore_ascii_case("none") { style.display_none = true; } }
        "margin-left" | "padding-left" | "text-indent" => {
            if let Some(px) = parse_px_value(val) { style.indent_px = px; }
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
                if let Some(px) = parse_px_value(v) { style.indent_px = px; }
            }
        }
        "font-weight" => style.bold = is_bold_weight(val),
        "text-decoration" | "text-decoration-line" => {
            if val.contains("underline") { style.underline = true; }
        }
        _ => {}
    }
}

/// Seçicileri ve bildirim bloklarını ayrıştırır; `@media` gibi iç içe bloklar recursive işlenir.
fn parse_css_rules(bytes: &[u8], rules: &mut Vec<CssRule>) {
    let mut p = 0;
    while p < bytes.len() {
        while p < bytes.len() && bytes[p].is_ascii_whitespace() { p += 1; }
        if p >= bytes.len() { break; }
        let sel_start = p;
        while p < bytes.len() && bytes[p] != b'{' && bytes[p] != b'<' { p += 1; }
        if p >= bytes.len() || bytes[p] != b'{' { break; }
        let selector = core::str::from_utf8(&bytes[sel_start..p]).unwrap_or("").trim();

        // Eşleşen süslü parantezi bul (iç içe bloklar için).
        let mut depth = 0usize;
        let mut block_end = bytes.len();
        let mut i = p;
        while i < bytes.len() {
            if bytes[i] == b'{' { depth += 1; }
            else if bytes[i] == b'}' {
                depth = depth.saturating_sub(1);
                if depth == 0 { block_end = i; break; }
            }
            i += 1;
        }
        let block = core::str::from_utf8(&bytes[p + 1..block_end]).unwrap_or("");
        p = if block_end < bytes.len() { block_end + 1 } else { bytes.len() };

        if selector.starts_with('@') {
            // @media / @supports / @layer: içindeki kuralları uygula.
            // @font-face, @keyframes, @import: atlanır.
            let at = selector.split_whitespace().next().unwrap_or("");
            if at == "@media" || at == "@supports" || at == "@layer" || at == "@container" {
                parse_css_rules(block.as_bytes(), rules);
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

pub fn parse_css(css_code: &str) -> Vec<CssRule> {
    let mut rules = Vec::new();
    parse_css_rules(strip_css_comments(css_code).as_bytes(), &mut rules);
    rules
}

pub fn match_css_style(rules: &[CssRule], tag: &str, class: Option<&str>, id: Option<&str>, inline_style: Option<&str>) -> ElementStyle {
    let mut resolved = ElementStyle::new();
    for rule in rules {
        let sel = rule.selector.as_str();
        let matches = if sel.starts_with('.') {
            class.map_or(false, |c| c.split_whitespace().any(|part| part.eq_ignore_ascii_case(&sel[1..])))
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
//! HTML tokenizer ve kelime sarma tabanlı yerleşim motoru (no_std).
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::cache::{cache_get_with_metadata, cache_store_with_metadata};
use super::css::blend_styles;
use super::css::transition_progress;
use super::css::{find_keyframes, match_css_style, parse_css};
use super::types::{BrowserLine, ElementStyle, HtmlTagType, TextAlign, TextSpan};
use crate::fs::vfs::VirtualFileSystem;
use crate::gui::{Color, MinGuiCompositor};
use alloc::collections::BTreeMap;

pub fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
pub fn eq_ascii(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Base64 decode (standard alphabet, with padding support). Whitespace is ignored.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for c in input.chars() {
        let val = match c {
            'A'..='Z' => c as u32 - b'A' as u32,
            'a'..='z' => c as u32 - b'a' as u32 + 26,
            '0'..='9' => c as u32 - b'0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            _ if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1u32 << bits) - 1;
        }
    }
    Some(out)
}

/// Renk u32'undan opacity ile (0.0-1.0) blend edilerek yeni renk dondurur.
fn blend_color_with_opacity(color: u32, opacity: f32) -> u32 {
    if opacity >= 1.0 {
        return color;
    }
    if opacity <= 0.0 {
        return 0x101010;
    }
    let r = (((color >> 16) & 0xFF) as f32) * opacity;
    let g = (((color >> 8) & 0xFF) as f32) * opacity;
    let b = ((color & 0xFF) as f32) * opacity;
    let ar = (r + 0.5) as u8;
    let ag = (g + 0.5) as u8;
    let ab = (b + 0.5) as u8;
    ((ar as u32) << 16) | ((ag as u32) << 8) | (ab as u32)
}

fn extract_attr<'a>(attrs: &'a [u8], key: &[u8]) -> Option<&'a str> {
    let mut i = 0;
    while i < attrs.len() {
        while i < attrs.len() && (attrs[i].is_ascii_whitespace() || attrs[i] == b'/') {
            i += 1;
        }
        let k_start = i;
        while i < attrs.len() && (attrs[i].is_ascii_alphanumeric() || attrs[i] == b'-') {
            i += 1;
        }
        let cur_key = &attrs[k_start..i];
        while i < attrs.len() && attrs[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < attrs.len() && attrs[i] == b'=' {
            i += 1;
            while i < attrs.len() && attrs[i].is_ascii_whitespace() {
                i += 1;
            }
            let quote = if i < attrs.len() && (attrs[i] == b'\'' || attrs[i] == b'"') {
                let q = attrs[i];
                i += 1;
                q
            } else {
                0
            };
            let v_start = i;
            while i < attrs.len()
                && ((quote != 0 && attrs[i] != quote)
                    || (quote == 0 && !attrs[i].is_ascii_whitespace() && attrs[i] != b'>'))
            {
                i += 1;
            }
            let val = &attrs[v_start..i];
            if quote != 0 && i < attrs.len() {
                i += 1;
            }
            if eq_ascii(cur_key, key) {
                return core::str::from_utf8(val).ok();
            }
        } else if i == k_start {
            i += 1;
        }
    }
    None
}

/// Robust UTF-8, HTML entity, and Turkish ISO-8859-9/Windows-1254 character decoder
fn decode_html_char(bytes: &[u8], p: usize) -> (char, usize) {
    if p >= bytes.len() {
        return ('\0', 0);
    }
    let b0 = bytes[p];

    // HTML Entity &...;
    if b0 == b'&' {
        let max_e = (p + 12).min(bytes.len());
        if let Some(pos) = bytes[p..max_e].iter().position(|&b| b == b';') {
            let entity = &bytes[p + 1..p + pos];
            let ent_str = core::str::from_utf8(entity).unwrap_or("");
            let resolved = match ent_str {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                // Turkish HTML Entities
                "ccedil" => Some('ç'),
                "Ccedil" => Some('Ç'),
                "ouml" => Some('ö'),
                "Ouml" => Some('Ö'),
                "uuml" => Some('ü'),
                "Uuml" => Some('Ü'),
                "gbreve" => Some('ğ'),
                "Gbreve" => Some('Ğ'),
                "scedil" => Some('ş'),
                "Scedil" => Some('Ş'),
                "inodot" => Some('ı'),
                "Idot" => Some('İ'),
                _ => {
                    if ent_str.starts_with('#') {
                        if ent_str.starts_with("#x") || ent_str.starts_with("#X") {
                            u32::from_str_radix(&ent_str[2..], 16)
                                .ok()
                                .and_then(core::char::from_u32)
                        } else {
                            ent_str[1..]
                                .parse::<u32>()
                                .ok()
                                .and_then(core::char::from_u32)
                        }
                    } else {
                        None
                    }
                }
            };
            if let Some(ch) = resolved {
                return (ch, pos + 1);
            }
        }
    }

    // Standard UTF-8 decoding
    if b0 < 0x80 {
        return (b0 as char, 1);
    } else if (b0 & 0xE0) == 0xC0 && p + 1 < bytes.len() {
        let b1 = bytes[p + 1];
        if (b1 & 0xC0) == 0x80 {
            let cp = (((b0 & 0x1F) as u32) << 6) | ((b1 & 0x3F) as u32);
            if let Some(ch) = core::char::from_u32(cp) {
                return (ch, 2);
            }
        }
    } else if (b0 & 0xF0) == 0xE0 && p + 2 < bytes.len() {
        let b1 = bytes[p + 1];
        let b2 = bytes[p + 2];
        if (b1 & 0xC0) == 0x80 && (b2 & 0xC0) == 0x80 {
            let cp =
                (((b0 & 0x0F) as u32) << 12) | (((b1 & 0x3F) as u32) << 6) | ((b2 & 0x3F) as u32);
            if let Some(ch) = core::char::from_u32(cp) {
                return (ch, 3);
            }
        }
    } else if (b0 & 0xF8) == 0xF0 && p + 3 < bytes.len() {
        let b1 = bytes[p + 1];
        let b2 = bytes[p + 2];
        let b3 = bytes[p + 3];
        if (b1 & 0xC0) == 0x80 && (b2 & 0xC0) == 0x80 && (b3 & 0xC0) == 0x80 {
            let cp = (((b0 & 0x07) as u32) << 18)
                | (((b1 & 0x3F) as u32) << 12)
                | (((b2 & 0x3F) as u32) << 6)
                | ((b3 & 0x3F) as u32);
            if let Some(ch) = core::char::from_u32(cp) {
                return (ch, 4);
            }
        }
    }

    // Fallback: Windows-1254 / ISO-8859-9 Turkish single-byte mapping
    let ch = match b0 {
        0xD0 => 'Ğ',
        0xF0 => 'ğ',
        0xDD => 'İ',
        0xFD => 'ı',
        0xDE => 'Ş',
        0xFE => 'ş',
        0xC7 => 'Ç',
        0xE7 => 'ç',
        0xD6 => 'Ö',
        0xF6 => 'ö',
        0xDC => 'Ü',
        0xFC => 'ü',
        _ => b0 as char,
    };
    (ch, 1)
}

pub fn decode_bytes_to_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut p = 0;
    while p < bytes.len() {
        let (ch, len) = decode_html_char(bytes, p);
        if len == 0 {
            break;
        }
        out.push(ch);
        p += len;
    }
    out
}

/// Solves relative/absolute URLs against an optional base URL.
pub fn resolve_url(src: &str, base_url: Option<&str>) -> Option<String> {
    let src = src.trim();
    if src.is_empty() {
        return None;
    }
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
        return Some(String::from(src));
    }
    let base = base_url?.trim();
    if src.starts_with("//") {
        let scheme = if base.starts_with("https://") {
            "https:"
        } else {
            "http:"
        };
        return Some(format!("{}{}", scheme, src));
    }
    if src.starts_with('/') {
        if let Some(scheme_end) = base.find("://") {
            let rest = &base[scheme_end + 3..];
            let host_end = rest.find('/').unwrap_or(rest.len());
            let authority = &base[..scheme_end + 3 + host_end];
            return Some(format!("{}{}", authority, src));
        }
        return None;
    }
    let src_clean = if src.starts_with("./") {
        &src[2..]
    } else {
        src
    };
    if let Some(scheme_end) = base.find("://") {
        let rest = &base[scheme_end + 3..];
        let host_and_path = if let Some(q_or_h) = rest.find(|c| c == '?' || c == '#') {
            &rest[..q_or_h]
        } else {
            rest
        };
        if let Some(last_slash) = host_and_path.rfind('/') {
            let base_dir = &base[..scheme_end + 3 + last_slash + 1];
            return Some(format!("{}{}", base_dir, src_clean));
        } else {
            return Some(format!("{}/{}", base, src_clean));
        }
    }
    None
}

/// Scales RGB pixel array to target dimensions using nearest-neighbor sampling.
fn scale_rgb(
    orig_w: usize,
    orig_h: usize,
    orig_rgb: &[u8],
    target_w: usize,
    target_h: usize,
) -> Vec<u8> {
    if orig_w == target_w && orig_h == target_h {
        return orig_rgb.to_vec();
    }
    let mut scaled = Vec::with_capacity(target_w * target_h * 3);
    for ty in 0..target_h {
        let sy = (ty * orig_h) / target_h.max(1);
        for tx in 0..target_w {
            let sx = (tx * orig_w) / target_w.max(1);
            let idx = (sy * orig_w + sx) * 3;
            if idx + 2 < orig_rgb.len() {
                scaled.push(orig_rgb[idx]);
                scaled.push(orig_rgb[idx + 1]);
                scaled.push(orig_rgb[idx + 2]);
            } else {
                scaled.push(0);
                scaled.push(0);
                scaled.push(0);
            }
        }
    }
    scaled
}

/// Tokenizer, CSS rule applicator, and word-wrapping HTML layout engine
pub fn render_html_to_lines(
    body: &[u8],
    wrap_width_px: usize,
    out_lines: &mut Vec<BrowserLine>,
    max_lines: usize,
    base_url: Option<&str>,
    animation_start_ms: usize,
    transition_start_ms: usize,
    previous_styles: &mut BTreeMap<String, ElementStyle>,
    mut vfs: Option<&mut VirtualFileSystem>,
    canvases: &mut Vec<super::canvas::CanvasElement>,
    media_players: &mut super::media::MediaPlayers,
    iframes: &mut super::iframe::IFrames,
) {
    // JS: <script> bloklarını sırayla çalıştır; document.write çıktısı HTML'e gömülür,
    // script etiketleri ise çıktidan çıkarılır.
    let js_body = crate::apps::js::inline_scripts(body);
    let body: &[u8] = &js_body;
    let mut current_color = Color::BLACK;
    let mut current_bg: Option<u32> = None;
    let mut active_link: Option<String> = None;
    let mut active_onclick: Option<String> = None;
    let mut active_is_button = false;
    let mut current_bold = false;
    let mut current_underline = false;
    let mut active_id: Option<String> = None;
    let mut css_rules = Vec::new();
    let mut css_keyframes: Vec<super::css::Keyframes> = Vec::new();

    // 1. First extract all <style> blocks
    let mut p = 0;
    while p < body.len() {
        if body[p] == b'<' && p + 6 <= body.len() && eq_ascii(&body[p..p + 6], b"<style") {
            let start = p + 6;
            let mut close_tag = start;
            while close_tag < body.len() && body[close_tag] != b'>' {
                close_tag += 1;
            }
            if close_tag < body.len() {
                close_tag += 1;
                let css_start = close_tag;
                if let Some(pos) = find_bytes(&body[css_start..], b"</style>") {
                    let css_text = decode_bytes_to_string(&body[css_start..css_start + pos]);
                    let parsed = parse_css(&css_text);
                    css_rules.extend(parsed.rules);
                    css_keyframes = parsed.keyframes;
                    p = css_start + pos + 8;
                    continue;
                }
            }
        }
        p += 1;
    }

    // Buffer of spans for the line currently being built
    let mut cur_line_spans: Vec<TextSpan> = Vec::new();
    let mut cur_line_w: usize = 0;
    let mut cur_line_tag = HtmlTagType::Normal;
    let mut cur_line_align = TextAlign::Left;

    // Sol kenar boşluğu (CSS margin-left / padding-left) — kapanışta sıfırlanır.
    let cur_line_indent = core::cell::Cell::new(0usize);

    // CSS Grid durumu
    let mut in_grid = false;
    let mut grid_columns: Vec<super::types::GridTrack> = Vec::new();
    let mut grid_gap: usize = 0;
    let mut grid_cells: Vec<Vec<Vec<TextSpan>>> = Vec::new(); // satir -> hucreler -> spanlar
    let mut grid_current_row: usize = 0;
    let mut grid_current_col: usize = 0;
    let mut grid_col_count: usize = 0;

    let tag_indent = |t: HtmlTagType| -> usize {
        let base: usize = match t {
            HtmlTagType::ListItem => 18,
            _ => 0,
        };
        base.saturating_add(cur_line_indent.get())
    };

    let flush_line = |lines: &mut Vec<BrowserLine>,
                      spans: &mut Vec<TextSpan>,
                      line_w: &mut usize,
                      tag: HtmlTagType,
                      align: TextAlign,
                      max: usize| {
        if lines.len() < max && !spans.is_empty() {
            let indent = tag_indent(tag);
            lines.push(BrowserLine {
                spans: core::mem::take(spans),
                tag,
                indent,
                align,
            });
            *line_w = 0;
        }
    };

    let add_word = |lines: &mut Vec<BrowserLine>,
                    spans: &mut Vec<TextSpan>,
                    line_w: &mut usize,
                    word: &str,
                    color: u32,
                    bg_color: Option<u32>,
                    is_link: bool,
                    link_url: &str,
                    is_button: bool,
                    is_image: bool,
                    onclick: &str,
                    elem_id: &str,
                    bold: bool,
                    underline: bool,
                    tag: HtmlTagType,
                    align: TextAlign,
                    max: usize| {
        if word.is_empty() {
            return;
        }
        let scale = match tag {
            HtmlTagType::Heading1 => 2,
            HtmlTagType::Heading2 => 1,
            _ => 1,
        };
        let word_w = MinGuiCompositor::text_width(word, scale)
            + if is_button { 16 } else { 0 }
            + if is_image { 64 } else { 0 };
        let space_w = MinGuiCompositor::text_width(" ", scale);
        let indent = tag_indent(tag);
        let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);

        if *line_w > 0 && *line_w + space_w + word_w > eff_wrap {
            flush_line(lines, spans, line_w, tag, align, max);
        }

        if spans.is_empty() {
            *line_w = word_w;
            spans.push(TextSpan {
                text: String::from(word),
                color,
                bg_color,
                is_link,
                link_url: String::from(link_url),
                is_button,
                is_image,
                image_width: 0,
                image_height: 0,
                image_rgb: None,
                image_url: String::new(),
                bold,
                underline,
                onclick: String::from(onclick),
                elem_id: String::from(elem_id),
            });
        } else {
            *line_w += space_w + word_w;
            if let Some(last) = spans.last_mut() {
                if !is_button
                    && !is_image
                    && last.color == color
                    && last.bg_color == bg_color
                    && last.is_link == is_link
                    && last.link_url == link_url
                    && !last.is_button
                    && !last.is_image
                    && last.elem_id == elem_id
                    && last.bold == bold
                    && last.underline == underline
                {
                    last.text.push(' ');
                    last.text.push_str(word);
                    return;
                }
            }
            let mut s = String::from(if is_button { "" } else { " " });
            s.push_str(word);
            spans.push(TextSpan {
                text: s,
                color,
                bg_color,
                is_link,
                link_url: String::from(link_url),
                is_button,
                is_image,
                image_width: 0,
                image_height: 0,
                image_rgb: None,
                image_url: String::new(),
                bold,
                underline,
                onclick: String::from(onclick),
                elem_id: String::from(elem_id),
            });
        }
    };

    p = 0;
    let mut word_buf = String::new();
    let mut hidden_until_close: Option<Vec<u8>> = None;
    // İç içe liste durumu: (sıralı mı?, mevcut madde numarası)
    let mut list_stack: Vec<(bool, usize)> = Vec::new();

    let flush_word = |lines: &mut Vec<BrowserLine>,
                      spans: &mut Vec<TextSpan>,
                      line_w: &mut usize,
                      buf: &mut String,
                      color: u32,
                      bg: Option<u32>,
                      is_link: bool,
                      link_url: &str,
                      is_btn: bool,
                      is_img: bool,
                      onclick: &str,
                      elem_id: &str,
                      bold: bool,
                      underline: bool,
                      tag: HtmlTagType,
                      align: TextAlign,
                      max: usize| {
        if !buf.is_empty() {
            add_word(
                lines, spans, line_w, buf, color, bg, is_link, link_url, is_btn, is_img, onclick,
                elem_id, bold, underline, tag, align, max,
            );
            buf.clear();
        }
    };

    while p < body.len() && out_lines.len() < max_lines {
        if body[p] == b'<' {
            let start = p + 1;
            let mut end = start;
            while end < body.len() && body[end] != b'>' {
                end += 1;
            }
            if end == body.len() {
                break;
            }
            let mut tag_content = &body[start..end];
            let closing = tag_content.first() == Some(&b'/');
            if closing {
                tag_content = &tag_content[1..];
            }
            let name_end = tag_content
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'/')
                .unwrap_or(tag_content.len());
            let name = &tag_content[..name_end];

            if let Some(ref h) = hidden_until_close {
                if closing && eq_ascii(name, h) {
                    hidden_until_close = None;
                }
                p = end + 1;
                continue;
            }
            if !closing && (eq_ascii(name, b"script") || eq_ascii(name, b"style")) {
                hidden_until_close = Some(name.to_vec());
                p = end + 1;
                continue;
            }

            let link_target = active_link.as_deref().unwrap_or("");
            let is_link = active_link.is_some();
            let onclick_str = active_onclick.as_deref().unwrap_or("");
            let elem_id_str = active_id.as_deref().unwrap_or("");
            // CSS Grid modu: kelimeyi grid hucreye ekle
            if in_grid && !word_buf.is_empty() {
                while grid_cells.len() <= grid_current_row {
                    grid_cells.push(Vec::new());
                }
                while grid_cells[grid_current_row].len() <= grid_current_col {
                    grid_cells[grid_current_row].push(Vec::new());
                }
                let cell_spans = &mut grid_cells[grid_current_row][grid_current_col];
                cell_spans.push(TextSpan {
                    text: core::mem::take(&mut word_buf),
                    color: current_color,
                    bg_color: current_bg,
                    is_link,
                    link_url: String::from(link_target),
                    is_button: active_is_button,
                    is_image: false,
                    image_width: 0,
                    image_height: 0,
                    image_rgb: None,
                    image_url: String::new(),
                    bold: current_bold,
                    underline: current_underline,
                    onclick: String::from(onclick_str),
                    elem_id: String::from(elem_id_str),
                });
                grid_current_col += 1;
                if grid_current_col >= grid_col_count.max(1) {
                    grid_current_col = 0;
                    grid_current_row += 1;
                    grid_cells.push(Vec::new());
                }
            } else {
                flush_word(
                    out_lines,
                    &mut cur_line_spans,
                    &mut cur_line_w,
                    &mut word_buf,
                    current_color,
                    current_bg,
                    is_link,
                    link_target,
                    active_is_button,
                    false,
                    onclick_str,
                    elem_id_str,
                    current_bold,
                    current_underline,
                    cur_line_tag,
                    cur_line_align,
                    max_lines,
                );
            }

            let tag_name_str = core::str::from_utf8(name).unwrap_or("");
            let attrs = &tag_content[name_end..];
            let class_attr = extract_attr(attrs, b"class");
            let id_attr = extract_attr(attrs, b"id");
            let style_attr = extract_attr(attrs, b"style");
            let href_attr = extract_attr(attrs, b"href");
            let onclick_attr = extract_attr(attrs, b"onclick");

            let mut matched = match_css_style(
                &css_rules,
                tag_name_str,
                class_attr,
                id_attr,
                style_attr,
                wrap_width_px,
                768,
            );

            // CSS animasyonu: zaman tabanlı keyframe interpolasyonu
            if let Some(kf_name) = matched.animation_name.clone() {
                if let Some(kf) = find_keyframes(&css_keyframes, &kf_name) {
                    let elapsed =
                        crate::boot::time::main_monotonic_ms().saturating_sub(animation_start_ms);
                    if let Some(progress) = super::css::animation_progress(
                        elapsed as u64,
                        matched.animation_duration,
                        matched.animation_iterations,
                    ) {
                        let animated = super::css::interpolate_keyframes(kf, progress);
                        matched.opacity = animated.opacity;
                        if let Some(c) = animated.color {
                            matched.color = Some(c);
                        }
                        if let Some(bg) = animated.bg_color {
                            matched.bg_color = Some(bg);
                        }
                        // Animasyon devam etsin, animation_name'i sıfırlama
                    }
                }
            }

            // CSS geçiş (transition): önceki stil ile mevcut stil arasında interpolasyon
            if let Some(ref tp) = matched.transition_property {
                if matched.transition_duration > 0.0 {
                    let elem_id = String::from(id_attr.unwrap_or(""));
                    let now = crate::boot::time::main_monotonic_ms();
                    let elapsed = now.saturating_sub(transition_start_ms);

                    if let Some(prev) = previous_styles.get(&elem_id) {
                        let progress =
                            transition_progress(elapsed as u64, matched.transition_duration);
                        if let Some(p) = progress {
                            if p >= 1.0 {
                                // Geçiş tamamlandı, mevcut stili kaydı olarak güncelle
                                previous_styles.insert(elem_id, matched.clone());
                            } else {
                                // Önceki ve mevcut stil arasında interpolasyon yap
                                let blended = blend_styles(prev, &matched, p);
                                // Geçişte yalnızca transition_property'de belirtilen özellikleri interpolasyonla
                                let tp_str = tp.as_str();
                                let apply_all = tp_str == "all";
                                if tp_str == "opacity" || apply_all {
                                    matched.opacity = blended.opacity;
                                }
                                if tp_str == "color" || apply_all {
                                    matched.color = blended.color;
                                }
                                if tp_str == "background-color"
                                    || tp_str == "background"
                                    || apply_all
                                {
                                    matched.bg_color = blended.bg_color;
                                }
                                // Geçiş devam ediyor, mevcut stili sonraki kare için kaydet
                                previous_styles.insert(elem_id, matched.clone());
                            }
                        }
                    } else {
                        // İlk kez görülen element, mevcut stili kaydet
                        previous_styles.insert(elem_id, matched.clone());
                    }
                }
            }

            if !closing {
                if let Some(c) = matched.color {
                    current_color = blend_color_with_opacity(c, matched.opacity);
                }
                if let Some(bg) = matched.bg_color {
                    current_bg = Some(blend_color_with_opacity(bg, matched.opacity));
                }
                if matched.align != TextAlign::Left {
                    cur_line_align = matched.align;
                }
                // CSS sol kenar boşluğu
                cur_line_indent.set(matched.indent_px);
                current_bold = matched.bold;
                current_underline = matched.underline;
                // Varsayılan kullanıcı aracısı stilleri
                if eq_ascii(name, b"b") || eq_ascii(name, b"strong") || eq_ascii(name, b"th") {
                    current_bold = true;
                }
                if eq_ascii(name, b"u") || eq_ascii(name, b"ins") {
                    current_underline = true;
                }
                // display:none — içeriği gizle
                if matched.display_none && hidden_until_close.is_none() {
                    hidden_until_close = Some(name.to_vec());
                }

                // CSS Grid konteynerı başlat
                if matched.display == Some(2) && !in_grid {
                    in_grid = true;
                    grid_columns = matched.grid_template_columns.clone();
                    grid_gap = matched.grid_gap;
                    grid_cells = Vec::new();
                    grid_current_row = 0;
                    grid_current_col = 0;
                    grid_col_count = if grid_columns.is_empty() {
                        1
                    } else {
                        grid_columns.len()
                    };
                    // İlk satırı oluştur
                    grid_cells.push(Vec::new());
                    // Grid konteynerı için mevcut satırı temizle
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                }

                if eq_ascii(name, b"a") {
                    if let Some(href) = href_attr {
                        active_link = Some(String::from(href));
                    }
                    if let Some(oc) = onclick_attr {
                        active_onclick = Some(String::from(oc));
                    }
                } else if eq_ascii(name, b"link") {
                    // <link rel="stylesheet" href="..."> — harici CSS
                    let rel_attr = extract_attr(attrs, b"rel");
                    if rel_attr.map_or(false, |r| r.eq_ignore_ascii_case("stylesheet")) {
                        if let Some(href) = href_attr {
                            if let Some(resolved_url) = resolve_url(href, base_url) {
                                if resolved_url.starts_with("http://")
                                    || resolved_url.starts_with("https://")
                                {
                                    let mut css_bytes: Option<Vec<u8>> = None;
                                    // Try cache with metadata first
                                    let current_time =
                                        crate::boot::time::main_monotonic_ms() / 1000;
                                    if let Some((cached_data, cached_meta)) =
                                        cache_get_with_metadata(vfs.as_deref(), &resolved_url)
                                    {
                                        if cached_meta.is_fresh(current_time as u64) {
                                            css_bytes = Some(cached_data);
                                        } else {
                                            // Stale - revalidate
                                            let validation_headers =
                                                super::cache::build_validation_headers(
                                                    &cached_meta,
                                                );
                                            let mut http_buf = vec![0u8; 1_048_576];
                                            if let Ok((status, hlen, total)) =
                                                crate::drivers::http::http_get_with_cache(
                                                    &resolved_url,
                                                    &mut http_buf,
                                                    Some(&validation_headers),
                                                )
                                            {
                                                if status == 304 {
                                                    // Not modified - use cached data
                                                    css_bytes = Some(cached_data.clone());
                                                    // Update metadata
                                                    let _ = cache_store_with_metadata(
                                                        vfs.as_deref_mut(),
                                                        &resolved_url,
                                                        &cached_data,
                                                        &cached_meta,
                                                    );
                                                } else if (200..300).contains(&status)
                                                    && total > hlen
                                                {
                                                    let body_bytes =
                                                        &http_buf[hlen..total].to_vec();
                                                    css_bytes = Some(body_bytes.clone());
                                                    let headers_str =
                                                        decode_bytes_to_string(&http_buf[..hlen]);
                                                    let new_meta =
                                                        super::cache::parse_cache_headers(
                                                            &headers_str,
                                                            current_time as u64,
                                                        );
                                                    let _ = cache_store_with_metadata(
                                                        vfs.as_deref_mut(),
                                                        &resolved_url,
                                                        body_bytes,
                                                        &new_meta,
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        // Not in cache - fetch fresh
                                        let mut http_buf = vec![0u8; 1_048_576];
                                        if let Ok((status, hlen, total)) =
                                            crate::drivers::http::http_get(
                                                &resolved_url,
                                                &mut http_buf,
                                            )
                                        {
                                            if (200..300).contains(&status) && total > hlen {
                                                let body_bytes = &http_buf[hlen..total].to_vec();
                                                css_bytes = Some(body_bytes.clone());
                                                let headers_str =
                                                    decode_bytes_to_string(&http_buf[..hlen]);
                                                let meta = super::cache::parse_cache_headers(
                                                    &headers_str,
                                                    current_time as u64,
                                                );
                                                let _ = cache_store_with_metadata(
                                                    vfs.as_deref_mut(),
                                                    &resolved_url,
                                                    body_bytes,
                                                    &meta,
                                                );
                                            }
                                        }
                                    }
                                    if let Some(ref css_data) = css_bytes {
                                        if let Ok(css_text) = core::str::from_utf8(css_data) {
                                            let parsed = parse_css(css_text);
                                            css_rules.extend(parsed.rules);
                                            css_keyframes = parsed.keyframes;
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if eq_ascii(name, b"script") && !closing {
                    // <script src="..."> — harici JS
                    if let Some(src) = extract_attr(attrs, b"src") {
                        if let Some(resolved_url) = resolve_url(src, base_url) {
                            if resolved_url.starts_with("http://")
                                || resolved_url.starts_with("https://")
                            {
                                let current_time = crate::boot::time::main_monotonic_ms() / 1000;
                                let mut js_bytes: Option<Vec<u8>> = None;
                                if let Some((cached_data, cached_meta)) =
                                    cache_get_with_metadata(vfs.as_deref(), &resolved_url)
                                {
                                    if cached_meta.is_fresh(current_time as u64) {
                                        js_bytes = Some(cached_data);
                                    } else {
                                        // Stale - revalidate
                                        let validation_headers =
                                            super::cache::build_validation_headers(&cached_meta);
                                        let mut http_buf = vec![0u8; 1_048_576];
                                        if let Ok((status, hlen, total)) =
                                            crate::drivers::http::http_get_with_cache(
                                                &resolved_url,
                                                &mut http_buf,
                                                Some(&validation_headers),
                                            )
                                        {
                                            if status == 304 {
                                                // Not modified - use cached data
                                                js_bytes = Some(cached_data.clone());
                                                let _ = cache_store_with_metadata(
                                                    vfs.as_deref_mut(),
                                                    &resolved_url,
                                                    &cached_data,
                                                    &cached_meta,
                                                );
                                            } else if (200..300).contains(&status) && total > hlen {
                                                let body_bytes = &http_buf[hlen..total].to_vec();
                                                js_bytes = Some(body_bytes.clone());
                                                let headers_str =
                                                    decode_bytes_to_string(&http_buf[..hlen]);
                                                let new_meta = super::cache::parse_cache_headers(
                                                    &headers_str,
                                                    current_time as u64,
                                                );
                                                let _ = cache_store_with_metadata(
                                                    vfs.as_deref_mut(),
                                                    &resolved_url,
                                                    body_bytes,
                                                    &new_meta,
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    // Not in cache - fetch fresh
                                    let mut http_buf = vec![0u8; 1_048_576];
                                    if let Ok((status, hlen, total)) =
                                        crate::drivers::http::http_get(&resolved_url, &mut http_buf)
                                    {
                                        if (200..300).contains(&status) && total > hlen {
                                            let body_bytes = &http_buf[hlen..total].to_vec();
                                            js_bytes = Some(body_bytes.clone());
                                            let headers_str =
                                                decode_bytes_to_string(&http_buf[..hlen]);
                                            let meta = super::cache::parse_cache_headers(
                                                &headers_str,
                                                current_time as u64,
                                            );
                                            let _ = cache_store_with_metadata(
                                                vfs.as_deref_mut(),
                                                &resolved_url,
                                                body_bytes,
                                                &meta,
                                            );
                                        }
                                    }
                                }
                                if let Some(ref js_data) = js_bytes {
                                    if let Ok(js_text) = core::str::from_utf8(js_data) {
                                        // Harici JS'yi çalıştır (basit: document.write destekli)
                                        for stmt in js_text.split(';') {
                                            let stmt = stmt.trim();
                                            if !stmt.is_empty() {
                                                // execute_js_statement yerine doğrudan işle
                                                // Burada sadece document.write desteklenir
                                                if stmt.starts_with("document.write(")
                                                    && stmt.ends_with(')')
                                                {
                                                    let inner = &stmt[15..stmt.len() - 1];
                                                    // Basit string literal çıkarma
                                                    if let Some(_written) = inner
                                                        .strip_prefix('"')
                                                        .and_then(|s| s.strip_suffix('"'))
                                                    {
                                                        // document.write içeriğini HTML'e eklemek için
                                                        // Bu basit bir implementasyon; gerçek JS engine yok
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if eq_ascii(name, b"button") {
                    active_is_button = true;
                    if let Some(oc) = onclick_attr {
                        active_onclick = Some(String::from(oc));
                    }
                    if current_bg.is_none() {
                        current_bg = Some(0xE0E0E8);
                    }
                } else if eq_ascii(name, b"h1") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Heading1;
                    if matched.color.is_none() {
                        current_color = Color::BLACK;
                    }
                } else if eq_ascii(name, b"h2") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Heading2;
                    if matched.color.is_none() {
                        current_color = Color::BLACK;
                    }
                } else if eq_ascii(name, b"h3") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Heading3;
                    if matched.color.is_none() {
                        current_color = Color::BLACK;
                    }
                } else if eq_ascii(name, b"span") {
                    // <span>: inline container, stilleri uygulamak için kullanılır
                    // Renk/background/align/indent gibi CSS stilleri uygulanır
                    if matched.color.is_none() { /* span varsayılan renk uygulanmaz, ebeveyne bakılır */
                    }
                    if matched.align != TextAlign::Left {
                        cur_line_align = matched.align;
                    }
                    if matched.display_none && hidden_until_close.is_none() {
                        hidden_until_close = Some(name.to_vec());
                    }
                } else if eq_ascii(name, b"em") {
                    // <em>: vurgulu metin
                    if matched.color.is_none() {
                        current_color = Color::BLACK;
                    }
                    if matched.underline {
                        current_underline = true;
                    }
                } else if eq_ascii(name, b"mark") {
                    // <mark>:Highlighted metin
                    if current_bg.is_none() {
                        current_bg = Some(0xFFFF00); // Sarı background
                    }
                } else if eq_ascii(name, b"small") {
                    // <small>: Küçük metin
                    // font_scale azaltılabilir, basit implementasyonda etkisiz
                } else if eq_ascii(name, b"input") {
                    // <input>: Form girişi elementi
                    // type attribute'e göre farklı davranır
                    let type_attr = extract_attr(attrs, b"type");
                    let input_label = extract_attr(attrs, b"value")
                        .or_else(|| extract_attr(attrs, b"placeholder"))
                        .unwrap_or("input");

                    if type_attr.map_or(false, |t| t.eq_ignore_ascii_case("text"))
                        || type_attr.is_none()
                    {
                        // Tek satır text input
                        if !word_buf.is_empty() {
                            flush_word(
                                out_lines,
                                &mut cur_line_spans,
                                &mut cur_line_w,
                                &mut word_buf,
                                current_color,
                                current_bg,
                                is_link,
                                link_target,
                                active_is_button,
                                false,
                                onclick_str,
                                elem_id_str,
                                current_bold,
                                current_underline,
                                cur_line_tag,
                                cur_line_align,
                                max_lines,
                            );
                        }
                        word_buf.push('"');
                        for ch in input_label.chars() {
                            if !ch.is_control() {
                                word_buf.push(ch);
                            }
                        }
                        word_buf.push('"');
                    } else if type_attr.map_or(false, |t| t.eq_ignore_ascii_case("password")) {
                        // Password input - gizli metin
                        if !word_buf.is_empty() {
                            flush_word(
                                out_lines,
                                &mut cur_line_spans,
                                &mut cur_line_w,
                                &mut word_buf,
                                current_color,
                                current_bg,
                                is_link,
                                link_target,
                                active_is_button,
                                false,
                                onclick_str,
                                elem_id_str,
                                current_bold,
                                current_underline,
                                cur_line_tag,
                                cur_line_align,
                                max_lines,
                            );
                        }
                        word_buf.push('●');
                        for ch in input_label.chars() {
                            if !ch.is_control() {
                                word_buf.push(ch);
                            }
                        }
                        word_buf.push('●');
                    } else if type_attr.map_or(false, |t| t.eq_ignore_ascii_case("checkbox"))
                        || type_attr.map_or(false, |t| t.eq_ignore_ascii_case("radio"))
                    {
                        // Checkbox/radio - basit gösterim
                        if !word_buf.is_empty() {
                            flush_word(
                                out_lines,
                                &mut cur_line_spans,
                                &mut cur_line_w,
                                &mut word_buf,
                                current_color,
                                current_bg,
                                is_link,
                                link_target,
                                active_is_button,
                                false,
                                onclick_str,
                                elem_id_str,
                                current_bold,
                                current_underline,
                                cur_line_tag,
                                cur_line_align,
                                max_lines,
                            );
                        }
                        word_buf.push('☐'); // Checkbox simgesi
                    }
                } else if eq_ascii(name, b"textarea") {
                    // <textarea>: Çok satırlı text input
                    let textarea_content = extract_attr(attrs, b"placeholder").unwrap_or("");
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('"');
                    for ch in textarea_content.chars() {
                        if !ch.is_control() {
                            word_buf.push(ch);
                        }
                    }
                    word_buf.push('"');
                    word_buf.push('\n');
                } else if eq_ascii(name, b"select") {
                    // <select>: Dropdown liste
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('▼'); // Dropdown simgesi
                } else if eq_ascii(name, b"option") {
                    // <option>: Dropdown seçenekleri
                    let option_label = extract_attr(attrs, b"value")
                        .or_else(|| extract_attr(attrs, b"label"))
                        .unwrap_or("option");
                    word_buf.push(' ');
                    word_buf.push_str(option_label);
                } else if eq_ascii(name, b"label") {
                    // <label>: Form etiketi
                    let label_text = extract_attr(attrs, b"for")
                        .or_else(|| extract_attr(attrs, b"value"))
                        .unwrap_or("");
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('ℹ'); // Etiket simgesi
                    for ch in label_text.chars() {
                        if !ch.is_control() {
                            word_buf.push(ch);
                        }
                    }
                } else if eq_ascii(name, b"form") {
                    // <form>: Form elementi (basit gösterim)
                } else if eq_ascii(name, b"header") {
                    // <header>: Sayfa başlığı
                } else if eq_ascii(name, b"nav") {
                    // <nav>: Navigation menüsü
                } else if eq_ascii(name, b"main") {
                    // <main>: Ana içerik alanı
                } else if eq_ascii(name, b"article") {
                    // <article>: Makale/artikel
                } else if eq_ascii(name, b"aside") {
                    // <aside>: Yan/Ic sidebar
                    // <pre>: Önceden formatlanmış metin
                    // Boşluklar ve satır sonları korunur
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('\n');
                    word_buf.push('\n');
                    // Satır sonu karakterleri preservation
                    word_buf.push('\n');
                } else if eq_ascii(name, b"sub") {
                    // <sub>: Alt indeks
                    // Basit implementasyon: [ ] etiketleri gibi gösterilir
                } else if eq_ascii(name, b"sup") {
                    // <sup>: Üst indeks
                    // Basit implementasyon: [ ] etiketleri gibi gösterilir
                } else if eq_ascii(name, b"input") {
                    // <input> kapanıyor, normal metin moduna
                } else if eq_ascii(name, b"textarea") {
                    // <textarea> kapanıyor, normal metin moduna
                    word_buf.push('\n');
                } else if eq_ascii(name, b"select") {
                    // <select> kapanıyor, normal moduna
                } else if eq_ascii(name, b"option") {
                    // <option> kapanıyor, zaten metin eklendi
                } else if eq_ascii(name, b"label") {
                    // <label> kapanıyor, normal moduna
                } else if eq_ascii(name, b"form") {
                    // <form> kapanıyor
                } else if eq_ascii(name, b"header")
                    | eq_ascii(name, b"nav")
                    | eq_ascii(name, b"main")
                    | eq_ascii(name, b"article")
                    | eq_ascii(name, b"aside")
                {
                    // Semantic elementler kapanıyor, normal moduna
                } else if eq_ascii(name, b"table") {
                    // <table>: Tablo elementi
                } else if eq_ascii(name, b"tr") {
                    // <tr>: Satir elementi
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('|'); // Satir ayracı
                } else if eq_ascii(name, b"th") {
                    // <th>: Tablo baslik hucresi
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('['); // Baslik simgesi
                    word_buf.push('[');
                } else if eq_ascii(name, b"td") {
                    // <td>: Tablo veri hucresi
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push(']'); // Veri simgesi
                } else if eq_ascii(name, b"caption") {
                    // <caption>: Tablo basligi
                } else if eq_ascii(name, b"img") {
                    // <img>: Görsel işleme
                    let label = extract_attr(attrs, b"alt")
                        .or_else(|| extract_attr(attrs, b"title"))
                        .or_else(|| extract_attr(attrs, b"src"))
                        .unwrap_or("görsel");
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Decode image from data URL or remote HTTP URL.
                    let mut image_decoded: Option<(usize, usize, Vec<u8>)> = None;
                    let mut image_src_url: Option<String> = None;
                    if let Some(src) = extract_attr(attrs, b"src") {
                        image_src_url = Some(String::from(src));
                        if src.starts_with("data:image/") {
                            if let Some(comma_pos) = src.find(',') {
                                let data_part = &src[comma_pos + 1..];
                                let is_base64 = src[..comma_pos].contains("base64");
                                if is_base64 {
                                    if let Some(decoded_bytes) = base64_decode(data_part) {
                                        if let Some(img) =
                                            crate::apps::imgdecode::decode(&decoded_bytes)
                                        {
                                            image_decoded = Some((img.width, img.height, img.rgb));
                                        }
                                    }
                                }
                            }
                        } else if let Some(resolved_url) = resolve_url(src, base_url) {
                            if resolved_url.starts_with("http://")
                                || resolved_url.starts_with("https://")
                            {
                                // Önbellekten dene önce (metadata ile)
                                let current_time = crate::boot::time::main_monotonic_ms() / 1000;
                                let mut img_bytes: Option<Vec<u8>> = None;
                                if let Some((cached_data, cached_meta)) =
                                    cache_get_with_metadata(vfs.as_deref(), &resolved_url)
                                {
                                    if cached_meta.is_fresh(current_time as u64) {
                                        img_bytes = Some(cached_data);
                                    } else {
                                        // Stale - revalidate
                                        let validation_headers =
                                            super::cache::build_validation_headers(&cached_meta);
                                        let mut http_buf = vec![0u8; 1_048_576];
                                        if let Ok((status, hlen, total)) =
                                            crate::drivers::http::http_get_with_cache(
                                                &resolved_url,
                                                &mut http_buf,
                                                Some(&validation_headers),
                                            )
                                        {
                                            if status == 304 {
                                                // Not modified - use cached data
                                                img_bytes = Some(cached_data.clone());
                                                let _ = cache_store_with_metadata(
                                                    vfs.as_deref_mut(),
                                                    &resolved_url,
                                                    &cached_data,
                                                    &cached_meta,
                                                );
                                            } else if (200..300).contains(&status) && total > hlen {
                                                let body_bytes = &http_buf[hlen..total].to_vec();
                                                img_bytes = Some(body_bytes.clone());
                                                let headers_str =
                                                    decode_bytes_to_string(&http_buf[..hlen]);
                                                let new_meta = super::cache::parse_cache_headers(
                                                    &headers_str,
                                                    current_time as u64,
                                                );
                                                let _ = cache_store_with_metadata(
                                                    vfs.as_deref_mut(),
                                                    &resolved_url,
                                                    body_bytes,
                                                    &new_meta,
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    // Not in cache - fetch fresh
                                    let mut http_buf = vec![0u8; 1_048_576];
                                    if let Ok((status, hlen, total)) =
                                        crate::drivers::http::http_get(&resolved_url, &mut http_buf)
                                    {
                                        if (200..300).contains(&status) && total > hlen {
                                            let body_bytes = &http_buf[hlen..total].to_vec();
                                            img_bytes = Some(body_bytes.clone());
                                            let headers_str =
                                                decode_bytes_to_string(&http_buf[..hlen]);
                                            let meta = super::cache::parse_cache_headers(
                                                &headers_str,
                                                current_time as u64,
                                            );
                                            let _ = cache_store_with_metadata(
                                                vfs.as_deref_mut(),
                                                &resolved_url,
                                                body_bytes,
                                                &meta,
                                            );
                                        }
                                    }
                                }
                                if let Some(ref img_data) = img_bytes {
                                    if let Some(img) = crate::apps::imgdecode::decode(img_data) {
                                        image_decoded = Some((img.width, img.height, img.rgb));
                                    }
                                }
                            }
                        }
                    }

                    // Calculate display size (max 200px wide, maintain aspect ratio)
                    let (disp_w, disp_h) = image_decoded.as_ref().map_or((64, 64), |(w, h, _)| {
                        let max_w = 200usize;
                        if *w > max_w {
                            let scale = max_w as f32 / *w as f32;
                            (max_w, ((*h as f32 * scale) as usize).max(1))
                        } else {
                            (*w, *h)
                        }
                    });

                    // Pre-downscale image pixels to display size for crisp rendering and smaller memory footprint
                    let pre_scaled_rgb = image_decoded
                        .as_ref()
                        .map(|(w, h, rgb)| scale_rgb(*w, *h, rgb, disp_w, disp_h));

                    // Check if image fits on current line, flush if needed
                    let scale = match cur_line_tag {
                        HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };
                    let space_w = MinGuiCompositor::text_width(" ", scale);
                    let indent = tag_indent(cur_line_tag);
                    let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);
                    let img_w_px = disp_w.max(1);

                    if cur_line_w > 0 && cur_line_w + space_w + img_w_px > eff_wrap {
                        flush_line(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Add image span
                    cur_line_w += if cur_line_w > 0 {
                        space_w + img_w_px
                    } else {
                        img_w_px
                    };

                    if let Some(scaled_rgb) = pre_scaled_rgb {
                        cur_line_spans.push(TextSpan {
                            text: format!("[{}]", label),
                            color: current_color,
                            bg_color: current_bg,
                            is_link,
                            link_url: String::from(link_target),
                            is_button: active_is_button,
                            is_image: true,
                            image_width: disp_w,
                            image_height: disp_h,
                            image_rgb: Some(scaled_rgb),
                            image_url: String::from(image_src_url.as_deref().unwrap_or("")),
                            onclick: String::from(onclick_str),
                            elem_id: String::from(elem_id_str),
                            bold: current_bold,
                            underline: current_underline,
                        });
                    } else {
                        // Fallback: show alt text as placeholder
                        let mut alt_text = String::from("[");
                        alt_text.push_str(label);
                        alt_text.push(']');
                        cur_line_spans.push(TextSpan {
                            text: alt_text,
                            color: current_color,
                            bg_color: current_bg,
                            is_link,
                            link_url: String::from(link_target),
                            is_button: active_is_button,
                            is_image: false,
                            image_width: 0,
                            image_height: 0,
                            image_rgb: None,
                            image_url: String::new(),
                            onclick: String::from(onclick_str),
                            elem_id: String::from(elem_id_str),
                            bold: current_bold,
                            underline: current_underline,
                        });
                    }
                } else if eq_ascii(name, b"canvas") {
                    // <canvas>: HTML5 canvas elementi
                    static CANVAS_ID_COUNTER: core::sync::atomic::AtomicUsize =
                        core::sync::atomic::AtomicUsize::new(0);
                    let canvas_id =
                        extract_attr(attrs, b"id")
                            .map(String::from)
                            .unwrap_or_else(|| {
                                let n = CANVAS_ID_COUNTER
                                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                format!("canvas_{}", n)
                            });
                    let canvas_w = extract_attr(attrs, b"width")
                        .and_then(|w| w.trim().parse::<usize>().ok())
                        .unwrap_or(300);
                    let canvas_h = extract_attr(attrs, b"height")
                        .and_then(|h| h.trim().parse::<usize>().ok())
                        .unwrap_or(150);

                    // Flush any pending word before the canvas element.
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Create the canvas element and add it to the page's canvas list.
                    // The canvas will be rendered separately in render_content.
                    let mut canvas = crate::apps::browser::canvas::CanvasElement::new(
                        canvas_id.clone(),
                        canvas_w,
                        canvas_h,
                    );
                    // Draw a default border and background so the canvas is visible.
                    {
                        let ctx = canvas.get_context_2d();
                        ctx.fill_rect(0.0, 0.0, canvas_w as f32, canvas_h as f32);
                        ctx.set_fill_style(0xFFFFFF);
                        ctx.fill_rect(1.0, 1.0, (canvas_w as f32) - 2.0, (canvas_h as f32) - 2.0);
                        ctx.set_fill_style(0x000000);
                        ctx.stroke_rect(0.0, 0.0, canvas_w as f32, canvas_h as f32);
                        ctx.fill_text("Canvas", 10.0, 10.0 + ctx.font_size);
                    }
                    canvases.push(canvas);

                    // Check if canvas fits on current line, flush if needed.
                    let scale = match cur_line_tag {
                        HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };
                    let space_w = MinGuiCompositor::text_width(" ", scale);
                    let indent = tag_indent(cur_line_tag);
                    let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);
                    let canvas_w_px = canvas_w.max(1);

                    if cur_line_w > 0 && cur_line_w + space_w + canvas_w_px > eff_wrap {
                        flush_line(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    cur_line_w += if cur_line_w > 0 {
                        space_w + canvas_w_px
                    } else {
                        canvas_w_px
                    };

                    // Add a canvas span that references the canvas element by index.
                    let _canvas_idx = out_lines.len(); // placeholder, will be resolved at render
                    cur_line_spans.push(TextSpan {
                        text: format!("[canvas:{}]", canvas_id),
                        color: current_color,
                        bg_color: current_bg,
                        is_link,
                        link_url: String::from(link_target),
                        is_button: active_is_button,
                        is_image: true,
                        image_width: canvas_w,
                        image_height: canvas_h,
                        image_rgb: None,
                        image_url: String::new(),
                        onclick: String::from(onclick_str),
                        elem_id: canvas_id,
                        bold: current_bold,
                        underline: current_underline,
                    });
                } else if eq_ascii(name, b"video") {
                    // <video>: Video oynatıcı
                    let video_id =
                        extract_attr(attrs, b"id")
                            .map(String::from)
                            .unwrap_or_else(|| {
                                static VIDEO_ID_COUNTER: core::sync::atomic::AtomicUsize =
                                    core::sync::atomic::AtomicUsize::new(0);
                                let n = VIDEO_ID_COUNTER
                                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                format!("video_{}", n)
                            });
                    let src_url = extract_attr(attrs, b"src").map(String::from);
                    let controls = extract_attr(attrs, b"controls").is_some();
                    let autoplay = extract_attr(attrs, b"autoplay").is_some();
                    let loop_mode = extract_attr(attrs, b"loop").is_some();

                    // Flush any pending word before the video element.
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Create media player for this video element.
                    let mut player = super::media::MediaPlayer::new(
                        video_id.clone(),
                        super::media::MediaType::Video,
                    );
                    if let Some(src) = &src_url {
                        player.src = src.clone();
                    }
                    player.controls = controls;
                    player.autoplay = autoplay;
                    player.loop_mode = loop_mode;
                    if autoplay {
                        player.state = super::media::MediaState::Playing;
                        player.last_update_ms = crate::boot::time::main_monotonic_ms() as u64;
                    }
                    media_players.add(player);

                    // Render video placeholder box with controls hint.
                    let video_w = extract_attr(attrs, b"width")
                        .and_then(|w| w.trim().parse::<usize>().ok())
                        .unwrap_or(320);
                    let video_h = extract_attr(attrs, b"height")
                        .and_then(|h| h.trim().parse::<usize>().ok())
                        .unwrap_or(240);

                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    let scale = match cur_line_tag {
                        HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };
                    let space_w = MinGuiCompositor::text_width(" ", scale);
                    let indent = tag_indent(cur_line_tag);
                    let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);
                    let video_w_px = video_w.max(1);

                    if cur_line_w > 0 && cur_line_w + space_w + video_w_px > eff_wrap {
                        flush_line(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    cur_line_w += if cur_line_w > 0 {
                        space_w + video_w_px
                    } else {
                        video_w_px
                    };

                    cur_line_spans.push(TextSpan {
                        text: format!("[video:{}]", video_id),
                        color: current_color,
                        bg_color: current_bg,
                        is_link,
                        link_url: String::from(link_target),
                        is_button: active_is_button,
                        is_image: true,
                        image_width: video_w,
                        image_height: video_h,
                        image_rgb: None,
                        image_url: src_url.unwrap_or_default(),
                        onclick: String::from(onclick_str),
                        elem_id: video_id,
                        bold: current_bold,
                        underline: current_underline,
                    });
                } else if eq_ascii(name, b"audio") {
                    // <audio>: Ses oynatıcı
                    let audio_id =
                        extract_attr(attrs, b"id")
                            .map(String::from)
                            .unwrap_or_else(|| {
                                static AUDIO_ID_COUNTER: core::sync::atomic::AtomicUsize =
                                    core::sync::atomic::AtomicUsize::new(0);
                                let n = AUDIO_ID_COUNTER
                                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                format!("audio_{}", n)
                            });
                    let src_url = extract_attr(attrs, b"src").map(String::from);
                    let controls = extract_attr(attrs, b"controls").is_some();
                    let autoplay = extract_attr(attrs, b"autoplay").is_some();
                    let loop_mode = extract_attr(attrs, b"loop").is_some();

                    // Flush any pending word before the audio element.
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Create media player for this audio element.
                    let mut player = super::media::MediaPlayer::new(
                        audio_id.clone(),
                        super::media::MediaType::Audio,
                    );
                    if let Some(src) = &src_url {
                        player.src = src.clone();
                    }
                    player.controls = controls;
                    player.autoplay = autoplay;
                    player.loop_mode = loop_mode;
                    if autoplay {
                        player.state = super::media::MediaState::Playing;
                        player.last_update_ms = crate::boot::time::main_monotonic_ms() as u64;
                    }
                    media_players.add(player);

                    // Render audio placeholder.
                    if controls {
                        word_buf.push_str("[♫ AUDIO]");
                        if let Some(url) = src_url {
                            word_buf.push(' ');
                            word_buf.push_str(&url);
                        }
                    } else {
                        word_buf.push_str("[AUDIO]");
                        if let Some(url) = src_url {
                            word_buf.push(' ');
                            word_buf.push_str(&url);
                        }
                    }
                } else if eq_ascii(name, b"iframe") {
                    // <iframe>: İç içe tarama bağlamı
                    let iframe_id =
                        extract_attr(attrs, b"id")
                            .map(String::from)
                            .unwrap_or_else(|| {
                                static IFRAME_ID_COUNTER: core::sync::atomic::AtomicUsize =
                                    core::sync::atomic::AtomicUsize::new(0);
                                let n = IFRAME_ID_COUNTER
                                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                                format!("iframe_{}", n)
                            });
                    let src_url = extract_attr(attrs, b"src").map(String::from);
                    let iframe_w = extract_attr(attrs, b"width")
                        .and_then(|w| w.trim().parse::<usize>().ok())
                        .unwrap_or(300);
                    let iframe_h = extract_attr(attrs, b"height")
                        .and_then(|h| h.trim().parse::<usize>().ok())
                        .unwrap_or(150);
                    let sandbox = extract_attr(attrs, b"sandbox").is_some();

                    // Flush any pending word before the iframe element.
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    // Create iframe element.
                    let mut frame =
                        super::iframe::IFrame::new(iframe_id.clone(), iframe_w, iframe_h);
                    if let Some(src) = &src_url {
                        frame.set_src(src.clone());
                    }
                    frame.sandbox = sandbox;
                    iframes.add(frame);

                    // Render iframe placeholder box.
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    let scale = match cur_line_tag {
                        HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };
                    let space_w = MinGuiCompositor::text_width(" ", scale);
                    let indent = tag_indent(cur_line_tag);
                    let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);
                    let iframe_w_px = iframe_w.max(1);

                    if cur_line_w > 0 && cur_line_w + space_w + iframe_w_px > eff_wrap {
                        flush_line(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }

                    cur_line_w += if cur_line_w > 0 {
                        space_w + iframe_w_px
                    } else {
                        iframe_w_px
                    };

                    cur_line_spans.push(TextSpan {
                        text: format!("[iframe:{}]", iframe_id),
                        color: current_color,
                        bg_color: current_bg,
                        is_link,
                        link_url: String::from(link_target),
                        is_button: active_is_button,
                        is_image: true,
                        image_width: iframe_w,
                        image_height: iframe_h,
                        image_rgb: None,
                        image_url: src_url.unwrap_or_default(),
                        onclick: String::from(onclick_str),
                        elem_id: iframe_id,
                        bold: current_bold,
                        underline: current_underline,
                    });
                } else if matches!(name, b"ul" | b"ol") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    list_stack.push((eq_ascii(name, b"ol"), 0usize));
                } else if eq_ascii(name, b"li") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::ListItem;
                    // Liste işareti: <ol> için "1. ", <ul> için "• "
                    let (ordered, counter) = match list_stack.last_mut() {
                        Some(entry) => {
                            entry.1 += 1;
                            (entry.0, entry.1)
                        }
                        None => (false, 1usize),
                    };
                    let mut marker = String::new();
                    if ordered {
                        let mut n = counter;
                        let mut digits = [0u8; 10];
                        let mut di = 0;
                        if n == 0 {
                            digits[0] = b'0';
                            di = 1;
                        }
                        while n > 0 {
                            digits[di] = b'0' + (n % 10) as u8;
                            n /= 10;
                            di += 1;
                        }
                        while di > 0 {
                            di -= 1;
                            marker.push(digits[di] as char);
                        }
                        marker.push_str(". ");
                    } else {
                        marker.push('\u{2022}');
                        marker.push(' ');
                    }
                    word_buf.push_str(&marker);
                } else if matches!(name, b"td" | b"th") {
                    // Hücreleri yatay olarak ayraçla birleştir.
                    if cur_line_w > 0 {
                        word_buf.push('|');
                    }
                } else if eq_ascii(name, b"hr") {
                    // <hr>: yatay ayraç — mevcut satırı bitir, boş bir Rule satırı ekle
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    if out_lines.len() < max_lines {
                        out_lines.push(BrowserLine {
                            spans: Vec::new(),
                            tag: HtmlTagType::Rule,
                            indent: 0,
                            align: TextAlign::Left,
                        });
                    }
                    cur_line_tag = HtmlTagType::Normal;
                } else if eq_ascii(name, b"blockquote") {
                    // <blockquote>: alıntı bloğu — sol dikey çubuk + girinti
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Quote;
                    cur_line_indent.set(matched.indent_px.max(16));
                    if matched.color.is_none() {
                        current_color = Color::BLACK;
                    }
                } else if matches!(
                    name,
                    b"p" | b"div"
                        | b"br"
                        | b"tr"
                        | b"table"
                        | b"caption"
                        | b"section"
                        | b"article"
                        | b"center"
                ) {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    if eq_ascii(name, b"center") {
                        cur_line_align = TextAlign::Center;
                    }
                }
                // id ataması, elem_id_str son kullanıldıktan sonra yapılır (borrow kuralı).
                if id_attr.is_some() {
                    active_id = id_attr.map(String::from);
                }
            } else {
                // CSS Grid konteynerını kapat ve grid'i render et
                if in_grid {
                    render_grid(
                        out_lines,
                        &mut grid_cells,
                        &grid_columns,
                        grid_gap,
                        grid_current_row,
                        grid_current_col,
                        wrap_width_px,
                        max_lines,
                    );
                    in_grid = false;
                    grid_cells = Vec::new();
                    grid_columns = Vec::new();
                    grid_gap = 0;
                    grid_current_row = 0;
                    grid_current_col = 0;
                    grid_col_count = 0;
                    // Grid sonrasi mevcut satiri temizle
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                }
                cur_line_indent.set(0);
                current_bold = false;
                current_underline = false;
                if eq_ascii(name, b"a") {
                    active_link = None;
                    active_onclick = None;
                } else if eq_ascii(name, b"button") {
                    active_is_button = false;
                    active_onclick = None;
                    current_bg = None;
                } else if eq_ascii(name, b"center") {
                    cur_line_align = TextAlign::Left;
                } else if matches!(
                    name,
                    b"h1"
                        | b"h2"
                        | b"h3"
                        | b"p"
                        | b"div"
                        | b"table"
                        | b"caption"
                        | b"section"
                        | b"article"
                ) {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    current_color = Color::BLACK;
                    current_bg = None;
                    cur_line_align = TextAlign::Left;
                } else if eq_ascii(name, b"blockquote") {
                    // <blockquote> kapanıyor — alıntı bloğunu bitir
                    cur_line_indent.set(matched.indent_px.max(16));
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    current_color = Color::BLACK;
                    cur_line_indent.set(0);
                } else if matches!(name, b"ul" | b"ol") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    current_color = Color::BLACK;
                    current_bg = None;
                    cur_line_align = TextAlign::Left;
                    list_stack.pop();
                } else if eq_ascii(name, b"li") {
                    flush_line(
                        out_lines,
                        &mut cur_line_spans,
                        &mut cur_line_w,
                        cur_line_tag,
                        cur_line_align,
                        max_lines,
                    );
                    cur_line_tag = HtmlTagType::Normal;
                    current_color = Color::BLACK;
                    current_bg = None;
                    cur_line_align = TextAlign::Left;
                } else if eq_ascii(name, b"span") {
                    // <span> kapanıyor, stilleri sıfırla
                    cur_line_indent.set(0);
                    current_bold = false;
                    current_underline = false;
                } else if eq_ascii(name, b"pre") {
                    // <pre> kapanıyor, halini sıfırla
                    cur_line_indent.set(0);
                    current_bold = false;
                    current_underline = false;
                } else if eq_ascii(name, b"code") {
                    // <code> kapanıyor, yine normal metin moduna
                    cur_line_indent.set(0);
                    current_bold = false;
                    current_underline = false;
                } else if eq_ascii(name, b"kbd") {
                    // <kbd> kapanıyor, normal metin moduna
                    cur_line_indent.set(0);
                    current_bold = false;
                    current_underline = false;
                    word_buf.push(']');
                } else if eq_ascii(name, b"samp") {
                    // <samp> kapanıyor
                    word_buf.push('>');
                } else if eq_ascii(name, b"dl") {
                    // <dl> kapanıyor
                } else if eq_ascii(name, b"dt") {
                    // <dt> kapanıyor
                } else if eq_ascii(name, b"dd") {
                    // <dd> kapanıyor
                } else if eq_ascii(name, b"em") {
                    // <em> kapanıyor, renyi/altı çiziliyi sıfırla
                    current_bold = false;
                    current_underline = false;
                } else if eq_ascii(name, b"mark") {
                    // <mark> kapanıyor, background'ı sıfırla
                    current_bg = None;
                } else if eq_ascii(name, b"small") | eq_ascii(name, b"sub") | eq_ascii(name, b"sup")
                {
                    // <small>, <sub>, <sup> kapanıyor, hiçbir şey yapma
                } else if eq_ascii(name, b"code") {
                    // <code>: Kod metni
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('>');
                    word_buf.push('<');
                    // Basit kod etiketini metin olarak göster
                } else if eq_ascii(name, b"kbd") {
                    // <kbd>: Klavye tuşu girişi
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('[');
                    word_buf.push('[');
                } else if eq_ascii(name, b"samp") {
                    // <samp>: Program çıktı örneği
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('<');
                    word_buf.push('s');
                    word_buf.push('a');
                    word_buf.push('m');
                    word_buf.push('p');
                    word_buf.push('>');
                } else if eq_ascii(name, b"dl") {
                    // <dl>: Tanım listesi
                } else if eq_ascii(name, b"dt") {
                    // <dt>: Tanım başlığı
                    if !word_buf.is_empty() {
                        flush_word(
                            out_lines,
                            &mut cur_line_spans,
                            &mut cur_line_w,
                            &mut word_buf,
                            current_color,
                            current_bg,
                            is_link,
                            link_target,
                            active_is_button,
                            false,
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    word_buf.push('>'); // Tanım başlığı simgesi
                } else if eq_ascii(name, b"dd") {
                    // <dd>: Tanım defini
                    word_buf.push(' '); // Giriş boşluğu
                } else if eq_ascii(name, b"table") {
                    // <table> kapanıyor
                } else if eq_ascii(name, b"tr") {
                    // <tr> kapanıyor, satır sonu
                    word_buf.push('\n');
                } else if eq_ascii(name, b"th") {
                    // <th> kapanıyor, zaten metin eklendi
                    word_buf.push(']');
                } else if eq_ascii(name, b"td") {
                    // <td> kapanıyor, zaten metin eklendi
                    word_buf.push('[');
                } else if eq_ascii(name, b"video") {
                    // <video> kapanıyor
                    // Video içeriği zaten açılışta eklendi
                } else if eq_ascii(name, b"audio") {
                    // <audio> kapanıyor
                    // Ses içeriği zaten açılışta eklendi
                }
                active_id = None;
            }

            p = end + 1;
            continue;
        }

        let (ch, clen) = decode_html_char(body, p);
        p += clen;

        // <style> / <script> içindeki metin içeriğini gizle — sadece etiketler atlanıyordu,
        // metin içeriği yine kelime buffer'ına eklenip ekrana yazdıyordu.
        if hidden_until_close.is_some() {
            continue;
        }

        if ch == '\r' {
            continue;
        }
        if ch == '\n' || ch == '\t' || ch == ' ' {
            let link_target = active_link.as_deref().unwrap_or("");
            let is_link = active_link.is_some();
            let onclick_str = active_onclick.as_deref().unwrap_or("");
            let elem_id_str = active_id.as_deref().unwrap_or("");
            flush_word(
                out_lines,
                &mut cur_line_spans,
                &mut cur_line_w,
                &mut word_buf,
                current_color,
                current_bg,
                is_link,
                link_target,
                active_is_button,
                false,
                onclick_str,
                elem_id_str,
                current_bold,
                current_underline,
                cur_line_tag,
                cur_line_align,
                max_lines,
            );
            continue;
        }
        if (ch as u32) < 0x20 {
            continue;
        }
        word_buf.push(ch);
    }

    let link_target = active_link.as_deref().unwrap_or("");
    let is_link = active_link.is_some();
    let onclick_str = active_onclick.as_deref().unwrap_or("");
    let elem_id_str = active_id.as_deref().unwrap_or("");
    flush_word(
        out_lines,
        &mut cur_line_spans,
        &mut cur_line_w,
        &mut word_buf,
        current_color,
        current_bg,
        is_link,
        link_target,
        active_is_button,
        false,
        onclick_str,
        elem_id_str,
        current_bold,
        current_underline,
        cur_line_tag,
        cur_line_align,
        max_lines,
    );
    flush_line(
        out_lines,
        &mut cur_line_spans,
        &mut cur_line_w,
        cur_line_tag,
        cur_line_align,
        max_lines,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_url() {
        let base = "http://example.com/sub/page.html";
        assert_eq!(
            resolve_url("http://other.com/img.png", Some(base)).as_deref(),
            Some("http://other.com/img.png")
        );
        assert_eq!(
            resolve_url("//cdn.example.com/logo.png", Some(base)).as_deref(),
            Some("http://cdn.example.com/logo.png")
        );
        assert_eq!(
            resolve_url("/static/pic.jpg", Some(base)).as_deref(),
            Some("http://example.com/static/pic.jpg")
        );
        assert_eq!(
            resolve_url("photo.png", Some(base)).as_deref(),
            Some("http://example.com/sub/photo.png")
        );
        assert_eq!(
            resolve_url("./banner.gif", Some(base)).as_deref(),
            Some("http://example.com/sub/banner.gif")
        );
    }
}

/// CSS Grid render etme: hucreleri satir satir, sutunlara gore yerlestir.
fn render_grid(
    out_lines: &mut Vec<BrowserLine>,
    grid_cells: &mut Vec<Vec<Vec<TextSpan>>>,
    grid_columns: &[super::types::GridTrack],
    grid_gap: usize,
    _total_rows: usize,
    _total_cols: usize,
    wrap_width_px: usize,
    max_lines: usize,
) {
    if grid_cells.is_empty() {
        return;
    }

    let col_count = if grid_columns.is_empty() {
        1
    } else {
        grid_columns.len()
    };
    let avail_w =
        wrap_width_px.saturating_sub(grid_gap.saturating_mul(col_count.saturating_sub(1)));

    // Sutun genliklerini hesapla
    let mut col_widths: Vec<usize> = vec![0; col_count];
    let mut total_fr: u32 = 0;
    let mut total_fixed: usize = 0;
    for (i, track) in grid_columns.iter().enumerate() {
        match track {
            super::types::GridTrack::Fixed(w) => {
                col_widths[i] = *w;
                total_fixed += w;
            }
            super::types::GridTrack::Fraction(fr) => {
                total_fr += fr;
            }
            super::types::GridTrack::Auto => {}
        }
    }

    // Fr oranlarini piksele cevir
    let remaining_w = avail_w.saturating_sub(total_fixed);
    if total_fr > 0 && remaining_w > 0 {
        let per_fr = remaining_w as f32 / total_fr as f32;
        for (i, track) in grid_columns.iter().enumerate() {
            if let super::types::GridTrack::Fraction(fr) = track {
                col_widths[i] = (per_fr * *fr as f32) as usize;
            }
        }
    }

    // Her satiri render et
    for row in 0..grid_cells.len() {
        if out_lines.len() >= max_lines {
            break;
        }

        let mut line_spans: Vec<TextSpan> = Vec::new();
        let mut x_offset: usize = 0;

        for col in 0..col_count {
            if col > 0 {
                x_offset += grid_gap;
            }

            // Bosluk ile hucre ayracı
            if col > 0 && !line_spans.is_empty() {
                line_spans.push(TextSpan {
                    text: String::from(" "),
                    color: 0x000000,
                    bg_color: None,
                    is_link: false,
                    link_url: String::new(),
                    is_button: false,
                    is_image: false,
                    image_width: 0,
                    image_height: 0,
                    image_rgb: None,
                    image_url: String::new(),
                    bold: false,
                    underline: false,
                    onclick: String::new(),
                    elem_id: String::new(),
                });
                x_offset += 1;
            }

            let cell_spans = if row < grid_cells.len() && col < grid_cells[row].len() {
                &grid_cells[row][col]
            } else {
                &Vec::new()
            };

            for span in cell_spans.iter() {
                let mut s = span.clone();
                // Girinti ekle (basit: x_offset kadar bosluk onune)
                if x_offset > 0 && s.text.starts_with(' ') == false {
                    s.text = format!("{}{}", " ".repeat(x_offset.min(50)), s.text);
                }
                line_spans.push(s);
            }

            x_offset += col_widths[col];
        }

        if !line_spans.is_empty() {
            out_lines.push(BrowserLine {
                spans: line_spans,
                tag: HtmlTagType::Normal,
                indent: 0,
                align: TextAlign::Left,
            });
        }
    }
}

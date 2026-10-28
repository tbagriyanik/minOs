//! HTML tokenizer ve kelime sarma tabanlı yerleşim motoru (no_std).
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use super::css::{find_keyframes, match_css_style, parse_css};
use super::types::{BrowserLine, HtmlTagType, TextAlign, TextSpan};
use crate::gui::{Color, MinGuiCompositor};

pub fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
pub fn eq_ascii(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
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

/// Tokenizer, CSS rule applicator, and word-wrapping HTML layout engine
pub fn render_html_to_lines(
    body: &[u8],
    wrap_width_px: usize,
    out_lines: &mut Vec<BrowserLine>,
    max_lines: usize,
) {
    // JS: <script> bloklarını sırayla çalıştır; document.write çıktısı HTML'e gömülür,
    // script etiketleri ise çıktidan çıkarılır.
    let js_body = crate::apps::js::inline_scripts(body);
    let body: &[u8] = &js_body;
    let mut current_color = Color::TEXT_MAIN;
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
        let word_w = MinGuiCompositor::text_width(word, scale) + if is_button { 16 } else { 0 };
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
                bold,
                underline,
                onclick: String::from(onclick),
                elem_id: String::from(elem_id),
            });
        } else {
            *line_w += space_w + word_w;
            if let Some(last) = spans.last_mut() {
                if !is_button
                    && last.color == color
                    && last.bg_color == bg_color
                    && last.is_link == is_link
                    && last.link_url == link_url
                    && !last.is_button
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
                      onclick: &str,
                      elem_id: &str,
                      bold: bool,
                      underline: bool,
                      tag: HtmlTagType,
                      align: TextAlign,
                      max: usize| {
        if !buf.is_empty() {
            add_word(
                lines, spans, line_w, buf, color, bg, is_link, link_url, is_btn, onclick, elem_id,
                bold, underline, tag, align, max,
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
                onclick_str,
                elem_id_str,
                current_bold,
                current_underline,
                cur_line_tag,
                cur_line_align,
                max_lines,
            );

            let tag_name_str = core::str::from_utf8(name).unwrap_or("");
            let attrs = &tag_content[name_end..];
            let class_attr = extract_attr(attrs, b"class");
            let id_attr = extract_attr(attrs, b"id");
            let style_attr = extract_attr(attrs, b"style");
            let href_attr = extract_attr(attrs, b"href");
            let onclick_attr = extract_attr(attrs, b"onclick");

            let mut matched = match_css_style(&css_rules, tag_name_str, class_attr, id_attr, style_attr);

            // CSS animation: keyframes'ten frame bul ve uygula (statik render icin ilk frame)
            if let Some(kf_name) = matched.animation_name.clone() {
                if let Some(kf) = find_keyframes(&css_keyframes, &kf_name) {
                    if let Some(frame) = kf.frames.first() {
                        matched.opacity = frame.style.opacity;
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

                if eq_ascii(name, b"a") {
                    if let Some(href) = href_attr {
                        active_link = Some(String::from(href));
                    }
                    if let Some(oc) = onclick_attr {
                        active_onclick = Some(String::from(oc));
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
                        current_color = Color::ACCENT_PINK;
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
                        current_color = Color::ACCENT_BLUE;
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
                        current_color = Color::ACCENT_GREEN;
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
                        current_color = Color::ACCENT_BLUE;
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
                    // <img>: Görsel çizilemez; erişilebilir metin + basit görsel placeholder göster.
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
                            onclick_str,
                            elem_id_str,
                            current_bold,
                            current_underline,
                            cur_line_tag,
                            cur_line_align,
                            max_lines,
                        );
                    }
                    // Simple image placeholder box: show alt text
                    word_buf.push('[');
                    for ch in label.chars() {
                        if !ch.is_control() {
                            word_buf.push(ch);
                        }
                    }
                    word_buf.push(']');
                    word_buf.push(' ');
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
                        current_color = Color::TEXT_MUTED;
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
                    current_color = Color::TEXT_MAIN;
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
                    current_color = Color::TEXT_MAIN;
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
                    current_color = Color::TEXT_MAIN;
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
                    current_color = Color::TEXT_MAIN;
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
                }
                active_id = None;
            }

            p = end + 1;
            continue;
        }

        let (ch, clen) = decode_html_char(body, p);
        p += clen;

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

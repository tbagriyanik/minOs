//! HTML tokenizer ve kelime sarma tabanlı yerleşim motoru (no_std/alloc).
use alloc::string::String;
use alloc::vec::Vec;

use super::css::{match_css_style, parse_css};
use super::types::{BrowserLine, HtmlTagType, TextAlign, TextSpan};
use super::imgdecode::decode as decode_image;

pub fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
pub fn eq_ascii(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Basit metin genişliği ölçümü (host için).
/// Ortalama karakter genişliği = 8 piksel, scale çarpanı uygulanır.
pub fn text_width(text: &str, scale: usize) -> usize {
    let char_w = 8usize;
    text.len() * char_w * scale.max(1)
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
            if cur_key.eq_ignore_ascii_case(key) {
                return Some(core::str::from_utf8(val).unwrap_or(""));
            }
            if quote != 0 && i < attrs.len() {
                i += 1;
            }
        } else {
            if cur_key.eq_ignore_ascii_case(key) {
                return Some(core::str::from_utf8(&attrs[k_start..i]).unwrap_or(""));
            }
        }
    }
    None
}

fn tag_indent(tag: HtmlTagType) -> usize {
    match tag {
        HtmlTagType::ListItem => 20,
        HtmlTagType::Heading1 => 0,
        HtmlTagType::Heading2 => 0,
        HtmlTagType::Heading3 => 0,
        _ => 0,
    }
}

fn flush_line(
    lines: &mut Vec<BrowserLine>,
    spans: &mut Vec<TextSpan>,
    line_w: &mut usize,
    tag: HtmlTagType,
    align: TextAlign,
    max: usize,
) {
    if spans.is_empty() {
        *line_w = 0;
        return;
    }
    lines.push(BrowserLine {
        spans: core::mem::take(spans),
        tag,
        indent: 0,
        align,
    });
    *line_w = 0;
}

fn tag_from_name(name: &[u8]) -> HtmlTagType {
    match name {
        b"h1" => HtmlTagType::Heading1,
        b"h2" => HtmlTagType::Heading2,
        b"h3" => HtmlTagType::Heading3,
        b"li" => HtmlTagType::ListItem,
        b"span" => HtmlTagType::Span,
        b"em" => HtmlTagType::Em,
        b"mark" => HtmlTagType::Mark,
        b"small" => HtmlTagType::Small,
        b"sub" => HtmlTagType::Sub,
        b"sup" => HtmlTagType::Sup,
        b"hr" => HtmlTagType::Rule,
        b"blockquote" => HtmlTagType::Quote,
        _ => HtmlTagType::Normal,
    }
}

fn is_void_tag(name: &[u8]) -> bool {
    matches!(
        name,
        b"br" | b"hr" | b"img" | b"input" | b"meta" | b"link" | b"area" | b"col"
    )
}

/// HTML içeriğini satırlara dönüştürür.
/// `wrap_width_px`: satır genişliği sınırı (piksel).
/// `max_lines`: maksimum satır sayısı.
pub fn render_html_to_lines(
    body: &[u8],
    css: &str,
    wrap_width_px: usize,
    max_lines: usize,
) -> Vec<BrowserLine> {
    let parsed_css = parse_css(css);
    let mut lines: Vec<BrowserLine> = Vec::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut line_w: usize = 0;

    let mut p: usize = 0;
    let mut word_buf = String::new();
    let mut hidden_until_close: Option<Vec<u8>> = None;

    // Aktif stil durumu
    let mut current_color: u32 = 0x000000;
    let mut current_bg: Option<u32> = None;
    let mut current_bold: bool = false;
    let mut current_underline: bool = false;
    let mut active_link: Option<String> = None;
    let mut active_onclick: String = String::new();
    let mut active_id: String = String::new();

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
        let word_w = text_width(word, scale)
            + if is_button { 16 } else { 0 }
            + if is_image { 64 } else { 0 };
        let space_w = text_width(" ", scale);
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
                bold,
                underline,
                onclick: String::from(onclick),
                elem_id: String::from(elem_id),
            });
        }
    };

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

    while p < body.len() && lines.len() < max_lines {
        if body[p] == b'<' {
            // Önceki kelimeyi bitir
            flush_word(
                &mut lines, &mut spans, &mut line_w, &mut word_buf,
                current_color, current_bg, active_link.is_some(),
                active_link.as_deref().unwrap_or(""), false, false,
                &active_onclick, &active_id, current_bold, current_underline,
                HtmlTagType::Normal, TextAlign::Left, max_lines,
            );

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
            let attrs = &tag_content[name_end..];

            if let Some(ref h) = hidden_until_close {
                if closing && eq_ascii(name, h) {
                    hidden_until_close = None;
                }
                p = end + 1;
                continue;
            }

            // <style> bloğu: CSS'i çıkar
            if !closing && eq_ascii(name, b"style") {
                hidden_until_close = Some(name.to_vec());
                p = end + 1;
                continue;
            }

            if closing {
                // Kapanış etiketi: stil sıfırlama
                match name {
                    b"a" => {
                        active_link = None;
                        active_onclick.clear();
                    }
                    b"b" | b"strong" => current_bold = false,
                    b"u" => current_underline = false,
                    _ => {}
                }
                p = end + 1;
                continue;
            }

            // Açılış etiketi
            let tag = tag_from_name(name);

            // Stil eşleştirmesi
            let mut style = match_css_style(
                &parsed_css.rules,
                core::str::from_utf8(name).unwrap_or(""),
                extract_attr(attrs, b"class"),
                extract_attr(attrs, b"id"),
                extract_attr(attrs, b"style"),
            );

            if let Some(c) = style.color {
                current_color = c;
            }
            current_bg = style.bg_color;
            current_bold = style.bold || current_bold;
            current_underline = style.underline || current_underline;

            // <a> bağlantı
            if eq_ascii(name, b"a") {
                if let Some(href) = extract_attr(attrs, b"href") {
                    active_link = Some(String::from(href));
                }
                if let Some(oc) = extract_attr(attrs, b"onclick") {
                    active_onclick = String::from(oc);
                }
                if let Some(id) = extract_attr(attrs, b"id") {
                    active_id = String::from(id);
                }
            }

            // <img> etiketi
            if eq_ascii(name, b"img") {
                if let Some(src) = extract_attr(attrs, b"src") {
                    if src.starts_with("data:image/") {
                        // data:image/png;base64,... veya data:image/jpeg;base64,...
                        if let Some(comma_pos) = src.find(',') {
                            let mime_and_encoding = &src[..comma_pos];
                            let b64_data = &src[comma_pos + 1..];
                            let is_base64 = mime_and_encoding.contains("base64");

                            if is_base64 {
                                if let Some(decoded_bytes) = base64_decode(b64_data) {
                                    if let Some(decoded_img) = decode_image(&decoded_bytes) {
                                        let img_w = decoded_img.width.min(200);
                                        let img_h = decoded_img.height;
                                        let aspect = if img_w > 0 {
                                            img_h as f32 / img_w as f32
                                        } else {
                                            1.0
                                        };
                                        let display_w = img_w.min(wrap_width_px.max(100));
                                        let display_h = (display_w as f32 * aspect) as usize;

                                        // Resim verisini bir TextSpan olarak ekle
                                        flush_word(
                                            &mut lines, &mut spans, &mut line_w, &mut word_buf,
                                            current_color, current_bg, false, "", false, false,
                                            &active_onclick, &active_id, current_bold,
                                            current_underline, HtmlTagType::Normal,
                                            TextAlign::Left, max_lines,
                                        );

                                        spans.push(TextSpan {
                                            text: String::from(""),
                                            color: current_color,
                                            bg_color: current_bg,
                                            is_link: false,
                                            link_url: String::new(),
                                            is_button: false,
                                            is_image: true,
                                            image_width: display_w,
                                            image_height: display_h,
                                            image_rgb: Some(decoded_img.rgb),
                                            onclick: String::new(),
                                            elem_id: String::new(),
                                            bold: false,
                                            underline: false,
                                        });
                                        line_w += display_w;
                                    }
                                }
                            }
                        }
                    }
                }
                p = end + 1;
                continue;
            }

            // <br> satır sonu
            if eq_ascii(name, b"br") {
                flush_word(
                    &mut lines, &mut spans, &mut line_w, &mut word_buf,
                    current_color, current_bg, active_link.is_some(),
                    active_link.as_deref().unwrap_or(""), false, false,
                    &active_onclick, &active_id, current_bold, current_underline,
                    HtmlTagType::Normal, TextAlign::Left, max_lines,
                );
                flush_line(&mut lines, &mut spans, &mut line_w, tag, TextAlign::Left, max_lines);
                p = end + 1;
                continue;
            }

            // <p> paragraf
            if eq_ascii(name, b"p") {
                flush_word(
                    &mut lines, &mut spans, &mut line_w, &mut word_buf,
                    current_color, current_bg, active_link.is_some(),
                    active_link.as_deref().unwrap_or(""), false, false,
                    &active_onclick, &active_id, current_bold, current_underline,
                    HtmlTagType::Normal, TextAlign::Left, max_lines,
                );
                flush_line(&mut lines, &mut spans, &mut line_w, tag, style.align, max_lines);
                p = end + 1;
                continue;
            }

            // Metin içeriği topla
            p = end + 1;
            let mut text_start = p;
            while p < body.len() && body[p] != b'<' {
                p += 1;
            }
            let text = &body[text_start..p];
            if !text.is_empty() {
                let text_str = core::str::from_utf8(text).unwrap_or("");
                for ch in text_str.chars() {
                    if ch == '\n' || ch == '\r' {
                        flush_word(
                            &mut lines, &mut spans, &mut line_w, &mut word_buf,
                            current_color, current_bg, active_link.is_some(),
                            active_link.as_deref().unwrap_or(""), false, false,
                            &active_onclick, &active_id, current_bold, current_underline,
                            HtmlTagType::Normal, TextAlign::Left, max_lines,
                        );
                        if ch == '\n' {
                            flush_line(
                                &mut lines, &mut spans, &mut line_w, tag, style.align, max_lines,
                            );
                        }
                    } else if ch.is_ascii_whitespace() {
                        flush_word(
                            &mut lines, &mut spans, &mut line_w, &mut word_buf,
                            current_color, current_bg, active_link.is_some(),
                            active_link.as_deref().unwrap_or(""), false, false,
                            &active_onclick, &active_id, current_bold, current_underline,
                            HtmlTagType::Normal, TextAlign::Left, max_lines,
                        );
                    } else {
                        word_buf.push(ch);
                    }
                }
            }
        } else {
            p += 1;
        }
    }

    // Kalan kelimeyi bitir
    flush_word(
        &mut lines, &mut spans, &mut line_w, &mut word_buf,
        current_color, current_bg, active_link.is_some(),
        active_link.as_deref().unwrap_or(""), false, false,
        &active_onclick, &active_id, current_bold, current_underline,
        HtmlTagType::Normal, TextAlign::Left, max_lines,
    );
    if !spans.is_empty() {
        flush_line(&mut lines, &mut spans, &mut line_w, HtmlTagType::Normal, TextAlign::Left, max_lines);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_html() {
        let html = b"<html><body><p>Merhaba D\xFCnya</p></body></html>";
        let css = "";
        let lines = render_html_to_lines(html, css, 800, 100);
        assert!(!lines.is_empty());
        assert_eq!(lines[0].spans[0].text, "Merhaba Dünya");
    }

    #[test]
    fn test_image_data_url() {
        // 1x1 piksel siyah PNG (base64)
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/5+hHgAHggJ/chD2wbwAAAABJRU5ErkJggg==";
        let html = format!("<img src='data:image/png;base64,{}' />", png_b64);
        let css = "";
        let lines = render_html_to_lines(html.as_bytes(), css, 800, 100);
        assert!(!lines.is_empty());
        let img_span = lines.iter().flat_map(|l| &l.spans).find(|s| s.is_image);
        assert!(img_span.is_some());
        let img = img_span.unwrap();
        assert!(img.image_rgb.is_some());
        assert!(img.image_width > 0);
        assert!(img.image_height > 0);
    }
}

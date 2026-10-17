use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

fn find_bytes(haystack:&[u8], needle:&[u8])->Option<usize> {
    if needle.is_empty() || needle.len()>haystack.len(){return None;}
    haystack.windows(needle.len()).position(|w|w==needle)
}

fn split_http_lines(body:&[u8])->impl Iterator<Item=&str> {
    body.split(|b|*b==b'\n').filter_map(|line| {
        let line=if line.last()==Some(&b'\r'){&line[..line.len()-1]}else{line};
        if line.is_empty(){None}else{Some(core::str::from_utf8(line).unwrap_or("[binary or non-UTF8 content]"))}
    })
}

fn eq_ascii(a:&[u8], b:&[u8])->bool { a.eq_ignore_ascii_case(b) }

fn html_color(value:&[u8])->Option<u32> {
    let mut v=value;
    while v.first().map_or(false,|b|b.is_ascii_whitespace()||*b==b'\''||*b==b'"') { v=&v[1..]; }
    if v.first()==Some(&b'#') && v.len()>=7 {
        let mut rgb=0u32;
        for &b in &v[1..7] { let d=match b {b'0'..=b'9'=>(b-b'0') as u32,b'a'..=b'f'=>(b-b'a'+10) as u32,b'A'..=b'F'=>(b-b'A'+10) as u32,_=>return None};rgb=(rgb<<4)|d; }
        return Some(rgb);
    }
    match v.split(|b|*b==b';'||b.is_ascii_whitespace()||*b==b'\''||*b==b'"').next()? {
        b"black"=>Some(0x101010),b"white"=>Some(0xFFFFFF),b"red"=>Some(0xD03030),b"green"=>Some(0x188040),b"blue"=>Some(0x2050C0),b"yellow"=>Some(0xD0A020),b"gray"|b"grey"=>Some(0x808080),_=>None,
    }
}

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HtmlTagType {
    Normal,
    Heading1,
    Heading2,
    Heading3,
    ListItem,
}

#[derive(Clone)]
pub struct TextSpan {
    pub text: String,
    pub color: u32,
    pub is_link: bool,
    pub link_url: String,
}

#[derive(Clone)]
pub struct BrowserLine {
    pub spans: Vec<TextSpan>,
    pub tag: HtmlTagType,
    pub indent: usize,
}

#[derive(Clone)]
pub struct HttpCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

#[derive(Clone)]
pub struct RenderedLink {
    pub rect: (usize, usize, usize, usize),
    pub url: String,
}

/// Tokenizer and word-wrapping HTML layout engine
fn render_html_to_lines(body: &[u8], wrap_width_px: usize, out_lines: &mut Vec<BrowserLine>, max_lines: usize) {
    let mut current_color = Color::TEXT_MAIN;
    let mut active_link: Option<String> = None;
    let mut hidden: Option<&[u8]> = None;

    // Buffer of spans for the line currently being built
    let mut cur_line_spans: Vec<TextSpan> = Vec::new();
    let mut cur_line_w: usize = 0;
    let mut cur_line_tag = HtmlTagType::Normal;

    let tag_indent = |t: HtmlTagType| -> usize {
        match t {
            HtmlTagType::ListItem => 18,
            _ => 0,
        }
    };

    let flush_line = |lines: &mut Vec<BrowserLine>, spans: &mut Vec<TextSpan>, line_w: &mut usize, tag: HtmlTagType, max: usize| {
        if lines.len() < max && !spans.is_empty() {
            let indent = tag_indent(tag);
            lines.push(BrowserLine {
                spans: core::mem::take(spans),
                tag,
                indent,
            });
            *line_w = 0;
        }
    };

    let add_word = |lines: &mut Vec<BrowserLine>,
                    spans: &mut Vec<TextSpan>,
                    line_w: &mut usize,
                    word: &str,
                    color: u32,
                    is_link: bool,
                    link_url: &str,
                    tag: HtmlTagType,
                    max: usize| {
        if word.is_empty() { return; }
        let scale = match tag {
            HtmlTagType::Heading1 => 2,
            HtmlTagType::Heading2 => 1,
            _ => 1,
        };
        let word_w = MinGuiCompositor::text_width(word, scale);
        let space_w = MinGuiCompositor::text_width(" ", scale);
        let indent = tag_indent(tag);
        let eff_wrap = wrap_width_px.saturating_sub(indent).max(100);

        if *line_w > 0 && *line_w + space_w + word_w > eff_wrap {
            flush_line(lines, spans, line_w, tag, max);
        }

        if spans.is_empty() {
            *line_w = word_w;
            spans.push(TextSpan {
                text: String::from(word),
                color,
                is_link,
                link_url: String::from(link_url),
            });
        } else {
            *line_w += space_w + word_w;
            // Append with leading space
            if let Some(last) = spans.last_mut() {
                if last.color == color && last.is_link == is_link && last.link_url == link_url {
                    last.text.push(' ');
                    last.text.push_str(word);
                    return;
                }
            }
            let mut s = String::from(" ");
            s.push_str(word);
            spans.push(TextSpan {
                text: s,
                color,
                is_link,
                link_url: String::from(link_url),
            });
        }
    };

    let mut p = 0usize;
    let mut word_buf = String::new();

    let flush_word = |lines: &mut Vec<BrowserLine>,
                          spans: &mut Vec<TextSpan>,
                          line_w: &mut usize,
                          buf: &mut String,
                          color: u32,
                          is_link: bool,
                          link_url: &str,
                          tag: HtmlTagType,
                          max: usize| {
        if !buf.is_empty() {
            add_word(lines, spans, line_w, buf, color, is_link, link_url, tag, max);
            buf.clear();
        }
    };

    while p < body.len() && out_lines.len() < max_lines {
        if body[p] == b'<' {
            let start = p + 1;
            let mut end = start;
            while end < body.len() && body[end] != b'>' { end += 1; }
            if end == body.len() { break; }
            let mut tag = &body[start..end];
            let closing = tag.first() == Some(&b'/');
            if closing { tag = &tag[1..]; }
            let name_end = tag.iter().position(|b| b.is_ascii_whitespace() || *b == b'/').unwrap_or(tag.len());
            let name = &tag[..name_end];

            if let Some(h) = hidden {
                if closing && eq_ascii(name, h) { hidden = None; }
                p = end + 1;
                continue;
            }
            if !closing && (eq_ascii(name, b"script") || eq_ascii(name, b"style")) {
                hidden = Some(name);
                p = end + 1;
                continue;
            }

            // Word boundary at tag
            let link_target = active_link.as_deref().unwrap_or("");
            let is_link = active_link.is_some();
            flush_word(out_lines, &mut cur_line_spans, &mut cur_line_w, &mut word_buf, current_color, is_link, link_target, cur_line_tag, max_lines);

            if !closing && eq_ascii(name, b"a") {
                // Parse href
                let attrs = &tag[name_end..];
                let mut i = 0;
                let mut found_href = None;
                while i < attrs.len() {
                    while i < attrs.len() && (attrs[i].is_ascii_whitespace() || attrs[i] == b'/') { i += 1; }
                    let k = i;
                    while i < attrs.len() && (attrs[i].is_ascii_alphanumeric() || attrs[i] == b'-') { i += 1; }
                    let key = &attrs[k..i];
                    while i < attrs.len() && attrs[i].is_ascii_whitespace() { i += 1; }
                    if i < attrs.len() && attrs[i] == b'=' {
                        i += 1;
                        while i < attrs.len() && attrs[i].is_ascii_whitespace() { i += 1; }
                        let quote = if i < attrs.len() && (attrs[i] == b'\'' || attrs[i] == b'"') {
                            let q = attrs[i];
                            i += 1;
                            q
                        } else { 0 };
                        let v = i;
                        while i < attrs.len() && ((quote != 0 && attrs[i] != quote) || (quote == 0 && !attrs[i].is_ascii_whitespace())) {
                            i += 1;
                        }
                        if eq_ascii(key, b"href") {
                            found_href = core::str::from_utf8(&attrs[v..i]).ok();
                        }
                        if quote != 0 && i < attrs.len() { i += 1; }
                    } else if i == k {
                        i += 1;
                    }
                }
                if let Some(href) = found_href {
                    active_link = Some(String::from(href));
                }
            } else if closing && eq_ascii(name, b"a") {
                active_link = None;
            }

            // Parse style color
            if !closing && name_end < tag.len() {
                let attrs = &tag[name_end..];
                let mut i = 0;
                while i < attrs.len() {
                    while i < attrs.len() && (attrs[i].is_ascii_whitespace() || attrs[i] == b'/') { i += 1; }
                    let k = i;
                    while i < attrs.len() && (attrs[i].is_ascii_alphanumeric() || attrs[i] == b'-') { i += 1; }
                    let key = &attrs[k..i];
                    while i < attrs.len() && attrs[i].is_ascii_whitespace() { i += 1; }
                    if i < attrs.len() && attrs[i] == b'=' {
                        i += 1;
                        while i < attrs.len() && attrs[i].is_ascii_whitespace() { i += 1; }
                        let quote = if i < attrs.len() && (attrs[i] == b'\'' || attrs[i] == b'"') {
                            let q = attrs[i];
                            i += 1;
                            q
                        } else { 0 };
                        let v = i;
                        while i < attrs.len() && ((quote != 0 && attrs[i] != quote) || (quote == 0 && !attrs[i].is_ascii_whitespace())) {
                            i += 1;
                        }
                        if eq_ascii(key, b"style") {
                            let style = &attrs[v..i];
                            for rule in style.split(|b| *b == b';') {
                                if let Some(colon) = rule.iter().position(|b| *b == b':') {
                                    if eq_ascii(rule[..colon].trim_ascii(), b"color") {
                                        if let Some(c) = html_color(&rule[colon + 1..]) {
                                            current_color = c;
                                        }
                                    }
                                }
                            }
                        } else if eq_ascii(key, b"color") {
                            if let Some(c) = html_color(&attrs[v..i]) {
                                current_color = c;
                            }
                        }
                        if quote != 0 && i < attrs.len() { i += 1; }
                    } else if i == k {
                        i += 1;
                    }
                }
            }

            // Heading & list block detection
            if !closing {
                if eq_ascii(name, b"h1") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::Heading1;
                    if current_color == Color::TEXT_MAIN { current_color = Color::ACCENT_PINK; }
                } else if eq_ascii(name, b"h2") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::Heading2;
                    if current_color == Color::TEXT_MAIN { current_color = Color::ACCENT_BLUE; }
                } else if eq_ascii(name, b"h3") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::Heading3;
                    if current_color == Color::TEXT_MAIN { current_color = Color::ACCENT_GREEN; }
                } else if eq_ascii(name, b"li") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::ListItem;
                } else if matches!(name, b"p" | b"div" | b"br" | b"hr" | b"tr" | b"section" | b"article" | b"ul" | b"ol") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::Normal;
                }
            } else {
                if matches!(name, b"h1" | b"h2" | b"h3" | b"li" | b"p" | b"div" | b"ul" | b"ol") {
                    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
                    cur_line_tag = HtmlTagType::Normal;
                    current_color = Color::TEXT_MAIN;
                }
            }

            p = end + 1;
            continue;
        }

        if body[p] == b'&' {
            if let Some(semi) = body[p..].iter().take(12).position(|b| *b == b';') {
                let entity = &body[p + 1..p + semi];
                let decoded = match entity {
                    b"amp" => Some('&'),
                    b"lt" => Some('<'),
                    b"gt" => Some('>'),
                    b"quot" => Some('"'),
                    b"apos" => Some('\''),
                    b"nbsp" => Some(' '),
                    _ => None,
                };
                if let Some(ch) = decoded {
                    if ch == ' ' {
                        let link_target = active_link.as_deref().unwrap_or("");
                        let is_link = active_link.is_some();
                        flush_word(out_lines, &mut cur_line_spans, &mut cur_line_w, &mut word_buf, current_color, is_link, link_target, cur_line_tag, max_lines);
                    } else {
                        word_buf.push(ch);
                    }
                    p += semi + 1;
                    continue;
                }
            }
        }

        let b = body[p];
        p += 1;
        if b == b'\r' { continue; }
        if b == b'\n' || b == b'\t' || b == b' ' {
            let link_target = active_link.as_deref().unwrap_or("");
            let is_link = active_link.is_some();
            flush_word(out_lines, &mut cur_line_spans, &mut cur_line_w, &mut word_buf, current_color, is_link, link_target, cur_line_tag, max_lines);
            continue;
        }
        if b < 0x20 { continue; }
        word_buf.push(b as char);
    }

    let link_target = active_link.as_deref().unwrap_or("");
    let is_link = active_link.is_some();
    flush_word(out_lines, &mut cur_line_spans, &mut cur_line_w, &mut word_buf, current_color, is_link, link_target, cur_line_tag, max_lines);
    flush_line(out_lines, &mut cur_line_spans, &mut cur_line_w, cur_line_tag, max_lines);
}

pub struct BrowserApp {
    pub url: [u8; 128],
    pub url_len: usize,
    pub address_focused: bool,
    pub address_cursor: usize,
    pub address_scroll: usize,
    pub address_selection: Option<(usize, usize)>,
    address_undo: [[u8; 128]; 16],
    address_undo_len: [usize; 16],
    address_undo_count: usize,
    address_redo: [[u8; 128]; 16],
    address_redo_len: [usize; 16],
    address_redo_count: usize,
    pub active_tab: usize,
    pub focused_control: usize,
    pub page_loaded: bool,
    pub status: &'static str,
    pub net_online: bool,
    pub ping_ok: bool,
    pub web_request_sent: bool,
    pub page_title: [u8; 64],
    pub page_title_len: usize,
    pub page_lines: Vec<BrowserLine>,
    pub rendered_links: Vec<RenderedLink>,
    pub scroll_y: usize,
    pub request_pending: bool,
    pub request_host: [u8; 128],
    pub request_host_len: usize,
    pub request_path: [u8; 256],
    pub request_path_len: usize,
    pub request_ip: crate::drivers::netstack::Ipv4Address,
    pub request_port: u16,
    pub request_deadline: usize,
    pub request_state: u8,
    pub redirect_count: usize,
    pub is_post: bool,
    pub post_body: Vec<u8>,
    pub cookies: Vec<HttpCookie>,
    pub response_data: Vec<u8>,
    pub response_len: usize,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128], url_len: 0, address_focused: false, address_cursor: 0, address_scroll: 0, address_selection: None,
            address_undo: [[0; 128]; 16], address_undo_len: [0; 16], address_undo_count: 0,
            address_redo: [[0; 128]; 16], address_redo_len: [0; 16], address_redo_count: 0,
            active_tab: 0, focused_control: 0, page_loaded: true, status: crate::gui::ui_text("minOs yerel yardım sayfası", "minOs local help page"),
            net_online: false, ping_ok: false,
            web_request_sent: false,
            page_title: [0; 64], page_title_len: 0,
            page_lines: Vec::new(),
            rendered_links: Vec::new(),
            scroll_y: 0,
            request_pending: false, request_host: [0; 128], request_host_len: 0,
            request_path: [0; 256], request_path_len: 0, request_ip: crate::drivers::netstack::Ipv4Address([0; 4]), request_port: 80,
            request_deadline: 0, request_state: 0, redirect_count: 0,
            is_post: false, post_body: Vec::new(), cookies: Vec::new(),
            response_data: Vec::new(), response_len: 0,
        };
        app.set_url(b"minos://help");
        app
    }

    pub fn content_height(&self) -> usize {
        if self.active_tab == 3 {
            let mut total_h = 40;
            for line in &self.page_lines {
                let lh = match line.tag {
                    HtmlTagType::Heading1 => 26,
                    HtmlTagType::Heading2 => 22,
                    HtmlTagType::Heading3 => 20,
                    _ => 18,
                };
                total_h += lh;
            }
            total_h + 40
        } else {
            280
        }
    }

    pub fn click_link_at(&mut self, mx: usize, my: usize) -> Option<String> {
        for link in &self.rendered_links {
            let (rx, ry, rw, rh) = link.rect;
            if mx >= rx && mx < rx + rw && my >= ry && my < ry + rh {
                return Some(link.url.clone());
            }
        }
        None
    }

    pub fn focus_next_control(&mut self) {
        // 0..2: Tabs (Help, Cats, Web), 3: Back, 4: Fwd, 5: Refresh, 6: Address bar, 7: Go button
        self.focused_control = (self.focused_control + 1) % 8;
        if self.focused_control == 6 {
            self.address_focused = true;
        } else {
            self.address_focused = false;
        }
        if self.focused_control < 3 {
            match self.focused_control {
                0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
                1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
                _ => { self.active_tab = 3; self.page_loaded = true; },
            }
        }
    }

    pub fn select_tab_prev(&mut self) {
        let cur = match self.active_tab {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let next = (cur + 2) % 3;
        self.focused_control = next;
        match next {
            0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
            1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
            _ => { self.active_tab = 3; self.page_loaded = true; },
        }
    }

    pub fn select_tab_next(&mut self) {
        let cur = match self.active_tab {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let next = (cur + 1) % 3;
        self.focused_control = next;
        match next {
            0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
            1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
            _ => { self.active_tab = 3; self.page_loaded = true; },
        }
    }

    pub fn set_url(&mut self, value: &[u8]) {
        self.url = [0; 128];
        self.url_len = value.len().min(self.url.len());
        self.url[..self.url_len].copy_from_slice(&value[..self.url_len]);
        self.address_cursor = self.url_len;
        self.address_scroll = self.url_len.saturating_sub(40);
        self.address_selection = None;
    }

    pub fn focus_address_at(&mut self, x: usize, field_x: usize) {
        self.address_focused = true;
        self.focused_control = 6;
        let column = x.saturating_sub(field_x) / 10;
        self.address_cursor = (self.address_scroll + column).min(self.url_len);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn focus_address_end(&mut self) {
        self.address_focused = true;
        self.focused_control = 6;
        self.address_selection = None;
        self.address_cursor = self.url_len;
        self.keep_address_cursor_visible();
    }

    fn keep_address_cursor_visible(&mut self) {
        if self.address_cursor < self.address_scroll {
            self.address_scroll = self.address_cursor;
        } else if self.address_cursor >= self.address_scroll + 40 {
            self.address_scroll = self.address_cursor + 1 - 40;
        }
    }

    pub fn set_web_content(&mut self, title: &str, lines: &[&str]) {
        self.page_title = [0; 64];
        let tb = title.as_bytes();
        self.page_title_len = tb.len().min(64);
        self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);

        self.page_lines.clear();
        for &line in lines.iter().take(128) {
            self.page_lines.push(BrowserLine {
                spans: alloc::vec![TextSpan {
                    text: String::from(line),
                    color: Color::TEXT_MAIN,
                    is_link: false,
                    link_url: String::new(),
                }],
                tag: HtmlTagType::Normal,
                indent: 0,
            });
        }
    }

    pub fn address_char(&mut self, ch: char) {
        if ch.is_ascii_graphic() || ch == ' ' {
            if self.url_len < self.url.len() { self.address_push_undo(); }
            self.address_delete_selection();
            if self.url_len < self.url.len() {
                self.url.copy_within(self.address_cursor..self.url_len, self.address_cursor + 1);
                self.url[self.address_cursor] = ch as u8;
                self.url_len += 1;
                self.address_cursor += 1;
                self.keep_address_cursor_visible();
            }
        }
    }

    pub fn address_backspace(&mut self) {
        if self.address_selection.is_some() || self.address_cursor > 0 { self.address_push_undo(); }
        if self.address_delete_selection() { return; }
        if self.address_cursor > 0 {
            self.address_cursor -= 1;
            self.url.copy_within(self.address_cursor + 1..self.url_len, self.address_cursor);
            self.url_len -= 1;
            self.url[self.url_len] = 0;
            self.keep_address_cursor_visible();
        }
    }

    pub fn address_delete(&mut self) {
        if self.address_selection.is_some() || self.address_cursor < self.url_len { self.address_push_undo(); }
        if self.address_delete_selection() { return; }
        if self.address_cursor < self.url_len {
            self.url.copy_within(self.address_cursor + 1..self.url_len, self.address_cursor);
            self.url_len -= 1;
            self.url[self.url_len] = 0;
            self.keep_address_cursor_visible();
        }
    }

    fn address_move_to(&mut self, target: usize, select: bool) {
        if select {
            let anchor = match self.address_selection {
                Some((a, b)) => if self.address_cursor == b { a } else { b },
                None => self.address_cursor,
            };
            self.address_cursor = target.min(self.url_len);
            if anchor == self.address_cursor {
                self.address_selection = None;
            } else {
                self.address_selection = Some((anchor.min(self.address_cursor), anchor.max(self.address_cursor)));
            }
        } else {
            self.address_cursor = target.min(self.url_len);
            self.address_selection = None;
        }
        self.keep_address_cursor_visible();
    }

    pub fn address_move_left(&mut self) {
        self.address_move_left_ext(false);
    }

    pub fn address_move_left_ext(&mut self, select: bool) {
        let target = if !select {
            if let Some((a, b)) = self.address_selection.take() {
                a.min(b)
            } else {
                self.address_cursor.saturating_sub(1)
            }
        } else {
            self.address_cursor.saturating_sub(1)
        };
        self.address_move_to(target, select);
    }

    pub fn address_move_right(&mut self) {
        self.address_move_right_ext(false);
    }

    pub fn address_move_right_ext(&mut self, select: bool) {
        let target = if !select {
            if let Some((a, b)) = self.address_selection.take() {
                a.max(b)
            } else {
                (self.address_cursor + 1).min(self.url_len)
            }
        } else {
            (self.address_cursor + 1).min(self.url_len)
        };
        self.address_move_to(target, select);
    }

    pub fn address_home(&mut self) {
        self.address_home_ext(false);
    }

    pub fn address_home_ext(&mut self, select: bool) {
        self.address_move_to(0, select);
    }

    pub fn address_end(&mut self) {
        self.address_end_ext(false);
    }

    pub fn address_end_ext(&mut self, select: bool) {
        self.address_move_to(self.url_len, select);
    }

    pub fn address_move_word_left_ext(&mut self, select: bool) {
        let base = if !select {
            if let Some((a, b)) = self.address_selection.take() {
                a.min(b)
            } else {
                self.address_cursor
            }
        } else {
            self.address_cursor
        };
        let mut idx = base;
        while idx > 0 && self.url[idx - 1] == b' ' {
            idx -= 1;
        }
        if idx > 0 {
            let is_alnum = self.url[idx - 1].is_ascii_alphanumeric() || self.url[idx - 1] == b'_';
            while idx > 0 {
                let prev = self.url[idx - 1];
                if prev == b' ' { break; }
                if (prev.is_ascii_alphanumeric() || prev == b'_') != is_alnum { break; }
                idx -= 1;
            }
        }
        self.address_move_to(idx, select);
    }

    pub fn address_move_word_right_ext(&mut self, select: bool) {
        let base = if !select {
            if let Some((a, b)) = self.address_selection.take() {
                a.max(b)
            } else {
                self.address_cursor
            }
        } else {
            self.address_cursor
        };
        let mut idx = base;
        if idx < self.url_len {
            let is_alnum = self.url[idx].is_ascii_alphanumeric() || self.url[idx] == b'_';
            let is_space = self.url[idx] == b' ';
            if !is_space {
                while idx < self.url_len {
                    let cur = self.url[idx];
                    if cur == b' ' { break; }
                    if (cur.is_ascii_alphanumeric() || cur == b'_') != is_alnum { break; }
                    idx += 1;
                }
            }
            while idx < self.url_len && self.url[idx] == b' ' {
                idx += 1;
            }
        }
        self.address_move_to(idx, select);
    }

    pub fn address_delete_word_back(&mut self) {
        if self.address_selection.is_some() {
            self.address_push_undo();
            self.address_delete_selection();
            return;
        }
        if self.address_cursor == 0 { return; }
        let mut idx = self.address_cursor;
        while idx > 0 && self.url[idx - 1] == b' ' {
            idx -= 1;
        }
        if idx > 0 {
            let is_alnum = self.url[idx - 1].is_ascii_alphanumeric() || self.url[idx - 1] == b'_';
            while idx > 0 {
                let prev = self.url[idx - 1];
                if prev == b' ' { break; }
                if (prev.is_ascii_alphanumeric() || prev == b'_') != is_alnum { break; }
                idx -= 1;
            }
        }
        self.address_push_undo();
        self.address_selection = Some((idx, self.address_cursor));
        self.address_delete_selection();
    }

    pub fn address_delete_word_forward(&mut self) {
        if self.address_selection.is_some() {
            self.address_push_undo();
            self.address_delete_selection();
            return;
        }
        if self.address_cursor >= self.url_len { return; }
        let mut idx = self.address_cursor;
        if idx < self.url_len {
            let is_alnum = self.url[idx].is_ascii_alphanumeric() || self.url[idx] == b'_';
            let is_space = self.url[idx] == b' ';
            if !is_space {
                while idx < self.url_len {
                    let cur = self.url[idx];
                    if cur == b' ' { break; }
                    if (cur.is_ascii_alphanumeric() || cur == b'_') != is_alnum { break; }
                    idx += 1;
                }
            }
            while idx < self.url_len && self.url[idx] == b' ' {
                idx += 1;
            }
        }
        self.address_push_undo();
        self.address_selection = Some((self.address_cursor, idx));
        self.address_delete_selection();
    }

    fn address_delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.address_selection.take() {
            let start = a.min(b).min(self.url_len);
            let end = a.max(b).min(self.url_len);
            if start < end {
                self.url.copy_within(end..self.url_len, start);
                self.url_len -= end - start;
                self.url[self.url_len] = 0;
                self.address_cursor = start;
                self.keep_address_cursor_visible();
            }
            return start < end;
        }
        false
    }

    fn address_push_undo(&mut self) {
        self.address_record_undo();
        self.address_redo_count = 0;
    }

    fn address_record_undo(&mut self) {
        if self.address_undo_count == self.address_undo.len() {
            self.address_undo.copy_within(1.., 0);
            self.address_undo_len.copy_within(1.., 0);
            self.address_undo_count -= 1;
        }
        let i = self.address_undo_count;
        self.address_undo[i] = self.url;
        self.address_undo_len[i] = self.url_len;
        self.address_undo_count += 1;
    }

    fn address_save_redo(&mut self) {
        if self.address_redo_count == self.address_redo.len() {
            self.address_redo.copy_within(1.., 0);
            self.address_redo_len.copy_within(1.., 0);
            self.address_redo_count -= 1;
        }
        let i = self.address_redo_count;
        self.address_redo[i] = self.url;
        self.address_redo_len[i] = self.url_len;
        self.address_redo_count += 1;
    }

    pub fn address_undo_edit(&mut self) {
        if self.address_undo_count == 0 { return; }
        self.address_save_redo();
        self.address_undo_count -= 1;
        self.url = self.address_undo[self.address_undo_count];
        self.url_len = self.address_undo_len[self.address_undo_count];
        self.address_cursor = self.address_cursor.min(self.url_len);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn address_redo_edit(&mut self) {
        if self.address_redo_count == 0 { return; }
        self.address_record_undo();
        self.address_redo_count -= 1;
        self.url = self.address_redo[self.address_redo_count];
        self.url_len = self.address_redo_len[self.address_redo_count];
        self.address_cursor = self.address_cursor.min(self.url_len);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn address_select_all(&mut self) {
        self.address_focused = true;
        self.focused_control = 6;
        if self.url_len > 0 {
            self.address_selection = Some((0, self.url_len));
            self.address_cursor = self.url_len;
            self.keep_address_cursor_visible();
        }
    }

    pub fn address_copy(&self) {
        if let Some((a, b)) = self.address_selection {
            let start = a.min(b).min(self.url_len);
            let end = a.max(b).min(self.url_len);
            if start < end {
                if let Ok(text) = core::str::from_utf8(&self.url[start..end]) {
                    crate::gui::clipboard::set_str(text);
                }
            }
        }
    }

    pub fn address_cut(&mut self) {
        self.address_copy();
        if self.address_selection.is_some() { self.address_push_undo(); }
        self.address_delete_selection();
    }

    pub fn address_paste(&mut self) {
        if self.address_selection.is_some() { self.address_push_undo(); }
        self.address_delete_selection();
        let mut pasted = ['\0'; crate::gui::clipboard::CAPACITY];
        let len = crate::gui::clipboard::copy_chars(&mut pasted);
        for ch in pasted[..len].iter().copied() {
            if ch.is_ascii_graphic() || ch == ' ' { self.address_char(ch); }
        }
    }

    pub fn navigate(&mut self) {
        let mut input = [0u8; 128];
        input[..self.url_len].copy_from_slice(&self.url[..self.url_len]);
        let entered = core::str::from_utf8(&input[..self.url_len]).unwrap_or("").trim();
        let mut normalized = [0u8; 128];
        let mut normalized_len = entered.len().min(normalized.len());
        if !entered.is_empty() && !entered.contains("://") && !entered.starts_with("minos:") {
            let n = entered.len().min(normalized.len().saturating_sub(7));
            normalized[..7].copy_from_slice(b"http://");
            normalized[7..7 + n].copy_from_slice(&entered.as_bytes()[..n]);
            normalized_len = 7 + n;
        } else {
            normalized[..normalized_len].copy_from_slice(&entered.as_bytes()[..normalized_len]);
        }
        let value = core::str::from_utf8(&normalized[..normalized_len]).unwrap_or("");
        if value.as_bytes() != &self.url[..self.url_len] {
            self.set_url(value.as_bytes());
        }
        self.web_request_sent = false;
        if value.starts_with("https://") {
            self.active_tab=3; self.page_loaded=false; self.request_pending=false;
            self.status=crate::gui::ui_text("HTTPS/TLS bu çekirdek motorunda desteklenmiyor.","HTTPS/TLS is not supported by this kernel renderer.");
            self.address_focused=false; return;
        }
        if let Some(rest) = value.strip_prefix("http://") {
            let authority_end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let suffix = &rest[authority_end..];
            let suffix = suffix.split('#').next().unwrap_or("");
            let (host, port) = if authority.starts_with('[') {
                ("", 0) // Bu ağ yığını şu an yalnızca IPv4 hedeflerini destekliyor.
            } else {
                match authority.rsplit_once(':') {
                    Some((h,p)) => match p.parse::<u16>() { Ok(n) if n>0 => (h,n), _ => ("",0) },
                    _ => (authority,80),
                }
            };
            let path_len = if suffix.is_empty() { 1 } else if suffix.starts_with('?') { suffix.len() + 1 } else { suffix.len() };
            if host.is_empty() || host.len() > 128 || path_len > 256 || port == 0 {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address.");
            } else if !crate::drivers::e1000::is_ready() {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable.");
            } else {
                self.active_tab = 3;
                self.page_loaded = false;
                self.web_request_sent = false;
                self.redirect_count = 0;
                self.is_post = false;
                self.post_body.clear();
                self.request_host = [0; 128];
                self.request_host_len = host.len();
                self.request_host[..host.len()].copy_from_slice(host.as_bytes());
                self.request_path = [0; 256];
                self.request_path_len = path_len;
                if suffix.is_empty() {
                    self.request_path[0] = b'/';
                } else if suffix.starts_with('?') {
                    self.request_path[0] = b'/';
                    self.request_path[1..path_len].copy_from_slice(suffix.as_bytes());
                } else {
                    self.request_path[..path_len].copy_from_slice(suffix.as_bytes());
                }
                self.request_port = port;
                self.request_state = 0;
                self.request_pending = true;
                self.response_len = 0;
                self.response_data.clear();
                self.response_data.resize(65536, 0);
                self.status = crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host...");
                self.page_title = [0; 64];
                self.page_title_len = 0;
                self.page_lines.clear();
            }
            self.address_focused = false;
            return;
        }
        match value {
            "minos://help" => {
                self.active_tab = 0;
                self.page_loaded = true;
                self.status = crate::gui::ui_text("Yerel minOs yardım sayfası açıldı.", "Local minOs help page opened.");
            }
            "minos://cats" => {
                self.active_tab = 1;
                self.page_loaded = true;
                self.status = crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened.");
            }
            "minos://net" | "net://status" => {
                // Ağ durumu paneli — e1000 durumu + gateway ARP/ping testi
                self.active_tab = 2;
                self.net_online = crate::drivers::e1000::is_ready();
                if self.net_online {
                    // Gateway'e ARP çözümlemesi ve ICMP Echo gönder
                    let gateway=crate::drivers::netstack::gateway_ip();
                    let sent = crate::drivers::netstack::ping(gateway);
                    self.ping_ok = sent || crate::drivers::netstack::arp_lookup(gateway).is_some();
                    self.page_loaded = true;
                    self.status = crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent).");
                } else {
                    self.page_loaded = false;
                    self.status = crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive.");
                }
            }
            _ => {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Desteklenen: http:// ve minos:// adresleri.", "Supported schemes: http:// and minos://.");
            }
        }
        self.address_focused = false;
    }

    pub fn navigate_post(&mut self, url_bytes: &[u8], post_data: &[u8]) {
        self.set_url(url_bytes);
        self.navigate();
        self.is_post = true;
        self.post_body.clear();
        self.post_body.extend_from_slice(post_data);
    }

    /// Ana döngüden çağrılır; ağ bekleyişi ekran olaylarını durdurmaz.
    pub fn poll_network(&mut self) {
        if !self.request_pending { return; }
        let now = crate::main_monotonic_ms();
        if self.request_deadline == 0 { self.request_deadline = now.wrapping_add(15000); }
        if now.wrapping_sub(self.request_deadline) < usize::MAX / 2 {
            self.request_pending = false;
            self.page_loaded = false;
            self.status = crate::gui::ui_text("Ağ isteği zaman aşımına uğradı.", "Network request timed out.");
            crate::drivers::netstack::tcp_clear_result();
            self.request_state = 0;
            self.request_deadline = 0;
            return;
        }
        match self.request_state {
            0 => {
                let host = core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                if let Some(ip) = crate::drivers::netstack::parse_ipv4(host) {
                    self.request_ip = ip;
                    let port = self.request_port;
                    if crate::drivers::netstack::tcp_begin(ip, port, 49153) {
                        self.request_state = 1;
                        self.status = crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP...");
                    } else {
                        self.finish_network_error(crate::gui::ui_text("TCP bağlantısı başlatılamadı.", "Could not start TCP connection."));
                    }
                } else if crate::drivers::netstack::dns_query(host) {
                    self.request_state = 4;
                    self.status = crate::gui::ui_text("DNS yanıtı bekleniyor...", "Waiting for DNS response...");
                } else {
                    self.finish_network_error(crate::gui::ui_text("DNS sorgusu gönderilemedi.", "DNS query could not be sent."));
                }
            }
            4 => {
                if let Some(ip) = crate::drivers::netstack::take_dns_result() {
                    self.request_ip = ip;
                    if crate::drivers::netstack::tcp_begin(ip, self.request_port, 49153) {
                        self.request_state = 1;
                        self.status = crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP...");
                    } else {
                        self.finish_network_error(crate::gui::ui_text("TCP bağlantısı başlatılamadı.", "Could not start TCP connection."));
                    }
                }
            }
            1 => {
                if crate::drivers::netstack::tcp_reset_received() {
                    self.finish_network_error(crate::gui::ui_text("Sunucu bağlantıyı reddetti.", "Server rejected the connection."));
                } else if crate::drivers::netstack::tcp_syn_acked() {
                    let host = core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                    let path = core::str::from_utf8(&self.request_path[..self.request_path_len]).unwrap_or("/");

                    let mut req_buf: Vec<u8> = Vec::with_capacity(1024);
                    if self.is_post {
                        req_buf.extend_from_slice(b"POST ");
                    } else {
                        req_buf.extend_from_slice(b"GET ");
                    }
                    req_buf.extend_from_slice(path.as_bytes());
                    req_buf.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                    req_buf.extend_from_slice(host.as_bytes());
                    req_buf.extend_from_slice(b"\r\nUser-Agent: minOs/1.5\r\nAccept: text/html,application/xhtml+xml,text/plain,*/*\r\nConnection: close\r\n");

                    // Cookie başlığı ekle (uygun domain ve path için)
                    let mut cookie_header = String::new();
                    for c in &self.cookies {
                        if (c.domain.is_empty() || host.ends_with(&c.domain))
                            && (c.path.is_empty() || path.starts_with(&c.path))
                        {
                            if !cookie_header.is_empty() {
                                cookie_header.push_str("; ");
                            }
                            cookie_header.push_str(&c.name);
                            cookie_header.push('=');
                            cookie_header.push_str(&c.value);
                        }
                    }
                    if !cookie_header.is_empty() {
                        req_buf.extend_from_slice(b"Cookie: ");
                        req_buf.extend_from_slice(cookie_header.as_bytes());
                        req_buf.extend_from_slice(b"\r\n");
                    }

                    if self.is_post {
                        req_buf.extend_from_slice(b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: ");
                        let mut len_digits = [0u8; 12];
                        let mut val = self.post_body.len();
                        let mut pos = 0;
                        if val == 0 {
                            len_digits[0] = b'0';
                            pos = 1;
                        } else {
                            while val > 0 && pos < len_digits.len() {
                                len_digits[pos] = b'0' + (val % 10) as u8;
                                val /= 10;
                                pos += 1;
                            }
                            len_digits[..pos].reverse();
                        }
                        req_buf.extend_from_slice(&len_digits[..pos]);
                        req_buf.extend_from_slice(b"\r\n\r\n");
                        req_buf.extend_from_slice(&self.post_body);
                    } else {
                        req_buf.extend_from_slice(b"\r\n");
                    }

                    if crate::drivers::netstack::tcp_send(&req_buf) {
                        self.request_state = 2;
                        self.web_request_sent = true;
                        self.status = crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response...");
                    }
                }
            }
            2 => {
                if self.response_data.len() < 65536 {
                    self.response_data.resize(65536, 0);
                }
                let room = self.response_data.len().saturating_sub(self.response_len);
                if room > 0 {
                    self.response_len += crate::drivers::netstack::take_tcp_data(&mut self.response_data[self.response_len..]);
                }
                if self.response_len > 0 {
                    if let Some(end) = find_bytes(&self.response_data[..self.response_len], b"\r\n\r\n") {
                        let headers = &self.response_data[..end];
                        let status_end = find_bytes(headers, b"\r\n").unwrap_or(end);
                        let status_line = &headers[..status_end];

                        // HTTP durum kodunu ayrıştır
                        let status_code = if status_line.len() >= 12 && &status_line[..5] == b"HTTP/" {
                            if let Some(sp) = status_line.iter().position(|&b| b == b' ') {
                                if status_line.len() >= sp + 4 {
                                    let d = |c: u8| (c as char).to_digit(10).unwrap_or(0) as u16;
                                    if status_line[sp + 1].is_ascii_digit() && status_line[sp + 2].is_ascii_digit() && status_line[sp + 3].is_ascii_digit() {
                                        Some(d(status_line[sp + 1]) * 100 + d(status_line[sp + 2]) * 10 + d(status_line[sp + 3]))
                                    } else { None }
                                } else { None }
                            } else { None }
                        } else { None };

                        // Set-Cookie başlıklarını ayrıştır ve sakla
                        let current_host_str = core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                        for line in headers.split(|b| *b == b'\n') {
                            let l = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
                            if let Some(colon) = l.iter().position(|&b| b == b':') {
                                if l[..colon].eq_ignore_ascii_case(b"Set-Cookie") {
                                    let cookie_str = core::str::from_utf8(&l[colon + 1..]).unwrap_or("").trim();
                                    if let Some(first_part) = cookie_str.split(';').next() {
                                        if let Some((k, v)) = first_part.split_once('=') {
                                            let name = k.trim();
                                            let val = v.trim();
                                            if !name.is_empty() {
                                                // Var olan çerezi güncelle veya yenisini ekle
                                                if let Some(pos) = self.cookies.iter().position(|c| c.name == name) {
                                                    self.cookies[pos].value = String::from(val);
                                                } else if self.cookies.len() < 32 {
                                                    self.cookies.push(HttpCookie {
                                                        name: String::from(name),
                                                        value: String::from(val),
                                                        domain: String::from(current_host_str),
                                                        path: String::from("/"),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // 3xx Yönlendirme (Redirect) Kontrolü
                        if let Some(code) = status_code {
                            if (code == 301 || code == 302 || code == 303 || code == 307 || code == 308) && self.redirect_count < 3 {
                                let mut loc_target: Option<&[u8]> = None;
                                for line in headers.split(|b| *b == b'\n') {
                                    let l = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
                                    if let Some(colon) = l.iter().position(|&b| b == b':') {
                                        let key = &l[..colon];
                                        if key.eq_ignore_ascii_case(b"Location") {
                                            let mut val = &l[colon + 1..];
                                            while val.first().map_or(false, |b| b.is_ascii_whitespace()) { val = &val[1..]; }
                                            while val.last().map_or(false, |b| b.is_ascii_whitespace()) { val = &val[..val.len() - 1]; }
                                            loc_target = Some(val);
                                            break;
                                        }
                                    }
                                }

                                if let Some(loc) = loc_target {
                                    let mut loc_buf = [0u8; 256];
                                    let loc_len = loc.len().min(loc_buf.len());
                                    loc_buf[..loc_len].copy_from_slice(&loc[..loc_len]);

                                    self.redirect_count += 1;
                                    crate::drivers::netstack::tcp_clear_result();
                                    self.response_len = 0;
                                    self.response_data.clear();
                                    self.response_data.resize(65536, 0);

                                    let loc_slice = &loc_buf[..loc_len];
                                    let loc_str = core::str::from_utf8(loc_slice).unwrap_or("");
                                    if loc_str.starts_with("https://") {
                                        self.finish_network_error(crate::gui::ui_text(
                                            "Yönlendirme HTTPS adresine yapıldı (TLS gerekli).",
                                            "Redirected to HTTPS (TLS required)."
                                        ));
                                        return;
                                    }

                                    if loc_str.starts_with("http://") {
                                        self.set_url(loc_slice);
                                        self.navigate();
                                    } else {
                                        let path_bytes = loc_slice;
                                        let p_len = path_bytes.len().min(256);
                                        self.request_path = [0; 256];
                                        if path_bytes.first() != Some(&b'/') {
                                            self.request_path[0] = b'/';
                                            self.request_path[1..1 + p_len.min(255)].copy_from_slice(&path_bytes[..p_len.min(255)]);
                                            self.request_path_len = 1 + p_len.min(255);
                                        } else {
                                            self.request_path[..p_len].copy_from_slice(&path_bytes[..p_len]);
                                            self.request_path_len = p_len;
                                        }
                                        self.request_state = 0;
                                        self.request_pending = true;
                                        self.request_deadline = now.wrapping_add(15000);
                                        self.status = crate::gui::ui_text("Yönlendiriliyor...", "Redirecting...");
                                    }
                                    return;
                                }
                            }
                        }

                        // Normal yanıt içeriği
                        self.page_title = [0; 64];
                        let title_len = self.request_host_len.min(64);
                        self.page_title[..title_len].copy_from_slice(&self.request_host[..title_len]);
                        self.page_title_len = title_len;

                        self.page_lines.clear();
                        let status_str = core::str::from_utf8(status_line).unwrap_or("HTTP Response");
                        self.page_lines.push(BrowserLine {
                            spans: alloc::vec![TextSpan {
                                text: String::from(status_str),
                                color: Color::ACCENT_BLUE,
                                is_link: false,
                                link_url: String::new(),
                            }],
                            tag: HtmlTagType::Heading2,
                            indent: 0,
                        });

                        // Transfer-Encoding: chunked kontrolü
                        let is_chunked = headers.split(|b| *b == b'\n').any(|line| {
                            let l = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
                            if let Some(colon) = l.iter().position(|&b| b == b':') {
                                if l[..colon].eq_ignore_ascii_case(b"Transfer-Encoding") {
                                    let val = &l[colon + 1..];
                                    return val.windows(7).any(|w| w.eq_ignore_ascii_case(b"chunked"));
                                }
                            }
                            false
                        });

                        let body_start = end + 4;
                        let raw_body_len = self.response_len.saturating_sub(body_start);
                        let mut body_bytes: Vec<u8> = Vec::with_capacity(raw_body_len);
                        body_bytes.extend_from_slice(&self.response_data[body_start..self.response_len]);

                        if is_chunked {
                            if let Some(decoded_len) = crate::drivers::http::decode_chunked(&mut body_bytes, raw_body_len) {
                                body_bytes.truncate(decoded_len);
                            }
                        }

                        if body_bytes.iter().any(|b| *b == b'<') {
                            render_html_to_lines(&body_bytes, 600, &mut self.page_lines, 256);
                        } else {
                            for line in split_http_lines(&body_bytes).take(256) {
                                self.page_lines.push(BrowserLine {
                                    spans: alloc::vec![TextSpan {
                                        text: String::from(line),
                                        color: Color::TEXT_MAIN,
                                        is_link: false,
                                        link_url: String::new(),
                                    }],
                                    tag: HtmlTagType::Normal,
                                    indent: 0,
                                });
                            }
                        }
                        self.page_loaded = true;
                    }
                }
                if crate::drivers::netstack::tcp_finished() || self.response_len >= self.response_data.len() {
                    if self.response_len > 0 && !self.page_loaded { self.finish_http_response(); }
                    if self.page_loaded {
                        self.status = crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received.");
                        self.request_pending = false;
                        self.request_state = 0;
                        self.request_deadline = 0;
                        crate::drivers::netstack::tcp_clear_result();
                    } else {
                        self.finish_network_error(crate::gui::ui_text("HTTP yanıtı okunamadı.", "Could not parse HTTP response."));
                    }
                }
            }
            _ => self.finish_network_error(crate::gui::ui_text("Ağ isteği başarısız.", "Network request failed.")),
        }
    }

    fn finish_http_response(&mut self) { self.page_loaded=true; self.status=crate::gui::ui_text("HTTP yanıtı alındı.","HTTP response received."); }
    fn finish_network_error(&mut self, status:&'static str) { self.request_pending=false; self.page_loaded=false; self.status=status; self.request_state=0; self.request_deadline=0; crate::drivers::netstack::tcp_clear_result(); }

    pub fn back(&mut self) {
        self.set_url(b"minos://help");
        self.navigate();
    }

    pub fn forward(&mut self) {
        self.set_url(b"minos://cats");
        self.navigate();
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize, scroll: &crate::gui::ScrollState) {
        let english = crate::gui::ui_english();
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR_INACTIVE);
        let tab0_bg = if self.active_tab == 0 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 8, win_y + 30, 140, 20, tab0_bg);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, crate::gui::ui_text("minOs Yardım", "minOs Help"), Color::TEXT_MAIN);
        if self.focused_control == 0 { fb.draw_rect_outline(win_x + 8, win_y + 30, 140, 20, Color::ACCENT_YELLOW); }

        let tab1_bg = if self.active_tab == 1 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 152, win_y + 30, 85, 20, tab1_bg);
        MinGuiCompositor::draw_string(fb, win_x + 158, win_y + 34, "KediWiki", Color::TEXT_MAIN);
        if self.focused_control == 1 { fb.draw_rect_outline(win_x + 152, win_y + 30, 85, 20, Color::ACCENT_YELLOW); }

        let tab3_bg = if self.active_tab == 3 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 241, win_y + 30, 100, 20, tab3_bg);
        MinGuiCompositor::draw_string(fb, win_x + 247, win_y + 34, "Web Sayfasi", Color::ACCENT_GREEN);
        if self.focused_control == 2 { fb.draw_rect_outline(win_x + 241, win_y + 30, 100, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 6, win_y + 52, win_w - 12, 28, Color::TITLE_BAR);
        fb.draw_rect(win_x + 12, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 60, "<", Color::TEXT_MAIN);
        if self.focused_control == 3 { fb.draw_rect_outline(win_x + 12, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 38, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 60, ">", Color::TEXT_MAIN);
        if self.focused_control == 4 { fb.draw_rect_outline(win_x + 38, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 64, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 60, "R", Color::ACCENT_GREEN);
        if self.focused_control == 5 { fb.draw_rect_outline(win_x + 64, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        let url_x = win_x + 94;
        let url_w = win_w.saturating_sub(160);
        fb.draw_rect(url_x, win_y + 56, url_w, 20, if self.address_focused { Color::WINDOW_BG } else { Color::DARK_BG });
        let char_width = MinGuiCompositor::text_width("M", 1).max(1);
        let visible_chars = url_w.saturating_sub(12).checked_div(char_width).unwrap_or(1).max(1);
        let mut addr_scroll = self.address_scroll.min(self.url_len);
        if self.address_focused && self.address_cursor < addr_scroll { addr_scroll = self.address_cursor; }
        if self.address_focused && self.address_cursor >= addr_scroll + visible_chars {
            addr_scroll = self.address_cursor + 1 - visible_chars;
        }
        let visible_end = (addr_scroll + visible_chars).min(self.url_len);
        let clip = (url_x + 4, win_y + 56, url_w.saturating_sub(8), 20);
        let text_x = url_x + 6;
        let selection = self.address_selection.map(|(a,b)| (a.min(b).max(addr_scroll), a.max(b).min(visible_end)))
            .filter(|(a,b)| a < b);
        if let Some((sel_start, sel_end)) = selection {
            let prefix = core::str::from_utf8(&self.url[addr_scroll..sel_start]).unwrap_or("");
            let selected = core::str::from_utf8(&self.url[sel_start..sel_end]).unwrap_or("");
            let suffix = core::str::from_utf8(&self.url[sel_end..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(fb, text_x, win_y + 60, prefix, Color::TEXT_MAIN, clip);
            let selection_x = text_x + MinGuiCompositor::text_width(prefix, 1);
            let selection_w = MinGuiCompositor::text_width(selected, 1);
            fb.draw_rect(selection_x, win_y + 56, selection_w, 20, Color::ACCENT_BLUE);
            MinGuiCompositor::draw_string_clipped(fb, selection_x, win_y + 60, selected,
                MinGuiCompositor::text_on(Color::ACCENT_BLUE), clip);
            MinGuiCompositor::draw_string_clipped(fb, selection_x + selection_w, win_y + 60, suffix, Color::TEXT_MAIN, clip);
        } else {
            let shown = core::str::from_utf8(&self.url[addr_scroll..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(fb, text_x, win_y + 60, shown, Color::TEXT_MAIN, clip);
        }
        if self.address_focused && selection.is_none() {
            let caret_end = self.address_cursor.min(self.url_len);
            let caret_start = addr_scroll.min(caret_end);
            let caret_prefix = core::str::from_utf8(&self.url[caret_start..caret_end]).unwrap_or("");
            let caret_x = text_x + MinGuiCompositor::text_width(caret_prefix, 1);
            if caret_x < url_x + url_w.saturating_sub(2) {
                fb.draw_rect(caret_x, win_y + 58, 1, 16, Color::ACCENT_YELLOW);
            }
        }
        if self.focused_control == 6 { fb.draw_rect_outline(url_x, win_y + 56, url_w, 20, Color::ACCENT_YELLOW); }

        let git_r = (win_x + win_w - 60, win_y + 56, 50, 20);
        fb.draw_rect(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 48, win_y + 60, "GIT", MinGuiCompositor::text_on(Color::ACCENT_BLUE));
        if self.focused_control == 7 { fb.draw_rect_outline(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_YELLOW); }

        let status_h = 20;
        let status_y = win_y + win_h.saturating_sub(status_h + 4);
        let page_x = win_x + 6;
        let page_y = win_y + 82;
        let page_w = win_w.saturating_sub(12);
        let page_h = status_y.saturating_sub(page_y + 4).max(120);
        fb.draw_rect(page_x, page_y, page_w, page_h, Color::DARK_BG);
        if self.page_loaded {
            if self.active_tab == 3 {
                // Canlı Web Sayfası Render Paneli (HTML DOM)
                fb.draw_rect(page_x + 8, page_y + 8, page_w.saturating_sub(16), 26, Color::TITLE_BAR);
                let title_str = core::str::from_utf8(&self.page_title[..self.page_title_len]).unwrap_or("Web Document");
                MinGuiCompositor::draw_string(fb, page_x + 16, page_y + 14, title_str, Color::ACCENT_GREEN);

                // Beyaz/Açık HTML Kağıt Alanı
                let doc_x = page_x + 8;
                let doc_y = page_y + 38;
                let doc_w = page_w.saturating_sub(16);
                let doc_h = page_h.saturating_sub(46);
                fb.draw_rect(doc_x, doc_y, doc_w, doc_h, Color::WINDOW_BG);

                let doc_clip = (doc_x, doc_y, doc_w, doc_h);
                let scroll_y = scroll.y;

                // Clear rendered links for hit testing
                let links_ptr = &self.rendered_links as *const Vec<RenderedLink> as *mut Vec<RenderedLink>;
                unsafe {
                    (*links_ptr).clear();
                }

                let mut cur_line_y = doc_y + 12;
                for line in &self.page_lines {
                    let line_h = match line.tag {
                        HtmlTagType::Heading1 => 26,
                        HtmlTagType::Heading2 => 22,
                        HtmlTagType::Heading3 => 20,
                        _ => 18,
                    };

                    let scale = match line.tag {
                        HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };

                    let visible = cur_line_y + line_h >= doc_y + scroll_y && cur_line_y <= doc_y + scroll_y + doc_h + 30;
                    let draw_y = cur_line_y.saturating_sub(scroll_y);

                    if visible {
                        let mut cur_x = doc_x + 12 + line.indent;

                        // Draw bullet for list items
                        if line.tag == HtmlTagType::ListItem {
                            let bullet_x = doc_x + 14;
                            let bullet_y = draw_y + (line_h / 2).saturating_sub(2);
                            if bullet_y >= doc_y && bullet_y + 5 <= doc_y + doc_h {
                                fb.draw_rect(bullet_x, bullet_y, 4, 4, Color::TEXT_MUTED);
                            }
                        }

                        for span in &line.spans {
                            let span_w = MinGuiCompositor::text_width(&span.text, scale);
                            let text_color = if span.is_link { Color::ACCENT_BLUE } else { span.color };

                            MinGuiCompositor::draw_string_scaled_clipped(fb, cur_x, draw_y, &span.text, text_color, scale, doc_clip);

                            if span.is_link {
                                let underline_y = draw_y + line_h.saturating_sub(3);
                                if underline_y >= doc_y && underline_y < doc_y + doc_h {
                                    let ul_w = span_w.min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x)));
                                    fb.draw_rect(cur_x, underline_y, ul_w, 1, Color::ACCENT_BLUE);
                                }
                                unsafe {
                                    (*links_ptr).push(RenderedLink {
                                        rect: (cur_x, draw_y, span_w, line_h),
                                        url: span.link_url.clone(),
                                    });
                                }
                            }
                            cur_x += span_w;
                        }
                    }

                    cur_line_y += line_h;
                }
            } else if self.active_tab == 2 {
                // Ağ durumu paneli
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("minOs | Ağ Durumu (e1000)", "minOs | Network Status (e1000)"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("ICMP / ARP tabanlı sınama paneli", "ICMP / ARP test panel"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                let net_str = match (self.net_online, english) { (true,true) => "e1000: ONLINE (MMIO active)", (false,true) => "e1000: OFFLINE", (true,false) => "e1000: ÇEVRIMICI (MMIO aktif)", (false,false) => "e1000: ÇEVRIMDIŞI" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, net_str, Color::ACCENT_GREEN);
                let ping_str = match (self.ping_ok, english) { (true,true) => "Gateway ARP: FOUND (10.0.2.2)", (false,true) => "Gateway ARP: WAITING / NO REPLY", (true,false) => "Gateway ARP: BULUNDU (10.0.2.2)", (false,false) => "Gateway ARP: BEKLIYOR / YANIT YOK" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, ping_str, Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 104, "IP: 10.0.2.15/24 GW: 10.0.2.2", Color::TEXT_MUTED);
            } else if self.active_tab == 0 {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("minOs | Yerel Yardım ve Belgeler", "minOs | Local Help and Documents"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats, minos://help, minos://net", "Local addresses: minos://cats, minos://help, minos://net"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("minos://net ile ağ durumunu sınayın.", "Check network status at minos://net."), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, crate::gui::ui_text("Yerel sayfalar adres çubuğundan açılabilir.", "Open local pages from the address bar."), Color::TEXT_MUTED);
            } else {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("KediWiki | Yerel Sayfa", "KediWiki | Local Page"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats ve minos://help", "Local addresses: minos://cats and minos://help"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("Kediler ve minOs!", "Cats and minOs!"), Color::TEXT_MAIN);
            }
        } else {
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 28, crate::gui::ui_text("Bu adres açılamadı.", "This address could not be opened."), Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, crate::gui::ui_text("Desteklenen adresler: minos://help, minos://cats, minos://net", "Supported: minos://help, minos://cats, minos://net"), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 72, crate::gui::ui_text("Yerel: minos://help | Ağ: minos://net", "Local: minos://help | Network: minos://net"), Color::TEXT_MUTED);
        }
        fb.draw_rect(win_x + 6, status_y, win_w.saturating_sub(12), status_h, Color::TASKBAR_BG);
        let status = match self.status {
            "minOs yerel yardım sayfası" | "minOs local help page" => crate::gui::ui_text("minOs yerel yardım sayfası", "minOs local help page"),
            "Yerel minOs yardım sayfası açıldı." | "Local minOs help page opened." => crate::gui::ui_text("Yerel minOs yardım sayfası açıldı.", "Local minOs help page opened."),
            "Yerel KediWiki sayfası açıldı." | "Local KediWiki page opened." => crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened."),
            "Ağ durumu sayfası açıldı (sınama gönderildi)." | "Network status page opened (test sent)." => crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent)."),
            "e1000 ağ kartı bulunamadı; ağ yığını pasif." | "e1000 adapter unavailable; network stack is inactive." => crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive."),
            "Geçersiz web adresi." | "Invalid web address." => crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address."),
            "Ağ kartı kullanılamıyor." | "Network adapter unavailable." => crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable."),
            "DNS çözümlemesi bekleniyor..." | "Resolving host..." => crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host..."),
            "DNS adı çözümlenemedi." | "DNS lookup failed." => crate::gui::ui_text("DNS adı çözümlenemedi.", "DNS lookup failed."),
            "TCP bağlantısı kuruluyor..." | "Connecting over TCP..." => crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP..."),
            "TCP bağlantısı başlatılamadı." | "Could not start TCP connection." => crate::gui::ui_text("TCP bağlantısı başlatılamadı.", "Could not start TCP connection."),
            "Sunucu bağlantıyı reddetti." | "Server rejected the connection." => crate::gui::ui_text("Sunucu bağlantıyı reddetti.", "Server rejected the connection."),
            "HTTP yanıtı bekleniyor..." | "Waiting for HTTP response..." => crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response..."),
            "HTTP yanıtı alındı." | "HTTP response received." => crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received."),
            "HTTP yanıtı okunamadı." | "Could not parse HTTP response." => crate::gui::ui_text("HTTP yanıtı okunamadı.", "Could not parse HTTP response."),
            "Ağ isteği zaman aşımına uğradı." | "Network request timed out." => crate::gui::ui_text("Ağ isteği zaman aşımına uğradı.", "Network request timed out."),
            "Desteklenen: http:// ve minos:// adresleri." | "Supported schemes: http:// and minos://." => crate::gui::ui_text("Desteklenen: http:// ve minos:// adresleri.", "Supported schemes: http:// and minos://."),
            "HTTPS/TLS bu çekirdek motorunda desteklenmiyor." | "HTTPS/TLS is not supported by this kernel renderer." => crate::gui::ui_text("HTTPS/TLS bu çekirdek motorunda desteklenmiyor.", "HTTPS/TLS is not supported by this kernel renderer."),
            "DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor." | "DNS query sent; preparing HTTP connection." => crate::gui::ui_text("DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor.", "DNS query sent; preparing HTTP connection."),
            "DNS sorgusu gönderilemedi." | "DNS query could not be sent." => crate::gui::ui_text("DNS sorgusu gönderilemedi.", "DNS query could not be sent."),
            _ => self.status,
        };
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 3, status, Color::TEXT_MUTED);
    }
}

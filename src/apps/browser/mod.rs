//! minOs tarayıcı uygulaması.
//!
//! Modül yapısı:
//! - `types`: paylaşılan veri tipleri (`TextSpan`, `BrowserLine` ...)
//! - `css`: CSS ayrıştırma ve stil eşleştirme
//! - `html`: HTML tokenize + yerleşim (`render_html_to_lines`)
//! - `crate::apps::js`: minJs betik motoru
//! - bu dosya: HTTP/TLS akışı, geçmiş ve `BrowserApp` pencere denetçisi
mod types;
mod css;
mod html;

pub use types::{
    html_color, Bookmark, BrowserLine, HtmlTagType, HttpCookie, RenderedLink, TextAlign, TextSpan,
    BOOKMARKS,
};

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

use html::{decode_bytes_to_string, eq_ascii, find_bytes, render_html_to_lines};

extern crate alloc;

fn split_http_lines(body:&[u8])->impl Iterator<Item=&str> {
    body.split(|b|*b==b'\n').filter_map(|line| {
        let line=if line.last()==Some(&b'\r'){&line[..line.len()-1]}else{line};
        if line.is_empty(){None}else{Some(core::str::from_utf8(line).unwrap_or("[binary or non-UTF8 content]"))}
    })
}

// ── Tarayıcı geçmişi ──────────────────────────────────────────────────────
// Son açılan adresler oturum boyunca saklanır; pencere kapatılıp yeniden
// açılsa bile geçmiş korunur (modül düzeyinde atomik diziler).
const HISTORY_MAX: usize = 32;
const HISTORY_URL_CAP: usize = 128;

static HISTORY: [[AtomicU32; HISTORY_URL_CAP]; HISTORY_MAX] =
    [const { [const { AtomicU32::new(0) }; HISTORY_URL_CAP] }; HISTORY_MAX];
static HISTORY_LEN: [AtomicUsize; HISTORY_MAX] = [const { AtomicUsize::new(0) }; HISTORY_MAX];
static HISTORY_COUNT: AtomicUsize = AtomicUsize::new(0);
static HISTORY_POS: AtomicUsize = AtomicUsize::new(0);

/// Belirtilen geçmiş girdisini (0 = en eski) metne çevirir.
fn history_copy(index: usize) -> Option<String> {
    let count = HISTORY_COUNT.load(Ordering::Acquire);
    if index >= count || index >= HISTORY_MAX { return None; }
    let len = HISTORY_LEN[index].load(Ordering::Acquire).min(HISTORY_URL_CAP);
    if len == 0 { return None; }
    let mut out = String::with_capacity(len);
    for slot in HISTORY[index].iter().take(len) {
        out.push(char::from_u32(slot.load(Ordering::Relaxed)).unwrap_or('\u{FFFD}'));
    }
    Some(out)
}

/// Kayıtlı geçmiş girdisi sayısı.
fn history_count() -> usize { HISTORY_COUNT.load(Ordering::Acquire) }

/// Yeni bir adresi geçmişe ekler (art arda yinelemeleri atlar).
fn history_push(url: &str) {
    if url.is_empty() { return; }
    let mut count = HISTORY_COUNT.load(Ordering::Acquire);
    // Atomic state can briefly be observed while a navigation update is in
    // progress. Always normalize it before using it as an array index.
    count = count.min(HISTORY_MAX);
    let pos = HISTORY_POS.load(Ordering::Acquire).min(count.saturating_sub(1));
    if count > 0 && pos < count {
        if let Some(cur) = history_copy(pos) {
            if cur == url { return; }
        }
    }
    // Geri gidildikten sonra yeni gezinme, ileri geçmişi budar.
    if count > 0 { count = pos.saturating_add(1).min(count); }
    // Kapasite dolduysa en eski girdiyi düşür.
    if count >= HISTORY_MAX {
        for i in 1..HISTORY_MAX {
            for j in 0..HISTORY_URL_CAP {
                HISTORY[i - 1][j].store(HISTORY[i][j].load(Ordering::Relaxed), Ordering::Relaxed);
            }
            HISTORY_LEN[i - 1].store(HISTORY_LEN[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        count = HISTORY_MAX - 1;
    }
    let mut len = 0;
    for (i, ch) in url.chars().take(HISTORY_URL_CAP).enumerate() {
        HISTORY[count][i].store(ch as u32, Ordering::Relaxed);
        len = i + 1;
    }
    HISTORY_LEN[count].store(len, Ordering::Release);
    count += 1;
    HISTORY_COUNT.store(count, Ordering::Release);
    HISTORY_POS.store(count - 1, Ordering::Release);
}

/// Bir önceki adrese döner; yoksa `None`.
fn history_back() -> Option<String> {
    let count = HISTORY_COUNT.load(Ordering::Acquire);
    let pos = HISTORY_POS.load(Ordering::Acquire);
    if count == 0 || pos == 0 || pos >= count { return None; }
    let target = pos - 1;
    HISTORY_POS.store(target, Ordering::Release);
    history_copy(target)
}

/// İleri geçmişte bir sonraki adrese gider; yoksa `None`.
fn history_forward() -> Option<String> {
    let count = HISTORY_COUNT.load(Ordering::Acquire);
    let pos = HISTORY_POS.load(Ordering::Acquire);
    if count == 0 || pos >= count || pos.saturating_add(1) >= count { return None; }
    let target = pos + 1;
    HISTORY_POS.store(target, Ordering::Release);
    history_copy(target)
}

/// Tüm tarayıcı geçmişini temizler.
pub fn history_clear() {
    HISTORY_COUNT.store(0, Ordering::Release);
    HISTORY_POS.store(0, Ordering::Release);
    for i in 0..HISTORY_MAX {
        HISTORY_LEN[i].store(0, Ordering::Release);
    }
}

/// Belirtilen indeksteki geçmiş kaydını siler ve kalanları kaydırır.
pub fn history_remove(index: usize) -> bool {
    let count = HISTORY_COUNT.load(Ordering::Acquire).min(HISTORY_MAX);
    if index >= count || count == 0 { return false; }
    for i in index + 1..count {
        for j in 0..HISTORY_URL_CAP {
            HISTORY[i - 1][j].store(HISTORY[i][j].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        HISTORY_LEN[i - 1].store(HISTORY_LEN[i].load(Ordering::Relaxed), Ordering::Relaxed);
    }
    HISTORY_LEN[count - 1].store(0, Ordering::Relaxed);
    let new_count = count - 1;
    HISTORY_COUNT.store(new_count, Ordering::Release);
    let pos = HISTORY_POS.load(Ordering::Acquire);
    if pos >= new_count && new_count > 0 {
        HISTORY_POS.store(new_count - 1, Ordering::Release);
    } else if new_count == 0 {
        HISTORY_POS.store(0, Ordering::Release);
    }
    true
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
    suppress_history: bool,
    pub page_loaded: bool,
    pub status: &'static str,
    pub net_online: bool,
    pub ping_ok: bool,
    pub web_request_sent: bool,
    pub page_title: [u8; 64],
    pub page_title_len: usize,
    pub page_lines: Vec<BrowserLine>,
    pub rendered_links: RefCell<Vec<RenderedLink>>,
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
    pub is_https: bool,
    pub is_post: bool,
    pub post_body: Vec<u8>,
    pub cookies: Vec<HttpCookie>,
    pub response_data: Vec<u8>,
    pub response_len: usize,
    pub tls_session: Option<crate::drivers::tls::TlsSession>,
    pub tls_app_data: Vec<u8>,
    /// El sıkışması tamamlanınca TLS üzerinden gönderilecek HTTP isteği.
    pub tls_pending_request: Vec<u8>,
    pub tls_request_sent: bool,
    pub page_bg_color: Option<u32>,
    pub js_alert: Option<String>,
    pub js_vars: Vec<(String, String)>,
    pub js_functions: Vec<(String, String)>,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128], url_len: 0, address_focused: false, address_cursor: 0, address_scroll: 0, address_selection: None,
            address_undo: [[0; 128]; 16], address_undo_len: [0; 16], address_undo_count: 0,
            address_redo: [[0; 128]; 16], address_redo_len: [0; 16], address_redo_count: 0,
            active_tab: 0, focused_control: 0, suppress_history: false, page_loaded: true, status: crate::gui::ui_text("minOs yerel yardım sayfası", "minOs local help page"),
            net_online: false, ping_ok: false,
            web_request_sent: false,
            page_title: [0; 64], page_title_len: 0,
            page_lines: Vec::new(),
            rendered_links: RefCell::new(Vec::new()),
            scroll_y: 0,
            request_pending: false, request_host: [0; 128], request_host_len: 0,
            request_path: [0; 256], request_path_len: 0, request_ip: crate::drivers::netstack::Ipv4Address([0; 4]), request_port: 80,
            request_deadline: 0, request_state: 0, redirect_count: 0,
            is_https: false,
            is_post: false, post_body: Vec::new(), cookies: Vec::new(),
            response_data: Vec::new(), response_len: 0,
            tls_session: None,
            tls_app_data: Vec::new(),
            tls_pending_request: Vec::new(),
            tls_request_sent: false,
            page_bg_color: None,
            js_alert: None,
            js_vars: Vec::new(),
            js_functions: Vec::new(),
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
        let mut clicked_url = None;
        let mut clicked_js = None;
        for link in self.rendered_links.borrow().iter() {
            let (rx, ry, rw, rh) = link.rect;
            if mx >= rx && mx < rx + rw && my >= ry && my < ry + rh {
                if !link.onclick.is_empty() {
                    clicked_js = Some(link.onclick.clone());
                } else if !link.url.is_empty() {
                    clicked_url = Some(link.url.clone());
                }
                break;
            }
        }
        if let Some(js_code) = clicked_js {
            self.execute_js(&js_code);
            return None;
        }
        clicked_url
    }

    pub fn execute_js(&mut self, code: &str) {
        for stmt in code.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                self.execute_js_statement(stmt);
            }
        }
    }

    fn eval_js_expr(&self, expr: &str) -> String {
        let expr = expr.trim();
        if (expr.starts_with('\'') && expr.ends_with('\'') && expr.len() >= 2)
            || (expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2) {
            return String::from(&expr[1..expr.len() - 1]);
        }
        for (var, val) in &self.js_vars {
            if var == expr {
                return val.clone();
            }
        }
        if expr.contains('+') {
            let mut result = String::new();
            for part in expr.split('+') {
                result.push_str(&self.eval_js_expr(part));
            }
            return result;
        }
        String::from(expr)
    }

    fn execute_js_statement(&mut self, stmt: &str) {
        let stmt = stmt.trim();
        if stmt.starts_with("alert(") && stmt.ends_with(')') {
            let inner = &stmt[6..stmt.len() - 1];
            let val = self.eval_js_expr(inner);
            self.js_alert = Some(val);
        } else if stmt.starts_with("console.log(") && stmt.ends_with(')') {
            // No-op
        } else if stmt.starts_with("document.title =") || stmt.starts_with("document.title=") {
            let val_part = if stmt.starts_with("document.title =") { &stmt[16..] } else { &stmt[15..] };
            let val = self.eval_js_expr(val_part);
            let vb = val.as_bytes();
            self.page_title_len = vb.len().min(64);
            self.page_title[..self.page_title_len].copy_from_slice(&vb[..self.page_title_len]);
        } else if stmt.starts_with("document.body.style.backgroundColor =") || stmt.starts_with("document.body.style.backgroundColor=")
            || stmt.starts_with("document.body.style.background =") || stmt.starts_with("document.body.style.background=") {
            let val_part = stmt.split('=').nth(1).unwrap_or("");
            let val = self.eval_js_expr(val_part);
            if let Some(c) = html_color(val.as_bytes()) {
                self.page_bg_color = Some(c);
            }
        } else if stmt.starts_with("document.getElementById(") {
            if let Some(close_paren) = stmt.find(')') {
                let id_expr = &stmt[24..close_paren];
                let id = self.eval_js_expr(id_expr);
                let rest = stmt[close_paren + 1..].trim();
                if rest.starts_with(".innerText =") || rest.starts_with(".innerText=")
                    || rest.starts_with(".innerHTML =") || rest.starts_with(".innerHTML=")
                    || rest.starts_with(".textContent =") || rest.starts_with(".textContent=") {
                    let val_part = rest.split('=').nth(1).unwrap_or("");
                    let val = self.eval_js_expr(val_part);
                    for line in &mut self.page_lines {
                        for span in &mut line.spans {
                            if span.elem_id == id {
                                span.text = val.clone();
                            }
                        }
                    }
                } else if rest.starts_with(".style.color =") || rest.starts_with(".style.color=") {
                    let val_part = rest.split('=').nth(1).unwrap_or("");
                    let val = self.eval_js_expr(val_part);
                    if let Some(c) = html_color(val.as_bytes()) {
                        for line in &mut self.page_lines {
                            for span in &mut line.spans {
                                if span.elem_id == id {
                                    span.color = c;
                                }
                            }
                        }
                    }
                }
            }
        } else if stmt.starts_with("var ") || stmt.starts_with("let ") {
            let decl = &stmt[4..];
            if let Some((var, val_part)) = decl.split_once('=') {
                let var = var.trim();
                let val = self.eval_js_expr(val_part);
                if let Some(existing) = self.js_vars.iter_mut().find(|(k, _)| k == var) {
                    existing.1 = val;
                } else {
                    self.js_vars.push((String::from(var), val));
                }
            }
        } else if let Some((var, val_part)) = stmt.split_once('=') {
            let var = var.trim();
            if !var.contains(' ') && !var.contains('.') {
                let val = self.eval_js_expr(val_part);
                if let Some(existing) = self.js_vars.iter_mut().find(|(k, _)| k == var) {
                    existing.1 = val;
                } else {
                    self.js_vars.push((String::from(var), val));
                }
            }
        }
    }

    pub fn load_html(&mut self, body: &[u8], wrap_width_px: usize, max_lines: usize) {
        self.page_lines.clear();
        self.rendered_links.borrow_mut().clear();
        self.page_bg_color = None;
        self.js_alert = None;
        self.js_vars.clear();

        // Preprocess: Extract <script> tags and handle document.write()
        let mut preprocessed_html = Vec::with_capacity(body.len());
        let mut p = 0;
        while p < body.len() {
            if body[p] == b'<' && p + 7 <= body.len() && eq_ascii(&body[p..p + 7], b"<script") {
                let start = p + 7;
                let mut close_tag = start;
                while close_tag < body.len() && body[close_tag] != b'>' { close_tag += 1; }
                if close_tag < body.len() {
                    close_tag += 1;
                    let script_start = close_tag;
                    if let Some(pos) = find_bytes(&body[script_start..], b"</script>") {
                        let script_code = decode_bytes_to_string(&body[script_start..script_start + pos]);
                        for stmt in script_code.split(';') {
                            let stmt = stmt.trim();
                            if stmt.starts_with("document.write(") && stmt.ends_with(')') {
                                let inner = &stmt[15..stmt.len() - 1];
                                let written = self.eval_js_expr(inner);
                                preprocessed_html.extend_from_slice(written.as_bytes());
                            } else if !stmt.is_empty() {
                                self.execute_js_statement(stmt);
                            }
                        }
                        p = script_start + pos + 9;
                        continue;
                    }
                }
            }
            preprocessed_html.push(body[p]);
            p += 1;
        }

        render_html_to_lines(&preprocessed_html, wrap_width_px, &mut self.page_lines, max_lines);
    }

    pub fn focus_next_control(&mut self) {
        // 0: Geri, 1: İleri, 2: Yenile, 3: Geçmiş, 4: Adres çubuğu, 5: GIT
        self.focused_control = (self.focused_control + 1) % 6;
        self.address_focused = self.focused_control == 4;
    }

    pub fn focus_prev_control(&mut self) {
        self.focused_control = (self.focused_control + 5) % 6;
        self.address_focused = self.focused_control == 4;
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
        self.focused_control = 4;
        let column = x.saturating_sub(field_x) / 10;
        self.address_cursor = (self.address_scroll + column).min(self.url_len);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn focus_address_end(&mut self) {
        self.address_focused = true;
        self.focused_control = 4;
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
                    bg_color: None,
                    is_link: false,
                    link_url: String::new(),
                    is_button: false,
                    bold: false,
                    underline: false,
                    onclick: String::new(),
                    elem_id: String::new(),
                }],
                tag: HtmlTagType::Normal,
                indent: 0,
                align: TextAlign::Left,
            });
        }
    }

    pub fn build_history_page(&mut self) {
        self.page_title = [0; 64];
        let title = crate::gui::ui_text("Tarayıcı Geçmişi", "Browser History");
        let tb = title.as_bytes();
        self.page_title_len = tb.len().min(64);
        self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);

        self.page_lines.clear();
        let count = history_count().min(HISTORY_MAX);

        let mut header_spans = alloc::vec![TextSpan {
            text: String::from(crate::gui::ui_text(
                "Son açılan bağlantılar (en yeni en üstte):",
                "Recently opened links (newest first):",
            )),
            color: Color::TEXT_MUTED,
            bg_color: None,
            is_link: false,
            link_url: String::new(),
            is_button: false,
            bold: false,
            underline: false,
            onclick: String::new(),
            elem_id: String::new(),
        }];

        if count > 0 {
            header_spans.push(TextSpan {
                text: String::from("   "),
                color: Color::TEXT_MUTED,
                bg_color: None,
                is_link: false,
                link_url: String::new(),
                is_button: false,
            bold: false,
            underline: false,
                onclick: String::new(),
                elem_id: String::new(),
            });
            header_spans.push(TextSpan {
                text: String::from(crate::gui::ui_text("[ Tümünü Temizle ]", "[ Clear All History ]")),
                color: Color::ACCENT_RED,
                bg_color: Some(Color::TITLE_BAR),
                is_link: true,
                link_url: String::from("minos://history?action=clear"),
                is_button: true,
                bold: false,
                underline: false,
                onclick: String::new(),
                elem_id: String::new(),
            });
        }

        self.page_lines.push(BrowserLine {
            spans: header_spans,
            tag: HtmlTagType::Heading3,
            indent: 0,
            align: TextAlign::Left,
        });

        if count == 0 {
            self.page_lines.push(BrowserLine {
                spans: alloc::vec![TextSpan {
                    text: String::from(crate::gui::ui_text("Henüz geçmiş yok.", "No history yet.")),
                    color: Color::TEXT_MAIN,
                    bg_color: None,
                    is_link: false,
                    link_url: String::new(),
                    is_button: false,
                    bold: false,
                    underline: false,
                    onclick: String::new(),
                    elem_id: String::new(),
                }],
                tag: HtmlTagType::Normal,
                indent: 0,
                align: TextAlign::Left,
            });
            return;
        }

        for i in (0..count).rev() {
            if let Some(url) = history_copy(i) {
                let mut remove_action = String::from("minos://history?action=delete&idx=");
                let mut num_buf = [0u8; 16];
                let num_str = {
                    let mut n = i;
                    let len = if n == 0 {
                        num_buf[0] = b'0';
                        1
                    } else {
                        let mut temp = [0u8; 16];
                        let mut tlen = 0;
                        while n > 0 {
                            temp[tlen] = b'0' + (n % 10) as u8;
                            n /= 10;
                            tlen += 1;
                        }
                        for j in 0..tlen {
                            num_buf[j] = temp[tlen - 1 - j];
                        }
                        tlen
                    };
                    core::str::from_utf8(&num_buf[..len]).unwrap_or("0")
                };
                remove_action.push_str(num_str);

                self.page_lines.push(BrowserLine {
                    spans: alloc::vec![
                        TextSpan {
                            text: String::from("[X] "),
                            color: Color::ACCENT_RED,
                            bg_color: Some(Color::TITLE_BAR),
                            is_link: true,
                            link_url: remove_action,
                            is_button: true,
                            bold: false,
                            underline: false,
                            onclick: String::new(),
                            elem_id: String::new(),
                        },
                        TextSpan {
                            text: String::from(" "),
                            color: Color::TEXT_MUTED,
                            bg_color: None,
                            is_link: false,
                            link_url: String::new(),
                            is_button: false,
                    bold: false,
                    underline: false,
                            onclick: String::new(),
                            elem_id: String::new(),
                        },
                        TextSpan {
                            text: url.clone(),
                            color: Color::TEXT_MAIN,
                            bg_color: None,
                            is_link: true,
                            link_url: url,
                            is_button: false,
                    bold: false,
                    underline: false,
                            onclick: String::new(),
                            elem_id: String::new(),
                        }
                    ],
                    tag: HtmlTagType::ListItem,
                    indent: 8,
                    align: TextAlign::Left,
                });
            }
        }
    }

    pub fn build_bookmarks_page(&mut self) {
        self.page_title = [0; 64];
        let title = crate::gui::ui_text("Sık Kullanılanlar", "Bookmarks");
        let tb = title.as_bytes();
        self.page_title_len = tb.len().min(64);
        self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);

        self.page_lines.clear();

        self.page_lines.push(BrowserLine {
            spans: alloc::vec![TextSpan {
                text: String::from(crate::gui::ui_text(
                    "minOs Tarayıcı | Sık Kullanılanlar ve Yer İmleri",
                    "minOs Browser | Favorites & Bookmarks",
                )),
                color: Color::ACCENT_PINK,
                bg_color: None,
                is_link: false,
                link_url: String::new(),
                is_button: false,
                bold: true,
                underline: false,
                onclick: String::new(),
                elem_id: String::new(),
            }],
            tag: HtmlTagType::Heading2,
            indent: 0,
            align: TextAlign::Left,
        });

        self.page_lines.push(BrowserLine {
            spans: alloc::vec![TextSpan {
                text: String::from(crate::gui::ui_text(
                    "Aşağıdaki bağlantılara tıklayarak doğrudan web ve yerel sayfalara gidebilirsiniz:",
                    "Click on any bookmark link below to navigate directly:",
                )),
                color: Color::TEXT_MUTED,
                bg_color: None,
                is_link: false,
                link_url: String::new(),
                is_button: false,
                bold: false,
                underline: false,
                onclick: String::new(),
                elem_id: String::new(),
            }],
            tag: HtmlTagType::Normal,
            indent: 0,
            align: TextAlign::Left,
        });

        self.page_lines.push(BrowserLine {
            spans: alloc::vec![],
            tag: HtmlTagType::Normal,
            indent: 0,
            align: TextAlign::Left,
        });

        for bm in BOOKMARKS {
            let title_text = crate::gui::ui_text(bm.title, bm.title_en);
            let desc_text = crate::gui::ui_text(bm.description, bm.description_en);
            let is_web = bm.url.starts_with("http");

            self.page_lines.push(BrowserLine {
                spans: alloc::vec![
                    TextSpan {
                        text: String::from(if is_web { "* [WEB] " } else { "* [LOCAL] " }),
                        color: if is_web { Color::ACCENT_YELLOW } else { Color::ACCENT_BLUE },
                        bg_color: None,
                        is_link: false,
                        link_url: String::new(),
                        is_button: false,
                        bold: true,
                        underline: false,
                        onclick: String::new(),
                        elem_id: String::new(),
                    },
                    TextSpan {
                        text: String::from(title_text),
                        color: Color::WHITE,
                        bg_color: Some(if is_web { 0x188040 } else { Color::TITLE_BAR }),
                        is_link: true,
                        link_url: String::from(bm.url),
                        is_button: true,
                        bold: true,
                        underline: false,
                        onclick: String::new(),
                        elem_id: String::new(),
                    },
                    TextSpan {
                        text: alloc::format!("  {}", bm.url),
                        color: Color::ACCENT_BLUE,
                        bg_color: None,
                        is_link: true,
                        link_url: String::from(bm.url),
                        is_button: false,
                        bold: false,
                        underline: true,
                        onclick: String::new(),
                        elem_id: String::new(),
                    },
                ],
                tag: HtmlTagType::Heading3,
                indent: 4,
                align: TextAlign::Left,
            });

            self.page_lines.push(BrowserLine {
                spans: alloc::vec![TextSpan {
                    text: alloc::format!("      {}", desc_text),
                    color: Color::TEXT_MUTED,
                    bg_color: None,
                    is_link: false,
                    link_url: String::new(),
                    is_button: false,
                    bold: false,
                    underline: false,
                    onclick: String::new(),
                    elem_id: String::new(),
                }],
                tag: HtmlTagType::Normal,
                indent: 8,
                align: TextAlign::Left,
            });

            self.page_lines.push(BrowserLine {
                spans: alloc::vec![],
                tag: HtmlTagType::Normal,
                indent: 0,
                align: TextAlign::Left,
            });
        }
    }

    pub fn url_matches(&self, target: &str) -> bool {
        let current = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        if current.eq_ignore_ascii_case(target) {
            return true;
        }
        let t_trimmed = target.trim_end_matches('/');
        let c_trimmed = current.trim_end_matches('/');
        !t_trimmed.is_empty() && c_trimmed.eq_ignore_ascii_case(t_trimmed)
    }

    pub fn click_bookmark_at(&self, mx: usize, my: usize, win_x: usize, win_y: usize, win_w: usize) -> Option<&'static str> {
        let bbar_y = win_y + 59;
        let bbar_h = 22;
        if my < bbar_y || my >= bbar_y + bbar_h {
            return None;
        }
        let star_w = MinGuiCompositor::text_width("*", 1);
        let mut cur_bx = win_x + 12 + star_w + 6;
        for bm in BOOKMARKS {
            let label = crate::gui::ui_text(bm.title, bm.title_en);
            let tw = MinGuiCompositor::text_width(label, 1);
            let chip_w = tw + 12;
            if cur_bx + chip_w > win_x + win_w - 10 {
                break;
            }
            if mx >= cur_bx && mx < cur_bx + chip_w {
                return Some(bm.url);
            }
            cur_bx += chip_w + 5;
        }
        None
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
        self.focused_control = 4;
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
        let suppress = self.suppress_history;
        self.suppress_history = false;
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
        let (is_https, rest_opt) = if let Some(rest) = value.strip_prefix("https://") {
            (true, Some(rest))
        } else if let Some(rest) = value.strip_prefix("http://") {
            (false, Some(rest))
        } else {
            (false, None)
        };

        if let Some(rest) = rest_opt {
            let authority_end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let suffix = &rest[authority_end..];
            let suffix = suffix.split('#').next().unwrap_or("");
            let default_port = if is_https { 443 } else { 80 };
            let (host, port) = if authority.starts_with('[') {
                ("", 0) // Bu ağ yığını şu an yalnızca IPv4 hedeflerini destekliyor.
            } else {
                match authority.rsplit_once(':') {
                    Some((h,p)) => match p.parse::<u16>() { Ok(n) if n>0 => (h,n), _ => ("",0) },
                    _ => (authority, default_port),
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
                self.is_https = is_https;
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
                self.tls_session = None;
                self.tls_pending_request.clear();
                self.tls_request_sent = false;
                self.tls_app_data.clear();
                self.response_len = 0;
                self.response_data.clear();
                self.response_data.resize(65536, 0);
                self.status = if is_https {
                    crate::gui::ui_text("Güvenli bağlantı (HTTPS/TLS) başlatılıyor...", "Starting secure (HTTPS/TLS) connection...")
                } else {
                    crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host...")
                };
                self.page_title = [0; 64];
                self.page_title_len = 0;
                self.page_lines.clear();
                if !suppress { history_push(value); }
            }
            self.address_focused = false;
            return;
        }
        match value {
            "minos://help" => {
                self.active_tab = 0;
                self.page_loaded = true;
                if !suppress { history_push(value); }
                self.status = crate::gui::ui_text("Yerel minOs yardım sayfası açıldı.", "Local minOs help page opened.");
            }
            "minos://cats" => {
                self.active_tab = 1;
                self.page_loaded = true;
                if !suppress { history_push(value); }
                self.status = crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened.");
            }
            s if s.starts_with("minos://history") => {
                if s == "minos://history?action=clear" {
                    history_clear();
                    self.status = crate::gui::ui_text("Tüm geçmiş temizlendi.", "All history cleared.");
                } else if let Some(idx_str) = s.strip_prefix("minos://history?action=delete&idx=") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        history_remove(idx);
                        self.status = crate::gui::ui_text("Geçmiş kaydı silindi.", "History entry deleted.");
                    }
                } else {
                    self.status = crate::gui::ui_text("Geçmiş sayfası açıldı.", "History page opened.");
                }
                self.active_tab = 3;
                self.page_loaded = true;
                self.set_url(b"minos://history");
                self.build_history_page();
            }
            "minos://bookmarks" | "minos://favorites" => {
                self.active_tab = 3;
                self.page_loaded = true;
                if !suppress { history_push(value); }
                self.status = crate::gui::ui_text("Sık kullanılanlar sayfası açıldı.", "Bookmarks page opened.");
                self.set_url(b"minos://bookmarks");
                self.build_bookmarks_page();
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
                if self.page_loaded && !suppress { history_push(value); }
            }
            "minos://demo" | "minos://tr" => {
                self.active_tab = 3;
                self.page_loaded = true;
                let title = "minOs Türkçe & Web Motoru Testi";
                let tb = title.as_bytes();
                self.page_title_len = tb.len().min(64);
                self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);
                let demo_html = b"<html><head><style>h1 { color: #D03030; text-align: center; } .sub { color: #188040; text-align: center; } .btn { color: #FFFFFF; background-color: #2050C0; }</style></head><body><h1>minOs Web & TR Destegi</h1><p class='sub'>Turkce Karakterler: \xc3\xa7 \xc4\x9f \xc4\xb1 \xc3\xb6 \xc5\x9f \xc3\xbc - \xc3\x87 \xc4\x9e \xc4\xb0 \xc3\x96 \xc5\x9e \xc3\x9c</p><p id='counter_msg' style='color: #2050C0;'>Etkilesimli JavaScript Testi:</p><p><button class='btn' onclick=\"document.getElementById('counter_msg').innerText = 'Dugmeye tiklandi! JS calisiyor.'; alert('minOs JS Uyari: Dugmeye basildi!');\">Tikla (JS Test)</button></p><p><button onclick=\"document.getElementById('counter_msg').style.color = '#D03030';\">Kirmizi Yap</button> <button onclick=\"document.getElementById('counter_msg').style.color = '#188040';\">Yesil Yap</button></p><hr><script>document.write('<p style=\"color: #D0A020;\">Bu satir JavaScript document.write() ile olusturuldu!</p>');</script></body></html>";
                self.load_html(demo_html, 600, 256);
                self.status = crate::gui::ui_text("Türkçe & CSS & JS test sayfası yüklendi.", "Turkish & CSS & JS demo page loaded.");
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
        let now = crate::boot::time::main_monotonic_ms();
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

                    if self.is_https {
                        self.tls_pending_request = req_buf;
                        self.tls_request_sent = false;
                        let mut client_random = [0u8; 32];
                        let seed = crate::boot::time::main_monotonic_ms() as u64;
                        let mut state = seed ^ 0x9e3779b97f4a7c15u64;
                        for b in client_random.iter_mut() {
                            // xorshift64* tabanlı daha iyi karıştırma
                            state ^= state >> 12;
                            state ^= state << 25;
                            state ^= state >> 27;
                            *b = (state.wrapping_mul(0x2545F4914F6CDD1D) >> 56) as u8;
                        }
                        let mut hello = Vec::new();
                        crate::drivers::tls::build_client_hello(host, &client_random, &mut hello);
                        let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
                        let mut tls_session = crate::drivers::tls::TlsSession::new(host, client_random);
                        tls_session.note_client_hello(ch_body);
                        if crate::drivers::netstack::tcp_send(&hello) {
                            self.tls_session = Some(tls_session);
                            self.request_state = 2;
                            self.web_request_sent = true;
                            self.status = crate::gui::ui_text("TLS el sıkışması ve yanıt bekleniyor...", "Waiting for TLS handshake & response...");
                        } else {
                            self.finish_network_error(crate::gui::ui_text("TLS ClientHello gönderilemedi.", "Failed to send TLS ClientHello."));
                        }
                    } else {
                        if crate::drivers::netstack::tcp_send(&req_buf) {
                            self.request_state = 2;
                            self.web_request_sent = true;
                            self.status = crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response...");
                        }
                    }
                }
            }
            2 => {
                // TCP verisini al ve TLS oturumuna besle (https için).
                if self.response_data.len() < 65536 {
                    self.response_data.resize(65536, 0);
                }
                let room = self.response_data.len().saturating_sub(self.response_len);
                if room > 0 {
                    let n = crate::drivers::netstack::take_tcp_data(&mut self.response_data[self.response_len..]);
                    if n > 0 {
                        self.response_len += n;
                        if let Some(ref mut tls) = self.tls_session {
                            tls.feed_tcp_data(&self.response_data[self.response_len - n..self.response_len]);
                        }
                    }
                }

                // TLS oturumundan giden mesajları (ChangeCipherSpec + Finished) gönder.
                if let Some(ref mut tls) = self.tls_session {
                    let outgoing = tls.take_outgoing();
                    if !outgoing.is_empty() {
                        let _ = crate::drivers::netstack::tcp_send(&outgoing);
                    }
                }

                // TLS kayıtlarını işle: el sıkışma mesajları ve şifreli application data.
                let mut tls_failed: Option<Option<(u8, u8)>> = None;
                if let Some(ref mut tls) = self.tls_session {
                    let mut hs_out = Vec::new();
                    let mut app_out = Vec::new();
                    match tls.process_records(&mut hs_out, &mut app_out) {
                        Ok(_) => {}
                        Err(()) => {
                            tls_failed = Some(tls.last_alert());
                        }
                    }
                    self.tls_app_data.extend_from_slice(&app_out);
                }
                if let Some(alert) = tls_failed {
                    let detail: &'static str = match alert {
                        Some((_, desc)) => crate::drivers::tls::alert_description_text(desc),
                        None => crate::gui::ui_text("TLS kayıt işleme hatası.", "TLS record processing error."),
                    };
                    self.finish_network_error(detail);
                }

                // El sıkışma tamamlandıysa HTTP isteğini şifreleyip gönder.
                let tls_established = self.tls_session.as_ref().map_or(false, |t| t.state() == crate::drivers::tls::TlsState::Established);
                if tls_established && !self.tls_request_sent && !self.tls_pending_request.is_empty() {
                    let request = core::mem::take(&mut self.tls_pending_request);
                    let record = self.tls_session.as_mut().and_then(|t| t.encrypt_app_data(&request));
                    match record {
                        Some(rec) => {
                            let _ = crate::drivers::netstack::tcp_send(&rec);
                            self.tls_request_sent = true;
                            self.status = crate::gui::ui_text("Güvenli istek gönderildi, yanıt bekleniyor...", "Secure request sent, waiting for response...");
                        }
                        None => {
                            self.finish_network_error(crate::gui::ui_text(
                                "TLS isteği şifrelenemedi.",
                                "Could not encrypt TLS request.",
                            ));
                        }
                    }
                }
                let data_to_parse = if tls_established && !self.tls_app_data.is_empty() {
                    &self.tls_app_data
                } else {
                    &self.response_data[..self.response_len]
                };

                if data_to_parse.len() > 0 {
                    if let Some(end) = find_bytes(data_to_parse, b"\r\n\r\n") {
                        let headers = &data_to_parse[..end];
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
                                    self.tls_app_data.clear();
                                    self.tls_session = None;
                self.tls_pending_request.clear();
                self.tls_request_sent = false;

                                    let loc_slice = &loc_buf[..loc_len];
                                    let loc_str = core::str::from_utf8(loc_slice).unwrap_or("");
                                    if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
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
                                bg_color: None,
                                is_link: false,
                                link_url: String::new(),
                                is_button: false,
                    bold: false,
                    underline: false,
                                onclick: String::new(),
                                elem_id: String::new(),
                            }],
                            tag: HtmlTagType::Heading2,
                            indent: 0,
                            align: TextAlign::Left,
                        });

                        let body_start = end + 4;
                        let raw_body_len = data_to_parse.len().saturating_sub(body_start);
                        let mut body_bytes: Vec<u8> = Vec::with_capacity(raw_body_len);
                        body_bytes.extend_from_slice(&data_to_parse[body_start..data_to_parse.len()]);

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

                        if is_chunked {
                            if let Some(decoded_len) = crate::drivers::http::decode_chunked(&mut body_bytes, raw_body_len) {
                                body_bytes.truncate(decoded_len);
                            }
                        }

                        if body_bytes.iter().any(|b| *b == b'<') {
                            self.load_html(&body_bytes, 600, 256);
                        } else {
                            for line in split_http_lines(&body_bytes).take(256) {
                                self.page_lines.push(BrowserLine {
                                    spans: alloc::vec![TextSpan {
                                        text: String::from(line),
                                        color: Color::TEXT_MAIN,
                                        bg_color: None,
                                        is_link: false,
                                        link_url: String::new(),
                                        is_button: false,
                    bold: false,
                    underline: false,
                                        onclick: String::new(),
                                        elem_id: String::new(),
                                    }],
                                    tag: HtmlTagType::Normal,
                                    indent: 0,
                                    align: TextAlign::Left,
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
        if let Some(url) = history_back() {
            self.set_url(url.as_bytes());
            self.suppress_history = true;
            self.navigate();
        }
    }

    pub fn forward(&mut self) {
        if let Some(url) = history_forward() {
            self.set_url(url.as_bytes());
            self.suppress_history = true;
            self.navigate();
        }
    }

    pub fn show_history(&mut self) {
        self.set_url(b"minos://history");
        self.navigate();
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize, scroll: &crate::gui::ScrollState) {
        let english = crate::gui::ui_english();
        // Bağlantı isabet alanlarını sıfırla; yalnızca HTML içerik yeniden doldurur.
        self.rendered_links.borrow_mut().clear();
        // Araç çubuğu (üst sekmeler kaldırıldı): Geri, İleri, Yenile, Geçmiş, adres, GIT
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 30, Color::TITLE_BAR);
        fb.draw_rect(win_x + 12, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 36, "<", Color::TEXT_MAIN);
        if self.focused_control == 0 { fb.draw_rect_outline(win_x + 12, win_y + 32, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 38, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 36, ">", Color::TEXT_MAIN);
        if self.focused_control == 1 { fb.draw_rect_outline(win_x + 38, win_y + 32, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 64, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 36, "R", Color::ACCENT_GREEN);
        if self.focused_control == 2 { fb.draw_rect_outline(win_x + 64, win_y + 32, 22, 20, Color::ACCENT_YELLOW); }

        let hist_r = (win_x + win_w - 124, win_y + 32, 58, 20);
        fb.draw_rect(hist_r.0, hist_r.1, hist_r.2, hist_r.3, Color::ACCENT_PINK);
        MinGuiCompositor::draw_string(fb, hist_r.0 + 6, hist_r.1 + 4,
            crate::gui::ui_text("Geçmiş", "HIST."), MinGuiCompositor::text_on(Color::ACCENT_PINK));
        if self.focused_control == 3 { fb.draw_rect_outline(hist_r.0, hist_r.1, hist_r.2, hist_r.3, Color::ACCENT_YELLOW); }

        let url_x = win_x + 94;
        let url_w = win_w.saturating_sub(224);
        fb.draw_rect(url_x, win_y + 32, url_w, 20, if self.address_focused { Color::WINDOW_BG } else { Color::DARK_BG });
        let char_width = MinGuiCompositor::text_width("M", 1).max(1);
        let visible_chars = url_w.saturating_sub(12).checked_div(char_width).unwrap_or(1).max(1);
        let mut addr_scroll = self.address_scroll.min(self.url_len);
        if self.address_focused && self.address_cursor < addr_scroll { addr_scroll = self.address_cursor; }
        if self.address_focused && self.address_cursor >= addr_scroll + visible_chars {
            addr_scroll = self.address_cursor + 1 - visible_chars;
        }
        let visible_end = (addr_scroll + visible_chars).min(self.url_len);
        let clip = (url_x + 4, win_y + 32, url_w.saturating_sub(8), 20);
        let text_x = url_x + 6;
        let selection = self.address_selection.map(|(a,b)| (a.min(b).max(addr_scroll), a.max(b).min(visible_end)))
            .filter(|(a,b)| a < b);
        if let Some((sel_start, sel_end)) = selection {
            let prefix = core::str::from_utf8(&self.url[addr_scroll..sel_start]).unwrap_or("");
            let selected = core::str::from_utf8(&self.url[sel_start..sel_end]).unwrap_or("");
            let suffix = core::str::from_utf8(&self.url[sel_end..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(fb, text_x, win_y + 36, prefix, Color::TEXT_MAIN, clip);
            let selection_x = text_x + MinGuiCompositor::text_width(prefix, 1);
            let selection_w = MinGuiCompositor::text_width(selected, 1);
            fb.draw_rect(selection_x, win_y + 32, selection_w, 20, Color::ACCENT_BLUE);
            MinGuiCompositor::draw_string_clipped(fb, selection_x, win_y + 36, selected,
                MinGuiCompositor::text_on(Color::ACCENT_BLUE), clip);
            MinGuiCompositor::draw_string_clipped(fb, selection_x + selection_w, win_y + 36, suffix, Color::TEXT_MAIN, clip);
        } else {
            let shown = core::str::from_utf8(&self.url[addr_scroll..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(fb, text_x, win_y + 36, shown, Color::TEXT_MAIN, clip);
        }
        if self.address_focused && selection.is_none() {
            let caret_end = self.address_cursor.min(self.url_len);
            let caret_start = addr_scroll.min(caret_end);
            let caret_prefix = core::str::from_utf8(&self.url[caret_start..caret_end]).unwrap_or("");
            let caret_x = text_x + MinGuiCompositor::text_width(caret_prefix, 1);
            if caret_x < url_x + url_w.saturating_sub(2) {
                fb.draw_rect(caret_x, win_y + 34, 1, 16, Color::ACCENT_YELLOW);
            }
        }
        if self.focused_control == 4 { fb.draw_rect_outline(url_x, win_y + 32, url_w, 20, Color::ACCENT_YELLOW); }

        let git_r = (win_x + win_w - 60, win_y + 32, 50, 20);
        fb.draw_rect(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 48, win_y + 36, "GIT", MinGuiCompositor::text_on(Color::ACCENT_BLUE));
        if self.focused_control == 5 { fb.draw_rect_outline(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_YELLOW); }

        // Sık Kullanılanlar / Yer İmleri Çubuğu (Bookmarks Bar)
        let bbar_y = win_y + 59;
        let bbar_h = 22;
        fb.draw_rect(win_x + 6, bbar_y, win_w - 12, bbar_h, Color::TITLE_BAR);
        fb.draw_rect(win_x + 6, bbar_y + bbar_h - 1, win_w - 12, 1, Color::BORDER);

        let mut cur_bx = win_x + 12;
        let star_w = MinGuiCompositor::text_width("*", 1);
        MinGuiCompositor::draw_string(fb, cur_bx, bbar_y + 4, "*", Color::ACCENT_YELLOW);
        cur_bx += star_w + 6;

        for bm in BOOKMARKS {
            let label = crate::gui::ui_text(bm.title, bm.title_en);
            let tw = MinGuiCompositor::text_width(label, 1);
            let chip_w = tw + 12;
            if cur_bx + chip_w > win_x + win_w - 10 {
                break;
            }
            let is_active = self.url_matches(bm.url);
            let bg_col = if is_active { Color::ACCENT_BLUE } else { Color::WINDOW_BG };
            let fg_col = if is_active {
                MinGuiCompositor::text_on(Color::ACCENT_BLUE)
            } else if bm.url.starts_with("http") {
                Color::ACCENT_GREEN
            } else {
                Color::TEXT_MAIN
            };
            fb.draw_rect(cur_bx, bbar_y + 2, chip_w, 18, bg_col);
            fb.draw_rect_outline(cur_bx, bbar_y + 2, chip_w, 18, Color::BORDER);
            MinGuiCompositor::draw_string(fb, cur_bx + 6, bbar_y + 4, label, fg_col);
            cur_bx += chip_w + 5;
        }

        let status_h = 20;
        let status_y = win_y + win_h.saturating_sub(status_h + 4);
        let page_x = win_x + 6;
        let page_y = win_y + 83;
        let page_w = win_w.saturating_sub(12);
        let page_h = status_y.saturating_sub(page_y + 4).max(120);
        fb.draw_rect(page_x, page_y, page_w, page_h, Color::DARK_BG);
        if self.page_loaded {
            if self.active_tab == 3 {
                // Canlı Web Sayfası Render Paneli (HTML DOM)
                fb.draw_rect(page_x + 8, page_y + 8, page_w.saturating_sub(16), 26, Color::TITLE_BAR);
                let title_str = core::str::from_utf8(&self.page_title[..self.page_title_len]).unwrap_or("Web Document");
                MinGuiCompositor::draw_string(fb, page_x + 16, page_y + 14, title_str, Color::ACCENT_GREEN);

                // HTML Kağıt Alanı (CSS body bg_color desteğiyle)
                let doc_x = page_x + 8;
                let doc_y = page_y + 38;
                let doc_w = page_w.saturating_sub(16);
                let doc_h = page_h.saturating_sub(46);
                let paper_bg = self.page_bg_color.unwrap_or(Color::WINDOW_BG);
                fb.draw_rect(doc_x, doc_y, doc_w, doc_h, paper_bg);

                let doc_clip = (doc_x, doc_y, doc_w, doc_h);
                let scroll_y = scroll.y;

                // JS alert kutusu bildirimi
                if let Some(ref alert_msg) = self.js_alert {
                    fb.draw_rect(doc_x + 10, doc_y + 6, doc_w.saturating_sub(20), 24, Color::ACCENT_YELLOW);
                    MinGuiCompositor::draw_string_clipped(fb, doc_x + 16, doc_y + 10, alert_msg, Color::BLACK, doc_clip);
                }

                // Clear rendered links for hit testing
                let mut links = self.rendered_links.borrow_mut();
                links.clear();

                let mut cur_line_y = doc_y + 12 + if self.js_alert.is_some() { 28 } else { 0 };
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
                        let total_line_w: usize = line.spans.iter().map(|s| {
                            MinGuiCompositor::text_width(&s.text, scale) + if s.is_button { 16 } else { 0 }
                        }).sum();

                        let mut cur_x = match line.align {
                            TextAlign::Center => {
                                let avail = doc_w.saturating_sub(24);
                                doc_x + 12 + (avail.saturating_sub(total_line_w) / 2)
                            }
                            TextAlign::Right => {
                                doc_x + doc_w.saturating_sub(12 + total_line_w)
                            }
                            TextAlign::Left => doc_x + 12 + line.indent,
                        };

                        // Draw bullet for list items
                        if line.tag == HtmlTagType::ListItem {
                            let bullet_x = doc_x + 14;
                            let bullet_y = draw_y + (line_h / 2).saturating_sub(2);
                            if bullet_y >= doc_y && bullet_y + 5 <= doc_y + doc_h {
                                fb.draw_rect(bullet_x, bullet_y, 4, 4, Color::TEXT_MUTED);
                            }
                        }

                        for span in &line.spans {
                            let raw_text_w = MinGuiCompositor::text_width(&span.text, scale);
                            let span_w = raw_text_w + if span.is_button { 16 } else { 0 };
                            let text_color = if span.is_link {
                                Color::ACCENT_BLUE
                            } else {
                                span.color
                            };

                            if span.is_button {
                                let btn_bg = span.bg_color.unwrap_or(Color::TITLE_BAR);
                                fb.draw_rect(cur_x, draw_y, span_w, line_h, btn_bg);
                                fb.draw_rect_outline(cur_x, draw_y, span_w, line_h, Color::BORDER);
                                MinGuiCompositor::draw_string_scaled_clipped(fb, cur_x + 8, draw_y + 2, &span.text, span.color, scale, doc_clip);
                                {
                                    links.push(RenderedLink {
                                        rect: (cur_x, draw_y, span_w, line_h),
                                        url: span.link_url.clone(),
                                        onclick: span.onclick.clone(),
                                    });
                                }
                            } else {
                                if let Some(bg) = span.bg_color {
                                    fb.draw_rect(cur_x, draw_y, span_w, line_h, bg);
                                }
                                MinGuiCompositor::draw_string_scaled_clipped(fb, cur_x, draw_y, &span.text, text_color, scale, doc_clip);
                                // Sentez kalın: 1px sağa ikinci geçiş
                                if span.bold && !span.text.trim().is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(fb, cur_x + 1, draw_y, &span.text, text_color, scale, doc_clip);
                                }
                                // Altı çizili
                                if span.underline && !span.text.trim().is_empty() {
                                    let ul_y = draw_y + line_h.saturating_sub(2);
                                    if ul_y >= doc_y && ul_y < doc_y + doc_h {
                                        let ul_w = span_w.min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x)));
                                        fb.draw_rect(cur_x, ul_y, ul_w, 1, text_color);
                                    }
                                }

                                if span.is_link || !span.onclick.is_empty() {
                                    let underline_y = draw_y + line_h.saturating_sub(3);
                                    if underline_y >= doc_y && underline_y < doc_y + doc_h {
                                        let ul_w = span_w.min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x)));
                                        fb.draw_rect(cur_x, underline_y, ul_w, 1, Color::ACCENT_BLUE);
                                    }
                                    {
                                        links.push(RenderedLink {
                                            rect: (cur_x, draw_y, span_w, line_h),
                                            url: span.link_url.clone(),
                                            onclick: span.onclick.clone(),
                                        });
                                    }
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
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("minOs | Yerel Yardım ve Sık Kullanılanlar", "minOs | Local Help and Bookmarks"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://bookmarks, minos://cats, minos://help, minos://net", "Local addresses: minos://bookmarks, minos://cats, minos://help, minos://net"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 110, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("★ Sık Kullanılan: http://httpforever.com/ (saf HTTP test sitesi)", "★ Favorite: http://httpforever.com/ (pure HTTP test site)"), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, crate::gui::ui_text("minos://net ile ağ durumunu, minos://history ile geçmişi açın.", "Check the network at minos://net, open history at minos://history."), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 104, crate::gui::ui_text("Yer imleri çubuğundan veya minos://bookmarks ile tüm favorileri açın.", "Open bookmarks via the toolbar or visit minos://bookmarks."), Color::TEXT_MUTED);
            } else {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("KediWiki | Yerel Sayfa", "KediWiki | Local Page"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats ve minos://help", "Local addresses: minos://cats and minos://help"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("Kediler ve minOs!", "Cats and minOs!"), Color::TEXT_MAIN);
            }
        } else {
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 28, crate::gui::ui_text("Bu adres açılamadı.", "This address could not be opened."), Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, crate::gui::ui_text("Desteklenen adresler: minos://help, minos://cats, minos://net, minos://history", "Supported: minos://help, minos://cats, minos://net, minos://history"), Color::TEXT_MAIN);
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
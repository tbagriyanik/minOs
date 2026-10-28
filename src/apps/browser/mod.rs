//! minOs tarayıcı uygulaması.
//!
//! Modül yapısı:
//! - `types`: paylaşılan veri tipleri (`TextSpan`, `BrowserLine` ...)
//! - `css`: CSS ayrıştırma ve stil eşleştirme
//! - `html`: HTML tokenize + yerleşim (`render_html_to_lines`)
//! - `crate::apps::js`: minJs betik motoru
//! - bu dosya: HTTP/TLS akışı, geçmiş ve `BrowserApp` pencere denetçisi
mod css;
mod html;
mod types;

pub use types::{
    html_color, Bookmark, BrowserLine, HtmlTagType, HttpCookie, RenderedLink, TextAlign, TextSpan,
    BOOKMARKS,
};

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

use html::{decode_bytes_to_string, eq_ascii, find_bytes, render_html_to_lines};

extern crate alloc;

const BROWSER_ERROR_LOG_LIMIT: usize = 64;

// ── HTML Demo içeriği ──────────────────────────────────────────────
/// Tem demo HTML sayfası (basit tanıttık)
const DEMO_HTML_BASIC: &[u8] = b"<html><head><style>body{font-family:sans-serif;margin:40px;background:#f0f0f0;color:#333}h1{color:#d03030;text-align:center}.section{margin:20px 0}.btn{padding:8px 16px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer}.btn:hover{background:#184aae}.card{border:1px solid #ddd;border-radius:8px;padding:16px;margin:16px 0;background:white}.tag{display:inline-block;padding:4px 8px;margin:2px;border:1px solid #ccc;border-radius:12px;background:#e0e0e0;color:#333;font-size:0.8em}.highlight{background:#fff3a0;padding:4px 8px;border-radius:4;margin:2px}</style></head><body><h1>minOs HTML CSS JS Demo</h1><div class='section'><p>This page shows HTML CSS JavaScript capabilities.</p><p>Image demo: <a href='minos://demo-img'>image demo</a> | Video demo: <a href='minos://demo-video'>video demo</a></p><span class='tag'>HTML</span><span class='tag'>CSS</span><span class='tag'>JS</span></div></body></html>";

/// Resim gösteren demo HTML sayfası
const DEMO_HTML_IMG: &[u8] = b"<html><head><style>body{font-family:sans-serif;margin:40px;background:#e0e0e0;color:#333}h1{color:#d03030;text-align:center}.section{margin:20px 0}.card{border:2px solid #2050c0;padding:20px;margin:20px 0;background:white;box-shadow:0 4px 12px rgba(0,0,0,0.15)}.img-container{border:2px solid #2050c0;padding:16px;margin:16px 0;background:#fafafa;text-align:center;border-radius:8}.img-container img{max-width:100%;height:auto;border:1px solid #ccc;border-radius:4}.img-info{margin-top:12px;color:#666;font-size:0.9em}.btn{padding:8px 16px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer;margin:4px}.btn:hover{background:#184aae}.tag{display:inline-block;padding:4px 8px;margin:2px;border:1px solid #ccc;border-radius:12px;background:#e0e0e0;color:#333;font-size:0.8em}.counter{color:#2050c0;font-family:monospace;font-weight:bold}.progress-bar{width:100%;height:8px;background:#e0e0e0;margin:16px 0;border-radius:4;overflow:hidden}.progress-fill{height:100%;width:0%;background:#2050c0;transition:width 0.1s linear}.tag-demo{animation:pulse 2s infinite}.@keyframes pulse{0%{opacity:1}.50%{opacity:0.5}.100%{opacity:1}}</style></head><body><h1>minOs - Image Demo</h1><div class='section'><p>Shows image loading and display capability with CSS enhancements.</p><div class='card'><div class='img-container'><img src='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/5+hHgAHggJ/chD2wbwAAAABJRU5ErkJggg==' alt='Sample PNG Image' /><div class='img-info'>PNG format, sample image data</div></div><p><span class='counter' id='frame_counter'>0</span> / 30 frame</p><p class='tag tag-demo'>Live Demo</p><div class='progress-bar'><div class='progress-fill' id='progress_bar' style='width:0%'></div></div><div class='control'><button class='btn' onclick='incrementCounter()'>Frame up</button> <button class='btn' onclick='decrementCounter()'>Frame down</button></div><script>let counter = 0; function incrementCounter() { counter++; document.getElementById('frame_counter').innerText = counter; document.getElementById('progress_bar').style.width = (counter * 100/30) + '%'; } function decrementCounter() { if (counter > 0) { counter--; document.getElementById('frame_counter').innerText = counter; document.getElementById('progress_bar').style.width = (counter * 100/30) + '%'; } }</script></div></body></html>";

/// Video oynatan demo HTML sayfası
const DEMO_HTML_VIDEO: &[u8] = b"<html><head><style>body{font-family:sans-serif;margin:20px;background:#101018;color:#e0e0e0}h1{color:#d03030;text-align:center}.section{margin:20px 0}.video-container{border:2px solid #2050c0;padding:12px;margin:15px 0;background:#181820;text-align:center}video{max-width:100%;background:#000}.video-info{margin-top:8px;color:#aaa;font-size:0.9em}.btn{padding:6px 12px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer;margin:2px}.btn:hover{background:#184aae}.control{margin:8px 0}.counter{color:#2050c0;font-family:monospace;font-weight:bold}.p{line-height:1.6}</style></head><body><h1>minOs - Video Demo</h1><div class='section'><p>Shows video playback capability.</p><div class='video-container'><video id='demoVideo' controls autoplay><source src='minos://sample.mp4' type='video/mp4'>Your browser does not support the video tag or the video file was not found.</video><div class='video-info'>Sample video player</div></div><p><span class='counter' id='play_counter'>0</span> / 90 seconds</p><div class='control'><button class='btn' onclick='playPauseVideo()'>Play/Pause</button> <button class='btn' onclick='skipForward()'>+10s</button> <button class='btn' onclick='skipBackward()'>-10s</button></div><script>let playCounter = 0; let isPlaying = true; function playPauseVideo() { isPlaying = !isPlaying; const vid = document.getElementById('demoVideo'); if (isPlaying) { vid.play(); } else { vid.pause(); } document.getElementById('play_counter').innerText = Math.floor(vid.currentTime); } function skipForward() { const vid = document.getElementById('demoVideo'); vid.currentTime = Math.min(vid.currentTime + 10, 90); document.getElementById('play_counter').innerText = Math.floor(vid.currentTime); } function skipBackward() { const vid = document.getElementById('demoVideo'); vid.currentTime = Math.max(vid.currentTime - 10, 0); document.getElementById('play_counter').innerText = Math.floor(vid.currentTime); } // Simulated progress update setInterval(() => { const vid = document.getElementById('demoVideo'); if (isPlaying && vid.currentTime < 90) { vid.currentTime += 0.5; document.getElementById('play_counter').innerText = Math.floor(vid.currentTime); } }, 500);</script></div></body></html>";

#[allow(dead_code)]
// Örnek resim verisi (base64 encode PNG 64x64 kare)
const SAMPLE_IMAGE_DATA: [u8; 29] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
    0x00, 0x00, 0x00, 0x0d, // IHDR chunk length
    0x49, 0x48, 0x44, 0x52, // IHDR chunk type
    0x00, 0x00, 0x00, 0x40, // Width: 64
    0x00, 0x00, 0x00, 0x40, // Height: 64
    0x08, 0x02, 0x00, 0x00, 0x00, // Bit depth, color type, compression, filter, interlace
];

fn split_http_lines(body: &[u8]) -> impl Iterator<Item = &str> {
    body.split(|b| *b == b'\n').filter_map(|line| {
        let line = if line.last() == Some(&b'\r') {
            &line[..line.len() - 1]
        } else {
            line
        };
        if line.is_empty() {
            None
        } else {
            Some(core::str::from_utf8(line).unwrap_or("[binary or non-UTF8 content]"))
        }
    })
}

// URL'den path kısmını çıkar (minos://demo/img -> "/img")
fn url_path(url: &[u8]) -> Option<&str> {
    let url_str = core::str::from_utf8(url).ok()?;
    // minos:// prefixini kaldır
    let rest = if url_str.starts_with("minos://") {
        &url_str[8..]
    } else if url_str.starts_with("http://") || url_str.starts_with("https://") {
        &url_str[7..]
    } else {
        return None;
    };
    // Sorguyu ve frammenti kaldır
    let path = rest.split(|c: char| c == '?' || c == '#').next()?;
    // Trailing slash'i temizle
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        Some("/")
    } else {
        Some(path)
    }
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

// ── Ertelenmiş hata günlüğü ───────────────────────────────────────────────
// Ağ döngüsü, yanıt tamponlarına ödünç verdiği `self` alanlarını tutarken
// `record_network_error(&mut self)` çağıramaz (E0502). Hata mesajı bu halka
// tampona yazılır, ardından `flush_network_errors()` ile günlüğe aktarılır.
const PENDING_ERROR_CAP: usize = 8;
const PENDING_ERROR_LEN: usize = 96;
static PENDING_ERROR_COUNT: AtomicUsize = AtomicUsize::new(0);
static PENDING_ERRORS: [[AtomicU32; PENDING_ERROR_LEN]; PENDING_ERROR_CAP] =
    [const { [const { AtomicU32::new(0) }; PENDING_ERROR_LEN] }; PENDING_ERROR_CAP];

fn defer_network_error(message: &str) {
    let slot = PENDING_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) % PENDING_ERROR_CAP;
    for cell in PENDING_ERRORS[slot].iter() {
        cell.store(0, Ordering::Relaxed);
    }
    for (cell, byte) in PENDING_ERRORS[slot].iter().zip(message.bytes()) {
        cell.store(u32::from(byte), Ordering::Relaxed);
    }
    // Son hücre ayırıcı olarak sıfırda bırakılır (güvenli kısa metin).
}

/// Belirtilen geçmiş girdisini (0 = en eski) metne çevirir.
fn history_copy(index: usize) -> Option<String> {
    let count = HISTORY_COUNT.load(Ordering::Acquire);
    if index >= count || index >= HISTORY_MAX {
        return None;
    }
    let len = HISTORY_LEN[index]
        .load(Ordering::Acquire)
        .min(HISTORY_URL_CAP);
    if len == 0 {
        return None;
    }
    let mut out = String::with_capacity(len);
    for slot in HISTORY[index].iter().take(len) {
        out.push(char::from_u32(slot.load(Ordering::Relaxed)).unwrap_or('\u{FFFD}'));
    }
    Some(out)
}

/// Kayıtlı geçmiş girdisi sayısı.
fn history_count() -> usize {
    HISTORY_COUNT.load(Ordering::Acquire)
}

/// Yeni bir adresi geçmişe ekler (art arda yinelemeleri atlar).
fn history_push(url: &str) {
    if url.is_empty() {
        return;
    }
    let mut count = HISTORY_COUNT.load(Ordering::Acquire);
    // Atomic state can briefly be observed while a navigation update is in
    // progress. Always normalize it before using it as an array index.
    count = count.min(HISTORY_MAX);
    let pos = HISTORY_POS
        .load(Ordering::Acquire)
        .min(count.saturating_sub(1));
    if count > 0 && pos < count {
        if let Some(cur) = history_copy(pos) {
            if cur == url {
                return;
            }
        }
    }
    // Geri gidildikten sonra yeni gezinme, ileri geçmişi budar.
    if count > 0 {
        count = pos.saturating_add(1).min(count);
    }
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
    if count == 0 || pos == 0 || pos >= count {
        return None;
    }
    let target = pos - 1;
    HISTORY_POS.store(target, Ordering::Release);
    history_copy(target)
}

/// İleri geçmişte bir sonraki adrese gider; yoksa `None`.
fn history_forward() -> Option<String> {
    let count = HISTORY_COUNT.load(Ordering::Acquire);
    let pos = HISTORY_POS.load(Ordering::Acquire);
    if count == 0 || pos >= count || pos.saturating_add(1) >= count {
        return None;
    }
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
    if index >= count || count == 0 {
        return false;
    }
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
    address_selection_anchor: Option<usize>,
    address_last_click: (u64, usize, usize),
    address_click_streak: u8,
    status_last_click: (u64, usize, usize),
    status_click_streak: u8,
    address_undo: [[u8; 128]; 16],
    address_undo_len: [usize; 16],
    address_undo_count: usize,
    address_redo: [[u8; 128]; 16],
    address_redo_len: [usize; 16],
    address_redo_count: usize,
    pub active_tab: usize,
    pub focused_control: usize,
    pub toolbar_menu_open: bool,
    pub find_active: bool,
    pub find_query: [u8; 64],
    pub find_len: usize,
    pub find_match_index: Cell<usize>,
    pub find_match_count: Cell<usize>,
    suppress_history: bool,
    pub page_loaded: bool,
    pub status: &'static str,
    pub error_logs: Vec<String>,
    pub net_online: bool,
    pub ping_ok: bool,
    pub web_request_sent: bool,
    pub page_title: [u8; 64],
    pub page_title_len: usize,
    pub page_lines: Vec<BrowserLine>,
    pub rendered_links: RefCell<Vec<RenderedLink>>,
    pub page_selection_anchor: Option<(usize, usize)>,
    pub page_selection_start: Option<(usize, usize)>,
    pub page_selection_end: Option<(usize, usize)>,
    pub page_dragging: bool,
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
    /// TLS 1.3 istemci Finished kaydı gönderildi mi?
    pub tls13_finished_sent: bool,
    pub page_bg_color: Option<u32>,
    pub js_alert: Option<String>,
    pub js_vars: Vec<(String, String)>,
    pub js_functions: Vec<(String, String)>,
    pub bookmarks_extra: Vec<crate::apps::browser::types::Bookmark>,
    pub download_path: Option<alloc::string::String>,
    pub download_data: Option<Vec<u8>>,
}

impl BrowserApp {
    pub fn open_find(&mut self) {
        self.find_active = true;
        self.find_len = 0;
        self.find_match_index.set(0);
        self.find_match_count.set(0);
    }
    pub fn close_find(&mut self) {
        self.find_active = false;
    }
    pub fn find_char(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        for byte in ch.encode_utf8(&mut buf).bytes() {
            if self.find_len < self.find_query.len() {
                self.find_query[self.find_len] = byte;
                self.find_len += 1;
            }
        }
        self.find_match_index.set(0);
    }
    pub fn find_backspace(&mut self) {
        if self.find_len == 0 {
            return;
        }
        self.find_len -= 1;
        while self.find_len > 0 && self.find_query[self.find_len] & 0xC0 == 0x80 {
            self.find_len -= 1;
        }
        self.find_match_index.set(0);
    }
    pub fn find_next(&mut self) {
        let count = self.find_match_count.get();
        if count > 0 {
            self.find_match_index
                .set((self.find_match_index.get() + 1) % count);
        }
    }

    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128],
            url_len: 0,
            address_focused: false,
            address_cursor: 0,
            address_scroll: 0,
            address_selection: None,
            address_selection_anchor: None,
            address_last_click: (0, 0, 0),
            address_click_streak: 0,
            status_last_click: (0, 0, 0),
            status_click_streak: 0,
            address_undo: [[0; 128]; 16],
            address_undo_len: [0; 16],
            address_undo_count: 0,
            address_redo: [[0; 128]; 16],
            address_redo_len: [0; 16],
            address_redo_count: 0,
            active_tab: 0,
            focused_control: 0,
            toolbar_menu_open: false,
            find_active: false,
            find_query: [0; 64],
            find_len: 0,
            find_match_index: Cell::new(0),
            find_match_count: Cell::new(0),
            suppress_history: false,
            page_loaded: true,
            status: crate::gui::ui_text("minOs yerel yardım sayfası", "minOs local help page"),
            error_logs: Vec::new(),
            net_online: false,
            ping_ok: false,
            web_request_sent: false,
            page_title: [0; 64],
            page_title_len: 0,
            page_lines: Vec::new(),
            rendered_links: RefCell::new(Vec::new()),
            page_selection_anchor: None,
            page_selection_start: None,
            page_selection_end: None,
            page_dragging: false,
            scroll_y: 0,
            request_pending: false,
            request_host: [0; 128],
            request_host_len: 0,
            request_path: [0; 256],
            request_path_len: 0,
            request_ip: crate::drivers::netstack::Ipv4Address([0; 4]),
            request_port: 80,
            request_deadline: 0,
            request_state: 0,
            redirect_count: 0,
            is_https: false,
            is_post: false,
            post_body: Vec::new(),
            cookies: Vec::new(),
            response_data: Vec::new(),
            response_len: 0,
            tls_session: None,
            tls_app_data: Vec::new(),
            tls_pending_request: Vec::new(),
            tls_request_sent: false,
            tls13_finished_sent: false,
            page_bg_color: None,
            js_alert: None,
            js_vars: Vec::new(),
            js_functions: Vec::new(),
            bookmarks_extra: Vec::new(),
            download_path: None,
            download_data: None,
        };
        app.set_url(b"minos://help");
        app
    }

    pub fn add_bookmark_current(&mut self) {
        let current = alloc::string::String::from(
            core::str::from_utf8(&self.url[..self.url_len]).unwrap_or(""),
        );
        if current.is_empty() {
            return;
        }
        let title = alloc::string::String::from(
            core::str::from_utf8(&self.page_title[..self.page_title_len]).unwrap_or(&current),
        );
        self.bookmarks_extra
            .push(crate::apps::browser::types::Bookmark {
                title: Box::leak(title.clone().into_boxed_str()),
                title_en: Box::leak(title.into_boxed_str()),
                url: Box::leak(current.clone().into_boxed_str()),
                description: "Kullanıcı yer imi",
                description_en: "User bookmark",
            });
        self.status = crate::gui::ui_text("Yer imi eklendi.", "Bookmark added.");
    }

    pub fn remove_bookmark_current(&mut self) {
        let current = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        self.bookmarks_extra.retain(|b| b.url != current);
        self.status = crate::gui::ui_text("Yer imi kaldırıldı.", "Bookmark removed.");
    }

    pub fn save_current_download(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem) -> bool {
        let Some(data) = self.download_data.clone() else {
            self.status = crate::gui::ui_text("İndirilecek içerik yok.", "Nothing to download.");
            return false;
        };
        let path = alloc::format!("/root/Downloads/indirme_{}.html", crate::monotonic_ms());
        if vfs.write_file(&path, &data) {
            self.download_path = Some(path);
            self.status = crate::gui::ui_text(
                "İndirme /root/Downloads klasörüne kaydedildi.",
                "Download saved to /root/Downloads.",
            );
            true
        } else {
            self.status =
                crate::gui::ui_text("İndirme kaydedilemedi.", "Download could not be saved.");
            false
        }
    }

    pub fn bookmark_btn_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(220), win_y + 32, 22, 20)
    }

    pub fn download_btn_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(194), win_y + 32, 22, 20)
    }

    pub fn toolbar_menu_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(38), win_y + 32, 28, 20)
    }

    pub fn toolbar_menu_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(190), win_y + 56, 180, 128)
    }

    pub fn toolbar_menu_item_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        index: usize,
    ) -> (usize, usize, usize, usize) {
        let (x, y, w, _) = Self::toolbar_menu_rect(win_x, win_y, win_w);
        (x + 4, y + 4 + index * 24, w - 8, 22)
    }

    pub fn find_bar_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(258), win_y + 87, 244, 24)
    }

    pub fn status_bar_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        (
            win_x + 6,
            win_y + win_h.saturating_sub(24),
            win_w.saturating_sub(12),
            20,
        )
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

    pub fn point_to_page_pos(
        &self,
        mx: usize,
        my: usize,
        doc_x: usize,
        doc_y: usize,
        doc_w: usize,
        doc_h: usize,
        scroll_y: usize,
    ) -> Option<(usize, usize)> {
        if mx < doc_x || mx >= doc_x + doc_w || my < doc_y || my >= doc_y + doc_h {
            return None;
        }
        let my_doc = my.saturating_sub(doc_y).saturating_add(scroll_y);
        let mut cur_line_y = 12 + if self.js_alert.is_some() { 28 } else { 0 };

        for (line_idx, line) in self.page_lines.iter().enumerate() {
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

            if my_doc >= cur_line_y && my_doc < cur_line_y + line_h {
                let total_line_w: usize = line
                    .spans
                    .iter()
                    .map(|s| {
                        MinGuiCompositor::text_width(&s.text, scale)
                            + if s.is_button { 16 } else { 0 }
                    })
                    .sum();

                let mut cur_x = match line.align {
                    TextAlign::Center => {
                        let avail = doc_w.saturating_sub(24);
                        doc_x + 12 + (avail.saturating_sub(total_line_w) / 2)
                    }
                    TextAlign::Right => doc_x + doc_w.saturating_sub(12 + total_line_w),
                    TextAlign::Left => doc_x + 12 + line.indent,
                };

                let mut char_offset = 0usize;
                for span in &line.spans {
                    let span_w = MinGuiCompositor::text_width(&span.text, scale)
                        + if span.is_button { 16 } else { 0 };
                    if mx < cur_x {
                        return Some((line_idx, char_offset));
                    } else if mx >= cur_x && mx < cur_x + span_w {
                        let rel_x = mx - cur_x - if span.is_button { 8 } else { 0 };
                        let mut acc = 0;
                        let mut best = 0;
                        for (idx, ch) in span.text.char_indices() {
                            let mut tmp = [0u8; 4];
                            let cw = MinGuiCompositor::text_width(ch.encode_utf8(&mut tmp), scale);
                            if rel_x < acc + cw / 2 {
                                break;
                            }
                            acc += cw;
                            best = idx + ch.len_utf8();
                        }
                        return Some((line_idx, char_offset + best));
                    }
                    char_offset += span.text.len();
                    cur_x += span_w;
                }
                return Some((line_idx, char_offset));
            }

            cur_line_y += line_h;
        }

        if !self.page_lines.is_empty() {
            let last_idx = self.page_lines.len() - 1;
            let last_len = self.page_lines[last_idx]
                .spans
                .iter()
                .map(|s| s.text.len())
                .sum();
            Some((last_idx, last_len))
        } else {
            None
        }
    }

    pub fn start_page_selection_at(
        &mut self,
        mx: usize,
        my: usize,
        doc_x: usize,
        doc_y: usize,
        doc_w: usize,
        doc_h: usize,
        scroll_y: usize,
    ) {
        if let Some(pos) = self.point_to_page_pos(mx, my, doc_x, doc_y, doc_w, doc_h, scroll_y) {
            self.page_selection_anchor = Some(pos);
            self.page_selection_start = Some(pos);
            self.page_selection_end = Some(pos);
            self.page_dragging = true;
        } else {
            self.clear_page_selection();
        }
    }

    pub fn update_page_selection_at(
        &mut self,
        mx: usize,
        my: usize,
        doc_x: usize,
        doc_y: usize,
        doc_w: usize,
        doc_h: usize,
        scroll_y: usize,
    ) {
        if let Some(anchor) = self.page_selection_anchor {
            if let Some(pos) = self.point_to_page_pos(mx, my, doc_x, doc_y, doc_w, doc_h, scroll_y)
            {
                if pos < anchor {
                    self.page_selection_start = Some(pos);
                    self.page_selection_end = Some(anchor);
                } else {
                    self.page_selection_start = Some(anchor);
                    self.page_selection_end = Some(pos);
                }
            }
        }
    }

    pub fn clear_page_selection(&mut self) {
        self.page_selection_anchor = None;
        self.page_selection_start = None;
        self.page_selection_end = None;
        self.page_dragging = false;
    }

    pub fn has_page_selection(&self) -> bool {
        match (self.page_selection_start, self.page_selection_end) {
            (Some(s), Some(e)) => s != e,
            _ => false,
        }
    }

    pub fn selected_page_text(&self) -> String {
        let mut result = String::new();
        if let (Some((start_line, start_col)), Some((end_line, end_col))) =
            (self.page_selection_start, self.page_selection_end)
        {
            for line_idx in start_line..=end_line.min(self.page_lines.len().saturating_sub(1)) {
                let line = &self.page_lines[line_idx];
                let mut line_text = String::new();
                for span in &line.spans {
                    line_text.push_str(&span.text);
                }

                let len = line_text.len();
                let s_col = if line_idx == start_line {
                    start_col.min(len)
                } else {
                    0
                };
                let e_col = if line_idx == end_line {
                    end_col.min(len)
                } else {
                    len
                };

                if s_col < e_col && s_col < len {
                    let slice = &line_text[s_col..e_col.min(len)];
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(slice);
                }
            }
        }
        result
    }

    pub fn copy_page_selection(&self) -> bool {
        let text = self.selected_page_text();
        if !text.is_empty() {
            crate::gui::clipboard::set_str(&text);
            true
        } else {
            false
        }
    }

    pub fn click_link_at(&mut self, mx: usize, my: usize) -> Option<String> {
        if self.has_page_selection() {
            return None;
        }
        let mut clicked_url = None;
        let mut clicked_js = None;

        // Check rendered links first
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

        // If no JS from links, check page lines spans for onclick handlers
        if clicked_js.is_none() {
            // Iterate through page lines and spans to find the clicked element
            for (line_idx, line) in self.page_lines.iter().enumerate() {
                let mut span_idx = 0usize; // Reset for each line
                let mut char_offset = 0usize;
                for span in &line.spans {
                    let span_w = MinGuiCompositor::text_width(&span.text, 1)
                        + if span.is_button { 16 } else { 0 };
                    // Check if mouse is within this span's area
                    if line_idx < self.page_lines.len() {
                        // Simple check: if we have roughly the right number of spans before this one
                        let span_count_before = line.spans[..span_idx]
                            .iter()
                            .map(|s| s.text.len() + 1)
                            .sum::<usize>();
                        if span_count_before > 0 {
                            // Check bounds
                            if mx >= char_offset && mx < char_offset + span_w {
                                // Check if this span has an onclick handler
                                if !span.onclick.is_empty() {
                                    clicked_js = Some(span.onclick.clone());
                                }
                                break;
                            }
                        }
                    }
                    char_offset += span.text.len() + 1; // +1 for space
                    span_idx += 1;
                }
                if clicked_js.is_some() {
                    break;
                }
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
            || (expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2)
        {
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
            // No-op (logs are collected but not displayed in this simple env)
        } else if stmt.starts_with("document.title =") || stmt.starts_with("document.title=") {
            let val_part = if stmt.starts_with("document.title =") {
                &stmt[16..]
            } else {
                &stmt[15..]
            };
            let val = self.eval_js_expr(val_part);
            let vb = val.as_bytes();
            self.page_title_len = vb.len().min(64);
            self.page_title[..self.page_title_len].copy_from_slice(&vb[..self.page_title_len]);
        } else if stmt.starts_with("document.body.style.backgroundColor =")
            || stmt.starts_with("document.body.style.backgroundColor=")
            || stmt.starts_with("document.body.style.background =")
            || stmt.starts_with("document.body.style.background=")
        {
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
                if rest.starts_with(".innerText =")
                    || rest.starts_with(".innerText=")
                    || rest.starts_with(".innerHTML =")
                    || rest.starts_with(".innerHTML=")
                    || rest.starts_with(".textContent =")
                    || rest.starts_with(".textContent=")
                {
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
                } else if rest.starts_with(".className =") || rest.starts_with(".className=") {
                    let val_part = rest.split('=').nth(1).unwrap_or("");
                    let val = self.eval_js_expr(val_part);
                    for line in &mut self.page_lines {
                        for span in &mut line.spans {
                            if span.elem_id == id {
                                span.text = val.clone();
                            }
                        }
                    }
                } else if rest.starts_with(".setAttribute(") || rest.starts_with(".setAttribute(") {
                    // setAttribute support - parse attr="value"
                    if let Some(open_paren) = rest.find('(') {
                        if let Some(close_paren) = rest[open_paren + 1..].rfind(')') {
                            // Simple attribute setting - store in a lookup
                            let _attr_part = &rest[open_paren + 1..open_paren + 1 + close_paren];
                        }
                    }
                } else if rest.starts_with(".remove()") {
                    // Remove element - find and filter out spans with this elem_id
                    for line in &mut self.page_lines {
                        line.spans.retain(|s| s.elem_id != id);
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
                while close_tag < body.len() && body[close_tag] != b'>' {
                    close_tag += 1;
                }
                if close_tag < body.len() {
                    close_tag += 1;
                    let script_start = close_tag;
                    if let Some(pos) = find_bytes(&body[script_start..], b"</script>") {
                        let script_code =
                            decode_bytes_to_string(&body[script_start..script_start + pos]);
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

        render_html_to_lines(
            &preprocessed_html,
            wrap_width_px,
            &mut self.page_lines,
            max_lines,
        );
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
        self.address_selection_anchor = None;
        self.address_click_streak = 0;
    }

    /// Adres alanı metninin başladığı x konumu (alan başlangıcı + sol iç boşluk).
    const ADDRESS_TEXT_INSET: usize = 6;

    /// Adres çubuğunda aynı anda görülebilen karakter sayısı.
    pub fn address_visible_chars(field_w: usize) -> usize {
        let char_width = MinGuiCompositor::text_width("M", 1).max(1);
        field_w
            .saturating_sub(12)
            .checked_div(char_width)
            .unwrap_or(1)
            .max(1)
    }

    /// Fare x konumunu imleç indeksine çevirir. Karakter genişlikleri değişken
    /// olduğu için her karakterin gerçek piksel genişliği kullanılır; tıklanan
    /// noktaya en yakın karakter sınırı seçilir.
    fn address_index_at_x(&self, x: usize, field_x: usize, scroll: usize) -> usize {
        let text_x = field_x.saturating_add(Self::ADDRESS_TEXT_INSET);
        let rel = x.saturating_sub(text_x);
        let start = scroll.min(self.url_len);
        let mut best = start;
        let mut best_dist = rel;
        let mut acc = 0usize;
        for i in start..self.url_len {
            let ch_len = core::str::from_utf8(&self.url[i..i + 1])
                .map(|s| s.len())
                .unwrap_or(1);
            let ch =
                core::str::from_utf8(&self.url[i..(i + ch_len).min(self.url_len)]).unwrap_or("");
            acc += MinGuiCompositor::text_width(ch, 1);
            let dist = rel.abs_diff(acc);
            if dist <= best_dist {
                best_dist = dist;
                best = i + ch_len;
            }
        }
        best.min(self.url_len)
    }

    /// Adres çubuğunun görünür kaydırma ofseti (çizim ve isabet testi ortak kullanır).
    pub fn address_effective_scroll(&self, field_w: usize) -> usize {
        let visible_chars = Self::address_visible_chars(field_w);
        let mut addr_scroll = self.address_scroll.min(self.url_len);
        if self.address_focused && self.address_cursor < addr_scroll {
            addr_scroll = self.address_cursor;
        }
        if self.address_focused && self.address_cursor >= addr_scroll + visible_chars {
            addr_scroll = self.address_cursor + 1 - visible_chars;
        }
        addr_scroll
    }

    pub fn focus_address_at(&mut self, x: usize, field_x: usize) {
        self.focus_address_at_in(x, field_x, 0);
    }

    /// Adres alanına tıklama: imleç tıklanan karakter sınırına taşınır.
    /// `field_w`, alanın piksel genişliğidir (kaydırma hesabı için).
    pub fn focus_address_at_in(&mut self, x: usize, field_x: usize, field_w: usize) {
        self.address_focused = true;
        self.focused_control = 4;
        let scroll = self.address_effective_scroll(field_w);
        self.address_scroll = scroll;
        self.address_cursor = self.address_index_at_x(x, field_x, scroll);
        self.address_selection_anchor = Some(self.address_cursor);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn address_click_at(
        &mut self,
        x: usize,
        y: usize,
        field_x: usize,
        field_w: usize,
        now_ms: u64,
        interval_ms: u64,
    ) -> u8 {
        self.focus_address_at_in(x, field_x, field_w);
        let same_target = now_ms.abs_diff(self.address_last_click.0) <= interval_ms
            && self.address_last_click.1.abs_diff(x) < 4
            && self.address_last_click.2.abs_diff(y) < 4;
        self.address_click_streak = if same_target && self.address_click_streak < 3 {
            self.address_click_streak + 1
        } else {
            1
        };
        self.address_last_click = (now_ms, x, y);
        match self.address_click_streak {
            2 => self.address_select_word_at(self.address_cursor),
            3 => self.address_select_all(),
            _ => {}
        }
        self.address_click_streak
    }

    pub fn status_click_at(&mut self, x: usize, y: usize, now_ms: u64, interval_ms: u64) -> bool {
        let same_target = now_ms.abs_diff(self.status_last_click.0) <= interval_ms
            && self.status_last_click.1.abs_diff(x) < 8
            && self.status_last_click.2.abs_diff(y) < 8;
        self.status_click_streak = if same_target {
            self.status_click_streak.saturating_add(1).min(2)
        } else {
            1
        };
        self.status_last_click = (now_ms, x, y);
        if self.status_click_streak == 2 {
            self.status_click_streak = 0;
            true
        } else {
            false
        }
    }

    pub fn reset_status_click_streak(&mut self) {
        self.status_click_streak = 0;
    }

    pub fn address_drag_to(&mut self, x: usize, field_x: usize) {
        let scroll = self.address_scroll;
        let target = self.address_index_at_x(x, field_x, scroll);
        self.address_cursor = target;
        let anchor = self.address_selection_anchor.unwrap_or(target);
        self.address_selection = if anchor == target {
            None
        } else {
            Some((anchor.min(target), anchor.max(target)))
        };
        self.keep_address_cursor_visible();
    }

    pub fn address_select_word_at(&mut self, index: usize) {
        if self.url_len == 0 {
            return;
        }
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
        let mut at = index.min(self.url_len);
        if at == self.url_len || !is_word(self.url[at]) {
            if at > 0 && is_word(self.url[at - 1]) {
                at -= 1;
            }
        }
        if at >= self.url_len || !is_word(self.url[at]) {
            return;
        }
        let mut start = at;
        let mut end = at + 1;
        while start > 0 && is_word(self.url[start - 1]) {
            start -= 1;
        }
        while end < self.url_len && is_word(self.url[end]) {
            end += 1;
        }
        self.address_selection_anchor = Some(start);
        self.address_selection = Some((start, end));
        self.address_cursor = end;
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
                text: String::from(crate::gui::ui_text(
                    "[ Tümünü Temizle ]",
                    "[ Clear All History ]",
                )),
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

        for bm in BOOKMARKS.iter().chain(self.bookmarks_extra.iter()) {
            let title_text = crate::gui::ui_text(bm.title, bm.title_en);
            let desc_text = crate::gui::ui_text(bm.description, bm.description_en);
            let is_web = bm.url.starts_with("http");

            self.page_lines.push(BrowserLine {
                spans: alloc::vec![
                    TextSpan {
                        text: String::from(if is_web { "* [WEB] " } else { "* [LOCAL] " }),
                        color: if is_web {
                            Color::ACCENT_YELLOW
                        } else {
                            Color::ACCENT_BLUE
                        },
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

    pub fn click_bookmark_at(
        &self,
        mx: usize,
        my: usize,
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> Option<&'static str> {
        let bbar_y = win_y + 59;
        let bbar_h = 22;
        if my < bbar_y || my >= bbar_y + bbar_h {
            return None;
        }
        let star_w = MinGuiCompositor::text_width("*", 1);
        let mut cur_bx = win_x + 12 + star_w + 6;
        for bm in BOOKMARKS.iter().chain(self.bookmarks_extra.iter()) {
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
            if self.url_len < self.url.len() {
                self.address_push_undo();
            }
            self.address_delete_selection();
            if self.url_len < self.url.len() {
                self.url
                    .copy_within(self.address_cursor..self.url_len, self.address_cursor + 1);
                self.url[self.address_cursor] = ch as u8;
                self.url_len += 1;
                self.address_cursor += 1;
                self.keep_address_cursor_visible();
            }
        }
    }

    pub fn address_backspace(&mut self) {
        if self.address_selection.is_some() || self.address_cursor > 0 {
            self.address_push_undo();
        }
        if self.address_delete_selection() {
            return;
        }
        if self.address_cursor > 0 {
            self.address_cursor -= 1;
            self.url
                .copy_within(self.address_cursor + 1..self.url_len, self.address_cursor);
            self.url_len -= 1;
            self.url[self.url_len] = 0;
            self.keep_address_cursor_visible();
        }
    }

    pub fn address_delete(&mut self) {
        if self.address_selection.is_some() || self.address_cursor < self.url_len {
            self.address_push_undo();
        }
        if self.address_delete_selection() {
            return;
        }
        if self.address_cursor < self.url_len {
            self.url
                .copy_within(self.address_cursor + 1..self.url_len, self.address_cursor);
            self.url_len -= 1;
            self.url[self.url_len] = 0;
            self.keep_address_cursor_visible();
        }
    }

    fn address_move_to(&mut self, target: usize, select: bool) {
        if select {
            let anchor = match self.address_selection {
                Some((a, b)) => {
                    if self.address_cursor == b {
                        a
                    } else {
                        b
                    }
                }
                None => self.address_cursor,
            };
            self.address_cursor = target.min(self.url_len);
            if anchor == self.address_cursor {
                self.address_selection = None;
            } else {
                self.address_selection = Some((
                    anchor.min(self.address_cursor),
                    anchor.max(self.address_cursor),
                ));
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
                if prev == b' ' {
                    break;
                }
                if (prev.is_ascii_alphanumeric() || prev == b'_') != is_alnum {
                    break;
                }
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
                    if cur == b' ' {
                        break;
                    }
                    if (cur.is_ascii_alphanumeric() || cur == b'_') != is_alnum {
                        break;
                    }
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
        if self.address_cursor == 0 {
            return;
        }
        let mut idx = self.address_cursor;
        while idx > 0 && self.url[idx - 1] == b' ' {
            idx -= 1;
        }
        if idx > 0 {
            let is_alnum = self.url[idx - 1].is_ascii_alphanumeric() || self.url[idx - 1] == b'_';
            while idx > 0 {
                let prev = self.url[idx - 1];
                if prev == b' ' {
                    break;
                }
                if (prev.is_ascii_alphanumeric() || prev == b'_') != is_alnum {
                    break;
                }
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
        if self.address_cursor >= self.url_len {
            return;
        }
        let mut idx = self.address_cursor;
        if idx < self.url_len {
            let is_alnum = self.url[idx].is_ascii_alphanumeric() || self.url[idx] == b'_';
            let is_space = self.url[idx] == b' ';
            if !is_space {
                while idx < self.url_len {
                    let cur = self.url[idx];
                    if cur == b' ' {
                        break;
                    }
                    if (cur.is_ascii_alphanumeric() || cur == b'_') != is_alnum {
                        break;
                    }
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
        if self.address_undo_count == 0 {
            return;
        }
        self.address_save_redo();
        self.address_undo_count -= 1;
        self.url = self.address_undo[self.address_undo_count];
        self.url_len = self.address_undo_len[self.address_undo_count];
        self.address_cursor = self.address_cursor.min(self.url_len);
        self.address_selection = None;
        self.keep_address_cursor_visible();
    }

    pub fn address_redo_edit(&mut self) {
        if self.address_redo_count == 0 {
            return;
        }
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
            self.address_selection_anchor = Some(0);
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
        if self.address_selection.is_some() {
            self.address_push_undo();
        }
        self.address_delete_selection();
    }

    pub fn address_paste(&mut self) {
        if self.address_selection.is_some() {
            self.address_push_undo();
        }
        self.address_delete_selection();
        let mut pasted = ['\0'; crate::gui::clipboard::CAPACITY];
        let len = crate::gui::clipboard::copy_chars(&mut pasted);
        for ch in pasted[..len].iter().copied() {
            if ch.is_ascii_graphic() || ch == ' ' {
                self.address_char(ch);
            }
        }
    }

    pub fn navigate(&mut self) {
        let suppress = self.suppress_history;
        self.suppress_history = false;
        let mut input = [0u8; 128];
        input[..self.url_len].copy_from_slice(&self.url[..self.url_len]);
        let entered = core::str::from_utf8(&input[..self.url_len])
            .unwrap_or("")
            .trim();
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
            let authority_end = rest
                .find(|c| c == '/' || c == '?' || c == '#')
                .unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let suffix = &rest[authority_end..];
            let suffix = suffix.split('#').next().unwrap_or("");
            let default_port = if is_https { 443 } else { 80 };
            let (host, port) = if authority.starts_with('[') {
                ("", 0) // Bu ağ yığını şu an yalnızca IPv4 hedeflerini destekliyor.
            } else {
                match authority.rsplit_once(':') {
                    Some((h, p)) => match p.parse::<u16>() {
                        Ok(n) if n > 0 => (h, n),
                        _ => ("", 0),
                    },
                    _ => (authority, default_port),
                }
            };
            let path_len = if suffix.is_empty() {
                1
            } else if suffix.starts_with('?') {
                suffix.len() + 1
            } else {
                suffix.len()
            };
            if host.is_empty() || host.len() > 128 || path_len > 256 || port == 0 {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address.");
            } else if !crate::drivers::e1000::is_ready() {
                self.page_loaded = false;
                self.status =
                    crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable.");
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
                self.tls13_finished_sent = false;
                self.tls_app_data.clear();
                self.response_len = 0;
                self.response_data.clear();
                self.response_data.resize(65536, 0);
                self.status = if is_https {
                    crate::gui::ui_text(
                        "Güvenli bağlantı (HTTPS/TLS) başlatılıyor...",
                        "Starting secure (HTTPS/TLS) connection...",
                    )
                } else {
                    crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host...")
                };
                self.page_title = [0; 64];
                self.page_title_len = 0;
                self.page_lines.clear();
                if !suppress {
                    history_push(value);
                }
            }
            self.address_focused = false;
            return;
        }
        match value {
            "minos://help" => {
                self.active_tab = 0;
                self.page_loaded = true;
                if !suppress {
                    history_push(value);
                }
                self.status = crate::gui::ui_text(
                    "Yerel minOs yardım sayfası açıldı.",
                    "Local minOs help page opened.",
                );
            }
            "minos://cats" => {
                self.active_tab = 1;
                self.page_loaded = true;
                if !suppress {
                    history_push(value);
                }
                self.status = crate::gui::ui_text(
                    "Yerel KediWiki sayfası açıldı.",
                    "Local KediWiki page opened.",
                );
            }
            s if s.starts_with("minos://history") => {
                if s == "minos://history?action=clear" {
                    history_clear();
                    self.status =
                        crate::gui::ui_text("Tüm geçmiş temizlendi.", "All history cleared.");
                } else if let Some(idx_str) = s.strip_prefix("minos://history?action=delete&idx=") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        history_remove(idx);
                        self.status =
                            crate::gui::ui_text("Geçmiş kaydı silindi.", "History entry deleted.");
                    }
                } else {
                    self.status =
                        crate::gui::ui_text("Geçmiş sayfası açıldı.", "History page opened.");
                }
                self.active_tab = 3;
                self.page_loaded = true;
                self.set_url(b"minos://history");
                self.build_history_page();
            }
            "minos://bookmarks" | "minos://favorites" => {
                self.active_tab = 3;
                self.page_loaded = true;
                if !suppress {
                    history_push(value);
                }
                self.status = crate::gui::ui_text(
                    "Sık kullanılanlar sayfası açıldı.",
                    "Bookmarks page opened.",
                );
                self.set_url(b"minos://bookmarks");
                self.build_bookmarks_page();
            }
            "minos://console" => {
                self.active_tab = 4;
                self.page_loaded = true;
                if !suppress {
                    history_push(value);
                }
                self.status =
                    crate::gui::ui_text("Tarayıcı konsolu açıldı.", "Browser console opened.");
            }
            "minos://net" | "net://status" => {
                // Ağ durumu paneli — e1000 durumu + gateway ARP/ping testi
                self.active_tab = 2;
                self.net_online = crate::drivers::e1000::is_ready();
                if self.net_online {
                    // Gateway'e ARP çözümlemesi ve ICMP Echo gönder
                    let gateway = crate::drivers::netstack::gateway_ip();
                    let sent = crate::drivers::netstack::ping(gateway);
                    self.ping_ok = sent || crate::drivers::netstack::arp_lookup(gateway).is_some();
                    self.page_loaded = true;
                    self.status = crate::gui::ui_text(
                        "Ağ durumu sayfası açıldı (sınama gönderildi).",
                        "Network status page opened (test sent).",
                    );
                } else {
                    self.page_loaded = false;
                    self.status = crate::gui::ui_text(
                        "e1000 ağ kartı bulunamadı; ağ yığını pasif.",
                        "e1000 adapter unavailable; network stack is inactive.",
                    );
                }
                if self.page_loaded && !suppress {
                    history_push(value);
                }
            }
            "minos://demo" | "minos://demo-img" | "minos://demo-video" => {
                self.active_tab = 3;
                self.page_loaded = true;
                let title = "minOs Web Demo - HTML/CSS/JS";
                let tb = title.as_bytes();
                self.page_title_len = tb.len().min(64);
                self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);
                // Demo sayfası seçimine göre farklı HTML içeriği göster
                let demo_html = match url_path(&self.url) {
                    Some(path) if path == "/img" => DEMO_HTML_IMG,
                    Some(path) if path == "/video" => DEMO_HTML_VIDEO,
                    _ => DEMO_HTML_BASIC,
                };
                self.load_html(demo_html, 600, 256);
                self.status = crate::gui::ui_text(
                    "HTML/CSS/JS demo sayfası yüklendi.",
                    "HTML/CSS/JS demo page loaded.",
                );
            }
            _ => {
                self.page_loaded = false;
                self.status = crate::gui::ui_text(
                    "Desteklenen: https://, http:// ve minos:// adresleri.",
                    "Supported schemes: https://, http:// and minos://.",
                );
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
        if !self.request_pending {
            return;
        }
        let now = crate::boot::time::main_monotonic_ms();
        if self.request_deadline == 0 {
            self.request_deadline = now.wrapping_add(15000);
        }
        if now.wrapping_sub(self.request_deadline) < usize::MAX / 2 {
            self.request_pending = false;
            self.page_loaded = false;
            self.status = crate::gui::ui_text(
                "Ağ isteği zaman aşımına uğradı.",
                "Network request timed out.",
            );
            crate::drivers::netstack::tcp_clear_result();
            self.request_state = 0;
            self.request_deadline = 0;
            return;
        }
        match self.request_state {
            0 => {
                let host =
                    core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                let clean_host = host.trim().trim_start_matches('[').trim_end_matches(']');
                if let Some(_v6) = crate::drivers::netstack::parse_ipv6(clean_host) {
                    // IPv6 adresi doğrudan girildiğinde (ör: http://[::1]/ veya http://[fe80::1]/)
                    self.finish_network_error(crate::gui::ui_text(
                        "IPv6 yönlendirme hazır değil (IPv4 Gateway etkin).",
                        "IPv6 routing not active (IPv4 Gateway active).",
                    ));
                } else if let Some(ip) = crate::drivers::netstack::parse_ipv4(host) {
                    self.request_ip = ip;
                    let port = self.request_port;
                    if crate::drivers::netstack::tcp_begin(ip, port, 49153) {
                        self.request_state = 1;
                        self.status = crate::gui::ui_text(
                            "TCP bağlantısı kuruluyor...",
                            "Connecting over TCP...",
                        );
                    } else {
                        self.finish_network_error(crate::gui::ui_text(
                            "TCP bağlantısı başlatılamadı.",
                            "Could not start TCP connection.",
                        ));
                    }
                } else if crate::drivers::netstack::dns_query(host) {
                    self.request_state = 4;
                    self.status = crate::gui::ui_text(
                        "DNS yanıtı bekleniyor...",
                        "Waiting for DNS response...",
                    );
                } else {
                    self.finish_network_error(crate::gui::ui_text(
                        "DNS sorgusu gönderilemedi.",
                        "DNS query could not be sent.",
                    ));
                }
            }
            4 => {
                if let Some(ip) = crate::drivers::netstack::take_dns_result() {
                    self.request_ip = ip;
                    if crate::drivers::netstack::tcp_begin(ip, self.request_port, 49153) {
                        self.request_state = 1;
                        self.status = crate::gui::ui_text(
                            "TCP bağlantısı kuruluyor...",
                            "Connecting over TCP...",
                        );
                    } else {
                        self.finish_network_error(crate::gui::ui_text(
                            "TCP bağlantısı başlatılamadı.",
                            "Could not start TCP connection.",
                        ));
                    }
                }
            }
            1 => {
                if crate::drivers::netstack::tcp_reset_received() {
                    self.finish_network_error(crate::gui::ui_text(
                        "Sunucu bağlantıyı reddetti.",
                        "Server rejected the connection.",
                    ));
                } else if crate::drivers::netstack::tcp_syn_acked() {
                    let host = core::str::from_utf8(&self.request_host[..self.request_host_len])
                        .unwrap_or("");
                    let path = core::str::from_utf8(&self.request_path[..self.request_path_len])
                        .unwrap_or("/");

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
                        req_buf.extend_from_slice(
                            b"Content-Type: application/x-www-form-urlencoded\r\nContent-Length: ",
                        );
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
                        self.tls13_finished_sent = false;
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
                        let mut ecdhe_secret = [0u8; 32];
                        crate::drivers::tls::build_client_hello(
                            host,
                            &client_random,
                            &mut hello,
                            &mut ecdhe_secret,
                        );
                        let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
                        let mut tls_session =
                            crate::drivers::tls::TlsSession::new(host, client_random);
                        tls_session.note_client_hello(ch_body);
                        // TLS 1.3 anahtar türetimi için istemci ECDHE gizlisi saklanır.
                        tls_session.set_client_ecdhe_secret(ecdhe_secret);
                        if crate::drivers::netstack::tcp_send(&hello) {
                            self.tls_session = Some(tls_session);
                            self.request_state = 2;
                            self.web_request_sent = true;
                            self.status = crate::gui::ui_text(
                                "TLS el sıkışması ve yanıt bekleniyor...",
                                "Waiting for TLS handshake & response...",
                            );
                        } else {
                            self.finish_network_error(crate::gui::ui_text(
                                "TLS ClientHello gönderilemedi.",
                                "Failed to send TLS ClientHello.",
                            ));
                        }
                    } else {
                        if crate::drivers::netstack::tcp_send(&req_buf) {
                            self.request_state = 2;
                            self.web_request_sent = true;
                            self.status = crate::gui::ui_text(
                                "HTTP yanıtı bekleniyor...",
                                "Waiting for HTTP response...",
                            );
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
                    let n = crate::drivers::netstack::take_tcp_data(
                        &mut self.response_data[self.response_len..],
                    );
                    if n > 0 {
                        self.response_len += n;
                        if let Some(ref mut tls) = self.tls_session {
                            tls.feed_tcp_data(
                                &self.response_data[self.response_len - n..self.response_len],
                            );
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
                let mut tls_failed: Option<(Option<(u8, u8)>, String)> = None;
                if let Some(ref mut tls) = self.tls_session {
                    let mut hs_out = Vec::new();
                    let mut app_out = Vec::new();
                    match tls.process_records(&mut hs_out, &mut app_out) {
                        Ok(_) => {
                            if !app_out.is_empty() {
                                crate::serial_print("[HTTPS] Çözülen uygulama verisi: ");
                                crate::serial::print_dec(app_out.len() as u64);
                                crate::serial_print(" bayt\n");
                            }
                        }
                        Err(()) => {
                            let alert = tls.last_alert();
                            let context = tls.diagnostic_context();
                            let detail = match alert {
                                Some((level, desc)) => alloc::format!(
                                    "{} (description={}, level={} ({}))\n{}",
                                    crate::drivers::tls::alert_description_text(desc),
                                    desc,
                                    crate::drivers::tls::alert_level_text(level),
                                    level,
                                    context
                                ),
                                None => alloc::format!(
                                    "{}\n{}",
                                    crate::gui::ui_text(
                                        "TLS kayıt işleme hatası",
                                        "TLS record processing failed"
                                    ),
                                    context
                                ),
                            };
                            tls_failed = Some((alert, detail));
                        }
                    }
                    self.tls_app_data.extend_from_slice(&app_out);
                }
                if let Some((alert, detail)) = tls_failed {
                    let status = match alert {
                        Some((_, desc)) => crate::drivers::tls::alert_description_text(desc),
                        None => crate::gui::ui_text(
                            "TLS kayıt işleme hatası.",
                            "TLS record processing error.",
                        ),
                    };
                    // Sertifika doğrulaması başarısızsa el sıkışma burada durur;
                    // kullanıcıya sunucunun uyarısı yerine asıl nedeni göster.
                    let cert_error = self.tls_session.as_ref().and_then(|t| t.cert_error());
                    let status = match cert_error {
                        Some(crate::drivers::crypto::x509::CertValidationError::Expired) => {
                            crate::gui::ui_text(
                                "Sertifikanın süresi dolmuş.",
                                "The certificate has expired.",
                            )
                        }
                        Some(crate::drivers::crypto::x509::CertValidationError::NotYetValid) => {
                            crate::gui::ui_text(
                                "Sertifika henüz geçerli değil (sistem saati hatalı olabilir).",
                                "The certificate is not valid yet (system clock may be wrong).",
                            )
                        }
                        Some(
                            crate::drivers::crypto::x509::CertValidationError::HostnameMismatch,
                        ) => crate::gui::ui_text(
                            "Sertifika bu site adıyla eşleşmiyor.",
                            "The certificate does not match this host name.",
                        ),
                        Some(crate::drivers::crypto::x509::CertValidationError::UntrustedRoot) => {
                            crate::gui::ui_text(
                                "Sertifika güvenilir bir kök CA zincirine bağlanamadı.",
                                "The certificate does not chain to a trusted root CA.",
                            )
                        }
                        Some(_) => crate::gui::ui_text(
                            "Sertifika doğrulanamadı.",
                            "Could not verify the certificate.",
                        ),
                        None => status,
                    };
                    self.finish_network_error_with_detail(status, &detail);
                    crate::serial_print("[HTTPS] TLS işleme hatası ayrıntısı: ");
                    crate::serial_print(&detail);
                    crate::serial_print("\n");
                }

                // TLS 1.3: sunucunun Finished mesajı doğrulandıysa istemci
                // Finished kaydını üretip gönder (istemci el sıkışma trafik
                // anahtarıyla şifrelenir). Bu kayıt gönderilmeden uygulama
                // verisi gönderilemez.
                if !self.tls13_finished_sent {
                    let server_finished = self.tls_session.as_ref().map_or(false, |t| {
                        t.state() == crate::drivers::tls::TlsState::Tls13ServerFinished
                    });
                    if server_finished {
                        if let Some(ref mut tls) = self.tls_session {
                            // Eski ara cihazlar için ChangeCipherSpec (RFC 8446 D.4).
                            if let Some(ccs) = tls.tls13_change_cipher_spec() {
                                let _ = crate::drivers::netstack::tcp_send(&ccs);
                            }
                            if let Some(record) = tls.build_tls13_client_finished() {
                                let _ = crate::drivers::netstack::tcp_send(&record);
                                self.tls13_finished_sent = true;
                            } else {
                                self.finish_network_error(crate::gui::ui_text(
                                    "TLS 1.3 istemci Finished üretilemedi.",
                                    "Could not build TLS 1.3 client Finished.",
                                ));
                            }
                        }
                    }
                }

                // El sıkışma tamamlandıysa HTTP isteğini şifreleyip gönder.
                let tls_established = self.tls_session.as_ref().map_or(false, |t| {
                    t.state() == crate::drivers::tls::TlsState::Established
                });
                if tls_established && !self.tls_request_sent && !self.tls_pending_request.is_empty()
                {
                    let request = core::mem::take(&mut self.tls_pending_request);
                    let record = self
                        .tls_session
                        .as_mut()
                        .and_then(|t| t.encrypt_app_data(&request));
                    match record {
                        Some(rec) => {
                            let _ = crate::drivers::netstack::tcp_send(&rec);
                            self.tls_request_sent = true;
                            crate::serial_print("[HTTPS] Şifreli HTTP isteği gönderildi: ");
                            crate::serial::print_dec(rec.len() as u64);
                            crate::serial_print(" bayt\n");
                            self.status = crate::gui::ui_text(
                                "Güvenli istek gönderildi, yanıt bekleniyor...",
                                "Secure request sent, waiting for response...",
                            );
                        }
                        None => {
                            self.finish_network_error(crate::gui::ui_text(
                                "TLS isteği şifrelenemedi.",
                                "Could not encrypt TLS request.",
                            ));
                        }
                    }
                }
                // HTTPS'te ham (şifreli) kayıtlar HTTP olarak ayrıştırılmaz;
                // yalnızca çözülmüş uygulama verisi kullanılır.
                let data_to_parse: &[u8] =
                    if self.is_https || (tls_established && !self.tls_app_data.is_empty()) {
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
                        let status_code = if status_line.len() >= 12
                            && &status_line[..5] == b"HTTP/"
                        {
                            if let Some(sp) = status_line.iter().position(|&b| b == b' ') {
                                if status_line.len() >= sp + 4 {
                                    let d = |c: u8| (c as char).to_digit(10).unwrap_or(0) as u16;
                                    if status_line[sp + 1].is_ascii_digit()
                                        && status_line[sp + 2].is_ascii_digit()
                                        && status_line[sp + 3].is_ascii_digit()
                                    {
                                        Some(
                                            d(status_line[sp + 1]) * 100
                                                + d(status_line[sp + 2]) * 10
                                                + d(status_line[sp + 3]),
                                        )
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        // Set-Cookie başlıklarını ayrıştır ve sakla
                        let current_host_str =
                            core::str::from_utf8(&self.request_host[..self.request_host_len])
                                .unwrap_or("");
                        for line in headers.split(|b| *b == b'\n') {
                            let l = if line.last() == Some(&b'\r') {
                                &line[..line.len() - 1]
                            } else {
                                line
                            };
                            if let Some(colon) = l.iter().position(|&b| b == b':') {
                                if l[..colon].eq_ignore_ascii_case(b"Set-Cookie") {
                                    let cookie_str =
                                        core::str::from_utf8(&l[colon + 1..]).unwrap_or("").trim();
                                    if let Some(first_part) = cookie_str.split(';').next() {
                                        if let Some((k, v)) = first_part.split_once('=') {
                                            let name = k.trim();
                                            let val = v.trim();
                                            if !name.is_empty() {
                                                if let Some(pos) =
                                                    self.cookies.iter().position(|c| c.name == name)
                                                {
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
                            if code >= 400 {
                                let mut http_error = String::from("HTTP status ");
                                http_error.push_str(&alloc::format!("{}", code));
                                defer_network_error(&http_error);
                            }
                            if (code == 301
                                || code == 302
                                || code == 303
                                || code == 307
                                || code == 308)
                                && self.redirect_count < 3
                            {
                                let mut loc_target: Option<&[u8]> = None;
                                for line in headers.split(|b| *b == b'\n') {
                                    let l = if line.last() == Some(&b'\r') {
                                        &line[..line.len() - 1]
                                    } else {
                                        line
                                    };
                                    if let Some(colon) = l.iter().position(|&b| b == b':') {
                                        let key = &l[..colon];
                                        if key.eq_ignore_ascii_case(b"Location") {
                                            let mut val = &l[colon + 1..];
                                            while val
                                                .first()
                                                .map_or(false, |b| b.is_ascii_whitespace())
                                            {
                                                val = &val[1..];
                                            }
                                            while val
                                                .last()
                                                .map_or(false, |b| b.is_ascii_whitespace())
                                            {
                                                val = &val[..val.len() - 1];
                                            }
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
                                    self.tls13_finished_sent = false;

                                    let loc_slice = &loc_buf[..loc_len];
                                    let loc_str = core::str::from_utf8(loc_slice).unwrap_or("");
                                    let current_scheme =
                                        if self.is_https { "https://" } else { "http://" };
                                    let host = core::str::from_utf8(
                                        &self.request_host[..self.request_host_len],
                                    )
                                    .unwrap_or("");
                                    let current_path = core::str::from_utf8(
                                        &self.request_path[..self.request_path_len],
                                    )
                                    .unwrap_or("/");
                                    let current_path =
                                        current_path.split('?').next().unwrap_or("/");
                                    let base_path = current_path
                                        .rsplit_once('/')
                                        .map(|(base, _)| base)
                                        .unwrap_or("");
                                    let mut target = String::new();
                                    if loc_str.starts_with("http://")
                                        || loc_str.starts_with("https://")
                                    {
                                        target.push_str(loc_str);
                                    } else if loc_str.starts_with("//") {
                                        target.push_str(if self.is_https {
                                            "https:"
                                        } else {
                                            "http:"
                                        });
                                        target.push_str(loc_str);
                                    } else {
                                        target.push_str(current_scheme);
                                        target.push_str(host);
                                        let default_port = if self.is_https { 443 } else { 80 };
                                        if self.request_port != default_port {
                                            use core::fmt::Write as _;
                                            let _ = write!(target, ":{}", self.request_port);
                                        }
                                        if loc_str.starts_with('/') {
                                            target.push_str(loc_str);
                                        } else if loc_str.starts_with('?') {
                                            target.push_str(current_path);
                                            target.push_str(loc_str);
                                        } else {
                                            target.push_str(base_path);
                                            target.push('/');
                                            target.push_str(loc_str);
                                        }
                                    }
                                    if target.len() <= self.url.len() {
                                        self.set_url(target.as_bytes());
                                        self.request_pending = false;
                                        self.request_state = 0;
                                        self.request_deadline = 0;
                                        crate::drivers::netstack::tcp_clear_result();
                                        self.suppress_history = true;
                                        let redirect_count = self.redirect_count;
                                        self.navigate();
                                        // navigate() normal gezinme için sayacı sıfırlar.
                                        self.redirect_count = redirect_count;
                                    } else {
                                        self.finish_network_error(crate::gui::ui_text(
                                            "Yönlendirme adresi çok uzun.",
                                            "Redirect URL is too long.",
                                        ));
                                    }
                                    return;
                                }
                            }
                        }

                        // Normal yanıt içeriği
                        self.page_title = [0; 64];
                        let title_len = self.request_host_len.min(64);
                        self.page_title[..title_len]
                            .copy_from_slice(&self.request_host[..title_len]);
                        self.page_title_len = title_len;

                        self.page_lines.clear();
                        let status_str =
                            core::str::from_utf8(status_line).unwrap_or("HTTP Response");
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
                        body_bytes
                            .extend_from_slice(&data_to_parse[body_start..data_to_parse.len()]);

                        // Transfer-Encoding: chunked kontrolü
                        let is_chunked = headers.split(|b| *b == b'\n').any(|line| {
                            let l = if line.last() == Some(&b'\r') {
                                &line[..line.len() - 1]
                            } else {
                                line
                            };
                            if let Some(colon) = l.iter().position(|&b| b == b':') {
                                if l[..colon].eq_ignore_ascii_case(b"Transfer-Encoding") {
                                    let val = &l[colon + 1..];
                                    return val
                                        .windows(7)
                                        .any(|w| w.eq_ignore_ascii_case(b"chunked"));
                                }
                            }
                            false
                        });

                        if is_chunked {
                            if let Some(decoded_len) =
                                crate::drivers::http::decode_chunked(&mut body_bytes, raw_body_len)
                            {
                                body_bytes.truncate(decoded_len);
                            }
                        }

                        // Basit indirme için son yanıt gövdesini sakla.
                        self.download_data = Some(body_bytes.clone());

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
                // Sunucu close_notify gönderdiyse ve yanıt zaten çözülüp
                // işlendiyse isteği başarıyla bitir. close_notify bir hata
                // değil, düzgün bağlantı kapanışıdır (RFC 8446 §6.1).
                let peer_closed = self.tls_session.as_ref().map_or(false, |t| t.peer_closed());
                if peer_closed && self.page_loaded {
                    self.status =
                        crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received.");
                    self.request_pending = false;
                    self.request_state = 0;
                    self.request_deadline = 0;
                    crate::drivers::netstack::tcp_clear_result();
                }
                self.flush_network_errors();
                if crate::drivers::netstack::tcp_finished()
                    || self.response_len >= self.response_data.len()
                {
                    crate::serial_print("[HTTPS] Bitiş: resp_len=");
                    crate::serial::print_dec(self.response_len as u64);
                    crate::serial_print(" app_len=");
                    crate::serial::print_dec(self.tls_app_data.len() as u64);
                    crate::serial_print(" page_loaded=");
                    crate::serial_print(if self.page_loaded { "1" } else { "0" });
                    crate::serial_print(" req_sent=");
                    crate::serial_print(if self.tls_request_sent { "1" } else { "0" });
                    crate::serial_print(" ctx=");
                    crate::serial_print(
                        &self
                            .tls_session
                            .as_ref()
                            .map(|t| t.diagnostic_context())
                            .unwrap_or_else(|| String::from("oturum yok")),
                    );
                    crate::serial_print("\n");
                    if self.response_len > 0 && !self.page_loaded {
                        self.finish_http_response();
                    }
                    if self.page_loaded {
                        self.status =
                            crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received.");
                        self.request_pending = false;
                        self.request_state = 0;
                        self.request_deadline = 0;
                        crate::drivers::netstack::tcp_clear_result();
                    } else {
                        self.finish_network_error(crate::gui::ui_text(
                            "HTTP yanıtı okunamadı.",
                            "Could not parse HTTP response.",
                        ));
                    }
                }
            }
            _ => self.finish_network_error(crate::gui::ui_text(
                "Ağ isteği başarısız.",
                "Network request failed.",
            )),
        }
    }

    fn finish_http_response(&mut self) {
        self.page_loaded = true;
        self.status = crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received.");
    }
    fn finish_network_error(&mut self, status: &'static str) {
        self.finish_network_error_with_detail(status, status);
    }

    fn finish_network_error_with_detail(&mut self, status: &'static str, detail: &str) {
        self.record_network_error(detail);
        self.request_pending = false;
        self.page_loaded = false;
        self.status = status;
        self.request_state = 0;
        self.request_deadline = 0;
        crate::drivers::netstack::tcp_clear_result();
    }

    /// Ertelenmiş ağ hata mesajlarını hata günlüğüne aktarır.
    fn flush_network_errors(&mut self) {
        let mut pending: Vec<String> = Vec::new();
        for slot in PENDING_ERRORS.iter() {
            let mut buf = [0u8; PENDING_ERROR_LEN];
            for (dst, cell) in buf.iter_mut().zip(slot.iter()) {
                *dst = cell.load(Ordering::Relaxed) as u8;
            }
            if buf[0] == 0 {
                continue;
            }
            if let Ok(text) = core::str::from_utf8(&buf) {
                pending.push(String::from(text.trim_end_matches('\0')));
            }
            for cell in slot.iter() {
                cell.store(0, Ordering::Relaxed);
            }
        }
        for message in pending {
            self.record_network_error(&message);
        }
    }

    fn record_network_error(&mut self, message: &str) {
        let url = String::from(
            core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("(unknown URL)"),
        );
        let severity = crate::gui::ui_text("HATA", "ERROR");
        self.error_logs
            .push(alloc::format!("{} | {} | {}", severity, url, message));
        if self.error_logs.len() > BROWSER_ERROR_LOG_LIMIT {
            self.error_logs.remove(0);
        }
    }

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

    pub fn show_console(&mut self) {
        self.set_url(b"minos://console");
        self.navigate();
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        scroll: &crate::gui::ScrollState,
    ) {
        let english = crate::gui::ui_english();
        // Bağlantı isabet alanlarını sıfırla; yalnızca HTML içerik yeniden doldurur.
        self.rendered_links.borrow_mut().clear();
        // Araç çubuğu: gezinme, adres ve taşma menüsü.
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 30, Color::TITLE_BAR);
        fb.draw_rect(win_x + 12, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 36, "<", Color::TEXT_MAIN);
        if self.focused_control == 0 {
            fb.draw_rect_outline(win_x + 12, win_y + 32, 22, 20, Color::ACCENT_YELLOW);
        }

        fb.draw_rect(win_x + 38, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 36, ">", Color::TEXT_MAIN);
        if self.focused_control == 1 {
            fb.draw_rect_outline(win_x + 38, win_y + 32, 22, 20, Color::ACCENT_YELLOW);
        }

        fb.draw_rect(win_x + 64, win_y + 32, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 36, "R", Color::ACCENT_GREEN);
        if self.focused_control == 2 {
            fb.draw_rect_outline(win_x + 64, win_y + 32, 22, 20, Color::ACCENT_YELLOW);
        }

        let url_x = win_x + 94;
        let url_w = win_w.saturating_sub(142);
        fb.draw_rect(
            url_x,
            win_y + 32,
            url_w,
            20,
            if self.address_focused {
                Color::WINDOW_BG
            } else {
                Color::DARK_BG
            },
        );
        let visible_chars = Self::address_visible_chars(url_w);
        let addr_scroll = self.address_effective_scroll(url_w);
        let visible_end = (addr_scroll + visible_chars).min(self.url_len);
        let clip = (url_x + 4, win_y + 32, url_w.saturating_sub(8), 20);
        let text_x = url_x + 6;
        let selection = self
            .address_selection
            .map(|(a, b)| (a.min(b).max(addr_scroll), a.max(b).min(visible_end)))
            .filter(|(a, b)| a < b);
        if let Some((sel_start, sel_end)) = selection {
            let prefix = core::str::from_utf8(&self.url[addr_scroll..sel_start]).unwrap_or("");
            let selected = core::str::from_utf8(&self.url[sel_start..sel_end]).unwrap_or("");
            let suffix = core::str::from_utf8(&self.url[sel_end..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(
                fb,
                text_x,
                win_y + 36,
                prefix,
                Color::TEXT_MAIN,
                clip,
            );
            let selection_x = text_x + MinGuiCompositor::text_width(prefix, 1);
            let selection_w = MinGuiCompositor::text_width(selected, 1);
            fb.draw_rect(selection_x, win_y + 32, selection_w, 20, Color::ACCENT_BLUE);
            MinGuiCompositor::draw_string_clipped(
                fb,
                selection_x,
                win_y + 36,
                selected,
                MinGuiCompositor::text_on(Color::ACCENT_BLUE),
                clip,
            );
            MinGuiCompositor::draw_string_clipped(
                fb,
                selection_x + selection_w,
                win_y + 36,
                suffix,
                Color::TEXT_MAIN,
                clip,
            );
        } else {
            let shown = core::str::from_utf8(&self.url[addr_scroll..visible_end]).unwrap_or("");
            MinGuiCompositor::draw_string_clipped(
                fb,
                text_x,
                win_y + 36,
                shown,
                Color::TEXT_MAIN,
                clip,
            );
        }
        if self.address_focused && selection.is_none() {
            let caret_end = self.address_cursor.min(self.url_len);
            let caret_start = addr_scroll.min(caret_end);
            let caret_prefix =
                core::str::from_utf8(&self.url[caret_start..caret_end]).unwrap_or("");
            let caret_x = text_x + MinGuiCompositor::text_width(caret_prefix, 1);
            if caret_x < url_x + url_w.saturating_sub(2) {
                fb.draw_rect(caret_x, win_y + 34, 1, 16, Color::ACCENT_YELLOW);
            }
        }
        if self.focused_control == 4 {
            fb.draw_rect_outline(url_x, win_y + 32, url_w, 20, Color::ACCENT_YELLOW);
        }

        // Araç çubuğu seçenekleri taşma menüsünde sunulur.
        let overflow = Self::toolbar_menu_button_rect(win_x, win_y, win_w);
        fb.draw_rect(
            overflow.0,
            overflow.1,
            overflow.2,
            overflow.3,
            Color::WINDOW_BG,
        );
        for dot in 0..3 {
            fb.draw_rect(
                overflow.0 + 13,
                overflow.1 + 4 + dot * 5,
                2,
                2,
                Color::TEXT_MAIN,
            );
        }

        // Sık Kullanılanlar / Yer İmleri Çubuğu (Bookmarks Bar)
        let bbar_y = win_y + 59;
        let bbar_h = 22;
        fb.draw_rect(win_x + 6, bbar_y, win_w - 12, bbar_h, Color::TITLE_BAR);
        fb.draw_rect(win_x + 6, bbar_y + bbar_h - 1, win_w - 12, 1, Color::BORDER);

        let mut cur_bx = win_x + 12;
        let star_w = MinGuiCompositor::text_width("*", 1);
        MinGuiCompositor::draw_string(fb, cur_bx, bbar_y + 4, "*", Color::ACCENT_YELLOW);
        cur_bx += star_w + 6;

        for bm in BOOKMARKS.iter().chain(self.bookmarks_extra.iter()) {
            let label = crate::gui::ui_text(bm.title, bm.title_en);
            let tw = MinGuiCompositor::text_width(label, 1);
            let chip_w = tw + 12;
            if cur_bx + chip_w > win_x + win_w - 10 {
                break;
            }
            let is_active = self.url_matches(bm.url);
            let bg_col = if is_active {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            };
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
            if self.active_tab == 4 {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 16,
                    crate::gui::ui_text("minOs | Tarayıcı Konsolu", "minOs | Browser Console"),
                    Color::ACCENT_PINK,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 32,
                    crate::gui::ui_text(
                        "Ağ, TLS ve HTTP hata kayıtları",
                        "Network, TLS, and HTTP errors",
                    ),
                    Color::ACCENT_BLUE,
                );
                let count = alloc::format!("{}", self.error_logs.len());
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + page_w.saturating_sub(48),
                    page_y + 32,
                    &count,
                    Color::TEXT_MUTED,
                );

                if self.error_logs.is_empty() {
                    MinGuiCompositor::draw_string(
                        fb,
                        page_x + 20,
                        page_y + 66,
                        crate::gui::ui_text("Henüz hata kaydı yok.", "No errors recorded."),
                        Color::TEXT_MUTED,
                    );
                } else {
                    let visible_rows = page_h.saturating_sub(68) / 18;
                    let total_rows: usize = self
                        .error_logs
                        .iter()
                        .map(|entry| entry.lines().count())
                        .sum();
                    let first_row = total_rows.saturating_sub(visible_rows);
                    let mut log_row = 0;
                    for entry in &self.error_logs {
                        for (part, line) in entry.lines().enumerate() {
                            if log_row >= first_row {
                                let row = log_row - first_row;
                                let row_y = page_y + 62 + row * 18;
                                if row % 2 == 0 {
                                    fb.draw_rect(
                                        page_x + 12,
                                        row_y,
                                        page_w.saturating_sub(24),
                                        17,
                                        Color::WINDOW_BG,
                                    );
                                }
                                MinGuiCompositor::draw_string_clipped(
                                    fb,
                                    page_x + 18,
                                    row_y + 4,
                                    line,
                                    if part == 0 {
                                        Color::ACCENT_RED
                                    } else {
                                        Color::TEXT_MUTED
                                    },
                                    (page_x + 14, row_y, page_w.saturating_sub(28), 17),
                                );
                            }
                            log_row += 1;
                        }
                    }
                }
            } else if self.active_tab == 3 {
                // Canlı Web Sayfası Render Paneli (HTML DOM)
                fb.draw_rect(
                    page_x + 8,
                    page_y + 8,
                    page_w.saturating_sub(16),
                    26,
                    Color::TITLE_BAR,
                );
                let title_str = if self.page_title_len > 0 {
                    let page = core::str::from_utf8(&self.page_title[..self.page_title_len])
                        .unwrap_or("");
                    alloc::format!("SarmanNet - {}", page)
                } else {
                    alloc::format!("SarmanNet - Title")
                };
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 16,
                    page_y + 14,
                    &title_str,
                    Color::ACCENT_GREEN,
                );

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
                    fb.draw_rect(
                        doc_x + 10,
                        doc_y + 6,
                        doc_w.saturating_sub(20),
                        24,
                        Color::ACCENT_YELLOW,
                    );
                    MinGuiCompositor::draw_string_clipped(
                        fb,
                        doc_x + 16,
                        doc_y + 10,
                        alert_msg,
                        Color::BLACK,
                        doc_clip,
                    );
                }

                // Clear rendered links for hit testing
                let mut links = self.rendered_links.borrow_mut();
                links.clear();

                let mut cur_line_y = doc_y + 12 + if self.js_alert.is_some() { 28 } else { 0 };
                let mut find_span_index = 0usize;
                for (line_idx, line) in self.page_lines.iter().enumerate() {
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

                    let visible = cur_line_y + line_h >= doc_y + scroll_y
                        && cur_line_y <= doc_y + scroll_y + doc_h + 30;
                    let draw_y = cur_line_y.saturating_sub(scroll_y);

                    if visible {
                        let total_line_w: usize = line
                            .spans
                            .iter()
                            .map(|s| {
                                MinGuiCompositor::text_width(&s.text, scale)
                                    + if s.is_button { 16 } else { 0 }
                            })
                            .sum();

                        let mut cur_x = match line.align {
                            TextAlign::Center => {
                                let avail = doc_w.saturating_sub(24);
                                doc_x + 12 + (avail.saturating_sub(total_line_w) / 2)
                            }
                            TextAlign::Right => doc_x + doc_w.saturating_sub(12 + total_line_w),
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

                        // <hr> yatay ayraç çizgisi
                        if line.tag == HtmlTagType::Rule {
                            let rule_y = draw_y + line_h / 2;
                            if rule_y >= doc_y && rule_y < doc_y + doc_h {
                                let rule_x = doc_x + 12;
                                let rule_w = doc_w.saturating_sub(24);
                                if rule_w > 0 {
                                    fb.draw_rect(rule_x, rule_y, rule_w, 1, Color::BORDER);
                                }
                            }
                        }

                        // <blockquote> sol dikey çubuk
                        if line.tag == HtmlTagType::Quote {
                            if draw_y + line_h > doc_y && draw_y < doc_y + doc_h {
                                let bar_top = draw_y.max(doc_y);
                                let bar_bottom = (draw_y + line_h).min(doc_y + doc_h);
                                if bar_bottom > bar_top {
                                    fb.draw_rect(
                                        doc_x + 8,
                                        bar_top,
                                        3,
                                        bar_bottom - bar_top,
                                        Color::BORDER,
                                    );
                                }
                            }
                        }

                        let mut span_char_start = 0usize;
                        for span in &line.spans {
                            let raw_text_w = MinGuiCompositor::text_width(&span.text, scale);
                            let span_w = raw_text_w + if span.is_button { 16 } else { 0 };
                            let text_color = if span.is_link {
                                Color::ACCENT_BLUE
                            } else {
                                span.color
                            };
                            let span_len = span.text.len();
                            let span_char_end = span_char_start + span_len;

                            let sel_range =
                                match (self.page_selection_start, self.page_selection_end) {
                                    (Some((sl, sc)), Some((el, ec))) if sl != el || sc != ec => {
                                        if line_idx < sl || line_idx > el {
                                            None
                                        } else {
                                            let line_sel_s = if line_idx == sl { sc } else { 0 };
                                            let line_sel_e =
                                                if line_idx == el { ec } else { usize::MAX };
                                            if span_char_end > line_sel_s
                                                && span_char_start < line_sel_e
                                            {
                                                let s = line_sel_s
                                                    .saturating_sub(span_char_start)
                                                    .min(span_len);
                                                let e = line_sel_e
                                                    .saturating_sub(span_char_start)
                                                    .min(span_len);
                                                if s < e {
                                                    Some((s, e))
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        }
                                    }
                                    _ => None,
                                };

                            if span.is_button {
                                let btn_bg = span.bg_color.unwrap_or(Color::TITLE_BAR);
                                fb.draw_rect(cur_x, draw_y, span_w, line_h, btn_bg);
                                fb.draw_rect_outline(cur_x, draw_y, span_w, line_h, Color::BORDER);
                                MinGuiCompositor::draw_string_scaled_clipped(
                                    fb,
                                    cur_x + 8,
                                    draw_y + 2,
                                    &span.text,
                                    span.color,
                                    scale,
                                    doc_clip,
                                );
                                {
                                    links.push(RenderedLink {
                                        rect: (cur_x, draw_y, span_w, line_h),
                                        url: span.link_url.clone(),
                                        onclick: span.onclick.clone(),
                                    });
                                }
                            } else if let Some((s, e)) = sel_range {
                                if let Some(bg) = span.bg_color {
                                    fb.draw_rect(cur_x, draw_y, span_w, line_h, bg);
                                }
                                let prefix = &span.text[..s];
                                let selected = &span.text[s..e];
                                let suffix = &span.text[e..];
                                let prefix_w = MinGuiCompositor::text_width(prefix, scale);
                                let sel_w = MinGuiCompositor::text_width(selected, scale);

                                if !prefix.is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb, cur_x, draw_y, prefix, text_color, scale, doc_clip,
                                    );
                                }
                                if !selected.is_empty() {
                                    fb.draw_rect(
                                        cur_x + prefix_w,
                                        draw_y,
                                        sel_w,
                                        line_h,
                                        Color::ACCENT_BLUE,
                                    );
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x + prefix_w,
                                        draw_y,
                                        selected,
                                        Color::WHITE,
                                        scale,
                                        doc_clip,
                                    );
                                }
                                if !suffix.is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x + prefix_w + sel_w,
                                        draw_y,
                                        suffix,
                                        text_color,
                                        scale,
                                        doc_clip,
                                    );
                                }
                            } else if let Some((s, e)) = sel_range {
                                if let Some(bg) = span.bg_color {
                                    fb.draw_rect(cur_x, draw_y, span_w, line_h, bg);
                                }
                                let prefix = &span.text[..s];
                                let selected = &span.text[s..e];
                                let suffix = &span.text[e..];
                                let prefix_w = MinGuiCompositor::text_width(prefix, scale);
                                let sel_w = MinGuiCompositor::text_width(selected, scale);

                                if !prefix.is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb, cur_x, draw_y, prefix, text_color, scale, doc_clip,
                                    );
                                }
                                if !selected.is_empty() {
                                    fb.draw_rect(
                                        cur_x + prefix_w,
                                        draw_y,
                                        sel_w,
                                        line_h,
                                        Color::ACCENT_BLUE,
                                    );
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x + prefix_w,
                                        draw_y,
                                        selected,
                                        Color::WHITE,
                                        scale,
                                        doc_clip,
                                    );
                                }
                                if !suffix.is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x + prefix_w + sel_w,
                                        draw_y,
                                        suffix,
                                        text_color,
                                        scale,
                                        doc_clip,
                                    );
                                }
                            } else {
                                if let Some(bg) = span.bg_color {
                                    fb.draw_rect(cur_x, draw_y, span_w, line_h, bg);
                                }
                                let query = core::str::from_utf8(&self.find_query[..self.find_len])
                                    .unwrap_or("");
                                let matched = self.find_active
                                    && !query.is_empty()
                                    && span
                                        .text
                                        .to_ascii_lowercase()
                                        .contains(&query.to_ascii_lowercase());
                                if matched {
                                    let active_match =
                                        find_span_index == self.find_match_index.get();
                                    find_span_index += 1;
                                    let match_color = if active_match {
                                        Color::ACCENT_GREEN
                                    } else {
                                        Color::ACCENT_YELLOW
                                    };
                                    fb.draw_rect(
                                        cur_x,
                                        draw_y,
                                        span_w
                                            .min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x))),
                                        line_h,
                                        match_color,
                                    );
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x,
                                        draw_y,
                                        &span.text,
                                        MinGuiCompositor::text_on(match_color),
                                        scale,
                                        doc_clip,
                                    );
                                } else {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb, cur_x, draw_y, &span.text, text_color, scale, doc_clip,
                                    );
                                }
                                // Sentez kalın: 1px sağa ikinci geçiş
                                if span.bold && !span.text.trim().is_empty() {
                                    MinGuiCompositor::draw_string_scaled_clipped(
                                        fb,
                                        cur_x + 1,
                                        draw_y,
                                        &span.text,
                                        text_color,
                                        scale,
                                        doc_clip,
                                    );
                                }
                                // Altı çizili
                                if span.underline && !span.text.trim().is_empty() {
                                    let ul_y = draw_y + line_h.saturating_sub(2);
                                    if ul_y >= doc_y && ul_y < doc_y + doc_h {
                                        let ul_w = span_w
                                            .min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x)));
                                        fb.draw_rect(cur_x, ul_y, ul_w, 1, text_color);
                                    }
                                }

                                if span.is_link || !span.onclick.is_empty() {
                                    let underline_y = draw_y + line_h.saturating_sub(3);
                                    if underline_y >= doc_y && underline_y < doc_y + doc_h {
                                        let ul_w = span_w
                                            .min(doc_w.saturating_sub(cur_x.saturating_sub(doc_x)));
                                        fb.draw_rect(
                                            cur_x,
                                            underline_y,
                                            ul_w,
                                            1,
                                            Color::ACCENT_BLUE,
                                        );
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
                            span_char_start = span_char_end;
                            cur_x += span_w;
                        }
                    }

                    cur_line_y += line_h;
                }
                if self.find_active && self.find_len > 0 {
                    let needle = core::str::from_utf8(&self.find_query[..self.find_len])
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let mut count = 0usize;
                    for line in &self.page_lines {
                        for span in &line.spans {
                            if span.text.to_ascii_lowercase().contains(&needle) {
                                count += 1;
                            }
                        }
                    }
                    self.find_match_count.set(count);
                } else {
                    self.find_match_count.set(0);
                }
            } else if self.active_tab == 2 {
                // Ağ durumu paneli
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 16,
                    crate::gui::ui_text(
                        "minOs | Ağ Durumu (e1000)",
                        "minOs | Network Status (e1000)",
                    ),
                    Color::ACCENT_PINK,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 32,
                    crate::gui::ui_text(
                        "ICMP / ARP tabanlı sınama paneli",
                        "ICMP / ARP test panel",
                    ),
                    Color::ACCENT_BLUE,
                );
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                let net_str = match (self.net_online, english) {
                    (true, true) => "e1000: ONLINE (MMIO active)",
                    (false, true) => "e1000: OFFLINE",
                    (true, false) => "e1000: ÇEVRIMICI (MMIO aktif)",
                    (false, false) => "e1000: ÇEVRIMDIŞI",
                };
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 64,
                    net_str,
                    Color::ACCENT_GREEN,
                );
                let ping_str = match (self.ping_ok, english) {
                    (true, true) => "Gateway ARP: FOUND (10.0.2.2)",
                    (false, true) => "Gateway ARP: WAITING / NO REPLY",
                    (true, false) => "Gateway ARP: BULUNDU (10.0.2.2)",
                    (false, false) => "Gateway ARP: BEKLIYOR / YANIT YOK",
                };
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 84,
                    ping_str,
                    Color::TEXT_MAIN,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 104,
                    "IP: 10.0.2.15/24 GW: 10.0.2.2",
                    Color::TEXT_MUTED,
                );
            } else if self.active_tab == 0 {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 16,
                    crate::gui::ui_text(
                        "minOs | Yerel Yardım ve Sık Kullanılanlar",
                        "minOs | Local Help and Bookmarks",
                    ),
                    Color::ACCENT_PINK,
                );
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://bookmarks, minos://cats, minos://help, minos://net", "Local addresses: minos://bookmarks, minos://cats, minos://help, minos://net"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 110, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 64,
                    crate::gui::ui_text(
                        "★ Sık Kullanılan:(saf HTTP test sitesi)",
                        "★ Favorite: (pure HTTP test site)",
                    ),
                    Color::ACCENT_GREEN,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 84,
                    crate::gui::ui_text(
                        "minos://net ile ağ durumunu, minos://history ile geçmişi açın.",
                        "Check the network at minos://net, open history at minos://history.",
                    ),
                    Color::TEXT_MAIN,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 104,
                    crate::gui::ui_text(
                        "Yer imleri çubuğundan veya minos://bookmarks ile tüm favorileri açın.",
                        "Open bookmarks via the toolbar or visit minos://bookmarks.",
                    ),
                    Color::TEXT_MUTED,
                );
            } else {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 16,
                    crate::gui::ui_text("KediWiki | Yerel Sayfa", "KediWiki | Local Page"),
                    Color::ACCENT_PINK,
                );
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 32,
                    crate::gui::ui_text(
                        "Yerel adresler: minos://cats ve minos://help",
                        "Local addresses: minos://cats and minos://help",
                    ),
                    Color::ACCENT_BLUE,
                );
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(
                    fb,
                    page_x + 20,
                    page_y + 64,
                    crate::gui::ui_text("Kediler ve minOs!", "Cats and minOs!"),
                    Color::TEXT_MAIN,
                );
            }
        } else {
            MinGuiCompositor::draw_string(
                fb,
                page_x + 18,
                page_y + 28,
                crate::gui::ui_text("Bu adres açılamadı.", "This address could not be opened."),
                Color::ACCENT_RED,
            );
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, crate::gui::ui_text("Desteklenen adresler: minos://help, minos://cats, minos://net, minos://history", "Supported: minos://help, minos://cats, minos://net, minos://history"), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(
                fb,
                page_x + 18,
                page_y + 72,
                crate::gui::ui_text(
                    "Yerel: minos://help | Ağ: minos://net",
                    "Local: minos://help | Network: minos://net",
                ),
                Color::TEXT_MUTED,
            );
        }
        if self.find_active {
            let (find_x, find_y, _, _) = Self::find_bar_rect(win_x, win_y, win_w);
            fb.draw_rect(find_x, find_y, 244, 24, Color::TITLE_BAR);
            fb.draw_rect_outline(find_x, find_y, 244, 24, Color::BORDER);
            let query = core::str::from_utf8(&self.find_query[..self.find_len]).unwrap_or("");
            MinGuiCompositor::draw_string(
                fb,
                find_x + 6,
                find_y + 7,
                crate::gui::ui_text("Bul:", "Find:"),
                Color::TEXT_MAIN,
            );
            MinGuiCompositor::draw_string_clipped(
                fb,
                find_x + 48,
                find_y + 7,
                if query.is_empty() {
                    crate::gui::ui_text("Aramak için yaz", "type to search")
                } else {
                    query
                },
                Color::ACCENT_YELLOW,
                (find_x + 46, find_y + 2, 132, 20),
            );
            let count_text = alloc::format!(
                "{}/{}",
                if self.find_match_count.get() == 0 {
                    0
                } else {
                    self.find_match_index.get() + 1
                },
                self.find_match_count.get()
            );
            MinGuiCompositor::draw_string(
                fb,
                find_x + 184,
                find_y + 7,
                &count_text,
                Color::TEXT_MUTED,
            );
        }
        fb.draw_rect(
            win_x + 6,
            status_y,
            win_w.saturating_sub(12),
            status_h,
            Color::TASKBAR_BG,
        );
        let status = match self.status {
            "minOs yerel yardım sayfası" | "minOs local help page" => {
                crate::gui::ui_text("minOs yerel yardım sayfası", "minOs local help page")
            }
            "Yerel minOs yardım sayfası açıldı." | "Local minOs help page opened." => {
                crate::gui::ui_text(
                    "Yerel minOs yardım sayfası açıldı.",
                    "Local minOs help page opened.",
                )
            }
            "Yerel KediWiki sayfası açıldı." | "Local KediWiki page opened." => {
                crate::gui::ui_text(
                    "Yerel KediWiki sayfası açıldı.",
                    "Local KediWiki page opened.",
                )
            }
            "Ağ durumu sayfası açıldı (sınama gönderildi)."
            | "Network status page opened (test sent)." => crate::gui::ui_text(
                "Ağ durumu sayfası açıldı (sınama gönderildi).",
                "Network status page opened (test sent).",
            ),
            "e1000 ağ kartı bulunamadı; ağ yığını pasif."
            | "e1000 adapter unavailable; network stack is inactive." => crate::gui::ui_text(
                "e1000 ağ kartı bulunamadı; ağ yığını pasif.",
                "e1000 adapter unavailable; network stack is inactive.",
            ),
            "Geçersiz web adresi." | "Invalid web address." => {
                crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address.")
            }
            "Ağ kartı kullanılamıyor." | "Network adapter unavailable." => {
                crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable.")
            }
            "DNS çözümlemesi bekleniyor..." | "Resolving host..." => {
                crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host...")
            }
            "DNS adı çözümlenemedi." | "DNS lookup failed." => {
                crate::gui::ui_text("DNS adı çözümlenemedi.", "DNS lookup failed.")
            }
            "TCP bağlantısı kuruluyor..." | "Connecting over TCP..." => {
                crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP...")
            }
            "TCP bağlantısı başlatılamadı." | "Could not start TCP connection." => {
                crate::gui::ui_text(
                    "TCP bağlantısı başlatılamadı.",
                    "Could not start TCP connection.",
                )
            }
            "Sunucu bağlantıyı reddetti." | "Server rejected the connection." => {
                crate::gui::ui_text(
                    "Sunucu bağlantıyı reddetti.",
                    "Server rejected the connection.",
                )
            }
            "HTTP yanıtı bekleniyor..." | "Waiting for HTTP response..." => {
                crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response...")
            }
            "HTTP yanıtı alındı." | "HTTP response received." => {
                crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received.")
            }
            "HTTP yanıtı okunamadı." | "Could not parse HTTP response." => {
                crate::gui::ui_text("HTTP yanıtı okunamadı.", "Could not parse HTTP response.")
            }
            "Ağ isteği zaman aşımına uğradı." | "Network request timed out." => {
                crate::gui::ui_text(
                    "Ağ isteği zaman aşımına uğradı.",
                    "Network request timed out.",
                )
            }
            "Desteklenen: https://, http:// ve minos:// adresleri."
            | "Supported schemes: https://, http:// and minos://." => crate::gui::ui_text(
                "Desteklenen: https://, http:// ve minos:// adresleri.",
                "Supported schemes: https://, http:// and minos://.",
            ),
            "HTTPS/TLS bu çekirdek motorunda desteklenmiyor."
            | "HTTPS/TLS is not supported by this kernel renderer." => crate::gui::ui_text(
                "HTTPS/TLS bu çekirdek motorunda desteklenmiyor.",
                "HTTPS/TLS is not supported by this kernel renderer.",
            ),
            "DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor."
            | "DNS query sent; preparing HTTP connection." => crate::gui::ui_text(
                "DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor.",
                "DNS query sent; preparing HTTP connection.",
            ),
            "DNS sorgusu gönderilemedi." | "DNS query could not be sent." => {
                crate::gui::ui_text("DNS sorgusu gönderilemedi.", "DNS query could not be sent.")
            }
            _ => self.status,
        };
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 3, status, Color::TEXT_MUTED);
        if self.toolbar_menu_open {
            let (menu_x, menu_y, menu_w, menu_h) = Self::toolbar_menu_rect(win_x, win_y, win_w);
            fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
            fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
            let current_url = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
            let is_bookmarked = BOOKMARKS
                .iter()
                .chain(self.bookmarks_extra.iter())
                .any(|b| b.url == current_url);
            let labels = [
                crate::gui::ui_text("Geçmiş", "History"),
                crate::gui::ui_text(
                    if is_bookmarked {
                        "Yer imini kaldır"
                    } else {
                        "Yer imi ekle"
                    },
                    if is_bookmarked {
                        "Remove bookmark"
                    } else {
                        "Add bookmark"
                    },
                ),
                crate::gui::ui_text("İndirmeyi kaydet", "Save download"),
                "GIT",
                crate::gui::ui_text("Konsol", "Console"),
            ];
            for (i, label) in labels.iter().enumerate() {
                let item = Self::toolbar_menu_item_rect(win_x, win_y, win_w, i);
                fb.draw_rect(item.0, item.1, item.2, item.3, Color::WINDOW_BG);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    item.0 + 8,
                    item.1 + 6,
                    label,
                    Color::TEXT_MAIN,
                    (
                        item.0 + 3,
                        item.1 + 1,
                        item.2.saturating_sub(6),
                        item.3.saturating_sub(2),
                    ),
                );
            }
        }
    }
}

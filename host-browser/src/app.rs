//! minOs web tarayıcısı uygulaması (no_std, çekirdek entegrasyonu).
//! HTML rendering, gezinme ve sayfa durumu yönetimi.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::html::render_html_to_lines;
use crate::types::{Bookmark, BrowserLine};

/// Cizim arami. Çekirdek `Framebuffer` ile bu trait'i gerceklestirir.
pub trait Painter {
    /// Dolu dikdortgen cizer.
    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32);

    /// Metin cizer (Inria Sans veya esit).
    fn text(&mut self, x: usize, y: usize, text: &str, color: u32, scale: usize);

    /// Metin genisligini dondurur.
    fn text_width(&self, text: &str, scale: usize) -> usize;

    /// Metin yuksekligi dondurur.
    fn text_height(&self, scale: usize) -> usize;

    /// RGB24 resim cizer (w*h*3 bayt).
    fn image(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: &[u8]);

    /// Tum ekrani temizler.
    fn clear(&mut self, color: u32);
}

const DEMO_HTML_BASIC: &str = "<html><head><style>body{font-family:sans-serif;margin:40px;background:#f0f0f0;color:#333}h1{color:#d03030;text-align:center}.section{margin:20px 0}.btn{padding:8px 16px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer}.btn:hover{background:#184aae}.card{border:1px solid #ddd;border-radius:8px;padding:16px;margin:16px 0;background:white}.tag{display:inline-block;padding:4px 8px;margin:2px;border:1px solid #ccc;border-radius:12px;background:#e0e0e0;color:#333;font-size:0.8em}.highlight{background:#fff3a0;padding:4px 8px;border-radius:4;margin:2px}</style></head><body><h1>minOs HTML CSS JS Demo</h1><div class='section'><p>This page shows HTML CSS JavaScript capabilities.</p><p>Image demo: <a href='minos://demo-img'>image demo</a> | Video demo: <a href='minos://demo-video'>video demo</a></p><span class='tag'>HTML</span><span class='tag'>CSS</span><span class='tag'>JS</span></div></body></html>";

const DEMO_HTML_IMG: &str = "<html><head><style>body{font-family:sans-serif;margin:40px;background:#e0e0e0;color:#333}h1{color:#d03030;text-align:center}.section{margin:20px 0}.card{border:2px solid #2050c0;padding:20px;margin:20px 0;background:white;box-shadow:0 4px 12px rgba(0,0,0,0.15)}.img-container{border:2px solid #2050c0;padding:16px;margin:16px 0;background:#fafafa;text-align:center;border-radius:8}.img-container img{max-width:100%;height:auto;border:1px solid #ccc;border-radius:4}.img-info{margin-top:12px;color:#666;font-size:0.9em}.btn{padding:8px 16px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer;margin:4px}.btn:hover{background:#184aae}.tag{display:inline-block;padding:4px 8px;margin:2px;border:1px solid #ccc;border-radius:12px;background:#e0e0e0;color:#333;font-size:0.8em}</style></head><body><h1>minOs - Image Demo</h1><div class='section'><p>Shows image loading and display capability with CSS enhancements.</p><div class='card'><div class='img-container'><img src='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/5+hHgAHggJ/chD2wbwAAAABJRU5ErkJggg==' alt='Sample PNG Image' /><div class='img-info'>PNG format, sample image data</div></div><p class='tag tag-demo'>Live Image Demo</p></div></body></html>";

const DEMO_HTML_VIDEO: &str = "<html><head><style>body{font-family:sans-serif;margin:20px;background:#101018;color:#e0e0e0}h1{color:#d03030;text-align:center}.section{margin:20px 0}.video-container{border:2px solid #2050c0;padding:12px;margin:15px 0;background:#181820;text-align:center}video{max-width:100%;background:#000}.video-info{margin-top:8px;color:#aaa;font-size:0.9em}.btn{padding:6px 12px;background:#2050c0;color:white;border:none;border-radius:4px;cursor:pointer;margin:2px}.btn:hover{background:#184aae}</style></head><body><h1>minOs - Video Demo</h1><div class='section'><p>Shows video playback capability.</p><div class='video-container'><div style='width:320px;height:180px;background:#000;border:1px solid #333;display:flex;align-items:center;justify-content:center;color:#666;margin:0 auto;'>Video placeholder</div><div class='video-info'>Sample video player</div></div></body></html>";

pub const BOOKMARKS: &[Bookmark] = &[
    Bookmark {
        title: "HttpForever",
        title_en: "HttpForever",
        url: "http://httpforever.com/",
        description: "Hafif ve saf HTTP test sitesi (port 80).",
        description_en: "Lightweight and pure HTTP test site (port 80).",
    },
    Bookmark {
        title: "Example (HTTPS)",
        title_en: "Example (HTTPS)",
        url: "https://example.com/",
        description: "Güvenli HTTPS/TLS test sitesi (port 443).",
        description_en: "Secure HTTPS/TLS test site (port 443).",
    },
    Bookmark {
        title: "KediWiki",
        title_en: "KediWiki",
        url: "minos://cats",
        description: "Yerel kediler ve ansiklopedi sayfası.",
        description_en: "Local cat wiki and encyclopedia page.",
    },
    Bookmark {
        title: "Ağ Durumu",
        title_en: "Network",
        url: "minos://net",
        description: "e1000 kartı, ARP ve ağ tanılama aracı.",
        description_en: "e1000 card, ARP and network diagnostics tool.",
    },
    Bookmark {
        title: "Geçmiş",
        title_en: "History",
        url: "minos://history",
        description: "Ziyaret edilen web sayfaları geçmişi.",
        description_en: "History of visited web pages.",
    },
    Bookmark {
        title: "Yardım",
        title_en: "Help",
        url: "minos://help",
        description: "minOs web tarayıcısı kullanım kılavuzu.",
        description_en: "minOs web browser user guide.",
    },
    Bookmark {
        title: "Web Testi",
        title_en: "Web Demo",
        url: "minos://demo",
        description: "CSS, DOM ve JavaScript etkileşim testi.",
        description_en: "CSS, DOM and JavaScript interaction test.",
    },
];

/// Önbelleğe alınmış görüntü verisi (RGB24).
struct CachedImage {
    rgb: Vec<u8>,
    width: usize,
    height: usize,
}

pub struct HostBrowser {
    url: String,
    page_title: String,
    page_lines: Vec<BrowserLine>,
    page_loaded: bool,
    status: String,
    active_tab: usize,
    images: BTreeMap<usize, CachedImage>,
    bookmarks_extra: Vec<Bookmark>,
    favorites: Vec<String>,
    history: Vec<String>,
    history_pos: usize,
    scroll_offset: f32,
    selected_text: String,
    next_image_id: usize,
}

impl HostBrowser {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            page_title: String::new(),
            page_lines: Vec::new(),
            page_loaded: false,
            status: String::from("minOs Host Browser - Bir URL girin veya yerel sayfa açın."),
            active_tab: 0,
            images: BTreeMap::new(),
            bookmarks_extra: Vec::new(),
            favorites: Vec::new(),
            history: Vec::new(),
            history_pos: 0,
            scroll_offset: 0.0,
            selected_text: String::new(),
            next_image_id: 1,
        }
    }

    fn load_html(&mut self, html: &str, title: &str, wrap_width_px: usize, max_lines: usize) {
        self.page_title = String::from(title);
        let css = ""; // CSS html içinde inline olarak var
        let lines = render_html_to_lines(html.as_bytes(), css, wrap_width_px, max_lines);
        self.page_lines = lines;
        self.page_loaded = true;
        self.status = String::from("Sayfa yüklendi.");

        // Resimleri önbelleğe al (Painter üzerinden çizim için)
        self.cache_images();
    }

    fn cache_images(&mut self) {
        for line in &self.page_lines {
            for span in &line.spans {
                if span.is_image {
                    if let Some(rgb) = &span.image_rgb {
                        // Benzersiz anahtar için span adresini kullan
                        let key = rgb.as_ptr() as usize;
                        if !self.images.contains_key(&key) {
                            let width = span.image_width.max(1);
                            let height = span.image_height.max(1);
                            self.images.insert(
                                key,
                                CachedImage {
                                    rgb: rgb.clone(),
                                    width,
                                    height,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    fn get_demo_html(&self, path: Option<&str>) -> String {
        match path {
            Some("/img") => String::from(DEMO_HTML_IMG),
            Some("/video") => String::from(DEMO_HTML_VIDEO),
            _ => String::from(DEMO_HTML_BASIC),
        }
    }

    pub fn navigate(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }

        let value = if !trimmed.contains("://") && !trimmed.starts_with("minos:") {
            format!("http://{}", trimmed)
        } else {
            String::from(trimmed)
        };

        self.url = value.clone();
        self.page_loaded = false;
        self.page_lines.clear();
        self.images.clear();

        if value.starts_with("minos://help") {
            self.active_tab = 0;
            self.page_loaded = true;
            self.status = String::from("Yerel minOs yardım sayfası açıldı.");
            self.load_html(
                "<html><body><h1>minOs Yardım</h1><p>Bu, minOs web tarayıcısının çekirdek sürümüdür.</p><p>Özellikler:</p><ul><li>HTML rendering</li><li>CSS stilleri</li><li>PNG görüntüleme (data: URLs)</li><li>Bağlantılar ve yer imleri</li></ul></body></html>",
                "minOs Help",
                800,
                1000,
            );
        } else if value.starts_with("minos://cats") {
            self.active_tab = 1;
            self.page_loaded = true;
            self.status = String::from("Yerel KediWiki sayfası açıldı.");
            self.load_html(
                "<html><body><h1>KediWiki</h1><p>Yerel kediler ve ansiklopedi sayfası.</p><p><img src='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/5+hHgAHggJ/chD2wbwAAAABJRU5ErkJggg==' alt='Kedi' /></p></body></html>",
                "KediWiki",
                800,
                1000,
            );
        } else if value == "minos://history" || value.starts_with("minos://history?") {
            self.active_tab = 3;
            self.page_loaded = true;
            self.status = String::from("Geçmiş sayfası açıldı.");
            self.build_history_page();
        } else if value == "minos://bookmarks" || value == "minos://favorites" {
            self.active_tab = 3;
            self.page_loaded = true;
            self.status = String::from("Sık kullanılanlar sayfası açıldı.");
            self.build_bookmarks_page();
        } else if value == "minos://console" {
            self.active_tab = 4;
            self.page_loaded = true;
            self.status = String::from("Tarayıcı konsolu açıldı.");
            self.load_html(
                "<html><body><h1>Tarayıcı Konsolu</h1><p>JavaScript konsolu (çekirdek sürümünde sınırlı).</p></body></html>",
                "Console",
                800,
                1000,
            );
        } else if value == "minos://net" || value == "net://status" {
            self.active_tab = 2;
            self.page_loaded = true;
            self.status = String::from("Ağ durumu sayfası açıldı.");
            self.load_html(
                "<html><body><h1>Ağ Durumu</h1><p>Çekirdek sürümünde ağ testleri sınırlıdır.</p><p>Durum: Çevrimiçi (simülasyon)</p></body></html>",
                "Network",
                800,
                1000,
            );
        } else if value == "minos://demo" || value == "minos://demo-img" || value == "minos://demo-video" {
            self.active_tab = 3;
            self.page_loaded = true;
            let path = if value.contains("/img") { Some("/img") } else if value.contains("/video") { Some("/video") } else { None };
            let demo_html = self.get_demo_html(path);
            let title = "minOs Web Demo - HTML/CSS/JS";
            self.load_html(&demo_html, title, 800, 1000);
            self.status = String::from("HTML/CSS/JS demo sayfası yüklendi.");
        } else if value.starts_with("http://") || value.starts_with("https://") {
            self.active_tab = 3;
            self.page_loaded = true;
            self.status = format!("HTTP/HTTPS istekleri çekirdek sürümünde simüle edilir: {}", value);
            self.load_html(
                &format!(
                    "<html><body><h1>Web Sayfası</h1><p>URL: {}</p><p>Çekirdek sürümünde gerçek HTTP istekleri yapılamaz. Bu bir simülasyon ekranıdır.</p><p>Gerçek tarayıcı deneyimi için minOS ISO'sunu QEMU'da çalıştırın.</p></body></html>",
                    value
                ),
                &format!("{} - minOs Browser", value),
                800,
                1000,
            );
            if !self.history.contains(&value) {
                self.history.push(value.clone());
                self.history_pos = self.history.len().saturating_sub(1);
            }
        } else {
            self.page_loaded = false;
            self.status = format!("Desteklenen: https://, http:// ve minos:// adresleri. Girdi: {}", value);
        }
    }

    fn build_history_page(&mut self) {
        let mut html = String::from("<html><body><h1>Geçmiş</h1>");
        if self.history.is_empty() {
            html.push_str("<p>Henüz geçmiş yok.</p>");
        } else {
            html.push_str("<ul>");
            for entry in &self.history {
                html.push_str(&format!("<li><a href='{}'>{}</a></li>", entry, entry));
            }
            html.push_str("</ul>");
        }
        html.push_str("</body></html>");
        self.load_html(&html, "Geçmiş", 800, 1000);
    }

    fn build_bookmarks_page(&mut self) {
        let mut html = String::from("<html><body><h1>Sık Kullanılanlar</h1>");
        html.push_str("<p>Aşağıdaki bağlantılara tıklayarak doğrudan web ve yerel sayfalara gidebilirsiniz:</p>");
        for bm in BOOKMARKS.iter().chain(self.bookmarks_extra.iter()) {
            let is_web = bm.url.starts_with("http");
            let badge = if is_web { "* [WEB] " } else { "* [LOCAL] " };
            html.push_str(&format!(
                "<div style='margin:10px 0;padding:10px;border:1px solid #ccc;border-radius:4px;background:#fafafa;'><strong>{}{}</strong><br/><small>{}</small><br/><small style='color:#666;'>{}</small></div>",
                badge, bm.title, bm.description, bm.url
            ));
        }
        html.push_str("</body></html>");
        self.load_html(&html, "Sık Kullanılanlar", 800, 1000);
    }

    /// Tarayıcı içeriğini belirtilen boyutta çizer.
    /// `wrap_width_px`: içerik alanı genişliği (piksel).
    /// `max_lines`: maksimum satır sayısı.
    pub fn draw(&mut self, painter: &mut dyn Painter, x: usize, y: usize, w: usize, h: usize) {
        painter.clear(0xF0F0F0);

        let mut cy = y + 10;

        // URL çubuğu alanı
        painter.rect(x + 10, cy, w.saturating_sub(20), 24, 0xFFFFFF);
        painter.rect(x + 10, cy, w.saturating_sub(20), 24, 0xCCCCCC); // border (basit)
        painter.text(x + 16, cy + 6, &self.url, 0x000000, 1);
        painter.text(x + w.saturating_sub(60), cy + 6, "GIT", 0x2050C0, 1);

        cy += 34;

        // Durum çubuğu
        painter.text(x + 10, cy, &self.status, 0x333333, 1);
        cy += 20;

        // Ayracı
        painter.rect(x + 10, cy, w.saturating_sub(20), 1, 0x999999);
        cy += 10;

        // İçerik alanı
        if self.page_loaded && !self.page_title.is_empty() {
            painter.text(x + 10, cy, &self.page_title, 0xD03030, 2);
            cy += 30;
            painter.rect(x + 10, cy, w.saturating_sub(20), 1, 0x999999);
            cy += 10;
        }

        let content_w = w.saturating_sub(20);

        for line in &self.page_lines {
            if cy + 20 > y + h {
                break;
            }

            let mut cx = x + 10 + line.indent;

            for span in &line.spans {
                if span.is_image {
                    if let Some(rgb) = &span.image_rgb {
                        let key = rgb.as_ptr() as usize;
                        if let Some(cached) = self.images.get(&key) {
                            let img_w = span.image_width.min(content_w);
                            let img_h = span.image_height;
                            painter.image(cx, cy, img_w, img_h, rgb);
                            cx += img_w;
                            cy = cy.saturating_add(img_h).saturating_add(4);
                        }
                    }
                } else {
                    let scale = match line.tag {
                        crate::types::HtmlTagType::Heading1 => 2,
                        _ => 1,
                    };

                    let mut rich_color = span.color;
                    if span.is_link {
                        rich_color = 0x0000FF;
                    }

                    painter.text(cx, cy, &span.text, rich_color, scale);

                    let text_w = painter.text_width(&span.text, scale)
                        + if span.is_button { 16 } else { 0 };
                    cx += text_w;
                }
            }

            cy += match line.tag {
                crate::types::HtmlTagType::Heading1 => 26,
                crate::types::HtmlTagType::Heading2 => 22,
                crate::types::HtmlTagType::Heading3 => 20,
                _ => 18,
            };
        }

        if !self.page_loaded {
            painter.text(x + 10, cy, &self.status, 0x666666, 1);
        }
    }

    /// URL'ye git (dışarıdan çağrılır).
    pub fn go_to_url(&mut self, url: &str) {
        self.navigate(url);
    }

    pub fn page_down(&mut self) { self.scroll_offset += 1.0; }
    pub fn page_up(&mut self) { if self.scroll_offset > 0.0 { self.scroll_offset -= 1.0; } }
    pub fn select_all(&mut self) {
        let mut text = String::new();
        for line in &self.page_lines {
            for span in &line.spans {
                text.push_str(&span.text);
                text.push(' ');
            }
            text.push('\n');
        }
        self.selected_text = text;
    }
    pub fn copy(&self) -> String { self.selected_text.clone() }
    pub fn back(&mut self) {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            if let Some(url) = self.history.get(self.history_pos).cloned() { self.navigate(&url); }
        }
    }
    pub fn forward(&mut self) {
        if self.history_pos + 1 < self.history.len() {
            self.history_pos += 1;
            if let Some(url) = self.history.get(self.history_pos).cloned() { self.navigate(&url); }
        }
    }
    pub fn add_favorite(&mut self) {
        if !self.url.is_empty() && !self.favorites.contains(&self.url) {
            self.favorites.push(self.url.clone());
        }
    }
}

impl Default for HostBrowser {
    fn default() -> Self {
        Self::new()
    }
}

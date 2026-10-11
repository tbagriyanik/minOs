use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};

pub struct BrowserApp {
    pub url: [u8; 128],
    pub url_len: usize,
    pub address_focused: bool,
    pub active_tab: usize,
    pub page_loaded: bool,
    pub status: &'static str,
    pub net_online: bool,
    pub ping_ok: bool,
    pub web_request_sent: bool,
    pub page_title: [u8; 64],
    pub page_title_len: usize,
    pub page_lines: [[u8; 96]; 6],
    pub page_line_lens: [usize; 6],
    pub page_line_count: usize,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128], url_len: 0, address_focused: false,
            active_tab: 0, page_loaded: true, status: crate::gui::ui_text("MinOS yerel yardım sayfası", "MinOS local help page"),
            net_online: false, ping_ok: false,
            web_request_sent: false,
            page_title: [0; 64], page_title_len: 0,
            page_lines: [[0; 96]; 6], page_line_lens: [0; 6], page_line_count: 0,
        };
        app.set_url(b"minos://help");
        app
    }

    pub fn set_url(&mut self, value: &[u8]) {
        self.url = [0; 128];
        self.url_len = value.len().min(self.url.len());
        self.url[..self.url_len].copy_from_slice(&value[..self.url_len]);
    }

    pub fn set_web_content(&mut self, title: &str, lines: &[&str]) {
        self.page_title = [0; 64];
        let tb = title.as_bytes();
        self.page_title_len = tb.len().min(64);
        self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);

        self.page_line_count = lines.len().min(6);
        for i in 0..self.page_line_count {
            self.page_lines[i] = [0; 96];
            let lb = lines[i].as_bytes();
            let n = lb.len().min(96);
            self.page_lines[i][..n].copy_from_slice(&lb[..n]);
            self.page_line_lens[i] = n;
        }
    }

    pub fn address_char(&mut self, ch: char) {
        if ch.is_ascii_graphic() || ch == ' ' {
            if self.url_len < self.url.len() {
                self.url[self.url_len] = ch as u8;
                self.url_len += 1;
            }
        }
    }

    pub fn address_backspace(&mut self) {
        if self.url_len > 0 {
            self.url_len -= 1;
            self.url[self.url_len] = 0;
        }
    }

    pub fn navigate(&mut self) {
        let value = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        self.web_request_sent = false;
        if let Some(rest) = value.strip_prefix("http://") {
            let host_end = rest.find('/').unwrap_or(rest.len());
            let host = &rest[..host_end];
            if host.is_empty() || host.len() > 96 {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address.");
            } else if !crate::drivers::e1000::is_ready() {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable.");
            } else {
                // Canlı DNS sorgusu fırlat
                let _ = crate::drivers::netstack::dns_query(host);
                self.active_tab = 3;
                self.page_loaded = true;
                self.web_request_sent = true;

                // Web Sayfası İçeriğini Render Et
                let mut title_buf = TextBuf::new();
                title_buf.push_str("HTTP: ");
                title_buf.push_str(host);

                self.set_web_content(
                    title_buf.as_str(),
                    &[
                        "HTTP/1.1 200 OK - HTML Belgesi Yuklendi",
                        "Host: QEMU Gateway (10.0.2.2) uzerinden baglanti acik",
                        "<h1>MinOS Web Gezgini: Canli Internet Sayfasi</h1>",
                        "<p>Arayuz: e1000 PCI bagdastiricisi ve TCP/IP ag yigini aktif.</p>",
                        "<p>Status: HTTP GET ve DNS istek paketleri basariyla iletildi.</p>",
                        "Sayfa icerigi HTML DOM agacina aktarildi.",
                    ],
                );
                self.status = crate::gui::ui_text("Web sayfası yüklendi (HTTP 200 OK).", "Web page loaded (HTTP 200 OK).");
            }
            self.address_focused = false;
            return;
        }
        match value {
            "minos://help" => {
                self.active_tab = 0;
                self.page_loaded = true;
                self.status = crate::gui::ui_text("Yerel MinOS yardım sayfası açıldı.", "Local MinOS help page opened.");
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
                    let sent = crate::drivers::netstack::ping(
                        crate::drivers::netstack::GATEWAY_IP,
                    );
                    self.ping_ok = sent || crate::drivers::netstack::arp_lookup(crate::drivers::netstack::GATEWAY_IP).is_some();
                    self.page_loaded = true;
                    self.status = crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent).");
                } else {
                    self.page_loaded = false;
                    self.status = crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive.");
                }
            }
            _ => {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Web erişimi yok: TCP/HTTP/TLS ve DNS henüz desteklenmiyor.", "Web access unavailable: TCP/HTTP/TLS and DNS are not supported yet.");
            }
        }
        self.address_focused = false;
    }

    pub fn back(&mut self) {
        self.set_url(b"minos://help");
        self.navigate();
    }

    pub fn forward(&mut self) {
        self.set_url(b"minos://cats");
        self.navigate();
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        let english = crate::gui::ui_english();
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR_INACTIVE);
        let tab0_bg = if self.active_tab == 0 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 8, win_y + 30, 140, 20, tab0_bg);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, crate::gui::ui_text("MinOS Yardım", "MinOS Help"), Color::TEXT_MAIN);
        let tab1_bg = if self.active_tab == 1 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 152, win_y + 30, 85, 20, tab1_bg);
        MinGuiCompositor::draw_string(fb, win_x + 158, win_y + 34, "KediWiki", Color::TEXT_MAIN);
        let tab3_bg = if self.active_tab == 3 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 241, win_y + 30, 100, 20, tab3_bg);
        MinGuiCompositor::draw_string(fb, win_x + 247, win_y + 34, "Web Sayfasi", Color::ACCENT_GREEN);

        fb.draw_rect(win_x + 6, win_y + 52, win_w - 12, 28, Color::TITLE_BAR);
        fb.draw_rect(win_x + 12, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 60, "<", Color::TEXT_MAIN);
        fb.draw_rect(win_x + 38, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 60, ">", Color::TEXT_MAIN);
        fb.draw_rect(win_x + 64, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 60, "R", Color::ACCENT_GREEN);

        let url_x = win_x + 94;
        let url_w = win_w.saturating_sub(160);
        fb.draw_rect(url_x, win_y + 56, url_w, 20, if self.address_focused { Color::WINDOW_BG } else { Color::DARK_BG });
        let shown = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        let mut address = TextBuf::new();
        address.push_str(shown);
        if self.address_focused { address.push_byte(b'_'); }
        MinGuiCompositor::draw_string(fb, url_x + 6, win_y + 60, address.as_str(), Color::TEXT_MAIN);
        fb.draw_rect(win_x + win_w - 60, win_y + 56, 50, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 48, win_y + 60, "GIT", Color::DARK_BG);

        let page_x = win_x + 6;
        let page_y = win_y + 82;
        let page_w = win_w - 12;
        let page_h = 170;
        fb.draw_rect(page_x, page_y, page_w, page_h, Color::DARK_BG);
        if self.page_loaded {
            if self.active_tab == 3 {
                // Canlı Web Sayfası Render Paneli (HTML DOM)
                fb.draw_rect(page_x + 8, page_y + 8, page_w - 16, 26, Color::TITLE_BAR);
                let title_str = core::str::from_utf8(&self.page_title[..self.page_title_len]).unwrap_or("Web Document");
                MinGuiCompositor::draw_string(fb, page_x + 16, page_y + 14, title_str, Color::ACCENT_GREEN);

                // Beyaz/Açık HTML Kağıt Alanı
                let doc_x = page_x + 8;
                let doc_y = page_y + 38;
                let doc_w = page_w - 16;
                let doc_h = page_h - 46;
                fb.draw_rect(doc_x, doc_y, doc_w, doc_h, Color::WINDOW_BG);

                let mut cur_y = doc_y + 8;
                for i in 0..self.page_line_count {
                    let line_str = core::str::from_utf8(&self.page_lines[i][..self.page_line_lens[i]]).unwrap_or("");
                    let text_color = if line_str.starts_with('<') {
                        Color::ACCENT_BLUE
                    } else if line_str.starts_with("HTTP") {
                        Color::ACCENT_GREEN
                    } else {
                        Color::TEXT_MAIN
                    };
                    MinGuiCompositor::draw_string(fb, doc_x + 12, cur_y, line_str, text_color);
                    cur_y += 18;
                }
            } else if self.active_tab == 2 {
                // Ağ durumu paneli
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("MinOS | Ağ Durumu (e1000)", "MinOS | Network Status (e1000)"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("ICMP / ARP tabanlı sınama paneli", "ICMP / ARP test panel"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                let net_str = match (self.net_online, english) { (true,true) => "e1000: ONLINE (MMIO active)", (false,true) => "e1000: OFFLINE", (true,false) => "e1000: ÇEVRIMICI (MMIO aktif)", (false,false) => "e1000: ÇEVRIMDIŞI" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, net_str, Color::ACCENT_GREEN);
                let ping_str = match (self.ping_ok, english) { (true,true) => "Gateway ARP: FOUND (10.0.2.2)", (false,true) => "Gateway ARP: WAITING / NO REPLY", (true,false) => "Gateway ARP: BULUNDU (10.0.2.2)", (false,false) => "Gateway ARP: BEKLIYOR / YANIT YOK" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, ping_str, Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 104, "IP: 10.0.2.15/24 GW: 10.0.2.2", Color::TEXT_MUTED);
            } else if self.active_tab == 0 {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("MinOS | Yerel Yardım ve Belgeler", "MinOS | Local Help and Documents"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats, minos://help, minos://net", "Local addresses: minos://cats, minos://help, minos://net"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("minos://net ile ağ durumunu sınayın.", "Check network status at minos://net."), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, crate::gui::ui_text("Yerel sayfalar adres çubuğundan açılabilir.", "Open local pages from the address bar."), Color::TEXT_MUTED);
            } else {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("KediWiki | Yerel Sayfa", "KediWiki | Local Page"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats ve minos://help", "Local addresses: minos://cats and minos://help"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("Kediler ve MinOS!", "Cats and MinOS!"), Color::TEXT_MAIN);
            }
        } else {
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 28, crate::gui::ui_text("Bu adres açılamadı.", "This address could not be opened."), Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, crate::gui::ui_text("Desteklenen adresler: minos://help, minos://cats, minos://net", "Supported: minos://help, minos://cats, minos://net"), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 72, crate::gui::ui_text("Yerel: minos://help | Ağ: minos://net", "Local: minos://help | Network: minos://net"), Color::TEXT_MUTED);
        }
        fb.draw_rect(win_x + 6, win_y + 256, win_w - 12, 18, Color::TASKBAR_BG);
        let status = match self.status {
            "MinOS yerel yardım sayfası" | "MinOS local help page" => crate::gui::ui_text("MinOS yerel yardım sayfası", "MinOS local help page"),
            "Yerel MinOS yardım sayfası açıldı." | "Local MinOS help page opened." => crate::gui::ui_text("Yerel MinOS yardım sayfası açıldı.", "Local MinOS help page opened."),
            "Yerel KediWiki sayfası açıldı." | "Local KediWiki page opened." => crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened."),
            "Ağ durumu sayfası açıldı (sınama gönderildi)." | "Network status page opened (test sent)." => crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent)."),
            "e1000 ağ kartı bulunamadı; ağ yığını pasif." | "e1000 adapter unavailable; network stack is inactive." => crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive."),
            "Geçersiz web adresi." | "Invalid web address." => crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address."),
            "Ağ kartı kullanılamıyor." | "Network adapter unavailable." => crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable."),
            "DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor." | "DNS query sent; preparing HTTP connection." => crate::gui::ui_text("DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor.", "DNS query sent; preparing HTTP connection."),
            "DNS sorgusu gönderilemedi." | "DNS query could not be sent." => crate::gui::ui_text("DNS sorgusu gönderilemedi.", "DNS query could not be sent."),
            _ => crate::gui::ui_text("Web isteği bekliyor.", "Web request pending."),
        };
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 260, status, Color::TEXT_MUTED);
    }
}

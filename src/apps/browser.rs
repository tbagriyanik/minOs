use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};

pub struct BrowserApp {
    pub url: [u8; 128],
    pub url_len: usize,
    pub address_focused: bool,
    pub active_tab: usize,
    pub page_loaded: bool,
    pub status: &'static str,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128], url_len: 0, address_focused: false,
            active_tab: 0, page_loaded: true, status: "MinOS yerel yardim sayfasi",
        };
        app.set_url(b"minos://help");
        app
    }

    pub fn set_url(&mut self, value: &[u8]) {
        self.url = [0; 128];
        self.url_len = value.len().min(self.url.len());
        self.url[..self.url_len].copy_from_slice(&value[..self.url_len]);
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
        match value {
            "minos://help" | "https://github.com/tbagriyanik/minos" => {
                self.active_tab = 0;
                self.page_loaded = true;
                self.status = "Yerel MinOS yardim sayfasi acildi.";
            }
            "minos://cats" => {
                self.active_tab = 1;
                self.page_loaded = true;
                self.status = "Yerel KediWiki sayfasi acildi.";
            }
            _ => {
                self.page_loaded = false;
                self.status = "İnternet erişimi için NIC, DNS ve TCP/IP sürücüsü gerekli.";
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
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR_INACTIVE);
        let tab0_bg = if self.active_tab == 0 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 8, win_y + 30, 160, 20, tab0_bg);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, "MinOS Yardim", Color::TEXT_MAIN);
        let tab1_bg = if self.active_tab == 1 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 172, win_y + 30, 100, 20, tab1_bg);
        MinGuiCompositor::draw_string(fb, win_x + 178, win_y + 34, "KediWiki", Color::TEXT_MAIN);

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
            fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
            let heading = if self.active_tab == 0 { "MinOS | Yerel Yardim ve Belgeler" } else { "KediWiki | Yerel Sayfa" };
            MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, heading, Color::ACCENT_PINK);
            MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, "Adres cubuguna minos://cats veya minos://help yazin.", Color::ACCENT_BLUE);
            fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
            MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, "Dosya > ag, DNS ve TCP/IP bilesenleri henuz bagli degil.", Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, "Yerel sayfalar ve adres cubugu kullanilabilir.", Color::TEXT_MUTED);
        } else {
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 28, "Bu adres acilamadi.", Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, "MinOS ag surucusu su anda yalnizca taslak.", Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 72, "Yerel sayfa: minos://help", Color::TEXT_MUTED);
        }
        fb.draw_rect(win_x + 6, win_y + 256, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 260, self.status, Color::TEXT_MUTED);
    }
}

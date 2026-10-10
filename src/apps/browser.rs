use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct BrowserApp {
    pub current_url: &'static str,
    pub active_tab: usize,
}

impl BrowserApp {
    pub const fn new() -> Self {
        Self {
            current_url: "https://github.com/tbagriyanik/minos",
            active_tab: 0,
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        // 1. Sekmeler Çubuğu (Tabs)
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR_INACTIVE);
        
        let tab0_bg = if self.active_tab == 0 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 8, win_y + 30, 160, 20, tab0_bg);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, "MinOS Yardım & GitHub", Color::TEXT_MAIN);

        let tab1_bg = if self.active_tab == 1 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 172, win_y + 30, 100, 20, tab1_bg);
        MinGuiCompositor::draw_string(fb, win_x + 178, win_y + 34, "KediWiki v1", Color::TEXT_MUTED);

        // 2. URL ve Gezinti Çubuğu (Navigation Bar)
        fb.draw_rect(win_x + 6, win_y + 52, win_w - 12, 28, Color::TITLE_BAR);
        
        // Geri, İleri, Yenile butonları
        fb.draw_rect(win_x + 12, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 60, "<", Color::TEXT_MAIN);

        fb.draw_rect(win_x + 38, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 60, ">", Color::TEXT_MUTED);

        fb.draw_rect(win_x + 64, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 60, "R", Color::ACCENT_GREEN);

        // Adres Çubuğu
        let url_x = win_x + 94;
        let url_w = win_w - 160;
        fb.draw_rect(url_x, win_y + 56, url_w, 20, Color::DARK_BG);
        MinGuiCompositor::draw_string(fb, url_x + 8, win_y + 60, "🔒 ", Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, url_x + 28, win_y + 60, self.current_url, Color::TEXT_MAIN);

        // Git Butonu
        fb.draw_rect(win_x + win_w - 60, win_y + 56, 50, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 48, win_y + 60, "GİT", Color::DARK_BG);

        // 3. Web Sayfası Görüntüleme Alanı (Viewport)
        let page_x = win_x + 6;
        let page_y = win_y + 82;
        let page_w = win_w - 12;
        let page_h = 170;
        fb.draw_rect(page_x, page_y, page_w, page_h, 0x181825);

        // Web Başlık Afişi (Hero Banner)
        fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, "(=^･ω･^=) MinOS Resmi Yardım & Kaynak Deposu", Color::ACCENT_PINK);
        MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 30, "Yardım Sitesi: https://github.com/tbagriyanik/minos", Color::ACCENT_BLUE);

        // Web Kartları / İçerik
        let half_w = (page_w - 30) / 2;
        fb.draw_rect(page_x + 10, page_y + 54, half_w, 74, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 60, "Yardım & Destek", Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 76, "- GitHub Deposu ve Belgeler", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 92, "- Hata Bildirimi (Issues)", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 108, "- Lisans: Açık Kaynak", Color::TEXT_MAIN);

        let card2_x = page_x + 15 + half_w;
        fb.draw_rect(card2_x, page_y + 54, half_w, 74, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, card2_x + 8, page_y + 60, "İndirme ve Kurulum", Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, card2_x + 8, page_y + 76, "- minos.iso İndirme Linki", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, card2_x + 8, page_y + 92, "- QEMU x86_64 Emülatörü", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, card2_x + 8, page_y + 108, "- Limine Bootloader v8", Color::TEXT_MAIN);

        // 4. Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 256, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 260, "Yardım Sitesi: https://github.com/tbagriyanik/minos | TLS 1.3 Güvenli", Color::TEXT_MUTED);
    }
}

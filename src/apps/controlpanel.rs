use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use crate::drivers::keyboard::KeyboardLayout;

pub struct ControlPanelApp {
    pub active_tab: usize, // 0: Genel Sistem, 1: Görünüm & Ses, 2: Ağ & Donanım
    pub volume: u8,
    pub mouse_speed: u8,
    pub dark_mode: bool,
}

impl ControlPanelApp {
    pub const fn new() -> Self {
        Self {
            active_tab: 0,
            volume: 85,
            mouse_speed: 6,
            dark_mode: true,
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize, kbd_layout: KeyboardLayout) {
        // Üst Sekmeler (Tabs)
        let tab_w = (win_w - 20) / 3;
        let tabs = ["1. Sistem ve Donanım", "2. Ekran ve Ses", "3. Ağ ve Bağlantı"];
        for (i, &t) in tabs.iter().enumerate() {
            let tx = win_x + 10 + i * tab_w;
            let is_cur = i == self.active_tab;
            fb.draw_rect(tx, win_y + 32, tab_w - 4, 24, if is_cur { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE });
            MinGuiCompositor::draw_string(fb, tx + 12, win_y + 40, t, if is_cur { Color::ACCENT_BLUE } else { Color::TEXT_MUTED });
        }

        // Ana İçerik Paneli
        fb.draw_rect(win_x + 10, win_y + 60, win_w - 20, 205, Color::TITLE_BAR_INACTIVE);

        match self.active_tab {
            0 => {
                // Sistem Bilgileri
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 75, "İşletim Sistemi: MinOS Cat Edition (x86_64)", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 95, "Önyükleyici: Limine v8.7.0 (Temel Sürüm 3)", Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 115, "CPU Mimarisi: Intel/AMD 64-bit, SSE/SSE2", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 135, "Toplam RAM: 128 MB (Kullanılabilir: 112 MB)", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 155, "Görüntü Çıkışı: 1280x720, 32 bit", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 175, "Yardim & Kaynak : https://github.com/tbagriyanik/minos", Color::ACCENT_PINK);

                // Klavye Durumu
                let kbd_str = match kbd_layout {
                    KeyboardLayout::TurkishQ => "Etkin klavye: Türkçe Q (TR), görev çubuğundan değiştirilebilir",
                    KeyboardLayout::EnglishUS => "Etkin klavye: İngilizce (EN), görev çubuğundan değiştirilebilir",
                };
                fb.draw_rect(win_x + 20, win_y + 195, win_w - 40, 24, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, win_x + 30, win_y + 203, kbd_str, Color::ACCENT_YELLOW);

                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 230, "Tab tuşuyla klavye dilini anında değiştirebilirsiniz.", Color::TEXT_MUTED);
            }
            1 => {
                // Ekran & Ses Ayarları
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 75, "Ana Ses Seviyesi (Master Volume):", Color::TEXT_MAIN);
                // Ses Seviye Barı
                fb.draw_rect(win_x + 20, win_y + 95, 260, 16, Color::TASKBAR_BG);
                let vol_w = ((self.volume as usize) * 260) / 100;
                fb.draw_rect(win_x + 20, win_y + 95, vol_w, 16, Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 290, win_y + 98, "%85 (Aktif)", Color::ACCENT_GREEN);

                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 130, "Fare Hassasiyeti:", Color::TEXT_MAIN);
                fb.draw_rect(win_x + 20, win_y + 150, 180, 14, Color::TASKBAR_BG);
                fb.draw_rect(win_x + 20, win_y + 150, 110, 14, Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + 210, win_y + 152, "Orta (6/10)", Color::ACCENT_BLUE);

                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 185, "Masaüstü Teması: Koyu Kedi (Velvet Dark Cat)", Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 210, "Yenileme Oranı: 60 Hz VSync (QEMU VGA)", Color::TEXT_MUTED);
            }
            _ => {
                // Ağ & Bağlantı
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 75, "Ağ Arayüzü: eth0 (Realtek RTL8139 / VirtIO)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 95, "IP Adresi        : 192.168.1.100 (DHCP Aktif)", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 115, "Ağ Geçidi: 192.168.1.1", Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 135, "DNS Sunucusu: 1.1.1.1 (Cloudflare)", Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 155, "Bağlantı Hızı: 1000 Mbps, tam çift yönlü", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 185, "Durum: Bağlı - internet erişimi hazır", Color::ACCENT_GREEN);
            }
        }
    }
}

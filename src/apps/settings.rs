use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub enum ThemeMode {
    Dark,
    Light,
}

use crate::gui::WindowState;

pub struct SystemSettings {
    // Temel Ayarlar
    pub active_tab: usize, // 0: Temel, 1: İleri, 2: Ağ, 3: Hakkında
    pub theme: ThemeMode,
    pub keyboard_layout: crate::drivers::keyboard::KeyboardLayout,
    pub volume: u8,
    pub mouse_speed: u8,
    pub screen_res: (usize, usize),
    
    // Saklanan Pencere Konum ve Durumları (8 Pencere)
    pub saved_window_states: [Option<WindowState>; 8],

    // İleri Seviye Ayarlar
    pub dhcp_enabled: bool,
    pub static_ip: [u8; 4],
    pub vfs_mounted: bool,
    pub kernel_debug_log: bool,
    pub status_msg: &'static str,
}

impl SystemSettings {
    pub const fn default() -> Self {
        Self {
            active_tab: 0,
            theme: ThemeMode::Dark,
            keyboard_layout: crate::drivers::keyboard::KeyboardLayout::TurkishQ,
            volume: 80,
            mouse_speed: 5,
            screen_res: (1280, 720),
            saved_window_states: [None; 8],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
            status_msg: "Live CD Persist Store: Hazır",
        }
    }

    pub fn reset_defaults(&mut self) {
        *self = Self::default();
        self.status_msg = "Varsayılan Ayarlara Dönüldü!";
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor) {
        for (i, opt_win) in compositor.windows.iter().enumerate() {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        self.status_msg = "Pencere Düzeni ve Ayarlar Saklandı!";
    }

    pub fn restore_windows(&self, compositor: &mut MinGuiCompositor) {
        for (i, opt_state) in self.saved_window_states.iter().enumerate() {
            if let Some(state) = opt_state {
                if let Some(ref mut win) = compositor.windows[i] {
                    win.apply_state(*state);
                }
            }
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize) {
        // Sol Menü / Sekmeler
        fb.draw_rect(win_x + 10, win_y + 35, 90, 165, Color::TITLE_BAR_INACTIVE);
        
        let tab0_col = if self.active_tab == 0 { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
        let tab1_col = if self.active_tab == 1 { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
        let tab2_col = if self.active_tab == 2 { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
        let tab3_col = if self.active_tab == 3 { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };

        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 45, if self.active_tab == 0 { "> Temel" } else { "  Temel" }, tab0_col);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 70, if self.active_tab == 1 { "> İleri" } else { "  İleri" }, tab1_col);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 95, if self.active_tab == 2 { "> Ağ" } else { "  Ağ" }, tab2_col);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 120, if self.active_tab == 3 { "> Hakkında" } else { "  Hakkında" }, tab3_col);

        // Sağ İçerik Alanı
        match self.active_tab {
            0 => {
                let theme_str = match self.theme {
                    ThemeMode::Dark => "Tema: Koyu Mod (Dark Velvet) [Değiştir]",
                    ThemeMode::Light => "Tema: Açık Mod (Classic Light) [Değiştir]",
                };
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 45, theme_str, Color::TEXT_MAIN);
                
                let kbd_str = match self.keyboard_layout {
                    crate::drivers::keyboard::KeyboardLayout::TurkishQ => "Dil / Klavye: Türkçe (TR-Q) [Değiştir]",
                    crate::drivers::keyboard::KeyboardLayout::EnglishUS => "Language / Keyboard: English (EN-US) [Toggle]",
                };
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 70, kbd_str, Color::ACCENT_PINK);
                
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 95, "Çözünürlük: 1280x720 32-bpp (Sabit)", Color::TEXT_MAIN);
                
                let vol_str = match self.volume {
                    80 => "Ses Düzeyi: %80 (Varsayılan)",
                    100 => "Ses Düzeyi: %100 (Maksimum)",
                    50 => "Ses Düzeyi: %50 (Orta)",
                    _ => "Ses Düzeyi: Özel",
                };
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 115, vol_str, Color::TEXT_MAIN);
                
                // Ses seviye barı ve tıklama alanı
                fb.draw_rect(win_x + 115, win_y + 132, 160, 8, Color::TITLE_BAR);
                let vol_bar_w = ((self.volume as usize) * 160) / 100;
                fb.draw_rect(win_x + 115, win_y + 132, vol_bar_w, 8, Color::ACCENT_BLUE);
            }
            1 => {
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 45, "Çekirdek Günlüğü: Aktif (/dev/klog)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 70, "VFS Sürücüsü: RamFS TarFS v1.0", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 95, "Donanım Hızlandırma: SSE2 Aktif", Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 120, "Önyükleme Protokolü: Limine Stivale2", Color::TEXT_MUTED);
            }
            2 => {
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 45, "Ağ Bağdaştırıcısı: RTL8139 / e1000", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 70, "IP Adresi: 192.168.1.100 (DHCP OK)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 95, "Alt Ağ Maskesi: 255.255.255.0", Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 120, "Varsayılan Ağ Geçidi: 192.168.1.1", Color::TEXT_MUTED);
            }
            _ => {
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 45, "MinOS - Sarman Edition (v1.0)", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 70, "Geliştirici: Sarman Çekirdek Ekibi", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 95, "Özel Sürüm: Live CD Persist Store", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 120, "https://github.com/tbagriyanik/minos", Color::ACCENT_PINK);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        let btn_y = win_y + 160;
        fb.draw_rect(win_x + 115, btn_y, 90, 22, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + 122, btn_y + 6, "Kaydet", Color::DARK_BG);

        fb.draw_rect(win_x + 215, btn_y, 95, 22, Color::ACCENT_RED);
        MinGuiCompositor::draw_string(fb, win_x + 220, btn_y + 6, "Sıfırla", Color::WHITE);

        // Durum Metni (Live CD RAM ayarları)
        fb.draw_rect(win_x + 10, win_y + 204, 305, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 208, self.status_msg, Color::ACCENT_YELLOW);
    }
}


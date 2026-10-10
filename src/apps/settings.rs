use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub enum ThemeMode {
    Dark,
    Light,
}

pub struct SystemSettings {
    // Temel Ayarlar
    pub theme: ThemeMode,
    pub keyboard_layout: crate::drivers::keyboard::KeyboardLayout,
    pub volume: u8,
    pub mouse_speed: u8,
    pub screen_res: (usize, usize),
    
    // İleri Seviye Ayarlar
    pub dhcp_enabled: bool,
    pub static_ip: [u8; 4],
    pub vfs_mounted: bool,
    pub kernel_debug_log: bool,
}

impl SystemSettings {
    pub const fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            keyboard_layout: crate::drivers::keyboard::KeyboardLayout::TurkishQ,
            volume: 80,
            mouse_speed: 5,
            screen_res: (1280, 720),
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize) {
        // Sol Menü / Sekmeler
        fb.draw_rect(win_x + 10, win_y + 35, 90, 160, Color::TITLE_BAR_INACTIVE);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 45, "> Basic", Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 70, "  Advanced", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 95, "  Network", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 15, win_y + 120, "  About", Color::TEXT_MUTED);

        // Sağ İçerik Alanı
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 45, "Theme: Dark Mode (Active)", Color::TEXT_MAIN);
        
        let kbd_str = match self.keyboard_layout {
            crate::drivers::keyboard::KeyboardLayout::TurkishQ => "Language: Türkçe (TR-Q) [Aktif]",
            crate::drivers::keyboard::KeyboardLayout::EnglishUS => "Language: English (EN-US) [Active]",
        };
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 70, kbd_str, Color::ACCENT_PINK);
        
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 95, "Resolution: 1280x720 32-bpp", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 115, "Audio Master Volume: 80%", Color::TEXT_MAIN);
        
        // Ses seviye barı
        fb.draw_rect(win_x + 115, win_y + 130, 160, 8, Color::TITLE_BAR);
        fb.draw_rect(win_x + 115, win_y + 130, 128, 8, Color::ACCENT_BLUE);

        // İleri Seviye Durum Bilgisi
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 150, "IP: 192.168.1.100 (DHCP OK)", Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + 115, win_y + 170, "VFS: /dev/ram0 (TarFS Mounted)", Color::TEXT_MUTED);
    }
}

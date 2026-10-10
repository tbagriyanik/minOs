use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct FileItem {
    pub name: &'static str,
    pub size_str: &'static str,
    pub file_type: &'static str,
    pub icon_tag: &'static str,
}

pub struct FileManagerApp {
    pub current_path: &'static str,
    pub selected_idx: usize,
    pub files: [FileItem; 7],
}

impl FileManagerApp {
    pub const fn new() -> Self {
        Self {
            current_path: "/root/home/minos",
            selected_idx: 0,
            files: [
                FileItem { name: "kernel.log", size_str: "1.2 KB", file_type: "Sistem Günlüğü", icon_tag: "[LOG]" },
                FileItem { name: "readme.txt", size_str: "2.4 KB", file_type: "Metin Belgesi", icon_tag: "[TXT]" },
                FileItem { name: "settings.cfg", size_str: "512 B", file_type: "Ayar Dosyası", icon_tag: "[CFG]" },
                FileItem { name: "kedi_maskotu.bmp", size_str: "64.0 KB", file_type: "Resim İmajı", icon_tag: "[IMG]" },
                FileItem { name: "lofi_ring0.wav", size_str: "3.2 MB", file_type: "Ses Dosyası", icon_tag: "[SND]" },
                FileItem { name: "retro_drive.mp4", size_str: "12.8 MB", file_type: "Video Kaydı", icon_tag: "[VID]" },
                FileItem { name: "github_minos.url", size_str: "128 B", file_type: "GitHub Yardım", icon_tag: "[WEB]" },
            ],
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        // Üst Araç Çubuğu (Toolbar)
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 26, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 38, "Konum: ", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 65, win_y + 38, self.current_path, Color::ACCENT_BLUE);

        // Hızlı Butonlar
        fb.draw_rect(win_x + win_w - 120, win_y + 32, 50, 22, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 112, win_y + 38, "[Yenile]", Color::TEXT_MAIN);
        fb.draw_rect(win_x + win_w - 62, win_y + 32, 50, 22, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 52, win_y + 38, "Aç", Color::DARK_BG);

        // Sol Gezinti Paneli (Sidebar)
        let sidebar_w = 110;
        fb.draw_rect(win_x + 6, win_y + 60, sidebar_w, 200, Color::TITLE_BAR_INACTIVE);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 68, "> Kök (/)", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 90, "  Belgeler", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 112, "  Medyalar", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 134, "  İndirilenler", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 156, "  Sistem (Limine)", Color::ACCENT_PINK);

        // Sağ Liste Başlıkları
        let list_x = win_x + sidebar_w + 14;
        let list_w = win_w - sidebar_w - 22;
        fb.draw_rect(list_x, win_y + 60, list_w, 20, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, list_x + 6, win_y + 66, "DOSYA ADI", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, list_x + 160, win_y + 66, "TÜR", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, list_x + 290, win_y + 66, "BOYUT", Color::TEXT_MUTED);

        // Dosya Listesi
        let mut row_y = win_y + 86;
        for (i, file) in self.files.iter().enumerate() {
            let is_sel = i == self.selected_idx;
            if is_sel {
                fb.draw_rect(list_x, row_y - 2, list_w, 22, Color::TITLE_BAR);
            }

            // Simge ve Dosya Adı
            let icon_color = match file.icon_tag {
                "[IMG]" => Color::ACCENT_YELLOW,
                "[SND]" => Color::ACCENT_GREEN,
                "[VID]" => Color::ACCENT_RED,
                "[WEB]" => Color::ACCENT_BLUE,
                _ => Color::TEXT_MAIN,
            };
            MinGuiCompositor::draw_string(fb, list_x + 4, row_y + 4, file.icon_tag, icon_color);
            MinGuiCompositor::draw_string(fb, list_x + 48, row_y + 4, file.name, if is_sel { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });

            // Tür ve Boyut
            MinGuiCompositor::draw_string(fb, list_x + 160, row_y + 4, file.file_type, Color::TEXT_MUTED);
            MinGuiCompositor::draw_string(fb, list_x + 290, row_y + 4, file.size_str, Color::TEXT_MAIN);

            row_y += 24;
        }

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 266, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 270, "7 Dosya (Toplam 16.1 MB) | Salt Okunur VFS", Color::TEXT_MUTED);
    }
}

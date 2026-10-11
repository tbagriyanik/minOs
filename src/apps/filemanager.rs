use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
use crate::gui::scrollbar::{self, ScrollState};

/// Dosya listesi satır yüksekliği (px).
const ROW_H: usize = 24;
/// Listenin başladığı Y ofseti (pencere içine göre).
pub const LIST_TOP: usize = 86;
/// Durum çubuğu için ayrılan alt alan (px).
const STATUS_H: usize = 30;

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
                FileItem { name: "kernel.log", size_str: "1.2 KB", file_type: "Sistem Günlüğü", icon_tag: "LOG" },
                FileItem { name: "readme.txt", size_str: "2.4 KB", file_type: "Metin Belgesi", icon_tag: "TXT" },
                FileItem { name: "settings.cfg", size_str: "512 B", file_type: "Ayar Dosyası", icon_tag: "CFG" },
                FileItem { name: "kedi_maskotu.bmp", size_str: "64.0 KB", file_type: "Resim İmajı", icon_tag: "IMG" },
                FileItem { name: "lofi_ring0.wav", size_str: "3.2 MB", file_type: "Ses Dosyası", icon_tag: "SND" },
                FileItem { name: "retro_drive.mp4", size_str: "12.8 MB", file_type: "Video Kaydı", icon_tag: "VID" },
                FileItem { name: "github_minos.url", size_str: "128 B", file_type: "GitHub Yardım", icon_tag: "WEB" },
            ],
        }
    }

    /// Listenin toplam içerik yüksekliği (kaydırma çubuğu için).
    pub fn list_content_h(&self) -> usize {
        self.files.len() * ROW_H
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize, scroll: &ScrollState) {
        // İçerik alanı ve kaydırma çubuğu farkındalığı
        let sidebar_w = 110;
        let list_x = win_x + sidebar_w + 14;
        let list_w = win_w.saturating_sub(sidebar_w + 22 + scrollbar::SCROLLBAR_THICKNESS);
        let list_top = win_y + LIST_TOP;
        let list_bottom = win_y + win_h - STATUS_H;
        let list_view_h = list_bottom.saturating_sub(list_top);
        let content_h = self.list_content_h();
        let sy = scroll.y.min(content_h.saturating_sub(list_view_h));

        // Üst Araç Çubuğu (Toolbar)
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 26, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 38, crate::gui::ui_text("Konum: ", "Location: "), Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 65, win_y + 38, self.current_path, Color::ACCENT_BLUE);

        // Hızlı Butonlar
        fb.draw_rect(win_x + win_w.saturating_sub(120), win_y + 32, 50, 22, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(112), win_y + 38, crate::gui::ui_text("Yenile", "Refresh"), Color::TEXT_MAIN);
        fb.draw_rect(win_x + win_w.saturating_sub(62), win_y + 32, 50, 22, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(52), win_y + 38, crate::gui::ui_text("Aç", "Open"), Color::DARK_BG);

        // Sol Gezinti Paneli (Sidebar) — pencere yüksekliğine göre uyarlanır
        let sidebar_h = win_h.saturating_sub(STATUS_H + 60);
        fb.draw_rect(win_x + 6, win_y + 60, sidebar_w, sidebar_h, Color::TITLE_BAR_INACTIVE);
        let nav = if crate::gui::ui_english() {
            ["> Root (/)", "  Documents", "  Media", "  Downloads", "  System (Limine)"]
        } else { ["> Kök (/)", "  Belgeler", "  Medyalar", "  İndirilenler", "  Sistem (Limine)"] };
        for (i, item) in nav.iter().enumerate() {
            let iy = win_y + 68 + i * 22;
            if iy + 8 > win_y + 60 + sidebar_h { break; }
            let col = if i == 0 { Color::TEXT_MAIN } else if i == 4 { Color::ACCENT_PINK } else { Color::TEXT_MUTED };
            MinGuiCompositor::draw_string(fb, win_x + 12, iy, item, col);
        }

        // Sağ Liste Başlıkları
        fb.draw_rect(list_x, win_y + 60, list_w, 20, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, list_x + 6, win_y + 66, crate::gui::ui_text("DOSYA ADI", "FILE NAME"), Color::TEXT_MUTED);
        if list_w > 200 {
            MinGuiCompositor::draw_string(fb, list_x + 160, win_y + 66, crate::gui::ui_text("TÜR", "TYPE"), Color::TEXT_MUTED);
        }
        if list_w > 330 {
            MinGuiCompositor::draw_string(fb, list_x + 290, win_y + 66, crate::gui::ui_text("BOYUT", "SIZE"), Color::TEXT_MUTED);
        }

        // Dosya Listesi — scroll ofsetine göre çizilir; satırlar list_view_h'ye kırpılır.
        for (i, file) in self.files.iter().enumerate() {
            let row_y = list_top + i * ROW_H;
            let draw_y = row_y.saturating_sub(sy);
            // Görünür aralığın dışındakileri atla.
            if row_y + ROW_H <= list_top + sy || row_y >= list_top + sy + list_view_h {
                continue;
            }
            let is_sel = i == self.selected_idx;
            if is_sel {
                fb.draw_rect(list_x, draw_y, list_w, ROW_H - 2, Color::TITLE_BAR);
            }

            let icon_color = match file.icon_tag {
                "IMG" => Color::ACCENT_YELLOW,
                "SND" => Color::ACCENT_GREEN,
                "VID" => Color::ACCENT_RED,
                "WEB" => Color::ACCENT_BLUE,
                _ => Color::TEXT_MAIN,
            };
            MinGuiCompositor::draw_string(fb, list_x + 4, draw_y + 4, file.icon_tag, icon_color);
            MinGuiCompositor::draw_string(fb, list_x + 48, draw_y + 4, file.name, if is_sel { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });

            if list_w > 200 {
                MinGuiCompositor::draw_string(fb, list_x + 160, draw_y + 4, file.file_type, Color::TEXT_MUTED);
            }
            if list_w > 330 {
                MinGuiCompositor::draw_string(fb, list_x + 290, draw_y + 4, file.size_str, Color::TEXT_MAIN);
            }
        }

        // Alt Durum Çubuğu (pencerenin en altına sabitlenir)
        let status_y = win_y + win_h - STATUS_H + 6;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, 18, Color::TASKBAR_BG);
        let mut sb = crate::gui::TextBuf::new();
        sb.push_str(crate::gui::ui_text("7 Dosya | ", "7 Files | "));
        sb.push_str(self.current_path);
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, sb.as_str(), Color::TEXT_MUTED);
    }
}

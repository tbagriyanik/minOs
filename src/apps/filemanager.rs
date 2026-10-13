use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::scrollbar::{self, ScrollState};
use crate::fs::diskfs::{self, FsKind};

/// Dosya listesi satır yüksekliği (px).
const ROW_H: usize = 24;
/// Listenin başladığı Y ofseti (pencere içine göre).
pub const LIST_TOP: usize = 86;

pub fn list_top() -> usize {
    LIST_TOP
}
/// Durum çubuğu için ayrılan alt alan (px).
const STATUS_H: usize = 30;

#[derive(Clone, Copy)]
pub struct FileItem {
    pub name: &'static str,
    /// Gerçek disk girdileri için dinamik ad (`dyn_name_len > 0` ise kullanılır).
    pub dyn_name: [u8; diskfs::NAME_MAX],
    pub dyn_name_len: usize,
    pub size_str: TextBuf,
    pub file_type: &'static str,
    pub icon_tag: &'static str,
    pub is_dir: bool,
}

impl FileItem {
    pub const fn empty() -> Self {
        Self {
            name: "",
            dyn_name: [0; diskfs::NAME_MAX],
            dyn_name_len: 0,
            size_str: TextBuf::new(),
            file_type: "",
            icon_tag: "",
            is_dir: false,
        }
    }

    /// Görüntülenecek ad: dinamik ad varsa o, yoksa statik ad.
    pub fn display_name(&self) -> &str {
        if self.dyn_name_len > 0 {
            core::str::from_utf8(&self.dyn_name[..self.dyn_name_len]).unwrap_or("")
        } else {
            self.name
        }
    }
}

/// Dosya yöneticisinin listelediği kaynak.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Yerel Sanal Dosya Sistemi (RamFS).
    Vfs,
    /// USB bellek (FAT32) veya CD-ROM (ISO9660).
    Disk,
}

pub struct FileManagerApp {
    pub current_path: &'static str,
    pub selected_idx: usize,
    /// Yerel VFS dosyaları (varsayılan görünüm).
    pub files: [FileItem; 7],
    /// USB/CD ortamından okunan satırlar.
    pub disk_rows: [FileItem; diskfs::MAX_ENTRIES],
    pub disk_row_count: usize,
    /// Etkin görünüm kaynağı.
    pub source: Source,
    /// USB ortamının dosya sistemi türü (durum çubuğu için).
    pub disk_kind: FsKind,
    /// Ortam okunamadıysa gösterilecek durum mesajı.
    pub status: &'static str,
    pub focused_control: usize, // 0: Refresh, 1: Open, 2: File list
}

impl FileManagerApp {
    pub fn row_rect(win_x: usize, win_y: usize, win_w: usize, idx: usize) -> (usize, usize, usize, usize) {
        let sidebar_w = 110;
        let list_x = win_x + sidebar_w + 14;
        let list_w = win_w.saturating_sub(sidebar_w + 22 + scrollbar::SCROLLBAR_THICKNESS);
        let row_y = win_y + LIST_TOP + idx * ROW_H;
        (list_x, row_y, list_w, ROW_H)
    }

    pub fn refresh_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(120), win_y + 32, 50, 22)
    }

    pub fn open_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(62), win_y + 32, 50, 22)
    }

    pub fn focus_next_control(&mut self) {
        self.focused_control = (self.focused_control + 1) % 3;
    }

    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    pub fn select_next(&mut self) {
        let count = self.row_count();
        if self.selected_idx + 1 < count {
            self.selected_idx += 1;
        }
    }

    pub const fn new() -> Self {
        Self {
            current_path: "/root/home/minos",
            selected_idx: 0,
            files: [
                FileItem { name: "kernel.log", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("1.2 KB"), file_type: "Sistem Günlüğü", icon_tag: "LOG", is_dir: false },
                FileItem { name: "readme.txt", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("2.4 KB"), file_type: "Metin Belgesi", icon_tag: "TXT", is_dir: false },
                FileItem { name: "settings.cfg", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("512 B"), file_type: "Ayar Dosyası", icon_tag: "CFG", is_dir: false },
                FileItem { name: "kedi_maskotu.bmp", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("64.0 KB"), file_type: "Resim İmajı", icon_tag: "IMG", is_dir: false },
                FileItem { name: "lofi_ring0.wav", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("3.2 MB"), file_type: "Ses Dosyası", icon_tag: "SND", is_dir: false },
                FileItem { name: "retro_drive.mp4", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("12.8 MB"), file_type: "Video Kaydı", icon_tag: "VID", is_dir: false },
                FileItem { name: "github_minos.url", dyn_name: [0; diskfs::NAME_MAX], dyn_name_len: 0, size_str: TextBuf::from_str("128 B"), file_type: "GitHub Yardım", icon_tag: "WEB", is_dir: false },
            ],
            disk_rows: [FileItem::empty(); diskfs::MAX_ENTRIES],
            disk_row_count: 0,
            source: Source::Vfs,
            disk_kind: FsKind::None,
            status: "",
            focused_control: 2,
        }
    }

    /// USB bellek / CD-ROM içeriğini (yeniden) okur ve listeyi kurar.
    pub fn refresh_disk(&mut self) {
        diskfs::refresh();
        self.disk_kind = if diskfs::usb_kind() != FsKind::None {
            diskfs::usb_kind()
        } else {
            diskfs::cd_kind()
        };
        self.disk_row_count = 0;
        for i in 0..diskfs::entry_count() {
            let Some(entry) = diskfs::entry(i) else { break };
            let mut row = FileItem::empty();
            let n = entry.name_len.min(diskfs::NAME_MAX);
            row.dyn_name[..n].copy_from_slice(&entry.name[..n]);
            row.dyn_name_len = n;
            row.is_dir = entry.is_dir;
            row.size_str = format_size(entry.size, entry.is_dir);
            row.icon_tag = if entry.is_dir { "DIR" } else { file_kind(entry.as_str()) };
            row.file_type = if entry.is_dir {
                crate::gui::ui_text("Klasör", "Folder")
            } else {
                match row.icon_tag {
                    "TXT" => crate::gui::ui_text("Metin Belgesi", "Text Document"),
                    "IMG" => crate::gui::ui_text("Resim İmajı", "Image"),
                    "SND" => crate::gui::ui_text("Ses Dosyası", "Audio"),
                    "VID" => crate::gui::ui_text("Video Kaydı", "Video"),
                    "WEB" => crate::gui::ui_text("Web Sayfası", "Web Page"),
                    "BIN" => crate::gui::ui_text("İkili Dosya", "Binary File"),
                    _ => crate::gui::ui_text("Dosya", "File"),
                }
            };
            self.disk_rows[self.disk_row_count] = row;
            self.disk_row_count += 1;
        }
        if self.disk_row_count == 0 {
            self.status = crate::gui::ui_text(
                "USB disk bulunamadı. run.ps1 -UsbDisks ile başlatın.",
                "No USB disk found. Start with run.ps1 -UsbDisks.",
            );
        } else {
            self.status = "";
        }
    }

    /// USB görünümüne geçer (ortam içeriğini okur).
    pub fn show_disk(&mut self) {
        self.source = Source::Disk;
        self.selected_idx = 0;
        self.current_path = if diskfs::usb_available() { "/media/usb0" } else { "/media/cdrom" };
        self.refresh_disk();
    }

    /// Yerel VFS görünümüne döner.
    pub fn show_vfs(&mut self, path: &'static str) {
        self.source = Source::Vfs;
        self.selected_idx = 0;
        self.current_path = path;
        self.status = "";
    }

    /// Etkin görünümdeki satır sayısı.
    pub fn row_count(&self) -> usize {
        match self.source {
            Source::Vfs => self.files.len(),
            Source::Disk => self.disk_row_count,
        }
    }

    /// Etkin görünümdeki `idx` satırı.
    pub fn row(&self, idx: usize) -> Option<&FileItem> {
        match self.source {
            Source::Vfs => self.files.get(idx),
            Source::Disk => self.disk_rows.get(idx).filter(|_| idx < self.disk_row_count),
        }
    }

    pub fn selected_icon_tag(&self) -> &'static str {
        self.row(self.selected_idx).map(|f| f.icon_tag).unwrap_or("TXT")
    }

    /// Etkin satırın gerçek disk adı (USB görünümünde).
    pub fn selected_disk_name(&self) -> Option<&str> {
        match self.source {
            Source::Disk => self.row(self.selected_idx).map(|f| f.display_name()),
            Source::Vfs => None,
        }
    }

    /// Listenin toplam içerik yüksekliği (kaydırma çubuğu için).
    pub fn list_content_h(&self) -> usize {
        self.row_count() * ROW_H
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
        let ref_x = win_x + win_w.saturating_sub(120);
        fb.draw_rect(ref_x, win_y + 32, 50, 22, Color::WINDOW_BG);
        if self.focused_control == 0 {
            fb.draw_rect_outline(ref_x, win_y + 32, 50, 22, Color::ACCENT_YELLOW);
        }
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(112), win_y + 38, crate::gui::ui_text("Yenile", "Refresh"), Color::TEXT_MAIN);

        let open_x = win_x + win_w.saturating_sub(62);
        fb.draw_rect(open_x, win_y + 32, 50, 22, Color::ACCENT_GREEN);
        if self.focused_control == 1 {
            fb.draw_rect_outline(open_x, win_y + 32, 50, 22, Color::ACCENT_YELLOW);
        }
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(52), win_y + 38, crate::gui::ui_text("Aç", "Open"), MinGuiCompositor::text_on(Color::ACCENT_GREEN));

        // Sol Gezinti Paneli (Sidebar) — pencere yüksekliğine göre uyarlanır
        let sidebar_h = win_h.saturating_sub(STATUS_H + 60);
        fb.draw_rect(win_x + 6, win_y + 60, sidebar_w, sidebar_h, Color::TITLE_BAR_INACTIVE);
        let nav = if crate::gui::ui_english() {
            ["> Root (/)", "  Documents", "  Media", "  Downloads", "  System (Limine)", "  USB Drive"]
        } else { ["> Kök (/)", "  Belgeler", "  Medyalar", "  İndirilenler", "  Sistem (Limine)", "  USB Disk"] };
        for (i, item) in nav.iter().enumerate() {
            let iy = win_y + 68 + i * 22;
            if iy + 8 > win_y + 60 + sidebar_h { break; }
            // USB Disk satırı ortam durumuna göre vurgulanır.
            let is_usb_row = i == 5;
            let active_usb = is_usb_row && self.source == Source::Disk;
            let col = if is_usb_row {
                if diskfs::usb_available() || active_usb { Color::ACCENT_GREEN } else { Color::TEXT_MUTED }
            } else if i == 0 && self.source == Source::Vfs { Color::TEXT_MAIN } else if i == 4 { Color::ACCENT_PINK } else { Color::TEXT_MUTED };
            MinGuiCompositor::draw_string(fb, win_x + 12, iy, item, col);
        }

        // Sağ Liste Başlıkları
        fb.draw_rect(list_x, win_y + 60, list_w, 20, Color::TITLE_BAR);
        let name_end = if list_w > 330 { 150 } else if list_w > 200 { 150 } else { list_w.saturating_sub(12) };
        MinGuiCompositor::draw_string_clipped(fb, list_x + 6, win_y + 66, crate::gui::ui_text("DOSYA ADI", "FILE NAME"), Color::TEXT_MUTED,
            (list_x + 4, win_y + 60, name_end.saturating_sub(4), 20));
        if list_w > 200 {
            MinGuiCompositor::draw_string_clipped(fb, list_x + 160, win_y + 66, crate::gui::ui_text("TÜR", "TYPE"), Color::TEXT_MUTED,
                (list_x + 154, win_y + 60, 124, 20));
        }
        if list_w > 330 {
            MinGuiCompositor::draw_string_clipped(fb, list_x + 290, win_y + 66, crate::gui::ui_text("BOYUT", "SIZE"), Color::TEXT_MUTED,
                (list_x + 284, win_y + 60, list_w.saturating_sub(290), 20));
        }

        // Dosya Listesi — scroll ofsetine göre çizilir; satırlar list_view_h'ye kırpılır.
        let row_count = self.row_count();
        for i in 0..row_count {
            let Some(file) = self.row(i) else { continue };
            let row_y = list_top + i * ROW_H;
            let draw_y = row_y.saturating_sub(sy);
            // Görünür aralığın dışındakileri atla.
            if row_y + ROW_H <= list_top + sy || row_y >= list_top + sy + list_view_h {
                continue;
            }
            let is_sel = i == self.selected_idx;
            if is_sel {
                fb.draw_rect(list_x, draw_y, list_w, ROW_H - 2, Color::TITLE_BAR);
                if self.focused_control == 2 {
                    fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::ACCENT_YELLOW);
                }
            }

            let icon_color = match file.icon_tag {
                "IMG" => Color::ACCENT_YELLOW,
                "SND" => Color::ACCENT_GREEN,
                "VID" => Color::ACCENT_RED,
                "WEB" => Color::ACCENT_BLUE,
                "DIR" => Color::ACCENT_PINK,
                _ => Color::TEXT_MAIN,
            };
            MinGuiCompositor::draw_string(fb, list_x + 4, draw_y + 4, file.icon_tag, icon_color);
            let name_width = if list_w > 330 { 104 } else if list_w > 200 { 104 } else { list_w.saturating_sub(56) };
            MinGuiCompositor::draw_string_clipped(fb, list_x + 48, draw_y + 4, file.display_name(), if is_sel { MinGuiCompositor::text_on(Color::TITLE_BAR) } else { Color::TEXT_MAIN },
                (list_x + 48, draw_y + 2, name_width, ROW_H - 4));

            if list_w > 200 {
                MinGuiCompositor::draw_string_clipped(fb, list_x + 160, draw_y + 4, file.file_type, Color::TEXT_MUTED,
                    (list_x + 160, draw_y + 2, 118, ROW_H - 4));
            }
            if list_w > 330 {
                MinGuiCompositor::draw_string_clipped(fb, list_x + 290, draw_y + 4, file.size_str.as_str(), Color::TEXT_MAIN,
                    (list_x + 290, draw_y + 2, list_w.saturating_sub(294), ROW_H - 4));
            }
        }

        // Alt Durum Çubuğu (pencerenin en altına sabitlenir)
        let status_y = win_y + win_h - STATUS_H + 6;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, 18, Color::TASKBAR_BG);
        let mut sb = TextBuf::new();
        if !self.status.is_empty() {
            sb.push_str(self.status);
        } else {
            sb.push_usize(row_count);
            sb.push_str(crate::gui::ui_text(" Dosya | ", " Files | "));
            sb.push_str(self.current_path);
            if self.source == Source::Disk {
                sb.push_str("  [");
                sb.push_str(self.disk_kind.label(crate::gui::ui_english()));
                sb.push_str("]");
            }
        }
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, sb.as_str(), Color::TEXT_MUTED);
    }
}

/// Dosya uzantısına göre simge etiketi.
pub fn file_kind(name: &str) -> &'static str {
    let lower = |b: u8| if b.is_ascii_uppercase() { b + 32 } else { b };
    let bytes = name.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i] == b'.' {
            let ext = &bytes[i + 1..];
            let matches_ext = |want: &[u8]| {
                ext.len() == want.len() && ext.iter().zip(want).all(|(a, b)| lower(*a) == *b)
            };
            if matches_ext(b"txt") || matches_ext(b"md") || matches_ext(b"log") || matches_ext(b"cfg") || matches_ext(b"ini") || matches_ext(b"nfo") {
                return "TXT";
            }
            if matches_ext(b"bmp") || matches_ext(b"png") || matches_ext(b"jpg") || matches_ext(b"jpeg") || matches_ext(b"gif") {
                return "IMG";
            }
            if matches_ext(b"wav") || matches_ext(b"mp3") || matches_ext(b"flac") || matches_ext(b"ogg") {
                return "SND";
            }
            if matches_ext(b"mp4") || matches_ext(b"avi") || matches_ext(b"mkv") || matches_ext(b"webm") {
                return "VID";
            }
            if matches_ext(b"htm") || matches_ext(b"html") || matches_ext(b"url") || matches_ext(b"xhtml") {
                return "WEB";
            }
            return "BIN";
        }
    }
    "BIN"
}

/// Boyutu okunabilir metne çevirir (B / KB / MB).
pub fn format_size(bytes: usize, is_dir: bool) -> TextBuf {
    let mut out = TextBuf::new();
    if is_dir {
        out.push_str("<DIR>");
        return out;
    }
    if bytes >= 1024 * 1024 {
        out.push_usize(bytes / (1024 * 1024));
        out.push_byte(b'.');
        out.push_usize((bytes / (1024 * 102)) % 10);
        out.push_str(" MB");
    } else if bytes >= 1024 {
        out.push_usize(bytes / 1024);
        out.push_byte(b'.');
        out.push_usize((bytes / 102) % 10);
        out.push_str(" KB");
    } else {
        out.push_usize(bytes);
        out.push_str(" B");
    }
    out
}

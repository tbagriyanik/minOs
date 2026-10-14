#![allow(dead_code, static_mut_refs)]

//! Dosya Yöneticisi Uygulaması (File Manager) — Klasör Desteği, Dosya Açma, Yeni Dosya/Klasör ve Silme.

extern crate alloc;
use alloc::string::String;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::scrollbar::{self, ScrollState};
use crate::fs::diskfs::{self, FsKind};
use crate::fs::vfs::{VirtualFileSystem, NodeType};

/// Dosya listesi satır yüksekliği (px).
const ROW_H: usize = 24;
/// Listenin başladığı Y ofseti (pencere içine göre).
pub const LIST_TOP: usize = 86;

pub fn list_top() -> usize {
    LIST_TOP
}
/// Durum çubuğu için ayrılan alt alan (px).
const STATUS_H: usize = 30;

pub const MAX_ITEMS: usize = 64;

#[derive(Clone, Copy)]
pub struct FileItem {
    pub name: &'static str,
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

    pub fn display_name(&self) -> &str {
        if self.dyn_name_len > 0 {
            core::str::from_utf8(&self.dyn_name[..self.dyn_name_len]).unwrap_or("")
        } else {
            self.name
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Vfs,
    Disk,
}

pub struct FileManagerApp {
    pub current_path: &'static str,
    pub path_buf: [u8; 96],
    pub path_len: usize,
    pub selected_idx: usize,
    pub files: [FileItem; MAX_ITEMS],
    pub file_count: usize,
    pub source: Source,
    pub disk_kind: FsKind,
    pub status: &'static str,
    pub focused_control: usize, // 0: Refresh, 1: New, 2: Folder, 3: Delete, 4: Open, 5: List
    item_counter: usize,
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
        (win_x + win_w.saturating_sub(286), win_y + 32, 50, 22)
    }

    pub fn new_file_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(230), win_y + 32, 50, 22)
    }

    pub fn new_folder_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(174), win_y + 32, 54, 22)
    }

    pub fn delete_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(114), win_y + 32, 46, 22)
    }

    pub fn open_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(62), win_y + 32, 50, 22)
    }

    pub fn focus_next_control(&mut self) {
        self.focused_control = (self.focused_control + 1) % 6;
    }

    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_idx + 1 < self.file_count {
            self.selected_idx += 1;
        }
    }

    pub const fn new() -> Self {
        Self {
            current_path: "/root/Documents",
            path_buf: [0; 96],
            path_len: 0,
            selected_idx: 0,
            files: [FileItem::empty(); MAX_ITEMS],
            file_count: 0,
            source: Source::Vfs,
            disk_kind: FsKind::None,
            status: "",
            focused_control: 5,
            item_counter: 1,
        }
    }

    pub fn path(&self) -> &str {
        if self.path_len > 0 {
            core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or(self.current_path)
        } else {
            self.current_path
        }
    }

    pub fn set_path(&mut self, p: &str) {
        self.path_len = 0;
        for b in p.bytes() {
            if self.path_len < self.path_buf.len() {
                self.path_buf[self.path_len] = b;
                self.path_len += 1;
            }
        }
        self.current_path = match p {
            "/" => "/",
            "/root" => "/root",
            "/root/Documents" => "/root/Documents",
            "/root/Media" => "/root/Media",
            "/root/Downloads" => "/root/Downloads",
            "/boot" => "/boot",
            "/media/usb0" => "/media/usb0",
            "/media/hdd0" => "/media/hdd0",
            "/media/cdrom" => "/media/cdrom",
            _ => "/root/Documents",
        };
    }

    /// VFS'ten mevcut dizinin içeriğini okur ve dosya listesini doldurur.
    pub fn refresh_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        let is_not_root = self.path() != "/";
        let items = vfs.list_dir(self.path());

        self.file_count = 0;
        self.selected_idx = 0;

        // Kök dizinde değilsek üst dizine dönme girdisi ekle [..]
        if is_not_root {
            let mut up = FileItem::empty();
            let up_name = b"..";
            up.dyn_name[..2].copy_from_slice(up_name);
            up.dyn_name_len = 2;
            up.is_dir = true;
            up.icon_tag = "DIR";
            up.file_type = crate::gui::ui_text("Ust Klasor", "Parent Folder");
            let mut s = TextBuf::new();
            s.push_str("<UP>");
            up.size_str = s;
            self.files[self.file_count] = up;
            self.file_count += 1;
        }

        // Önce klasörler
        for it in items.iter().filter(|i| i.node_type == NodeType::Directory) {
            if self.file_count >= MAX_ITEMS { break; }
            let mut row = FileItem::empty();
            let name_bytes = it.name.as_bytes();
            let take = name_bytes.len().min(diskfs::NAME_MAX);
            row.dyn_name[..take].copy_from_slice(&name_bytes[..take]);
            row.dyn_name_len = take;
            row.is_dir = true;
            row.icon_tag = "DIR";
            row.file_type = crate::gui::ui_text("Klasor", "Folder");
            row.size_str = format_size(0, true);
            self.files[self.file_count] = row;
            self.file_count += 1;
        }

        // Sonra dosyalar
        for it in items.iter().filter(|i| i.node_type == NodeType::File) {
            if self.file_count >= MAX_ITEMS { break; }
            let mut row = FileItem::empty();
            let name_bytes = it.name.as_bytes();
            let take = name_bytes.len().min(diskfs::NAME_MAX);
            row.dyn_name[..take].copy_from_slice(&name_bytes[..take]);
            row.dyn_name_len = take;
            row.is_dir = false;
            row.icon_tag = file_kind(&it.name);
            row.file_type = match row.icon_tag {
                "TXT" => crate::gui::ui_text("Metin Belgesi", "Text Document"),
                "IMG" => crate::gui::ui_text("Resim Imaji", "Image"),
                "SND" => crate::gui::ui_text("Ses Dosyasi", "Audio"),
                "VID" => crate::gui::ui_text("Video Kaydi", "Video"),
                "WEB" => crate::gui::ui_text("Web Sayfasi", "Web Page"),
                "CFG" => crate::gui::ui_text("Ayar Dosyasi", "Config"),
                "LOG" => crate::gui::ui_text("Sistem Gunlugu", "Log"),
                _ => crate::gui::ui_text("Dosya", "File"),
            };
            row.size_str = format_size(it.size, false);
            self.files[self.file_count] = row;
            self.file_count += 1;
        }

        self.status = "";
    }

    /// Yeni dosya oluşturur.
    pub fn new_file(&mut self, vfs: &mut VirtualFileSystem) {
        let is_fat = self.path().starts_with("/media/hdd0") || self.path().starts_with("/media/usb0");
        let mut name = if is_fat { String::from("NOT_") } else { String::from("yeni_belge_") };
        let mut num_buf = [0u8; 10];
        let mut v = self.item_counter;
        let mut n = 0;
        if v == 0 { num_buf[0] = b'0'; n = 1; }
        while v > 0 && n < num_buf.len() {
            num_buf[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
        }
        num_buf[..n].reverse();
        if let Ok(s) = core::str::from_utf8(&num_buf[..n]) {
            name.push_str(s);
        }
        if is_fat { name.push_str(".TXT"); } else { name.push_str(".txt"); }
        self.item_counter += 1;

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&name);

        vfs.create_file(&full_path, b"Yeni olusturulan dosya icerigi.\n");
        self.refresh_from_vfs(vfs);
        self.status = crate::gui::ui_text("Yeni dosya olusturuldu.", "New file created.");
    }

    /// Yeni klasör oluşturur.
    pub fn new_folder(&mut self, vfs: &mut VirtualFileSystem) {
        let is_fat = self.path().starts_with("/media/hdd0") || self.path().starts_with("/media/usb0");
        let mut name = if is_fat { String::from("KLASOR_") } else { String::from("Yeni_Klasor_") };
        let mut num_buf = [0u8; 10];
        let mut v = self.item_counter;
        let mut n = 0;
        if v == 0 { num_buf[0] = b'0'; n = 1; }
        while v > 0 && n < num_buf.len() {
            num_buf[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
        }
        num_buf[..n].reverse();
        if let Ok(s) = core::str::from_utf8(&num_buf[..n]) {
            name.push_str(s);
        }
        self.item_counter += 1;

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&name);

        vfs.create_dir(&full_path);
        self.refresh_from_vfs(vfs);
        self.status = crate::gui::ui_text("Yeni klasor olusturuldu.", "New folder created.");
    }

    /// Seçili dosya veya klasörü siler.
    pub fn delete_selected(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        if self.selected_idx >= self.file_count {
            return false;
        }
        let item = &self.files[self.selected_idx];
        let name = item.display_name();
        if name == ".." {
            return false;
        }

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(name);

        let ok = if item.is_dir {
            vfs.delete_dir(&full_path)
        } else {
            vfs.delete_file(&full_path)
        };

        if ok {
            self.refresh_from_vfs(vfs);
            self.status = crate::gui::ui_text("Oge basariyla silindi.", "Item deleted successfully.");
        } else {
            self.status = crate::gui::ui_text("Silinemedi (klasor dolu olabilir).", "Delete failed.");
        }
        ok
    }

    /// Seçili öğeyi açar veya klasöre girer. Klasör ise `None` döner ve dizin değiştirir.
    /// Dosya ise `Some((icon_tag, full_path_string))` döner.
    pub fn enter_selected(&mut self, vfs: &VirtualFileSystem) -> Option<(&'static str, String)> {
        if self.selected_idx >= self.file_count {
            return None;
        }
        let item = self.files[self.selected_idx];
        let name = item.display_name();

        if item.is_dir {
            if name == ".." {
                // Üst klasöre çık
                let cur = self.path();
                if let Some(pos) = cur.rfind('/') {
                    let parent = if pos == 0 { "/" } else { &cur[..pos] };
                    let parent_clone = String::from(parent);
                    self.set_path(&parent_clone);
                } else {
                    self.set_path("/");
                }
            } else {
                // Alt klasöre gir
                let mut next = String::from(self.path());
                if !next.ends_with('/') {
                    next.push('/');
                }
                next.push_str(name);
                self.set_path(&next);
            }
            self.refresh_from_vfs(vfs);
            None
        } else {
            let mut full_path = String::from(self.path());
            if !full_path.ends_with('/') {
                full_path.push('/');
            }
            full_path.push_str(name);
            Some((item.icon_tag, full_path))
        }
    }

    pub fn selected_full_path(&self) -> String {
        let mut p = String::from(self.path());
        if self.selected_idx < self.file_count {
            let name = self.files[self.selected_idx].display_name();
            if name != ".." {
                if !p.ends_with('/') {
                    p.push('/');
                }
                p.push_str(name);
            }
        }
        p
    }

    pub fn row_count(&self) -> usize {
        self.file_count
    }

    pub fn row(&self, idx: usize) -> Option<&FileItem> {
        if idx < self.file_count {
            Some(&self.files[idx])
        } else {
            None
        }
    }

    pub fn list_content_h(&self) -> usize {
        self.file_count * ROW_H
    }

    /// Klasör Simgesi: Arka sekme, gövde, parlak sarı/amber kapak ve yukarı oku (..) desteği
    pub fn draw_folder_icon(fb: &mut Framebuffer, x: usize, y: usize, is_up: bool) {
        // Klasör Arka Sekmesi (Tab)
        fb.draw_rect(x + 1, y, 6, 3, Color::ACCENT_YELLOW);
        // Klasör Arka Gövdesi (Back Body)
        fb.draw_rect(x + 1, y + 2, 14, 10, MinGuiCompositor::shade(Color::ACCENT_YELLOW, -30));
        // Klasör Ön Kapağı (Front Flap - canlı parlak sarı)
        fb.draw_rect(x, y + 4, 16, 9, Color::ACCENT_YELLOW);
        // Üst kenar parıltısı (Highlight)
        fb.draw_line(x + 1, y + 4, x + 15, y + 4, 0xFFE082);
        // İnce alt kenarlık / gölge
        fb.draw_line(x, y + 12, x + 15, y + 12, MinGuiCompositor::shade(Color::ACCENT_YELLOW, -50));

        // Eğer üst klasör ise (..) içine beyaz yukarı oku çiz
        if is_up {
            fb.draw_line(x + 8, y + 6, x + 8, y + 10, Color::WHITE);
            fb.draw_line(x + 7, y + 7, x + 9, y + 7, Color::WHITE);
            fb.draw_line(x + 6, y + 8, x + 10, y + 8, Color::WHITE);
        }
    }

    /// Belge / Dosya Simgesi: Katlanmış köşeli sayfa ve dosya türü vurgusu
    pub fn draw_file_icon(fb: &mut Framebuffer, x: usize, y: usize, tag: &str) {
        let accent = match tag {
            "IMG" => Color::ACCENT_YELLOW,
            "SND" => Color::ACCENT_GREEN,
            "VID" => Color::ACCENT_RED,
            "WEB" => Color::ACCENT_BLUE,
            "CFG" => Color::ACCENT_PINK,
            _ => Color::TEXT_MAIN,
        };
        // Sayfa beyaz kağıt gövdesi
        fb.draw_rect(x + 2, y + 1, 11, 13, Color::WHITE);
        // Sağ üst katlanmış köşe (Folded Corner)
        fb.draw_rect(x + 9, y + 1, 4, 4, Color::TITLE_BAR);
        // İçerik satır çizgileri
        fb.draw_line(x + 4, y + 5, x + 10, y + 5, accent);
        fb.draw_line(x + 4, y + 7, x + 10, y + 7, accent);
        fb.draw_line(x + 4, y + 9, x + 8, y + 9, accent);
        // İnce sayfa çerçevesi
        fb.draw_rect_outline(x + 2, y + 1, 11, 13, Color::BORDER);
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize, scroll: &ScrollState) {
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
        MinGuiCompositor::draw_string(fb, win_x + 65, win_y + 38, self.path(), Color::ACCENT_BLUE);

        // Aksiyon Butonları (Yenile, +Dosya, +Klasör, Sil, Aç)
        let buttons = [
            (Self::refresh_button_rect(win_x, win_y, win_w), crate::gui::ui_text("Yenile", "Ref"), 0usize),
            (Self::new_file_button_rect(win_x, win_y, win_w), crate::gui::ui_text("+Dosya", "+File"), 1usize),
            (Self::new_folder_button_rect(win_x, win_y, win_w), crate::gui::ui_text("+Klasor", "+Dir"), 2usize),
            (Self::delete_button_rect(win_x, win_y, win_w), crate::gui::ui_text("Sil", "Del"), 3usize),
            (Self::open_button_rect(win_x, win_y, win_w), crate::gui::ui_text("Ac", "Open"), 4usize),
        ];
        for (r, label, ctrl_id) in buttons {
            let is_focused = self.focused_control == ctrl_id;
            let bg = if is_focused {
                MinGuiCompositor::shade(Color::WINDOW_BG, 22)
            } else {
                MinGuiCompositor::shade(Color::WINDOW_BG, 8)
            };
            let border = if is_focused { Color::ACCENT_BLUE } else { Color::BORDER };
            fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, bg);
            fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 4, border);
            let tw = MinGuiCompositor::text_width(label, 1);
            let tx = r.0 + (r.2.saturating_sub(tw)) / 2;
            let ty = r.1 + (r.3.saturating_sub(MinGuiCompositor::text_height())) / 2;
            MinGuiCompositor::draw_string(fb, tx, ty, label, Color::TEXT_MAIN);
        }

        // Sol Gezinti Paneli (Sidebar) — Sürücüler ve Hızlı Klasörler
        let sidebar_h = win_h.saturating_sub(STATUS_H + 60);
        fb.draw_rect(win_x + 6, win_y + 60, sidebar_w, sidebar_h, Color::TITLE_BAR_INACTIVE);

        let nav = if crate::gui::ui_english() {
            ["Root (/)", "Documents", "Media", "Downloads", "System", "USB Drive", "Hard Disk", "CD-ROM"]
        } else {
            ["Kok (/)", "Belgeler", "Medyalar", "Indirilenler", "Sistem", "USB Disk", "Sabit Disk", "CD-ROM"]
        };

        for (i, item) in nav.iter().enumerate() {
            let iy = win_y + 66 + i * 22;
            if iy + 8 > win_y + 60 + sidebar_h { break; }
            let cur = self.path();
            let is_active = match i {
                0 => cur == "/",
                1 => cur == "/root/Documents",
                2 => cur == "/root/Media",
                3 => cur == "/root/Downloads",
                4 => cur == "/boot",
                5 => cur == "/media/usb0",
                6 => cur == "/media/hdd0",
                7 => cur == "/media/cdrom",
                _ => false,
            };
            if is_active {
                fb.draw_rect(win_x + 7, iy - 2, sidebar_w - 2, 20, Color::TITLE_BAR);
            }
            if i <= 4 {
                Self::draw_folder_icon(fb, win_x + 9, iy + 2, false);
            } else if i == 5 {
                // USB Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_GREEN);
                fb.draw_rect(win_x + 12, iy + 1, 8, 3, Color::WHITE);
            } else if i == 6 {
                // HDD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_YELLOW);
                fb.draw_line(win_x + 11, iy + 8, win_x + 21, iy + 8, Color::BLACK);
            } else {
                // CD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_PINK);
                fb.draw_rect(win_x + 14, iy + 6, 4, 4, Color::WHITE);
            }
            let col = if is_active {
                Color::ACCENT_BLUE
            } else if i == 5 {
                if diskfs::usb_available() { Color::ACCENT_GREEN } else { Color::TEXT_MUTED }
            } else if i == 6 {
                if crate::drivers::usb::hdd_available() || diskfs::usb_available() { Color::ACCENT_YELLOW } else { Color::TEXT_MUTED }
            } else if i == 7 {
                if diskfs::cd_available() { Color::ACCENT_PINK } else { Color::TEXT_MUTED }
            } else {
                Color::TEXT_MAIN
            };
            MinGuiCompositor::draw_string(fb, win_x + 28, iy + 2, item, col);
        }

        // Sağ Liste Başlıkları
        fb.draw_rect(list_x, win_y + 60, list_w, 20, Color::TITLE_BAR);
        let col_name_x = list_x + 28;
        let col_type_x = list_x + 180;
        let col_size_x = list_x + 285;

        MinGuiCompositor::draw_string_clipped(fb, col_name_x, win_y + 65, crate::gui::ui_text("Ad", "Name"), Color::TEXT_MUTED,
            (col_name_x, win_y + 60, 145, 20));
        if list_w > 210 {
            MinGuiCompositor::draw_string_clipped(fb, col_type_x, win_y + 65, crate::gui::ui_text("Tür", "Type"), Color::TEXT_MUTED,
                (col_type_x, win_y + 60, 95, 20));
        }
        if list_w > 320 {
            MinGuiCompositor::draw_string_clipped(fb, col_size_x, win_y + 65, crate::gui::ui_text("Boyut", "Size"), Color::TEXT_MUTED,
                (col_size_x, win_y + 60, list_w.saturating_sub(285), 20));
        }

        // Dosya ve Klasör Listesi
        for i in 0..self.file_count {
            let row_y = list_top + i * ROW_H;
            let draw_y = row_y.saturating_sub(sy);
            if row_y + ROW_H <= list_top + sy || row_y >= list_top + sy + list_view_h {
                continue;
            }
            let file = &self.files[i];
            let is_sel = i == self.selected_idx;
            if is_sel {
                fb.draw_rect(list_x, draw_y, list_w, ROW_H - 2, Color::TITLE_BAR);
                if self.focused_control == 5 {
                    fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::ACCENT_BLUE);
                }
            }

            let is_up = file.display_name() == "..";
            if file.is_dir {
                Self::draw_folder_icon(fb, list_x + 6, draw_y + 3, is_up);
            } else {
                Self::draw_file_icon(fb, list_x + 6, draw_y + 3, file.icon_tag);
            }

            let name_col = if is_sel { MinGuiCompositor::text_on(Color::TITLE_BAR) } else { Color::TEXT_MAIN };
            MinGuiCompositor::draw_string_clipped(fb, col_name_x, draw_y + 4, file.display_name(), name_col,
                (col_name_x, draw_y + 2, 145, ROW_H - 4));

            if list_w > 210 {
                MinGuiCompositor::draw_string_clipped(fb, col_type_x, draw_y + 4, file.file_type, Color::TEXT_MUTED,
                    (col_type_x, draw_y + 2, 95, ROW_H - 4));
            }
            if list_w > 320 {
                MinGuiCompositor::draw_string_clipped(fb, col_size_x, draw_y + 4, file.size_str.as_str(), Color::TEXT_MAIN,
                    (col_size_x, draw_y + 2, list_w.saturating_sub(285), ROW_H - 4));
            }
        }

        // Alt Durum Çubuğu
        let status_y = win_y + win_h - STATUS_H + 6;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, 18, Color::TASKBAR_BG);
        let mut sb = TextBuf::new();
        if !self.status.is_empty() {
            sb.push_str(self.status);
        } else {
            sb.push_usize(self.file_count);
            sb.push_str(crate::gui::ui_text(" Oge | ", " Items | "));
            sb.push_str(self.path());
        }
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, sb.as_str(), Color::TEXT_MUTED);
    }
}

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
            if matches_ext(b"txt") || matches_ext(b"md") || matches_ext(b"nfo") {
                return "TXT";
            }
            if matches_ext(b"cfg") || matches_ext(b"ini") {
                return "CFG";
            }
            if matches_ext(b"log") {
                return "LOG";
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

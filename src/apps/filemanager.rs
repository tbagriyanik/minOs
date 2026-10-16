#![allow(dead_code, static_mut_refs)]

//! Dosya Yöneticisi Uygulaması (File Manager) — Klasör Desteği, Dosya Açma, Yeni Dosya/Klasör ve Silme.

extern crate alloc;
use alloc::string::String;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::scrollbar::{self, ScrollState};
use crate::fs::diskfs::{self, FsKind};
use crate::fs::vfs::{VirtualFileSystem, NodeType};

fn numbered_name(prefix: &str, mut value: usize, extension: &str) -> String {
    let mut digits = [0u8; 10];
    let mut len = 0;
    if value == 0 { digits[0] = b'0'; len = 1; }
    while value > 0 && len < digits.len() { digits[len] = b'0' + (value % 10) as u8; len += 1; value /= 10; }
    digits[..len].reverse();
    let mut name = String::from(prefix);
    for _ in len..3 { name.push('0'); }
    if let Ok(number) = core::str::from_utf8(&digits[..len]) { name.push_str(number); }
    name.push_str(extension);
    name
}

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

pub struct FileClipboard {
    pub paths: [TextBuf; MAX_ITEMS],
    pub count: usize,
    pub cut: bool,
}

impl FileClipboard {
    pub const fn new() -> Self { Self { paths: [TextBuf::new(); MAX_ITEMS], count: 0, cut: false } }
}

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget {
    Sidebar(usize),
    Folder(usize),
}

pub struct FileManagerApp {
    pub current_path: &'static str,
    pub path_buf: [u8; 96],
    pub path_len: usize,
    pub selected_idx: usize,
    pub selected_items: [bool; MAX_ITEMS],
    pub selection_anchor: usize,
    pub files: [FileItem; MAX_ITEMS],
    pub file_count: usize,
    pub source: Source,
    pub disk_kind: FsKind,
    pub status: &'static str,
    pub status_buf: TextBuf,
    pub rename_active: bool,
    pub rename_buf: [u8; diskfs::NAME_MAX],
    pub rename_len: usize,
    pub focused_control: usize, // 0: Refresh, 1: New, 2: Folder, 3: Delete, 4: Open, 5: List
    pub last_click_idx: Option<usize>,
    pub last_click_ms: u64,
    pub drag_item: Option<usize>,
    pub drag_start: (usize, usize),
    pub drag_active: bool,
    pub drag_target: Option<DropTarget>,
    pub drag_items: [bool; MAX_ITEMS],
    pub rename_cursor: usize,
    item_counter: usize,
}

impl FileManagerApp {
    pub fn row_rect(win_x: usize, win_y: usize, win_w: usize, idx: usize) -> (usize, usize, usize, usize) {
        Self::row_rect_scrolled(win_x, win_y, win_w, idx, 0)
    }

    pub fn row_rect_scrolled(win_x: usize, win_y: usize, win_w: usize, idx: usize, sy: usize) -> (usize, usize, usize, usize) {
        let sidebar_w = 110;
        let list_x = win_x + sidebar_w + 14;
        let list_w = win_w.saturating_sub(sidebar_w + 22 + scrollbar::SCROLLBAR_THICKNESS);
        let row_y = (win_y + LIST_TOP + idx * ROW_H).saturating_sub(sy);
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

    /// Satır yüksekliği (px) — kaydırma hesapları için.
    pub fn row_h() -> usize { ROW_H }

    /// Listenin ilk öğesini seçer (Home).
    pub fn select_first(&mut self) { self.selected_idx = 0; }

    /// Listenin son öğesini seçer (End).
    pub fn select_last(&mut self) {
        if self.file_count > 0 { self.selected_idx = self.file_count - 1; }
    }

    /// Sayfa sayfa gezinme (PageUp / PageDown).
    pub fn select_page(&mut self, down: bool, rows: usize) {
        if self.file_count == 0 { return; }
        let step = rows.max(1);
        if down {
            self.selected_idx = (self.selected_idx + step).min(self.file_count - 1);
        } else {
            self.selected_idx = self.selected_idx.saturating_sub(step);
        }
    }

    pub const fn new() -> Self {
        Self {
            current_path: "/root/Documents",
            path_buf: [0; 96],
            path_len: 0,
            selected_idx: 0,
            selected_items: [false; MAX_ITEMS],
            selection_anchor: 0,
            files: [FileItem::empty(); MAX_ITEMS],
            file_count: 0,
            source: Source::Vfs,
            disk_kind: FsKind::None,
            status: "",
            status_buf: TextBuf::new(),
            rename_active: false,
            rename_buf: [0; diskfs::NAME_MAX],
            rename_len: 0,
            focused_control: 5,
            last_click_idx: None,
            last_click_ms: 0,
            drag_item: None,
            drag_start: (0, 0),
            drag_active: false,
            drag_target: None,
            drag_items: [false; MAX_ITEMS],
            rename_cursor: 0,
            item_counter: 1,
        }
    }

    pub fn set_status_text(&mut self, text: &str) {
        self.status = "";
        self.status_buf.clear();
        self.status_buf.push_str(text);
    }

    pub fn path(&self) -> &str {
        if self.path_len > 0 {
            core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or(self.current_path)
        } else {
            self.current_path
        }
    }

    pub fn set_path(&mut self, p: &str) {
        self.selected_items = [false; MAX_ITEMS];
        self.last_click_idx = None;
        self.drag_item = None;
        self.drag_active = false;
        self.drag_target = None;
        self.status = "";
        self.status_buf.clear();
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
            "/root/Trash" => "/root/Trash",
            _ => "/root/Documents",
        };
    }

    /// VFS'ten mevcut dizinin içeriğini okur ve dosya listesini doldurur.
    pub fn refresh_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        self.selected_items = [false; MAX_ITEMS];
        self.last_click_idx = None;
        self.drag_item = None;
        self.drag_active = false;
        self.drag_target = None;
        let is_not_root = self.path() != "/";
        let items = vfs.list_dir(self.path());

        self.file_count = 0;
        self.selected_idx = 0;

        // Kök dizinde değilsek üst dizine dönme girdisi ekle [..].
        // Masaüstünde (/desktop) üst klasör kavramı yoktur: ".." simgesi
        // çizilmesin ve "/desktop/.." gibi geçersiz bir yol açılmasın.
        if is_not_root && self.path() != "/desktop" {
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
                "3D" => crate::gui::ui_text("3D Model", "3D Model"),
                "IMG" => crate::gui::ui_text("Resim Imaji", "Image"),
                "SND" => crate::gui::ui_text("Ses Dosyası", "Audio"),
                "VID" => crate::gui::ui_text("Video Kaydi", "Video"),
                "WEB" => crate::gui::ui_text("Web Sayfasi", "Web Page"),
                "CFG" => crate::gui::ui_text("Ayar Dosyası", "Config"),
                "LOG" => crate::gui::ui_text("Sistem Gunlugu", "Log"),
                _ => crate::gui::ui_text("Dosya", "File"),
            };
            row.size_str = format_size(it.size, false);
            self.files[self.file_count] = row;
            self.file_count += 1;
        }

        self.status = "";
    }

    pub fn select_row(&mut self, idx: usize, ctrl: bool, shift: bool) {
        if idx >= self.file_count { return; }
        if self.files[idx].display_name() == ".." {
            self.selected_items = [false; MAX_ITEMS];
            self.selected_idx = idx;
            return;
        }
        if shift {
            self.selected_items = [false; MAX_ITEMS];
            let start = self.selection_anchor.min(idx);
            let end = self.selection_anchor.max(idx);
            for i in start..=end.min(self.file_count.saturating_sub(1)) {
                if self.files[i].display_name() != ".." { self.selected_items[i] = true; }
            }
        } else if ctrl {
            self.selected_items[idx] = !self.selected_items[idx];
            self.selection_anchor = idx;
        } else {
            self.selected_items = [false; MAX_ITEMS];
            self.selected_items[idx] = true;
            self.selection_anchor = idx;
        }
        self.selected_idx = idx;
    }

    pub fn is_row_selected(&self, idx: usize) -> bool {
        self.selected_items.get(idx).copied().unwrap_or(false) || idx == self.selected_idx
    }

    pub fn copy_selection(&mut self, clipboard: &mut FileClipboard, cut: bool) {
        if self.selected_idx >= self.file_count { return; }
        if !self.selected_items[self.selected_idx] {
            self.selected_items = [false; MAX_ITEMS];
            self.selected_items[self.selected_idx] = true;
        }
        clipboard.count = 0;
        let current = String::from(self.path());
        for i in 0..self.file_count {
            if !self.selected_items[i] || self.files[i].display_name() == ".." { continue; }
            let mut path = current.clone();
            if !path.ends_with('/') { path.push('/'); }
            path.push_str(self.files[i].display_name());
            let mut saved = TextBuf::new();
            saved.push_str(path.as_str());
            clipboard.paths[clipboard.count] = saved;
            clipboard.count += 1;
        }
        clipboard.cut = cut;
        self.status = if cut { crate::gui::ui_text("Taşımak için seçildi.", "Selected to move.") }
            else { crate::gui::ui_text("Kopyalamak için seçildi.", "Selected to copy.") };
    }

    pub fn paste_selection(&mut self, clipboard: &mut FileClipboard, vfs: &mut VirtualFileSystem) -> usize {
        let dest = String::from(self.path());
        if dest == "/media/cdrom" { self.status = crate::gui::ui_text("CD-ROM salt okunurdur.", "CD-ROM is read-only."); return 0; }
        let mut done = 0;
        for i in 0..clipboard.count {
            let src = String::from(clipboard.paths[i].as_str());
            let result = if clipboard.cut { vfs.move_item(&src, &dest).map(|_| ()) }
                else { vfs.copy_item(&src, &dest).map(|_| ()) };
            if result.is_ok() { done += 1; }
        }
        if clipboard.cut && done > 0 { clipboard.count = 0; clipboard.cut = false; }
        self.refresh_from_vfs(vfs);
        self.status = if done > 0 { crate::gui::ui_text("Yapıştırma tamamlandı.", "Paste completed.") }
            else { crate::gui::ui_text("Yapıştırılacak öğe yok.", "Nothing to paste.") };
        done
    }

    /// Yeni dosya oluşturur.
    pub fn new_file(&mut self, vfs: &mut VirtualFileSystem) {
        let mut counter = self.item_counter;
        let name = loop {
            let candidate = numbered_name("yeni_", counter, ".txt");
            if !vfs.list_dir(self.path()).iter().any(|item| item.name.eq_ignore_ascii_case(&candidate)) { break candidate; }
            counter += 1;
        };
        self.item_counter = counter + 1;

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&name);

        let saved = vfs.write_file(&full_path, "Yeni oluşturulan dosya içeriği.\n".as_bytes());
        self.refresh_from_vfs(vfs);
        self.status = if saved { crate::gui::ui_text("Yeni dosya oluşturuldu.", "New file created.") }
            else { crate::gui::ui_text("Dosya oluşturuldu; diske kaydedilemedi.", "File created; disk save failed.") };
    }

    /// Yeni klasör oluşturur.
    pub fn new_folder(&mut self, vfs: &mut VirtualFileSystem) {
        let mut counter = self.item_counter;
        let name = loop {
            let candidate = numbered_name("klas_", counter, "");
            if !vfs.list_dir(self.path()).iter().any(|item| item.name.eq_ignore_ascii_case(&candidate)) { break candidate; }
            counter += 1;
        };
        self.item_counter = counter + 1;

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&name);

        let saved = vfs.create_dir(&full_path);
        self.refresh_from_vfs(vfs);
        self.status = if saved { crate::gui::ui_text("Yeni klasör oluşturuldu.", "New folder created.") }
            else { crate::gui::ui_text("Klasör oluşturulamadı.", "Folder creation failed.") };
    }

    /// Seçili dosya veya klasörü siler. Öğe kalıcı olarak silinmez; bunun yerine
    /// Geri Dönüşüm Kutusu'na (`/root/Trash`) taşınır. Çöp kutusundayken yapılan
    /// silme işlemi ise kalıcıdır (boşaltma davranışı).
    pub fn delete_selected(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        if self.selected_idx >= self.file_count {
            return false;
        }
        if !self.selected_items[self.selected_idx] { self.selected_items = [false; MAX_ITEMS]; self.selected_items[self.selected_idx] = true; }
        let mut any = false;
        for i in (0..self.file_count).rev() {
            if !self.selected_items[i] { continue; }
            let item = self.files[i];
            let name = item.display_name();
            if name == ".." { continue; }
            let mut full_path = String::from(self.path());
            if !full_path.ends_with('/') { full_path.push('/'); }
            full_path.push_str(name);
            let ok = if self.path() == crate::fs::vfs::TRASH_DIR {
                if item.is_dir { vfs.delete_tree(&full_path) } else { vfs.delete_file(&full_path) }
            } else { vfs.move_to_trash(&full_path) };
            any |= ok;
        }
        if any {
            self.refresh_from_vfs(vfs);
            self.status = if self.path() == crate::fs::vfs::TRASH_DIR {
                crate::gui::ui_text("Seçilen öğeler kalıcı olarak silindi.", "Selected items permanently deleted.")
            } else {
                crate::gui::ui_text("Seçilen öğeler geri dönüşüm kutusuna taşındı.", "Selected items moved to Recycle Bin.")
            };
        } else {
            self.status = crate::gui::ui_text("Silinemedi (klasör dolu olabilir).", "Delete failed.");
        }
        any
    }

    pub fn begin_rename(&mut self) {
        if self.selected_idx >= self.file_count || self.files[self.selected_idx].display_name() == ".." { return; }
        if !self.selected_items[self.selected_idx] { self.selected_items = [false; MAX_ITEMS]; self.selected_items[self.selected_idx] = true; }
        let name = self.files[self.selected_idx].display_name().as_bytes();
        self.rename_len = name.len().min(self.rename_buf.len());
        self.rename_buf[..self.rename_len].copy_from_slice(&name[..self.rename_len]);
        self.rename_cursor = self.rename_len;
        self.rename_active = true;
    }

    pub fn rename_char(&mut self, c: char) {
        let mut b = [0u8; 4]; let s = c.encode_utf8(&mut b);
        for &ch in s.as_bytes() { if self.rename_len < self.rename_buf.len() { self.rename_buf.copy_within(self.rename_cursor..self.rename_len, self.rename_cursor + 1); self.rename_buf[self.rename_cursor] = ch; self.rename_cursor += 1; self.rename_len += 1; } }
    }

    pub fn rename_backspace(&mut self) { if self.rename_cursor > 0 { self.rename_cursor -= 1; self.rename_buf.copy_within(self.rename_cursor + 1..self.rename_len, self.rename_cursor); self.rename_len -= 1; } }
    pub fn rename_delete(&mut self) { if self.rename_cursor < self.rename_len { self.rename_buf.copy_within(self.rename_cursor + 1..self.rename_len, self.rename_cursor); self.rename_len -= 1; } }
    pub fn rename_move_cursor(&mut self, right: bool) { if right { self.rename_cursor = (self.rename_cursor + 1).min(self.rename_len); } else { self.rename_cursor = self.rename_cursor.saturating_sub(1); } }

    pub fn commit_rename(&mut self, vfs: &mut VirtualFileSystem) {
        if !self.rename_active { return; }
        self.rename_active = false;
        let name = core::str::from_utf8(&self.rename_buf[..self.rename_len]).unwrap_or("").trim();
        if name.is_empty() || name == "." || name == ".." || name.contains('/') { self.status = crate::gui::ui_text("Geçersiz ad.", "Invalid name."); return; }
        if self.selected_idx >= self.file_count { return; }
        let old_name = self.files[self.selected_idx].display_name();
        let mut old_path = String::from(self.path()); if !old_path.ends_with('/') { old_path.push('/'); } old_path.push_str(old_name);
        let mut new_path = String::from(self.path()); if !new_path.ends_with('/') { new_path.push('/'); } new_path.push_str(name);
        if vfs.rename(&old_path, &new_path) { self.refresh_from_vfs(vfs); self.status = crate::gui::ui_text("Ad değiştirildi.", "Name changed."); }
        else { self.status = crate::gui::ui_text("Ad değiştirilemedi.", "Rename failed."); }
    }

    /// Geri Dönüşüm Kutusu'nda seçili öğeyi önceki konumuna değil, `/root/Documents`
    /// klasörüne geri yükler. Öğe adı çakışırsa numaralandırılır.
    pub fn restore_selected(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        if self.path() != crate::fs::vfs::TRASH_DIR {
            return false;
        }
        if self.selected_idx >= self.file_count {
            return false;
        }
        let item = &self.files[self.selected_idx];
        let name = item.display_name();
        if name == ".." {
            return false;
        }
        let mut trash_path = String::from(crate::fs::vfs::TRASH_DIR);
        trash_path.push('/');
        trash_path.push_str(name);
        let ok = vfs.restore_from_trash(&trash_path, "/root/Documents");
        if ok {
            self.refresh_from_vfs(vfs);
            self.status = crate::gui::ui_text("Öğe geri yüklendi.", "Item restored.");
        } else {
            self.status = crate::gui::ui_text("Geri yüklenemedi.", "Restore failed.");
        }
        ok
    }

    /// Geri Dönüşüm Kutusu'nu boşaltır.
    pub fn empty_trash(&mut self, vfs: &mut VirtualFileSystem) -> usize {
        let n = vfs.empty_trash();
        if self.path() == crate::fs::vfs::TRASH_DIR {
            self.refresh_from_vfs(vfs);
        }
        self.status = if n > 0 {
            crate::gui::ui_text("Geri dönüşüm kutusu boşaltıldı.", "Recycle Bin emptied.")
        } else {
            crate::gui::ui_text("Geri dönüşüm kutusu zaten boş.", "Recycle Bin already empty.")
        };
        n
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

    pub fn sidebar_path(index: usize) -> &'static str {
        match index {
            0 => "/",
            1 => "/desktop",
            2 => "/root/Documents",
            3 => "/root/Media",
            4 => "/root/Downloads",
            5 => "/boot",
            6 => "/media/usb0",
            7 => "/media/hdd0",
            8 => "/media/cdrom",
            _ => crate::fs::vfs::TRASH_DIR,
        }
    }

    pub fn sidebar_name(index: usize) -> &'static str {
        if crate::gui::ui_english() {
            match index {
                0 => "Root (/)",
                1 => "Desktop",
                2 => "Documents",
                3 => "Media",
                4 => "Downloads",
                5 => "System",
                6 => "USB Drive",
                7 => "Hard Disk",
                8 => "CD-ROM",
                _ => "Recycle Bin",
            }
        } else {
            match index {
                0 => "Kök (/)",
                1 => "Masaüstü",
                2 => "Belgeler",
                3 => "Medyalar",
                4 => "İndirilenler",
                5 => "Sistem",
                6 => "USB Disk",
                7 => "Sabit Disk",
                8 => "CD-ROM",
                _ => "Geri Dönüşüm",
            }
        }
    }

    pub fn drop_target_at(&self, win_x: usize, win_y: usize, win_w: usize, win_h: usize, sy: usize, mx: usize, my: usize) -> Option<DropTarget> {
        let sidebar_w = 110;
        let sidebar_h = win_h.saturating_sub(STATUS_H + 60);

        // Sidebar drop target check
        if mx >= win_x + 6 && mx < win_x + 6 + sidebar_w && my >= win_y + 66 && my < win_y + 66 + 10 * 22 {
            let idx = (my.saturating_sub(win_y + 66)) / 22;
            if idx < 10 && (win_y + 66 + idx * 22 + 8) <= win_y + 60 + sidebar_h {
                return Some(DropTarget::Sidebar(idx));
            }
        }

        // File list drop target check (must be a directory and not the item being dragged)
        let list_top = win_y + LIST_TOP;
        let list_bottom = win_y + win_h - STATUS_H;
        for i in 0..self.file_count {
            if self.files[i].is_dir && Some(i) != self.drag_item {
                if my >= list_top && my < list_bottom {
                    let r = Self::row_rect_scrolled(win_x, win_y, win_w, i, sy);
                    if crate::gui::in_rect(mx, my, r) {
                        return Some(DropTarget::Folder(i));
                    }
                }
            }
        }

        None
    }

    pub fn target_directory(&self, target: DropTarget) -> Option<String> {
        match target {
            DropTarget::Sidebar(idx) => Some(String::from(Self::sidebar_path(idx))),
            DropTarget::Folder(i) => {
                if i >= self.file_count || !self.files[i].is_dir {
                    return None;
                }
                let name = self.files[i].display_name();
                if name == ".." {
                    let cur = self.path();
                    if let Some(pos) = cur.rfind('/') {
                        let parent = if pos == 0 { "/" } else { &cur[..pos] };
                        Some(String::from(parent))
                    } else {
                        Some(String::from("/"))
                    }
                } else {
                    let mut path = String::from(self.path());
                    if !path.ends_with('/') { path.push('/'); }
                    path.push_str(name);
                    Some(path)
                }
            }
        }
    }

    pub fn target_display_name(&self, target: DropTarget) -> String {
        match target {
            DropTarget::Sidebar(idx) => String::from(Self::sidebar_name(idx)),
            DropTarget::Folder(i) => {
                if i < self.file_count {
                    String::from(self.files[i].display_name())
                } else {
                    String::from("Klasör")
                }
            }
        }
    }

    pub fn execute_drop(
        &mut self,
        src_idx: usize,
        target: DropTarget,
        is_copy: bool,
        vfs: &mut crate::fs::vfs::VirtualFileSystem,
    ) -> Result<String, &'static str> {
        if src_idx >= self.file_count {
            return Err(crate::gui::ui_text("Kaynak öğe bulunamadı.", "Source item not found."));
        }
        if self.files[src_idx].display_name() == ".." {
            return Err(crate::gui::ui_text("Üst klasör taşınamaz.", "Parent folder cannot be moved."));
        }

        let Some(target_dir) = self.target_directory(target) else {
            return Err(crate::gui::ui_text("Hedef klasör geçersiz.", "Invalid target folder."));
        };

        if target_dir == "/media/cdrom" {
            return Err(crate::gui::ui_text("CD-ROM salt okunurdur.", "CD-ROM is read-only."));
        }

        let target_disp = self.target_display_name(target);
        if self.selected_items[src_idx] { self.drag_items = self.selected_items; }
        else { self.drag_items = [false; MAX_ITEMS]; self.drag_items[src_idx] = true; }
        let mut count = 0;
        let mut first_name = String::new();
        for i in 0..self.file_count {
            if !self.drag_items[i] || self.files[i].display_name() == ".." { continue; }
            let src_name = self.files[i].display_name();
            if first_name.is_empty() { first_name.push_str(src_name); }
            let mut src_path = String::from(self.path());
            if !src_path.ends_with('/') { src_path.push('/'); }
            src_path.push_str(src_name);
            if is_copy && target_dir != crate::fs::vfs::TRASH_DIR { vfs.copy_item(&src_path, &target_dir)?; }
            else { vfs.move_item(&src_path, &target_dir)?; }
            count += 1;
        }
        if count == 0 { return Err(crate::gui::ui_text("Kaynak öğe bulunamadı.", "Source item not found.")); }

        let mut msg = TextBuf::new();
        if is_copy && target_dir != crate::fs::vfs::TRASH_DIR {
            msg.push_str(crate::gui::ui_text("Kopyalandı: ", "Copied: "));
        } else if target_dir == crate::fs::vfs::TRASH_DIR {
            msg.push_str(crate::gui::ui_text("Çöpe taşındı: ", "Moved to Trash: "));
        } else {
            msg.push_str(crate::gui::ui_text("Taşındı: ", "Moved: "));
        }
        msg.push_str(&first_name);
        if count > 1 { msg.push_str(" + "); msg.push_usize(count - 1); }
        msg.push_str(" -> ");
        msg.push_str(&target_disp);
        self.set_status_text(msg.as_str());

        Ok(target_dir)
    }

    pub fn draw_drag_badge(&self, fb: &mut Framebuffer, mx: usize, my: usize, is_copy: bool) {
        let Some(item_idx) = self.drag_item else { return; };
        if item_idx >= self.file_count { return; }
        let file = &self.files[item_idx];
        let name = file.display_name();

        let op_prefix = if is_copy {
            crate::gui::ui_text("+ Kopyala: ", "+ Copy: ")
        } else {
            crate::gui::ui_text("-> Taşı: ", "-> Move: ")
        };
        let accent_color = if is_copy { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE };

        let mut text = TextBuf::new();
        text.push_str(op_prefix);
        text.push_str(name);

        if let Some(target) = self.drag_target {
            text.push_str(" -> ");
            let target_name = self.target_display_name(target);
            text.push_str(&target_name);
        }

        let tw = MinGuiCompositor::text_width(text.as_str(), 1);
        let badge_w = tw + 28;
        let badge_h = 24;
        let badge_x = (mx + 14).min(fb.width.saturating_sub(badge_w + 4));
        let badge_y = (my + 14).min(fb.height.saturating_sub(badge_h + 4));

        fb.draw_rounded_rect(badge_x, badge_y, badge_w, badge_h, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(badge_x, badge_y, badge_w, badge_h, 4, accent_color);

        if file.is_dir {
            Self::draw_folder_icon(fb, badge_x + 5, badge_y + 4, false);
        } else {
            Self::draw_file_icon(fb, badge_x + 5, badge_y + 4, file.icon_tag);
        }

        MinGuiCompositor::draw_string(fb, badge_x + 24, badge_y + 5, text.as_str(), Color::TEXT_MAIN);
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
            "3D" => Color::ACCENT_PINK,
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

    /// Geri Dönüşüm Kutusu Simgesi: Kapaklı, çizgili bir çöp kutusu.
    pub fn draw_trash_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        // Kapak (üst yatay şerit)
        fb.draw_rect(x + 2, y + 1, 12, 2, Color::ACCENT_GREEN);
        // Kapak tutamacı (küçük çıkıntı)
        fb.draw_rect(x + 6, y, 4, 1, Color::ACCENT_GREEN);
        // Gövde (aşağı doğru daralan bir kutu)
        fb.draw_rect(x + 3, y + 4, 10, 10, MinGuiCompositor::shade(Color::ACCENT_GREEN, -40));
        // Gövde kenar çizgileri (dikey çizgiler)
        fb.draw_line(x + 6, y + 5, x + 6, y + 12, Color::ACCENT_GREEN);
        fb.draw_line(x + 10, y + 5, x + 10, y + 12, Color::ACCENT_GREEN);
        // Alt kenar
        fb.draw_line(x + 3, y + 13, x + 12, y + 13, Color::ACCENT_GREEN);
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize, scroll: &ScrollState, is_copy: bool) {
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
            ["Root (/)", "Desktop", "Documents", "Media", "Downloads", "System", "USB Drive", "Hard Disk", "CD-ROM", "Recycle Bin"]
        } else {
            ["Kök (/)", "Masaüstü", "Belgeler", "Medyalar", "İndirilenler", "Sistem", "USB Disk", "Sabit Disk", "CD-ROM", "Geri Dönüşüm"]
        };

        for (i, item) in nav.iter().enumerate() {
            let iy = win_y + 66 + i * 22;
            if iy + 8 > win_y + 60 + sidebar_h { break; }
            let cur = self.path();
            let is_active = match i {
                0 => cur == "/",
                1 => cur == "/desktop",
                2 => cur == "/root/Documents",
                3 => cur == "/root/Media",
                4 => cur == "/root/Downloads",
                5 => cur == "/boot",
                6 => cur == "/media/usb0",
                7 => cur == "/media/hdd0",
                8 => cur == "/media/cdrom",
                9 => cur == crate::fs::vfs::TRASH_DIR,
                _ => false,
            };
            let is_drag_target = self.drag_active && self.drag_target == Some(DropTarget::Sidebar(i));
            if is_drag_target {
                let target_color = if is_copy { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE };
                fb.draw_rounded_rect(win_x + 7, iy - 2, sidebar_w - 2, 20, 3, MinGuiCompositor::shade(Color::WINDOW_BG, 26));
                fb.draw_rounded_rect_outline(win_x + 7, iy - 2, sidebar_w - 2, 20, 3, target_color);
            } else if is_active {
                fb.draw_rect(win_x + 7, iy - 2, sidebar_w - 2, 20, Color::TITLE_BAR);
            }
            if i <= 5 {
                Self::draw_folder_icon(fb, win_x + 9, iy + 2, false);
            } else if i == 6 {
                // USB Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_GREEN);
                fb.draw_rect(win_x + 12, iy + 1, 8, 3, Color::WHITE);
            } else if i == 7 {
                // HDD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_YELLOW);
                fb.draw_line(win_x + 11, iy + 8, win_x + 21, iy + 8, Color::BLACK);
            } else if i == 8 {
                // CD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_PINK);
                fb.draw_rect(win_x + 14, iy + 6, 4, 4, Color::WHITE);
            } else {
                // Geri Dönüşüm Kutusu Simgesi (çöp kutusu)
                Self::draw_trash_icon(fb, win_x + 9, iy + 2);
            }
            let col = if is_drag_target {
                if is_copy { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE }
            } else if is_active {
                Color::ACCENT_BLUE
            } else if i == 6 {
                if diskfs::usb_available() { Color::ACCENT_GREEN } else { Color::TEXT_MUTED }
            } else if i == 7 {
                if crate::drivers::usb::hdd_available() || diskfs::usb_available() { Color::ACCENT_YELLOW } else { Color::TEXT_MUTED }
            } else if i == 8 {
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
        let col_size_w = list_w.saturating_sub(285);

        MinGuiCompositor::draw_string_clipped(fb, col_name_x, win_y + 65, crate::gui::ui_text("Ad", "Name"), Color::TEXT_MUTED,
            (col_name_x, win_y + 60, 145, 20));
        if list_w > 210 {
            MinGuiCompositor::draw_string_clipped(fb, col_type_x, win_y + 65, crate::gui::ui_text("Tür", "Type"), Color::TEXT_MUTED,
                (col_type_x, win_y + 60, 95, 20));
        }
        if list_w > 320 {
            let size_header = crate::gui::ui_text("Boyut", "Size");
            let header_x = col_size_x + col_size_w.saturating_sub(MinGuiCompositor::text_width(size_header, 1) + 6);
            MinGuiCompositor::draw_string_clipped(fb, header_x, win_y + 65, size_header, Color::TEXT_MUTED,
                (col_size_x, win_y + 60, col_size_w, 20));
        }

        // Dosya ve Klasör Listesi
        for i in 0..self.file_count {
            let row_y = list_top + i * ROW_H;
            let draw_y = row_y.saturating_sub(sy);
            if row_y + ROW_H <= list_top + sy || row_y >= list_top + sy + list_view_h {
                continue;
            }
            let file = &self.files[i];
            let is_sel = self.is_row_selected(i);
            let is_drag_target = self.drag_active && self.drag_target == Some(DropTarget::Folder(i));
            let is_drag_source = self.drag_active && self.drag_item == Some(i);

            if is_drag_target {
                let target_color = if is_copy { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE };
                fb.draw_rounded_rect(list_x, draw_y, list_w, ROW_H - 2, 3, MinGuiCompositor::shade(Color::WINDOW_BG, 26));
                fb.draw_rounded_rect_outline(list_x, draw_y, list_w, ROW_H - 2, 3, target_color);
            } else if is_drag_source {
                fb.draw_rect(list_x, draw_y, list_w, ROW_H - 2, MinGuiCompositor::shade(Color::WINDOW_BG, 14));
                fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::TEXT_MUTED);
            } else if is_sel {
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

            let name_col = if is_drag_target {
                if is_copy { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE }
            } else if is_sel {
                MinGuiCompositor::text_on(Color::TITLE_BAR)
            } else {
                Color::TEXT_MAIN
            };
            MinGuiCompositor::draw_string_clipped(fb, col_name_x, draw_y + 4, file.display_name(), name_col,
                (col_name_x, draw_y + 2, 145, ROW_H - 4));

            if list_w > 210 {
                MinGuiCompositor::draw_string_clipped(fb, col_type_x, draw_y + 4, file.file_type, Color::TEXT_MUTED,
                    (col_type_x, draw_y + 2, 95, ROW_H - 4));
            }
            if list_w > 320 {
                let size_text = file.size_str.as_str();
                let size_x = col_size_x + col_size_w.saturating_sub(MinGuiCompositor::text_width(size_text, 1) + 6);
                MinGuiCompositor::draw_string_clipped(fb, size_x, draw_y + 4, size_text, Color::TEXT_MAIN,
                    (col_size_x, draw_y + 2, col_size_w, ROW_H - 4));
            }
        }

        // Alt Durum Çubuğu
        let status_y = win_y + win_h - STATUS_H + 6;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, 18, Color::TASKBAR_BG);
        let mut sb = TextBuf::new();
        if self.rename_active {
            sb.push_str(crate::gui::ui_text("Yeni ad: ", "New name: "));
            if let Ok(name) = core::str::from_utf8(&self.rename_buf[..self.rename_len]) { sb.push_str(name); }
            sb.push_str(crate::gui::ui_text("  | Enter: kaydet  Esc: iptal", "  | Enter: save  Esc: cancel"));
        } else if !self.status_buf.is_empty() {
            sb.push_str(self.status_buf.as_str());
        } else if !self.status.is_empty() {
            sb.push_str(self.status);
        } else {
            sb.push_usize(self.file_count);
            sb.push_str(crate::gui::ui_text(" Öğe | ", " Items | "));
            sb.push_str(self.path());
        }
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, sb.as_str(), Color::TEXT_MUTED);
        if self.rename_active {
            let prefix = core::str::from_utf8(&self.rename_buf[..self.rename_cursor]).unwrap_or("");
            let cx = win_x + 12 + MinGuiCompositor::text_width(crate::gui::ui_text("Yeni ad: ", "New name: "), 1)
                + MinGuiCompositor::text_width(prefix, 1);
            fb.draw_rect(cx, status_y + 4, 1, 12, Color::ACCENT_BLUE);
        }
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
            if matches_ext(b"3d") || matches_ext(b"obj") {
                return "3D";
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

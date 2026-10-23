use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::scrollbar::{self, ScrollState};
use alloc::vec::Vec;

const INITIAL_BUFFER_CAPACITY: usize = 512;
const HISTORY_DEPTH: usize = 4;

/// Metin satır yüksekliği (px) — aktif font ölçeğine göre büyür.
#[inline(always)]
fn line_h() -> usize { crate::gui::sp(16) }
/// Karakter hücre ilerlemesi (px) — aktif font ölçeğine göre büyür.
#[inline(always)]
fn char_w() -> usize { MinGuiCompositor::terminal_text_width("M").max(1) }
/// Satır numarası şeridi (gutter) genişliği — font ölçeğine göre.
#[inline(always)]
fn gutter_w() -> usize { crate::gui::sp(44) }
/// Araç çubuğu yüksekliği (px) — font ölçeğine göre.
#[inline(always)]
fn toolbar_h() -> usize { crate::gui::sp(24) }
/// Alt durum çubuğu yüksekliği (px) — font ölçeğine göre.
#[inline(always)]
fn status_h() -> usize { crate::gui::sp(22) }

// --- Menü geometrisi ---
// Aşağıdaki yardımcılar, menü kutusunun çizimi ile fare tıklama testlerinin
// aynı koordinatları kullanmasını sağlar. Sabit (hard-coded) koordinatlar
// font ölçeği 100'den farklı olduğunda çizimle örtüşmüyordu.

pub struct NotepadApp {
    pub buffer: Vec<char>,
    pub len: usize,
    pub cursor: usize,
    // Selection range (inclusive start, exclusive end)
    pub selection_anchor: Option<usize>,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    // Show right‑click context menu
    pub show_context_menu: bool,
    // Scroll offset for content rendering
    pub scroll_offset: usize,
    // For double‑click detection
    pub last_click_tick: usize,
    pub last_click_pos: (usize, usize),
    pub click_streak: u8,
    pub menu: u8, // 0 kapalı, 1 Dosya, 2 Düzen, 3 Yardım
    pub file_path: [u8; 64],
    pub file_path_len: usize,
    pub status_msg: &'static str,
    /// Üzerine yaz modu (Insert tuşu ile değiştirilir). Açıkken yazılan
    /// karakter imlecin altındaki karakteri eklemek yerine değiştirir.
    pub overwrite: bool,
    // Geri al / ileri al (undo/redo) yığın sayacı. Gerçek anlık görüntüler
    // büyük bellek kapladığı için global `UNDO_STACK`/`REDO_STACK` içinde tutulur.
    undo_len: u8,
    redo_len: u8,
    history: History,
    // Find / Replace
    pub find_active: bool,
    pub replace_active: bool,
    pub find_text: [u8; 64],
    pub find_len: usize,
    pub replace_text: [u8; 64],
    pub replace_len: usize,
    pub find_focused_replace: bool,
}

/// Her not defteri penceresi için undo/redo geçmiş derinliği.
/// Metin anlık görüntüsü (undo/redo için).
#[derive(Clone)]
struct Snapshot {
    chars: Vec<char>,
    len: usize,
    cursor: usize,
}

struct History { undo: Vec<Snapshot>, redo: Vec<Snapshot> }

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum NotepadMenuAction {
    New,
    Open,
    Save,
    Clear,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Find,
    Replace,
    About,
    Shortcuts,
}

#[derive(Copy, Clone, Debug)]
pub struct MenuItemDef {
    pub label_tr: &'static str,
    pub label_en: &'static str,
    pub shortcut: &'static str,
    pub action: NotepadMenuAction,
}

const FILE_MENU_ITEMS: [MenuItemDef; 4] = [
    MenuItemDef { label_tr: "Yeni Belge", label_en: "New Document", shortcut: "Ctrl+N", action: NotepadMenuAction::New },
    MenuItemDef { label_tr: "Aç", label_en: "Open", shortcut: "Ctrl+O", action: NotepadMenuAction::Open },
    MenuItemDef { label_tr: "Kaydet", label_en: "Save", shortcut: "Ctrl+S", action: NotepadMenuAction::Save },
    MenuItemDef { label_tr: "Temizle", label_en: "Clear", shortcut: "", action: NotepadMenuAction::Clear },
];

const EDIT_MENU_ITEMS: [MenuItemDef; 9] = [
    MenuItemDef { label_tr: "Geri Al", label_en: "Undo", shortcut: "Ctrl+Z", action: NotepadMenuAction::Undo },
    MenuItemDef { label_tr: "Yinele", label_en: "Redo", shortcut: "Ctrl+Y", action: NotepadMenuAction::Redo },
    MenuItemDef { label_tr: "Kes", label_en: "Cut", shortcut: "Ctrl+X", action: NotepadMenuAction::Cut },
    MenuItemDef { label_tr: "Kopyala", label_en: "Copy", shortcut: "Ctrl+C", action: NotepadMenuAction::Copy },
    MenuItemDef { label_tr: "Yapıştır", label_en: "Paste", shortcut: "Ctrl+V", action: NotepadMenuAction::Paste },
    MenuItemDef { label_tr: "Sil", label_en: "Delete", shortcut: "Del", action: NotepadMenuAction::Delete },
    MenuItemDef { label_tr: "Tümünü Seç", label_en: "Select All", shortcut: "Ctrl+A", action: NotepadMenuAction::SelectAll },
    MenuItemDef { label_tr: "Bul...", label_en: "Find...", shortcut: "Ctrl+F", action: NotepadMenuAction::Find },
    MenuItemDef { label_tr: "Değiştir...", label_en: "Replace...", shortcut: "Ctrl+H", action: NotepadMenuAction::Replace },
];

const HELP_MENU_ITEMS: [MenuItemDef; 2] = [
    MenuItemDef { label_tr: "Not Defteri Hakkında", label_en: "About Notepad", shortcut: "", action: NotepadMenuAction::About },
    MenuItemDef { label_tr: "Kısayol Tuşları", label_en: "Shortcuts", shortcut: "", action: NotepadMenuAction::Shortcuts },
];

impl NotepadApp {
    // --- Menü geometrisi ---
    // Çizim (render) ile fare tıklama testlerinin aynı koordinatları
    // kullanmasını sağlayan ortak yardımcılar.

    pub fn menu_items(menu: u8) -> &'static [MenuItemDef] {
        match menu {
            1 => &FILE_MENU_ITEMS,
            2 => &EDIT_MENU_ITEMS,
            3 => &HELP_MENU_ITEMS,
            _ => &[],
        }
    }

    /// Menü öğesi yüksekliği (px) — font ölçeğine göre.
    #[inline(always)]
    pub fn menu_item_h() -> usize { crate::gui::sp(22) }
    /// Menü kutusunun iç (dolgu) boşluğu (px).
    #[inline(always)]
    pub fn menu_pad() -> usize { crate::gui::sp(4) }
    /// Menü kutusu genişliği (px).
    #[inline(always)]
    pub fn menu_box_w() -> usize { crate::gui::sp(180) }

    /// Menü kutusunun sol üst köşesinin pencere içi X koordinatı.
    #[inline(always)]
    pub fn menu_box_x(menu: u8) -> usize {
        Self::menu_tab_rect(menu).0
    }

    /// Menü kutusunun sol üst köşesinin pencere içi Y koordinatı.
    #[inline(always)]
    pub fn menu_box_y() -> usize { crate::gui::sp(54) }

    /// Menü öğesi sayısı.
    #[inline(always)]
    pub fn menu_item_count(menu: u8) -> usize {
        Self::menu_items(menu).len()
    }

    /// Menü kutusunun yüksekliği (px).
    #[inline(always)]
    pub fn menu_box_h(menu: u8) -> usize {
        Self::menu_pad() * 2 + Self::menu_item_count(menu) * Self::menu_item_h()
    }

    /// Araç çubuğundaki Dosya/Düzen/Yardım başlık düğmelerinin
    /// (pencere içi x, genişlik) sınırları.
    #[inline(always)]
    pub fn menu_tab_rect(menu: u8) -> (usize, usize) {
        match menu {
            1 => (crate::gui::sp(8), crate::gui::sp(54)),
            2 => (crate::gui::sp(66), crate::gui::sp(56)),
            _ => (crate::gui::sp(126), crate::gui::sp(64)),
        }
    }

    /// Menü açıkken bir öğenin pencere içi dikey aralığı (üst, alt).
    #[inline(always)]
    pub fn menu_item_bounds(menu: u8, item: usize) -> (usize, usize) {
        let _ = menu;
        let top = Self::menu_box_y() + Self::menu_pad() + item * Self::menu_item_h();
        (top, top + Self::menu_item_h())
    }

    /// Menü öğesinin tıklanabilir / aktif olup olmadığını döndürür.
    pub fn is_item_enabled(&self, action: NotepadMenuAction) -> bool {
        match action {
            NotepadMenuAction::Undo => self.undo_len > 0,
            NotepadMenuAction::Redo => self.redo_len > 0,
            NotepadMenuAction::Cut | NotepadMenuAction::Copy => self.has_selection(),
            NotepadMenuAction::Paste => crate::gui::clipboard::len() > 0,
            NotepadMenuAction::Delete => self.has_selection() || self.cursor < self.len,
            _ => true,
        }
    }

    /// Seçim varsa seçimi, yoksa imleç önündeki karakteri siler.
    pub fn delete_action(&mut self) {
        if self.has_selection() {
            self.delete_selection();
        } else {
            self.delete_forward();
        }
    }

    /// Menü aksiyonunu yürütür.
    pub fn execute_action(&mut self, action: NotepadMenuAction, vfs: &mut crate::fs::vfs::VirtualFileSystem) {
        match action {
            NotepadMenuAction::New => {
                self.clear();
                self.status_msg = crate::gui::ui_text("Yeni belge oluşturuldu", "New document created");
            }
            NotepadMenuAction::Open => {
                let path = self.file_path_str();
                if let Some(data) = vfs.read_file(path) {
                    let mut path_buf = [0u8; 64];
                    let p_len = self.file_path_len;
                    path_buf[..p_len].copy_from_slice(&self.file_path[..p_len]);
                    if let Ok(p_str) = core::str::from_utf8(&path_buf[..p_len]) {
                        self.open_file(p_str, &data);
                    }
                } else {
                    self.status_msg = crate::gui::ui_text("Dosya açılamadı!", "Could not open file!");
                }
            }
            NotepadMenuAction::Save => {
                self.save_file(vfs);
            }
            NotepadMenuAction::Clear => {
                self.clear();
            }
            NotepadMenuAction::Undo => {
                self.undo();
            }
            NotepadMenuAction::Redo => {
                self.redo();
            }
            NotepadMenuAction::Cut => {
                self.cut_selection();
            }
            NotepadMenuAction::Copy => {
                self.copy_selection();
            }
            NotepadMenuAction::Paste => {
                self.paste_clipboard();
            }
            NotepadMenuAction::Delete => {
                self.delete_action();
            }
            NotepadMenuAction::SelectAll => {
                self.select_all();
            }
            NotepadMenuAction::Find => {
                self.open_find();
            }
            NotepadMenuAction::Replace => {
                self.open_replace();
            }
            NotepadMenuAction::About => {
                self.status_msg = crate::gui::ui_text("minOs Not Defteri v1.3", "minOs Notepad v1.3");
            }
            NotepadMenuAction::Shortcuts => {
                self.status_msg = crate::gui::ui_text("Ctrl+N Yeni, Ctrl+O Aç, Ctrl+S Kaydet, Ctrl+Z/Y Geri/İleri", "Ctrl+N New, Ctrl+O Open, Ctrl+S Save, Ctrl+Z/Y Undo/Redo");
            }
        }
    }

    // Copy selected text to the clipboard
    pub fn copy_selection(&self) {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let range = start.min(end)..start.max(end);
            let len = (range.end - range.start).min(512);
            crate::gui::clipboard::set_chars(&self.buffer[range.start..range.start + len]);
        }
    }

    // Cut selected text (copy then delete)
    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    // Paste clipboard content at cursor
    pub fn paste_clipboard(&mut self) {
        self.delete_selection();
        self.push_history();
        let mut pasted = ['\0'; 512];
        let clipboard_len = crate::gui::clipboard::copy_chars(&mut pasted);
        let amount = clipboard_len;
        if self.len + amount > self.buffer.len() {
            let needed = self.len + amount - self.buffer.len();
            if self.buffer.try_reserve(needed).is_err() { return; }
            self.buffer.resize(self.len + amount, ' ');
        }
        for i in (self.cursor..self.len).rev() {
            self.buffer[i + amount] = self.buffer[i];
        }
        self.buffer[self.cursor..self.cursor + amount].copy_from_slice(&pasted[..amount]);
        self.len += amount;
        self.cursor += amount;
    }

    #[inline(always)]
    pub fn has_selection(&self) -> bool {
        match (self.selection_start, self.selection_end) {
            (Some(s), Some(e)) => s != e,
            _ => false,
        }
    }

    // Delete selected text
    pub fn delete_selection(&mut self) {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (s, e) = (start.min(end), start.max(end));
            let del_len = e - s;
            if del_len == 0 {
                self.selection_anchor = None;
                self.selection_start = None;
                self.selection_end = None;
                return;
            }
            self.push_history();
            // Shift remaining characters left
            for i in s..self.len - del_len {
                self.buffer[i] = self.buffer[i + del_len];
            }
            // Clear tail
            for i in (self.len - del_len)..self.len {
                self.buffer[i] = ' ';
            }
            self.len -= del_len;
            self.cursor = s.min(self.len);
            self.selection_anchor = None;
            self.selection_start = None;
            self.selection_end = None;
        }
        self.selection_anchor = None;
    }
}

impl NotepadApp {
    pub fn new() -> Self {
        let mut app = Self {
            buffer: alloc::vec![ ' '; INITIAL_BUFFER_CAPACITY ],
            len: 0,
            cursor: 0,
            selection_anchor: None,
            selection_start: None,
            selection_end: None,
            show_context_menu: false,
            scroll_offset: 0,
            last_click_tick: 0,
            last_click_pos: (0, 0),
            click_streak: 0,
            menu: 0,
            file_path: [0; 64],
            file_path_len: 0,
            status_msg: "",
            overwrite: false,
            undo_len: 0,
            redo_len: 0,
            history: History { undo: Vec::new(), redo: Vec::new() },
            find_active: false,
            replace_active: false,
            find_text: [0; 64],
            find_len: 0,
            replace_text: [0; 64],
            replace_len: 0,
            find_focused_replace: false,
        };

        let default_path = "/root/Documents/notlar.txt";
        for b in default_path.bytes() {
            if app.file_path_len < app.file_path.len() {
                app.file_path[app.file_path_len] = b;
                app.file_path_len += 1;
            }
        }

        let initial_text = "Not Defteri\n-----------\nYardım: https://github.com/tbagriyanik/minos\n\nKlavyenizle doğrudan metin yazabilirsiniz.\n(TR-Q ve EN-US düzenleri desteklenir)\n\nNotlarınızı buraya ekleyin:";
        for c in initial_text.chars() {
            if app.len < app.buffer.len() {
                app.buffer[app.len] = c;
                app.len += 1;
            }
        }
        app.cursor = app.len;
        app
    }

    /// Değişiklikten önce mevcut durumu geri al yığınına iter ve
    /// ileri al (redo) yığınını temizler.
    fn push_history(&mut self) {
        if self.history.undo.len() == HISTORY_DEPTH { self.history.undo.remove(0); }
        self.history.undo.push(self.snapshot());
        self.undo_len = self.history.undo.len() as u8;
        self.history.redo.clear();
        self.redo_len = 0;
    }

    fn snapshot(&self) -> Snapshot {
        let chars = self.buffer[..self.len].to_vec();
        Snapshot { chars, len: self.len, cursor: self.cursor }
    }

    fn restore(&mut self, s: &Snapshot) {
        let capacity = self.buffer.len().max(INITIAL_BUFFER_CAPACITY).max(s.len);
        self.buffer.resize(capacity, ' ');
        self.buffer[..s.len].copy_from_slice(&s.chars[..s.len]);
        self.buffer[s.len..].fill(' ');
        self.len = s.len;
        self.cursor = s.cursor.min(s.len);
        self.selection_anchor = None;
        self.selection_start = None;
        self.selection_end = None;
    }

    /// Son düzenlemeyi geri alır (Ctrl+Z).
    pub fn undo(&mut self) {
        if self.undo_len == 0 {
            return;
        }
        let cur = self.snapshot();
        let prev = self.history.undo.pop().unwrap();
        if self.history.redo.len() == HISTORY_DEPTH { self.history.redo.remove(0); }
        self.history.redo.push(cur);
        self.undo_len = self.history.undo.len() as u8;
        self.redo_len = self.history.redo.len() as u8;
        self.restore(&prev);
    }

    /// Geri alınan düzenlemeyi yeniden uygular (Ctrl+Y).
    pub fn redo(&mut self) {
        if self.redo_len == 0 {
            return;
        }
        let cur = self.snapshot();
        let next = self.history.redo.pop().unwrap();
        if self.history.undo.len() == HISTORY_DEPTH { self.history.undo.remove(0); }
        self.history.undo.push(cur);
        self.undo_len = self.history.undo.len() as u8;
        self.redo_len = self.history.redo.len() as u8;
        self.restore(&next);
    }

    pub fn insert_char(&mut self, c: char) {
        let had_selection = self.has_selection();
        if had_selection {
            self.delete_selection();
        } else {
            self.push_history();
        }
        // Üzerine yaz modu: seçim yokken satır ortasında karakteri eklemek yerine değiştir.
        if !had_selection && self.overwrite && c != '\n' && self.cursor < self.len && self.buffer[self.cursor] != '\n' {
            self.buffer[self.cursor] = c;
            self.cursor += 1;
            return;
        }
        self.insert_raw(c);
    }

    /// Geçmiş kaydı ve seçim silme yapmadan imlece tek karakter ekler (dahili).
    fn insert_raw(&mut self, c: char) {
        if self.len == self.buffer.len() {
            if self.buffer.try_reserve(1).is_err() { return; }
            self.buffer.push(' ');
        }
        for i in (self.cursor..self.len).rev() { self.buffer[i + 1] = self.buffer[i]; }
        self.buffer[self.cursor] = c;
        self.len += 1;
        self.cursor += 1;
    }

    /// Insert tuşu: üzerine yaz modunu aç/kapat.
    pub fn toggle_overwrite(&mut self) {
        self.overwrite = !self.overwrite;
        self.status_msg = if self.overwrite {
            crate::gui::ui_text("Üzerine yaz modu: AÇIK", "Overwrite mode: ON")
        } else {
            crate::gui::ui_text("Üzerine yaz modu: KAPALI", "Overwrite mode: OFF")
        };
    }

    /// Tab: imlece 4 boşluk girintisi ekler.
    pub fn insert_indent(&mut self) {
        self.push_history();
        self.delete_selection();
        for _ in 0..4 { self.insert_raw(' '); }
    }

    /// Shift+Tab: geçerli satırın başındaki girintiyi (en çok 4 boşluk) kaldırır.
    pub fn unindent(&mut self) {
        let mut start = self.cursor.min(self.len);
        while start > 0 && self.buffer[start - 1] != '\n' { start -= 1; }
        let mut remove = 0usize;
        while remove < 4 && start + remove < self.len && self.buffer[start + remove] == ' ' {
            remove += 1;
        }
        if remove == 0 { return; }
        self.push_history();
        self.selection_anchor = None;
        self.selection_start = None;
        self.selection_end = None;
        for i in start..self.len - remove {
            self.buffer[i] = self.buffer[i + remove];
        }
        for i in (self.len - remove)..self.len {
            self.buffer[i] = ' ';
        }
        self.len -= remove;
        self.cursor = self.cursor.saturating_sub(remove).max(start.min(self.len));
    }

    pub fn backspace(&mut self) {
        if self.has_selection() { self.delete_selection(); return; }
        if self.cursor > 0 {
            self.push_history();
            let deleted = self.cursor - 1;
            for i in deleted..self.len.saturating_sub(1) {
                self.buffer[i] = self.buffer[i + 1];
            }
            self.len -= 1;
            self.buffer[self.len] = ' ';
            self.cursor -= 1;
        }
    }

    pub fn delete_forward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
        } else if self.cursor < self.len {
            self.selection_anchor = Some(self.cursor);
            self.selection_start = Some(self.cursor);
            self.selection_end = Some(self.cursor + 1);
            self.delete_selection();
        }
    }

    pub fn delete_word_backward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.cursor == 0 { return; }
        let mut idx = self.cursor;
        while idx > 0 && self.buffer[idx - 1].is_whitespace() {
            idx -= 1;
        }
        if idx > 0 {
            let is_alnum = self.buffer[idx - 1].is_alphanumeric() || self.buffer[idx - 1] == '_';
            while idx > 0 {
                let prev = self.buffer[idx - 1];
                if prev.is_whitespace() { break; }
                if (prev.is_alphanumeric() || prev == '_') != is_alnum { break; }
                idx -= 1;
            }
        }
        self.selection_anchor = Some(self.cursor);
        self.selection_start = Some(idx);
        self.selection_end = Some(self.cursor);
        self.delete_selection();
    }

    pub fn delete_word_forward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.cursor >= self.len { return; }
        let mut idx = self.cursor;
        if idx < self.len {
            let is_alnum = self.buffer[idx].is_alphanumeric() || self.buffer[idx] == '_';
            let is_ws = self.buffer[idx].is_whitespace();
            if !is_ws {
                while idx < self.len {
                    let cur = self.buffer[idx];
                    if cur.is_whitespace() { break; }
                    if (cur.is_alphanumeric() || cur == '_') != is_alnum { break; }
                    idx += 1;
                }
            }
            while idx < self.len && self.buffer[idx].is_whitespace() && self.buffer[idx] != '\n' {
                idx += 1;
            }
        }
        self.selection_anchor = Some(self.cursor);
        self.selection_start = Some(self.cursor);
        self.selection_end = Some(idx);
        self.delete_selection();
    }

    /// İmleci satır başına taşır (Home tuşu). `select` true ise seçimi genişletir.
    pub fn move_home_ext(&mut self, select: bool) {
        let mut start = self.cursor;
        while start > 0 && self.buffer[start - 1] != '\n' { start -= 1; }
        self.move_to(start, select);
    }

    /// İmleci satır sonuna taşır (End tuşu). `select` true ise seçimi genişletir.
    pub fn move_end_ext(&mut self, select: bool) {
        let mut end = self.cursor;
        while end < self.len && self.buffer[end] != '\n' { end += 1; }
        self.move_to(end, select);
    }

    /// İmleci satır başına taşır (Home tuşu).
    pub fn move_home(&mut self) { self.move_home_ext(false); }

    /// İmleci satır sonuna taşır (End tuşu).
    pub fn move_end(&mut self) { self.move_end_ext(false); }

    /// Belgenin en başına taşır (Ctrl+Home).
    pub fn move_document_home(&mut self, select: bool) { self.move_to(0, select); }

    /// Belgenin en sonuna taşır (Ctrl+End).
    pub fn move_document_end(&mut self, select: bool) { self.move_to(self.len, select); }

    /// Bir kelimenin (harf/rakam/alt çizgi dizisi) başlangıç ve bitiş
    /// dizinini döndürür (çift tık kelime seçimi için).
    pub fn word_range_at(&self, idx: usize) -> (usize, usize) {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let idx = idx.min(self.len);
        // İmleç bir kelimenin sonundaysa o kelimeyi seç.
        let mut start = idx;
        while start > 0 && is_word(self.buffer[start - 1]) { start -= 1; }
        let mut end = idx;
        while end < self.len && is_word(self.buffer[end]) { end += 1; }
        (start, end)
    }

    /// Bir kelimeyi seçer (çift tık davranışı).
    pub fn select_word_at(&mut self, idx: usize) {
        let (s, e) = self.word_range_at(idx);
        if s == e {
            self.cursor = idx.min(self.len);
            self.selection_anchor = None;
            self.selection_start = None;
            self.selection_end = None;
        } else {
            self.selection_anchor = Some(s);
            self.selection_start = Some(s);
            self.selection_end = Some(e);
            self.cursor = e;
        }
    }

    /// Record a click in the same small hit area and return its sequence
    /// number: 1 (caret), 2 (word), or 3 (paragraph). A fourth click starts
    /// a new sequence.
    pub fn record_click(&mut self, now_ms: usize, pos: (usize, usize), interval_ms: usize) -> u8 {
        let same_target = now_ms.abs_diff(self.last_click_tick) <= interval_ms
            && self.last_click_pos.0.abs_diff(pos.0) < 4
            && self.last_click_pos.1.abs_diff(pos.1) < 4;
        self.click_streak = if same_target && self.click_streak < 3 {
            self.click_streak + 1
        } else { 1 };
        self.last_click_tick = now_ms;
        self.last_click_pos = pos;
        self.click_streak
    }

    /// Select the paragraph containing `idx`. Lines separated by a blank line
    /// form a paragraph; a lone line break remains inside the paragraph.
    pub fn select_paragraph_at(&mut self, idx: usize) {
        if self.len == 0 {
            self.selection_anchor = None;
            self.selection_start = None;
            self.selection_end = None;
            self.cursor = 0;
            return;
        }

        let idx = idx.min(self.len);
        let mut start = idx;
        while start > 0 && self.buffer[start - 1] != '\n' { start -= 1; }
        let mut end = idx;
        while end < self.len && self.buffer[end] != '\n' { end += 1; }

        // A blank line is itself the paragraph target; do not absorb the
        // preceding or following paragraph when the caret is on it.
        if start == end {
            let selection_end = if end < self.len { end + 1 } else { end };
            self.set_selection(start, selection_end);
            return;
        }

        while start > 0 {
            let previous_end = start - 1;
            let mut previous_start = previous_end;
            while previous_start > 0 && self.buffer[previous_start - 1] != '\n' { previous_start -= 1; }
            if previous_start == previous_end { break; }
            start = previous_start;
        }

        while end < self.len && self.buffer[end] == '\n' {
            let next_start = end + 1;
            if next_start >= self.len { end = self.len; break; }
            let mut next_end = next_start;
            while next_end < self.len && self.buffer[next_end] != '\n' { next_end += 1; }
            if next_start == next_end { break; }
            end = next_end;
        }
        if end < self.len && self.buffer[end] == '\n' { end += 1; }
        self.set_selection(start, end);
    }

    fn set_selection(&mut self, start: usize, end: usize) {
        let start = start.min(self.len);
        let end = end.min(self.len).max(start);
        self.selection_anchor = Some(start);
        self.selection_start = if start == end { None } else { Some(start) };
        self.selection_end = if start == end { None } else { Some(end) };
        self.cursor = end;
    }

    /// İmleci bir sonraki konuma taşır; `select` true ise seçimi genişletir.
    fn move_to(&mut self, target: usize, select: bool) {
        if select {
            let anchor = match self.selection_anchor {
                Some(a) => a,
                None => {
                    let a = self.cursor;
                    self.selection_anchor = Some(a);
                    a
                }
            };
            self.cursor = target;
            if anchor == target {
                self.selection_start = None;
                self.selection_end = None;
            } else {
                self.selection_start = Some(anchor.min(target));
                self.selection_end = Some(anchor.max(target));
            }
        } else {
            self.cursor = target;
            self.selection_anchor = None;
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn move_word_left_ext(&mut self, select: bool) {
        let base = match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) if start != end && !select => start.min(end),
            _ => self.cursor,
        };
        let mut idx = base;
        while idx > 0 && self.buffer[idx - 1].is_whitespace() {
            idx -= 1;
        }
        if idx > 0 {
            let is_alnum = self.buffer[idx - 1].is_alphanumeric() || self.buffer[idx - 1] == '_';
            while idx > 0 {
                let prev = self.buffer[idx - 1];
                if prev.is_whitespace() { break; }
                if (prev.is_alphanumeric() || prev == '_') != is_alnum { break; }
                idx -= 1;
            }
        }
        self.move_to(idx, select);
    }

    pub fn move_word_right_ext(&mut self, select: bool) {
        let base = match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) if start != end && !select => start.max(end),
            _ => self.cursor,
        };
        let mut idx = base;
        if idx < self.len {
            let is_alnum = self.buffer[idx].is_alphanumeric() || self.buffer[idx] == '_';
            let is_ws = self.buffer[idx].is_whitespace();
            if !is_ws {
                while idx < self.len {
                    let cur = self.buffer[idx];
                    if cur.is_whitespace() { break; }
                    if (cur.is_alphanumeric() || cur == '_') != is_alnum { break; }
                    idx += 1;
                }
            }
            while idx < self.len && self.buffer[idx].is_whitespace() && self.buffer[idx] != '\n' {
                idx += 1;
            }
        }
        self.move_to(idx, select);
    }

    pub fn move_left_ext(&mut self, select: bool) {
        let target = match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) if start != end && !select => start.min(end),
            _ => self.cursor.saturating_sub(1),
        };
        self.move_to(target, select);
    }

    pub fn move_right_ext(&mut self, select: bool) {
        let target = match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) if start != end && !select => start.max(end),
            _ => (self.cursor + 1).min(self.len),
        };
        self.move_to(target, select);
    }

    pub fn move_left(&mut self) { self.move_left_ext(false); }

    pub fn move_right(&mut self) { self.move_right_ext(false); }

    pub fn move_vertical(&mut self, up: bool) { self.move_vertical_ext(up, false); }

    /// Bir konumdan bir satır yukarı/aşağı gidilecek indeksi hesaplar
    /// (sütun korunur, mutasyon yapmaz).
    fn vertical_target(&self, from: usize, up: bool) -> usize {
        let mut line_start = from;
        while line_start > 0 && self.buffer[line_start - 1] != '\n' { line_start -= 1; }
        let column = from - line_start;
        if up {
            if line_start == 0 {
                0
            } else {
                let previous_end = line_start - 1;
                let mut previous_start = previous_end;
                while previous_start > 0 && self.buffer[previous_start - 1] != '\n' { previous_start -= 1; }
                (previous_start + column).min(previous_end)
            }
        } else {
            let mut line_end = from;
            while line_end < self.len && self.buffer[line_end] != '\n' { line_end += 1; }
            if line_end == self.len {
                self.len
            } else {
                let next_start = line_end + 1;
                let mut next_end = next_start;
                while next_end < self.len && self.buffer[next_end] != '\n' { next_end += 1; }
                (next_start + column).min(next_end)
            }
        }
    }

    pub fn move_vertical_ext(&mut self, up: bool, select: bool) {
        let target = self.vertical_target(self.cursor, up);
        self.move_to(target, select);
    }

    /// PageUp/PageDown: `lines` satır yukarı/aşağı (sütun korunur).
    pub fn move_page_ext(&mut self, up: bool, lines: usize, select: bool) {
        let mut target = self.cursor;
        for _ in 0..lines.max(1) {
            let next = self.vertical_target(target, up);
            if next == target { break; }
            target = next;
        }
        self.move_to(target, select);
    }

    /// İmlecin görsel (wrap'li) satır ve sütun konumunu döndürür.
    pub fn cursor_row_col(&self, wrap_cols: usize) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for i in 0..self.cursor.min(self.len) {
            let ch = self.buffer[i];
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
                if wrap_cols > 0 && col >= wrap_cols { line += 1; col = 0; }
            }
        }
        (line, col)
    }

    /// İmleci görünür tutacak dikey kaydırma ofsetini (px) döndürür.
    /// Render ile aynı geometriyi kullanır, böylece imleç her zaman ekranda kalır.
    pub fn ensure_cursor_visible(&self, win_w: usize, win_h: usize, current_scroll_y: usize) -> usize {
        let area_h = win_h.saturating_sub(58 + status_h() + scrollbar::SCROLLBAR_THICKNESS);
        let area_w = win_w.saturating_sub(12 + scrollbar::SCROLLBAR_THICKNESS);
        let text_w = area_w.saturating_sub(gutter_w() + 10);
        let wrap_cols = (text_w / char_w()).max(1);
        let (total_lines, _) = self.text_extent(wrap_cols);
        let (row, _) = self.cursor_row_col(wrap_cols);
        let view_lines = (area_h / line_h()).max(1);
        let max_line_off = total_lines.saturating_sub(view_lines);
        let mut first = (current_scroll_y / line_h()).min(max_line_off);
        if row < first {
            first = row;
        } else if row >= first + view_lines {
            first = (row + 1).saturating_sub(view_lines);
        }
        first.min(max_line_off) * line_h()
    }

    /// Görünür (wrap'li) satır sayısı — PageUp/PageDown adımı bu kadar satır atlar.
    pub fn page_lines(&self, win_h: usize) -> usize {
        let area_h = win_h.saturating_sub(58 + status_h() + scrollbar::SCROLLBAR_THICKNESS);
        (area_h / line_h()).max(1)
    }

    pub fn clear(&mut self) {
        self.push_history();
        self.click_streak = 0;
        self.len = 0;
        self.cursor = 0;
        for c in self.buffer.iter_mut() {
            *c = ' ';
        }
        self.selection_anchor = None;
        self.selection_start = None;
        self.selection_end = None;
        self.status_msg = "";
    }

    pub fn open_file(&mut self, path: &str, content: &[u8]) {
        self.clear();
        self.file_path_len = 0;
        for b in path.bytes() {
            if self.file_path_len < self.file_path.len() {
                self.file_path[self.file_path_len] = b;
                self.file_path_len += 1;
            }
        }
        if let Ok(text) = core::str::from_utf8(content) {
            let needed = text.chars().count();
            if needed > self.buffer.len() {
                if self.buffer.try_reserve(needed - self.buffer.len()).is_err() {
                    self.status_msg = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
                    return;
                }
                self.buffer.resize(needed, ' ');
            }
            for c in text.chars() {
                self.buffer[self.len] = c;
                self.len += 1;
            }
        } else {
            if content.len() > self.buffer.len() {
                if self.buffer.try_reserve(content.len() - self.buffer.len()).is_err() {
                    self.status_msg = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
                    return;
                }
                self.buffer.resize(content.len(), ' ');
            }
            for &b in content {
                self.buffer[self.len] = b as char;
                self.len += 1;
            }
        }
        self.cursor = 0;
        self.status_msg = crate::gui::ui_text("Dosya açıldı", "File opened");
    }

    pub fn save_file(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem) -> bool {
        let path = self.file_path_str();
        let mut utf8_buf = Vec::new();
        if utf8_buf.try_reserve(self.len.saturating_mul(4)).is_err() {
            self.status_msg = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
            return false;
        }
        for i in 0..self.len {
            let mut char_buf = [0u8; 4];
            let s = self.buffer[i].encode_utf8(&mut char_buf);
            utf8_buf.extend_from_slice(s.as_bytes());
        }
        let ok = vfs.write_file(path, &utf8_buf);
        if ok {
            self.status_msg = crate::gui::ui_text("Dosya başarıyla kaydedildi!", "File saved successfully!");
        } else {
            self.status_msg = crate::gui::ui_text("Kaydetme başarısız!", "Save failed!");
        }
        ok
    }

    pub fn file_path_str(&self) -> &str {
        if self.file_path_len > 0 {
            core::str::from_utf8(&self.file_path[..self.file_path_len]).unwrap_or("/root/Documents/notlar.txt")
        } else {
            "/root/Documents/notlar.txt"
        }
    }

    /// Tüm metni seçer (Kopyala/Kes/Sil ile birlikte kullanılır).
    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.selection_start = Some(0);
        self.selection_end = Some(self.len);
        self.cursor = self.len;
    }
    pub fn open_find(&mut self) {
        self.find_active = true;
        self.replace_active = false;
        self.find_focused_replace = false;
        self.status_msg = crate::gui::ui_text("Metin Ara (Enter: Sonraki, Esc: Kapat)", "Find Text (Enter: Next, Esc: Close)");
    }

    pub fn open_replace(&mut self) {
        self.find_active = true;
        self.replace_active = true;
        self.status_msg = crate::gui::ui_text("Bul ve Değiştir (Tab: Geçiş, Enter: Değiştir)", "Find & Replace (Tab: Switch, Enter: Replace)");
    }

    pub fn close_find(&mut self) {
        self.find_active = false;
        self.replace_active = false;
        self.status_msg = "";
    }

    pub fn find_str(&self) -> &str {
        core::str::from_utf8(&self.find_text[..self.find_len]).unwrap_or("")
    }

    pub fn replace_str(&self) -> &str {
        core::str::from_utf8(&self.replace_text[..self.replace_len]).unwrap_or("")
    }

    pub fn find_input_char(&mut self, ch: char) {
        let is_rep = self.find_focused_replace;
        let (buf, len) = if is_rep {
            (&mut self.replace_text, &mut self.replace_len)
        } else {
            (&mut self.find_text, &mut self.find_len)
        };
        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b);
        for byte in s.bytes() {
            if *len < buf.len() {
                buf[*len] = byte;
                *len += 1;
            }
        }
    }

    pub fn find_input_backspace(&mut self) {
        let is_rep = self.find_focused_replace;
        let len = if is_rep { &mut self.replace_len } else { &mut self.find_len };
        if *len > 0 {
            *len -= 1;
        }
    }

    pub fn find_next(&mut self, forward: bool) -> bool {
        let needle = self.find_str();
        if needle.is_empty() || self.len == 0 {
            return false;
        }
        let needle_chars: alloc::vec::Vec<char> = needle.chars().collect();
        let n_len = needle_chars.len();
        if n_len > self.len {
            self.status_msg = crate::gui::ui_text("Eşleşme bulunamadı", "No match found");
            return false;
        }

        let start_pos = if forward {
            if let Some(e) = self.selection_end { e.min(self.len) } else { self.cursor }
        } else {
            if let Some(s) = self.selection_start { s.saturating_sub(1) } else { self.cursor.saturating_sub(1) }
        };

        let mut found = None;
        if forward {
            // Check from start_pos to end
            for i in start_pos..=(self.len.saturating_sub(n_len)) {
                let mut match_ok = true;
                for j in 0..n_len {
                    if self.buffer[i + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                        match_ok = false;
                        break;
                    }
                }
                if match_ok {
                    found = Some(i);
                    break;
                }
            }
            // Wrap around
            if found.is_none() && start_pos > 0 {
                for i in 0..start_pos.min(self.len.saturating_sub(n_len) + 1) {
                    let mut match_ok = true;
                    for j in 0..n_len {
                        if self.buffer[i + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                            match_ok = false;
                            break;
                        }
                    }
                    if match_ok {
                        found = Some(i);
                        break;
                    }
                }
            }
        } else {
            // Backward search
            let check_limit = start_pos.min(self.len.saturating_sub(n_len));
            for i in (0..=check_limit).rev() {
                let mut match_ok = true;
                for j in 0..n_len {
                    if self.buffer[i + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                        match_ok = false;
                        break;
                    }
                }
                if match_ok {
                    found = Some(i);
                    break;
                }
            }
            if found.is_none() && self.len >= n_len {
                for i in (start_pos..=(self.len - n_len)).rev() {
                    let mut match_ok = true;
                    for j in 0..n_len {
                        if self.buffer[i + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                            match_ok = false;
                            break;
                        }
                    }
                    if match_ok {
                        found = Some(i);
                        break;
                    }
                }
            }
        }

        if let Some(pos) = found {
            self.selection_anchor = Some(pos);
            self.selection_start = Some(pos);
            self.selection_end = Some(pos + n_len);
            self.cursor = pos + n_len;
            self.status_msg = crate::gui::ui_text("Eşleşme bulundu", "Match found");
            true
        } else {
            self.status_msg = crate::gui::ui_text("Eşleşme bulunamadı", "No match found");
            false
        }
    }

    pub fn replace_current(&mut self) -> bool {
        let needle = self.find_str();
        if needle.is_empty() { return false; }
        let needle_chars: alloc::vec::Vec<char> = needle.chars().collect();
        let rep_chars: alloc::vec::Vec<char> = self.replace_str().chars().collect();

        // Check if current selection matches needle
        let is_match = if let (Some(s), Some(e)) = (self.selection_start, self.selection_end) {
            if e - s == needle_chars.len() {
                let mut ok = true;
                for j in 0..needle_chars.len() {
                    if self.buffer[s + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                        ok = false;
                        break;
                    }
                }
                ok
            } else {
                false
            }
        } else {
            false
        };

        if is_match {
            let s = self.selection_start.unwrap();
            let e = self.selection_end.unwrap();
            self.push_history();
            // Delete selection
            let del_len = e - s;
            for i in s..self.len - del_len {
                self.buffer[i] = self.buffer[i + del_len];
            }
            self.len -= del_len;
            // Insert replacement
            let add_len = rep_chars.len();
            if self.len + add_len > self.buffer.len() {
                let needed = self.len + add_len - self.buffer.len();
                if self.buffer.try_reserve(needed).is_err() { return false; }
                self.buffer.resize(self.len + add_len, ' ');
            }
            for i in (s..self.len).rev() {
                self.buffer[i + add_len] = self.buffer[i];
            }
            self.buffer[s..s + add_len].copy_from_slice(&rep_chars[..add_len]);
            self.len += add_len;
            self.cursor = s + add_len;
            self.selection_anchor = Some(s);
            self.selection_start = Some(s);
            self.selection_end = Some(s + add_len);
            self.status_msg = crate::gui::ui_text("Değiştirildi", "Replaced");
            // Find next
            self.find_next(true);
            true
        } else {
            // Find next first
            self.find_next(true)
        }
    }

    pub fn replace_all(&mut self) -> usize {
        let needle = self.find_str();
        if needle.is_empty() { return 0; }
        let needle_chars: alloc::vec::Vec<char> = needle.chars().collect();
        let rep_chars: alloc::vec::Vec<char> = self.replace_str().chars().collect();
        let n_len = needle_chars.len();
        if n_len == 0 { return 0; }

        self.push_history();
        let mut count = 0usize;
        let mut i = 0usize;
        while i + n_len <= self.len {
            let mut match_ok = true;
            for j in 0..n_len {
                if self.buffer[i + j].to_ascii_lowercase() != needle_chars[j].to_ascii_lowercase() {
                    match_ok = false;
                    break;
                }
            }
            if match_ok {
                // replace at i
                for k in i..self.len - n_len {
                    self.buffer[k] = self.buffer[k + n_len];
                }
                self.len -= n_len;

                let add_len = rep_chars.len();
                if self.len + add_len > self.buffer.len() {
                    let needed = self.len + add_len - self.buffer.len();
                    if self.buffer.try_reserve(needed).is_err() { break; }
                    self.buffer.resize(self.len + add_len, ' ');
                }
                for k in (i..self.len).rev() {
                    self.buffer[k + add_len] = self.buffer[k];
                }
                self.buffer[i..i + add_len].copy_from_slice(&rep_chars[..add_len]);
                self.len += add_len;
                i += add_len;
                count += 1;
            } else {
                i += 1;
            }
        }
        self.cursor = self.cursor.min(self.len);
        self.selection_anchor = None;
        self.selection_start = None;
        self.selection_end = None;
        self.status_msg = if count > 0 {
            crate::gui::ui_text("Tüm eşleşmeler değiştirildi", "All matches replaced")
        } else {
            crate::gui::ui_text("Eşleşme bulunamadı", "No match found")
        };
        count
    }

    /// Metin alanı başlangıcının pencere içi X koordinatı (gutter + boşluk).
    #[inline(always)]
    pub fn text_x() -> usize { gutter_w() + crate::gui::sp(12) }
    /// Bul/Değiştir çubuğunun pencere içi (x, y, w, h) dikdörtgeni.
    /// `render_content` ile aynı geometriyi kullanır (`by = win_y + 56`,
    /// `bw = win_w - 12`).
    pub fn find_bar_area_rect(win_w: usize, replace_active: bool) -> (usize, usize, usize, usize) {
        let bar_h = if replace_active { crate::gui::sp(58) } else { crate::gui::sp(30) };
        (6, 56, win_w.saturating_sub(12), bar_h)
    }

    /// Bul/Değiştir çubuğunun kapatma (X) düğmesi — çizimle aynı konum:
    /// sağ kenara dayalı, `bw - 20`.
    pub fn find_close_rect(win_w: usize) -> (usize, usize, usize, usize) {
        let bw = win_w.saturating_sub(12);
        let close_x = bw.saturating_sub(20);
        (close_x, 56 + 4, 20, 20)
    }

    /// "Bul" giriş kutusu dikdörtgeni.
    pub fn find_input_rect() -> (usize, usize, usize, usize) {
        (crate::gui::sp(50), 56 + 4, crate::gui::sp(140), 20)
    }

    /// "Sonraki" düğmesi.
    pub fn find_next_rect() -> (usize, usize, usize, usize) {
        let f_box_x = crate::gui::sp(50);
        let input_w = crate::gui::sp(140);
        (f_box_x + input_w + 8, 56 + 4, 60, 20)
    }

    /// "Önceki" düğmesi.
    pub fn find_prev_rect() -> (usize, usize, usize, usize) {
        let f_box_x = crate::gui::sp(50);
        let input_w = crate::gui::sp(140);
        (f_box_x + input_w + 74, 56 + 4, 60, 20)
    }

    /// "Değiştir" giriş kutusu (replace aktifse).
    pub fn replace_input_rect() -> (usize, usize, usize, usize) {
        (crate::gui::sp(50), 56 + 34, crate::gui::sp(140), 20)
    }

    /// "Değiştir" düğmesi.
    pub fn replace_btn_rect() -> (usize, usize, usize, usize) {
        let f_box_x = crate::gui::sp(50);
        let input_w = crate::gui::sp(140);
        (f_box_x + input_w + 8, 56 + 34, 60, 20)
    }

    /// "Tümünü Değiştir" düğmesi.
    pub fn replace_all_rect() -> (usize, usize, usize, usize) {
        let f_box_x = crate::gui::sp(50);
        let input_w = crate::gui::sp(140);
        (f_box_x + input_w + 74, 56 + 34, 76, 20)
    }

    /// Metin alanı başlangıcının pencere içi Y koordinatı (araç çubuğu + boşluk).
    #[inline(always)]
    pub fn text_y(&self) -> usize {
        if self.find_active {
            if self.replace_active { crate::gui::sp(120) } else { crate::gui::sp(92) }
        } else {
            crate::gui::sp(62)
        }
    }

    /// Pencere içi fare (x, y) koordinatını metin indeksine çevirir.
    /// `win_w` satır kaydırma (wrap) hesabı için, `scroll_y` dikey kaydırma
    /// ofseti için kullanılır. Windows Notepad gibi tıklanan karaktere imleci
    /// konumlandırır.
    pub fn index_at_scrolled(
        &self,
        x: usize,
        y: usize,
        win_w: usize,
        scroll_y: usize,
    ) -> usize {
        // Metin alanı genişliği ve satır başına düşen karakter sayısı.
        let text_w = win_w
            .saturating_sub(crate::gui::sp(12) + gutter_w() + crate::gui::sp(16) + crate::gui::scrollbar::SCROLLBAR_THICKNESS);
        let wrap_cols = (text_w / char_w()).max(1);

        let click_col = x.saturating_sub(Self::text_x()) / char_w();
        // Kaydırma ofseti satır cinsindendir.
        let scroll_lines = scroll_y / line_h();
        let click_row = scroll_lines + (y.saturating_sub(self.text_y())) / line_h();

        // Satır kaydırmayı (wrap) hesaba katarak istenen satır/sütuna ulaş.
        let mut line = 0usize;
        let mut col = 0usize;
        let mut idx = 0usize;
        while idx < self.len {
            if line == click_row && col >= click_col { break; }
            let ch = self.buffer[idx];
            if ch == '\n' {
                if line == click_row { break; }
                line += 1;
                col = 0;
            } else {
                col += 1;
                if col >= wrap_cols {
                    if line == click_row { idx += 1; break; }
                    line += 1;
                    col = 0;
                }
            }
            idx += 1;
        }
        idx.min(self.len)
    }

    /// Eski imza (kaydırma/wrap bilgisi olmadan). Geriye dönük uyumluluk için.
    pub fn index_at(&self, x: usize, y: usize, win_x: usize, win_y: usize) -> usize {
        let _ = win_y;
        self.index_at_scrolled(x.saturating_sub(win_x), y.saturating_sub(win_y), 460, 0)
    }

    /// Metnin kapladığı toplam satır sayısı ve en uzun satır uzunluğu (px).
    /// Kaydırma çubuğu gereksinimini hesaplamak için kullanılır.
    pub fn text_extent(&self, wrap_cols: usize) -> (usize, usize) {
        let mut lines = 1usize;
        let mut col = 0usize;
        let mut max_cols = 0usize;
        for i in 0..self.len {
            let ch = self.buffer[i];
            if ch == '\n' {
                max_cols = max_cols.max(col);
                lines += 1;
                col = 0;
            } else {
                col += 1;
                if wrap_cols > 0 && col >= wrap_cols {
                    max_cols = max_cols.max(col);
                    lines += 1;
                    col = 0;
                }
            }
        }
        max_cols = max_cols.max(col);
        (lines, max_cols)
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        tick: usize,
        scroll: &ScrollState,
    ) {
        // Araç Çubuğu (Toolbar)
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, toolbar_h(), Color::TITLE_BAR);
        let english = crate::gui::ui_english();

        let tab_defs = [
            (1u8, crate::gui::ui_text("Dosya", "File")),
            (2u8, crate::gui::ui_text("Düzen", "Edit")),
            (3u8, crate::gui::ui_text("Yardım", "Help")),
        ];
        for &(m_id, label) in &tab_defs {
            let (tx, tw) = Self::menu_tab_rect(m_id);
            if self.menu == m_id {
                fb.draw_rect(win_x + tx, win_y + 32, tw, toolbar_h().saturating_sub(4), Color::WINDOW_BG);
                fb.draw_rect_outline(win_x + tx, win_y + 32, tw, toolbar_h().saturating_sub(4), Color::BORDER);
            }
            MinGuiCompositor::draw_string(fb, win_x + tx + crate::gui::sp(6), win_y + 36, label, Color::TEXT_MAIN);
        }

        // Hızlı Aksiyon Düğmeleri (sağa hizalı)
        fb.draw_rect(win_x + win_w.saturating_sub(130), win_y + 32, 56, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(124), win_y + 36, crate::gui::ui_text("Temizle", "Clear"), Color::ACCENT_RED);
        fb.draw_rect(win_x + win_w.saturating_sub(68), win_y + 32, 56, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(60), win_y + 36, crate::gui::ui_text("Kaydet", "Save"), MinGuiCompositor::text_on(Color::ACCENT_BLUE));

        // Metin Editörü Alanı — pencere boyutuna göre DİNAMİK
        // Bul / Değiştir Çubuğu (Find & Replace Bar)
        let bar_h = if self.find_active {
            if self.replace_active { crate::gui::sp(58) } else { crate::gui::sp(30) }
        } else {
            0
        };

        if self.find_active {
            let by = win_y + 56;
            let bw = win_w.saturating_sub(12);
            fb.draw_rect(win_x + 6, by, bw, bar_h, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect_outline(win_x + 6, by, bw, bar_h, Color::BORDER);

            // Satır 1: Bul
            MinGuiCompositor::draw_string(fb, win_x + 12, by + 6, crate::gui::ui_text("Bul:", "Find:"), Color::TEXT_MAIN);
            let input_w = crate::gui::sp(140);
            let f_box_x = win_x + crate::gui::sp(50);
            let f_box_y = by + 4;
            let f_box_h = 20;
            let find_foc = !self.find_focused_replace;
            fb.draw_rect(f_box_x, f_box_y, input_w, f_box_h, Color::DARK_BG);
            fb.draw_rect_outline(f_box_x, f_box_y, input_w, f_box_h, if find_foc { Color::ACCENT_BLUE } else { Color::BORDER });
            MinGuiCompositor::draw_string_clipped(fb, f_box_x + 4, f_box_y + 3, self.find_str(), Color::TEXT_MAIN, (f_box_x + 4, f_box_y, input_w - 8, f_box_h));
            if find_foc && (tick / 20) % 2 == 0 {
                let tw = MinGuiCompositor::terminal_text_width(self.find_str()).min(input_w.saturating_sub(10));
                fb.draw_rect(f_box_x + 4 + tw, f_box_y + 3, 2, 14, Color::ACCENT_YELLOW);
            }

            // Butonlar: Sonraki, Önceki, Kapat
            let mut btn_x = f_box_x + input_w + 8;
            fb.draw_rect(btn_x, f_box_y, 60, f_box_h, Color::WINDOW_BG);
            MinGuiCompositor::draw_string(fb, btn_x + 6, f_box_y + 3, crate::gui::ui_text("Sonraki", "Next"), Color::TEXT_MAIN);
            btn_x += 66;
            fb.draw_rect(btn_x, f_box_y, 60, f_box_h, Color::WINDOW_BG);
            MinGuiCompositor::draw_string(fb, btn_x + 6, f_box_y + 3, crate::gui::ui_text("Önceki", "Prev"), Color::TEXT_MAIN);

            let close_x = win_x + bw.saturating_sub(20);
            MinGuiCompositor::draw_string(fb, close_x, f_box_y + 2, "X", Color::ACCENT_RED);

            // Satır 2: Değiştir
            if self.replace_active {
                let ry = by + 30;
                MinGuiCompositor::draw_string(fb, win_x + 12, ry + 6, crate::gui::ui_text("Değiştir:", "Replace:"), Color::TEXT_MAIN);
                let r_box_x = f_box_x;
                let r_box_y = ry + 4;
                let rep_foc = self.find_focused_replace;
                fb.draw_rect(r_box_x, r_box_y, input_w, f_box_h, Color::DARK_BG);
                fb.draw_rect_outline(r_box_x, r_box_y, input_w, f_box_h, if rep_foc { Color::ACCENT_BLUE } else { Color::BORDER });
                MinGuiCompositor::draw_string_clipped(fb, r_box_x + 4, r_box_y + 3, self.replace_str(), Color::TEXT_MAIN, (r_box_x + 4, r_box_y, input_w - 8, f_box_h));
                if rep_foc && (tick / 20) % 2 == 0 {
                    let tw = MinGuiCompositor::terminal_text_width(self.replace_str()).min(input_w.saturating_sub(10));
                    fb.draw_rect(r_box_x + 4 + tw, r_box_y + 3, 2, 14, Color::ACCENT_YELLOW);
                }

                let mut rbtn_x = r_box_x + input_w + 8;
                fb.draw_rect(rbtn_x, r_box_y, 60, f_box_h, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, rbtn_x + 6, r_box_y + 3, crate::gui::ui_text("Değiştir", "Replace"), Color::TEXT_MAIN);
                rbtn_x += 66;
                fb.draw_rect(rbtn_x, r_box_y, 76, f_box_h, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, rbtn_x + 6, r_box_y + 3, crate::gui::ui_text("Tümünü", "All"), Color::TEXT_MAIN);
            }
        }

        // Metin Editörü Alanı — pencere boyutuna göre DİNAMİK
        let area_x = win_x + 6;
        let area_y = win_y + 58 + bar_h;
        // İçerik alanı, kaydırma çubuğu ve durum çubuğu düşülerek hesaplanır.
        let area_w = win_w.saturating_sub(12 + scrollbar::SCROLLBAR_THICKNESS);
        let area_h = win_h
            .saturating_sub((area_y - win_y) + status_h() + scrollbar::SCROLLBAR_THICKNESS);
        fb.draw_rect(area_x, area_y, area_w, area_h, Color::DARK_BG);

        // Kaydırılabilir metin alanı sınırları.
        let text_x = area_x + gutter_w() + 6;
        let text_w = area_w.saturating_sub(gutter_w() + 10);
        let wrap_cols = (text_w / char_w()).max(1);
        let (total_lines, _max_cols) = self.text_extent(wrap_cols);
        let view_lines = (area_h / line_h()).max(1);
        let sy = scroll.y / line_h(); // satır cinsinden kaydırma

        // Satır Numaraları Şeridi (Gutter) — görünür satırlara göre dinamik
        let gw = gutter_w();
        fb.draw_rect(area_x, area_y, gw, area_h, Color::TITLE_BAR_INACTIVE);
        fb.draw_rect(area_x + gw - 1, area_y, 1, area_h, Color::BORDER);
        let last_visible = (sy + view_lines).min(total_lines);
        let mut gutter = TextBuf::new();
        for ln in sy..last_visible {
            let row = ln - sy;
            let gy = area_y + row * line_h() + 4;
            gutter.clear();
            let n = (ln + 1) as u32;
            gutter.push_usize(n as usize);
            let num_str = gutter.as_str();
            let num_w = MinGuiCompositor::terminal_text_width(num_str);
            // Satır numarasını şeridin sağına düzgün aralıkla (6px boşluk bırakarak) hizala
            let num_x = area_x + gw.saturating_sub(num_w + 6);
            MinGuiCompositor::draw_terminal_string(fb, num_x, gy, num_str, Color::TEXT_MUTED);
        }

        // Metin İçeriği ve İmleç — kaydırma ofseti uygulanır, satır kaydırmalı (wrap).
        let first_line = sy;
        let mut line = 0usize;
        let mut col = 0usize;
        let mut cursor_screen: Option<(usize, usize)> = if self.cursor == 0 {
            Some((text_x, area_y + 4))
        } else {
            None
        };
        for i in 0..self.len {
            let ch = self.buffer[i];
            if line >= first_line && line < first_line + view_lines {
                let row = line - first_line;
                let cx = text_x + col * char_w();
                let cy = area_y + row * line_h() + 4;
                if ch != '\n' {
                    let mut char_buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut char_buf);
                    let selected = match (self.selection_start, self.selection_end) {
                        (Some(a), Some(b)) => i >= a.min(b) && i < a.max(b),
                        _ => false,
                    };
                    let sel_h = line_h().saturating_sub(4).max(8);
                    if selected { fb.draw_rect(cx, cy, char_w(), sel_h, Color::ACCENT_BLUE); }
                    MinGuiCompositor::draw_terminal_string(fb, cx, cy, s, if selected { MinGuiCompositor::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN });
                }
            }
            if i == self.cursor.saturating_sub(1) {
                let row = line.saturating_sub(first_line);
                cursor_screen = Some((text_x + (col + 1) * char_w(), area_y + row * line_h() + 4));
            }
            // İlerleme (satır kaydırma dahil)
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
                if col >= wrap_cols {
                    line += 1;
                    col = 0;
                }
            }
        }

        // Yanıp Sönen İmleç (Blinking Cursor) — imleç görünür aralıktaysa çiz
        if let Some((cx, cy)) = cursor_screen {
            let cur_h = line_h().saturating_sub(4).max(8);
            if (tick / 20) % 2 == 0 && cy >= area_y && cy + cur_h <= area_y + area_h {
                if self.overwrite {
                    fb.draw_rect(cx, cy, char_w(), cur_h, Color::ACCENT_YELLOW);
                    if self.cursor < self.len && self.buffer[self.cursor] != '\n' {
                        let mut char_buf = [0u8; 4];
                        let s = self.buffer[self.cursor].encode_utf8(&mut char_buf);
                        MinGuiCompositor::draw_terminal_string(fb, cx, cy, s, Color::BLACK);
                    }
                } else {
                    fb.draw_rect(cx, cy, crate::gui::sp(2).max(1), cur_h, Color::ACCENT_YELLOW);
                }
            }
        }

        // Alt Durum Çubuğu — pencerenin en altına sabitlenir
        let status_y = win_y + win_h - status_h() + 4;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, status_h() - 4, Color::TASKBAR_BG);
        let mut status = TextBuf::new();
        if !self.status_msg.is_empty() {
            status.push_str(self.status_msg);
            status.push_str("  |  ");
        }
        // Üzerine yaz (overwrite) modu göstergesi.
        if self.overwrite {
            status.push_str(crate::gui::ui_text("ÜYZ", "OVR"));
            status.push_str("  |  ");
        }
        status.push_str(self.file_path_str());
        status.push_str("  |  ");
        status.push_str(crate::gui::ui_text("Satir: ", "Line: "));
        status.push_u2(((sy + 1).min(total_lines)) as u32);
        status.push_byte(b'/');
        status.push_u2(total_lines.min(99) as u32);
        status.push_str("  |  " );
        status.push_u2(self.len.min(99) as u32);
        status.push_str(crate::gui::ui_text(" karakter", " chars"));
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, status.as_str(), Color::TEXT_MUTED);

        // Menü, metin alanının üst katmanında çizilmeli; aksi halde metin
        // editörü menü kutusunu kapatır.
        if self.menu != 0 {
            let mx = win_x + Self::menu_box_x(self.menu);
            let box_y = win_y + Self::menu_box_y();
            let box_w = Self::menu_box_w();
            let box_h = Self::menu_box_h(self.menu);
            let items = Self::menu_items(self.menu);

            fb.draw_shadow(mx, box_y, box_w, box_h, Color::SHADOW);
            fb.draw_rect(mx, box_y, box_w, box_h, Color::WINDOW_BG);
            fb.draw_rect_outline(mx, box_y, box_w, box_h.saturating_sub(2), Color::BORDER);

            let pad = Self::menu_pad();
            let item_h = Self::menu_item_h();
            for (i, def) in items.iter().enumerate() {
                let item_y = box_y + pad + i * item_h;
                let enabled = self.is_item_enabled(def.action);
                let label = if english { def.label_en } else { def.label_tr };

                let text_color = if enabled { Color::TEXT_MAIN } else { Color::TEXT_MUTED };
                let sc_color = Color::TEXT_MUTED;

                // Öğe satır zemini
                fb.draw_rect(mx + crate::gui::sp(4), item_y, box_w.saturating_sub(crate::gui::sp(8)), item_h.saturating_sub(crate::gui::sp(2)), Color::TITLE_BAR_INACTIVE);

                // Sol: Menü öğesi metni
                MinGuiCompositor::draw_string(fb, mx + crate::gui::sp(10), item_y + crate::gui::sp(4), label, text_color);

                // Sağ: Kısayol tuş bilgisi
                if !def.shortcut.is_empty() {
                    let sc_w = MinGuiCompositor::text_width(def.shortcut, 1);
                    let sc_x = mx + box_w.saturating_sub(sc_w + crate::gui::sp(12));
                    MinGuiCompositor::draw_string(fb, sc_x, item_y + crate::gui::sp(4), def.shortcut, sc_color);
                }
            }
        }
    }
}

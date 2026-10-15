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

pub struct NotepadApp {
    pub buffer: Vec<char>,
    pub len: usize,
    pub cursor: usize,
    // Selection range (inclusive start, exclusive end)
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    // Show right‑click context menu
    pub show_context_menu: bool,
    // Scroll offset for content rendering
    pub scroll_offset: usize,
    // For double‑click detection
    pub last_click_tick: usize,
    pub last_click_pos: (usize, usize),
    pub menu: u8, // 0 kapalı, 1 Dosya, 2 Düzen, 3 Yardım
    pub file_path: [u8; 64],
    pub file_path_len: usize,
    pub status_msg: &'static str,
    // Geri al / ileri al (undo/redo) yığın sayacı. Gerçek anlık görüntüler
    // büyük bellek kapladığı için global `UNDO_STACK`/`REDO_STACK` içinde tutulur.
    undo_len: u8,
    redo_len: u8,
    history: History,
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

impl NotepadApp {
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

    // Delete selected text
    pub fn delete_selection(&mut self) {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (s, e) = (start.min(end), start.max(end));
            let del_len = e - s;
            if del_len == 0 {
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
            self.selection_start = None;
            self.selection_end = None;
        }
    }
}

impl NotepadApp {
    pub fn new() -> Self {
        let mut app = Self {
            buffer: alloc::vec![ ' '; INITIAL_BUFFER_CAPACITY ],
            len: 0,
            cursor: 0,
            selection_start: None,
            selection_end: None,
            show_context_menu: false,
            scroll_offset: 0,
            last_click_tick: 0,
            last_click_pos: (0, 0),
            menu: 0,
            file_path: [0; 64],
            file_path_len: 0,
            status_msg: "",
            undo_len: 0,
            redo_len: 0,
            history: History { undo: Vec::new(), redo: Vec::new() },
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
        self.push_history();
        self.delete_selection();
        if self.len == self.buffer.len() {
            if self.buffer.try_reserve(1).is_err() { return; }
            self.buffer.push(' ');
        }
        for i in (self.cursor..self.len).rev() { self.buffer[i + 1] = self.buffer[i]; }
        self.buffer[self.cursor] = c;
        self.len += 1;
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.selection_start != self.selection_end && self.selection_start.is_some() { self.delete_selection(); return; }
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
    pub fn delete_forward(&mut self) { if self.selection_start.is_some() && self.selection_start != self.selection_end { self.delete_selection(); } else if self.cursor < self.len { self.selection_start=Some(self.cursor); self.selection_end=Some(self.cursor+1); self.delete_selection(); } }

    /// İmleci satır başına taşır (Home tuşu).
    pub fn move_home(&mut self) {
        let mut start = self.cursor;
        while start > 0 && self.buffer[start - 1] != '\n' { start -= 1; }
        self.cursor = start;
        self.selection_start = None;
        self.selection_end = None;
    }

    /// İmleci satır sonuna taşır (End tuşu).
    pub fn move_end(&mut self) {
        let mut end = self.cursor;
        while end < self.len && self.buffer[end] != '\n' { end += 1; }
        self.cursor = end;
        self.selection_start = None;
        self.selection_end = None;
    }

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
            self.selection_start = None;
            self.selection_end = None;
        } else {
            self.selection_start = Some(s);
            self.selection_end = Some(e);
            self.cursor = e;
        }
    }

    /// Seçim çapasını (anchor) ayarlar; Shift+ok ile seçim genişletmek için.
    fn anchor(&mut self) -> usize {
        match (self.selection_start, self.selection_end) {
            (Some(a), Some(b)) => if self.cursor == b { a } else { b },
            _ => self.cursor,
        }
    }

    /// İmleci bir sonraki konuma taşır; `select` true ise seçimi genişletir.
    fn move_to(&mut self, target: usize, select: bool) {
        if select {
            let a = self.anchor();
            self.cursor = target;
            self.selection_start = Some(a.min(target));
            self.selection_end = Some(a.max(target));
        } else {
            self.cursor = target;
            self.selection_start = None;
            self.selection_end = None;
        }
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

    pub fn move_vertical_ext(&mut self, up: bool, select: bool) {
        let mut line_start = self.cursor;
        while line_start > 0 && self.buffer[line_start - 1] != '\n' { line_start -= 1; }
        let column = self.cursor - line_start;
        let target = if up {
            if line_start == 0 {
                0
            } else {
                let previous_end = line_start - 1;
                let mut previous_start = previous_end;
                while previous_start > 0 && self.buffer[previous_start - 1] != '\n' { previous_start -= 1; }
                (previous_start + column).min(previous_end)
            }
        } else {
            let mut line_end = self.cursor;
            while line_end < self.len && self.buffer[line_end] != '\n' { line_end += 1; }
            if line_end == self.len {
                self.len
            } else {
                let next_start = line_end + 1;
                let mut next_end = next_start;
                while next_end < self.len && self.buffer[next_end] != '\n' { next_end += 1; }
                (next_start + column).min(next_end)
            }
        };
        if select {
            let a = self.anchor();
            self.cursor = target;
            self.selection_start = Some(a.min(target));
            self.selection_end = Some(a.max(target));
        } else {
            self.cursor = target;
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn clear(&mut self) {
        self.push_history();
        self.len = 0;
        self.cursor = 0;
        for c in self.buffer.iter_mut() {
            *c = ' ';
        }
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
        self.status_msg = crate::gui::ui_text("Dosya acildi", "File opened");
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
            self.status_msg = crate::gui::ui_text("Dosya basariyla kaydedildi!", "File saved successfully!");
        } else {
            self.status_msg = crate::gui::ui_text("Kaydetme basarisiz!", "Save failed!");
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
        self.selection_start = Some(0);
        self.selection_end = Some(self.len);
    }
    /// Metin alanı başlangıcının pencere içi X koordinatı (gutter + boşluk).
    #[inline(always)]
    pub fn text_x() -> usize { gutter_w() + crate::gui::sp(12) }
    /// Metin alanı başlangıcının pencere içi Y koordinatı (araç çubuğu + boşluk).
    #[inline(always)]
    pub fn text_y() -> usize { crate::gui::sp(62) }

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
        let click_row = scroll_lines + (y.saturating_sub(Self::text_y())) / line_h();

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
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 36, crate::gui::ui_text("Dosya", "File"), Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, win_x + 72, win_y + 36, crate::gui::ui_text("Düzen", "Edit"), Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 132, win_y + 36, crate::gui::ui_text("Yardım", "Help"), Color::TEXT_MUTED);
        // Hızlı Aksiyon Düğmeleri (sağa hizalı)
        fb.draw_rect(win_x + win_w.saturating_sub(130), win_y + 32, 56, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(124), win_y + 36, crate::gui::ui_text("Temizle", "Clear"), Color::ACCENT_RED);
        fb.draw_rect(win_x + win_w.saturating_sub(68), win_y + 32, 56, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(60), win_y + 36, crate::gui::ui_text("Kaydet", "Save"), MinGuiCompositor::text_on(Color::ACCENT_BLUE));

        // Metin Editörü Alanı — pencere boyutuna göre DİNAMİK
        let area_x = win_x + 6;
        let area_y = win_y + 58;
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
                fb.draw_rect(cx, cy, crate::gui::sp(2).max(1), cur_h, Color::ACCENT_YELLOW);
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
            let mx = win_x + match self.menu { 1=>8, 2=>68, _=>128 };
            let items = match (self.menu, english) {
                (1, true) => ["New Document", "Open", "Save", "Clear"],
                (2, true) => ["Cut", "Copy", "Paste", "Select All"],
                (_, true) => ["About Notepad", "", "", ""],
                (1, false) => ["Yeni Belge", "Aç", "Kaydet", "Temizle"],
                (2, false) => ["Kes", "Kopyala", "Yapıştır", "Tümünü Seç"],
                _ => ["Not Defteri Hakkında", "", "", ""],
            };
            // Yardım menüsünde tek öğe olduğundan kutu yüksekliği ona göre küçülür.
            let box_h = if items[1].is_empty() { 34 } else { 114 };
            fb.draw_shadow(mx, win_y + 54, 150, box_h, Color::SHADOW);
            fb.draw_rect(mx, win_y + 54, 150, box_h, Color::WINDOW_BG);
            fb.draw_rect_outline(mx, win_y + 54, 150, box_h.saturating_sub(2), Color::BORDER);
            for i in 0..4 {
                if !items[i].is_empty() {
                    fb.draw_rect(mx + 4, win_y + 60 + i * 20, 142, 18, Color::TITLE_BAR_INACTIVE);
                    MinGuiCompositor::draw_string(fb, mx + 10, win_y + 64 + i * 20, items[i], Color::TEXT_MAIN);
                }
            }
        }
    }
}

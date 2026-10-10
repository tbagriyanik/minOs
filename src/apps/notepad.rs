use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::scrollbar::{self, ScrollState};

/// Metin satır yüksekliği (px).
const LINE_H: usize = 16;
/// Karakter hücre ilerlemesi (px).
const CHAR_W: usize = 7;
/// Satır numarası şeridi (gutter) genişliği.
const GUTTER_W: usize = 32;
/// Araç çubuğu yüksekliği (px).
const TOOLBAR_H: usize = 24;
/// Alt durum çubuğu yüksekliği (px).
const STATUS_H: usize = 22;

pub struct NotepadApp {
    pub buffer: [char; 512],
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
    pub menu: u8, // 0 kapalı, 1 Dosya, 2 Düzen, 3 Biçim, 4 Yardım
}

// Simple clipboard for copy / paste (static mutable, unsafe)
static mut CLIPBOARD: [char; 512] = [' '; 512];
static mut CLIPBOARD_LEN: usize = 0;

impl NotepadApp {
    // Copy selected text to the clipboard
    pub fn copy_selection(&self) {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let range = start.min(end)..start.max(end);
            let len = (range.end - range.start).min(512);
            unsafe {
                for (i, &c) in self.buffer[range.clone()].iter().take(len).enumerate() {
                    CLIPBOARD[i] = c;
                }
                CLIPBOARD_LEN = len;
            }
        }
    }

    // Cut selected text (copy then delete)
    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    // Paste clipboard content at cursor
    pub fn paste_clipboard(&mut self) {
        unsafe {
            for i in 0..CLIPBOARD_LEN {
                if self.len >= 512 { break; }
                self.buffer[self.len] = CLIPBOARD[i];
                self.len += 1;
                self.cursor = self.len;
            }
        }
    }

    // Delete selected text
    pub fn delete_selection(&mut self) {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (s, e) = (start.min(end), start.max(end));
            let del_len = e - s;
            if del_len == 0 { return; }
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
            buffer: [' '; 512],
            len: 0,
            cursor: 0,
            selection_start: None,
            selection_end: None,
            show_context_menu: false,
            scroll_offset: 0,
            last_click_tick: 0,
            last_click_pos: (0, 0),
            menu: 0,
        };

        let initial_text = "Not Defteri\n-----------\nYardım: https://github.com/tbagriyanik/minos\n\nKlavyenizle doğrudan metin yazabilirsiniz.\n(TR-Q ve EN-US düzenleri desteklenir)\n\nNotlarınızı buraya ekleyin:";
        for c in initial_text.chars() {
            if app.len < 510 {
                app.buffer[app.len] = c;
                app.len += 1;
            }
        }
        app.cursor = app.len;
        app
    }

    pub fn insert_char(&mut self, c: char) {
        self.delete_selection();
        if self.len < 500 {
            // Sona ekle
            self.buffer[self.len] = c;
            self.len += 1;
            self.cursor = self.len;
        }
    }

    pub fn backspace(&mut self) {
        if self.selection_start != self.selection_end && self.selection_start.is_some() { self.delete_selection(); return; }
        if self.len > 0 {
            self.len -= 1;
            self.buffer[self.len] = ' ';
            self.cursor = self.len;
        }
    }
    pub fn delete_forward(&mut self) { if self.selection_start.is_some() && self.selection_start != self.selection_end { self.delete_selection(); } else if self.cursor < self.len { self.selection_start=Some(self.cursor); self.selection_end=Some(self.cursor+1); self.delete_selection(); } }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
        for c in self.buffer.iter_mut() {
            *c = ' ';
        }
        self.selection_start = None;
        self.selection_end = None;
    }

    /// Tüm metni seçer (Kopyala/Kes/Sil ile birlikte kullanılır).
    pub fn select_all(&mut self) {
        self.selection_start = Some(0);
        self.selection_end = Some(self.len);
    }
    pub fn index_at(&self, x: usize, y: usize, win_x: usize, win_y: usize) -> usize {
        let col = x.saturating_sub(win_x + 42) / 7;
        let row = y.saturating_sub(win_y + 66) / 16;
        let mut line = 0; let mut idx = 0;
        while idx < self.len && line < row { if self.buffer[idx] == '\n' { line += 1; } idx += 1; }
        let mut pos = 0; while idx < self.len && pos < col && self.buffer[idx] != '\n' { idx += 1; pos += 1; } idx.min(self.len)
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
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, TOOLBAR_H, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 36, "Dosya", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, win_x + 72, win_y + 36, "Düzen", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 132, win_y + 36, "Biçim", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 192, win_y + 36, "Yardım", Color::TEXT_MUTED);
        // Hızlı Aksiyon Düğmeleri (sağa hizalı)
        fb.draw_rect(win_x + win_w.saturating_sub(130), win_y + 32, 56, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(124), win_y + 36, "Temizle", Color::ACCENT_RED);
        fb.draw_rect(win_x + win_w.saturating_sub(68), win_y + 32, 56, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(60), win_y + 36, "Kaydet", Color::DARK_BG);

        // Metin Editörü Alanı — pencere boyutuna göre DİNAMİK
        let area_x = win_x + 6;
        let area_y = win_y + 58;
        // İçerik alanı, kaydırma çubuğu ve durum çubuğu düşülerek hesaplanır.
        let area_w = win_w.saturating_sub(12 + scrollbar::SCROLLBAR_THICKNESS);
        let area_h = win_h
            .saturating_sub((area_y - win_y) + STATUS_H + scrollbar::SCROLLBAR_THICKNESS);
        fb.draw_rect(area_x, area_y, area_w, area_h, Color::DARK_BG);

        // Kaydırılabilir metin alanı sınırları.
        let text_x = area_x + GUTTER_W + 6;
        let text_w = area_w.saturating_sub(GUTTER_W + 10);
        let wrap_cols = (text_w / CHAR_W).max(1);
        let (total_lines, _max_cols) = self.text_extent(wrap_cols);
        let view_lines = (area_h / LINE_H).max(1);
        let sy = scroll.y / LINE_H; // satır cinsinden kaydırma

        // Satır Numaraları Şeridi (Gutter) — görünür satırlara göre dinamik
        fb.draw_rect(area_x, area_y, GUTTER_W, area_h, Color::TITLE_BAR_INACTIVE);
        let last_visible = (sy + view_lines).min(total_lines);
        let mut gutter = TextBuf::new();
        for ln in sy..last_visible {
            let row = ln - sy;
            let gy = area_y + row * LINE_H + 4;
            gutter.clear();
            let n = ln + 1;
            if n < 10 { gutter.push_byte(b'0'); }
            if n < 100 { gutter.push_byte(b'0' + ((n / 10) % 10) as u8); }
            gutter.push_byte(b'0' + (n % 10) as u8);
            MinGuiCompositor::draw_string(fb, area_x + 8, gy, gutter.as_str(), Color::TEXT_MUTED);
        }

        // Metin İçeriği ve İmleç — kaydırma ofseti uygulanır, satır kaydırmalı (wrap).
        let first_line = sy;
        let mut line = 0usize;
        let mut col = 0usize;
        let mut cursor_screen: Option<(usize, usize)> = None;
        for i in 0..self.len {
            let ch = self.buffer[i];
            if line >= first_line && line < first_line + view_lines {
                let row = line - first_line;
                let cx = text_x + col * CHAR_W;
                let cy = area_y + row * LINE_H + 4;
                if ch != '\n' {
                    let mut char_buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut char_buf);
                    let selected = match (self.selection_start, self.selection_end) {
                        (Some(a), Some(b)) => i >= a.min(b) && i < a.max(b),
                        _ => false,
                    };
                    if selected { fb.draw_rect(cx, cy, CHAR_W, 12, Color::ACCENT_BLUE); }
                    MinGuiCompositor::draw_string(fb, cx, cy, s, if selected { Color::DARK_BG } else { Color::TEXT_MAIN });
                }
            }
            if i == self.cursor.saturating_sub(1) {
                let row = line.saturating_sub(first_line);
                cursor_screen = Some((text_x + (col + 1) * CHAR_W, area_y + row * LINE_H + 4));
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
            if (tick / 20) % 2 == 0 && cy >= area_y && cy + 12 <= area_y + area_h {
                fb.draw_rect(cx, cy, 2, 12, Color::ACCENT_YELLOW);
            }
        }

        // Alt Durum Çubuğu — pencerenin en altına sabitlenir
        let status_y = win_y + win_h - STATUS_H + 4;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, STATUS_H - 4, Color::TASKBAR_BG);
        let mut status = TextBuf::new();
        status.push_str("Satir: ");
        status.push_u2(((sy + 1).min(total_lines)) as u32);
        status.push_byte(b'/');
        status.push_u2(total_lines.min(99) as u32);
        status.push_str("  |  " );
        status.push_u2(self.len.min(99) as u32);
        status.push_str(" karakter  |  UTF-8 / TR-Q");
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, status.as_str(), Color::TEXT_MUTED);

        // Menü, metin alanının üst katmanında çizilmeli; aksi halde metin
        // editörü menü kutusunu kapatır.
        if self.menu != 0 {
            let mx = win_x + match self.menu { 1=>8, 2=>68, 3=>128, _=>188 };
            let items = match self.menu {
                1 => ["Yeni Belge", "Kaydet", "Temizle", ""],
                2 => ["Kes", "Kopyala", "Yapıştır", "Tümünü Seç"],
                3 => ["Piksel Font", "Sıkıştırılmış", "ClearType Font", ""],
                _ => ["Not Defteri Hakkında", "", "", ""],
            };
            fb.draw_shadow(mx, win_y + 54, 150, 94, Color::SHADOW);
            fb.draw_rect(mx, win_y + 54, 150, 94, Color::WINDOW_BG);
            fb.draw_rect_outline(mx, win_y + 54, 150, 94, Color::BORDER);
            for i in 0..4 {
                if !items[i].is_empty() {
                    fb.draw_rect(mx + 4, win_y + 60 + i * 20, 142, 18, Color::TITLE_BAR_INACTIVE);
                    MinGuiCompositor::draw_string(fb, mx + 10, win_y + 64 + i * 20, items[i], Color::TEXT_MAIN);
                }
            }
        }
    }
}

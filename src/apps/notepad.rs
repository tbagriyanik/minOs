use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

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
        };
        
        let initial_text = "MinOS Not Defteri v1.0 (Sarman Edition)\n---------------------------------------\nYardım: https://github.com/tbagriyanik/minos\n\nKlavyenizle doğrudan metin yazabilirsiniz.\n(TR-Q ve EN-US düzenleri desteklenir)\n\nNotlarınızı buraya ekleyin:";
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
        if self.len < 500 {
            // Sona ekle
            self.buffer[self.len] = c;
            self.len += 1;
            self.cursor = self.len;
        }
    }

    pub fn backspace(&mut self) {
        if self.len > 0 {
            self.len -= 1;
            self.buffer[self.len] = ' ';
            self.cursor = self.len;
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
        for c in self.buffer.iter_mut() {
            *c = ' ';
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize, tick: usize) {
        // Araç Çubuğu (Toolbar)
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 24, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 36, "[Dosya]", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, win_x + 72, win_y + 36, "[Düzen]", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 132, win_y + 36, "[Biçim]", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + 192, win_y + 36, "[Yardım]", Color::TEXT_MUTED);

        // Hızlı Aksiyon Düğmeleri
        fb.draw_rect(win_x + win_w - 130, win_y + 32, 56, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 124, win_y + 36, "Temizle", Color::ACCENT_RED);
        fb.draw_rect(win_x + win_w - 68, win_y + 32, 56, 20, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 60, win_y + 36, "Kaydet", Color::DARK_BG);
        // Context menu (right‑click) placeholder
        if self.show_context_menu {
            // Simple box
            fb.draw_rect(win_x + 100, win_y + 100, 80, 60, Color::TITLE_BAR);
            MinGuiCompositor::draw_string(fb, win_x + 108, win_y + 112, "Kopyala", Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, win_x + 108, win_y + 132, "Kes", Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, win_x + 108, win_y + 152, "Sil", Color::TEXT_MAIN);
        }


        // Metin Editör Alanı (Beyazımsı / Koyu Arka Plan)
        let text_bg = 0x11111B;
        let area_x = win_x + 6;
        let area_y = win_y + 58;
        let area_w = win_w - 12;
        let area_h = 180;
        fb.draw_rect(area_x, area_y, area_w, area_h, text_bg);

        // Satır Numaraları Şeridi (Gutter)
        fb.draw_rect(area_x, area_y, 28, area_h, Color::TITLE_BAR_INACTIVE);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 8, "01", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 24, "02", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 40, "03", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 56, "04", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 72, "05", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 88, "06", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 104, "07", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 120, "08", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, area_x + 6, area_y + 136, "09", Color::TEXT_MUTED);

        // Metin İçeriği ve İmleç
        let mut cur_x = area_x + 36;
        let mut cur_y = area_y + 8 + self.scroll_offset * 16;

        for i in 0..self.len {
            let ch = self.buffer[i];
            if ch == '\n' {
                cur_y += 16;
                cur_x = area_x + 36;
                if cur_y + 16 > area_y + area_h {
                    break;
                }
                continue;
            }

            let mut char_buf = [0u8; 4];
            let s = ch.encode_utf8(&mut char_buf);
            MinGuiCompositor::draw_string(fb, cur_x, cur_y, s, Color::TEXT_MAIN);
            cur_x += 7;

            if cur_x + 16 > area_x + area_w {
                cur_y += 16;
                cur_x = area_x + 36;
                if cur_y + 16 > area_y + area_h {
                    break;
                }
            }
        }

        // Yanıp Sönen İmleç (Blinking Cursor)
        if (tick / 20) % 2 == 0 {
            fb.draw_rect(cur_x, cur_y, 2, 12, Color::ACCENT_YELLOW);
        }

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 242, win_w - 12, 18, Color::TASKBAR_BG);
        let status = "Yardım: https://github.com/tbagriyanik/minos | UTF-8 / TR-Q";
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 246, status, Color::TEXT_MUTED);
    }
}

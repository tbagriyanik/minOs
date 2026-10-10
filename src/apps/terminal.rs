//! MinShell — MinOS Komut İstemi (Command Prompt).
//!
//! Gerçek bir komut satırı: girdi tamponu, komut geçmişi, çıktı satırları ve
//! `help`, `ls`, `cat`, `echo`, `clear`, `mem`, `ps`, `date`, `uname`, `theme`,
//! `whoami` gibi yerleşik (built-in) komutlar. VFS ve PMM'e salt-okunur erişir.

use crate::drivers::video::Framebuffer;
use crate::drivers::rtc::DateTime;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::theme;
use crate::fs::vfs::{VirtualFileSystem, NodeType};
use crate::mm::pmm::PhysicalMemoryManager;

/// Çıktı satırı sayısı (kaydırma geçmişi).
pub const OUT_LINES: usize = 22;
/// Tek satırın azami uzunluğu.
pub const LINE_MAX: usize = 64;

#[derive(Copy, Clone)]
pub struct OutLine {
    pub text: [u8; LINE_MAX],
    pub len: usize,
    pub color: u32,
}

impl OutLine {
    pub const fn new() -> Self {
        Self { text: [0; LINE_MAX], len: 0, color: 0 }
    }

    pub fn set(&mut self, s: &str, color: u32) {
        self.len = 0;
        self.color = color;
        for b in s.bytes() {
            if self.len < LINE_MAX {
                self.text[self.len] = b;
                self.len += 1;
            }
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.text[..self.len]).unwrap_or("")
    }
}

pub struct TerminalApp {
    /// Aktif girdi satırı.
    pub input: [u8; LINE_MAX],
    pub input_len: usize,
    /// Çıktı satırları (kayan pencere).
    pub out: [OutLine; OUT_LINES],
    pub out_count: usize,
    /// Komut geçmişi.
    pub history: [[u8; LINE_MAX]; 8],
    pub history_len: [usize; 8],
    pub history_count: usize,
    /// Geçmişte gezinme imleci.
    pub hist_cursor: usize,
    /// Yürütülmeyi bekleyen komut (Enter'a basılınca).
    pub pending: [u8; LINE_MAX],
    pub pending_len: usize,
    pub has_pending: bool,
}

impl TerminalApp {
    pub const fn new() -> Self {
        Self {
            input: [0; LINE_MAX],
            input_len: 0,
            out: [OutLine::new(); OUT_LINES],
            out_count: 0,
            history: [[0; LINE_MAX]; 8],
            history_len: [0; 8],
            history_count: 0,
            hist_cursor: 0,
            pending: [0; LINE_MAX],
            pending_len: 0,
            has_pending: false,
        }
    }

    /// Başlangıç banner'ı.
    pub fn init_banner(&mut self) {
        self.push_line("MinOS MinShell v2.0 (Sarman Edition)", Color::ACCENT_PINK);
        self.push_line("Yardim icin 'help' yazin.", Color::TEXT_MUTED);
    }

    /// Çıktıya yeni bir satır ekler; tampon dolarsa en eskisini kaydırır.
    pub fn push_line(&mut self, s: &str, color: u32) {
        if self.out_count < OUT_LINES {
            self.out[self.out_count].set(s, color);
            self.out_count += 1;
        } else {
            for i in 1..OUT_LINES {
                self.out[i - 1] = self.out[i];
            }
            self.out[OUT_LINES - 1].set(s, color);
            self.out_count = OUT_LINES;
        }
    }

    pub fn insert(&mut self, c: char) {
        if c == '\n' {
            self.submit();
            return;
        }
        if (c as u32) < 32 {
            return;
        }
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        for b in s.bytes() {
            if self.input_len < LINE_MAX - 1 {
                self.input[self.input_len] = b;
                self.input_len += 1;
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.input_len > 0 {
            self.input_len -= 1;
        }
    }

    /// Enter'a basılınca: istemi yaz, komutu `pending`'e al (yürütme ana döngüde).
    pub fn submit(&mut self) {
        let n = self.input_len;
        let mut prompt = TextBuf::new();
        prompt.push_str("> ");
        if let Ok(s) = core::str::from_utf8(&self.input[..n]) {
            prompt.push_str(s);
        }
        self.push_line(prompt.as_str(), Color::ACCENT_GREEN);

        if n > 0 {
            // Geçmişe kaydet.
            self.record_history();
            // Yürütülecek komutu sakla (ana döngü VFS/PMM bağlamında çalıştırır).
            self.pending[..n].copy_from_slice(&self.input[..n]);
            self.pending_len = n;
            self.has_pending = true;
        }
        self.input_len = 0;
        self.hist_cursor = self.history_count;
    }

    /// Yürütülmeyi bekleyen komutu alır ve bayrağı temizler.
    pub fn take_pending(&mut self) -> Option<[u8; LINE_MAX]> {
        if self.has_pending {
            self.has_pending = false;
            Some(self.pending)
        } else {
            None
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending_len
    }

    pub fn history_up(&mut self) {
        if self.history_count == 0 {
            return;
        }
        if self.hist_cursor > 0 {
            self.hist_cursor -= 1;
        }
        self.load_history(self.hist_cursor);
    }

    pub fn history_down(&mut self) {
        if self.history_count == 0 {
            return;
        }
        if self.hist_cursor + 1 < self.history_count {
            self.hist_cursor += 1;
            self.load_history(self.hist_cursor);
        } else {
            self.hist_cursor = self.history_count;
            self.input_len = 0;
        }
    }

    fn load_history(&mut self, idx: usize) {
        if idx < self.history_count {
            let n = self.history_len[idx];
            self.input[..n].copy_from_slice(&self.history[idx][..n]);
            self.input_len = n;
        }
    }

    fn record_history(&mut self) {
        if self.input_len == 0 {
            return;
        }
        let idx = if self.history_count < 8 {
            self.history_count
        } else {
            for i in 1..8 {
                self.history[i - 1] = self.history[i];
                self.history_len[i - 1] = self.history_len[i];
            }
            7
        };
        let n = self.input_len;
        self.history[idx][..n].copy_from_slice(&self.input[..n]);
        self.history_len[idx] = n;
        if self.history_count < 8 {
            self.history_count += 1;
        }
        self.hist_cursor = self.history_count;
    }

    /// Verilen komut satırını VFS/PMM/DateTime bağlamında yürütür.
    pub fn execute(&mut self, line: &str, vfs: &VirtualFileSystem, pmm: &PhysicalMemoryManager, now: &DateTime) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();

        match cmd {
            "help" => {
                self.push_line("Komutlar: help ls cat echo clear mem ps date uname theme whoami", Color::TEXT_MAIN);
            }
            "ls" => {
                let mut any = false;
                for node in vfs.nodes.iter().flatten() {
                    let mut buf = TextBuf::new();
                    buf.push_str(match node.node_type { NodeType::File => "[F] ", NodeType::Directory => "[D] " });
                    buf.push_str(node.name);
                    self.push_line(buf.as_str(), Color::ACCENT_BLUE);
                    any = true;
                }
                if !any {
                    self.push_line("(bos dizin)", Color::TEXT_MUTED);
                }
            }
            "cat" => {
                if arg.is_empty() {
                    self.push_line("Kullanim: cat <dosya>", Color::ACCENT_YELLOW);
                } else {
                    match vfs.read_file(arg) {
                        Some(data) => {
                            let s = core::str::from_utf8(data).unwrap_or("<ikili veri>");
                            for l in s.split('\n') {
                                self.push_line(l, Color::TEXT_MAIN);
                            }
                        }
                        None => {
                            let mut b = TextBuf::new();
                            b.push_str("Bulunamadi: ");
                            b.push_str(arg);
                            self.push_line(b.as_str(), Color::ACCENT_RED);
                        }
                    }
                }
            }
            "echo" => {
                self.push_line(arg, Color::TEXT_MAIN);
            }
            "clear" => {
                self.out_count = 0;
            }
            "mem" => {
                let mut b = TextBuf::new();
                b.push_str("Toplam: ");
                b.push_usize((pmm.total_memory_bytes / 1024) as usize);
                b.push_str(" KB  Bos: ");
                b.push_usize((pmm.free_memory_bytes / 1024) as usize);
                b.push_str(" KB");
                self.push_line(b.as_str(), Color::ACCENT_GREEN);
            }
            "ps" => {
                self.push_line("PID  ISIM", Color::TEXT_MUTED);
                self.push_line("1    kernel_core", Color::TEXT_MAIN);
                self.push_line("2    mingui_compositor", Color::TEXT_MAIN);
                self.push_line("3    net_stack", Color::TEXT_MAIN);
                self.push_line("4    minshell", Color::TEXT_MAIN);
            }
            "date" => {
                let mut b = TextBuf::new();
                b.push_u2(now.day as u32);
                b.push_byte(b' ');
                b.push_str(DateTime::month_name(now.month));
                b.push_byte(b' ');
                b.push_u4(now.year as u32);
                b.push_str("  ");
                b.push_u2(now.hour as u32);
                b.push_byte(b':');
                b.push_u2(now.minute as u32);
                b.push_byte(b':');
                b.push_u2(now.second as u32);
                self.push_line(b.as_str(), Color::ACCENT_BLUE);
            }
            "uname" => {
                self.push_line("MinOS x86_64 Sarman Edition v2.0", Color::ACCENT_PINK);
            }
            "whoami" => {
                self.push_line("sarman (root)", Color::ACCENT_YELLOW);
            }
            "theme" => {
                let t = theme::cycle();
                let mut b = TextBuf::new();
                b.push_str("Tema: ");
                b.push_str(t.name());
                self.push_line(b.as_str(), Color::ACCENT_PINK);
            }
            _ => {
                let mut b = TextBuf::new();
                b.push_str("Bilinmeyen komut: ");
                b.push_str(cmd);
                self.push_line(b.as_str(), Color::ACCENT_RED);
                self.push_line("'help' yazin.", Color::TEXT_MUTED);
            }
        }
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) {
        // Terminal zemin
        fb.draw_rect(x + 8, y + 34, w.saturating_sub(16), h.saturating_sub(44), Color::BLACK);

        // Başlık
        MinGuiCompositor::draw_string(fb, x + 16, y + 44, "MinOS Komut Istemi v2.0 (MinShell)", Color::ACCENT_GREEN);

        // Çıktı satırları
        let mut ly = y + 60;
        for i in 0..self.out_count {
            MinGuiCompositor::draw_string(fb, x + 16, ly, self.out[i].as_str(), self.out[i].color);
            ly += 13;
        }

        // Aktif Istem + Girdi
        let prompt_y = y + h.saturating_sub(26);
        MinGuiCompositor::draw_string(fb, x + 16, prompt_y, ">", Color::ACCENT_GREEN);
        let input_str = core::str::from_utf8(&self.input[..self.input_len]).unwrap_or("");
        MinGuiCompositor::draw_string(fb, x + 30, prompt_y, input_str, Color::TEXT_MAIN);

        // Durum satırı
        let mut status = TextBuf::new();
        status.push_str("gecmis: ");
        status.push_usize(self.history_count);
        status.push_str(" komut | UTF-8 / TR-Q");
        MinGuiCompositor::draw_string(fb, x + 16, y + h.saturating_sub(12), status.as_str(), Color::TEXT_MUTED);
    }
}

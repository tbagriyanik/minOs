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

const COMPLETION_COMMANDS: &[&str] = &[
    "help", "ls", "cat", "echo", "clear", "mem", "ps", "apps", "ipconfig", "ping", "dns",
    "date", "uname", "whoami", "theme", "open", "run", "files", "settings", "notepad",
    "browser", "image", "music", "video", "taskmgr", "calc", "paint", "cmd",
];
const COMPLETION_APPS: &[&str] = &[
    "files", "settings", "notepad", "browser", "image", "music", "video", "taskmgr",
    "calc", "paint", "cmd",
];

fn app_index(name: &str) -> Option<usize> {
    match name {
        "files" | "filemanager" | "explorer" | "dosyalar" => Some(0),
        "settings" | "ayarlar" | "config" => Some(1),
        "notepad" | "notes" | "editor" | "notdefteri" => Some(2),
        "browser" | "web" | "internet" => Some(3),
        "image" | "imageviewer" | "photos" | "resim" => Some(4),
        "music" | "musicplayer" | "muzik" => Some(5),
        "video" | "videoplayer" => Some(6),
        "taskmgr" | "taskmanager" | "tasks" | "gorev" => Some(7),
        "calc" | "calculator" | "hesap" => Some(8),
        "paint" | "draw" | "cizim" => Some(9),
        "cmd" | "terminal" | "shell" => Some(10),
        _ => None,
    }
}

fn app_name(index: usize) -> &'static str {
    match (index, crate::gui::ui_english()) {
        (0,false) => "Dosya Yöneticisi", (1,false) => "Ayarlar", (2,false) => "Not Defteri",
        (3,false) => "Web Gezgini", (4,false) => "Resim Gösterici", (5,false) => "Müzik Çalar",
        (6,false) => "Video Oynatıcı", (7,false) => "Görev Yöneticisi", (8,false) => "Hesap Makinesi",
        (9,false) => "Çizim Tahtası", (10,false) => "Komut İstemi",
        (0,true) => "File Manager", (1,true) => "Settings", (2,true) => "Notepad", (3,true) => "Browser",
        (4,true) => "Image Viewer", (5,true) => "Music Player", (6,true) => "Video Player",
        (7,true) => "Task Manager", (8,true) => "Calculator", (9,true) => "Paint", _ => "Terminal",
    }
}

fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    let mut parts = text.split('.');
    let mut address = [0u8; 4];
    for octet in &mut address {
        *octet = parts.next()?.parse::<u8>().ok()?;
    }
    if parts.next().is_some() { return None; }
    Some(address)
}

fn push_ipv4(text: &mut TextBuf, address: [u8; 4]) {
    for (i, octet) in address.iter().enumerate() {
        if i > 0 { text.push_byte(b'.'); }
        text.push_usize(*octet as usize);
    }
}

fn push_ipv4_line(terminal: &mut TerminalApp, label: &str, address: [u8; 4]) {
    let mut line = TextBuf::new();
    line.push_str(label);
    push_ipv4(&mut line, address);
    terminal.push_line(line.as_str(), Color::TEXT_MAIN);
}

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
    /// Çıktı geçmişinin alt kenardan geriye doğru kaydırma miktarı (satır).
    pub output_scroll: usize,
    /// Komut geçmişi.
    pub history: [[u8; LINE_MAX]; 8],
    pub history_len: [usize; 8],
    pub history_count: usize,
    /// Geçmişte gezinme imleci.
    pub hist_cursor: usize,
    /// Tab ile aynı kısmi girdinin eşleşmeleri arasında gezinme durumu.
    completion_base: [u8; LINE_MAX],
    completion_base_len: usize,
    completion_index: usize,
    completion_active: bool,
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
            output_scroll: 0,
            history: [[0; LINE_MAX]; 8],
            history_len: [0; 8],
            history_count: 0,
            hist_cursor: 0,
            completion_base: [0; LINE_MAX],
            completion_base_len: 0,
            completion_index: 0,
            completion_active: false,
            pending: [0; LINE_MAX],
            pending_len: 0,
            has_pending: false,
        }
    }

    /// Başlangıç banner'ı.
    pub fn init_banner(&mut self) {
        self.push_line(crate::gui::ui_text("Komut İstemi", "Command Prompt"), Color::ACCENT_PINK);
        self.push_line(crate::gui::ui_text("Yardım için 'help' yazın.", "Type 'help' for commands."), Color::TEXT_MUTED);
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
        self.output_scroll = 0;
    }

    /// Terminal çıktı geçmişini fare tekeriyle kaydırır; giriş satırına dokunmaz.
    pub fn scroll_output(&mut self, delta: isize, view_lines: usize) {
        let max_scroll = self.out_count.saturating_sub(view_lines.max(1));
        self.output_scroll = ((self.output_scroll as isize + delta).max(0) as usize).min(max_scroll);
    }

    pub fn insert(&mut self, c: char) {
        self.completion_active = false;
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
        self.completion_active = false;
        if self.input_len > 0 {
            self.input_len -= 1;
        }
    }

    /// Enter'a basılınca: istemi yaz, komutu `pending`'e al (yürütme ana döngüde).
    pub fn submit(&mut self) {
        self.completion_active = false;
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
        self.completion_active = false;
        if self.history_count == 0 {
            return;
        }
        if self.hist_cursor > 0 {
            self.hist_cursor -= 1;
        }
        self.load_history(self.hist_cursor);
    }

    pub fn history_down(&mut self) {
        self.completion_active = false;
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

    /// Tamamlar komut/uygulama adını. Tekrarlanan Tab eşleşmeler arasında gezinir.
    pub fn complete(&mut self) {
        if !self.completion_active {
            self.completion_base[..self.input_len].copy_from_slice(&self.input[..self.input_len]);
            self.completion_base_len = self.input_len;
            self.completion_index = 0;
            self.completion_active = true;
        }

        let base = &self.completion_base[..self.completion_base_len];
        let token_start = base.iter().rposition(|b| *b == b' ').map_or(0, |i| i + 1);
        let prefix = &base[token_start..];
        let app_arg = token_start > 0 && {
            let command = &base[..token_start - 1];
            command.eq_ignore_ascii_case(b"open") || command.eq_ignore_ascii_case(b"run")
        };
        let candidates = if app_arg { COMPLETION_APPS } else { COMPLETION_COMMANDS };
        let mut match_count = 0usize;
        let mut selected = "";
        for candidate in candidates.iter().copied() {
            if candidate.as_bytes().len() >= prefix.len()
                && candidate.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                if match_count == self.completion_index { selected = candidate; }
                match_count += 1;
            }
        }
        if match_count == 0 {
            self.completion_active = false;
            return;
        }
        if self.completion_index >= match_count {
            self.completion_index = 0;
            selected = "";
            for candidate in candidates.iter().copied() {
                if candidate.as_bytes().len() >= prefix.len()
                    && candidate.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix)
                {
                    selected = candidate;
                    break;
                }
            }
        }
        let new_len = token_start + selected.len();
        if new_len >= LINE_MAX { return; }
        self.input[..token_start].copy_from_slice(&base[..token_start]);
        self.input[token_start..new_len].copy_from_slice(selected.as_bytes());
        self.input_len = new_len;
        self.completion_index = (self.completion_index + 1) % match_count;
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
    pub fn execute(&mut self, line: &str, vfs: &VirtualFileSystem, pmm: &PhysicalMemoryManager, now: &DateTime) -> Option<usize> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();

        if cmd == "open" || cmd == "run" {
            if let Some(app_idx) = app_index(arg) {
                let mut msg = TextBuf::new();
                msg.push_str(crate::gui::ui_text("Açılıyor: ", "Opening: "));
                msg.push_str(app_name(app_idx));
                self.push_line(msg.as_str(), Color::ACCENT_GREEN);
                return Some(app_idx);
            }
            self.push_line(crate::gui::ui_text("Kullanım: open <uygulama>", "Usage: open <app>"), Color::ACCENT_YELLOW);
            return None;
        }
        if let Some(app_idx) = app_index(cmd) {
            let mut msg = TextBuf::new();
            msg.push_str(crate::gui::ui_text("Açılıyor: ", "Opening: "));
            msg.push_str(app_name(app_idx));
            self.push_line(msg.as_str(), Color::ACCENT_GREEN);
            return Some(app_idx);
        }

        match cmd {
            "help" => {
                self.push_line("help ls cat echo clear mem ps date uname theme whoami", Color::TEXT_MAIN);
                self.push_line("ping <IPv4> | dns <host> | ipconfig | apps | open <app>", Color::ACCENT_BLUE);
                self.push_line("apps: files settings notepad browser image music", Color::ACCENT_PINK);
                self.push_line("video taskmgr calc paint cmd", Color::ACCENT_PINK);
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
                    self.push_line(crate::gui::ui_text("(boş dizin)", "(empty directory)"), Color::TEXT_MUTED);
                }
            }
            "cat" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: cat <dosya>", "Usage: cat <file>"), Color::ACCENT_YELLOW);
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
                            b.push_str(crate::gui::ui_text("Bulunamadı: ", "Not found: "));
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
                b.push_str(crate::gui::ui_text("Toplam: ", "Total: "));
                b.push_usize((pmm.total_memory_bytes / 1024) as usize);
                b.push_str(crate::gui::ui_text(" KB  Boş: ", " KB  Free: "));
                b.push_usize((pmm.free_memory_bytes / 1024) as usize);
                b.push_str(" KB");
                self.push_line(b.as_str(), Color::ACCENT_GREEN);
            }
            "ps" => {
                self.push_line(crate::gui::ui_text("PID  İSİM", "PID  NAME"), Color::TEXT_MUTED);
                self.push_line("1    kernel_core", Color::TEXT_MAIN);
                self.push_line("2    mingui_compositor", Color::TEXT_MAIN);
                self.push_line("3    net_stack", Color::TEXT_MAIN);
                self.push_line("4    minshell", Color::TEXT_MAIN);
            }
            "apps" => {
                self.push_line("0 files       1 settings    2 notepad", Color::ACCENT_BLUE);
                self.push_line("3 browser     4 image      5 music", Color::ACCENT_BLUE);
                self.push_line("6 video       7 taskmgr    8 calc", Color::ACCENT_BLUE);
                self.push_line("9 paint      10 cmd", Color::ACCENT_BLUE);
            }
            "ipconfig" => {
                use crate::drivers::netstack::{gateway_ip, local_ip, subnet_mask};
                self.push_line(crate::gui::ui_text("Ethernet bağdaştırıcısı eth0:", "Ethernet adapter eth0:"), Color::ACCENT_GREEN);
                push_ipv4_line(self, "IPv4: ", local_ip().0);
                push_ipv4_line(self, crate::gui::ui_text("Maske: ", "Mask: "), subnet_mask().0);
                push_ipv4_line(self, crate::gui::ui_text("Ağ geçidi: ", "Gateway: "), gateway_ip().0);
                let mac = unsafe { crate::drivers::e1000::MAC_ADDR };
                let mut b = TextBuf::new();
                b.push_str("MAC: ");
                for (i, octet) in mac.iter().enumerate() {
                    if i > 0 { b.push_byte(b':'); }
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    b.push_byte(HEX[(octet >> 4) as usize]);
                    b.push_byte(HEX[(octet & 0x0F) as usize]);
                }
                self.push_line(b.as_str(), Color::TEXT_MAIN);
                self.push_line(if crate::drivers::e1000::is_ready() { "Link: ready" } else { "Link: unavailable" }, Color::ACCENT_YELLOW);
            }
            "ping" => {
                match parse_ipv4(arg) {
                    Some(_) if !crate::drivers::e1000::is_ready() => {
                        self.push_line(crate::gui::ui_text("Ağ bağdaştırıcısı bulunamadı.", "Network adapter is unavailable."), Color::ACCENT_RED);
                    }
                    Some(ip) => {
                        let sent = crate::drivers::netstack::ping(crate::drivers::netstack::Ipv4Address(ip));
                        let mut b = TextBuf::new();
                        b.push_str(if sent { "Ping tx: " } else { "Ping failed: " });
                        push_ipv4(&mut b, ip);
                        self.push_line(b.as_str(), if sent { Color::ACCENT_GREEN } else { Color::ACCENT_RED });
                        if sent { self.push_line(crate::gui::ui_text("Echo isteği gönderildi; yanıt bekleme henüz yok.", "Echo request transmitted; reply wait is unavailable."), Color::TEXT_MUTED); }
                    }
                    None => self.push_line(crate::gui::ui_text("Kullanım: ping <IPv4 adresi>", "Usage: ping <IPv4 address>"), Color::ACCENT_YELLOW),
                }
            }
            "dns" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: dns <alan adı>", "Usage: dns <hostname>"), Color::ACCENT_YELLOW);
                } else {
                    let sent = crate::drivers::netstack::dns_query(arg);
                    self.push_line(if sent { "DNS query transmitted" } else { "DNS query failed" }, if sent { Color::ACCENT_GREEN } else { Color::ACCENT_RED });
                }
            }
            "date" => {
                let mut b = TextBuf::new();
                b.push_u2(now.day as u32);
                b.push_byte(b' ');
                b.push_str(if crate::gui::ui_english() { DateTime::month_name_en(now.month) } else { DateTime::month_name(now.month) });
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
                b.push_str(crate::gui::ui_text("Tema: ", "Theme: "));
                b.push_str(t.name());
                self.push_line(b.as_str(), Color::ACCENT_PINK);
            }
            _ => {
                let mut b = TextBuf::new();
                b.push_str(crate::gui::ui_text("Bilinmeyen komut: ", "Unknown command: "));
                b.push_str(cmd);
                self.push_line(b.as_str(), Color::ACCENT_RED);
                self.push_line(crate::gui::ui_text("'help' yazın.", "Type 'help'."), Color::TEXT_MUTED);
            }
        }
        None
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
        MinGuiCompositor::draw_string(fb, x + 16, y + 44, crate::gui::ui_text("MinOS Komut İstemi v2.0 (MinShell)", "MinOS Command Prompt v2.0 (MinShell)"), Color::ACCENT_GREEN);

        // Çıktı geçmişi: giriş satırı ve durum satırından ayrı bir bölgede çizilir.
        let prompt_y = y + h.saturating_sub(26);
        let view_lines = h.saturating_sub(86) / 13;
        let first = self.out_count.saturating_sub(view_lines).saturating_sub(self.output_scroll);
        let last = (first + view_lines).min(self.out_count);
        let mut ly = y + 60;
        for i in first..last {
            MinGuiCompositor::draw_string(fb, x + 16, ly, self.out[i].as_str(), self.out[i].color);
            ly += 13;
        }

        // Aktif Istem + Girdi
        MinGuiCompositor::draw_string(fb, x + 16, prompt_y, ">", Color::ACCENT_GREEN);
        let input_str = core::str::from_utf8(&self.input[..self.input_len]).unwrap_or("");
        MinGuiCompositor::draw_string(fb, x + 30, prompt_y, input_str, Color::TEXT_MAIN);

        // Durum satırı
        let mut status = TextBuf::new();
        status.push_str(crate::gui::ui_text("geçmiş: ", "history: "));
        status.push_usize(self.history_count);
        status.push_str(crate::gui::ui_text(" komut | UTF-8 / TR-Q", " commands | UTF-8 / TR-Q"));
        MinGuiCompositor::draw_string(fb, x + 16, y + h.saturating_sub(12), status.as_str(), Color::TEXT_MUTED);
    }
}

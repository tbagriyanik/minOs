//! MinShell — minOs Komut İstemi (Command Prompt).
//!
//! Gerçek bir komut satırı: girdi tamponu, komut geçmişi, çıktı satırları ve
//! `help`, `ls`, `cat`, `echo`, `clear`, `mem`, `ps`, `date`, `uname`, `theme`,
//! `whoami` gibi yerleşik (built-in) komutlar. VFS ve PMM'e salt-okunur erişir.

use crate::drivers::video::Framebuffer;
use crate::drivers::rtc::DateTime;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::fs::vfs::{VirtualFileSystem, NodeType};
use crate::mm::pmm::PhysicalMemoryManager;

/// Çıktı satırı sayısı (kaydırma geçmişi).
pub const OUT_LINES: usize = 64;
/// Tek satırın azami uzunluğu (karakter sınırı).
pub const LINE_MAX: usize = 256;

const COMPLETION_COMMANDS: &[&str] = &[
    "help", "ls", "cd", "pwd", "mkdir", "touch", "rm", "cat", "write", "disks", "df",
    "format", "mkfs", "echo", "clear", "mem", "ps", "apps", "ipconfig", "ping", "dns", "nslookup", "curl", "wget",
    "date", "uname", "whoami", "theme", "uptime", "fetch", "neofetch", "open", "run", "files", "settings", "notepad",
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
    /// Aktif çalışma dizini (Current Working Directory).
    pub cwd: [u8; 64],
    pub cwd_len: usize,
}

impl TerminalApp {
    pub const fn new() -> Self {
        let mut app = Self {
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
            cwd: [0; 64],
            cwd_len: 1,
        };
        app.cwd[0] = b'/';
        app
    }

    pub fn cwd_str(&self) -> &str {
        if self.cwd_len > 0 {
            core::str::from_utf8(&self.cwd[..self.cwd_len]).unwrap_or("/")
        } else {
            "/"
        }
    }

    pub fn set_cwd(&mut self, path: &str) {
        self.cwd_len = 0;
        for b in path.bytes() {
            if self.cwd_len < self.cwd.len() {
                self.cwd[self.cwd_len] = b;
                self.cwd_len += 1;
            }
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

    /// Çıktının en eskisine (en üste) kaydırır (Home / Ctrl+Home).
    pub fn scroll_top(&mut self, view_lines: usize) {
        self.output_scroll = self.out_count.saturating_sub(view_lines.max(1));
    }

    /// Çıktının en yenisine (en alta) kaydırır (End / Ctrl+End).
    pub fn scroll_bottom(&mut self) {
        self.output_scroll = 0;
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

    pub fn delete_word(&mut self) {
        self.completion_active = false;
        while self.input_len > 0 && self.input[self.input_len - 1] == b' ' {
            self.input_len -= 1;
        }
        while self.input_len > 0 && self.input[self.input_len - 1] != b' ' {
            self.input_len -= 1;
        }
    }

    /// Panoya terminal giriş satırını veya son satırı kopyalar.
    pub fn copy_input(&self) {
        if self.input_len > 0 {
            if let Ok(s) = core::str::from_utf8(&self.input[..self.input_len]) {
                crate::gui::clipboard::set_str(s);
            }
        } else if self.out_count > 0 {
            crate::gui::clipboard::set_str(self.out[self.out_count - 1].as_str());
        }
    }

    /// Panodaki metni terminal girişine ekler.
    pub fn paste_clipboard(&mut self) {
        let mut pasted = ['\0'; crate::gui::clipboard::CAPACITY];
        let len = crate::gui::clipboard::copy_chars(&mut pasted);
        for &ch in &pasted[..len] {
            if ch == '\0' || ch == '\r' { continue; }
            if ch == '\n' { self.submit(); continue; }
            self.insert(ch);
        }
    }

    /// Enter'a basılınca: istemi yaz, komutu `pending`'e al (yürütme ana döngüde).
    pub fn submit(&mut self) {
        self.completion_active = false;
        let n = self.input_len;
        let mut prompt = alloc::string::String::from("> ");
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
    pub fn execute(&mut self, line: &str, vfs: &mut VirtualFileSystem, pmm: &mut PhysicalMemoryManager, now: &DateTime) -> Option<usize> {
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
                self.push_line("help ls cd pwd mkdir touch rm cat write disks df format echo clear mem ps exec", Color::TEXT_MAIN);
                self.push_line("uptime fetch neofetch date uname whoami theme ipconfig ping dns nslookup curl wget", Color::ACCENT_GREEN);
                self.push_line("minpkg list|install|remove | exec <file> | open <app>", Color::ACCENT_BLUE);
                self.push_line("apps: files settings notepad browser image music video taskmgr calc paint cmd", Color::ACCENT_PINK);
            }
            "pwd" => {
                let mut buf = TextBuf::new();
                buf.push_str(self.cwd_str());
                self.push_line(buf.as_str(), Color::ACCENT_BLUE);
            }
            "cd" => {
                if arg.is_empty() || arg == "~" {
                    self.set_cwd("/root/Documents");
                } else if arg == "/" {
                    self.set_cwd("/");
                } else if arg == ".." {
                    let cur = self.cwd_str();
                    if let Some(pos) = cur.rfind('/') {
                        let parent = if pos == 0 { "/" } else { &cur[..pos] };
                        let parent_clone = alloc::string::String::from(parent);
                        self.set_cwd(&parent_clone);
                    } else {
                        self.set_cwd("/");
                    }
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    if vfs.is_dir(&target) || target == "/media/usb0" || target == "/media/cdrom" || target == "/media/hdd0" {
                        self.set_cwd(&target);
                    } else {
                        self.push_line(crate::gui::ui_text("Hata: Dizin bulunamadı.", "Error: Directory not found."), Color::ACCENT_RED);
                    }
                }
            }
            "ls" => {
                let target = if arg.is_empty() { self.cwd_str() } else { arg };
                let items = vfs.list_dir(target);
                if items.is_empty() {
                    self.push_line(crate::gui::ui_text("(boş dizin)", "(empty directory)"), Color::TEXT_MUTED);
                } else {
                    for it in &items {
                        let mut buf = TextBuf::new();
                        match it.node_type {
                            NodeType::Directory => {
                                buf.push_str("[D] ");
                                buf.push_str(&it.name);
                                self.push_line(buf.as_str(), Color::ACCENT_PINK);
                            }
                            NodeType::File => {
                                buf.push_str("[F] ");
                                buf.push_str(&it.name);
                                buf.push_str("  (");
                                buf.push_usize(it.size);
                                buf.push_str(" B)");
                                self.push_line(buf.as_str(), Color::ACCENT_BLUE);
                            }
                        }
                    }
                }
            }
            "mkdir" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: mkdir <klasor>", "Usage: mkdir <dir>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    if vfs.create_dir(&target) {
                        self.push_line(crate::gui::ui_text("Klasör oluşturuldu.", "Directory created."), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(crate::gui::ui_text("Oluşturulamadı.", "Failed to create directory."), Color::ACCENT_RED);
                    }
                }
            }
            "touch" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: touch <dosya>", "Usage: touch <file>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    if vfs.create_file(&target, b"") {
                        self.push_line(crate::gui::ui_text("Dosya oluşturuldu.", "File created."), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(crate::gui::ui_text("Oluşturulamadı.", "Failed to create file."), Color::ACCENT_RED);
                    }
                }
            }
            "rm" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: rm <dosya_veya_klasör>", "Usage: rm <file_or_dir>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    if vfs.move_to_trash(&target) {
                        self.push_line(crate::gui::ui_text("Geri dönüşüm kutusuna taşındı.", "Moved to Recycle Bin."), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(crate::gui::ui_text("Silinemedi.", "Remove failed."), Color::ACCENT_RED);
                    }
                }
            }
            "write" => {
                let mut sp = arg.splitn(2, ' ');
                let file_arg = sp.next().unwrap_or("");
                let text_arg = sp.next().unwrap_or("");
                if file_arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: write <dosya> <metin>", "Usage: write <file> <text>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if file_arg.starts_with('/') {
                        alloc::string::String::from(file_arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(file_arg);
                        p
                    };
                    if vfs.write_file(&target, text_arg.as_bytes()) {
                        self.push_line(crate::gui::ui_text("Dosyaya yazıldı.", "Written to file."), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(crate::gui::ui_text("Yazılamadı.", "Write failed."), Color::ACCENT_RED);
                    }
                }
            }
            "disks" | "df" => {
                self.push_line(crate::gui::ui_text("--- BAĞLI DİSKLER VE SÜRÜCÜLER ---", "--- ATTACHED STORAGE DEVICES ---"), Color::ACCENT_PINK);
                for drive in 0..4u8 {
                    if let Some(dev) = crate::drivers::ata::device(drive) {
                        let mut b = TextBuf::new();
                        b.push_str("IDE ");
                        b.push_usize(drive as usize);
                        b.push_str(": ");
                        match dev.role {
                            crate::drivers::ata::DriveRole::UsbStorage => b.push_str(crate::gui::ui_text("[USB / FAT32] ", "[USB / FAT32] ")),
                            crate::drivers::ata::DriveRole::HardDisk => b.push_str(crate::gui::ui_text("[Sabit Disk] ", "[Hard Disk] ")),
                            crate::drivers::ata::DriveRole::CdRom => b.push_str(crate::gui::ui_text("[CD-ROM / ISO] ", "[CD-ROM / ISO] ")),
                            _ => b.push_str(crate::gui::ui_text("[Bilinmeyen] ", "[Unknown] ")),
                        }
                        b.push_str(dev.model_str());
                        b.push_str(" (");
                        b.push_usize(dev.size_mb() as usize);
                        b.push_str(" MB)");
                        self.push_line(b.as_str(), Color::ACCENT_GREEN);
                    }
                }
                self.push_line(crate::gui::ui_text("Bağlantı: / -> RamFS | /media/usb0 -> FAT32 | /media/cdrom -> ISO", "Mounts: / -> RamFS | /media/usb0 -> FAT32 | /media/cdrom -> ISO"), Color::TEXT_MUTED);
            }
            "format" | "mkfs" => {
                let mut sp = arg.split_whitespace();
                let fs_type = sp.next().unwrap_or("").to_ascii_lowercase();
                let drive_str = sp.next().unwrap_or("");
                let drive_num = if drive_str.starts_with("ide") || drive_str.starts_with("hd") {
                    drive_str.chars().last().and_then(|c| c.to_digit(10)).map(|d| d as u8)
                } else {
                    drive_str.parse::<u8>().ok()
                };

                if fs_type.is_empty() || drive_num.is_none() {
                    self.push_line(crate::gui::ui_text("Kullanım: format <fat32|ext4|exfat|ext2> <0..3>", "Usage: format <fat32|ext4|exfat|ext2> <0..3>"), Color::ACCENT_YELLOW);
                    self.push_line(crate::gui::ui_text("Örnek: format ext4 1  veya  format exfat 0", "Example: format ext4 1  or  format exfat 0"), Color::TEXT_MUTED);
                } else {
                    let drive = drive_num.unwrap();
                    if drive >= 4 || crate::drivers::ata::device(drive).map(|d| d.kind != crate::drivers::ata::DeviceKind::Ata).unwrap_or(true) {
                        self.push_line(crate::gui::ui_text("Hata: Geçersiz veya bağlı olmayan ATA sürücü!", "Error: Invalid or unattached ATA drive!"), Color::ACCENT_RED);
                    } else {
                        let cap = crate::drivers::ata::drive_capacity_sectors(drive);
                        let mut b = TextBuf::new();
                        b.push_str(crate::gui::ui_text("Biçimlendiriliyor: IDE ", "Formatting: IDE "));
                        b.push_usize(drive as usize);
                        b.push_str(" [");
                        b.push_str(&fs_type);
                        b.push_str("]...");
                        self.push_line(b.as_str(), Color::ACCENT_YELLOW);

                        let success = match fs_type.as_str() {
                            "ext4" => crate::fs::ext4::format(drive, 0, cap),
                            "exfat" => crate::fs::exfat::format(drive, 0, cap),
                            "ext2" => crate::fs::ext2::format(drive, 0, cap),
                            "fat32" | "fat" => crate::fs::diskfs::fat_format(drive),
                            _ => {
                                self.push_line(crate::gui::ui_text("Desteklenmeyen dosya sistemi. (fat32, ext4, exfat, ext2)", "Unsupported filesystem. (fat32, ext4, exfat, ext2)"), Color::ACCENT_RED);
                                false
                            }
                        };

                        if success {
                            let mut res = TextBuf::new();
                            res.push_str(crate::gui::ui_text("Başarıyla biçimlendirildi: ", "Successfully formatted: "));
                            res.push_str(&fs_type);
                            self.push_line(res.as_str(), Color::ACCENT_GREEN);
                        } else {
                            self.push_line(crate::gui::ui_text("Biçimlendirme başarısız oldu!", "Formatting failed!"), Color::ACCENT_RED);
                        }
                    }
                }
            }
            "exec" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: exec <ikili_dosya_yolu>", "Usage: exec <binary_path>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    match vfs.read_file(&target) {
                        Some(data) => {
                            let mut aspace = crate::mm::vmm::AddressSpace::current();
                            match crate::mm::loader::AppLoader::load_binary(data, &mut aspace, pmm) {
                                Ok(loaded) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text("[EXEC] ELF/İkili dosya yüklendi: RIP=0x", "[EXEC] ELF/Binary loaded: RIP=0x"));
                                    b.push_hex64(loaded.entry_point);
                                    self.push_line(b.as_str(), Color::ACCENT_GREEN);

                                    let ok = crate::arch::x86_64::ring3::execute_loaded_program(&loaded);
                                    if ok {
                                        self.push_line(crate::gui::ui_text("[EXEC] Program başarıyla çalıştırıldı ve sonlandı.", "[EXEC] Program executed successfully and terminated."), Color::ACCENT_GREEN);
                                    } else {
                                        self.push_line(crate::gui::ui_text("[EXEC] Program çalışırken hata oluştu (izole edildi).", "[EXEC] Program faulted during execution (isolated)."), Color::ACCENT_RED);
                                    }
                                    loaded.unmap_and_free(&mut aspace, pmm);
                                }
                                Err(err) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text("Yükleme hatası: ", "Load error: "));
                                    b.push_str(err);
                                    self.push_line(b.as_str(), Color::ACCENT_RED);
                                }
                            }
                        }
                        None => {
                            let mut b = TextBuf::new();
                            b.push_str(crate::gui::ui_text("Dosya bulunamadı: ", "File not found: "));
                            b.push_str(&target);
                            self.push_line(b.as_str(), Color::ACCENT_RED);
                        }
                    }
                }
            }
            "pkg" | "minpkg" => {
                let mut sp = arg.splitn(2, ' ');
                let subcmd = sp.next().unwrap_or("").trim();
                let subarg = sp.next().unwrap_or("").trim();
                match subcmd {
                    "list" => {
                        self.push_line(crate::gui::ui_text("--- KURULU PAKETLER (/bin) ---", "--- INSTALLED PACKAGES (/bin) ---"), Color::ACCENT_PINK);
                        let pkgs = crate::apps::pkgmgr::MinPkg::list_installed(vfs);
                        if pkgs.is_empty() {
                            self.push_line(crate::gui::ui_text("Kurulu paket bulunamadı.", "No packages installed."), Color::TEXT_MUTED);
                        } else {
                            for p in &pkgs {
                                let mut b = TextBuf::new();
                                b.push_str(&p.name);
                                b.push_str(" (v");
                                b.push_str(&p.version);
                                b.push_str(") -> ");
                                b.push_str(&p.install_path);
                                self.push_line(b.as_str(), Color::ACCENT_GREEN);
                            }
                        }
                    }
                    "install" => {
                        if subarg.is_empty() {
                            self.push_line(crate::gui::ui_text("Kullanım: minpkg install <paket_adi_veya_url>", "Usage: minpkg install <pkg_name_or_url>"), Color::ACCENT_YELLOW);
                        } else {
                            self.push_line(crate::gui::ui_text("Paket kuruluyor...", "Installing package..."), Color::TEXT_MUTED);
                            // Önce ağ üzerinden dene, ağ yoksa veya yerel simülasyonsa yerleşik paketi kur
                            let res = if crate::drivers::e1000::is_ready() && subarg.starts_with("http://") {
                                crate::apps::pkgmgr::MinPkg::install_from_url(subarg, vfs)
                            } else {
                                crate::apps::pkgmgr::MinPkg::install_builtin(subarg, vfs)
                            };
                            match res {
                                Ok(path) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text("Başarıyla kuruldu: ", "Successfully installed: "));
                                    b.push_str(&path);
                                    self.push_line(b.as_str(), Color::ACCENT_GREEN);
                                    let mut exec_hint = TextBuf::new();
                                    exec_hint.push_str(crate::gui::ui_text("Çalıştırmak için: exec ", "To execute: exec "));
                                    exec_hint.push_str(&path);
                                    self.push_line(exec_hint.as_str(), Color::ACCENT_BLUE);
                                }
                                Err(err) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text("Kurulum başarısız: ", "Installation failed: "));
                                    b.push_str(err);
                                    self.push_line(b.as_str(), Color::ACCENT_RED);
                                }
                            }
                        }
                    }
                    "remove" | "uninstall" => {
                        if subarg.is_empty() {
                            self.push_line(crate::gui::ui_text("Kullanım: minpkg remove <paket_adi>", "Usage: minpkg remove <pkg_name>"), Color::ACCENT_YELLOW);
                        } else {
                            if crate::apps::pkgmgr::MinPkg::remove(subarg, vfs) {
                                self.push_line(crate::gui::ui_text("Paket başarıyla kaldırıldı.", "Package removed successfully."), Color::ACCENT_GREEN);
                            } else {
                                self.push_line(crate::gui::ui_text("Paket bulunamadı veya silinemedi.", "Package not found or could not be removed."), Color::ACCENT_RED);
                            }
                        }
                    }
                    _ => {
                        self.push_line(crate::gui::ui_text("minOs Paket Yöneticisi v1.0 (minpkg)", "minOs Package Manager v1.0 (minpkg)"), Color::ACCENT_PINK);
                        self.push_line(crate::gui::ui_text("minpkg list                  - Kurulu paketleri listeler", "minpkg list                  - List installed packages"), Color::TEXT_MAIN);
                        self.push_line(crate::gui::ui_text("minpkg install <paket>       - Paket indirir ve /bin altına kurar", "minpkg install <package>     - Download and install to /bin"), Color::TEXT_MAIN);
                        self.push_line(crate::gui::ui_text("minpkg remove <paket>        - Kurulu paketi siler", "minpkg remove <package>      - Remove installed package"), Color::TEXT_MAIN);
                    }
                }
            }
            "cat" => {
                if arg.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: cat <dosya>", "Usage: cat <file>"), Color::ACCENT_YELLOW);
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') { p.push('/'); }
                        p.push_str(arg);
                        p
                    };
                    match vfs.read_file(&target) {
                        Some(data) => {
                            let s = core::str::from_utf8(data).unwrap_or("<ikili veri>");
                            for l in s.split('\n') {
                                if !l.is_empty() {
                                    self.push_line(l, Color::TEXT_MAIN);
                                }
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
                use crate::drivers::netstack::{dns_server, gateway_ip, local_ip, subnet_mask};
                self.push_line(crate::gui::ui_text("Ethernet bağdaştırıcısı eth0:", "Ethernet adapter eth0:"), Color::ACCENT_GREEN);
                push_ipv4_line(self, "IPv4: ", local_ip().0);
                push_ipv4_line(self, crate::gui::ui_text("Maske: ", "Mask: "), subnet_mask().0);
                push_ipv4_line(self, crate::gui::ui_text("Ağ geçidi: ", "Gateway: "), gateway_ip().0);
                push_ipv4_line(self, "DNS: ", dns_server().0);
                self.push_line(if crate::drivers::netstack::dhcp_ready() {
                    crate::gui::ui_text("DHCP: adres alındı", "DHCP: lease active")
                } else {
                    crate::gui::ui_text("DHCP: bekleniyor (yedek adres etkin)", "DHCP: waiting (fallback address active)")
                }, Color::ACCENT_YELLOW);
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
                self.push_line(if crate::drivers::e1000::is_ready() {
                    crate::gui::ui_text("Bağlantı: hazır", "Link: ready")
                } else {
                    crate::gui::ui_text("Bağlantı: kullanılamıyor", "Link: unavailable")
                }, Color::ACCENT_YELLOW);
            }
            "ping" => {
                let mut sp = arg.split_whitespace();
                let mut count = 4usize;
                let mut target = "";
                while let Some(tok) = sp.next() {
                    if tok == "-c" || tok == "-n" {
                        if let Some(cnt_str) = sp.next() {
                            if let Ok(c) = cnt_str.parse::<usize>() {
                                count = c.clamp(1, 10);
                            }
                        }
                    } else if !tok.starts_with('-') && target.is_empty() {
                        target = tok;
                    }
                }

                if target.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: ping [-c adet] <hedef_veya_ip>", "Usage: ping [-c count] <target_or_ip>"), Color::ACCENT_YELLOW);
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(crate::gui::ui_text("Ağ bağdaştırıcısı bulunamadı.", "Network adapter is unavailable."), Color::ACCENT_RED);
                } else {
                    let target_ip = if let Some(ip) = parse_ipv4(target) {
                        Some(crate::drivers::netstack::Ipv4Address(ip))
                    } else {
                        let mut msg = TextBuf::new();
                        msg.push_str(crate::gui::ui_text("Alan adı çözümleniyor: ", "Resolving host: "));
                        msg.push_str(target);
                        msg.push_str("...");
                        self.push_line(msg.as_str(), Color::TEXT_MUTED);
                        match crate::drivers::socket::resolve_host(target) {
                            Ok(ip) => Some(ip),
                            Err(_) => {
                                let mut err = TextBuf::new();
                                err.push_str(crate::gui::ui_text("Ping isteği hedef ana bilgisayarı bulamadı: ", "Ping request could not find host: "));
                                err.push_str(target);
                                self.push_line(err.as_str(), Color::ACCENT_RED);
                                None
                            }
                        }
                    };

                    if let Some(ip) = target_ip {
                        let mut banner = TextBuf::new();
                        banner.push_str(target);
                        banner.push_str(" [");
                        push_ipv4(&mut banner, ip.0);
                        banner.push_str(crate::gui::ui_text("] adresi 32 bayt veri ile pingleniyor:", "] with 32 bytes of data:"));
                        self.push_line(banner.as_str(), Color::TEXT_MAIN);

                        let mut recvd = 0usize;
                        let mut min_rtt = u64::MAX;
                        let mut max_rtt = 0u64;
                        let mut sum_rtt = 0u64;

                        for seq in 1..=(count as u16) {
                            crate::drivers::netstack::clear_ping_reply();
                            let t_start = crate::monotonic_ms();
                            let sent = crate::drivers::netstack::ping_ex(ip, seq, 32);
                            if !sent {
                                self.push_line(crate::gui::ui_text("İletim hatası (TX)", "Transmit failure (TX)"), Color::ACCENT_RED);
                            } else {
                                let mut reply: Option<crate::drivers::netstack::PingReply> = None;
                                while crate::monotonic_ms().wrapping_sub(t_start) < 1000 {
                                    crate::drivers::e1000::poll();
                                    crate::drivers::socket::pump_all();
                                    if let Some(r) = crate::drivers::netstack::take_ping_reply() {
                                        if r.src == ip {
                                            reply = Some(r);
                                            break;
                                        }
                                    }
                                    core::hint::spin_loop();
                                }

                                if let Some(r) = reply {
                                    let rtt = crate::monotonic_ms().wrapping_sub(t_start).max(1);
                                    recvd += 1;
                                    sum_rtt += rtt;
                                    min_rtt = min_rtt.min(rtt);
                                    max_rtt = max_rtt.max(rtt);

                                    let mut line = TextBuf::new();
                                    line.push_str(crate::gui::ui_text("Yanıt geldi: ", "Reply from "));
                                    push_ipv4(&mut line, r.src.0);
                                    line.push_str(": bayt=");
                                    line.push_usize(r.len.max(32));
                                    line.push_str(crate::gui::ui_text(" süre=", " time="));
                                    line.push_usize(rtt as usize);
                                    line.push_str("ms TTL=");
                                    line.push_usize(r.ttl as usize);
                                    self.push_line(line.as_str(), Color::ACCENT_GREEN);
                                } else {
                                    self.push_line(crate::gui::ui_text("İstek zaman aşımına uğradı.", "Request timed out."), Color::ACCENT_YELLOW);
                                }
                            }

                            if (seq as usize) < count {
                                let pause_until = crate::monotonic_ms().wrapping_add(150);
                                while crate::monotonic_ms() < pause_until {
                                    crate::drivers::socket::pump_all();
                                    core::hint::spin_loop();
                                }
                            }
                        }

                        let lost = count.saturating_sub(recvd);
                        let loss_pct = (lost * 100) / count.max(1);

                        let mut stat_hdr = TextBuf::new();
                        stat_hdr.push_str(crate::gui::ui_text("--- Ping istatistiği: ", "--- Ping statistics for "));
                        push_ipv4(&mut stat_hdr, ip.0);
                        stat_hdr.push_str(" ---");
                        self.push_line(stat_hdr.as_str(), Color::TEXT_MUTED);

                        let mut stat_pkt = TextBuf::new();
                        stat_pkt.push_str(crate::gui::ui_text("Paket: Gönderilen = ", "Packets: Sent = "));
                        stat_pkt.push_usize(count);
                        stat_pkt.push_str(crate::gui::ui_text(", Alınan = ", ", Received = "));
                        stat_pkt.push_usize(recvd);
                        stat_pkt.push_str(crate::gui::ui_text(", Kayıp = ", ", Lost = "));
                        stat_pkt.push_usize(lost);
                        stat_pkt.push_str(" (%");
                        stat_pkt.push_usize(loss_pct);
                        stat_pkt.push_str(crate::gui::ui_text(" kayıp)", " loss)"));
                        self.push_line(stat_pkt.as_str(), if lost == 0 { Color::ACCENT_GREEN } else { Color::ACCENT_YELLOW });

                        if recvd > 0 {
                            let avg_rtt = sum_rtt / (recvd as u64);
                            let mut stat_time = TextBuf::new();
                            stat_time.push_str(crate::gui::ui_text("Gidiş-dönüş süreleri: En az = ", "Round trip times: Minimum = "));
                            stat_time.push_usize(min_rtt as usize);
                            stat_time.push_str(crate::gui::ui_text("ms, En çok = ", "ms, Maximum = "));
                            stat_time.push_usize(max_rtt as usize);
                            stat_time.push_str(crate::gui::ui_text("ms, Ortalama = ", "ms, Average = "));
                            stat_time.push_usize(avg_rtt as usize);
                            stat_time.push_str("ms");
                            self.push_line(stat_time.as_str(), Color::TEXT_MAIN);
                        }
                    }
                }
            }
            "curl" | "wget" => {
                let is_wget = cmd == "wget";
                let mut sp = arg.split_whitespace();
                let mut out_file: Option<alloc::string::String> = None;
                let mut include_headers = false;
                let mut head_only = false;
                let mut silent = false;
                let mut raw_url: Option<&str> = None;

                while let Some(tok) = sp.next() {
                    if tok == "-o" || tok == "--output" {
                        if let Some(f) = sp.next() {
                            out_file = Some(alloc::string::String::from(f));
                        }
                    } else if tok == "-i" || tok == "--include" {
                        include_headers = true;
                    } else if tok == "-I" || tok == "--head" {
                        head_only = true;
                        include_headers = true;
                    } else if tok == "-s" || tok == "--silent" {
                        silent = true;
                    } else if !tok.starts_with('-') && raw_url.is_none() {
                        raw_url = Some(tok);
                    }
                }

                if raw_url.is_none() {
                    self.push_line(crate::gui::ui_text("Kullanım: curl [-i|-I|-s] [-o dosya] <url>", "Usage: curl [-i|-I|-s] [-o file] <url>"), Color::ACCENT_YELLOW);
                    self.push_line(crate::gui::ui_text("Örnek: curl 10.0.2.2  veya  curl -o sayfa.html http://10.0.2.2:8080/", "Example: curl 10.0.2.2  or  curl -o page.html http://10.0.2.2:8080/"), Color::TEXT_MUTED);
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(crate::gui::ui_text("Ağ bağdaştırıcısı bulunamadı.", "Network adapter is unavailable."), Color::ACCENT_RED);
                } else {
                    let target_url = raw_url.unwrap();
                    let normalized_url = if target_url.starts_with("http://") || target_url.starts_with("https://") || target_url.contains("://") {
                        alloc::string::String::from(target_url)
                    } else {
                        let mut s = alloc::string::String::from("http://");
                        s.push_str(target_url);
                        s
                    };

                    if is_wget && out_file.is_none() {
                        let last_seg = normalized_url.rsplit('/').next().unwrap_or("index.html");
                        let fname = if last_seg.is_empty() || last_seg.contains('?') || last_seg.contains(':') {
                            "index.html"
                        } else {
                            last_seg
                        };
                        out_file = Some(alloc::string::String::from(fname));
                    }

                    if !silent {
                        let mut msg = TextBuf::new();
                        msg.push_str(crate::gui::ui_text("Ağdan veri çekiliyor: ", "Fetching from network: "));
                        msg.push_str(&normalized_url);
                        msg.push_str("...");
                        self.push_line(msg.as_str(), Color::TEXT_MUTED);
                    }

                    let mut buf = alloc::vec::Vec::new();
                    buf.resize(32768, 0);
                    match crate::drivers::http::http_get(&normalized_url, &mut buf) {
                        Ok((status, hlen, total)) => {
                            if !silent {
                                let mut b = TextBuf::new();
                                b.push_str("HTTP ");
                                b.push_usize(status as usize);
                                b.push_str("  (");
                                b.push_usize(total as usize);
                                b.push_str(crate::gui::ui_text(" bayt)", " bytes)"));
                                self.push_line(b.as_str(), if status < 400 { Color::ACCENT_GREEN } else { Color::ACCENT_RED });
                            }

                            let body = &buf[hlen.min(total)..total];

                            if let Some(dest) = out_file {
                                let target_path = if dest.starts_with('/') {
                                    dest
                                } else {
                                    let mut p = alloc::string::String::from(self.cwd_str());
                                    if !p.ends_with('/') { p.push('/'); }
                                    p.push_str(&dest);
                                    p
                                };

                                let saved = if vfs.write_file(&target_path, body) {
                                    true
                                } else {
                                    vfs.create_file(&target_path, body)
                                };

                                if saved {
                                    let mut ok_b = TextBuf::new();
                                    ok_b.push_str(crate::gui::ui_text("Dosyaya kaydedildi: ", "Saved to file: "));
                                    ok_b.push_str(&target_path);
                                    ok_b.push_str(" (");
                                    ok_b.push_usize(body.len());
                                    ok_b.push_str(crate::gui::ui_text(" bayt)", " bytes)"));
                                    self.push_line(ok_b.as_str(), Color::ACCENT_GREEN);
                                } else {
                                    let mut err_b = TextBuf::new();
                                    err_b.push_str(crate::gui::ui_text("Dosyaya yazılamadı: ", "Failed to write file: "));
                                    err_b.push_str(&target_path);
                                    self.push_line(err_b.as_str(), Color::ACCENT_RED);
                                }
                            } else {
                                if include_headers {
                                    let hdr_slice = &buf[..hlen.min(total)];
                                    if let Ok(hdr_text) = core::str::from_utf8(hdr_slice) {
                                        for line in hdr_text.lines() {
                                            if !line.is_empty() {
                                                self.push_line(line, Color::ACCENT_BLUE);
                                            }
                                        }
                                    }
                                }

                                if !head_only {
                                    if body.is_empty() {
                                        self.push_line(crate::gui::ui_text("(boş gövde)", "(empty body)"), Color::TEXT_MUTED);
                                    } else {
                                        let mut line = TextBuf::new();
                                        let mut lines = 0usize;
                                        for &c in body {
                                            if c == b'\n' || line.len() >= 62 {
                                                self.push_line(line.as_str(), Color::TEXT_MAIN);
                                                line.clear();
                                                lines += 1;
                                                if lines >= 60 { break; }
                                                if c == b'\n' { continue; }
                                            }
                                            if c == b'\r' { continue; }
                                            line.push_byte(if c.is_ascii_graphic() || c == b' ' || c == b'\t' { c } else { b'?' });
                                        }
                                        if lines < 60 && !line.is_empty() {
                                            self.push_line(line.as_str(), Color::TEXT_MAIN);
                                        } else if lines >= 60 {
                                            self.push_line(crate::gui::ui_text("... (kısaltıldı)", "... (truncated)"), Color::TEXT_MUTED);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let mut err_b = TextBuf::new();
                            err_b.push_str("curl: ");
                            match e {
                                crate::drivers::socket::E_TIMEOUT => {
                                    err_b.push_str(crate::gui::ui_text("(28) Bağlantı zaman aşımı: sunucudan yanıt alınamadı.", "(28) Connection timed out: no response from server."));
                                }
                                crate::drivers::socket::E_CONNREFUSED => {
                                    err_b.push_str(crate::gui::ui_text("(7) Bağlantı reddedildi: hedef port kapalı.", "(7) Failed to connect: connection refused."));
                                }
                                crate::drivers::socket::E_NOTCONN => {
                                    err_b.push_str(crate::gui::ui_text("(6) Ağ bağlantısı yok veya ana bilgisayar çözümlenemedi.", "(6) Could not resolve host or network unavailable."));
                                }
                                crate::drivers::http::E_URL => {
                                    err_b.push_str(crate::gui::ui_text("(3) Geçersiz URL sözdizimi.", "(3) Invalid URL format."));
                                }
                                crate::drivers::http::E_REDIRECT => {
                                    err_b.push_str(crate::gui::ui_text("(47) Çok fazla HTTP yönlendirmesi (maksimum 3).", "(47) Maximum redirection limit exceeded."));
                                }
                                crate::drivers::http::E_HTTP => {
                                    err_b.push_str(crate::gui::ui_text("(52) Sunucu geçersiz/boş HTTP yanıtı döndürdü.", "(52) Empty or invalid HTTP response from server."));
                                }
                                _ => {
                                    err_b.push_str(crate::gui::ui_text("İndirme başarısız (hata kodu: ", "Download failed (error code: "));
                                    err_b.push_usize((-e).max(0) as usize);
                                    err_b.push_byte(b')');
                                }
                            }
                            self.push_line(err_b.as_str(), Color::ACCENT_RED);
                        }
                    }
                }
            }
            "dns" | "nslookup" => {
                let name = arg.trim();
                if name.is_empty() {
                    self.push_line(crate::gui::ui_text("Kullanım: dns <alan_adı>  veya  nslookup <alan_adı>", "Usage: dns <hostname>  or  nslookup <hostname>"), Color::ACCENT_YELLOW);
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(crate::gui::ui_text("Ağ bağdaştırıcısı bulunamadı.", "Network adapter is unavailable."), Color::ACCENT_RED);
                } else {
                    let mut srv = TextBuf::new();
                    srv.push_str(crate::gui::ui_text("Sunucu: ", "Server: "));
                    push_ipv4(&mut srv, crate::drivers::netstack::dns_server().0);
                    self.push_line(srv.as_str(), Color::TEXT_MUTED);

                    match crate::drivers::socket::resolve_host(name) {
                        Ok(ip) => {
                            let mut res = TextBuf::new();
                            res.push_str(crate::gui::ui_text("Ad:     ", "Name:   "));
                            res.push_str(name);
                            self.push_line(res.as_str(), Color::TEXT_MAIN);

                            let mut addr = TextBuf::new();
                            addr.push_str(crate::gui::ui_text("Adres:  ", "Address: "));
                            push_ipv4(&mut addr, ip.0);
                            self.push_line(addr.as_str(), Color::ACCENT_GREEN);
                        }
                        Err(code) => {
                            let mut err_b = TextBuf::new();
                            err_b.push_str(crate::gui::ui_text("DNS çözümlenemedi (hata kodu: ", "DNS resolution failed (error code: "));
                            err_b.push_u2((-code).max(0) as u32);
                            err_b.push_byte(b')');
                            self.push_line(err_b.as_str(), Color::ACCENT_RED);
                        }
                    }
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
            "uptime" => {
                let ms = crate::monotonic_ms();
                let secs = ms / 1000;
                let mins = secs / 60;
                let hours = mins / 60;
                let mut b = TextBuf::new();
                b.push_str(crate::gui::ui_text("Çalışma Süresi: ", "Uptime: "));
                b.push_usize(hours as usize);
                b.push_str("h ");
                b.push_usize((mins % 60) as usize);
                b.push_str("m ");
                b.push_usize((secs % 60) as usize);
                b.push_str("s (");
                b.push_usize(ms as usize);
                b.push_str(" ms)");
                self.push_line(b.as_str(), Color::ACCENT_GREEN);
            }
            "fetch" | "neofetch" => {
                self.push_line("   /\\_/\\     minOs Sarman Edition v2.0", Color::ACCENT_PINK);
                self.push_line("  ( o.o )    -------------------------", Color::ACCENT_PINK);
                self.push_line("   > ^ <     OS: minOs x86_64 [Rust no_std]", Color::ACCENT_BLUE);
                self.push_line("             Arch: AMD64 / Long Mode", Color::TEXT_MAIN);
                let mut m = TextBuf::new();
                m.push_str("             RAM: ");
                m.push_usize((pmm.used_memory_bytes / 1024 / 1024) as usize);
                m.push_str(" MB / ");
                m.push_usize((pmm.total_memory_bytes / 1024 / 1024) as usize);
                m.push_str(" MB");
                self.push_line(m.as_str(), Color::ACCENT_YELLOW);
                let mut f = TextBuf::new();
                f.push_str("             VFS: ");
                f.push_usize(vfs.items.len());
                f.push_str(crate::gui::ui_text(" dosya/dizin", " nodes registered"));
                self.push_line(f.as_str(), Color::ACCENT_GREEN);
                self.push_line("             GUI: MinGuiCompositor 60FPS", Color::TEXT_MUTED);
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
        is_focused: bool,
    ) {
        let is_light = crate::gui::theme::current() == crate::gui::theme::ThemeId::Daylight;
        let term_bg = if is_light { 0xF5F7FA } else { 0x0C0D14 };
        let default_fg = if is_light { 0x1A1C23 } else { 0xE6E9F5 };
        let muted_fg = if is_light { 0x606575 } else { 0x8C92A4 };
        let prompt_fg = if is_light { 0x1E8E3E } else { Color::ACCENT_GREEN };
        let title_fg = if is_light { 0x188038 } else { Color::ACCENT_GREEN };

        // Terminal zemin (temaya duyarlı iç konsol kutusu)
        fb.draw_rect(x + 8, y + 34, w.saturating_sub(16), h.saturating_sub(44), term_bg);
        fb.draw_rect_outline(x + 8, y + 34, w.saturating_sub(16), h.saturating_sub(44), Color::BORDER);

        // Başlık
        MinGuiCompositor::draw_terminal_string(fb, x + 16, y + 44, crate::gui::ui_text("minOs Komut İstemi v2.0 (MinShell)", "minOs Command Prompt v2.0 (MinShell)"), title_fg);

        let line_height = crate::gui::font_render::text_height(
            crate::gui::font_render::Face::Geist,
            14.0 * crate::gui::font_zoom() as f32 / 100.0,
        ).max(1);
        let line_gap = 5;
        let status_y = y + h.saturating_sub(line_height + 14);
        let prompt_y = status_y.saturating_sub(line_height + 8);
        let output_y = y + 60;
        let view_lines = prompt_y.saturating_sub(output_y) / (line_height + line_gap);
        let first = self.out_count.saturating_sub(view_lines).saturating_sub(self.output_scroll);
        let last = (first + view_lines).min(self.out_count);
        let mut ly = output_y;
        let content_clip = (x + 12, y + 36, w.saturating_sub(24), h.saturating_sub(48));
        for i in first..last {
            let col = if self.out[i].color == Color::TEXT_MAIN {
                default_fg
            } else if self.out[i].color == Color::TEXT_MUTED {
                muted_fg
            } else {
                self.out[i].color
            };
            MinGuiCompositor::draw_terminal_string_clipped(fb, x + 16, ly, self.out[i].as_str(), col, content_clip);
            ly += line_height + line_gap;
        }

        // Aktif Istem + Girdi
        MinGuiCompositor::draw_terminal_string(fb, x + 16, prompt_y, ">", prompt_fg);
        let input_str = core::str::from_utf8(&self.input[..self.input_len]).unwrap_or("");
        let input_clip = (x + 30, prompt_y, w.saturating_sub(42), line_height + 4);
        MinGuiCompositor::draw_terminal_string_clipped(fb, x + 30, prompt_y, input_str, default_fg, input_clip);
        if is_focused {
            let caret_x = x + 30 + MinGuiCompositor::terminal_text_width(input_str);
            if caret_x < x + w.saturating_sub(16) {
                fb.draw_rect(caret_x, prompt_y + 2, 3, line_height.saturating_sub(4).max(4), prompt_fg);
            }
        }

        // Durum satırı
        let mut status = TextBuf::new();
        status.push_str(crate::gui::ui_text("geçmiş: ", "history: "));
        status.push_usize(self.history_count);
        status.push_str(crate::gui::ui_text(" komut | UTF-8 / TR-Q", " commands | UTF-8 / TR-Q"));
        MinGuiCompositor::draw_terminal_string(fb, x + 16, status_y, status.as_str(), muted_fg);
    }
}

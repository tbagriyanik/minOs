//! MinShell — minOs Komut İstemi (Command Prompt).
//!
//! Gerçek bir komut satırı: girdi tamponu, komut geçmişi, çıktı satırları ve
//! `help`, `ls`, `cat`, `echo`, `clear`, `mem`, `ps`, `date`, `uname`, `theme`,
//! `whoami` gibi yerleşik (built-in) komutlar. VFS ve PMM'e salt-okunur erişir.

use crate::drivers::rtc::DateTime;
use crate::drivers::video::Framebuffer;
use crate::fs::vfs::{NodeType, VirtualFileSystem};
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;

/// Çıktı satırı sayısı (kaydırma geçmişi).
pub const OUT_LINES: usize = 64;
/// Tek satırın azami uzunluğu (karakter sınırı).
pub const LINE_MAX: usize = 256;

const COMPLETION_COMMANDS: &[&str] = &[
    "help",
    "ls",
    "cd",
    "pwd",
    "mkdir",
    "touch",
    "rm",
    "cat",
    "write",
    "disks",
    "df",
    "usb",
    "lsusb",
    "cam",
    "wifi",
    "format",
    "mkfs",
    "echo",
    "clear",
    "mem",
    "ps",
    "apps",
    "ipconfig",
    "ping",
    "ping6",
    "dns",
    "nslookup",
    "curl",
    "wget",
    "date",
    "uname",
    "whoami",
    "theme",
    "uptime",
    "fetch",
    "neofetch",
    "hexdump",
    "nc",
    "fortune",
    "benchmark",
    "sarman-cli",
    "open",
    "run",
    "files",
    "settings",
    "notepad",
    "browser",
    "image",
    "music",
    "video",
    "taskmgr",
    "calc",
    "paint",
    "cmd",
    "minPak",
    "grep",
    "wc",
    "sort",
    "head",
    "tail",
    "tee",
    "dir",
    "cls",
    "type",
    "md",
    "rd",
    "del",
    "copy",
    "move",
    "mv",
    "ren",
    "edit",
    "which",
    "where",
    "ver",
    "set",
    "exit",
];
const COMPLETION_APPS: &[&str] = &[
    "files",
    "settings",
    "notepad",
    "browser",
    "image",
    "music",
    "video",
    "taskmgr",
    "calc",
    "paint",
    "cmd",
    "installer",
    "3d",
    "pkgmgr",
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
        "install" | "installer" | "kur" => Some(11),
        "3d" | "viewer3d" => Some(12),
        "pkgmgr" | "packagemgr" | "packages" | "paket" => Some(13),
        "panel" | "controlpanel" | "control" | "kontrol" => Some(14),
        _ => None,
    }
}

fn app_name(index: usize) -> &'static str {
    match (index, crate::gui::ui_english()) {
        (0, false) => "Dosya Yöneticisi",
        (1, false) => "Ayarlar",
        (2, false) => "Not Defteri",
        (3, false) => "Web Gezgini",
        (4, false) => "Resim Gösterici",
        (5, false) => "Müzik Çalar",
        (6, false) => "Video Oynatıcı",
        (7, false) => "Görev Yöneticisi",
        (8, false) => "Hesap Makinesi",
        (9, false) => "Çizim Tahtası",
        (10, false) => "Komut İstemi",
        (11, false) => "Kurulum Sihirbazı",
        (12, false) => "3D Gösterici",
        (13, false) => "Paket Yöneticisi",
        (14, false) => "Kontrol Paneli",
        (0, true) => "File Manager",
        (1, true) => "Settings",
        (2, true) => "Notepad",
        (3, true) => "Browser",
        (4, true) => "Image Viewer",
        (5, true) => "Music Player",
        (6, true) => "Video Player",
        (7, true) => "Task Manager",
        (8, true) => "Calculator",
        (9, true) => "Paint",
        (10, true) => "Terminal",
        (11, true) => "Installer",
        (12, true) => "3D Viewer",
        (13, true) => "Package Manager",
        (14, true) => "Control Panel",
        _ => "Package Manager",
    }
}

/// Farkli kabuk geleneklerinden gelen komut adini tek bir kanonik ada esler.
///
/// CMD (Windows), Unix ve minOs adlari ayni komut isteminde birlikte
/// kullanilabildigi icin (`dir`/`ls`, `cls`/`clear`, `type`/`cat`, `del`/`rm` ...)
/// yurutmeden once bu tablo uygulanir.
fn canonical_command(cmd: &str) -> &'static str {
    const ALIASES: &[(&str, &str)] = &[
        ("dir", "ls"),
        ("cls", "clear"),
        ("type", "cat"),
        ("md", "mkdir"),
        ("rd", "rm"),
        ("rmdir", "rm"),
        ("del", "rm"),
        ("erase", "rm"),
        ("copy", "cp"),
        ("move", "mv"),
        ("ren", "rename"),
        ("chdir", "cd"),
        ("cwd", "pwd"),
        ("which", "where"),
        ("ipconfig", "ipconfig"),
        ("cat", "cat"),
        ("mkdir", "mkdir"),
        ("rm", "rm"),
        ("cp", "cp"),
        ("mv", "mv"),
        ("rename", "rename"),
        ("ls", "ls"),
        ("clear", "clear"),
        ("cd", "cd"),
        ("pwd", "pwd"),
    ];
    for (alias, target) in ALIASES.iter().copied() {
        if cmd.eq_ignore_ascii_case(alias) {
            return target;
        }
    }
    let bytes = cmd.as_bytes();
    for candidate in COMPLETION_COMMANDS.iter().copied() {
        if candidate.as_bytes().eq_ignore_ascii_case(bytes) {
            return candidate;
        }
    }
    // "minPak" gibi karisik buyuk/kucuk harfli komutlar icin referans kimligi
    // kullanilir; cagiran taraf esitlik karsilastirmasini ASCII-duyarsiz yapar.
    for candidate in COMPLETION_COMMANDS.iter().copied() {
        if candidate == cmd {
            return candidate;
        }
    }
    ""
}

/// Kabuk satirini bosluklardan ayirir, tirnakli parcalari tek arguman sayar.
fn split_args(text: &str) -> alloc::vec::Vec<alloc::string::String> {
    let mut args = alloc::vec::Vec::new();
    let mut current = alloc::string::String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == ' ' || ch == '\t' => {
                if !current.is_empty() {
                    args.push(core::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    let mut parts = text.split('.');
    let mut address = [0u8; 4];
    for octet in &mut address {
        *octet = parts.next()?.parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(address)
}

fn push_ipv4(text: &mut TextBuf, address: [u8; 4]) {
    for (i, octet) in address.iter().enumerate() {
        if i > 0 {
            text.push_byte(b'.');
        }
        text.push_usize(*octet as usize);
    }
}

fn push_ipv4_line(terminal: &mut TerminalApp, label: &str, address: [u8; 4]) {
    let mut line = TextBuf::new();
    line.push_str(label);
    push_ipv4(&mut line, address);
    terminal.push_line(line.as_str(), Color::TEXT_MAIN);
}

fn push_ipv6_line(
    terminal: &mut TerminalApp,
    label: &str,
    address: crate::drivers::netstack::Ipv6Address,
) {
    let mut line = TextBuf::new();
    line.push_str(label);
    let s = address.format();
    line.push_str(&s);
    terminal.push_line(line.as_str(), Color::TEXT_MAIN);
}

/// Kabuk Metinlerini tirnak isaretlerine saygi gostererek ayirir.
///
/// `echo "a | b"` gibi ifadelerde boru hatti yanlislikla bolunmez.
fn split_shell(line: &str, delimiter: char) -> alloc::vec::Vec<&str> {
    let mut parts = alloc::vec::Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == delimiter => {
                parts.push(&line[start..index]);
                start = index + ch.len_utf8();
            }
            None => {}
        }
    }
    parts.push(&line[start..]);
    parts
}

/// Kabuk sozdizimindeki tirnaklari ve ters bolu kacislarini temizler.
fn unquote_shell(text: &str) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => out.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None => out.push(ch),
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Bir asamanin `>` / `>>` yonlendirmesini ayirir.
///
/// Donus: (calistirilacak komut, istege bagli (hedef dosya, ekleme_modu)).
fn parse_redirection(segment: &str) -> (&str, Option<(alloc::string::String, bool)>) {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in segment.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '>' => {
                let rest = &segment[index + 1..];
                let (target_start, append) = if rest.starts_with('>') {
                    (index + 2, true)
                } else {
                    (index + 1, false)
                };
                let command = segment[..index].trim();
                let target = unquote_shell(segment[target_start..].trim());
                return (command, Some((target, append)));
            }
            None => {}
        }
    }
    (segment.trim(), None)
}

#[derive(Copy, Clone)]
pub struct OutLine {
    pub text: [u8; LINE_MAX],
    pub len: usize,
    pub color: u32,
}

impl OutLine {
    pub const fn new() -> Self {
        Self {
            text: [0; LINE_MAX],
            len: 0,
            color: 0,
        }
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
    /// Boru hattı (pipe) aşaması çıktısını (metin, renk) toplarken dolu olur.
    capture: Option<alloc::vec::Vec<(alloc::string::String, u32)>>,
    /// Boru hattından gelen standart girdi.
    stdin: Option<alloc::string::String>,
    /// `edit`/`start` ile duzenlemek uzere acilmasi istenen dosya yolu.
    open_request: Option<alloc::string::String>,
    /// Boru hattındaki ilk komutun gercek adi (duyarlı hata iletileri icin).
    invoked: [u8; 32],
    invoked_len: usize,
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
            capture: None,
            stdin: None,
            open_request: None,
            invoked: [0; 32],
            invoked_len: 0,
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
        self.push_line(
            crate::gui::ui_text("Komut İstemi", "Command Prompt"),
            Color::ACCENT_PINK,
        );
        self.push_line(
            crate::gui::ui_text("Yardım için 'help' yazın.", "Type 'help' for commands."),
            Color::TEXT_MUTED,
        );
    }

    /// Çıktıya yeni bir satır ekler; tampon dolarsa en eskisini kaydırır.
    ///
    /// Bir boru hattı aşaması çalışırken (`capture` dolu) satır ekrana değil,
    /// bir sonraki komuta aktarılacak tampona yazılır.
    pub fn push_line(&mut self, s: &str, color: u32) {
        if let Some(capture) = self.capture.as_mut() {
            capture.push((alloc::string::String::from(s), color));
            return;
        }
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
        self.output_scroll =
            ((self.output_scroll as isize + delta).max(0) as usize).min(max_scroll);
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
            if ch == '\0' || ch == '\r' {
                continue;
            }
            if ch == '\n' {
                self.submit();
                continue;
            }
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
        let candidates = if app_arg {
            COMPLETION_APPS
        } else {
            COMPLETION_COMMANDS
        };
        let mut match_count = 0usize;
        let mut selected = "";
        for candidate in candidates.iter().copied() {
            if candidate.as_bytes().len() >= prefix.len()
                && candidate.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                if match_count == self.completion_index {
                    selected = candidate;
                }
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
        if new_len >= LINE_MAX {
            return;
        }
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

    /// Bir boru hattı aşamasından toplanan satırları düz metne çevirir.
    fn capture_text(lines: &[(alloc::string::String, u32)]) -> alloc::string::String {
        let mut text = alloc::string::String::new();
        for (line, _) in lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// `edit <dosya>` gibi bir istekte bulunulan dosyayı (yolu mutlak) döndür.
    pub fn take_open_request(&mut self) -> Option<alloc::string::String> {
        self.open_request.take()
    }

    /// Verilen yolu uygun uygulamayla açar; metin dosyasında Not Defteri,
    /// diğerlerinde Web Gezgini kullanılır. Hata durumunda terminale yazar.
    fn launch_path(&mut self, cmd: &str, path_arg: &str) {
        let path = self.resolve_path(path_arg);
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let is_text = matches!(
            ext.as_str(),
            "txt"
                | "md"
                | "rs"
                | "c"
                | "h"
                | "cpp"
                | "js"
                | "json"
                | "toml"
                | "ini"
                | "cfg"
                | "log"
                | "csv"
                | "html"
                | "htm"
                | "css"
                | "sh"
                | "bat"
                | "mlc"
        );
        let app_idx = if is_text || cmd == "edit" { 2 } else { 3 };
        self.open_request = Some(path);
        let mut msg = TextBuf::new();
        msg.push_str(crate::gui::ui_text("Açılıyor: ", "Opening: "));
        msg.push_str(app_name(app_idx));
        msg.push_str("  ");
        msg.push_str(path_arg);
        self.push_line(msg.as_str(), Color::ACCENT_GREEN);
    }

    /// Argümandaki ilk yolu çözer; CMD'nin `/a` gibi anahtarlarını yok sayar.
    fn first_path_arg(&mut self, arg: &str, usage: &str) -> Option<alloc::string::String> {
        const SWITCHES: &[&str] = &["/a", "/b", "/s", "/q", "/f", "/d", "/y", "/w", "/p"];
        for candidate in split_args(arg) {
            if SWITCHES.iter().any(|s| s.eq_ignore_ascii_case(&candidate)) {
                continue;
            }
            return Some(candidate);
        }
        self.push_line(usage, Color::ACCENT_YELLOW);
        None
    }

    /// Göreli bir yolu aktif çalışma dizinine göre çözer.
    fn resolve_path(&self, arg: &str) -> alloc::string::String {
        if arg.starts_with('/') {
            alloc::string::String::from(arg)
        } else {
            let mut path = alloc::string::String::from(self.cwd_str());
            if !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(arg);
            path
        }
    }

    /// Boru hattı girdisini döndürür (`cat`, `grep`, `wc` gibi filtreler için).
    fn stdin_text(&self) -> Option<alloc::string::String> {
        self.stdin.clone()
    }

    /// Verilen komut satırını VFS/PMM/DateTime bağlamında yürütür.
    ///
    /// `|`, `>` ve `>>` kabuk operatörlerini destekler. Her aşamanın çıktısı
    /// yakalanır; boru hattında bir sonraki komuta, yönlendirmede dosyaya,
    /// aksi halde ekrana aktarılır.
    pub fn execute(
        &mut self,
        line: &str,
        vfs: &mut VirtualFileSystem,
        pmm: &mut PhysicalMemoryManager,
        now: &DateTime,
    ) -> Option<usize> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let stages = split_shell(line, '|');
        let has_pipe = stages.len() > 1;
        let single_redirect = !has_pipe && parse_redirection(stages[0]).1.is_some();
        if !has_pipe && !single_redirect {
            return self.execute_single(stages[0].trim(), vfs, pmm, now);
        }

        let stage_count = stages.len();
        let mut input: Option<alloc::string::String> = None;
        let mut launched: Option<usize> = None;

        for (index, raw) in stages.iter().enumerate() {
            let (command, redirect) = parse_redirection(raw);
            if command.is_empty() {
                continue;
            }
            self.capture = Some(alloc::vec::Vec::new());
            self.stdin = input.take();
            let result = self.execute_single(command, vfs, pmm, now);
            self.stdin = None;
            let lines = self.capture.take().unwrap_or_default();
            let output = Self::capture_text(&lines);

            if result.is_some() {
                if stage_count > 1 {
                    self.push_line(
                        crate::gui::ui_text(
                            "Hata: uygulamalar boru hattinda baslatilamaz.",
                            "Error: applications cannot be launched inside a pipeline.",
                        ),
                        Color::ACCENT_RED,
                    );
                } else {
                    launched = result;
                }
            }

            if let Some((target, append)) = redirect {
                if target.is_empty() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Hata: yonlendirme hedefi eksik.",
                            "Error: missing redirection target.",
                        ),
                        Color::ACCENT_RED,
                    );
                } else {
                    let path = self.resolve_path(&target);
                    let mut data = if append {
                        vfs.read_file(&path)
                            .map(|bytes| alloc::vec::Vec::from(bytes))
                            .unwrap_or_default()
                    } else {
                        alloc::vec::Vec::new()
                    };
                    data.extend_from_slice(output.as_bytes());
                    if vfs.write_file(&path, &data) {
                        let mut msg = TextBuf::new();
                        msg.push_str(crate::gui::ui_text("Yazildi: ", "Written: "));
                        msg.push_str(&path);
                        self.push_line(msg.as_str(), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(
                            crate::gui::ui_text(
                                "Hata: dosyaya yazilamadi.",
                                "Error: could not write file.",
                            ),
                            Color::ACCENT_RED,
                        );
                    }
                }
            } else if index + 1 < stage_count {
                input = Some(output);
            } else {
                for (text, color) in lines {
                    self.push_line(&text, color);
                }
            }
        }
        launched
    }

    fn execute_ping_v6(
        &mut self,
        target: &str,
        ip: crate::drivers::netstack::Ipv6Address,
        count: usize,
    ) {
        let mut banner = TextBuf::new();
        banner.push_str(target);
        banner.push_str(" [");
        let v6_formatted = ip.format();
        banner.push_str(&v6_formatted);
        banner.push_str(crate::gui::ui_text(
            "] adresi 32 bayt veri ile pingleniyor:",
            "] with 32 bytes of data:",
        ));
        self.push_line(banner.as_str(), Color::TEXT_MAIN);

        let mut recvd = 0usize;
        let mut min_rtt = u64::MAX;
        let mut max_rtt = 0u64;
        let mut sum_rtt = 0u64;

        for seq in 1..=(count as u16) {
            crate::drivers::netstack::clear_ping6_reply();
            let t_start = crate::monotonic_ms();
            let sent = crate::drivers::netstack::ping6_ex(ip, seq, 32);
            if !sent {
                self.push_line(
                    crate::gui::ui_text("İletim hatası (TX)", "Transmit failure (TX)"),
                    Color::ACCENT_RED,
                );
            } else {
                let mut reply: Option<crate::drivers::netstack::PingReply6> = None;
                while crate::monotonic_ms().wrapping_sub(t_start) < 1000 {
                    crate::drivers::e1000::poll();
                    crate::drivers::socket::pump_all();
                    if let Some(r) = crate::drivers::netstack::take_ping6_reply() {
                        if r.src == ip || ip.is_loopback() {
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
                    let r_src_str = r.src.format();
                    line.push_str(&r_src_str);
                    line.push_str(": bayt=");
                    line.push_usize(r.len.max(32));
                    line.push_str(crate::gui::ui_text(" süre=", " time="));
                    line.push_usize(rtt as usize);
                    line.push_str("ms HopLimit=");
                    line.push_usize(r.hop_limit as usize);
                    self.push_line(line.as_str(), Color::ACCENT_GREEN);
                } else {
                    self.push_line(
                        crate::gui::ui_text("İstek zaman aşımına uğradı.", "Request timed out."),
                        Color::ACCENT_YELLOW,
                    );
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
        stat_hdr.push_str(crate::gui::ui_text(
            "--- Ping istatistiği: ",
            "--- Ping statistics for ",
        ));
        stat_hdr.push_str(&v6_formatted);
        stat_hdr.push_str(" ---");
        self.push_line(stat_hdr.as_str(), Color::TEXT_MUTED);

        let mut stat_pkt = TextBuf::new();
        stat_pkt.push_str(crate::gui::ui_text(
            "Paket: Gönderilen = ",
            "Packets: Sent = ",
        ));
        stat_pkt.push_usize(count);
        stat_pkt.push_str(crate::gui::ui_text(", Alınan = ", ", Received = "));
        stat_pkt.push_usize(recvd);
        stat_pkt.push_str(crate::gui::ui_text(", Kayıp = ", ", Lost = "));
        stat_pkt.push_usize(lost);
        stat_pkt.push_str(" (%");
        stat_pkt.push_usize(loss_pct);
        stat_pkt.push_str(crate::gui::ui_text(" kayıp)", " loss)"));
        self.push_line(
            stat_pkt.as_str(),
            if lost == 0 {
                Color::ACCENT_GREEN
            } else {
                Color::ACCENT_YELLOW
            },
        );

        if recvd > 0 {
            let avg_rtt = sum_rtt / (recvd as u64);
            let mut stat_time = TextBuf::new();
            stat_time.push_str(crate::gui::ui_text(
                "Gidiş-dönüş süreleri: En az = ",
                "Round trip times: Minimum = ",
            ));
            stat_time.push_usize(min_rtt as usize);
            stat_time.push_str(crate::gui::ui_text("ms, En çok = ", "ms, Maximum = "));
            stat_time.push_usize(max_rtt as usize);
            stat_time.push_str(crate::gui::ui_text("ms, Ortalama = ", "ms, Average = "));
            stat_time.push_usize(avg_rtt as usize);
            stat_time.push_str("ms");
            self.push_line(stat_time.as_str(), Color::TEXT_MAIN);
        }
    }

    /// Tek bir komutu (boru hattı olmadan) yürütür.
    fn execute_single(
        &mut self,
        line: &str,
        vfs: &mut VirtualFileSystem,
        pmm: &mut PhysicalMemoryManager,
        now: &DateTime,
    ) -> Option<usize> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let mut parts = line.splitn(2, ' ');
        let typed = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        let alias = canonical_command(typed);
        let cmd = if alias.is_empty() { typed } else { alias };

        if cmd == "open" || cmd == "run" || cmd == "start" || cmd == "edit" {
            // Dosya yolu verildiyse uygun uygulamayla ac (edit/start).
            if app_index(arg).is_none()
                && !arg.is_empty()
                && !arg.contains(' ')
                && (cmd == "edit" || cmd == "start")
            {
                self.launch_path(cmd, arg);
                return None;
            }
            if let Some(app_idx) = app_index(arg) {
                let mut msg = TextBuf::new();
                msg.push_str(crate::gui::ui_text("Açılıyor: ", "Opening: "));
                msg.push_str(app_name(app_idx));
                self.push_line(msg.as_str(), Color::ACCENT_GREEN);
                return Some(app_idx);
            }
            if cmd == "edit" {
                self.push_line(
                    crate::gui::ui_text("Kullanım: edit <dosya>", "Usage: edit <file>"),
                    Color::ACCENT_YELLOW,
                );
                return None;
            }
            self.push_line(
                crate::gui::ui_text("Kullanım: open <uygulama>", "Usage: open <app>"),
                Color::ACCENT_YELLOW,
            );
            return None;
        }
        if let Some(app_idx) = app_index(cmd) {
            let mut msg = TextBuf::new();
            msg.push_str(crate::gui::ui_text("Açılıyor: ", "Opening: "));
            msg.push_str(app_name(app_idx));
            self.push_line(msg.as_str(), Color::ACCENT_GREEN);
            return Some(app_idx);
        }

        // Boru hatti/yonlendirmede hatalarin hangi komut adiyla olustugunu
        // bilebilmek icin yazilan adi sakla.
        let name: &[u8] = typed.as_bytes();
        self.invoked_len = name.len().min(self.invoked.len());
        self.invoked[..self.invoked_len].copy_from_slice(&name[..self.invoked_len]);

        match cmd {
            "help" => {
                self.push_line("help ls cd pwd mkdir touch rm cat write disks df usb lsusb format echo clear mem ps exec", Color::TEXT_MAIN);
                self.push_line(
                    "uptime fetch neofetch date uname whoami theme ipconfig ping ping6 dns nslookup curl wget cam wifi",
                    Color::ACCENT_GREEN,
                );
                self.push_line(
                    "hexdump <file> | nc <host> <port> [text] | fortune | benchmark",
                    Color::ACCENT_BLUE,
                );
                self.push_line(
                    "minPak list|search|install|remove | exec <file> [args] | open <app>",
                    Color::ACCENT_BLUE,
                );
                self.push_line(
                    "boru: cmd | grep <desen> | sort | wc   yonlendirme: cmd > dosya  cmd >> dosya",
                    Color::ACCENT_YELLOW,
                );
                self.push_line(
                    "filtreler: cat grep wc sort head tail tee",
                    Color::ACCENT_YELLOW,
                );
                self.push_line(
                    "karma (CMD/Unix): dir=ls  cls=clear  type=cat  md=mkdir  rd/del=rm",
                    Color::ACCENT_YELLOW,
                );
                self.push_line(
                    "  copy=cp  move=mv  ren <eski> <yeni>  edit <dosya>  where <komut>  ver  set",
                    Color::ACCENT_YELLOW,
                );
                self.push_line("apps: files settings panel notepad browser image music video taskmgr calc paint cmd", Color::ACCENT_PINK);
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
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(arg);
                        p
                    };
                    if vfs.is_dir(&target)
                        || target == "/media/usb0"
                        || target == "/media/cdrom"
                        || target == "/media/hdd0"
                    {
                        self.set_cwd(&target);
                    } else {
                        self.push_line(
                            crate::gui::ui_text(
                                "Hata: Dizin bulunamadı.",
                                "Error: Directory not found.",
                            ),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "ls" => {
                // `dir /w`, `ls -l` gibi anahtarlar yok sayilir; ilk yol kullanilir.
                let path_arg = if arg.is_empty() {
                    None
                } else {
                    self.first_path_arg(
                        arg,
                        crate::gui::ui_text("Kullanım: ls [klasör]", "Usage: ls [dir]"),
                    )
                };
                let target = match path_arg {
                    Some(path) => self.resolve_path(&path),
                    None => alloc::string::String::from(self.cwd_str()),
                };
                let items = vfs.list_dir(&target);
                if items.is_empty() {
                    self.push_line(
                        crate::gui::ui_text("(boş dizin)", "(empty directory)"),
                        Color::TEXT_MUTED,
                    );
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
                    self.push_line(
                        crate::gui::ui_text("Kullanım: mkdir <klasor>", "Usage: mkdir <dir>"),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(arg);
                        p
                    };
                    if vfs.create_dir(&target) {
                        self.push_line(
                            crate::gui::ui_text("Klasör oluşturuldu.", "Directory created."),
                            Color::ACCENT_GREEN,
                        );
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Oluşturulamadı.", "Failed to create directory."),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "touch" => {
                if arg.is_empty() {
                    self.push_line(
                        crate::gui::ui_text("Kullanım: touch <dosya>", "Usage: touch <file>"),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(arg);
                        p
                    };
                    if vfs.create_file(&target, b"") {
                        self.push_line(
                            crate::gui::ui_text("Dosya oluşturuldu.", "File created."),
                            Color::ACCENT_GREEN,
                        );
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Oluşturulamadı.", "Failed to create file."),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "rm" => {
                if arg.is_empty() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: rm <dosya_veya_klasör>",
                            "Usage: rm <file_or_dir>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(arg);
                        p
                    };
                    if vfs.move_to_trash(&target) {
                        self.push_line(
                            crate::gui::ui_text(
                                "Geri dönüşüm kutusuna taşındı.",
                                "Moved to Recycle Bin.",
                            ),
                            Color::ACCENT_GREEN,
                        );
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Silinemedi.", "Remove failed."),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "write" => {
                let mut sp = arg.splitn(2, ' ');
                let file_arg = sp.next().unwrap_or("");
                let text_arg = sp.next().unwrap_or("");
                if file_arg.is_empty() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: write <dosya> <metin>",
                            "Usage: write <file> <text>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let target = if file_arg.starts_with('/') {
                        alloc::string::String::from(file_arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(file_arg);
                        p
                    };
                    if vfs.write_file(&target, text_arg.as_bytes()) {
                        self.push_line(
                            crate::gui::ui_text("Dosyaya yazıldı.", "Written to file."),
                            Color::ACCENT_GREEN,
                        );
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Yazılamadı.", "Write failed."),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "disks" | "df" => {
                self.push_line(
                    crate::gui::ui_text(
                        "--- BAĞLI DİSKLER VE SÜRÜCÜLER ---",
                        "--- ATTACHED STORAGE DEVICES ---",
                    ),
                    Color::ACCENT_PINK,
                );
                for drive in 0..4u8 {
                    if let Some(dev) = crate::drivers::ata::device(drive) {
                        let mut b = TextBuf::new();
                        b.push_str("IDE ");
                        b.push_usize(drive as usize);
                        b.push_str(": ");
                        match dev.role {
                            crate::drivers::ata::DriveRole::UsbStorage => {
                                b.push_str(crate::gui::ui_text("[USB / FAT32] ", "[USB / FAT32] "))
                            }
                            crate::drivers::ata::DriveRole::HardDisk => {
                                b.push_str(crate::gui::ui_text("[Sabit Disk] ", "[Hard Disk] "))
                            }
                            crate::drivers::ata::DriveRole::CdRom => b.push_str(
                                crate::gui::ui_text("[CD-ROM / ISO] ", "[CD-ROM / ISO] "),
                            ),
                            _ => b.push_str(crate::gui::ui_text("[Bilinmeyen] ", "[Unknown] ")),
                        }
                        b.push_str(dev.model_str());
                        b.push_str(" (");
                        b.push_usize(dev.size_mb() as usize);
                        b.push_str(" MB)");
                        self.push_line(b.as_str(), Color::ACCENT_GREEN);
                    }
                }
                self.push_line(
                    crate::gui::ui_text(
                        "Bağlantı: / -> RamFS | /media/usb0 -> FAT32 | /media/cdrom -> ISO",
                        "Mounts: / -> RamFS | /media/usb0 -> FAT32 | /media/cdrom -> ISO",
                    ),
                    Color::TEXT_MUTED,
                );
            }
            "usb" | "lsusb" => {
                self.push_line(
                    crate::gui::ui_text(
                        "--- USB DENETLEYICILERI VE AYGITLARI ---",
                        "--- USB CONTROLLERS & DEVICES ---",
                    ),
                    Color::ACCENT_PINK,
                );
                if let Some(ctrl) = crate::drivers::usb::controller_info() {
                    let mut b = TextBuf::new();
                    b.push_str("[Host] ");
                    b.push_str(ctrl.controller_type.name());
                    b.push_str(" @ PCI 00:");
                    b.push_usize(ctrl.dev as usize);
                    b.push_str(".");
                    b.push_usize(ctrl.func as usize);
                    self.push_line(b.as_str(), Color::ACCENT_GREEN);
                } else {
                    self.push_line(
                        crate::gui::ui_text(
                            "[Host] Harici USB denetleyici yok (Emulasyon modu)",
                            "[Host] No host controller (Emulation mode)",
                        ),
                        Color::TEXT_MUTED,
                    );
                }

                if crate::drivers::usb_hid::ready() {
                    let mut b = TextBuf::new();
                    b.push_str("[HID] ");
                    b.push_usize(crate::drivers::usb_hid::device_count());
                    b.push_str(crate::gui::ui_text(" aygit (", " dev(s) ("));
                    b.push_usize(crate::drivers::usb_hid::keyboard_count());
                    b.push_str(crate::gui::ui_text(" klavye, ", " kbd, "));
                    b.push_usize(crate::drivers::usb_hid::mouse_count());
                    b.push_str(crate::gui::ui_text(" fare)", " mouse)"));
                    self.push_line(b.as_str(), Color::ACCENT_BLUE);
                } else {
                    self.push_line(
                        crate::gui::ui_text(
                            "[HID] PS/2 ve USB Legacy emulasyonu devrede",
                            "[HID] PS/2 and USB Legacy fallback active",
                        ),
                        Color::TEXT_MUTED,
                    );
                }

                if crate::drivers::usb::usb_available() {
                    self.push_line(
                        crate::gui::ui_text(
                            "[Disk] /media/usb0 (USB Bellek FAT32 bagli)",
                            "[Disk] /media/usb0 (USB Drive FAT32 mounted)",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    self.push_line(
                        crate::gui::ui_text(
                            "[Disk] Bagli USB bellek yok (/media/usb0)",
                            "[Disk] No USB drive attached (/media/usb0)",
                        ),
                        Color::TEXT_MUTED,
                    );
                }
            }
            "cam" | "camera" => {
                self.push_line(
                    crate::gui::ui_text("--- UVC KAMERA SURUCUSU ---", "--- UVC CAMERA DRIVER ---"),
                    Color::ACCENT_PINK,
                );
                if let Some(cam) = crate::drivers::camera::info() {
                    let mut b = TextBuf::new();
                    b.push_str(cam.name_str());
                    self.push_line(b.as_str(), Color::ACCENT_GREEN);

                    let mut b2 = TextBuf::new();
                    b2.push_str("[Cozunurluk] ");
                    b2.push_usize(cam.width as usize);
                    b2.push_str("x");
                    b2.push_usize(cam.height as usize);
                    b2.push_str(" @ ");
                    b2.push_usize(cam.fps as usize);
                    b2.push_str(" FPS");
                    self.push_line(b2.as_str(), Color::ACCENT_BLUE);

                    let mut b3 = TextBuf::new();
                    b3.push_str("[Format] ");
                    b3.push_str(cam.format.name());
                    self.push_line(b3.as_str(), Color::ACCENT_YELLOW);
                } else {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kamera algilanamadi (UVC bekleniyor).",
                            "No camera detected (UVC waiting).",
                        ),
                        Color::TEXT_MUTED,
                    );
                }
            }
            "wifi" | "wlan" => {
                let mut sp = arg.split_whitespace();
                let sub = sp.next().unwrap_or("status");
                match sub {
                    "scan" => {
                        self.push_line(
                            crate::gui::ui_text(
                                "--- KABLOSUZ AGLAR (802.11) ---",
                                "--- WIRELESS NETWORKS (802.11) ---",
                            ),
                            Color::ACCENT_PINK,
                        );
                        let aps = crate::drivers::wifi::scan_networks();
                        for ap_opt in aps {
                            if let Some(ap) = ap_opt {
                                let mut b = TextBuf::new();
                                b.push_str(ap.ssid_str());
                                b.push_str(" (CH ");
                                b.push_usize(ap.channel as usize);
                                b.push_str(", %");
                                b.push_usize(ap.signal_percent() as usize);
                                b.push_str(")");
                                self.push_line(b.as_str(), Color::ACCENT_GREEN);
                            }
                        }
                    }
                    "connect" => {
                        let target_ssid = sp.next().unwrap_or("");
                        let pass = sp.next().unwrap_or("");
                        if target_ssid.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanim: wifi connect <SSID> [sifre]",
                                    "Usage: wifi connect <SSID> [pass]",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            let mut b = TextBuf::new();
                            b.push_str(crate::gui::ui_text("Baglaniliyor: ", "Connecting: "));
                            b.push_str(target_ssid);
                            self.push_line(b.as_str(), Color::ACCENT_YELLOW);
                            if crate::drivers::wifi::connect(target_ssid, pass) {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Baglanti basarili! IP: 192.168.1.105",
                                        "Connected! IP: 192.168.1.105",
                                    ),
                                    Color::ACCENT_GREEN,
                                );
                            } else {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Hata: Ag bulunamadi.",
                                        "Error: Network not found.",
                                    ),
                                    Color::ACCENT_RED,
                                );
                            }
                        }
                    }
                    "disconnect" => {
                        crate::drivers::wifi::disconnect();
                        self.push_line(
                            crate::gui::ui_text("Wi-Fi baglantisi kesildi.", "Wi-Fi disconnected."),
                            Color::ACCENT_YELLOW,
                        );
                    }
                    _ => {
                        self.push_line(
                            crate::gui::ui_text("--- WI-FI DURUMU ---", "--- WI-FI STATUS ---"),
                            Color::ACCENT_PINK,
                        );
                        let st = crate::drivers::wifi::status();
                        match st.state {
                            crate::drivers::wifi::WifiState::Connected => {
                                let mut b = TextBuf::new();
                                b.push_str(crate::gui::ui_text("Bagli: ", "Connected: "));
                                b.push_str(st.ssid_str());
                                self.push_line(b.as_str(), Color::ACCENT_GREEN);

                                let mut b2 = TextBuf::new();
                                b2.push_str("IP: ");
                                b2.push_usize(st.ip_addr[0] as usize);
                                b2.push_str(".");
                                b2.push_usize(st.ip_addr[1] as usize);
                                b2.push_str(".");
                                b2.push_usize(st.ip_addr[2] as usize);
                                b2.push_str(".");
                                b2.push_usize(st.ip_addr[3] as usize);
                                self.push_line(b2.as_str(), Color::ACCENT_BLUE);
                            }
                            _ => {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Durum: Bagli Degil",
                                        "Status: Disconnected",
                                    ),
                                    Color::TEXT_MUTED,
                                );
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Komutlar: wifi scan | wifi connect <SSID>",
                                        "Commands: wifi scan | wifi connect <SSID>",
                                    ),
                                    Color::ACCENT_YELLOW,
                                );
                            }
                        }
                    }
                }
            }
            "format" | "mkfs" => {
                let mut sp = arg.split_whitespace();
                let fs_type = sp.next().unwrap_or("").to_ascii_lowercase();
                let drive_str = sp.next().unwrap_or("");
                let drive_num = if drive_str.starts_with("ide") || drive_str.starts_with("hd") {
                    drive_str
                        .chars()
                        .last()
                        .and_then(|c| c.to_digit(10))
                        .map(|d| d as u8)
                } else {
                    drive_str.parse::<u8>().ok()
                };

                if fs_type.is_empty() || drive_num.is_none() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: format <fat32|ext4|exfat|ext2> <0..3>",
                            "Usage: format <fat32|ext4|exfat|ext2> <0..3>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                    self.push_line(
                        crate::gui::ui_text(
                            "Örnek: format ext4 1  veya  format exfat 0",
                            "Example: format ext4 1  or  format exfat 0",
                        ),
                        Color::TEXT_MUTED,
                    );
                } else {
                    let drive = drive_num.unwrap();
                    if drive >= 4
                        || crate::drivers::ata::device(drive)
                            .map(|d| d.kind != crate::drivers::ata::DeviceKind::Ata)
                            .unwrap_or(true)
                    {
                        self.push_line(
                            crate::gui::ui_text(
                                "Hata: Geçersiz veya bağlı olmayan ATA sürücü!",
                                "Error: Invalid or unattached ATA drive!",
                            ),
                            Color::ACCENT_RED,
                        );
                    } else {
                        let cap = crate::drivers::ata::drive_capacity_sectors(drive);
                        let mut b = TextBuf::new();
                        b.push_str(crate::gui::ui_text(
                            "Biçimlendiriliyor: IDE ",
                            "Formatting: IDE ",
                        ));
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
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Desteklenmeyen dosya sistemi. (fat32, ext4, exfat, ext2)",
                                        "Unsupported filesystem. (fat32, ext4, exfat, ext2)",
                                    ),
                                    Color::ACCENT_RED,
                                );
                                false
                            }
                        };

                        if success {
                            let mut res = TextBuf::new();
                            res.push_str(crate::gui::ui_text(
                                "Başarıyla biçimlendirildi: ",
                                "Successfully formatted: ",
                            ));
                            res.push_str(&fs_type);
                            self.push_line(res.as_str(), Color::ACCENT_GREEN);
                        } else {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Biçimlendirme başarısız oldu!",
                                    "Formatting failed!",
                                ),
                                Color::ACCENT_RED,
                            );
                        }
                    }
                }
            }
            "exec" => {
                if arg.is_empty() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: exec <ikili_dosya_yolu> [argümanlar]",
                            "Usage: exec <binary_path> [arguments]",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let mut exec_parts = arg.splitn(2, ' ');
                    let exec_path = exec_parts.next().unwrap_or("");
                    let exec_args = exec_parts.next().unwrap_or("").trim();
                    let target = if exec_path.starts_with('/') {
                        alloc::string::String::from(exec_path)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
                        p.push_str(exec_path);
                        p
                    };
                    match vfs.read_file(&target) {
                        Some(data) => {
                            let file_name = target.rsplit('/').next().unwrap_or("");
                            let name = file_name
                                .strip_suffix(".pkg")
                                .or_else(|| file_name.strip_suffix(".elf"))
                                .unwrap_or(file_name);
                            let manifest_name = data.split(|b| *b == b'\n').nth(1).unwrap_or(&[]);
                            let builtin_manifest = target.ends_with(".pkg")
                                && data.starts_with(b"MINOS-BUILTIN\n")
                                && manifest_name == name.as_bytes();
                            let legacy_builtin =
                                target.starts_with("/bin/") && target.ends_with(".elf");
                            if target.starts_with("/bin/") && (builtin_manifest || legacy_builtin) {
                                match name {
                                    "tiny-editor" => return Some(2),
                                    "sarman-cli" => {
                                        self.run_sarman_cli(exec_args, pmm);
                                        return None;
                                    }
                                    "netcat-lite" => {
                                        self.run_netcat_lite(exec_args);
                                        return None;
                                    }
                                    "hexdump" => {
                                        self.run_hexdump(exec_args, vfs);
                                        return None;
                                    }
                                    "benchmark" => {
                                        self.run_benchmark(pmm);
                                        return None;
                                    }
                                    "fortune" => {
                                        self.run_fortune();
                                        return None;
                                    }
                                    _ => {}
                                }
                            }
                            let mut aspace = crate::mm::vmm::AddressSpace::current();
                            match crate::mm::loader::AppLoader::load_binary(data, &mut aspace, pmm)
                            {
                                Ok(loaded) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text(
                                        "[EXEC] ELF/İkili dosya yüklendi: RIP=0x",
                                        "[EXEC] ELF/Binary loaded: RIP=0x",
                                    ));
                                    b.push_hex64(loaded.entry_point);
                                    self.push_line(b.as_str(), Color::ACCENT_GREEN);

                                    let ok =
                                        crate::arch::x86_64::ring3::execute_loaded_program(&loaded);
                                    if let Some(out) =
                                        crate::arch::x86_64::ring3::take_last_output()
                                    {
                                        let clean = out.trim_end_matches(['\r', '\n']);
                                        if !clean.is_empty() {
                                            self.push_line(clean, Color::ACCENT_BLUE);
                                        }
                                    }
                                    if ok {
                                        self.push_line(crate::gui::ui_text("[EXEC] Program başarıyla çalıştırıldı ve sonlandı.", "[EXEC] Program executed successfully and terminated."), Color::ACCENT_GREEN);
                                    } else {
                                        self.push_line(crate::gui::ui_text("[EXEC] Program çalışırken hata oluştu (izole edildi).", "[EXEC] Program faulted during execution (isolated)."), Color::ACCENT_RED);
                                    }
                                    loaded.unmap_and_free(&mut aspace, pmm);
                                }
                                Err(err) => {
                                    let mut b = TextBuf::new();
                                    b.push_str(crate::gui::ui_text(
                                        "Yükleme hatası: ",
                                        "Load error: ",
                                    ));
                                    b.push_str(err);
                                    self.push_line(b.as_str(), Color::ACCENT_RED);
                                }
                            }
                        }
                        None => {
                            let mut b = TextBuf::new();
                            b.push_str(crate::gui::ui_text(
                                "Dosya bulunamadı: ",
                                "File not found: ",
                            ));
                            b.push_str(&target);
                            self.push_line(b.as_str(), Color::ACCENT_RED);
                        }
                    }
                }
            }
            "pkg" | "minpkg" | "minPak" | "minpak" => {
                let mut sp = arg.splitn(2, ' ');
                let subcmd = sp.next().unwrap_or("").trim();
                let subarg = sp.next().unwrap_or("").trim();
                match subcmd {
                    "list" => {
                        self.push_line(
                            crate::gui::ui_text(
                                "--- KURULU PAKETLER (/bin) ---",
                                "--- INSTALLED PACKAGES (/bin) ---",
                            ),
                            Color::ACCENT_PINK,
                        );
                        let pkgs = crate::apps::pkgmgr::MinPkg::list_installed(vfs);
                        if pkgs.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kurulu paket bulunamadı.",
                                    "No packages installed.",
                                ),
                                Color::TEXT_MUTED,
                            );
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
                    "search" => {
                        if subarg.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak search <paket_adi>",
                                    "Usage: minPak search <pkg_name>",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            self.push_line(
                                crate::gui::ui_text(
                                    "--- ARAMA SONUÇLARI ---",
                                    "--- SEARCH RESULTS ---",
                                ),
                                Color::ACCENT_PINK,
                            );
                            let needle = subarg.to_lowercase();
                            let mut found = 0;
                            for p in &crate::apps::pkgmgr::MinPkg::list_installed(vfs) {
                                if p.name.to_lowercase().contains(&needle)
                                    || p.description.to_lowercase().contains(&needle)
                                {
                                    let mut b = TextBuf::new();
                                    b.push_str(&p.name);
                                    b.push_str(" (v");
                                    b.push_str(&p.version);
                                    b.push_str(") -> ");
                                    b.push_str(&p.install_path);
                                    self.push_line(b.as_str(), Color::ACCENT_GREEN);
                                    found += 1;
                                }
                            }
                            if found == 0 {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Eşleşen paket bulunamadı.",
                                        "No matching packages found.",
                                    ),
                                    Color::TEXT_MUTED,
                                );
                            }
                        }
                    }
                    "install" => {
                        if subarg.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak install <paket_adi_veya_url> [sha256]",
                                    "Usage: minPak install <pkg_name_or_url> [sha256]",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            let mut parts = subarg.split_whitespace();
                            let target_pkg = parts.next().unwrap_or("");
                            let expected_sha256 = parts.next();

                            self.push_line(
                                crate::gui::ui_text("Paket kuruluyor...", "Installing package..."),
                                Color::TEXT_MUTED,
                            );
                            // Yerel katalog paketi ise bağımlılıkları önce çözüp kur; ağdan
                            // indirilecek paketlerde bağımlılık bilgisi bulunmadığı için tek
                            // olarak kurulur (checksum yolu korunur).
                            if !target_pkg.starts_with("http://") {
                                match crate::apps::pkgmgr::MinPkg::install_with_dependencies(
                                    target_pkg, vfs,
                                ) {
                                    Ok((paths, skipped)) => {
                                        for p in &paths {
                                            let mut b = TextBuf::new();
                                            b.push_str(crate::gui::ui_text(
                                                "Başarıyla kuruldu: ",
                                                "Successfully installed: ",
                                            ));
                                            b.push_str(p);
                                            self.push_line(b.as_str(), Color::ACCENT_GREEN);
                                        }
                                        for s in &skipped {
                                            let mut b = TextBuf::new();
                                            b.push_str(crate::gui::ui_text(
                                                "Zaten kurulu, atlandı: ",
                                                "Already installed, skipped: ",
                                            ));
                                            b.push_str(s);
                                            self.push_line(b.as_str(), Color::ACCENT_BLUE);
                                        }
                                    }
                                    Err(err) => {
                                        let mut b = TextBuf::new();
                                        b.push_str(crate::gui::ui_text(
                                            "Kurulum başarısız: ",
                                            "Installation failed: ",
                                        ));
                                        b.push_str(&err);
                                        self.push_line(b.as_str(), Color::ACCENT_RED);
                                    }
                                }
                            } else {
                                // Ağdan kurulum: SHA-256 doğrulamalı URL yolu.
                                let res =
                                    crate::apps::pkgmgr::MinPkg::install_from_url_with_checksum(
                                        target_pkg,
                                        expected_sha256,
                                        vfs,
                                    );
                                match res {
                                    Ok(path) => {
                                        let mut b = TextBuf::new();
                                        b.push_str(crate::gui::ui_text(
                                            "Başarıyla kuruldu: ",
                                            "Successfully installed: ",
                                        ));
                                        b.push_str(&path);
                                        self.push_line(b.as_str(), Color::ACCENT_GREEN);
                                    }
                                    Err(err) => {
                                        let mut b = TextBuf::new();
                                        b.push_str(crate::gui::ui_text(
                                            "Kurulum başarısız: ",
                                            "Installation failed: ",
                                        ));
                                        b.push_str(err);
                                        self.push_line(b.as_str(), Color::ACCENT_RED);
                                    }
                                }
                            }
                        }
                    }
                    "sha256" | "checksum" | "hash" => {
                        if subarg.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak sha256 <dosya_veya_paket>",
                                    "Usage: minPak sha256 <file_or_pkg>",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            let path = if subarg.starts_with('/') {
                                alloc::string::String::from(subarg)
                            } else {
                                alloc::format!("/bin/{}", subarg)
                            };
                            if let Some(data) = vfs.read_file(&path) {
                                let csum = crate::apps::pkgmgr::MinPkg::compute_sha256(data);
                                let mut b = TextBuf::new();
                                b.push_str("SHA-256: ");
                                b.push_str(&csum);
                                self.push_line(b.as_str(), Color::ACCENT_GREEN);
                            } else {
                                self.push_line(
                                    crate::gui::ui_text("Dosya bulunamadı.", "File not found."),
                                    Color::ACCENT_RED,
                                );
                            }
                        }
                    }
                    "verify" => {
                        let mut parts = subarg.split_whitespace();
                        let target_file = parts.next().unwrap_or("");
                        let expected = parts.next().unwrap_or("");
                        if target_file.is_empty() || expected.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak verify <dosya_veya_paket> <beklenen_sha256>",
                                    "Usage: minPak verify <file_or_pkg> <expected_sha256>",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            let path = if target_file.starts_with('/') {
                                alloc::string::String::from(target_file)
                            } else {
                                alloc::format!("/bin/{}", target_file)
                            };
                            match crate::apps::pkgmgr::MinPkg::verify_file_checksum(
                                &path, expected, vfs,
                            ) {
                                Ok(true) => {
                                    self.push_line(
                                        crate::gui::ui_text(
                                            "Doğrulama başarılı: SHA-256 checksum eşleşti.",
                                            "Verification successful: SHA-256 matches.",
                                        ),
                                        Color::ACCENT_GREEN,
                                    );
                                }
                                Ok(false) => {
                                    self.push_line(
                                        crate::gui::ui_text(
                                            "Doğrulama BAŞARISIZ: SHA-256 checksum uyuşmuyor!",
                                            "Verification FAILED: SHA-256 checksum mismatch!",
                                        ),
                                        Color::ACCENT_RED,
                                    );
                                }
                                Err(e) => {
                                    self.push_line(e, Color::ACCENT_RED);
                                }
                            }
                        }
                    }
                    "depends" | "deps" | "resolve" => {
                        if subarg.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak depends <paket_adi>",
                                    "Usage: minPak depends <pkg_name>",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            match crate::apps::pkgmgr::MinPkg::resolve_plan(subarg, vfs) {
                                Ok(plan) => {
                                    self.push_line(
                                        crate::gui::ui_text(
                                            "--- BAĞIMLILIK PLANI ---",
                                            "--- DEPENDENCY PLAN ---",
                                        ),
                                        Color::ACCENT_PINK,
                                    );
                                    for (i, line) in plan.to_install.iter().enumerate() {
                                        let mut b = TextBuf::new();
                                        b.push_str("  ");
                                        if i + 1 == plan.to_install.len() {
                                            b.push_str(crate::gui::ui_text("hedef: ", "target: "));
                                        }
                                        b.push_str(line);
                                        self.push_line(b.as_str(), Color::ACCENT_GREEN);
                                    }
                                    for s in &plan.already_installed {
                                        let mut b = TextBuf::new();
                                        b.push_str(crate::gui::ui_text(
                                            "  zaten kurulu: ",
                                            "  already installed: ",
                                        ));
                                        b.push_str(s);
                                        self.push_line(b.as_str(), Color::ACCENT_BLUE);
                                    }
                                }
                                Err(e) => {
                                    let msg = e.message();
                                    self.push_line(&msg, Color::ACCENT_RED);
                                }
                            }
                        }
                    }
                    "remove" | "uninstall" => {
                        if subarg.is_empty() {
                            self.push_line(
                                crate::gui::ui_text(
                                    "Kullanım: minPak remove <paket_adi>",
                                    "Usage: minPak remove <pkg_name>",
                                ),
                                Color::ACCENT_YELLOW,
                            );
                        } else {
                            if crate::apps::pkgmgr::MinPkg::remove(subarg, vfs) {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Paket başarıyla kaldırıldı.",
                                        "Package removed successfully.",
                                    ),
                                    Color::ACCENT_GREEN,
                                );
                            } else {
                                self.push_line(
                                    crate::gui::ui_text(
                                        "Paket bulunamadı veya silinemedi.",
                                        "Package not found or could not be removed.",
                                    ),
                                    Color::ACCENT_RED,
                                );
                            }
                        }
                    }
                    _ => {
                        self.push_line(
                            crate::gui::ui_text(
                                "minOs Paket Yöneticisi v1.1 (minPak)",
                                "minOs Package Manager v1.1 (minPak)",
                            ),
                            Color::ACCENT_PINK,
                        );
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak depends <paket>        - Bağımlılık çözüm planını gösterir",
                                "minPak depends <package>      - Show dependency resolution plan",
                            ),
                            Color::TEXT_MAIN,
                        );
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak list                   - Kurulu paketleri listeler",
                                "minPak list                   - List installed packages",
                            ),
                            Color::TEXT_MAIN,
                        );
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak search <paket>         - Kurulu paketlerde arama yapar",
                                "minPak search <package>       - Search installed packages",
                            ),
                            Color::TEXT_MAIN,
                        );
                        self.push_line(crate::gui::ui_text("minPak install <paket> [hash] - Paketi indirir, checksum'ı doğrular ve kurar", "minPak install <pkg> [hash]  - Download, verify checksum and install"), Color::TEXT_MAIN);
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak sha256 <dosya>         - Dosyanın SHA-256 özetini hesaplar",
                                "minPak sha256 <file>          - Calculate file SHA-256 hash",
                            ),
                            Color::TEXT_MAIN,
                        );
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak verify <dosya> <hash>  - Dosyanın SHA-256 özetini doğrular",
                                "minPak verify <file> <hash>   - Verify file SHA-256 checksum",
                            ),
                            Color::TEXT_MAIN,
                        );
                        self.push_line(
                            crate::gui::ui_text(
                                "minPak remove <paket>         - Kurulu paketi siler",
                                "minPak remove <package>       - Remove installed package",
                            ),
                            Color::TEXT_MAIN,
                        );
                    }
                }
            }
            "cat" => {
                // CMD `type /a dosya` ve `cat -n dosya` gibi anahtarlari yok say.
                let arg = {
                    let mut a = arg;
                    while (a.starts_with('-') || a.starts_with('/'))
                        && a.len() > 1
                        && !a[1..].contains(' ')
                    {
                        let (flag, rest) = a.split_once(' ').unwrap_or((a, ""));
                        if flag.len() > 3 {
                            break;
                        }
                        a = rest.trim();
                        if a.is_empty() {
                            break;
                        }
                    }
                    a
                };
                if arg.is_empty() {
                    if let Some(text) = self.stdin_text() {
                        for l in text.split('\n') {
                            if !l.is_empty() {
                                self.push_line(l, Color::TEXT_MAIN);
                            }
                        }
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Kullanım: cat <dosya>", "Usage: cat <file>"),
                            Color::ACCENT_YELLOW,
                        );
                    }
                } else {
                    let target = if arg.starts_with('/') {
                        alloc::string::String::from(arg)
                    } else {
                        let mut p = alloc::string::String::from(self.cwd_str());
                        if !p.ends_with('/') {
                            p.push('/');
                        }
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
            "hexdump" => self.run_hexdump(arg, vfs),
            "nc" => self.run_netcat_lite(arg),
            "fortune" => self.run_fortune(),
            "benchmark" => self.run_benchmark(pmm),
            "sarman-cli" => self.run_sarman_cli(arg, pmm),
            "echo" => {
                if arg.is_empty() {
                    if let Some(text) = self.stdin_text() {
                        for l in text.split('\n') {
                            if !l.is_empty() {
                                self.push_line(l, Color::TEXT_MAIN);
                            }
                        }
                        return None;
                    }
                }
                let text = unquote_shell(arg);
                self.push_line(&text, Color::TEXT_MAIN);
            }
            "grep" => {
                let mut sp = arg.splitn(2, ' ');
                let pattern = unquote_shell(sp.next().unwrap_or(""));
                let file = sp.next().unwrap_or("").trim();
                if pattern.is_empty() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: grep <desen> [dosya]",
                            "Usage: grep <pattern> [file]",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                    return None;
                }
                let source = if file.is_empty() {
                    self.stdin_text()
                } else {
                    let path = self.resolve_path(file);
                    vfs.read_file(&path).map(|data| {
                        alloc::string::String::from(core::str::from_utf8(data).unwrap_or(""))
                    })
                };
                if let Some(text) = source {
                    let mut matches = 0usize;
                    for l in text.split('\n') {
                        if l.contains(pattern.as_str()) {
                            self.push_line(l, Color::TEXT_MAIN);
                            matches += 1;
                        }
                    }
                    if matches == 0 {
                        self.push_line(
                            crate::gui::ui_text("(eşleşme yok)", "(no matches)"),
                            Color::TEXT_MUTED,
                        );
                    }
                } else {
                    self.push_line(
                        crate::gui::ui_text("grep: girdi yok", "grep: no input"),
                        Color::ACCENT_RED,
                    );
                }
            }
            "wc" => {
                let file = arg.trim();
                let source = if file.is_empty() {
                    self.stdin_text()
                } else {
                    let path = self.resolve_path(file);
                    vfs.read_file(&path).map(|data| {
                        alloc::string::String::from(core::str::from_utf8(data).unwrap_or(""))
                    })
                };
                let Some(text) = source else {
                    self.push_line(
                        crate::gui::ui_text("Kullanım: wc [dosya]", "Usage: wc [file]"),
                        Color::ACCENT_YELLOW,
                    );
                    return None;
                };
                let lines = text.matches('\n').count()
                    + usize::from(!text.is_empty() && !text.ends_with('\n'));
                let words = text.split_whitespace().count();
                let mut b = TextBuf::new();
                b.push_usize(lines);
                b.push_str("  ");
                b.push_usize(words);
                b.push_str("  ");
                b.push_usize(text.len());
                self.push_line(b.as_str(), Color::TEXT_MAIN);
            }
            "sort" => {
                let file = arg.trim();
                let source = if file.is_empty() {
                    self.stdin_text()
                } else {
                    let path = self.resolve_path(file);
                    vfs.read_file(&path).map(|data| {
                        alloc::string::String::from(core::str::from_utf8(data).unwrap_or(""))
                    })
                };
                if let Some(text) = source {
                    let mut lines: alloc::vec::Vec<&str> =
                        text.lines().filter(|l| !l.is_empty()).collect();
                    lines.sort_unstable();
                    for l in lines {
                        self.push_line(l, Color::TEXT_MAIN);
                    }
                } else {
                    self.push_line(
                        crate::gui::ui_text("sort: girdi yok", "sort: no input"),
                        Color::ACCENT_RED,
                    );
                }
            }
            "head" | "tail" => {
                let mut sp = arg.split_whitespace();
                let mut count = 10usize;
                let mut file = "";
                for token in sp.by_ref() {
                    if let Some(rest) = token.strip_prefix('-') {
                        count = rest.parse::<usize>().unwrap_or(10).max(1);
                    } else {
                        file = token;
                    }
                }
                let source = if file.is_empty() {
                    self.stdin_text()
                } else {
                    let path = self.resolve_path(file);
                    vfs.read_file(&path).map(|data| {
                        alloc::string::String::from(core::str::from_utf8(data).unwrap_or(""))
                    })
                };
                if let Some(text) = source {
                    let lines: alloc::vec::Vec<&str> = text.lines().collect();
                    let slice: &[&str] = if cmd == "head" {
                        &lines[..lines.len().min(count)]
                    } else {
                        &lines[lines.len().saturating_sub(count)..]
                    };
                    for l in slice {
                        if !l.is_empty() {
                            self.push_line(l, Color::TEXT_MAIN);
                        }
                    }
                } else {
                    self.push_line(
                        crate::gui::ui_text("head/tail: girdi yok", "head/tail: no input"),
                        Color::ACCENT_RED,
                    );
                }
            }
            "tee" => {
                let file = unquote_shell(arg.trim());
                if file.is_empty() {
                    self.push_line(
                        crate::gui::ui_text("Kullanım: tee <dosya>", "Usage: tee <file>"),
                        Color::ACCENT_YELLOW,
                    );
                    return None;
                }
                let text = self.stdin_text().unwrap_or_default();
                for l in text.split('\n') {
                    if !l.is_empty() {
                        self.push_line(l, Color::TEXT_MAIN);
                    }
                }
                let path = self.resolve_path(&file);
                vfs.write_file(&path, text.as_bytes());
            }
            "cp" => self.run_copy_or_move(arg, vfs, false),
            "mv" => self.run_copy_or_move(arg, vfs, true),
            "rename" => {
                let args = split_args(arg);
                if args.len() != 2 {
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: ren <eski> <yeni>",
                            "Usage: ren <old> <new>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    let from = self.resolve_path(&args[0]);
                    let mut to = self.resolve_path(&args[1]);
                    // `ren dosya yeni_ad` biciminde hedef ayni dizinde kalir.
                    if !args[1].contains('/') && !args[1].contains('\\') {
                        let parent = Self::parent_of(&from);
                        to = alloc::format!("{}/{}", parent.trim_end_matches('/'), args[1]);
                    }
                    if vfs.rename(&from, &to) {
                        let mut b = TextBuf::new();
                        b.push_str(crate::gui::ui_text("Yeniden adlandırıldı: ", "Renamed: "));
                        b.push_str(&args[1]);
                        self.push_line(b.as_str(), Color::ACCENT_GREEN);
                    } else {
                        self.push_line(
                            crate::gui::ui_text("Yeniden adlandırılamadı.", "Rename failed."),
                            Color::ACCENT_RED,
                        );
                    }
                }
            }
            "where" => {
                let name = unquote_shell(arg);
                if name.is_empty() {
                    self.push_line(
                        crate::gui::ui_text("Kullanım: where <komut>", "Usage: where <command>"),
                        Color::ACCENT_YELLOW,
                    );
                } else {
                    self.run_where(&name, vfs);
                }
            }
            "ver" => {
                self.push_line(
                    crate::gui::ui_text(
                        "minOs 1.0 (MinShell 2.0 · karma kabuk)",
                        "minOs 1.0 (MinShell 2.0 · hybrid shell)",
                    ),
                    Color::ACCENT_GREEN,
                );
            }
            "set" => self.run_set(arg),
            "exit" => {
                self.push_line(
                    crate::gui::ui_text(
                        "Bu oturum kalıcıdır; pencereyi kapatmak için pencere düğmesini kullanın.",
                        "This session is persistent; use the window button to close it.",
                    ),
                    Color::TEXT_MUTED,
                );
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
                self.push_line(
                    crate::gui::ui_text("PID  İSİM", "PID  NAME"),
                    Color::TEXT_MUTED,
                );
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
                self.push_line(
                    crate::gui::ui_text("Ethernet bağdaştırıcısı eth0:", "Ethernet adapter eth0:"),
                    Color::ACCENT_GREEN,
                );
                push_ipv4_line(self, "IPv4: ", local_ip().0);
                push_ipv4_line(
                    self,
                    crate::gui::ui_text("Maske: ", "Mask: "),
                    subnet_mask().0,
                );
                push_ipv4_line(
                    self,
                    crate::gui::ui_text("Ağ geçidi: ", "Gateway: "),
                    gateway_ip().0,
                );
                push_ipv4_line(self, "DNS: ", dns_server().0);
                push_ipv6_line(
                    self,
                    crate::gui::ui_text("IPv6 (Bağlantı-Yerel): ", "IPv6 (Link-Local): "),
                    crate::drivers::netstack::local_ipv6(),
                );
                self.push_line(
                    if crate::drivers::netstack::dhcp_ready() {
                        crate::gui::ui_text("DHCP: adres alındı", "DHCP: lease active")
                    } else {
                        crate::gui::ui_text(
                            "DHCP: bekleniyor (yedek adres etkin)",
                            "DHCP: waiting (fallback address active)",
                        )
                    },
                    Color::ACCENT_YELLOW,
                );
                let mac = unsafe { crate::drivers::e1000::MAC_ADDR };
                let mut b = TextBuf::new();
                b.push_str("MAC: ");
                for (i, octet) in mac.iter().enumerate() {
                    if i > 0 {
                        b.push_byte(b':');
                    }
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    b.push_byte(HEX[(octet >> 4) as usize]);
                    b.push_byte(HEX[(octet & 0x0F) as usize]);
                }
                self.push_line(b.as_str(), Color::TEXT_MAIN);
                self.push_line(
                    if crate::drivers::e1000::is_ready() {
                        crate::gui::ui_text("Bağlantı: hazır", "Link: ready")
                    } else {
                        crate::gui::ui_text("Bağlantı: kullanılamıyor", "Link: unavailable")
                    },
                    Color::ACCENT_YELLOW,
                );
            }
            "ping" | "ping6" => {
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
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: ping [-c adet] <hedef_veya_ip>",
                            "Usage: ping [-c count] <target_or_ip>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Ağ bağdaştırıcısı bulunamadı.",
                            "Network adapter is unavailable.",
                        ),
                        Color::ACCENT_RED,
                    );
                } else if let Some(v6_ip) = crate::drivers::netstack::parse_ipv6(target) {
                    self.execute_ping_v6(target, v6_ip, count);
                } else {
                    let target_ip = if let Some(ip) = parse_ipv4(target) {
                        Some(crate::drivers::netstack::Ipv4Address(ip))
                    } else {
                        let mut msg = TextBuf::new();
                        msg.push_str(crate::gui::ui_text(
                            "Alan adı çözümleniyor: ",
                            "Resolving host: ",
                        ));
                        msg.push_str(target);
                        msg.push_str("...");
                        self.push_line(msg.as_str(), Color::TEXT_MUTED);
                        match crate::drivers::socket::resolve_host(target) {
                            Ok(ip) => Some(ip),
                            Err(_) => {
                                let mut err = TextBuf::new();
                                err.push_str(crate::gui::ui_text(
                                    "Ping isteği hedef ana bilgisayarı bulamadı: ",
                                    "Ping request could not find host: ",
                                ));
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
                        banner.push_str(crate::gui::ui_text(
                            "] adresi 32 bayt veri ile pingleniyor:",
                            "] with 32 bytes of data:",
                        ));
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
                                self.push_line(
                                    crate::gui::ui_text(
                                        "İletim hatası (TX)",
                                        "Transmit failure (TX)",
                                    ),
                                    Color::ACCENT_RED,
                                );
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
                                    line.push_str(crate::gui::ui_text(
                                        "Yanıt geldi: ",
                                        "Reply from ",
                                    ));
                                    push_ipv4(&mut line, r.src.0);
                                    line.push_str(": bayt=");
                                    line.push_usize(r.len.max(32));
                                    line.push_str(crate::gui::ui_text(" süre=", " time="));
                                    line.push_usize(rtt as usize);
                                    line.push_str("ms TTL=");
                                    line.push_usize(r.ttl as usize);
                                    self.push_line(line.as_str(), Color::ACCENT_GREEN);
                                } else {
                                    self.push_line(
                                        crate::gui::ui_text(
                                            "İstek zaman aşımına uğradı.",
                                            "Request timed out.",
                                        ),
                                        Color::ACCENT_YELLOW,
                                    );
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
                        stat_hdr.push_str(crate::gui::ui_text(
                            "--- Ping istatistiği: ",
                            "--- Ping statistics for ",
                        ));
                        push_ipv4(&mut stat_hdr, ip.0);
                        stat_hdr.push_str(" ---");
                        self.push_line(stat_hdr.as_str(), Color::TEXT_MUTED);

                        let mut stat_pkt = TextBuf::new();
                        stat_pkt.push_str(crate::gui::ui_text(
                            "Paket: Gönderilen = ",
                            "Packets: Sent = ",
                        ));
                        stat_pkt.push_usize(count);
                        stat_pkt.push_str(crate::gui::ui_text(", Alınan = ", ", Received = "));
                        stat_pkt.push_usize(recvd);
                        stat_pkt.push_str(crate::gui::ui_text(", Kayıp = ", ", Lost = "));
                        stat_pkt.push_usize(lost);
                        stat_pkt.push_str(" (%");
                        stat_pkt.push_usize(loss_pct);
                        stat_pkt.push_str(crate::gui::ui_text(" kayıp)", " loss)"));
                        self.push_line(
                            stat_pkt.as_str(),
                            if lost == 0 {
                                Color::ACCENT_GREEN
                            } else {
                                Color::ACCENT_YELLOW
                            },
                        );

                        if recvd > 0 {
                            let avg_rtt = sum_rtt / (recvd as u64);
                            let mut stat_time = TextBuf::new();
                            stat_time.push_str(crate::gui::ui_text(
                                "Gidiş-dönüş süreleri: En az = ",
                                "Round trip times: Minimum = ",
                            ));
                            stat_time.push_usize(min_rtt as usize);
                            stat_time
                                .push_str(crate::gui::ui_text("ms, En çok = ", "ms, Maximum = "));
                            stat_time.push_usize(max_rtt as usize);
                            stat_time
                                .push_str(crate::gui::ui_text("ms, Ortalama = ", "ms, Average = "));
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
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: curl [-i|-I|-s] [-o dosya] <url>",
                            "Usage: curl [-i|-I|-s] [-o file] <url>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                    self.push_line(
                        crate::gui::ui_text(
                            "Örnek: curl 10.0.2.2  veya  curl -o sayfa.html http://10.0.2.2:8080/",
                            "Example: curl 10.0.2.2  or  curl -o page.html http://10.0.2.2:8080/",
                        ),
                        Color::TEXT_MUTED,
                    );
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Ağ bağdaştırıcısı bulunamadı.",
                            "Network adapter is unavailable.",
                        ),
                        Color::ACCENT_RED,
                    );
                } else {
                    let target_url = raw_url.unwrap();
                    let normalized_url = if target_url.starts_with("http://")
                        || target_url.starts_with("https://")
                        || target_url.contains("://")
                    {
                        alloc::string::String::from(target_url)
                    } else {
                        let mut s = alloc::string::String::from("http://");
                        s.push_str(target_url);
                        s
                    };

                    if is_wget && out_file.is_none() {
                        let last_seg = normalized_url.rsplit('/').next().unwrap_or("index.html");
                        let fname = if last_seg.is_empty()
                            || last_seg.contains('?')
                            || last_seg.contains(':')
                        {
                            "index.html"
                        } else {
                            last_seg
                        };
                        out_file = Some(alloc::string::String::from(fname));
                    }

                    if !silent {
                        let mut msg = TextBuf::new();
                        msg.push_str(crate::gui::ui_text(
                            "Ağdan veri çekiliyor: ",
                            "Fetching from network: ",
                        ));
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
                                self.push_line(
                                    b.as_str(),
                                    if status < 400 {
                                        Color::ACCENT_GREEN
                                    } else {
                                        Color::ACCENT_RED
                                    },
                                );
                            }

                            let body = &buf[hlen.min(total)..total];

                            if let Some(dest) = out_file {
                                let target_path = if dest.starts_with('/') {
                                    dest
                                } else {
                                    let mut p = alloc::string::String::from(self.cwd_str());
                                    if !p.ends_with('/') {
                                        p.push('/');
                                    }
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
                                    ok_b.push_str(crate::gui::ui_text(
                                        "Dosyaya kaydedildi: ",
                                        "Saved to file: ",
                                    ));
                                    ok_b.push_str(&target_path);
                                    ok_b.push_str(" (");
                                    ok_b.push_usize(body.len());
                                    ok_b.push_str(crate::gui::ui_text(" bayt)", " bytes)"));
                                    self.push_line(ok_b.as_str(), Color::ACCENT_GREEN);
                                } else {
                                    let mut err_b = TextBuf::new();
                                    err_b.push_str(crate::gui::ui_text(
                                        "Dosyaya yazılamadı: ",
                                        "Failed to write file: ",
                                    ));
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
                                        self.push_line(
                                            crate::gui::ui_text("(boş gövde)", "(empty body)"),
                                            Color::TEXT_MUTED,
                                        );
                                    } else {
                                        let mut line = TextBuf::new();
                                        let mut lines = 0usize;
                                        for &c in body {
                                            if c == b'\n' || line.len() >= 62 {
                                                self.push_line(line.as_str(), Color::TEXT_MAIN);
                                                line.clear();
                                                lines += 1;
                                                if lines >= 60 {
                                                    break;
                                                }
                                                if c == b'\n' {
                                                    continue;
                                                }
                                            }
                                            if c == b'\r' {
                                                continue;
                                            }
                                            line.push_byte(
                                                if c.is_ascii_graphic() || c == b' ' || c == b'\t' {
                                                    c
                                                } else {
                                                    b'?'
                                                },
                                            );
                                        }
                                        if lines < 60 && !line.is_empty() {
                                            self.push_line(line.as_str(), Color::TEXT_MAIN);
                                        } else if lines >= 60 {
                                            self.push_line(
                                                crate::gui::ui_text(
                                                    "... (kısaltıldı)",
                                                    "... (truncated)",
                                                ),
                                                Color::TEXT_MUTED,
                                            );
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
                                    err_b.push_str(crate::gui::ui_text(
                                        "(28) Bağlantı zaman aşımı: sunucudan yanıt alınamadı.",
                                        "(28) Connection timed out: no response from server.",
                                    ));
                                }
                                crate::drivers::socket::E_CONNREFUSED => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "(7) Bağlantı reddedildi: hedef port kapalı.",
                                        "(7) Failed to connect: connection refused.",
                                    ));
                                }
                                crate::drivers::socket::E_NOTCONN => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "(6) Ağ bağlantısı yok veya ana bilgisayar çözümlenemedi.",
                                        "(6) Could not resolve host or network unavailable.",
                                    ));
                                }
                                crate::drivers::http::E_URL => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "(3) Geçersiz URL sözdizimi.",
                                        "(3) Invalid URL format.",
                                    ));
                                }
                                crate::drivers::http::E_REDIRECT => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "(47) Çok fazla HTTP yönlendirmesi (maksimum 3).",
                                        "(47) Maximum redirection limit exceeded.",
                                    ));
                                }
                                crate::drivers::http::E_HTTP => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "(52) Sunucu geçersiz/boş HTTP yanıtı döndürdü.",
                                        "(52) Empty or invalid HTTP response from server.",
                                    ));
                                }
                                _ => {
                                    err_b.push_str(crate::gui::ui_text(
                                        "İndirme başarısız (hata kodu: ",
                                        "Download failed (error code: ",
                                    ));
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
                    self.push_line(
                        crate::gui::ui_text(
                            "Kullanım: dns <alan_adı>  veya  nslookup <alan_adı>",
                            "Usage: dns <hostname>  or  nslookup <hostname>",
                        ),
                        Color::ACCENT_YELLOW,
                    );
                } else if !crate::drivers::e1000::is_ready() {
                    self.push_line(
                        crate::gui::ui_text(
                            "Ağ bağdaştırıcısı bulunamadı.",
                            "Network adapter is unavailable.",
                        ),
                        Color::ACCENT_RED,
                    );
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
                            err_b.push_str(crate::gui::ui_text(
                                "DNS çözümlenemedi (hata kodu: ",
                                "DNS resolution failed (error code: ",
                            ));
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
                b.push_str(if crate::gui::ui_english() {
                    DateTime::month_name_en(now.month)
                } else {
                    DateTime::month_name(now.month)
                });
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
                let mut text = [0u8; crate::gui::timefmt::MAX_TEXT];
                let text_len =
                    crate::gui::timefmt::format_duration(secs, crate::gui::ui_english(), &mut text);
                let mut b = TextBuf::new();
                b.push_str(crate::gui::ui_text("Çalışma Süresi: ", "Uptime: "));
                for &byte in &text[..text_len] {
                    b.push_byte(byte);
                }
                b.push_str(" (");
                b.push_usize(ms as usize);
                b.push_str(" ms)");
                self.push_line(b.as_str(), Color::ACCENT_GREEN);
            }
            "fetch" | "neofetch" => {
                self.push_line(
                    "   /\\_/\\     minOs Sarman Edition v2.0",
                    Color::ACCENT_PINK,
                );
                self.push_line("  ( o.o )    -------------------------", Color::ACCENT_PINK);
                self.push_line(
                    "   > ^ <     OS: minOs x86_64 [Rust no_std]",
                    Color::ACCENT_BLUE,
                );
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
                self.push_line(
                    "             GUI: MinGuiCompositor 60FPS",
                    Color::TEXT_MUTED,
                );
            }
            _ => {
                let mut b = TextBuf::new();
                b.push_str(crate::gui::ui_text(
                    "Bilinmeyen komut: ",
                    "Unknown command: ",
                ));
                b.push_str(cmd);
                self.push_line(b.as_str(), Color::ACCENT_RED);
                self.push_line(
                    crate::gui::ui_text("'help' yazın.", "Type 'help'."),
                    Color::TEXT_MUTED,
                );
            }
        }
        None
    }

    /// `cp`/`copy` ve `mv`/`move` komutlarini yurutur (kaynak... hedef).
    fn run_copy_or_move(&mut self, arg: &str, vfs: &mut VirtualFileSystem, is_move: bool) {
        const SWITCHES: &[&str] = &["/y", "/-y", "/s", "/q", "/f", "-r", "-f", "-v"];
        let args: alloc::vec::Vec<alloc::string::String> = split_args(arg)
            .into_iter()
            .filter(|a| !SWITCHES.iter().any(|s| s.eq_ignore_ascii_case(a)))
            .collect();
        let name = if is_move { "mv" } else { "cp" };
        if args.len() < 2 {
            let mut usage = TextBuf::new();
            usage.push_str("Kullanım: ");
            usage.push_str(crate::gui::ui_text(
                "cp <kaynak...> <hedef> | ",
                "cp <source...> <dest> | ",
            ));
            usage.push_str(if is_move {
                "mv <kaynak> <hedef>"
            } else {
                "copy <kaynak> <hedef>"
            });
            self.push_line(usage.as_str(), Color::ACCENT_YELLOW);
            return;
        }
        let (last, sources) = args.split_last().expect("len >= 2");
        let mut dest = self.resolve_path(last);
        let dest_is_dir = vfs.is_dir(&dest) || sources.len() > 1;
        if dest_is_dir && !dest.starts_with('/') {
            dest = alloc::format!("/{}", dest);
        }

        for src in sources {
            let from = self.resolve_path(src);
            let mut target_dir = dest.clone();
            let mut final_path: Option<alloc::string::String> = None;
            if !vfs.is_dir(&target_dir) {
                if sources.len() > 1 {
                    self.push_line(
                        crate::gui::ui_text(
                            "Hedef klasor bulunamadi.",
                            "Destination folder not found.",
                        ),
                        Color::ACCENT_RED,
                    );
                    continue;
                }
                // Tek kaynak + dosya hedefi: yeni ad olarak kullan.
                if let Some(pos) = target_dir.rfind('/') {
                    final_path = Some(target_dir.clone());
                    target_dir = if pos == 0 {
                        alloc::string::String::from("/")
                    } else {
                        alloc::string::String::from(&target_dir[..pos])
                    };
                } else {
                    target_dir = alloc::string::String::from("/");
                }
            }
            if !vfs.is_dir(&target_dir) {
                self.push_line(
                    crate::gui::ui_text(
                        "Hedef klasor bulunamadi.",
                        "Destination folder not found.",
                    ),
                    Color::ACCENT_RED,
                );
                continue;
            }

            let result = match vfs.copy_item(&from, &target_dir) {
                Ok(path) => path,
                Err(err) => {
                    let mut b = TextBuf::new();
                    b.push_str(name);
                    b.push_str(": ");
                    b.push_str(err);
                    self.push_line(b.as_str(), Color::ACCENT_RED);
                    continue;
                }
            };
            let mut created = result.clone();
            if let Some(want) = final_path.as_deref() {
                if vfs.rename(&result, want) {
                    created = alloc::string::String::from(want);
                }
            }
            if is_move && !vfs.move_to_trash(&from) {
                self.push_line(
                    crate::gui::ui_text(
                        "Kaynak silinemedi (kopya olustu).",
                        "Could not remove source (copy kept).",
                    ),
                    Color::ACCENT_YELLOW,
                );
            }
            let mut b = TextBuf::new();
            b.push_str(if is_move {
                crate::gui::ui_text("Taşındı: ", "Moved: ")
            } else {
                crate::gui::ui_text("Kopyalandı: ", "Copied: ")
            });
            b.push_str(&created);
            self.push_line(b.as_str(), Color::ACCENT_GREEN);
        }
    }

    /// `where`/`which`: komut yerlesik mi, degilse VFS'te calistirilabilir mi?
    fn run_where(&mut self, name: &str, vfs: &VirtualFileSystem) {
        let flag = crate::gui::ui_text("yerleşik", "built-in");
        if !canonical_command(name).is_empty() || app_index(name).is_some() {
            let mut b = TextBuf::new();
            b.push_str(name);
            b.push_str(": ");
            b.push_str(flag);
            self.push_line(b.as_str(), Color::ACCENT_GREEN);
        }
        let mut found = false;
        for dir in ["/bin", "/usr/bin", "/system/bin"] {
            let path = alloc::format!("{}/{}", dir, name);
            if let Some(data) = vfs.read_file(&path) {
                let mut b = TextBuf::new();
                b.push_str(&path);
                b.push_str("  (");
                b.push_usize(data.len());
                b.push_str(" B)");
                self.push_line(b.as_str(), Color::ACCENT_BLUE);
                found = true;
            }
        }
        if !found && canonical_command(name).is_empty() && app_index(name).is_none() {
            let mut b = TextBuf::new();
            b.push_str("where: ");
            b.push_str(name);
            b.push_str(crate::gui::ui_text(" bulunamadi", " not found"));
            self.push_line(b.as_str(), Color::ACCENT_YELLOW);
        }
    }

    /// `set`: parametresiz cagrida ortam degiskenlerini listeler,
    /// `set AD=deger` biciminde yalnizca onbellegi korur (kalici ortam yok).
    fn run_set(&mut self, arg: &str) {
        if arg.is_empty() {
            let mut b = TextBuf::new();
            b.push_str("PWD=");
            b.push_str(self.cwd_str());
            self.push_line(b.as_str(), Color::TEXT_MAIN);
            self.push_line(
                crate::gui::ui_text(
                    "SHELL=MinShell  TERM=minOs  HOME=/root/Documents",
                    "SHELL=MinShell  TERM=minOs  HOME=/root/Documents",
                ),
                Color::TEXT_MAIN,
            );
            let mut lang = TextBuf::new();
            lang.push_str("LANG=");
            lang.push_str(if crate::gui::ui_english() { "en" } else { "tr" });
            self.push_line(lang.as_str(), Color::TEXT_MAIN);
            return;
        }
        if arg.contains('=') {
            self.push_line(
                crate::gui::ui_text(
                    "Ortam değişkenleri bu oturumda kalıcı ortam olarak saklanmaz.",
                    "Environment variables are not persisted in this session.",
                ),
                Color::ACCENT_YELLOW,
            );
        } else {
            self.push_line(
                crate::gui::ui_text("Kullanım: set [AD=deger]", "Usage: set [NAME=value]"),
                Color::ACCENT_YELLOW,
            );
        }
    }

    /// Bir yolun ust dizinini dondurur (`/a/b` -> `/a`).
    fn parent_of(path: &str) -> &str {
        match path.rfind('/') {
            Some(0) | None => "/",
            Some(pos) => &path[..pos],
        }
    }

    fn run_hexdump(&mut self, arg: &str, vfs: &VirtualFileSystem) {
        if arg.is_empty() {
            self.push_line("Usage: hexdump <file>", Color::ACCENT_YELLOW);
            return;
        }
        let path = if arg.starts_with('/') {
            alloc::string::String::from(arg)
        } else {
            alloc::format!("{}/{}", self.cwd_str().trim_end_matches('/'), arg)
        };
        let Some(data) = vfs.read_file(&path) else {
            let mut line = TextBuf::new();
            line.push_str("hexdump: file not found: ");
            line.push_str(arg);
            self.push_line(line.as_str(), Color::ACCENT_RED);
            return;
        };

        const HEX: &[u8; 16] = b"0123456789abcdef";
        const LIMIT: usize = 1024;
        for offset in (0..data.len().min(LIMIT)).step_by(16) {
            let mut line = TextBuf::new();
            for shift in (0..8).rev() {
                line.push_byte(HEX[(offset >> (shift * 4)) & 0x0f]);
            }
            line.push_str(": ");
            for byte in data[offset..(offset + 16).min(data.len())].iter().copied() {
                line.push_byte(HEX[(byte >> 4) as usize]);
                line.push_byte(HEX[(byte & 0x0f) as usize]);
                line.push_byte(b' ');
            }
            self.push_line(line.as_str(), Color::TEXT_MAIN);
        }
        if data.len() > LIMIT {
            let mut line = TextBuf::new();
            line.push_str("... truncated; total ");
            line.push_usize(data.len());
            line.push_str(" bytes");
            self.push_line(line.as_str(), Color::TEXT_MUTED);
        } else {
            let mut line = TextBuf::new();
            line.push_usize(data.len());
            line.push_str(" bytes");
            self.push_line(line.as_str(), Color::TEXT_MUTED);
        }
    }

    fn run_netcat_lite(&mut self, arg: &str) {
        let mut parts = arg.splitn(3, ' ');
        let host = parts.next().unwrap_or("");
        let port_text = parts.next().unwrap_or("");
        let payload = parts.next().unwrap_or("");
        let Ok(port) = port_text.parse::<u16>() else {
            self.push_line("Usage: nc <host> <port> [text]", Color::ACCENT_YELLOW);
            return;
        };
        if host.is_empty() || port == 0 {
            self.push_line("Usage: nc <host> <port> [text]", Color::ACCENT_YELLOW);
            return;
        }
        if !crate::drivers::e1000::is_ready() {
            self.push_line("nc: network adapter is unavailable", Color::ACCENT_RED);
            return;
        }
        let ip = match crate::drivers::netstack::parse_ipv4(host) {
            Some(ip) => ip,
            None => match crate::drivers::socket::resolve_host(host) {
                Ok(ip) => ip,
                Err(_) => {
                    self.push_line("nc: host lookup failed", Color::ACCENT_RED);
                    return;
                }
            },
        };
        use crate::drivers::socket;
        let fd = match socket::socket(socket::AF_INET, socket::SOCK_STREAM) {
            Ok(fd) => fd,
            Err(_) => {
                self.push_line("nc: could not allocate a socket", Color::ACCENT_RED);
                return;
            }
        };
        let _ = socket::set_timeout(fd, 3000);
        if socket::connect(fd, ip, port).is_err() {
            socket::close(fd);
            self.push_line("nc: connection failed or timed out", Color::ACCENT_RED);
            return;
        }
        if !payload.is_empty() && socket::send(fd, payload.as_bytes()).is_err() {
            socket::close(fd);
            self.push_line("nc: send failed", Color::ACCENT_RED);
            return;
        }
        let mut buf = [0u8; 256];
        match socket::recv(fd, &mut buf) {
            Ok(0) => self.push_line("nc: peer closed without sending data", Color::TEXT_MUTED),
            Ok(n) => {
                if let Ok(text) = core::str::from_utf8(&buf[..n]) {
                    for line in text.split('\n') {
                        if !line.is_empty() {
                            self.push_line(line.trim_end_matches('\r'), Color::TEXT_MAIN);
                        }
                    }
                } else {
                    self.push_line("nc: received binary data", Color::ACCENT_YELLOW);
                    let mut line = TextBuf::new();
                    for byte in buf[..n.min(24)].iter().copied() {
                        if !line.is_empty() {
                            line.push_byte(b' ');
                        }
                        line.push_byte(b"0123456789abcdef"[(byte >> 4) as usize]);
                        line.push_byte(b"0123456789abcdef"[(byte & 0x0f) as usize]);
                    }
                    self.push_line(line.as_str(), Color::TEXT_MAIN);
                }
            }
            Err(_) => self.push_line("nc: no response before timeout", Color::ACCENT_YELLOW),
        }
        socket::close(fd);
    }

    fn run_fortune(&mut self) {
        const QUOTES: [&str; 5] = [
            "Small steps still move you forward.",
            "A curious cat finds the useful bug.",
            "Make it work, then make it clear.",
            "The best backup is the one you tested.",
            "There is no shortcut around a good readme.",
        ];
        let idx = (crate::monotonic_ms() as usize / 1000) % QUOTES.len();
        self.push_line(QUOTES[idx], Color::ACCENT_PINK);
    }

    fn run_benchmark(&mut self, pmm: &PhysicalMemoryManager) {
        let start = crate::monotonic_ms();
        let mut checksum = 0u64;
        for i in 0..200_000u64 {
            checksum = checksum.rotate_left(5) ^ i.wrapping_mul(0x9e37_79b9);
        }
        core::hint::black_box(checksum);
        let elapsed = crate::monotonic_ms().wrapping_sub(start);
        let mut line = TextBuf::new();
        line.push_str("CPU: 200000 iterations in ");
        line.push_usize(elapsed as usize);
        line.push_str(" ms; checksum=0x");
        line.push_hex64(checksum);
        self.push_line(line.as_str(), Color::ACCENT_GREEN);
        let mut memory = TextBuf::new();
        memory.push_str("PMM free: ");
        memory.push_usize((pmm.free_memory_bytes / 1024) as usize);
        memory.push_str(" KB / ");
        memory.push_usize((pmm.total_memory_bytes / 1024) as usize);
        memory.push_str(" KB");
        self.push_line(memory.as_str(), Color::TEXT_MAIN);
    }

    fn run_sarman_cli(&mut self, arg: &str, pmm: &PhysicalMemoryManager) {
        match arg.trim() {
            "status" => {
                self.push_line("minOs Sarman CLI status", Color::ACCENT_PINK);
                let mut uptime = TextBuf::new();
                uptime.push_str("Uptime: ");
                uptime.push_usize((crate::monotonic_ms() / 1000) as usize);
                uptime.push_str(" seconds");
                self.push_line(uptime.as_str(), Color::TEXT_MAIN);
                let mut memory = TextBuf::new();
                memory.push_str("Free memory: ");
                memory.push_usize((pmm.free_memory_bytes / 1024) as usize);
                memory.push_str(" KB");
                self.push_line(memory.as_str(), Color::TEXT_MAIN);
                self.push_line(
                    if crate::drivers::e1000::is_ready() {
                        "Network: adapter ready"
                    } else {
                        "Network: adapter unavailable"
                    },
                    Color::ACCENT_GREEN,
                );
            }
            "fortune" => self.run_fortune(),
            _ => self.push_line("Usage: sarman-cli status|fortune", Color::ACCENT_YELLOW),
        }
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
        let prompt_fg = if is_light {
            0x1E8E3E
        } else {
            Color::ACCENT_GREEN
        };
        let title_fg = if is_light {
            0x188038
        } else {
            Color::ACCENT_GREEN
        };

        // Terminal zemin (temaya duyarlı iç konsol kutusu)
        fb.draw_rect(
            x + 8,
            y + 34,
            w.saturating_sub(16),
            h.saturating_sub(44),
            term_bg,
        );
        fb.draw_rect_outline(
            x + 8,
            y + 34,
            w.saturating_sub(16),
            h.saturating_sub(44),
            Color::BORDER,
        );

        // Başlık
        MinGuiCompositor::draw_terminal_string(
            fb,
            x + 16,
            y + 44,
            crate::gui::ui_text(
                "minOs Komut İstemi v2.0 (MinShell)",
                "minOs Command Prompt v2.0 (MinShell)",
            ),
            title_fg,
        );

        let line_height = crate::gui::font_render::text_height(
            crate::gui::font_render::Face::Geist,
            14.0 * crate::gui::font_zoom() as f32 / 100.0,
        )
        .max(1);
        let line_gap = 5;
        let status_y = y + h.saturating_sub(line_height + 14);
        let prompt_y = status_y.saturating_sub(line_height + 8);
        let output_y = y + 60;
        let view_lines = prompt_y.saturating_sub(output_y) / (line_height + line_gap);
        let first = self
            .out_count
            .saturating_sub(view_lines)
            .saturating_sub(self.output_scroll);
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
            MinGuiCompositor::draw_terminal_string_clipped(
                fb,
                x + 16,
                ly,
                self.out[i].as_str(),
                col,
                content_clip,
            );
            ly += line_height + line_gap;
        }

        // Aktif Istem + Girdi (CMD tarzi: C:\yol> )
        let prompt = alloc::format!("{}>", self.cwd_str());
        let prompt_w = MinGuiCompositor::terminal_text_width(&prompt) + 6;
        MinGuiCompositor::draw_terminal_string_clipped(
            fb,
            x + 16,
            prompt_y,
            &prompt,
            prompt_fg,
            (x + 12, prompt_y, w.saturating_sub(24), line_height + 4),
        );
        let input_str = core::str::from_utf8(&self.input[..self.input_len]).unwrap_or("");
        let input_x = x + 16 + prompt_w;
        let input_clip = (
            input_x,
            prompt_y,
            w.saturating_sub(prompt_w + 28),
            line_height + 4,
        );
        MinGuiCompositor::draw_terminal_string_clipped(
            fb, input_x, prompt_y, input_str, default_fg, input_clip,
        );
        if is_focused {
            let caret_x = input_x + MinGuiCompositor::terminal_text_width(input_str);
            if caret_x < x + w.saturating_sub(16) {
                fb.draw_rect(
                    caret_x,
                    prompt_y + 2,
                    3,
                    line_height.saturating_sub(4).max(4),
                    prompt_fg,
                );
            }
        }

        // Durum satırı
        let mut status = TextBuf::new();
        status.push_str(crate::gui::ui_text("geçmiş: ", "history: "));
        status.push_usize(self.history_count);
        status.push_str(crate::gui::ui_text(
            " komut | UTF-8 / TR-Q",
            " commands | UTF-8 / TR-Q",
        ));
        MinGuiCompositor::draw_terminal_string(fb, x + 16, status_y, status.as_str(), muted_fg);
    }
}

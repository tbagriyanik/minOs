//! Seviyeli ve hedefe göre filtrelenebilir çekirdek log katmanı.
//!
//! `log` crate'i façade olarak kullanılır; tek bir [`SerialLogger`] kayıtları
//! `boot::console::serial_print` üzerinden seri konsola (ve açıkken açılış
//! metin konsoluna) yazar. Seviyeler `trace < debug < info < warn < error`
//! sırasında değerlendirilir; `off` her şeyi kapatır.
//!
//! Filtre iki katmanlıdır:
//! - Genel seviye: [`set_global_level`] (varsayılan `info`).
//! - Hedef (target) kuralları: `"boot"`, `"driver:<ad>"`, `"net"` gibi kısa
//!   önekler. Kayıt hedefi kural önekiyle başlıyorsa (ya da modül yolu
//!   `minos_kernel::` öneki soyulduktan sonra) ilgili seviye geçerlidir;
//!   aksi halde genel seviye kullanılır.
//!
//! Başlangıç değerleri Limine cmdline'ındaki `log=` belirtecinden okunur:
//! `log=debug`, `log=info,net:trace,driver:debug` gibi virgülle ayrılmış
//! `hedef:seviye` çiftleri kabul edilir (`log=off` her şeyi kapatır).
//!
//! Çalışma anında seri konsoldan `log ...` komutlarıyla değiştirilebilir
//! (bkz. [`poll_serial_console`]; görev [`spawn_serial_console_task`] ile
//! sürücü başlatma sonrası kuyruğa alınır).

use crate::arch::x86_64::sync::SpinMutex;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, Ordering};
use heapless::{String as HString, Vec as HVec};

// Seviye kodları: 0 = kapali, 1..=5 artan önemde degil, dusuktan yükseğe.
pub const LEVEL_OFF: u8 = 0;
pub const LEVEL_ERROR: u8 = 1;
pub const LEVEL_WARN: u8 = 2;
pub const LEVEL_INFO: u8 = 3;
pub const LEVEL_DEBUG: u8 = 4;
pub const LEVEL_TRACE: u8 = 5;

const MAX_TARGET_RULES: usize = 8;
const TARGET_LEN: usize = 32;

static GLOBAL_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_INFO);

struct TargetRule {
    target: HString<TARGET_LEN>,
    level: u8,
}

static TARGET_RULES: SpinMutex<HVec<TargetRule, MAX_TARGET_RULES>> = SpinMutex::new(HVec::new());

// ---------------------------------------------------------------------------
// Seviye yardımcıları
// ---------------------------------------------------------------------------

/// `trace|debug|info|warn|error|off` (büyük/küçük harf duyarsız) → seviye kodu.
pub fn parse_level(s: &str) -> Option<u8> {
    if s.eq_ignore_ascii_case("off") {
        return Some(LEVEL_OFF);
    }
    if s.eq_ignore_ascii_case("error") {
        return Some(LEVEL_ERROR);
    }
    if s.eq_ignore_ascii_case("warn") || s.eq_ignore_ascii_case("warning") {
        return Some(LEVEL_WARN);
    }
    if s.eq_ignore_ascii_case("info") {
        return Some(LEVEL_INFO);
    }
    if s.eq_ignore_ascii_case("debug") {
        return Some(LEVEL_DEBUG);
    }
    if s.eq_ignore_ascii_case("trace") {
        return Some(LEVEL_TRACE);
    }
    None
}

pub fn level_name(level: u8) -> &'static str {
    match level {
        LEVEL_ERROR => "ERROR",
        LEVEL_WARN => "WARN",
        LEVEL_INFO => "INFO",
        LEVEL_DEBUG => "DEBUG",
        LEVEL_TRACE => "TRACE",
        _ => "OFF",
    }
}

fn log_level_to_u8(level: log::Level) -> u8 {
    match level {
        log::Level::Error => LEVEL_ERROR,
        log::Level::Warn => LEVEL_WARN,
        log::Level::Info => LEVEL_INFO,
        log::Level::Debug => LEVEL_DEBUG,
        log::Level::Trace => LEVEL_TRACE,
    }
}

// ---------------------------------------------------------------------------
// Filtre
// ---------------------------------------------------------------------------

/// `minos_kernel::` gibi crate kökü önekini soyup kalan yolu döndürür.
fn normalize_target(target: &str) -> &str {
    target
        .strip_prefix("minos_kernel::")
        .or_else(|| target.strip_prefix("::"))
        .unwrap_or(target)
}

/// Hedef için geçerli seviye: eşleşen en uzun kural, yoksa genel seviye.
fn effective_level(target: &str) -> u8 {
    let normalized = normalize_target(target);
    let mut best: Option<(usize, u8)> = None;
    {
        let rules = TARGET_RULES.lock();
        for rule in rules.iter() {
            let prefix: &str = &rule.target;
            if prefix.is_empty() {
                continue;
            }
            let matched = normalized == prefix
                || normalized.starts_with(prefix)
                || target.starts_with(prefix);
            if matched && best.is_none_or(|(len, _)| prefix.len() > len) {
                best = Some((prefix.len(), rule.level));
            }
        }
    }
    best.map(|(_, level)| level)
        .unwrap_or_else(|| GLOBAL_LEVEL.load(Ordering::Relaxed))
}

/// Bir kaydın yazılıp yazılmayacağını döndürür.
pub fn enabled(level: u8, target: &str) -> bool {
    level <= effective_level(target)
}

pub fn global_level() -> u8 {
    GLOBAL_LEVEL.load(Ordering::Relaxed)
}

pub fn set_global_level(level: u8) {
    GLOBAL_LEVEL.store(level, Ordering::Relaxed);
}

/// Hedef kuralı ekler/günceller. Kapasite doluysa `false` döner.
pub fn set_target_level(target: &str, level: u8) -> bool {
    let mut rules = TARGET_RULES.lock();
    if let Some(rule) = rules.iter_mut().find(|rule| &*rule.target == target) {
        rule.level = level;
        return true;
    }
    let Ok(name) = HString::<TARGET_LEN>::try_from(target) else {
        return false;
    };
    rules
        .push(TargetRule {
            target: name,
            level,
        })
        .is_ok()
}

pub fn clear_target_levels() {
    TARGET_RULES.lock().clear();
}

// ---------------------------------------------------------------------------
// Seri Logger (log::Log uygulaması)
// ---------------------------------------------------------------------------

/// Çıktı tamponu: yığın üstünde sabit boyut; taşma durumunda kuyruk
/// kırpılır ve `...` eklenir. `fmt::Write` uygulaması hiçbir zaman
/// ayırma yapmaz (IRQ bağlamında da güvenli).
struct LineBuf {
    buf: [u8; 512],
    len: usize,
    truncated: bool,
}

impl LineBuf {
    const fn new() -> Self {
        Self {
            buf: [0; 512],
            len: 0,
            truncated: false,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for LineBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // Sonda "..." için 4 bayt rezervi tut.
        if self.len + s.len() > self.buf.len().saturating_sub(4) {
            self.truncated = true;
            return Err(core::fmt::Error);
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

struct SerialLogger;

impl log::Log for SerialLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        enabled(log_level_to_u8(metadata.level()), metadata.target())
    }

    fn log(&self, record: &log::Record) {
        let level = log_level_to_u8(record.level());
        if !enabled(level, record.target()) {
            return;
        }
        let mut out = LineBuf::new();
        let _ = write!(out, "[{}][{}] ", level_name(level), record.target());
        let _ = write!(out, "{}", record.args());
        if out.truncated {
            let _ = write!(out, "...");
        }
        if !out.as_str().ends_with('\n') {
            let _ = writeln!(out);
        }
        crate::boot::console::serial_print(out.as_str());
    }

    fn flush(&self) {}
}

static LOGGER: SerialLogger = SerialLogger;

/// Logger'ı kaydeder ve cmdline `log=` belirtecini uygular.
/// `serial::init()` ve heap başlatıldıktan sonra çağrılmalıdır.
pub fn init() {
    let _ = log::set_logger(&LOGGER);
    // Tüm seviyeler logger'a ulaşsın; gerçek filtre `enabled`/`log` içinde.
    log::set_max_level(log::LevelFilter::Trace);
    apply_cmdline_defaults();
}

fn apply_cmdline_defaults() {
    let Some(cmdline) = crate::boot::limine::limine_boot_cmdline() else {
        return;
    };
    for token in cmdline.split(|c: char| c.is_whitespace()) {
        let Some(spec) = token.strip_prefix("log=") else {
            continue;
        };
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once(':') {
                Some((target, level)) => {
                    if let Some(level) = parse_level(level.trim()) {
                        set_target_level(target.trim(), level);
                    }
                }
                None => {
                    if let Some(level) = parse_level(part) {
                        set_global_level(level);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Çalışma anı seri konsol komutu: `log ...`
// ---------------------------------------------------------------------------

struct SerialCmdState {
    buf: [u8; 96],
    len: usize,
}

static SERIAL_CMD: SpinMutex<SerialCmdState> = SpinMutex::new(SerialCmdState {
    buf: [0; 96],
    len: 0,
});

fn cmd_respond(parts: core::fmt::Arguments) {
    let mut out = LineBuf::new();
    let _ = write!(out, "[LOG] ");
    let _ = write!(out, "{}", parts);
    if !out.as_str().ends_with('\n') {
        let _ = writeln!(out);
    }
    crate::boot::console::serial_print(out.as_str());
}

fn cmd_status() {
    let rules = TARGET_RULES.lock();
    let mut out = alloc::string::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut out,
        format_args!("genel={} kurallar=[", level_name(global_level())),
    );
    for (i, rule) in rules.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}:{}", &*rule.target, level_name(rule.level));
    }
    out.push_str("]\n");
    crate::boot::console::serial_print(&out);
}

fn cmd_help() {
    cmd_respond(format_args!(
        "kullanim: log | log status | log <off|error|warn|info|debug|trace> | log <hedef> <seviye> | log reset | log help"
    ));
}

fn handle_log_command(line: &str) {
    let line = line.trim();
    let rest = line.strip_prefix("log").unwrap_or("").trim_start();
    if rest.is_empty() || rest.eq_ignore_ascii_case("status") {
        cmd_status();
        return;
    }
    if rest.eq_ignore_ascii_case("help") || rest == "?" {
        cmd_help();
        return;
    }
    if rest.eq_ignore_ascii_case("reset") {
        clear_target_levels();
        set_global_level(LEVEL_INFO);
        cmd_respond(format_args!("filtreler sifirlandi (genel=info)"));
        return;
    }
    // `log <seviye>` → genel seviye; `log <hedef> <seviye>` → hedef kuralı.
    let mut parts = rest.rsplitn(2, char::is_whitespace);
    let level_word = parts.next().unwrap_or("");
    let target_word = parts.next().map(str::trim).filter(|s| !s.is_empty());
    match (target_word, parse_level(level_word)) {
        (None, Some(level)) => {
            set_global_level(level);
            cmd_respond(format_args!("genel seviye = {}", level_name(level)));
        }
        (Some(target), Some(level)) => {
            if set_target_level(target, level) {
                cmd_respond(format_args!("{} = {}", target, level_name(level)));
            } else {
                cmd_respond(format_args!(
                    "hedef kurali eklenemedi (kapasite dolu veya ad cok uzun)"
                ));
            }
        }
        _ => cmd_help(),
    }
}

/// Seri (COM1) girişini yoklar; `log` ile başlayan satırları komut olarak
/// işler. Görev adımından çağrılır; her çağrıda en fazla bir satır tüketir.
pub fn poll_serial_console() {
    // Tek adımda en fazla 64 bayt: uzun bir giriş flooding'i görev bütçesini
    // aşmasın.
    for _ in 0..64 {
        let Some(byte) = crate::serial::poll_rx_byte() else {
            return;
        };
        let mut completed: Option<([u8; 96], usize)> = None;
        {
            let mut guard = SERIAL_CMD.lock();
            let state = &mut *guard;
            match byte {
                b'\n' | b'\r' => {
                    let n = state.len.min(state.buf.len());
                    let mut tmp = [0u8; 96];
                    tmp[..n].copy_from_slice(&state.buf[..n]);
                    state.len = 0;
                    completed = Some((tmp, n));
                }
                0x7F | 0x08 => {
                    state.len = state.len.saturating_sub(1);
                }
                0x20..=0x7E if state.len < state.buf.len() => {
                    let idx = state.len;
                    state.buf[idx] = byte;
                    state.len = idx + 1;
                }
                _ => {}
            }
        }
        if let Some((buf, n)) = completed {
            let line = core::str::from_utf8(&buf[..n]).unwrap_or("");
            if line.starts_with("log") {
                handle_log_command(line);
            } else if !line.trim().is_empty() {
                cmd_respond(format_args!("bilinmeyen komut; 'log help' yazin"));
            }
        }
    }
}

/// Düşük öncelikli görev olarak seri log konsolunu kuyruğa alır.
/// `task::spawn` mevcut olduktan sonra (sürücü başlatma fazı) çağrılır.
pub fn spawn_serial_console_task() {
    crate::task::spawn("serial-log", crate::task::TaskPriority::Low, || {
        poll_serial_console();
        crate::task::TaskStatus::Yielded
    });
}

// ---------------------------------------------------------------------------
// Seviyeli log makroları
// ---------------------------------------------------------------------------
// Kullanım: `kinfo!("boot", "mesaj {}", x)` → `log` crate üzerine kurulu,
// `target` ilk argümandır. Hedefler kısa öneklerdir: "boot", "driver:<ad>",
// "net", "gui" gibi. Seviye/filtre ayrıntısı için modül dokümanına bakın.

#[macro_export]
macro_rules! ktrace {
    ($target:expr, $($arg:tt)*) => {
        ::log::trace!(target: $target, $($arg)*)
    };
}

#[macro_export]
macro_rules! kdebug {
    ($target:expr, $($arg:tt)*) => {
        ::log::debug!(target: $target, $($arg)*)
    };
}

#[macro_export]
macro_rules! kinfo {
    ($target:expr, $($arg:tt)*) => {
        ::log::info!(target: $target, $($arg)*)
    };
}

#[macro_export]
macro_rules! kwarn {
    ($target:expr, $($arg:tt)*) => {
        ::log::warn!(target: $target, $($arg)*)
    };
}

#[macro_export]
macro_rules! kerror {
    ($target:expr, $($arg:tt)*) => {
        ::log::error!(target: $target, $($arg)*)
    };
}

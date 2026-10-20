//! minJs — çok küçük bir istemci tarafı JavaScript alt kümesi (no_std).
//!
//! Tarayıcı `<script>` bloklarını bu modülle çalıştırır. Amaç tam JS değil,
//! sık kullanılan basit betiklerin çalışması ve `document.write` ile
//! sayfanın içeriğinin render'dan önce zenginleştirilmesidir.
//!
//! Desteklenenler:
//! - `console.log/info/warn/error(...)` — toplanır (`JsRuntime.logs`)
//! - `alert(...)` — aynı şekilde loglara yazılır
//! - `document.write(...)` / `document.writeln(...)` — HTML'e gömülür
//! - `var|let|const x = ...` ve `x = ...` atamaları
//! - `+` ile string birleştirme, tırnaklı string'ler
//! - `if` / `else` / `while` / `for` (tek gövdeli, basit koşullar)
//! - Koşullar: `!`, `&&`, `||`, `==`/`!=`/`===`, `<`/`>`, doğru/yanlış
//!
//! Bilinçli olarak desteklenmeyenler (belge amaçlı): nesneler/diziler,
//! `function` tanımları, prototipler, DOM metotları (`getElementById`),
//! `setTimeout`, modüller.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Sayfa boyunca süren JS durumu: değişkenler ve konsol çıktısı.
pub struct JsRuntime {
    pub vars: Vec<(String, String)>,
    pub logs: Vec<String>,
    /// Son `if` koşulunun sonucu (`else` dalı için).
    last_cond: bool,
}

impl JsRuntime {
    pub fn new() -> Self {
        Self { vars: Vec::new(), logs: Vec::new(), last_cond: false }
    }

    fn var_get(&self, name: &str) -> Option<&str> {
        self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn var_set(&mut self, name: &str, value: String) {
        if let Some(entry) = self.vars.iter_mut().find(|(k, _)| k == name) {
            entry.1 = value;
        } else {
            self.vars.push((String::from(name), value));
        }
    }

    /// String birleştirmeyi çözer: `"a" + x + 3`
    fn eval_expr(&self, expr: &str) -> String {
        let b = expr.as_bytes();
        let mut i = 0usize;
        let mut res = String::new();
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace() || c == b'+' { i += 1; continue; }
            if c == b'"' || c == b'\'' || c == b'`' {
                let quote = c;
                i += 1;
                let start = i;
                while i < b.len() {
                    if b[i] == b'\\' && i + 1 < b.len() { i += 2; continue; }
                    if b[i] == quote { break; }
                    i += 1;
                }
                res.push_str(&expr[start..i.min(b.len())]);
                if i < b.len() { i += 1; }
                continue;
            }
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$' || b[i] == b'.') {
                    i += 1;
                }
                let name = &expr[start..i];
                if name.contains('.') {
                    // Özellik erişimi okuma yönünde desteklenmiyor: dur.
                    break;
                }
                match name {
                    "true" => res.push_str("true"),
                    "false" | "null" | "undefined" => {}
                    _ => {
                        if let Some(v) = self.var_get(name) {
                            res.push_str(v);
                        } else if name.bytes().all(|c| c.is_ascii_digit()) {
                            res.push_str(name);
                        }
                    }
                }
                continue;
            }
            // Bilinmeyen token: ayrıştırmayı durdur.
            break;
        }
        res
    }

    /// Koşulu değerlendirir: `!`, `&&`, `||`, karşılaştırmalar ve doğruluk.
    fn eval_cond(&self, cond: &str) -> bool {
        let c = cond.trim();
        if c.is_empty() { return true; }

        if let Some(rest) = c.strip_prefix('!') {
            if !rest.trim_start().starts_with('=') {
                return !self.eval_cond(rest);
            }
        }
        if let Some(idx) = find_top_level(c.as_bytes(), b"&&") {
            return self.eval_cond(&c[..idx]) && self.eval_cond(&c[idx + 2..]);
        }
        if let Some(idx) = find_top_level(c.as_bytes(), b"||") {
            return self.eval_cond(&c[..idx]) || self.eval_cond(&c[idx + 2..]);
        }
        for (op, neg) in [("===", false), ("!==", true), ("==", false), ("!=", true), ("<=", false), (">=", false)] {
            if let Some(idx) = find_top_level(c.as_bytes(), op.as_bytes()) {
                let lhs = self.eval_expr(&c[..idx]);
                let rhs = self.eval_expr(&c[idx + op.len()..]);
                let ord = lhs.as_str().cmp(rhs.as_str());
                let r = match op {
                    "===" | "==" => lhs == rhs,
                    "!==" | "!=" => lhs != rhs,
                    "<=" => ord != core::cmp::Ordering::Greater,
                    _ => ord != core::cmp::Ordering::Less,
                };
                return r != neg;
            }
        }
        for op in ['<', '>'] {
            if let Some(idx) = find_top_level_char(c.as_bytes(), op) {
                let ln: i64 = self.eval_expr(&c[..idx]).trim().parse().unwrap_or(0);
                let rn: i64 = self.eval_expr(&c[idx + 1..]).trim().parse().unwrap_or(0);
                return if op == '<' { ln < rn } else { ln > rn };
            }
        }
        truthy(&self.eval_expr(c))
    }

    /// Tek bir deyimi çalıştırır.
    fn exec(&mut self, stmt: &str, out: &mut Vec<u8>) {
        let mut s = stmt.trim();
        s = s.trim_start_matches(|c| c == '{' || c == '}' || c == ')' || c == ',' || c == ';' || c == '\n' || c == '\r');
        s = s.trim();
        if s.is_empty() { return; }

        // Tanım gövdeleri çalıştırılmaz.
        if keyword(s, "function").is_some() || keyword(s, "class").is_some() || keyword(s, "return").is_some() {
            return;
        }

        // if / while / for — tek gövde, basit döngü taklidi
        for kw in ["if", "while", "for"] {
            if let Some(rest) = keyword(s, kw) {
                let r = rest.trim_start();
                if let Some(open) = r.find('(') {
                    if let Some((cond, after)) = call_args(r, open) {
                        let ok = self.eval_cond(cond);
                        self.last_cond = ok;
                        if ok {
                            self.exec(&r[after..], out);
                        }
                    }
                }
                return;
            }
        }
        if let Some(rest) = keyword(s, "else") {
            if !self.last_cond {
                self.exec(rest, out);
            }
            return;
        }
        if keyword(s, "do").is_some() {
            return;
        }

        // Bilinen çağrılar
        let mut best: Option<(usize, &'static str)> = None;
        for name in CALLS {
            if let Some(idx) = find_bytes(s.as_bytes(), name.as_bytes()) {
                let after = idx + name.len();
                let tail_ok = s.as_bytes().get(after).map_or(false, |c| *c == b'(' || c.is_ascii_whitespace());
                if tail_ok && best.map_or(true, |(b, _)| idx < b) {
                    best = Some((idx, name));
                }
            }
        }
        if let Some((idx, name)) = best {
            let tail = &s[idx + name.len()..];
            let open_rel = tail.find('(').unwrap_or(0);
            if let Some((args, _)) = call_args(tail, open_rel) {
                let value = self.eval_expr(args);
                if name.starts_with("console.") || name == "alert" || name == "window.alert" {
                    self.logs.push(value);
                } else {
                    out.extend_from_slice(value.as_bytes());
                    if name == "document.writeln" { out.push(b'\n'); }
                }
                return;
            }
        }

        // Atama: var/let/const name = expr  |  name = expr
        let mut rest = s;
        for kw in ["var", "let", "const"] {
            if let Some(r) = keyword(s, kw) {
                rest = r.trim_start();
                break;
            }
        }
        let bytes = rest.as_bytes();
        let mut n = 0;
        while n < bytes.len() && (bytes[n].is_ascii_alphanumeric() || bytes[n] == b'_' || bytes[n] == b'$') {
            n += 1;
        }
        if n > 0 && n < bytes.len() {
            let after_name = rest[n..].trim_start();
            if let Some(eq) = after_name.strip_prefix('=') {
                if !eq.trim_start().starts_with('=') {
                    let name = &rest[..n];
                    if !name.contains('.') {
                        let value = self.eval_expr(eq);
                        self.var_set(name, value);
                        return;
                    }
                }
            }
        }
    }

    /// Betiği sırayla çalıştırır; `document.write` çıktıları `out`'a eklenir.
    pub fn run(&mut self, code: &str, out: &mut Vec<u8>) {
        for stmt in split_statements(code) {
            self.exec(&stmt, out);
        }
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() { return None; }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() { return None; }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// `<script ...>` etiketi içindeki `attr="değer"` okur.
fn attr_of(tag: &[u8], key: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < tag.len() {
        while i < tag.len() && (tag[i].is_ascii_whitespace() || tag[i] == b'/') { i += 1; }
        let k_start = i;
        while i < tag.len() && (tag[i].is_ascii_alphanumeric() || tag[i] == b'-') { i += 1; }
        let cur = &tag[k_start..i];
        while i < tag.len() && tag[i].is_ascii_whitespace() { i += 1; }
        if i < tag.len() && tag[i] == b'=' {
            i += 1;
            while i < tag.len() && tag[i].is_ascii_whitespace() { i += 1; }
            let quote = if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                let q = tag[i]; i += 1; q
            } else { 0 };
            let v_start = i;
            while i < tag.len() && ((quote != 0 && tag[i] != quote) || (quote == 0 && !tag[i].is_ascii_whitespace() && tag[i] != b'>')) {
                i += 1;
            }
            let val = String::from_utf8_lossy(&tag[v_start..i]).into_owned();
            if quote != 0 && i < tag.len() { i += 1; }
            if cur.eq_ignore_ascii_case(key) { return Some(val); }
        } else if i == k_start {
            i += 1;
        }
    }
    None
}

/// `if` / `var` gibi anahtar kelimenin `s` ile başladığını kontrol eder.
fn keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    if let Some(rest) = s.strip_prefix(kw) {
        match rest.chars().next() {
            None => Some(rest),
            Some(c) if !(c.is_alphanumeric() || c == '_' || c == '$') => Some(rest),
            _ => None,
        }
    } else {
        None
    }
}

fn truthy(v: &str) -> bool {
    let t = v.trim();
    !(t.is_empty() || t == "false" || t == "0" || t == "null" || t == "undefined" || t == "NaN")
}

/// Parantez/süslü parantez içermeyen ilk eşleşmenin konumunu bulur.
fn find_top_level(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' { i += 2; continue; }
            if c == q { quote = None; }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {
                if depth == 0 && i + needle.len() <= hay.len() && &hay[i..i + needle.len()] == needle {
                    // `==` iken `===` ile karışmasın
                    if (needle == b"==" || needle == b"!=") && hay[i + 2..].first() == Some(&b'=') {
                        i += 1;
                        continue;
                    }
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

fn find_top_level_char(hay: &[u8], target: char) -> Option<usize> {
    let tb = target as u32;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < hay.len() {
        let c = hay[i];
        if let Some(q) = quote {
            if c == b'\\' { i += 2; continue; }
            if c == q { quote = None; }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {
                if depth == 0 && (c as u32) == tb {
                    // `<=`, `>=`, `=>` karşılaştırmaları değil
                    if (target == '<' || target == '>') && hay.get(i + 1) == Some(&b'=') {
                        i += 1;
                        continue;
                    }
                    if target == '>' && i > 0 && hay[i - 1] == b'=' {
                        i += 1;
                        continue;
                    }
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Bir çağrının argümanlarını (`(` ile `)` arası) ve kapanış konumunu bulur.
fn call_args(s: &str, open_idx: usize) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if open_idx >= b.len() || b[open_idx] != b'(' { return None; }
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = open_idx;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' { i += 2; continue; }
            if c == q { quote = None; }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 { return Some((&s[open_idx + 1..i], i + 1)); }
            }
            _ => {}
        }
        i += 1;
    }
    // Kapanmayan parantez: kalanı argüman say.
    let rest = s[open_idx + 1..].trim_end_matches(|c| c == ')' || c == '}' || c == ';' || c == '\n' || c == '\r');
    Some((rest, b.len()))
}

const CALLS: [&str; 9] = [
    "console.log", "console.info", "console.warn", "console.error", "console.debug",
    "document.write", "document.writeln", "alert", "window.alert",
];

/// JS kaynak kodunu `;` ve satır sonlarına göre deyimlere böler
/// (dize literalari ve parantez içleri korunur).
fn split_statements(code: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let b = code.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut brack = 0i32;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' { i += 2; continue; }
            if c == q { quote = None; }
            i += 1;
            continue;
        }
        let mut boundary = false;
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'{' => brace += 1,
            b'}' => {
                brace -= 1;
                if brace <= 0 && paren <= 0 && brack <= 0 {
                    brace = 0;
                    boundary = true;
                }
            }
            b'[' => brack += 1,
            b']' => brack -= 1,
            b';' if paren <= 0 && brace <= 0 && brack <= 0 => boundary = true,
            b'\n' if paren <= 0 && brace <= 0 && brack <= 0 => {
                let up = code[..i].trim_end();
                if up.ends_with(')') || up.ends_with('}') { boundary = true; }
            }
            _ => {}
        }
        if boundary {
            let piece = code[start..i + 1].trim();
            if !piece.is_empty() { out.push(String::from(piece)); }
            start = i + 1;
        }
        i += 1;
    }
    let tail = code[start..].trim();
    if !tail.is_empty() { out.push(String::from(tail)); }
    out
}

/// Betiği çalıştırır (kısa yol).
pub fn run_script(code: &str, rt: &mut JsRuntime, out: &mut Vec<u8>) {
    rt.run(code, out);
}

/// HTML içindeki tüm `<script>` bloklarını sırayla çalıştırır.
///
/// `document.write(...)` çıktıları, betiğin bulunduğu yere HTML olarak gömülür;
/// betik etiketleri ise çıktidan çıkarılır.
pub fn inline_scripts(html: &[u8]) -> Vec<u8> {
    let mut rt = JsRuntime::new();
    let mut out: Vec<u8> = Vec::with_capacity(html.len());
    let mut p = 0usize;
    while p < html.len() {
        match find_ci(&html[p..], b"<script") {
            None => {
                out.extend_from_slice(&html[p..]);
                break;
            }
            Some(rel) => {
                let tag_start = p + rel;
                let mut tag_end = tag_start;
                while tag_end < html.len() && html[tag_end] != b'>' { tag_end += 1; }
                if tag_end >= html.len() {
                    out.extend_from_slice(&html[p..]);
                    break;
                }
                out.extend_from_slice(&html[p..tag_start]);
                let opening = &html[tag_start..tag_end + 1];
                let content_start = tag_end + 1;
                let (content, next) = match find_ci(&html[content_start..], b"</script>") {
                    Some(r) => (&html[content_start..content_start + r], content_start + r + 9),
                    None => (&html[content_start..], html.len()),
                };
                let executable = match attr_of(opening, b"type") {
                    None => true,
                    Some(t) => {
                        let t = t.to_ascii_lowercase();
                        t.contains("javascript") || t.contains("module") || !t.contains('/')
                    }
                };
                if executable {
                    let code = String::from_utf8_lossy(content).into_owned();
                    rt.run(&code, &mut out);
                }
                p = next;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec::Vec;

    fn render(js: &str) -> String {
        let html = format!("<script>{}</script>", js);
        let out = inline_scripts(html.as_bytes());
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn document_write_basic() {
        assert_eq!(render(r#"document.write("Merhaba");"#), "Merhaba");
    }

    #[test]
    fn concatenation_and_vars() {
        let r = render(r#"var a = "Dünya"; document.write("Merhaba " + a + "!");"#);
        assert_eq!(r, "Merhaba Dünya!");
    }

    #[test]
    fn console_log_collected() {
        let mut rt = JsRuntime::new();
        let mut out: Vec<u8> = Vec::new();
        rt.run(r#"console.log("x", 1); document.write("y");"#, &mut out);
        assert_eq!(rt.logs.len(), 1);
        assert_eq!(rt.logs[0], "x");
        assert_eq!(String::from_utf8_lossy(&out), "y");
    }

    #[test]
    fn if_else_branches() {
        assert_eq!(render(r#"var x = "2"; if (x == "2") { document.write("esit"); } else { document.write("degil"); }"#), "esit");
        assert_eq!(render(r#"if (1 > 2) { document.write("a"); } else { document.write("b"); }"#), "b");
        assert_eq!(render(r#"if (!false) { document.write("ok"); }"#), "ok");
    }

    #[test]
    fn script_without_semicolons() {
        let r = render("document.write(\"A\")\ndocument.write(\"B\")");
        assert_eq!(r, "AB");
    }

    #[test]
    fn non_javascript_type_skipped() {
        let html = br#"<script type="application/json">{"a":1}</script>"#;
        assert_eq!(String::from_utf8_lossy(&inline_scripts(html)), "");
    }

    #[test]
    fn outer_html_kept() {
        let html = b"<p>Once</p><script>document.write('!')</script><p>Sonra</p>";
        assert_eq!(String::from_utf8_lossy(&inline_scripts(html)), "<p>Once</p>!<p>Sonra</p>");
    }

    #[test]
    fn function_bodies_not_executed_at_definition() {
        let r = render(r#"function f(){ document.write("x"); } document.write("y");"#);
        assert_eq!(r, "y");
    }
}

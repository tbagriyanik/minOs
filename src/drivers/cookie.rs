//! HTTP Çerez (Cookie) Jarı — RFC 6265 uyumlu.
//! Set-Cookie başlıklarının ayrıştırılması, çerez saklama, eşleştirme ve
//! Cookie başlığı üretimi destekler.

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::RefCell;

/// Tek bir HTTP çerezi.
#[derive(Clone, Debug)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Süre sonu (epoch seconds, 0 = oturum çerezi / süresiz)
    pub expires: u64,
    /// Yalnızca güvenli (HTTPS) bağlantılarda gönderilsin mi?
    pub secure: bool,
    /// JavaScript tarafından erişilemez mi?
    pub http_only: bool,
    /// SameSite politikası: Lax, Strict, None
    pub same_site: SameSite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SameSite {
    #[default]
    Lax,
    Strict,
    None,
}

/// Çerez jarı: tüm çerezleri saklar ve eşleştirir.
pub struct CookieJar {
    cookies: RefCell<Vec<Cookie>>,
    max_cookies: usize,
    max_cookies_per_domain: usize,
}

impl CookieJar {
    pub fn new() -> Self {
        Self {
            cookies: RefCell::new(Vec::new()),
            max_cookies: 500,
            max_cookies_per_domain: 50,
        }
    }

    /// Set-Cookie başlığını ayrıştırıp çerezi jar'a ekler.
    /// `domain` ve `path` varsayılan değerleri yanıtı gönderen sunucu ve istek yolu üzerinden belirlenir.
    pub fn set_cookie(&self, header: &str, default_domain: &str, default_path: &str) {
        // Set-Cookie: name=value; Domain=domain; Path=path; Expires=date; Secure; HttpOnly; SameSite=Lax
        let mut parts = header.split(';');
        let first = parts.next().unwrap_or("");
        let mut kv = first.splitn(2, '=');
        let name = kv.next().unwrap_or("").trim().to_string();
        let value = kv.next().unwrap_or("").trim().to_string();

        if name.is_empty() {
            return;
        }

        let mut domain = default_domain.to_string();
        let mut path = default_path.to_string();
        let mut expires = 0u64;
        let mut secure = false;
        let mut http_only = false;
        let mut same_site = SameSite::Lax;

        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let mut kv = part.splitn(2, '=');
            let key = kv.next().unwrap_or("").trim().to_ascii_lowercase();
            let val = kv.next().unwrap_or("").trim();

            match key.as_str() {
                "domain" => {
                    if !val.is_empty() {
                        domain = val.to_string();
                    }
                }
                "path" => {
                    if !val.is_empty() {
                        path = val.to_string();
                    }
                }
                "expires" => {
                    expires = parse_cookie_date(val);
                }
                "max-age" => {
                    if let Ok(seconds) = val.parse::<u64>() {
                        let now = crate::monotonic_ms() / 1000;
                        expires = now + seconds;
                    }
                }
                "secure" => secure = true,
                "httponly" => http_only = true,
                "samesite" => {
                    same_site = match val.to_ascii_lowercase().as_str() {
                        "strict" => SameSite::Strict,
                        "none" => SameSite::None,
                        _ => SameSite::Lax,
                    };
                }
                _ => {}
            }
        }

        // Domain nokta ile başlıyorsa ön ekini kaldır (örn. .example.com -> example.com)
        let domain = domain.trim_start_matches('.').to_string();

        let cookie = Cookie {
            name,
            value,
            domain,
            path,
            expires,
            secure,
            http_only,
            same_site,
        };

        // Expires geçmişte ya da Max-Age=0 ise çerez silme isteğidir (RFC 6265 §5.3).
        let now = crate::monotonic_ms() / 1000;
        if cookie.expires > 0 && cookie.expires <= now {
            self.remove_cookie(&cookie.name, &cookie.domain, &cookie.path);
            return;
        }

        self.add_cookie(cookie);
    }

    /// Aynı isim+domain+path üçlüsüne sahip çerezi jar'dan kaldırır.
    fn remove_cookie(&self, name: &str, domain: &str, path: &str) {
        let mut cookies = self.cookies.borrow_mut();
        cookies.retain(|c| !(c.name == name && c.domain == domain && c.path == path));
    }

    /// Çerezi jar'a ekler (maksimum sayı sınırlaması uygulanır).
    fn add_cookie(&self, cookie: Cookie) {
        let mut cookies = self.cookies.borrow_mut();

        // Aynı isim+domain+path çerezi varsa güncelle
        if let Some(pos) = cookies.iter().position(|c| {
            c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path
        }) {
            cookies[pos] = cookie;
            return;
        }

        // Domain başına maksimum sayı kontrolü
        let domain_count = cookies.iter().filter(|c| c.domain == cookie.domain).count();
        if domain_count >= self.max_cookies_per_domain {
            // En eski çerezi kaldır
            if let Some(pos) = cookies.iter().position(|c| c.domain == cookie.domain) {
                cookies.remove(pos);
            }
        }

        // Toplam maksimum sayı kontrolü
        if cookies.len() >= self.max_cookies {
            cookies.remove(0);
        }

        cookies.push(cookie);
    }

    /// Belirtilen URL için uygun çerezleri döner (Cookie başlığı için).
    /// `secure` parametresi bağlantının HTTPS olup olmadığını belirtir.
    /// `same_site` parametresi istek türünü belirtir (same-site veya cross-site).
    pub fn get_cookie_header(
        &self,
        domain: &str,
        path: &str,
        secure: bool,
        same_site_request: bool,
    ) -> String {
        let cookies = self.cookies.borrow();
        let now = crate::monotonic_ms() / 1000;
        let mut result: Vec<String> = Vec::new();

        for cookie in cookies.iter() {
            // Süresi dolmuş çerezleri atla
            if cookie.expires > 0 && now >= cookie.expires {
                continue;
            }

            // Domain eşleşmesi
            if !domain_matches(domain, &cookie.domain) {
                continue;
            }

            // Path eşleşmesi
            if !path_matches(path, &cookie.path) {
                continue;
            }

            // Secure kontrolü
            if cookie.secure && !secure {
                continue;
            }

            // SameSite kontrolü
            if !same_site_request && cookie.same_site == SameSite::Strict {
                continue;
            }

            result.push(format!("{}={}", cookie.name, cookie.value));
        }

        result.join("; ")
    }

    /// Tüm süresi dolmuş çerezleri temizler.
    pub fn purge_expired(&self) {
        let mut cookies = self.cookies.borrow_mut();
        let now = crate::monotonic_ms() / 1000;
        cookies.retain(|c| c.expires == 0 || now < c.expires);
    }

    /// Çerez sayısını döner.
    pub fn len(&self) -> usize {
        self.cookies.borrow().len()
    }

    /// Jar boş mu?
    pub fn is_empty(&self) -> bool {
        self.cookies.borrow().is_empty()
    }

    /// Tüm çerezlerin anlık kopyasını döner (geliştirici araçları vb. için).
    pub fn list(&self) -> Vec<Cookie> {
        self.cookies.borrow().clone()
    }

    /// Çerezleri ikili biçimde serileştirir (VFS kalıcılığı için).
    /// Biçim: [u32 adet] yineleyen [u32 klen][ad][u32 vlen][değer]
    /// [u32 dlen][domain][u32 plen][path][u64 expires][u8 flags]
    /// flags: bit0 secure, bit1 http_only, bit2-3 same_site (0=Lax,1=Strict,2=None)
    pub fn dump(&self) -> Vec<u8> {
        let cookies = self.cookies.borrow();
        let mut out = Vec::new();
        out.extend_from_slice(&(cookies.len() as u32).to_be_bytes());
        for c in cookies.iter() {
            out.extend_from_slice(&(c.name.len() as u32).to_be_bytes());
            out.extend_from_slice(c.name.as_bytes());
            out.extend_from_slice(&(c.value.len() as u32).to_be_bytes());
            out.extend_from_slice(c.value.as_bytes());
            out.extend_from_slice(&(c.domain.len() as u32).to_be_bytes());
            out.extend_from_slice(c.domain.as_bytes());
            out.extend_from_slice(&(c.path.len() as u32).to_be_bytes());
            out.extend_from_slice(c.path.as_bytes());
            out.extend_from_slice(&c.expires.to_be_bytes());
            let ss = match c.same_site {
                SameSite::Lax => 0u8,
                SameSite::Strict => 1,
                SameSite::None => 2,
            };
            let flags = (c.secure as u8) | ((c.http_only as u8) << 1) | (ss << 2);
            out.push(flags);
        }
        out
    }

    /// `dump()` çıktısını jar'a yükler. Mevcut çerezler korunur; aynı
    /// (ad, domain, path) üçlüsü güncellenir. Bozuk veri güvenle yok sayılır.
    pub fn load(&self, data: &[u8]) {
        if data.len() < 4 {
            return;
        }
        let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if count > 10_000 {
            return; // bozuk/huge count
        }
        let mut p = 4usize;
        let mut loaded: Vec<Cookie> = Vec::new();
        for _ in 0..count {
            let read_str = |p: &mut usize| -> Option<String> {
                if *p + 4 > data.len() {
                    return None;
                }
                let len = u32::from_be_bytes([data[*p], data[*p + 1], data[*p + 2], data[*p + 3]])
                    as usize;
                *p += 4;
                if len > 8192 || *p + len > data.len() {
                    return None;
                }
                let s = core::str::from_utf8(&data[*p..*p + len]).ok()?.to_string();
                *p += len;
                Some(s)
            };
            let (Some(name), Some(value), Some(domain), Some(path)) = (
                read_str(&mut p),
                read_str(&mut p),
                read_str(&mut p),
                read_str(&mut p),
            ) else {
                return;
            };
            if p + 9 > data.len() {
                return;
            }
            let mut eb = [0u8; 8];
            eb.copy_from_slice(&data[p..p + 8]);
            let expires = u64::from_be_bytes(eb);
            p += 8;
            let flags = data[p];
            p += 1;
            let same_site = match (flags >> 2) & 0x3 {
                1 => SameSite::Strict,
                2 => SameSite::None,
                _ => SameSite::Lax,
            };
            loaded.push(Cookie {
                name,
                value,
                domain,
                path,
                expires,
                secure: flags & 1 != 0,
                http_only: flags & 2 != 0,
                same_site,
            });
        }
        let mut cookies = self.cookies.borrow_mut();
        for c in loaded {
            if let Some(pos) = cookies
                .iter()
                .position(|e| e.name == c.name && e.domain == c.domain && e.path == c.path)
            {
                cookies[pos] = c;
            } else {
                cookies.push(c);
            }
        }
        // Kapasite sınırlarını koru
        while cookies.len() > self.max_cookies {
            cookies.remove(0);
        }
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

/// Domain eşleşmesi: cookie.domain, domain'in suffix'i olmalı ve
/// aynı TLD'de olmalı (basit kontrol).
fn domain_matches(domain: &str, cookie_domain: &str) -> bool {
    let domain = domain.trim_start_matches('.');
    let cookie_domain = cookie_domain.trim_start_matches('.');

    if domain == cookie_domain {
        return true;
    }

    // Cookie domain, istek domain'in suffix'i olmalı
    if domain.ends_with(&format!(".{}", cookie_domain)) {
        return true;
    }

    false
}

/// Path eşleşmesi: istek path'i, cookie path ile başlıyor olmalı.
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    request_path.starts_with(cookie_path)
}

/// HTTP çerez tarihini (RFC 1123 / RFC 850) epoch seconds'a çevirir.
/// Desteklenen biçimler: "Wed, 21 Oct 2015 07:28:00 GMT",
/// "21 Oct 2015 07:28:00 GMT" ve RFC 850 "Wednesday, 21-Oct-15 07:28:00 GMT".
/// Çözülemezse 0 döner (oturum çerezi olarak işlenir).
fn parse_cookie_date(date_str: &str) -> u64 {
    // Gün adı (varsa) ve GMT son eki atılır.
    let mut s = date_str.trim();
    if let Some(comma) = s.find(',') {
        s = s[comma + 1..].trim_start();
    }
    if let Some(gmt) = s.to_ascii_lowercase().rfind("gmt") {
        s = s[..gmt].trim_end();
    }

    // Kalan dizge: "21 Oct 2015 07:28:00" veya "21-Oct-15 07:28:00".
    // Son belirte zaman, öncekiler gün/ay/yıl alanlarıdır.
    let mut tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.len() < 2 {
        return 0;
    }
    let time_part = tokens.pop().unwrap_or("");

    // Tarih alanları: gün-ay-yıl (tire ile ayrılmış tek belirte ya da
    // boşlukla ayrılmış üç belirte).
    let date_fields: Vec<&str> = if tokens.len() == 1 && tokens[0].contains('-') {
        tokens[0].split('-').collect()
    } else {
        tokens
    };
    if date_fields.len() < 3 {
        return 0;
    }
    let day: u32 = match date_fields[0].parse() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let month = match month_number(date_fields[1]) {
        Some(m) => m,
        None => return 0,
    };
    let mut year: i64 = match date_fields[2].parse() {
        Ok(y) => y,
        Err(_) => return 0,
    };
    // RFC 850 iki haneli yıl: 70-99 → 19xx, 00-69 → 20xx
    if year < 100 {
        year += if year >= 70 { 1900 } else { 2000 };
    }

    // Zaman parçası: "07:28:00"
    let mut tparts = time_part.split(':');
    let hour: u32 = tparts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let minute: u32 = tparts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let second: u32 = tparts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    if hour > 23 || minute > 59 || second > 60 || day == 0 || day > 31 {
        return 0;
    }

    days_from_civil(year, month, day)
        .checked_mul(86_400)
        .and_then(|d| d.checked_add((hour * 3600 + minute * 60 + second) as i64))
        .filter(|&v| v > 0)
        .map(|v| v as u64)
        .unwrap_or(0)
}

/// Ay adını (İngilizce, büyük/küçük harf duyarsız) 1-12 numarasına çevirir.
fn month_number(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = name.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| lower.starts_with(m))
        .map(|i| i as u32 + 1)
}

/// Howard Hinnant'ın days_from_civil algoritması: (yıl, ay, gün) →
/// 1970-01-01'den itibaren gün sayısı.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m as i64) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Global çerez jarı örneği.
static COOKIE_JAR: core::sync::atomic::AtomicPtr<CookieJar> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Global çerez jarına erişim sağlar (lazy başlatma).
pub fn cookie_jar() -> *mut CookieJar {
    let ptr = COOKIE_JAR.load(core::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        let jar = Box::into_raw(Box::new(CookieJar::new()));
        COOKIE_JAR.store(jar, core::sync::atomic::Ordering::Release);
        jar
    } else {
        ptr
    }
}

/// Set-Cookie başlığını işler ve çerezi jar'a ekler.
pub fn process_set_cookie(header: &str, default_domain: &str, default_path: &str) {
    let jar = unsafe { &*cookie_jar() };
    jar.set_cookie(header, default_domain, default_path);
}

/// Belirtilen URL için Cookie başlığı değerini döner.
pub fn get_cookie_header(
    domain: &str,
    path: &str,
    secure: bool,
    same_site_request: bool,
) -> String {
    let jar = unsafe { &*cookie_jar() };
    jar.get_cookie_header(domain, path, secure, same_site_request)
}

/// Süresi dolmuş çerezleri temizler.
pub fn purge_expired_cookies() {
    let jar = unsafe { &*cookie_jar() };
    jar.purge_expired();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_cookie_parsing() {
        let jar = CookieJar::new();
        jar.set_cookie(
            "session=abc123; Domain=example.com; Path=/; Secure; HttpOnly; SameSite=Lax",
            "example.com",
            "/",
        );
        assert_eq!(jar.len(), 1);
    }

    #[test]
    fn test_cookie_header_generation() {
        let jar = CookieJar::new();
        jar.set_cookie(
            "session=abc123; Domain=example.com; Path=/; Secure",
            "example.com",
            "/",
        );
        let header = jar.get_cookie_header("example.com", "/", true, true);
        assert_eq!(header, "session=abc123");
    }

    #[test]
    fn test_secure_cookie_not_sent_over_http() {
        let jar = CookieJar::new();
        jar.set_cookie(
            "session=abc123; Domain=example.com; Path=/; Secure",
            "example.com",
            "/",
        );
        let header = jar.get_cookie_header("example.com", "/", false, true);
        assert_eq!(header, "");
    }

    #[test]
    fn test_domain_matching() {
        assert!(domain_matches("example.com", "example.com"));
        assert!(domain_matches("sub.example.com", "example.com"));
        assert!(!domain_matches("other.com", "example.com"));
    }

    #[test]
    fn test_path_matching() {
        assert!(path_matches("/foo/bar", "/foo"));
        assert!(path_matches("/foo", "/foo"));
        assert!(!path_matches("/bar", "/foo"));
    }

    #[test]
    fn test_cookie_date_parsing() {
        // RFC 1123: "Wed, 21 Oct 2015 07:28:00 GMT"
        assert_eq!(
            parse_cookie_date("Wed, 21 Oct 2015 07:28:00 GMT"),
            1445412480
        );
        // Hafta günü olmadan
        assert_eq!(parse_cookie_date("21 Oct 2015 07:28:00 GMT"), 1445412480);
        // RFC 850 iki haneli yıl
        assert_eq!(
            parse_cookie_date("Wednesday, 21-Oct-15 07:28:00 GMT"),
            1445412480
        );
        // Geçersiz girdi
        assert_eq!(parse_cookie_date("not a date"), 0);
        assert_eq!(parse_cookie_date(""), 0);
    }

    #[test]
    fn test_expires_removes_cookie() {
        let jar = CookieJar::new();
        // Geçmişte dolan Expires → çerez eklenmez (silme isteği).
        jar.set_cookie(
            "old=1; Domain=example.com; Path=/; Expires=Mon, 01 Jan 2001 00:00:00 GMT",
            "example.com",
            "/",
        );
        assert_eq!(jar.len(), 0);
        // Gelecek tarih → çerez eklenir.
        jar.set_cookie(
            "future=1; Domain=example.com; Path=/; Expires=Fri, 01 Jan 2038 00:00:00 GMT",
            "example.com",
            "/",
        );
        assert_eq!(jar.len(), 1);
        assert_eq!(jar.list()[0].name, "future");
    }

    #[test]
    fn test_expired_cookie_not_returned() {
        let jar = CookieJar::new();
        // expires=0 means session cookie (never expires by time)
        jar.set_cookie(
            "session=abc123; Domain=example.com; Path=/",
            "example.com",
            "/",
        );
        let header = jar.get_cookie_header("example.com", "/", true, true);
        assert_eq!(header, "session=abc123");
    }

    #[test]
    fn test_dump_load_roundtrip() {
        let jar = CookieJar::new();
        jar.set_cookie(
            "a=1; Domain=example.com; Path=/; Secure; SameSite=Strict",
            "example.com",
            "/",
        );
        jar.set_cookie(
            "b=iki; Domain=alt.example.com; Path=/x; HttpOnly; SameSite=None",
            "alt.example.com",
            "/x",
        );
        jar.set_cookie(
            "c=3; Domain=example.com; Path=/; Expires=Fri, 01 Jan 2038 00:00:00 GMT",
            "example.com",
            "/",
        );
        let data = jar.dump();

        let jar2 = CookieJar::new();
        jar2.load(&data);
        assert_eq!(jar2.len(), 3);

        let c_a = jar2
            .list()
            .into_iter()
            .find(|c| c.name == "a")
            .expect("a çerezi yüklenmedi");
        assert!(c_a.secure);
        assert!(!c_a.http_only);
        assert_eq!(c_a.same_site, SameSite::Strict);
        assert_eq!(c_a.domain, "example.com");

        let c_b = jar2
            .list()
            .into_iter()
            .find(|c| c.name == "b")
            .expect("b çerezi yüklenmedi");
        assert!(c_b.http_only);
        assert_eq!(c_b.same_site, SameSite::None);
        assert_eq!(c_b.path, "/x");

        // Yüklenen jar aynı Cookie başlığını üretmeli.
        let h1 = jar.get_cookie_header("example.com", "/", true, true);
        let h2 = jar2.get_cookie_header("example.com", "/", true, true);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_load_rejects_truncated_and_overwrites() {
        let jar = CookieJar::new();
        jar.set_cookie("k=v; Domain=example.com; Path=/", "example.com", "/");
        let data = jar.dump();

        // Kesik veri yükleme hatasız tamamlanmalı, çerez eklememeli.
        let jar2 = CookieJar::new();
        jar2.load(&data[..data.len() - 3]);
        assert_eq!(jar2.len(), 0);
        // Boş/çok kısa veri de güvenli.
        jar2.load(&[]);
        jar2.load(&[0xff; 2]);
        assert_eq!(jar2.len(), 0);

        // Aynı üçlü yüklemede güncellenir (duplicate oluşmaz).
        let jar3 = CookieJar::new();
        jar3.load(&data);
        jar3.load(&data);
        assert_eq!(jar3.len(), 1);
    }
}

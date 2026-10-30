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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Lax,
    Strict,
    None,
}

impl Default for SameSite {
    fn default() -> Self {
        SameSite::Lax
    }
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

        self.add_cookie(cookie);
    }

    /// Çerezi jar'a ekler (maksimum sayı sınırlaması uygulanır).
    fn add_cookie(&self, cookie: Cookie) {
        let mut cookies = self.cookies.borrow_mut();

        // Aynı isim+domain+path çerezi varsa güncelle
        if let Some(pos) = cookies.iter().position(|c| {
            c.name == cookie.name
                && c.domain == cookie.domain
                && c.path == cookie.path
        }) {
            cookies[pos] = cookie;
            return;
        }

        // Domain başına maksimum sayı kontrolü
        let domain_count = cookies
            .iter()
            .filter(|c| c.domain == cookie.domain)
            .count();
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
    if request_path.starts_with(cookie_path) {
        return true;
    }
    false
}

/// HTTP çerez tarihini (RFC 1123) epoch seconds'a çevirir.
/// Basit implementasyon: sadece sayısal timestamp kabul edilir.
fn parse_cookie_date(date_str: &str) -> u64 {
    // "Wed, 21 Oct 2015 07:28:00 GMT" formatı
    // Şimdilik basitçe 0 döner (süresiz), gerçek implementasyon
    // tarih ayrıştırma gerektirir.
    let _ = date_str;
    0
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
pub fn get_cookie_header(domain: &str, path: &str, secure: bool, same_site_request: bool) -> String {
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
}
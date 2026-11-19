//! minOs HTTP istemcilerinin User-Agent dizesi (tek kaynak).
//!
//! Tarayıcı, paket yöneticisi ve HTTP/2 sürücüsü bu değeri paylaşır; böylece
//! sürüm bilgisi birden fazla yerde kopyalanmaz.
//!
//! Modern tarayıcılar `Mozilla/5.0 (...) Gecko/... Firefox/...` biçiminde bir
//! User-Agent gönderir. `minOs/1.5` gibi tanınmayan dize, bazı koruma
//! katmanlarında (Cloudflare bot koruması gibi) isteği "bot" sayıp JS-challenge
//! veya CAPTCHA ile karşılayabiliyor. Bu yüzden gerçek bir tarayıcı imzası
//! kullanılır ve sürüm bilgisi sona eklenir.
//!
//! Değiştirmek için tek satır yeterlidir.

/// minOs istemcilerinin gönderdiği User-Agent dizesi.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 minOs/2.5";

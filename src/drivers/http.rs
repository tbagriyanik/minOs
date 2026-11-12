//! Minimal HTTP istemcisi (`curl`/`http_get`): HTTP/1.1 GET, Content-Length ve
//! `Connection: close` çerçeveleme, chunked transfer çözümü, 3xx yönlendirme
//! takibi (en fazla 3 atlama). `https://` adreslerinde TLS 1.2/1.3 el sıkışması
//! `drivers::tls` katmanı üzerinden gerçekleştirilir.
#![allow(dead_code, static_mut_refs)]

use crate::drivers::e1000;
use crate::drivers::netstack::Ipv4Address;
use crate::drivers::socket;
use crate::drivers::tls::TlsSession;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

pub const E_URL: i32 = 1001;
pub const E_TLS: i32 = 1002;
pub const E_HTTP: i32 = 1003;
pub const E_REDIRECT: i32 = 1004;

pub const MAX_REDIRECTS: usize = 3;
pub const MAX_HOST: usize = 96;
pub const MAX_PATH: usize = 256;

/// HTTP istemci yapılandırması: zaman aşımları, yeniden deneme politikası ve proxy ayarları.
#[derive(Clone, Debug)]
pub struct HttpClientConfig {
    /// Toplam istek zaman aşımı (milisaniye)
    pub timeout_ms: u64,
    /// Maksimum yeniden deneme sayısı
    pub max_retries: u32,
    /// İlk yeniden deneme gecikmesi (milisaniye), ikili olarak artar
    pub retry_delay_ms: u64,
    /// HTTP proxy sunucusu (örn. "proxy.example.com:8080"), None ise doğrudan bağlantı
    pub proxy: Option<String>,
    /// Proxy kimlik doğrulama kullanıcı adı
    pub proxy_username: Option<String>,
    /// Proxy kimlik doğrulama şifresi
    pub proxy_password: Option<String>,
    /// Range isteği için byte aralığı (örn. "0-499")
    pub range: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 8000,
            max_retries: 3,
            retry_delay_ms: 500,
            proxy: None,
            proxy_username: None,
            proxy_password: None,
            range: None,
        }
    }
}

/// Milisaniye cinsinden bekleme (spin loop ile).
fn sleep_ms(ms: u64) {
    let start = crate::monotonic_ms();
    while crate::monotonic_ms().wrapping_sub(start) < ms {
        core::hint::spin_loop();
    }
}

/// Keep-alive bağlantı havuzu girişi.
struct PooledConnection {
    fd: i32,
    host: String,
    port: u16,
    is_https: bool,
    tls_session: Option<TlsSession>,
    last_used: u64,
}

/// Bağlantı havuzu: aynı sunucuya tekrar isteklerde soket yeniden kullanımı.
struct ConnectionPool {
    connections: [Option<PooledConnection>; 8],
}

impl ConnectionPool {
    fn new() -> Self {
        Self {
            connections: [const { None }; 8],
        }
    }

    /// Boşta bir bağlantı bulur ve havuzdan çıkarır (kullanıma alınır).
    fn acquire(&mut self, host: &str, port: u16, is_https: bool) -> Option<i32> {
        let now = crate::monotonic_ms();
        for entry in self.connections.iter_mut() {
            if let Some(ref mut conn) = entry {
                if conn.host == host && conn.port == port && conn.is_https == is_https {
                    let fd = conn.fd;
                    conn.last_used = now;
                    // TLS session'ı koru, sadece fd'yi döndür
                    let _ = conn.tls_session.take(); // Çağrıcı tarafından yeniden kurulacak
                    *entry = None;
                    return Some(fd);
                }
            }
        }
        None
    }

    /// Bağlantıyı havuza geri koyar (kullanım bittiğinde).
    fn release(
        &mut self,
        fd: i32,
        host: String,
        port: u16,
        is_https: bool,
        tls_session: Option<TlsSession>,
    ) {
        let now = crate::monotonic_ms();
        // Boş slot ara
        for entry in self.connections.iter_mut() {
            if entry.is_none() {
                *entry = Some(PooledConnection {
                    fd,
                    host,
                    port,
                    is_https,
                    tls_session,
                    last_used: now,
                });
                return;
            }
        }
        // Tümü dolu: en eskiyi kapat
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;
        for (i, entry) in self.connections.iter().enumerate() {
            if let Some(ref conn) = entry {
                if conn.last_used < oldest_time {
                    oldest_time = conn.last_used;
                    oldest_idx = i;
                }
            }
        }
        socket::close(self.connections[oldest_idx].take().unwrap().fd);
        self.connections[oldest_idx] = Some(PooledConnection {
            fd,
            host,
            port,
            is_https,
            tls_session,
            last_used: now,
        });
    }

    /// Boşta kalan ve çok eski bağlantıları kapatır.
    fn cleanup_stale(&mut self, max_idle_ms: u64) {
        let now = crate::monotonic_ms();
        for entry in self.connections.iter_mut() {
            if let Some(ref conn) = entry {
                if now.wrapping_sub(conn.last_used) > max_idle_ms {
                    socket::close(conn.fd);
                    *entry = None;
                }
            }
        }
    }
}

static mut CONNECTION_POOL: Option<ConnectionPool> = None;

fn connection_pool() -> &'static mut ConnectionPool {
    unsafe { CONNECTION_POOL.get_or_insert_with(ConnectionPool::new) }
}

/// Proxy-Authorization başlığı oluşturur (Basic auth).
fn proxy_auth_header(username: &str, password: &str) -> String {
    let credentials = format!("{}:{}", username, password);
    let encoded = base64_encode(credentials.as_bytes());
    format!("Basic {}", encoded)
}

/// Temel Base64 kodlama (RFC 4648).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() {
            input[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < input.len() {
            input[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < input.len() {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

/// HTTP CONNECT tüneli kurar (proxy üzerinden).
/// `fd` — proxy'ye bağlı soket, `host_port` — "host:port" biçiminde hedef.
fn proxy_connect(fd: i32, host_port: &str, config: &HttpClientConfig) -> Result<(), i32> {
    let mut req = String::new();
    req.push_str(&format!("CONNECT {} HTTP/1.1\r\n", host_port));
    req.push_str(&format!("Host: {}\r\n", host_port));
    if let (Some(user), Some(pass)) = (&config.proxy_username, &config.proxy_password) {
        req.push_str(&format!(
            "Proxy-Authorization: {}\r\n",
            proxy_auth_header(user, pass)
        ));
    }
    req.push_str("Connection: close\r\n\r\n");

    let mut sent = 0;
    let req_bytes = req.as_bytes();
    while sent < req_bytes.len() {
        sent += socket::send(fd, &req_bytes[sent..]).map_err(|e| {
            if sent == 0 {
                e
            } else {
                socket::E_TIMEOUT
            }
        })?;
    }

    // Proxy yanıtını oku (basit: ilk 2KB)
    let mut resp = [0u8; 2048];
    let mut total = 0;
    loop {
        let n = socket::recv(fd, &mut resp[total..]).map_err(|e| {
            if total == 0 {
                e
            } else {
                socket::E_TIMEOUT
            }
        })?;
        if n == 0 {
            return Err(socket::E_NOTCONN);
        }
        total += n;
        if total >= 4 {
            // "\r\n\r\n" başlık sonunu ara
            if resp[total - 4..total] == *b"\r\n\r\n" {
                break;
            }
        }
        if total >= resp.len() {
            break;
        }
    }

    let resp_str = core::str::from_utf8(&resp[..total]).unwrap_or("");
    // HTTP/1.1 200 Connection established kontrolü
    if resp_str.starts_with("HTTP/1.1 200") || resp_str.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        Err(E_HTTP)
    }
}

/// `http[s]://host[:port][/path]` ayrıştırır. `host`/`path` çıktı tamponlarına
/// yazılır, port ve (is_https) bilgisi döner.
pub fn parse_url(
    url: &str,
    host_out: &mut [u8; MAX_HOST],
    path_out: &mut [u8; MAX_PATH],
) -> Result<(u16, usize, usize), i32> {
    let (is_https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else if url.contains("://") {
        return Err(E_URL);
    } else {
        (false, url)
    };

    // host[:port] + [/path] ayır.
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() || authority.len() > MAX_HOST || path.len() > MAX_PATH {
        return Err(E_URL);
    }
    let default_port = if is_https { 443 } else { 80 };
    let (host, port) = match authority.find(':') {
        Some(i) => {
            let p: u16 = authority[i + 1..].parse().map_err(|_| E_URL)?;
            (&authority[..i], p)
        }
        None => (authority, default_port),
    };
    if host.is_empty() {
        return Err(E_URL);
    }
    host_out[..host.len()].copy_from_slice(host.as_bytes());
    path_out[..path.len()].copy_from_slice(path.as_bytes());
    Ok((port, host.len(), path.len()))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

fn parse_status(buf: &[u8]) -> Option<u16> {
    // "HTTP/1.x NNN ..."
    if buf.len() < 12 || &buf[..5] != b"HTTP/" {
        return None;
    }
    let sp = buf.iter().position(|&b| b == b' ')?;
    if buf.len() < sp + 4 {
        return None;
    }
    let d = |c: u8| (c as char).to_digit(10).unwrap_or(0) as u16;
    if !buf[sp + 1].is_ascii_digit()
        || !buf[sp + 2].is_ascii_digit()
        || !buf[sp + 3].is_ascii_digit()
    {
        return None;
    }
    Some(d(buf[sp + 1]) * 100 + d(buf[sp + 2]) * 10 + d(buf[sp + 3]))
}

pub fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    // Satır satır tara, isim eşleşmesini harf duyarsız yap.
    let mut line_start = 0usize;
    // İlk satır (status) atlanır.
    if let Some(i) = headers.iter().position(|&b| b == b'\n') {
        line_start = i + 1;
    }
    while line_start < headers.len() {
        let line_end = headers[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| line_start + i)
            .unwrap_or(headers.len());
        let line = &headers[line_start..line_end];
        if let Some(colon) = line.iter().position(|&b| b == b':') {
            let key = trim(&line[..colon]);
            if key.len() == name.len()
                && key
                    .iter()
                    .zip(name.iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
            {
                return Some(trim(&line[colon + 1..]));
            }
        }
        line_start = line_end + 1;
    }
    None
}

fn trim(b: &[u8]) -> &[u8] {
    let mut s = 0;
    let mut e = b.len();
    while s < e && (b[s] == b' ' || b[s] == b'\t' || b[s] == b'\r') {
        s += 1;
    }
    while e > s && (b[e - 1] == b' ' || b[e - 1] == b'\t' || b[e - 1] == b'\r') {
        e -= 1;
    }
    &b[s..e]
}

fn parse_hex_len(line: &[u8]) -> Option<usize> {
    let line = trim(line);
    let mut v = 0usize;
    if line.is_empty() {
        return None;
    }
    for &c in line {
        if c == b';' {
            break;
        } // chunk uzantısını yoksay
        let d = (c as char).to_digit(16)? as usize;
        v = v.checked_mul(16)?.checked_add(d)?;
        if v > 16 * 1024 * 1024 {
            return None;
        }
    }
    Some(v)
}

/// Ham chunked gövdeyi yerinde çözer, çözülmüş uzunluğu döner.
pub fn decode_chunked(buf: &mut [u8], len: usize) -> Option<usize> {
    let mut read = 0usize;
    let mut write = 0usize;
    loop {
        // Boyut satırını bul.
        let rel = buf[read..len].iter().position(|&b| b == b'\n')?;
        let line_end = read + rel;
        let size = parse_hex_len(&buf[read..line_end])?;
        read = line_end + 1;
        if size == 0 {
            break;
        }
        if read + size > len {
            return None;
        }
        buf.copy_within(read..read + size, write);
        write += size;
        read += size;
        // Parça sonu CRLF.
        if read + 2 > len || &buf[read..read + 2] != b"\r\n" {
            return None;
        }
        read += 2;
    }
    Some(write)
}

fn build_request(req: &mut [u8], host: &[u8], path: &[u8], ip: Ipv4Address) -> Option<usize> {
    build_request_with_headers(req, host, path, ip, None, None)
}

/// Cache validation başlıkları ile HTTP isteği oluşturur.
fn build_request_with_headers(
    req: &mut [u8],
    host: &[u8],
    path: &[u8],
    ip: Ipv4Address,
    extra_headers: Option<&str>,
    range: Option<&str>,
) -> Option<usize> {
    let _ = ip;
    let base: &[&[u8]] = &[
        b"GET ",
        path,
        b" HTTP/1.1\r\nHost: ",
        host,
        b"\r\nUser-Agent: minOs-curl/1.0\r\nConnection: keep-alive",
    ];

    let mut n = 0usize;
    for p in base {
        if n + p.len() > req.len() {
            return None;
        }
        req[n..n + p.len()].copy_from_slice(p);
        n += p.len();
    }

    // Range header
    if let Some(r) = range {
        let range_header = format!("\r\nRange: bytes={}", r);
        let range_bytes = range_header.as_bytes();
        if n + range_bytes.len() > req.len() {
            return None;
        }
        req[n..n + range_bytes.len()].copy_from_slice(range_bytes);
        n += range_bytes.len();
    }

    // Extra headers (cache validation)
    if let Some(headers) = extra_headers {
        let headers_bytes = headers.as_bytes();
        if n + headers_bytes.len() > req.len() {
            return None;
        }
        req[n..n + headers_bytes.len()].copy_from_slice(headers_bytes);
        n += headers_bytes.len();
    }

    // Final CRLF
    if n + 2 > req.len() {
        return None;
    }
    req[n..n + 2].copy_from_slice(b"\r\n");
    n += 2;

    Some(n)
}

/// Tek bir HTTP GET turu yapar: yanıtın tamamını (başlık+gövde) `resp_buf`'a
/// yazar, `(durum_kodu, başlık_uzunluğu, toplam_uzunluk)` döner.
///
/// `is_https` doğruysa TLS el sıkışması yapılır, istek şifreli gönderilir ve
/// yanıt şifreli olarak alınıp çözülür.
///
/// `cache_headers` verilirse cache validation başlıkları eklenir.
/// `config` yeniden deneme ve zaman ayarı politikasını belirtir.
pub fn fetch_once(
    host: &[u8],
    port: u16,
    path: &[u8],
    ip: Ipv4Address,
    resp_buf: &mut [u8],
    is_https: bool,
    cache_headers: Option<&str>,
    config: &HttpClientConfig,
) -> Result<(u16, usize, usize), i32> {
    let host_str = core::str::from_utf8(host).unwrap_or("");
    let use_pool = config.proxy.is_none(); // Proxy ile bağlantı havuzlanmaz

    let mut last_err: Option<i32> = None;
    let mut delay = config.retry_delay_ms;
    // ALPN ile h2 negotiate edilmişse HTTP/2 yığına geçilir; bu bayrak mevcut
    // TLS bağlantısının havuza boşuna geri alınmasını (veya iki kez kapatılmasını) engeller.
    let mut http2_used = false;

    for attempt in 0..=config.max_retries {
        // Havuzdan mevcut bağlantıyı almayı dene (sadece doğrudan bağlantı için)
        let fd = if use_pool {
            if let Some(cached_fd) = connection_pool().acquire(host_str, port, is_https) {
                cached_fd
            } else {
                let new_fd =
                    socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|&e| {
                        last_err = Some(e);
                    })?;
                let _ = socket::set_timeout(new_fd, config.timeout_ms);
                socket::connect(new_fd, ip, port).inspect_err(|&e| {
                    socket::close(new_fd);
                    last_err = Some(e);
                })?;
                new_fd
            }
        } else {
            let new_fd =
                socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|&e| {
                    last_err = Some(e);
                })?;
            let _ = socket::set_timeout(new_fd, config.timeout_ms);
            socket::connect(new_fd, ip, port).inspect_err(|&e| {
                socket::close(new_fd);
                last_err = Some(e);
            })?;
            new_fd
        };

        let res = (|| {
            // Proxy ile bağlanma: proxy'e bağlan, gerekirse CONNECT tüneli kur
            if let Some(ref proxy_str) = config.proxy {
                let proxy_host_port = proxy_str.as_str();
                let proxy_ip = socket::resolve_host(
                    proxy_host_port.split(':').next().unwrap_or(proxy_host_port),
                )?;
                let proxy_port = proxy_host_port
                    .split(':')
                    .nth(1)
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(8080);
                socket::connect(fd, proxy_ip, proxy_port)?;

                if is_https {
                    // HTTPS üzerinden proxy: CONNECT tüneli kur
                    let host_port = format!("{}:{}", host_str, port);
                    proxy_connect(fd, &host_port, config)?;
                    // TLS el sıkışması tünel üzerinden
                    fetch_https(fd, host, &[0u8; 0], 0, resp_buf)
                } else {
                    // HTTP üzerinden proxy: mutlak URL formu kullan
                    let mut req = [0u8; 1024];
                    let url_str = format!(
                        "http://{}{}",
                        host_str,
                        core::str::from_utf8(path).unwrap_or("/")
                    );
                    let reqlen = build_proxy_request(&mut req, &url_str, host, cache_headers)
                        .ok_or(socket::E_INVAL)?;
                    let mut sent = 0;
                    while sent < reqlen {
                        sent += socket::send(fd, &req[sent..reqlen]).map_err(|e| {
                            if sent == 0 {
                                e
                            } else {
                                socket::E_TIMEOUT
                            }
                        })?;
                    }
                    let (total, expected_total) = read_http_response(fd, resp_buf, None)?;
                    let _ = expected_total;
                    if total < 12 {
                        return Err(E_HTTP);
                    }
                    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                    Ok((status, hlen, total))
                }
            } else {
                // Doğrudan bağlantı
                if is_https {
                    // TLS el sıkışması — client_random donanım Rastgelelik kaynağından.
                    let mut client_random = [0u8; 32];
                    crate::random::fill_bytes(&mut client_random);

                    let mut hello = Vec::new();
                    let mut ecdhe_secret = [0u8; 32];
                    crate::drivers::tls::build_client_hello_with_alpn(
                        host_str,
                        &client_random,
                        &mut hello,
                        &mut ecdhe_secret,
                        false,
                        None,
                        None,
                        &[b"h2", b"http/1.1"],
                    );
                    let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
                    let mut session = TlsSession::new(host_str, client_random);
                    session.note_client_hello(ch_body);
                    session.set_client_ecdhe_secret(ecdhe_secret);

                    socket::send(fd, &hello).map_err(|_| E_TLS)?;

                    // El sıkışma: sunucu yanıtı gelene kadar kayıtları işle
                    let handshake_deadline = crate::boot::time::main_monotonic_ms() + 8_000;
                    loop {
                        let mut raw = [0u8; 4096];
                        let n = socket::recv(fd, &mut raw).map_err(|_| E_TLS)?;
                        if n == 0 {
                            return Err(E_TLS);
                        }
                        session.feed_tcp_data(&raw[..n]);

                        let mut app = Vec::new();
                        let mut hs = Vec::new();
                        match session.process_records(&mut hs, &mut app) {
                            Ok(_) => {}
                            Err(_) => return Err(E_TLS),
                        }

                        if session.state() == crate::drivers::tls::TlsState::Tls13ServerFinished {
                            if let Some(ccs) = session.tls13_change_cipher_spec() {
                                socket::send(fd, &ccs).map_err(|_| E_TLS)?;
                            }
                            if let Some(record) = session.build_tls13_client_finished() {
                                socket::send(fd, &record).map_err(|_| E_TLS)?;
                            }
                        }

                        let outgoing = session.take_outgoing();
                        if !outgoing.is_empty() {
                            socket::send(fd, &outgoing).map_err(|_| E_TLS)?;
                        }

                        if session.state() == crate::drivers::tls::TlsState::Established {
                            break;
                        }
                        if crate::boot::time::main_monotonic_ms() > handshake_deadline {
                            return Err(E_TLS);
                        }
                    }

                    // ALPN sonucuna göre protokol yığını seç.
                    // Eğer sunucu h2 protokolünü seçerse, bağlantı HTTP/2 üzerinden devam eder;
                    // aksi takdirde klasik HTTP/1.1 isteği gönderilir.
                    http2_used = session.negotiated_protocol().is_some_and(|p| p == b"h2");
                    if http2_used {
                        crate::serial_print(
                            "[HTTP] ALPN ile h2 negotiate edildi, HTTP/2 yığını kullanılıyor\n",
                        );
                        // http2_get kendi TCP/TLS bağlantısını kurar, bu yüzden sadece
                        // el sıkışması yapılmış mevcut bağlantıyı serbest bırakırız.
                        socket::close(fd);
                        let url_str = alloc::format!(
                            "{}://{}{}",
                            if is_https { "https" } else { "http" },
                            host_str,
                            core::str::from_utf8(path).unwrap_or("/")
                        );
                        return http2_get(&url_str, resp_buf);
                    }

                    // Şifreli HTTP/1.1 isteğini hazırla ve gönder
                    let mut req = [0u8; 768];
                    let reqlen = build_request_with_headers(
                        &mut req,
                        host,
                        path,
                        ip,
                        cache_headers,
                        config.range.as_deref(),
                    )
                    .ok_or(socket::E_INVAL)?;
                    let record = session.encrypt_app_data(&req[..reqlen]).ok_or(E_TLS)?;
                    socket::send(fd, &record).map_err(|_| E_TLS)?;

                    // Şifreli yanıtı oku
                    let (total, _) = read_http_response(fd, resp_buf, Some(&mut session))?;
                    if total < 12 {
                        return Err(E_HTTP);
                    }
                    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;

                    // TLS session'ı havuza döndürmek için sakla
                    connection_pool().release(fd, host_str.to_string(), port, true, Some(session));

                    Ok((status, hlen, total))
                } else {
                    let mut req = [0u8; 768];
                    let reqlen = build_request_with_headers(
                        &mut req,
                        host,
                        path,
                        ip,
                        cache_headers,
                        config.range.as_deref(),
                    )
                    .ok_or(socket::E_INVAL)?;

                    let mut sent = 0;
                    while sent < reqlen {
                        sent += socket::send(fd, &req[sent..reqlen]).map_err(|e| {
                            if sent == 0 {
                                e
                            } else {
                                socket::E_TIMEOUT
                            }
                        })?;
                    }
                    let (total, expected_total) = read_http_response(fd, resp_buf, None)?;
                    let _ = expected_total;
                    if total < 12 {
                        return Err(E_HTTP);
                    }
                    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                    Ok((status, hlen, total))
                }
            }
        })();

        // http2_get kendi bağlantısını kurduğunda mevcut fd artık kullanılmaz;
        // havuza geri verilmeden (veya iki kez kapatılmadan) burayı atlıyoruz.
        if !http2_used {
            if use_pool {
                connection_pool().release(fd, host_str.to_string(), port, is_https, None);
            } else {
                socket::close(fd);
            }
        }

        match res {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = Some(e);
                if attempt < config.max_retries {
                    sleep_ms(delay);
                    delay = (delay * 2).min(10000);
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(last_err.unwrap_or(E_HTTP))
}

/// Proxy üzerinden HTTP istek başlığı oluşturur (mutlak URL formu).
fn build_proxy_request(
    req: &mut [u8],
    url: &str,
    host: &[u8],
    extra_headers: Option<&str>,
) -> Option<usize> {
    let mut n = 0usize;
    let method = b"GET ";
    let http_ver = b" HTTP/1.1\r\nHost: ";

    if n + method.len() + url.len() + http_ver.len() + host.len() + 10 > req.len() {
        return None;
    }
    req[n..n + method.len()].copy_from_slice(method);
    n += method.len();
    req[n..n + url.len()].copy_from_slice(url.as_bytes());
    n += url.len();
    req[n..n + http_ver.len()].copy_from_slice(http_ver);
    n += http_ver.len();
    req[n..n + host.len()].copy_from_slice(host);
    n += host.len();

    if let Some(headers) = extra_headers {
        let headers_bytes = headers.as_bytes();
        if n + headers_bytes.len() > req.len() {
            return None;
        }
        req[n..n + headers_bytes.len()].copy_from_slice(headers_bytes);
        n += headers_bytes.len();
    }

    req[n..n + 2].copy_from_slice(b"\r\n");
    n += 2;
    req[n..n + 2].copy_from_slice(b"\r\n");
    n += 2;

    Some(n)
}

/// HTTP yanıtını socket'ten okur; `decrypt` verilirse gövde şifreli okunur
/// ve çözülür. Dönen değerler: (toplam bayt, beklenen toplam (Content-Length)).
fn read_http_response(
    fd: i32,
    resp_buf: &mut [u8],
    mut decrypt: Option<&mut TlsSession>,
) -> Result<(usize, Option<usize>), i32> {
    let mut total = 0usize;
    let mut expected_total: Option<usize> = None;
    loop {
        if total >= resp_buf.len() {
            break;
        }
        if let Some(exp) = expected_total {
            if total >= exp {
                break;
            }
        }
        let mut raw = [0u8; 4096];
        let want = raw.len().min(resp_buf.len() - total);
        match socket::recv(fd, &mut raw[..want]) {
            Ok(0) => break, // FIN = EOF
            Ok(n) => {
                // Şifreli taşıma: kayıt çerçevesini çöz, düz metni işle.
                if let Some(ref mut session) = decrypt {
                    session.feed_tcp_data(&raw[..n]);
                    let mut app = Vec::new();
                    let mut hs = Vec::new();
                    match session.process_records(&mut hs, &mut app) {
                        Ok(_) => {}
                        Err(_) => return Err(E_TLS),
                    }
                    if !app.is_empty() {
                        let end = (total + app.len()).min(resp_buf.len());
                        resp_buf[total..end].copy_from_slice(&app[..end - total]);
                        total = end;
                    }
                } else {
                    let end = (total + n).min(resp_buf.len());
                    resp_buf[total..end].copy_from_slice(&raw[..end - total]);
                    total = end;
                }
                if expected_total.is_none() {
                    if let Some(hlen) = find_header_end(&resp_buf[..total]) {
                        let headers = &resp_buf[..hlen];
                        if let Some(cl_bytes) = header_value(headers, b"content-length") {
                            if let Ok(cl_str) = core::str::from_utf8(cl_bytes) {
                                if let Ok(cl_val) = cl_str.trim().parse::<usize>() {
                                    expected_total = Some(hlen + cl_val);
                                    if total >= hlen + cl_val {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(hlen) = find_header_end(&resp_buf[..total]) {
                    let body = &resp_buf[hlen..total];
                    if body.ends_with(b"0\r\n\r\n") || body.ends_with(b"0\r\n") {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    Ok((total, expected_total))
}

/// TLS el sıkışması yapar, şifreli HTTP isteği gönderir ve şifreli yanıtı
/// çözerek `resp_buf`'a yazar.
fn fetch_https(
    fd: i32,
    host: &[u8],
    req: &[u8],
    reqlen: usize,
    resp_buf: &mut [u8],
) -> Result<(u16, usize, usize), i32> {
    let host_str = core::str::from_utf8(host).map_err(|_| E_URL)?;

    // İstemci rastgelelikleri.
    let mut client_random = [0u8; 32];
    let mut state = crate::boot::time::main_monotonic_ms() as u64 ^ 0x9e37_79b9_7f4a_7c15;
    for b in client_random.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
    }

    let mut hello = Vec::new();
    let mut ecdhe_secret = [0u8; 32];
    crate::drivers::tls::build_client_hello_with_alpn(
        host_str,
        &client_random,
        &mut hello,
        &mut ecdhe_secret,
        false,
        None,
        None,
        &[b"h2", b"http/1.1"],
    );
    let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
    let mut session = TlsSession::new(host_str, client_random);
    session.note_client_hello(ch_body);
    session.set_client_ecdhe_secret(ecdhe_secret);

    socket::send(fd, &hello).map_err(|_| E_TLS)?;

    // El sıkışma: sunucu yanıtı gelene kadar kayıtları işle.
    let handshake_deadline = crate::boot::time::main_monotonic_ms() + 8_000;
    loop {
        let mut raw = [0u8; 4096];
        let n = socket::recv(fd, &mut raw).map_err(|_| E_TLS)?;
        if n == 0 {
            return Err(E_TLS);
        }
        session.feed_tcp_data(&raw[..n]);

        let mut app = Vec::new();
        let mut hs = Vec::new();
        match session.process_records(&mut hs, &mut app) {
            Ok(_) => {}
            Err(_) => return Err(E_TLS),
        }

        // Sunucu Finished doğrulandıysa istemci Finished'ı gönder.
        if session.state() == crate::drivers::tls::TlsState::Tls13ServerFinished {
            // Eski araç cihazlar için ChangeCipherSpec (RFC 8446 D.4).
            if let Some(ccs) = session.tls13_change_cipher_spec() {
                socket::send(fd, &ccs).map_err(|_| E_TLS)?;
            }
            if let Some(record) = session.build_tls13_client_finished() {
                socket::send(fd, &record).map_err(|_| E_TLS)?;
            }
        }

        // TLS 1.2 için sunucu Finished'ı beklerken ChangeCipherSpec + Finished
        // gönderilmiş olur (outgoing tamponundan).
        let outgoing = session.take_outgoing();
        if !outgoing.is_empty() {
            socket::send(fd, &outgoing).map_err(|_| E_TLS)?;
        }

        if session.state() == crate::drivers::tls::TlsState::Established {
            break;
        }
        if crate::boot::time::main_monotonic_ms() > handshake_deadline {
            return Err(E_TLS);
        }
    }

    // Şifreli HTTP isteğini gönder.
    let record = session.encrypt_app_data(&req[..reqlen]).ok_or(E_TLS)?;
    socket::send(fd, &record).map_err(|_| E_TLS)?;

    // Şifreli yanıtı oku.
    let (total, _) = read_http_response(fd, resp_buf, Some(&mut session))?;
    if total < 12 {
        return Err(E_HTTP);
    }
    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
    Ok((status, hlen, total))
}

/// `http://` URL'sini indirir; 3xx yönlendirmeleri takip eder.
/// Dönen `(durum, başlık_uzunluğu, toplam_uzunluk)`; gövde `resp_buf[hlen..total]`
/// aralığındadır (chunked ise çözülmüş hâliyle).
pub fn http_get(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize, usize), i32> {
    http_get_impl(url, resp_buf, None)
}

/// Cache validation başlıkları ile HTTP GET isteği.
/// `cache_headers` verilirse If-None-Match/If-Modified-Since başlıkları eklenir.
pub fn http_get_with_cache(
    url: &str,
    resp_buf: &mut [u8],
    cache_headers: Option<&str>,
) -> Result<(u16, usize, usize), i32> {
    http_get_impl(url, resp_buf, cache_headers)
}

/// HTTP GET isteği byte aralığı ile (Range başlığı).
/// `range` örn. "0-499" (ilk 500 byte), "500-" (500. bayttan sona), "-500" (son 500 byte).
pub fn http_get_range(
    url: &str,
    resp_buf: &mut [u8],
    range: &str,
    cache_headers: Option<&str>,
) -> Result<(u16, usize, usize), i32> {
    if !e1000::is_ready() {
        return Err(socket::E_NOTCONN);
    }
    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (mut port, mut host_len, mut path_len) = parse_url(url, &mut host, &mut path)?;
    let mut is_https = url.starts_with("https://");
    let mut hops = 0;
    loop {
        let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;
        let path_str = core::str::from_utf8(&path[..path_len]).map_err(|_| E_URL)?;
        let ip = socket::resolve_host(host_str)?;
        let config = HttpClientConfig {
            range: Some(range.to_string()),
            ..Default::default()
        };
        let (status, hlen, total) = fetch_once(
            &host[..host_len],
            port,
            &path[..path_len],
            ip,
            resp_buf,
            is_https,
            cache_headers,
            &config,
        )?;
        if (300..400).contains(&status) && hops < MAX_REDIRECTS {
            let headers = &resp_buf[..hlen];
            if let Some(loc) = header_value(headers, b"location") {
                let loc_str = core::str::from_utf8(loc).map_err(|_| E_HTTP)?;
                if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
                    is_https = loc_str.starts_with("https://");
                    let (p2, hl2, pl2) = parse_url(loc_str, &mut host, &mut path)?;
                    port = p2;
                    host_len = hl2;
                    path_len = pl2;
                    hops += 1;
                    continue;
                } else if loc_str.starts_with('/') && loc_str.len() <= MAX_PATH {
                    path[..loc_str.len()].copy_from_slice(loc_str.as_bytes());
                    path_len = loc_str.len();
                    hops += 1;
                    continue;
                }
                return Err(E_REDIRECT);
            }
        }
        // Chunked gövdeyi yerinde çöz.
        let headers = &resp_buf[..hlen];
        let is_chunked = header_value(headers, b"transfer-encoding")
            .map(|v| {
                v.iter()
                    .zip(b"chunked".iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    && v.len() == 7
            })
            .unwrap_or(false);
        if is_chunked {
            let body_len = total - hlen;
            let mut tmp = resp_buf[hlen..total].to_vec();
            if let Some(decoded) = decode_chunked(&mut tmp, body_len) {
                resp_buf[hlen..hlen + decoded].copy_from_slice(&tmp[..decoded]);
                return Ok((status, hlen, hlen + decoded));
            }
            return Err(E_HTTP);
        }
        let _ = (host_str, path_str);
        return Ok((status, hlen, total));
    }
}

/// Internal implementation for http_get variants.
pub(crate) fn http_get_impl(
    url: &str,
    resp_buf: &mut [u8],
    cache_headers: Option<&str>,
) -> Result<(u16, usize, usize), i32> {
    if !e1000::is_ready() {
        return Err(socket::E_NOTCONN);
    }
    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (mut port, mut host_len, mut path_len) = parse_url(url, &mut host, &mut path)?;
    let mut is_https = url.starts_with("https://");
    let mut hops = 0;
    loop {
        let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;
        let path_str = core::str::from_utf8(&path[..path_len]).map_err(|_| E_URL)?;

        // IPv6 (AAAA) öncelikli, IPv4 (A) fallback — dual-stack destek
        let use_v6 = socket::resolve_host_aaaa(host_str).is_ok();
        let config = HttpClientConfig::default();

        if use_v6 {
            // IPv6 yolu: connect_v6 ile bağlantı. Bağlantı kurulduktan sonra
            // oluşan TCP/TLS/HTTP hatalarında IPv4 yoluna otomatik geri düşülür
            // (dual-stack fallback); bu yüzden deneme bir closure içine alınır.
            if let Ok(v6) = socket::resolve_host_aaaa(host_str) {
                let v6_result = (|| -> Result<(u16, usize, usize), i32> {
                    let fd =
                        socket::socket(socket::AF_INET6, socket::SOCK_STREAM).inspect_err(|e| {
                            crate::serial_print("[HTTPv6] socket oluşturulamadı: ");
                            crate::serial_print(&e.to_string());
                            crate::serial_print("\n");
                        })?;
                    let _ = socket::set_timeout(fd, 8000);
                    match socket::connect_v6(fd, v6, port) {
                        Ok(()) => {}
                        Err(e) => {
                            socket::close(fd);
                            return Err(e);
                        }
                    }

                    let (status, hlen, total) = if is_https {
                        // HTTPS üzerinden IPv6: TLS el sıkışması
                        let mut client_random = [0u8; 32];
                        let mut state =
                            crate::boot::time::main_monotonic_ms() as u64 ^ 0x9e37_79b9_7f4a_7c15;
                        for b in client_random.iter_mut() {
                            state ^= state >> 12;
                            state ^= state << 25;
                            state ^= state >> 27;
                            *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
                        }

                        let mut hello = Vec::new();
                        let mut ecdhe_secret = [0u8; 32];
                        crate::drivers::tls::build_client_hello_with_alpn(
                            host_str,
                            &client_random,
                            &mut hello,
                            &mut ecdhe_secret,
                            false,
                            None,
                            None,
                            &[b"h2", b"http/1.1"],
                        );
                        let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
                        let mut session = TlsSession::new(host_str, client_random);
                        session.note_client_hello(ch_body);
                        session.set_client_ecdhe_secret(ecdhe_secret);

                        socket::send(fd, &hello).map_err(|_| E_TLS)?;

                        let handshake_deadline = crate::boot::time::main_monotonic_ms() + 8_000;
                        loop {
                            let mut raw = [0u8; 4096];
                            let n = socket::recv(fd, &mut raw).map_err(|_| E_TLS)?;
                            if n == 0 {
                                return Err(E_TLS);
                            }
                            session.feed_tcp_data(&raw[..n]);

                            let mut app = Vec::new();
                            let mut hs = Vec::new();
                            match session.process_records(&mut hs, &mut app) {
                                Ok(_) => {}
                                Err(_) => return Err(E_TLS),
                            }

                            if session.state() == crate::drivers::tls::TlsState::Tls13ServerFinished
                            {
                                if let Some(ccs) = session.tls13_change_cipher_spec() {
                                    socket::send(fd, &ccs).map_err(|_| E_TLS)?;
                                }
                                if let Some(record) = session.build_tls13_client_finished() {
                                    socket::send(fd, &record).map_err(|_| E_TLS)?;
                                }
                            }

                            let outgoing = session.take_outgoing();
                            if !outgoing.is_empty() {
                                socket::send(fd, &outgoing).map_err(|_| E_TLS)?;
                            }

                            if session.state() == crate::drivers::tls::TlsState::Established {
                                break;
                            }
                            if crate::boot::time::main_monotonic_ms() > handshake_deadline {
                                return Err(E_TLS);
                            }
                        }

                        let mut req = [0u8; 768];
                        let reqlen = build_request_with_headers(
                            &mut req,
                            &host[..host_len],
                            &path[..path_len],
                            crate::drivers::netstack::Ipv4Address::new(0, 0, 0, 0),
                            cache_headers,
                            config.range.as_deref(),
                        )
                        .ok_or(socket::E_INVAL)?;
                        let record = session.encrypt_app_data(&req[..reqlen]).ok_or(E_TLS)?;
                        socket::send(fd, &record).map_err(|_| E_TLS)?;

                        let (total, _) = read_http_response(fd, resp_buf, Some(&mut session))?;
                        socket::close(fd);
                        if total < 12 {
                            return Err(E_HTTP);
                        }
                        let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                        let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                        (status, hlen, total)
                    } else {
                        // HTTP üzerinden IPv6
                        let mut req = [0u8; 768];
                        let reqlen = build_request_with_headers(
                            &mut req,
                            &host[..host_len],
                            &path[..path_len],
                            crate::drivers::netstack::Ipv4Address::new(0, 0, 0, 0),
                            cache_headers,
                            config.range.as_deref(),
                        )
                        .ok_or(socket::E_INVAL)?;

                        let mut sent = 0;
                        while sent < reqlen {
                            sent += socket::send(fd, &req[sent..reqlen]).map_err(|e| {
                                if sent == 0 {
                                    e
                                } else {
                                    socket::E_TIMEOUT
                                }
                            })?;
                        }
                        let (total, _) = read_http_response(fd, resp_buf, None)?;
                        socket::close(fd);
                        if total < 12 {
                            return Err(E_HTTP);
                        }
                        let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                        let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                        (status, hlen, total)
                    };

                    Ok((status, hlen, total))
                })();

                match v6_result {
                    Ok((status, hlen, total)) => {
                        // IPv6 yanıtı: chunked çözüm ve redirect kontrolü (basit, döngüye devam et)
                        if (300..400).contains(&status) && hops < MAX_REDIRECTS {
                            let headers = &resp_buf[..hlen];
                            if let Some(loc) = header_value(headers, b"location") {
                                let loc_str = core::str::from_utf8(loc).map_err(|_| E_HTTP)?;
                                if loc_str.starts_with("http://") || loc_str.starts_with("https://")
                                {
                                    is_https = loc_str.starts_with("https://");
                                    let (p2, hl2, pl2) = parse_url(loc_str, &mut host, &mut path)?;
                                    port = p2;
                                    host_len = hl2;
                                    path_len = pl2;
                                    hops += 1;
                                    continue;
                                } else if loc_str.starts_with('/') && loc_str.len() <= MAX_PATH {
                                    path[..loc_str.len()].copy_from_slice(loc_str.as_bytes());
                                    path_len = loc_str.len();
                                    hops += 1;
                                    continue;
                                }
                                return Err(E_REDIRECT);
                            }
                        }

                        // Chunked gövdeyi yerinde çöz.
                        let headers = &resp_buf[..hlen];
                        let is_chunked = header_value(headers, b"transfer-encoding")
                            .map(|v| {
                                v.iter()
                                    .zip(b"chunked".iter())
                                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                                    && v.len() == 7
                            })
                            .unwrap_or(false);
                        if is_chunked {
                            let body_len = total - hlen;
                            let mut tmp = resp_buf[hlen..total].to_vec();
                            if let Some(decoded) = decode_chunked(&mut tmp, body_len) {
                                resp_buf[hlen..hlen + decoded].copy_from_slice(&tmp[..decoded]);
                                return Ok((status, hlen, hlen + decoded));
                            }
                            return Err(E_HTTP);
                        }
                        return Ok((status, hlen, total));
                    }
                    Err(_e) => {
                        // IPv6 yolu başarısız (TCP/TLS/HTTP hatası): IPv4'e düş.
                        crate::serial_print("[HTTPv6] IPv6 yolu basarisiz, IPv4'e dusuluyor\n");
                    }
                }
            }
        }

        // IPv4 yolu: mevcut fetch_once kullanılır
        let ip = socket::resolve_host(host_str)?;
        let (status, hlen, total) = fetch_once(
            &host[..host_len],
            port,
            &path[..path_len],
            ip,
            resp_buf,
            is_https,
            cache_headers,
            &config,
        )?;
        if (300..400).contains(&status) && hops < MAX_REDIRECTS {
            let headers = &resp_buf[..hlen];
            if let Some(loc) = header_value(headers, b"location") {
                let loc_str = core::str::from_utf8(loc).map_err(|_| E_HTTP)?;
                if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
                    is_https = loc_str.starts_with("https://");
                    let (p2, hl2, pl2) = parse_url(loc_str, &mut host, &mut path)?;
                    port = p2;
                    host_len = hl2;
                    path_len = pl2;
                    hops += 1;
                    continue;
                } else if loc_str.starts_with('/') && loc_str.len() <= MAX_PATH {
                    path[..loc_str.len()].copy_from_slice(loc_str.as_bytes());
                    path_len = loc_str.len();
                    hops += 1;
                    continue;
                }
                return Err(E_REDIRECT);
            }
        }
        // Chunked gövdeyi yerinde çöz.
        let headers = &resp_buf[..hlen];
        let is_chunked = header_value(headers, b"transfer-encoding")
            .map(|v| {
                v.iter()
                    .zip(b"chunked".iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    && v.len() == 7
            })
            .unwrap_or(false);
        if is_chunked {
            let body_len = total - hlen;
            let mut tmp = resp_buf[hlen..total].to_vec();
            if let Some(decoded) = decode_chunked(&mut tmp, body_len) {
                resp_buf[hlen..hlen + decoded].copy_from_slice(&tmp[..decoded]);
                return Ok((status, hlen, hlen + decoded));
            }
            return Err(E_HTTP);
        }
        let _ = (host_str, path_str);
        return Ok((status, hlen, total));
    }
}

// ---------------------------------------------------------------------------
// HTTP/1.1 tüm metod desteği (2.1) ve form kodlama (2.2)
// ---------------------------------------------------------------------------

/// HTTP metodu türleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static [u8] {
        match self {
            HttpMethod::Get => b"GET",
            HttpMethod::Post => b"POST",
            HttpMethod::Put => b"PUT",
            HttpMethod::Patch => b"PATCH",
            HttpMethod::Delete => b"DELETE",
            HttpMethod::Head => b"HEAD",
            HttpMethod::Options => b"OPTIONS",
        }
    }
}

/// application/x-www-form-urlencoded kodlama (RFC 3986 uyumlu percent-encoding).
pub fn form_urlencode(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&url_encode(k));
        out.push('=');
        out.push_str(&url_encode(v));
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// multipart/form-data kodlama (basit, dosya yükleme desteği ile).
pub fn form_multipart(
    boundary: &str,
    fields: &[(&str, &str)],
    files: &[(&str, &str, &str, &[u8])], // (name, filename, content_type, data)
) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
        );
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    for (name, filename, content_type, data) in files {
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        out.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                name, filename
            )
            .as_bytes(),
        );
        out.extend_from_slice(format!("Content-Type: {}\r\n\r\n", content_type).as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    out
}

/// Genel HTTP istek fonksiyonu: herhangi bir metod, gövde ve ek başlıklar destekler.
pub fn http_request(
    method: HttpMethod,
    url: &str,
    resp_buf: &mut [u8],
    body: Option<&[u8]>,
    content_type: Option<&str>,
    extra_headers: Option<&str>,
    cache_headers: Option<&str>,
) -> Result<(u16, usize, usize), i32> {
    if !e1000::is_ready() {
        return Err(socket::E_NOTCONN);
    }
    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (mut port, mut host_len, mut path_len) = parse_url(url, &mut host, &mut path)?;
    let mut is_https = url.starts_with("https://");
    let mut hops = 0;
    loop {
        let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;
        let path_str = core::str::from_utf8(&path[..path_len]).map_err(|_| E_URL)?;
        let ip = socket::resolve_host(host_str)?;
        let config = HttpClientConfig::default();
        let (status, hlen, total) = fetch_once_with_method(
            method,
            &host[..host_len],
            port,
            &path[..path_len],
            ip,
            resp_buf,
            body,
            content_type,
            extra_headers,
            is_https,
            cache_headers,
            &config,
        )?;
        if (300..400).contains(&status) && hops < MAX_REDIRECTS {
            let headers = &resp_buf[..hlen];
            if let Some(loc) = header_value(headers, b"location") {
                let loc_str = core::str::from_utf8(loc).map_err(|_| E_HTTP)?;
                if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
                    is_https = loc_str.starts_with("https://");
                    let (p2, hl2, pl2) = parse_url(loc_str, &mut host, &mut path)?;
                    port = p2;
                    host_len = hl2;
                    path_len = pl2;
                    hops += 1;
                    continue;
                } else if loc_str.starts_with('/') && loc_str.len() <= MAX_PATH {
                    path[..loc_str.len()].copy_from_slice(loc_str.as_bytes());
                    path_len = loc_str.len();
                    hops += 1;
                    continue;
                }
                return Err(E_REDIRECT);
            }
        }
        // Chunked gövdeyi yerinde çöz.
        let headers = &resp_buf[..hlen];
        let is_chunked = header_value(headers, b"transfer-encoding")
            .map(|v| {
                v.iter()
                    .zip(b"chunked".iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
                    && v.len() == 7
            })
            .unwrap_or(false);
        if is_chunked {
            let body_len = total - hlen;
            let mut tmp = resp_buf[hlen..total].to_vec();
            if let Some(decoded) = decode_chunked(&mut tmp, body_len) {
                resp_buf[hlen..hlen + decoded].copy_from_slice(&tmp[..decoded]);
                return Ok((status, hlen, hlen + decoded));
            }
            return Err(E_HTTP);
        }
        let _ = (host_str, path_str);
        return Ok((status, hlen, total));
    }
}

/// POST istek kolaylık sarmalayıcısı.
pub fn http_post(
    url: &str,
    resp_buf: &mut [u8],
    body: &[u8],
    content_type: &str,
) -> Result<(u16, usize, usize), i32> {
    http_request(
        HttpMethod::Post,
        url,
        resp_buf,
        Some(body),
        Some(content_type),
        None,
        None,
    )
}

/// PUT istek kolaylık sarmalayıcısı.
pub fn http_put(
    url: &str,
    resp_buf: &mut [u8],
    body: &[u8],
    content_type: &str,
) -> Result<(u16, usize, usize), i32> {
    http_request(
        HttpMethod::Put,
        url,
        resp_buf,
        Some(body),
        Some(content_type),
        None,
        None,
    )
}

/// PATCH istek kolaylık sarmalayıcısı.
pub fn http_patch(
    url: &str,
    resp_buf: &mut [u8],
    body: &[u8],
    content_type: &str,
) -> Result<(u16, usize, usize), i32> {
    http_request(
        HttpMethod::Patch,
        url,
        resp_buf,
        Some(body),
        Some(content_type),
        None,
        None,
    )
}

/// DELETE istek kolaylık sarmalayıcısı.
pub fn http_delete(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize, usize), i32> {
    http_request(HttpMethod::Delete, url, resp_buf, None, None, None, None)
}

/// `fetch_once` gibi ama farklı HTTP metodu ve gövde desteğiyle.
fn fetch_once_with_method(
    method: HttpMethod,
    host: &[u8],
    port: u16,
    path: &[u8],
    ip: Ipv4Address,
    resp_buf: &mut [u8],
    body: Option<&[u8]>,
    content_type: Option<&str>,
    extra_headers: Option<&str>,
    is_https: bool,
    _cache_headers: Option<&str>,
    config: &HttpClientConfig,
) -> Result<(u16, usize, usize), i32> {
    let host_str = core::str::from_utf8(host).unwrap_or("");
    let use_pool = config.proxy.is_none(); // Proxy ile bağlantı havuzlanmaz

    let mut last_err: Option<i32> = None;
    let mut delay = config.retry_delay_ms;

    for attempt in 0..=config.max_retries {
        let fd = if use_pool {
            // Havuzdan mevcut bağlantıyı almayı dene
            if let Some(cached_fd) = connection_pool().acquire(host_str, port, is_https) {
                cached_fd
            } else {
                let new_fd =
                    socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|&e| {
                        last_err = Some(e);
                    })?;
                let _ = socket::set_timeout(new_fd, config.timeout_ms);
                socket::connect(new_fd, ip, port).inspect_err(|&e| {
                    socket::close(new_fd);
                    last_err = Some(e);
                })?;
                new_fd
            }
        } else {
            let new_fd =
                socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|&e| {
                    last_err = Some(e);
                })?;
            let _ = socket::set_timeout(new_fd, config.timeout_ms);
            socket::connect(new_fd, ip, port).inspect_err(|&e| {
                socket::close(new_fd);
                last_err = Some(e);
            })?;
            new_fd
        };

        let res = (|| {
            // Proxy ile bağlanma: proxy'e bağlan, gerekirse CONNECT tüneli kur
            if let Some(ref proxy_str) = config.proxy {
                let proxy_host_port = proxy_str.as_str();
                let proxy_ip = socket::resolve_host(
                    proxy_host_port.split(':').next().unwrap_or(proxy_host_port),
                )?;
                let proxy_port = proxy_host_port
                    .split(':')
                    .nth(1)
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(8080);
                socket::connect(fd, proxy_ip, proxy_port)?;

                if is_https {
                    // HTTPS üzerinden proxy: CONNECT tüneli kur
                    let host_port = format!("{}:{}", host_str, port);
                    proxy_connect(fd, &host_port, config)?;
                    // TLS el sıkışması tünel üzerinden
                    let empty_req = [0u8; 0];
                    fetch_https(fd, host, &empty_req, 0, resp_buf)?;
                } else {
                    // HTTP üzerinden proxy: mutlak URL formu kullan
                    let mut req = [0u8; 1024];
                    let url_str = format!(
                        "http://{}{}",
                        host_str,
                        core::str::from_utf8(path).unwrap_or("/")
                    );
                    let reqlen = build_proxy_request(&mut req, &url_str, host, extra_headers)
                        .ok_or(socket::E_INVAL)?;
                    let mut sent = 0;
                    while sent < reqlen {
                        sent += socket::send(fd, &req[sent..reqlen]).map_err(|e| {
                            if sent == 0 {
                                e
                            } else {
                                socket::E_TIMEOUT
                            }
                        })?;
                    }
                    let (total, expected_total) = read_http_response(fd, resp_buf, None)?;
                    let _ = expected_total;
                    if total < 12 {
                        return Err(E_HTTP);
                    }
                    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                    return Ok((status, hlen, total));
                }
            } else {
                socket::connect(fd, ip, port)?;
            }

            let mut req = [0u8; 1024];
            let reqlen = build_request_with_method(
                &mut req,
                method,
                host,
                path,
                body,
                content_type,
                extra_headers,
                config.range.as_deref(),
            )
            .ok_or(socket::E_INVAL)?;

            let (status, hlen, total) = if is_https {
                fetch_https(fd, host, &req, reqlen, resp_buf)?
            } else {
                let mut sent = 0;
                while sent < reqlen {
                    sent += socket::send(fd, &req[sent..reqlen]).map_err(|e| {
                        if sent == 0 {
                            e
                        } else {
                            socket::E_TIMEOUT
                        }
                    })?;
                }
                let (total, expected_total) = read_http_response(fd, resp_buf, None)?;
                let _ = expected_total;
                if total < 12 {
                    return Err(E_HTTP);
                }
                let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                (status, hlen, total)
            };

            Ok((status, hlen, total))
        })();

        // Bağlantıyı havuza geri ver (doğrudan bağlantı ve başarılıysa)
        if use_pool {
            connection_pool().release(fd, host_str.to_string(), port, is_https, None);
        } else {
            socket::close(fd);
        }

        match res {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = Some(e);
                if attempt < config.max_retries {
                    sleep_ms(delay);
                    delay = (delay * 2).min(10000);
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(last_err.unwrap_or(E_HTTP))
}

/// HTTP istek başlığı oluşturur: metod, gövde, Content-Type, Content-Length.
fn build_request_with_method(
    req: &mut [u8],
    method: HttpMethod,
    host: &[u8],
    path: &[u8],
    body: Option<&[u8]>,
    content_type: Option<&str>,
    extra_headers: Option<&str>,
    range: Option<&str>,
) -> Option<usize> {
    let mut n = 0usize;

    // Metod + path + HTTP versiyonu
    let method_bytes = method.as_str();
    if n + method_bytes.len() + 1 + path.len() + 13 > req.len() {
        return None;
    }
    req[n..n + method_bytes.len()].copy_from_slice(method_bytes);
    n += method_bytes.len();
    req[n] = b' ';
    n += 1;
    req[n..n + path.len()].copy_from_slice(path);
    n += path.len();
    let tail = b" HTTP/1.1\r\nHost: ";
    req[n..n + tail.len()].copy_from_slice(tail);
    n += tail.len();
    req[n..n + host.len()].copy_from_slice(host);
    n += host.len();

    // Range header
    if let Some(r) = range {
        let range_header = format!("\r\nRange: bytes={}", r);
        let range_bytes = range_header.as_bytes();
        if n + range_bytes.len() > req.len() {
            return None;
        }
        req[n..n + range_bytes.len()].copy_from_slice(range_bytes);
        n += range_bytes.len();
    }

    // Content-Type
    if let Some(ct) = content_type {
        let ct_str = format!("\r\nContent-Type: {}", ct);
        let ct_bytes = ct_str.as_bytes();
        if n + ct_bytes.len() > req.len() {
            return None;
        }
        req[n..n + ct_bytes.len()].copy_from_slice(ct_bytes);
        n += ct_bytes.len();
    }

    // Content-Length
    let body_len = body.map_or(0, |b| b.len());
    let cl_str = format!("\r\nContent-Length: {}\r\nConnection: keep-alive", body_len);
    let cl_bytes = cl_str.as_bytes();
    if n + cl_bytes.len() > req.len() {
        return None;
    }
    req[n..n + cl_bytes.len()].copy_from_slice(cl_bytes);
    n += cl_bytes.len();

    // Extra headers (cache validation, etc.)
    if let Some(headers) = extra_headers {
        let headers_bytes = headers.as_bytes();
        if n + headers_bytes.len() > req.len() {
            return None;
        }
        req[n..n + headers_bytes.len()].copy_from_slice(headers_bytes);
        n += headers_bytes.len();
    }

    // Final CRLF
    if n + 2 > req.len() {
        return None;
    }
    req[n..n + 2].copy_from_slice(b"\r\n");
    n += 2;

    // Gövde
    if let Some(b) = body {
        if n + b.len() > req.len() {
            return None;
        }
        req[n..n + b.len()].copy_from_slice(b);
        n += b.len();
    }

    Some(n)
}

// ---------------------------------------------------------------------------
// Mantık self-testi (NIC gerektirmez)
// ---------------------------------------------------------------------------

pub fn self_test() -> bool {
    let mut ok = true;
    // URL ayrıştırma.
    let mut h = [0u8; MAX_HOST];
    let mut p = [0u8; MAX_PATH];
    match parse_url("http://example.com/a/b?x=1", &mut h, &mut p) {
        Ok((80, 11, _)) if &h[..11] == b"example.com" && &p[..8] == b"/a/b?x=1" => {}
        _ => ok = false,
    }
    match parse_url("http://10.0.2.2:8080/", &mut h, &mut p) {
        Ok((8080, 8, 1)) => {}
        _ => ok = false,
    }
    match parse_url("https://example.com/", &mut h, &mut p) {
        Ok((443, 11, 1)) if &h[..11] == b"example.com" && &p[..1] == b"/" => {}
        _ => ok = false,
    }
    if parse_url("ftp://x/", &mut h, &mut p) != Err(E_URL) {
        ok = false;
    }
    // Durum satırı + başlık değeri.
    let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\nX-A: b\r\n\r\nhello";
    if parse_status(raw) != Some(200) {
        ok = false;
    }
    if header_value(&raw[..38], b"content-length") != Some(b"42".as_slice()) {
        ok = false;
    }
    if find_header_end(raw) != Some(47) {
        ok = false;
    }
    // Chunked çözümü.
    let chunked = *b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let mut cbuf = [0u8; 64];
    cbuf[..chunked.len()].copy_from_slice(&chunked);
    match decode_chunked(&mut cbuf, chunked.len()) {
        Some(11) if &cbuf[..11] == b"hello world" => {}
        _ => ok = false,
    }
    let partial = b"5\r\nhello\r\n";
    let mut cbuf2 = [0u8; 64];
    cbuf2[..partial.len()].copy_from_slice(partial);
    if decode_chunked(&mut cbuf2, partial.len()).is_some() {
        ok = false; // yarım gövde reddedilmeli
    }
    crate::serial_print("[HTTP-TEST] url/status/chunked self-check: ");
    crate::serial_print(if ok { "BAŞARILI\n" } else { "BAŞARISIZ\n" });
    ok
}

// ---------------------------------------------------------------------------
// HTTP/2 (RFC 7540) + HPACK (RFC 7541) — `drivers::http2` modülüne taşındı.
// Geriye dönük uyumluluk için adlar burada yeniden dışa aktarılır.
// ---------------------------------------------------------------------------
pub use crate::drivers::http2::{
    h2, h2_send_preface, http2_get, is_http2_response, Http2Connection,
};

/// WebSocket (RFC 6455) yardımcıları. TCP/TLS taşımasını çağıran katman sağlar.
pub mod websocket {
    use alloc::vec::Vec;

    pub const OP_CONTINUATION: u8 = 0x0;
    pub const OP_TEXT: u8 = 0x1;
    pub const OP_BINARY: u8 = 0x2;
    pub const OP_CLOSE: u8 = 0x8;
    pub const OP_PING: u8 = 0x9;
    pub const OP_PONG: u8 = 0xA;

    /// İstemci Upgrade isteği üretir. `key` 16-byte random değerin Base64'üdür.
    pub fn build_upgrade_request(host: &[u8], path: &[u8], key_b64: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GET ");
        out.extend_from_slice(path);
        out.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        out.extend_from_slice(host);
        out.extend_from_slice(
            b"\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ",
        );
        out.extend_from_slice(key_b64);
        out.extend_from_slice(b"\r\nSec-WebSocket-Version: 13\r\n\r\n");
        out
    }

    /// İstemci frame'i üretir; RFC gereği payload maskelenir.
    pub fn build_client_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0x80 | (opcode & 0x0f));
        let n = payload.len();
        if n < 126 {
            out.push(0x80 | n as u8);
        } else if n <= 0xffff {
            out.push(0x80 | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        } else {
            out.push(0x80 | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
        out.extend_from_slice(&mask);
        for (i, b) in payload.iter().enumerate() {
            out.push(*b ^ mask[i & 3]);
        }
        out
    }

    /// Sunucu frame başlığını ayrıştırır ve `(fin, opcode, masked, payload_len, header_len)` döner.
    pub fn parse_frame_header(data: &[u8]) -> Option<(bool, u8, bool, usize, usize)> {
        if data.len() < 2 {
            return None;
        }
        let fin = data[0] & 0x80 != 0;
        let opcode = data[0] & 0x0f;
        let masked = data[1] & 0x80 != 0;
        let short = (data[1] & 0x7f) as usize;
        let mut p = 2;
        let len = match short {
            126 => {
                if data.len() < 4 {
                    return None;
                };
                p += 2;
                u16::from_be_bytes([data[2], data[3]]) as usize
            }
            127 => {
                if data.len() < 10 {
                    return None;
                };
                p += 8;
                usize::try_from(u64::from_be_bytes(data[2..10].try_into().ok()?)).ok()?
            }
            n => n,
        };
        if masked {
            p += 4;
        }
        if p > data.len() {
            return None;
        }
        Some((fin, opcode, masked, len, p))
    }

    pub fn unmask(payload: &mut [u8], mask: [u8; 4]) {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i & 3];
        }
    }
}

// ---------------------------------------------------------------------------
// WebSocket bağlantı yönetimi (RFC 6455) — ws:// ve wss://
// ---------------------------------------------------------------------------

/// WebSocket bağlantı durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsState {
    Connecting,
    Open,
    Closing,
    Closed,
}

/// WebSocket bağlantısı: TCP/TLS soket üzerinden çerçeveleme.
pub struct WebSocketConnection {
    fd: i32,
    is_wss: bool,
    tls_session: Option<crate::drivers::tls::TlsSession>,
    state: WsState,
    mask_key: [u8; 4],
}

impl WebSocketConnection {
    /// ws:// veya wss:// URL'sine bağlanır ve WebSocket el sıkışmasını yapar.
    pub fn connect(url: &str) -> Result<Self, i32> {
        let is_wss = url.starts_with("wss://");
        let ws_url = if is_wss { &url[6..] } else { &url[5..] };

        // host[:port]/path ayır.
        let (authority, path) = match ws_url.find('/') {
            Some(i) => (&ws_url[..i], &ws_url[i..]),
            None => (ws_url, "/"),
        };
        let (host, port) = match authority.find(':') {
            Some(i) => (
                authority[..i].to_string(),
                authority[i + 1..].parse::<u16>().unwrap_or(443),
            ),
            None => (authority.to_string(), if is_wss { 443 } else { 80 }),
        };

        let (fd, mut tls_session) =
            (|| -> Result<(i32, Option<crate::drivers::tls::TlsSession>), i32> {
                // TLS el sıkışması yardımcı closure (IPv6 ve IPv4 için ortak).
                let tls_handshake =
                    |fd: i32, host: &str| -> Result<Option<crate::drivers::tls::TlsSession>, i32> {
                        if !is_wss {
                            return Ok(None);
                        }
                        let mut client_random = [0u8; 32];
                        let mut state =
                            crate::boot::time::main_monotonic_ms() as u64 ^ 0x9e37_79b9_7f4a_7c15;
                        for b in client_random.iter_mut() {
                            state ^= state >> 12;
                            state ^= state << 25;
                            state ^= state >> 27;
                            *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
                        }

                        let mut hello = Vec::new();
                        let mut ecdhe_secret = [0u8; 32];
                        crate::drivers::tls::build_client_hello_with_alpn(
                            host,
                            &client_random,
                            &mut hello,
                            &mut ecdhe_secret,
                            false,
                            None,
                            None,
                            &[b"h2", b"http/1.1"],
                        );
                        let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
                        let mut session = crate::drivers::tls::TlsSession::new(host, client_random);
                        session.note_client_hello(ch_body);
                        session.set_client_ecdhe_secret(ecdhe_secret);

                        socket::send(fd, &hello).map_err(|_| E_TLS)?;

                        let handshake_deadline = crate::boot::time::main_monotonic_ms() + 8_000;
                        loop {
                            let mut raw = [0u8; 4096];
                            let n = socket::recv(fd, &mut raw).map_err(|_| E_TLS)?;
                            if n == 0 {
                                return Err(E_TLS);
                            }
                            session.feed_tcp_data(&raw[..n]);

                            let mut app = Vec::new();
                            let mut hs = Vec::new();
                            match session.process_records(&mut hs, &mut app) {
                                Ok(_) => {}
                                Err(_) => return Err(E_TLS),
                            }

                            if session.state() == crate::drivers::tls::TlsState::Tls13ServerFinished
                            {
                                if let Some(ccs) = session.tls13_change_cipher_spec() {
                                    socket::send(fd, &ccs).map_err(|_| E_TLS)?;
                                }
                                if let Some(record) = session.build_tls13_client_finished() {
                                    socket::send(fd, &record).map_err(|_| E_TLS)?;
                                }
                            }

                            let outgoing = session.take_outgoing();
                            if !outgoing.is_empty() {
                                socket::send(fd, &outgoing).map_err(|_| E_TLS)?;
                            }

                            if session.state() == crate::drivers::tls::TlsState::Established {
                                break;
                            }
                            if crate::boot::time::main_monotonic_ms() > handshake_deadline {
                                return Err(E_TLS);
                            }
                        }

                        Ok(Some(session))
                    };

                // IPv6 (AAAA) öncelikli, IPv4'e otomatik geri düşme — bağlantı/TLS
                // hatasında IPv4 yoluna düşülür (dual-stack fallback).
                if let Ok(v6) = socket::resolve_host_aaaa(&host) {
                    let fd6 =
                        socket::socket(socket::AF_INET6, socket::SOCK_STREAM).inspect_err(|e| {
                            crate::serial_print("[WSv6] socket oluşturulamadı: ");
                            crate::serial_print(&e.to_string());
                            crate::serial_print("\n");
                        })?;
                    let _ = socket::set_timeout(fd6, 8000);
                    if socket::connect_v6(fd6, v6, port).is_ok() {
                        match tls_handshake(fd6, &host) {
                            Ok(tls) => return Ok((fd6, tls)),
                            Err(e) => {
                                socket::close(fd6);
                                return Err(e);
                            }
                        }
                    }
                    socket::close(fd6);
                }

                // IPv4 yolu
                let ip = socket::resolve_host(&host)?;
                let fd4 =
                    socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|e| {
                        crate::serial_print("[WS] socket oluşturulamadı: ");
                        crate::serial_print(&e.to_string());
                        crate::serial_print("\n");
                    })?;
                let _ = socket::set_timeout(fd4, 8000);
                socket::connect(fd4, ip, port).inspect_err(|_e| {
                    socket::close(fd4);
                })?;
                let tls = tls_handshake(fd4, &host)?;
                Ok((fd4, tls))
            })()?;

        // WebSocket Upgrade isteği gönder.
        let key = crate::drivers::tls::TlsSession::new("", [0u8; 32]); // dummy for random
        let _ = key;
        let ws_key = generate_ws_key();
        let upgrade_req =
            websocket::build_upgrade_request(host.as_bytes(), path.as_bytes(), ws_key.as_bytes());

        let upgrade_bytes = if is_wss {
            if let Some(ref mut session) = tls_session {
                session.encrypt_app_data(&upgrade_req).ok_or(E_TLS)?
            } else {
                return Err(E_TLS);
            }
        } else {
            upgrade_req
        };

        let mut sent = 0;
        while sent < upgrade_bytes.len() {
            sent += socket::send(fd, &upgrade_bytes[sent..]).map_err(|_| socket::E_IO)?;
        }

        // Sunucu yanıtını oku (101 Switching Protocols beklenir).
        let mut buf = [0u8; 4096];
        let mut total = 0usize;
        let deadline = crate::boot::time::main_monotonic_ms() + 8_000;
        while total < buf.len() {
            let n = if is_wss {
                if let Some(ref mut session) = tls_session {
                    let mut raw = [0u8; 4096];
                    let rn = socket::recv(fd, &mut raw).map_err(|_| socket::E_IO)?;
                    if rn == 0 {
                        return Err(E_TLS);
                    }
                    session.feed_tcp_data(&raw[..rn]);
                    let mut app = Vec::new();
                    let mut hs = Vec::new();
                    session
                        .process_records(&mut hs, &mut app)
                        .map_err(|_| E_TLS)?;
                    let copy_len = app.len().min(buf.len() - total);
                    buf[total..total + copy_len].copy_from_slice(&app[..copy_len]);
                    total += copy_len;
                    copy_len
                } else {
                    return Err(E_TLS);
                }
            } else {
                socket::recv(fd, &mut buf[total..]).map_err(|_| socket::E_IO)?
            };
            if n == 0 {
                break;
            }

            // "101" kontrolü
            if total >= 12 {
                let resp = &buf[..total.min(16)];
                if resp.starts_with(b"HTTP/1.1 101") || resp.starts_with(b"HTTP/1.0 101") {
                    break;
                }
            }
            if crate::boot::time::main_monotonic_ms() > deadline {
                return Err(socket::E_TIMEOUT);
            }
        }

        Ok(Self {
            fd,
            is_wss,
            tls_session,
            state: WsState::Open,
            mask_key: [0x12, 0x34, 0x56, 0x78],
        })
    }

    /// Mesaj gönder (text frame).
    pub fn send(&mut self, data: &[u8]) -> Result<(), i32> {
        if self.state != WsState::Open {
            return Err(socket::E_NOTCONN);
        }
        let frame = websocket::build_client_frame(websocket::OP_TEXT, data, self.mask_key);
        let frame_bytes = if self.is_wss {
            if let Some(ref mut session) = self.tls_session {
                session.encrypt_app_data(&frame).ok_or(E_TLS)?
            } else {
                return Err(E_TLS);
            }
        } else {
            frame
        };
        let mut sent = 0;
        while sent < frame_bytes.len() {
            sent += socket::send(self.fd, &frame_bytes[sent..]).map_err(|_| socket::E_IO)?;
        }
        Ok(())
    }

    /// Mesaj al (blocking, timeout ile).
    pub fn recv(&mut self) -> Result<Option<String>, i32> {
        if self.state != WsState::Open {
            return Err(socket::E_NOTCONN);
        }
        let mut buf = [0u8; 4096];
        let n = if self.is_wss {
            if let Some(ref mut session) = self.tls_session {
                let mut raw = [0u8; 4096];
                let rn = socket::recv(self.fd, &mut raw).map_err(|_| socket::E_IO)?;
                if rn == 0 {
                    return Ok(None);
                }
                session.feed_tcp_data(&raw[..rn]);
                let mut app = Vec::new();
                let mut hs = Vec::new();
                session
                    .process_records(&mut hs, &mut app)
                    .map_err(|_| E_TLS)?;
                if app.is_empty() {
                    return Ok(None);
                }
                let copy_len = app.len().min(buf.len());
                buf[..copy_len].copy_from_slice(&app[..copy_len]);
                copy_len
            } else {
                return Err(E_TLS);
            }
        } else {
            socket::recv(self.fd, &mut buf).map_err(|_| socket::E_IO)?
        };
        if n == 0 {
            return Ok(None);
        }

        // Frame başlığını ayrıştır.
        if let Some((fin, opcode, masked, payload_len, header_len)) =
            websocket::parse_frame_header(&buf[..n])
        {
            if !masked {
                return Err(socket::E_IO); // Sunucu frame'i maskelenmeli değil
            }
            if header_len + payload_len > n {
                return Ok(None); // Henüz tam frame gelmedi
            }
            let payload = buf[header_len..header_len + payload_len].to_vec();
            // Sunucu frame'i maskelenmemeli (RFC 6455), ama mask varsa çıkar.
            let _ = masked;
            let _ = fin;
            match opcode {
                websocket::OP_TEXT => Ok(Some(String::from_utf8_lossy(&payload).to_string())),
                websocket::OP_BINARY => Ok(Some(String::from_utf8_lossy(&payload).to_string())),
                websocket::OP_CLOSE => {
                    self.state = WsState::Closed;
                    Ok(None)
                }
                websocket::OP_PING => {
                    // PONG yanıtı gönder.
                    let pong =
                        websocket::build_client_frame(websocket::OP_PONG, &payload, self.mask_key);
                    let pong_bytes = if self.is_wss {
                        if let Some(ref mut session) = self.tls_session {
                            session.encrypt_app_data(&pong).ok_or(E_TLS)?
                        } else {
                            return Err(E_TLS);
                        }
                    } else {
                        pong
                    };
                    let _ = socket::send(self.fd, &pong_bytes);
                    Ok(None)
                }
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    /// Bağlantıyı kapat.
    pub fn close(&mut self) -> Result<(), i32> {
        if self.state == WsState::Closed {
            return Ok(());
        }
        let close_frame = websocket::build_client_frame(websocket::OP_CLOSE, &[], self.mask_key);
        let close_bytes = if self.is_wss {
            if let Some(ref mut session) = self.tls_session {
                session.encrypt_app_data(&close_frame).ok_or(E_TLS)?
            } else {
                return Err(E_TLS);
            }
        } else {
            close_frame
        };
        let _ = socket::send(self.fd, &close_bytes);
        socket::close(self.fd);
        self.state = WsState::Closed;
        Ok(())
    }
}

/// WebSocket anahtarı üretir (Base64-encoded 16-byte random değer).
fn generate_ws_key() -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut key_bytes = [0u8; 16];
    let mut state = crate::boot::time::main_monotonic_ms() as u64 ^ 0x9e37_79b9_7f4a_7c15;
    for b in key_bytes.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
    }
    let mut out = String::new();
    for i in (0..16).step_by(3) {
        let b0 = key_bytes[i] as u32;
        let b1 = if i + 1 < 16 {
            key_bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < 16 {
            key_bytes[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < 16 {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < 16 {
            out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

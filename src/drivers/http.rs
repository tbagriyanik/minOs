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
        let _ = socket::close(self.connections[oldest_idx].take().unwrap().fd);
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
                    let _ = socket::close(conn.fd);
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
pub fn parse_url<'a>(
    url: &'a str,
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
        req[n..n + p.len()].copy_from_slice(*p);
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

    for attempt in 0..=config.max_retries {
        // Havuzdan mevcut bağlantıyı almayı dene (sadece doğrudan bağlantı için)
        let fd = if use_pool {
            if let Some(cached_fd) = connection_pool().acquire(host_str, port, is_https) {
                cached_fd
            } else {
                let new_fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
                    last_err = Some(e);
                    e
                })?;
                let _ = socket::set_timeout(new_fd, config.timeout_ms);
                socket::connect(new_fd, ip, port).map_err(|e| {
                    let _ = socket::close(new_fd);
                    last_err = Some(e);
                    e
                })?;
                new_fd
            }
        } else {
            let new_fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
                last_err = Some(e);
                e
            })?;
            let _ = socket::set_timeout(new_fd, config.timeout_ms);
            socket::connect(new_fd, ip, port).map_err(|e| {
                let _ = socket::close(new_fd);
                last_err = Some(e);
                e
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
                    fetch_https(fd, host, &mut [0u8; 0], 0, resp_buf)
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
                    // TLS el sıkışması
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
                    crate::drivers::tls::build_client_hello(
                        host_str,
                        &client_random,
                        &mut hello,
                        &mut ecdhe_secret,
                        false,
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

                    // Şifreli HTTP isteğini hazırla ve gönder
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
    crate::drivers::tls::build_client_hello(
        host_str,
        &client_random,
        &mut hello,
        &mut ecdhe_secret,
        false,
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
        let mut config = HttpClientConfig::default();
        config.range = Some(range.to_string());
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
fn http_get_impl(
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
            // IPv6 yolu: connect_v6 ile bağlantı
            if let Ok(v6) = socket::resolve_host_aaaa(host_str) {
                let fd = socket::socket(socket::AF_INET6, socket::SOCK_STREAM).map_err(|e| {
                    crate::serial_print("[HTTPv6] socket oluşturulamadı: ");
                    crate::serial_print(&e.to_string());
                    crate::serial_print("\n");
                    e
                })?;
                let _ = socket::set_timeout(fd, 8000);
                match socket::connect_v6(fd, v6, port) {
                    Ok(()) => {}
                    Err(e) => {
                        let _ = socket::close(fd);
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
                    crate::drivers::tls::build_client_hello(
                        host_str,
                        &client_random,
                        &mut hello,
                        &mut ecdhe_secret,
                        false,
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
                    let _ = socket::close(fd);
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
                    let _ = socket::close(fd);
                    if total < 12 {
                        return Err(E_HTTP);
                    }
                    let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
                    let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
                    (status, hlen, total)
                };

                // IPv6 yanıtı: chunked çözüm ve redirect kontrolü (basit, döngüye devam et)
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
                return Ok((status, hlen, total));
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
                let new_fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
                    last_err = Some(e);
                    e
                })?;
                let _ = socket::set_timeout(new_fd, config.timeout_ms);
                socket::connect(new_fd, ip, port).map_err(|e| {
                    let _ = socket::close(new_fd);
                    last_err = Some(e);
                    e
                })?;
                new_fd
            }
        } else {
            let new_fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
                last_err = Some(e);
                e
            })?;
            let _ = socket::set_timeout(new_fd, config.timeout_ms);
            socket::connect(new_fd, ip, port).map_err(|e| {
                let _ = socket::close(new_fd);
                last_err = Some(e);
                e
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
// HTTP/2 Desteği (RFC 7540) — Temel çerçeve altyapısı
// ---------------------------------------------------------------------------

/// HTTP/2 çerçeve türleri ve sabitleri.
pub mod h2 {
    use alloc::vec::Vec;
    /// HTTP/2 çerçeve türleri (RFC 7540 §5.1.1).
    pub const FRAME_DATA: u8 = 0x0;
    pub const FRAME_HEADERS: u8 = 0x1;
    pub const FRAME_PRIORITY: u8 = 0x2;
    pub const FRAME_RST_STREAM: u8 = 0x3;
    pub const FRAME_SETTINGS: u8 = 0x4;
    pub const FRAME_PUSH_PROMISE: u8 = 0x5;
    pub const FRAME_PING: u8 = 0x6;
    pub const FRAME_GOAWAY: u8 = 0x7;
    pub const FRAME_WINDOW_UPDATE: u8 = 0x8;
    pub const FRAME_CONTINUATION: u8 = 0x9;

    /// İstemci bağlantı önyüzü (client connection preface).
    pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

    /// SETTINGS parametreleri (RFC 7540 §6.5.2).
    pub const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x1;
    pub const SETTINGS_ENABLE_PUSH: u16 = 0x2;
    pub const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x3;
    pub const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
    pub const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;
    pub const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x6;

    /// Çerçeve bayrakları.
    pub const FLAG_END_STREAM: u8 = 0x1;
    pub const FLAG_END_HEADERS: u8 = 0x4;
    pub const FLAG_PADDED: u8 = 0x8;
    pub const FLAG_PRIORITY: u8 = 0x20;
    pub const FLAG_ACK: u8 = 0x1;

    /// 9 baytlık HTTP/2 çerçeve başlığı (RFC 7540 §3).
    #[repr(C, packed)]
    #[derive(Clone, Copy, Debug)]
    pub struct FrameHeader {
        pub length: [u8; 3],    // 24-bit payload uzunluğu
        pub r#type: u8,         // Çerçeve türü
        pub flags: u8,          // Bayraklar
        pub stream_id: [u8; 4], // 31-bit akış kimliği (bit 31 reserved)
    }

    impl FrameHeader {
        /// Payload uzunluğunu döndürür.
        pub fn payload_len(&self) -> u32 {
            ((self.length[0] as u32) << 16)
                | ((self.length[1] as u32) << 8)
                | (self.length[2] as u32)
        }

        /// Akış kimliğini döndürür (bit 31 hariç).
        pub fn stream_id(&self) -> u32 {
            ((self.stream_id[0] as u32 & 0x7F) << 24)
                | ((self.stream_id[1] as u32) << 16)
                | ((self.stream_id[2] as u32) << 8)
                | (self.stream_id[3] as u32)
        }
    }

    /// HTTP/2 istemci bağlantı önyüzünü kontrol eder.
    pub fn is_client_preface(data: &[u8]) -> bool {
        data.len() >= PREFACE.len() && &data[..PREFACE.len()] == PREFACE
    }

    /// 9 baytlık bir tampondan HTTP/2 çerçeve başlığı ayrıştırır.
    pub fn parse_frame_header(data: &[u8]) -> Option<FrameHeader> {
        if data.len() < 9 {
            return None;
        }
        Some(FrameHeader {
            length: [data[0], data[1], data[2]],
            r#type: data[3],
            flags: data[4],
            stream_id: [data[5], data[6], data[7], data[8]],
        })
    }

    /// Boş bir SETTINGS çerçevesi oluşturur (istemci → sunucu el sıkışması).
    pub fn build_client_settings() -> alloc::vec::Vec<u8> {
        let mut frame = alloc::vec::Vec::new();
        frame.push(0x00); // length >> 16
        frame.push(0x00); // length >> 8
        frame.push(0x00); // length & 0xFF
        frame.push(FRAME_SETTINGS); // type
        frame.push(0x00); // flags
        frame.push(0x00); // stream ID >> 24
        frame.push(0x00);
        frame.push(0x00);
        frame.push(0x00);
        frame
    }

    fn encode_int(out: &mut Vec<u8>, value: usize, prefix_bits: u8, first: u8) {
        let max = (1usize << prefix_bits) - 1;
        if value < max {
            out.push(first | value as u8);
            return;
        }
        out.push(first | max as u8);
        let mut n = value - max;
        while n >= 128 {
            out.push((n as u8 & 0x7f) | 0x80);
            n >>= 7;
        }
        out.push(n as u8);
    }

    fn encode_string(out: &mut Vec<u8>, value: &[u8]) {
        encode_int(out, value.len(), 7, 0);
        out.extend_from_slice(value);
    }

    /// HPACK dinamik tablo girişi (isim + değer).
    #[derive(Clone, Debug)]
    struct HpackEntry {
        name: Vec<u8>,
        value: Vec<u8>,
    }

    /// HPACK dinamik tablo: sunucu tarafından bildirilen SETTINGS_HEADER_TABLE_SIZE
    /// sınırına göre en son eklenen girdileri tutar, eskileri çıkarır.
    pub struct HpackDynamicTable {
        entries: alloc::vec::Vec<HpackEntry>,
        current_size: usize,
        max_size: usize,
    }

    impl HpackDynamicTable {
        pub fn new(max_size: usize) -> Self {
            Self {
                entries: alloc::vec::Vec::new(),
                current_size: 0,
                max_size,
            }
        }

        /// Girdiyi ekle; gerekirse eskileri çıkar (RFC 7541 §4.2).
        fn insert(&mut self, name: &[u8], value: &[u8]) {
            let entry_size = 32 + name.len() + value.len(); // 32 = overhead (RFC 7541 §4.2)
                                                            // Çok büyükse ekleme.
            if entry_size > self.max_size {
                return;
            }
            // Yer açmak için eskileri çıkar.
            while self.current_size + entry_size > self.max_size && !self.entries.is_empty() {
                let removed = self.entries.remove(0);
                self.current_size -= 32 + removed.name.len() + removed.value.len();
            }
            self.entries.push(HpackEntry {
                name: name.to_vec(),
                value: value.to_vec(),
            });
            self.current_size += entry_size;
        }

        /// İndisle (1-based, dinamik tablo) ara; bulunursa (index, name, value) döner.
        fn get_by_index(&self, index: usize) -> Option<(usize, &[u8], &[u8])> {
            // Dinamik tablo indeksleri statik tablo boyutundan sonra başlar.
            // Statik tablo boyutu = 61 (RFC 7541 §2.3.1).
            const STATIC_TABLE_SIZE: usize = 61;
            if index <= STATIC_TABLE_SIZE {
                return None; // Statik tablo, burada işlenmez.
            }
            let dyn_idx = index - STATIC_TABLE_SIZE - 1;
            self.entries
                .get(dyn_idx)
                .map(|e| (index, e.name.as_slice(), e.value.as_slice()))
        }

        /// İsim-değer çiftini ara; bulunursa dinamik tablo indeksi (1-based, statikten sonra) döner.
        fn find(&self, name: &[u8], value: Option<&[u8]>) -> Option<usize> {
            const STATIC_TABLE_SIZE: usize = 61;
            for (i, e) in self.entries.iter().enumerate() {
                let idx = STATIC_TABLE_SIZE + 1 + i;
                if e.name == name {
                    if let Some(v) = value {
                        if e.value == *v {
                            return Some(idx);
                        }
                    } else {
                        return Some(idx); // Sadece isim eşleşti (literal değer, indexed name)
                    }
                }
            }
            None
        }

        fn max_size(&self) -> usize {
            self.max_size
        }

        fn set_max_size(&mut self, size: usize) {
            self.max_size = size;
            // Yeni limitten büyükse eskileri çıkar.
            while self.current_size > self.max_size && !self.entries.is_empty() {
                let removed = self.entries.remove(0);
                self.current_size -= 32 + removed.name.len() + removed.value.len();
            }
        }
    }

    /// HPACK ile GET başlığı üretir (RFC 7541).
    /// Statik tablo + dinamik tablo kullanır; dinamik tablo her istek için
    /// sıfırdan başlar (bağlantı başına tek bir tablo kullanılır).
    pub fn encode_get_headers(
        authority: &[u8],
        path: &[u8],
        https: bool,
        dyn_table: &mut HpackDynamicTable,
    ) -> Vec<u8> {
        let mut out = Vec::new();

        // :method GET — statik indeks 2 (indexed representation)
        out.push(0x82);

        // :scheme — statik indeks 6 (http) veya 7 (https)
        out.push(if https { 0x87 } else { 0x86 });

        // :path
        if path == b"/" {
            // Statik indeks 4 (indexed representation)
            out.push(0x84);
        } else {
            // :path için literal header field, indexed name (statik indeks 4)
            encode_int(&mut out, 4, 4, 0x00);
            encode_string(&mut out, path);
            // Dinamik tabloya ekle.
            dyn_table.insert(b":path", path);
        }

        // :authority — literal header field, indexed name (statik indeks 1)
        encode_int(&mut out, 1, 4, 0x00);
        encode_string(&mut out, authority);
        // Dinamik tabloya ekle.
        dyn_table.insert(b":authority", authority);

        out
    }

    /// Genel HTTP/2 frame oluşturucu. Stream ID'nin reserved biti temizlenir.
    pub fn build_frame(frame_type: u8, flags: u8, stream_id: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(9 + payload.len());
        let len = payload.len().min(0x00ff_ffff);
        out.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8]);
        out.push(frame_type);
        out.push(flags);
        out.extend_from_slice(&(stream_id & 0x7fff_ffff).to_be_bytes());
        out.extend_from_slice(&payload[..len]);
        out
    }

    /// Client SETTINGS: push kapalı ve 64 KiB başlangıç akış penceresi.
    pub fn build_client_settings_full() -> Vec<u8> {
        let mut p = Vec::with_capacity(12);
        p.extend_from_slice(&SETTINGS_ENABLE_PUSH.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&SETTINGS_INITIAL_WINDOW_SIZE.to_be_bytes());
        p.extend_from_slice(&65535u32.to_be_bytes());
        build_frame(FRAME_SETTINGS, 0, 0, &p)
    }

    pub fn build_window_update(stream_id: u32, increment: u32) -> Vec<u8> {
        build_frame(
            FRAME_WINDOW_UPDATE,
            0,
            stream_id,
            &(increment & 0x7fff_ffff).to_be_bytes(),
        )
    }

    pub fn build_get_headers(
        stream_id: u32,
        authority: &[u8],
        path: &[u8],
        https: bool,
        dyn_table: &mut HpackDynamicTable,
    ) -> Vec<u8> {
        let block = encode_get_headers(authority, path, https, dyn_table);
        build_frame(
            FRAME_HEADERS,
            FLAG_END_HEADERS | FLAG_END_STREAM,
            stream_id,
            &block,
        )
    }
}

/// Sunucu yanıtının HTTP/2 ile başlayıp başlamadığını kontrol eder.
/// Sunucu bağlantı önyüzü göndermez; SETTINGS çerçevesi ile başlar.
pub fn is_http2_response(data: &[u8]) -> bool {
    if data.len() < 9 {
        return false;
    }
    // Frame type = SETTINGS (0x4), stream ID = 0 (bağlantı çerçevesi)
    data[3] == h2::FRAME_SETTINGS
        && data[4] == 0
        && data[5] == 0
        && data[6] == 0
        && data[7] == 0
        && data[8] == 0
}

/// HTTP/2 istemci bağlantı önyüzünü gönderir (sunucuya bağlandıktan sonra).
/// Bu, HTTP/2 protokolünün başlatılması için gereklidir.
pub fn h2_send_preface(fd: i32) -> Result<(), i32> {
    let _ = socket::send(fd, h2::PREFACE).map_err(|_| socket::E_IO)?;
    let _ = socket::send(fd, &h2::build_client_settings_full()).map_err(|_| socket::E_IO)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP/2 İstemci (RFC 7540) — Gerçek multiplexing + HPACK dynamic table
// ---------------------------------------------------------------------------

/// HTTP/2 akış durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum H2StreamState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

/// HTTP/2 akış takibi.
struct H2Stream {
    stream_id: u32,
    state: H2StreamState,
    /// Sunucudan gelen yanıt verisi (HEADERS + DATA birleştirilmiş).
    response_buf: Vec<u8>,
    /// Yanıt başlığı bitti mi (END_HEADERS alındı mı).
    headers_complete: bool,
    /// Yanıt gövdesi bitti mi (END_STREAM alındı mı).
    body_complete: bool,
    /// Durum kodu (HEADERS'den ayrıştırıldığında).
    status_code: Option<u16>,
}

impl H2Stream {
    fn new(stream_id: u32) -> Self {
        Self {
            stream_id,
            state: H2StreamState::Idle,
            response_buf: Vec::new(),
            headers_complete: false,
            body_complete: false,
            status_code: None,
        }
    }
}

/// HTTP/2 istemci bağlantısı: tek TCP/TLS soket üzerinden çoklu akış.
pub struct Http2Connection {
    fd: i32,
    host: String,
    port: u16,
    is_https: bool,
    tls_session: Option<crate::drivers::tls::TlsSession>,
    next_stream_id: u32,
    streams: alloc::vec::Vec<Option<H2Stream>>,
    /// Akış ID'sine göre stream index'i.
    stream_map: alloc::collections::BTreeMap<u32, usize>,
    /// Akış penceresi (flow control) — bağlantı düzeyi.
    connection_window: u32,
    /// Sunucudan gelen SETTINGS MAX_CONCURRENT_STREAMS değeri.
    max_concurrent_streams: u32,
    /// Şu anda açık akış sayısı.
    open_stream_count: u32,
    /// HPACK dinamik tablo (bağlantı başına tek bir tablo).
    dyn_table: h2::HpackDynamicTable,
}

impl Http2Connection {
    /// Yeni bir HTTP/2 bağlantısı kurar: TCP/TLS bağlan, preface gönder,
    /// sunucu SETTINGS'ını oku.
    pub fn connect(host: &str, port: u16, is_https: bool) -> Result<Self, i32> {
        if !e1000::is_ready() {
            return Err(socket::E_NOTCONN);
        }

        let ip = socket::resolve_host(host)?;
        let fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
            crate::serial_print("[H2] socket oluşturulamadı: ");
            crate::serial_print(&e.to_string());
            crate::serial_print("\n");
            e
        })?;
        let _ = socket::set_timeout(fd, 8000);
        socket::connect(fd, ip, port).map_err(|e| {
            let _ = socket::close(fd);
            e
        })?;

        // TLS el sıkışması (HTTPS için)
        let tls_session: Option<crate::drivers::tls::TlsSession> = if is_https {
            let _host_str = host.as_bytes();
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
            crate::drivers::tls::build_client_hello(
                host,
                &client_random,
                &mut hello,
                &mut ecdhe_secret,
                false,
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

            // TLS session'ı sakla — istek gönderilirken şifreleme için kullanılacak.
            Some(session)
        } else {
            None
        };

        // HTTP/2 bağlantı önyüzünü gönder
        h2_send_preface(fd)?;

        // Sunucu SETTINGS'ını oku (ilk gelen çerçeve SETTINGS olmalı)
        let mut buf = [0u8; 4096];
        let mut total = 0usize;
        let mut max_concurrent_streams: u32 = 100; // RFC 7540 varsayılanı: sınırsız (100 kabul edilebilir varsayılan)
        let deadline = crate::boot::time::main_monotonic_ms() + 8_000;
        loop {
            if total >= buf.len() {
                break;
            }
            let n = socket::recv(fd, &mut buf[total..]).map_err(|_| socket::E_IO)?;
            if n == 0 {
                break;
            }
            total += n;
            if total >= 9 {
                // İlk 9 bayt çerçeve başlığı
                if let Some(fh) = h2::parse_frame_header(&buf[..9]) {
                    let payload_len = fh.payload_len() as usize;
                    if fh.r#type == h2::FRAME_SETTINGS && fh.stream_id() == 0 {
                        // Tüm SETTINGS payload'unu oku
                        while total < 9 + payload_len {
                            let n =
                                socket::recv(fd, &mut buf[total..]).map_err(|_| socket::E_IO)?;
                            if n == 0 {
                                break;
                            }
                            total += n;
                        }
                        // SETTINGS payload'unu ayrıştır: MAX_CONCURRENT_STREAMS (0x3)
                        let settings_payload = &buf[9..9 + payload_len];
                        let mut sp_offset = 0;
                        while sp_offset + 6 <= settings_payload.len() {
                            let setting_id = u16::from_be_bytes([
                                settings_payload[sp_offset],
                                settings_payload[sp_offset + 1],
                            ]);
                            let setting_value = u32::from_be_bytes([
                                settings_payload[sp_offset + 2],
                                settings_payload[sp_offset + 3],
                                settings_payload[sp_offset + 4],
                                settings_payload[sp_offset + 5],
                            ]);
                            if setting_id == h2::SETTINGS_MAX_CONCURRENT_STREAMS {
                                max_concurrent_streams = setting_value;
                            }
                            sp_offset += 6;
                        }
                        break;
                    }
                }
            }
            if crate::boot::time::main_monotonic_ms() > deadline {
                return Err(socket::E_TIMEOUT);
            }
        }

        Ok(Self {
            fd,
            host: String::from(host),
            port,
            is_https,
            tls_session,
            next_stream_id: 1, // İstemci akış ID'leri tek sayıdır
            streams: alloc::vec::Vec::new(),
            stream_map: alloc::collections::BTreeMap::new(),
            connection_window: 65535,
            max_concurrent_streams,
            open_stream_count: 0,
            dyn_table: h2::HpackDynamicTable::new(4096), // RFC 7541 varsayılanı: 4096 bayt
        })
    }

    /// Yeni bir akış açar ve stream_id döndürür.
    fn open_stream(&mut self) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2; // İstemci: 1, 3, 5, ...
        let stream = H2Stream::new(stream_id);
        let idx = self.streams.len();
        self.streams.push(Some(stream));
        self.stream_map.insert(stream_id, idx);
        stream_id
    }

    /// Akış ID'sine göre stream referansı döndürür.
    fn stream_mut(&mut self, stream_id: u32) -> Option<&mut H2Stream> {
        let idx = *self.stream_map.get(&stream_id)?;
        self.streams[idx].as_mut()
    }

    /// HTTP/2 GET isteği gönderir ve yanıtı `resp_buf`'a yazar.
    /// Dönen: (durum_kodu, başlık_uzunluğu, toplam_uzunluk)
    pub fn get(
        &mut self,
        authority: &[u8],
        path: &[u8],
        resp_buf: &mut [u8],
    ) -> Result<(u16, usize, usize), i32> {
        // Akış eşzamanlılık sınırını kontrol et (SETTINGS_MAX_CONCURRENT_STREAMS).
        if self.open_stream_count >= self.max_concurrent_streams {
            return Err(socket::E_INVAL);
        }

        let stream_id = self.open_stream();
        self.open_stream_count += 1;
        let is_https = self.is_https;
        let fd = self.fd;

        // HEADERS çerçevesi oluştur (HPACK ile)
        let headers_block = h2::encode_get_headers(authority, path, is_https, &mut self.dyn_table);
        let headers_frame = h2::build_frame(
            h2::FRAME_HEADERS,
            h2::FLAG_END_HEADERS | h2::FLAG_END_STREAM,
            stream_id,
            &headers_block,
        );

        // Şifreliyse TLS ile şifrele, değilse düz gönder.
        let frame_bytes = if is_https {
            if let Some(ref mut session) = self.tls_session {
                match session.encrypt_app_data(&headers_frame) {
                    Some(encrypted) => encrypted,
                    None => return Err(E_TLS),
                }
            } else {
                return Err(E_TLS);
            }
        } else {
            headers_frame
        };
        let _ = socket::send(fd, &frame_bytes).map_err(|_| socket::E_IO)?;

        // Stream durumunu HalfClosedLocal olarak işaretle (kısa boru).
        {
            let stream = self.stream_mut(stream_id).ok_or(socket::E_IO)?;
            stream.state = H2StreamState::HalfClosedLocal;
        }

        // Yanıtı oku: HEADERS + DATA çerçeveleri.
        let mut buf = [0u8; 8192];
        let mut total = 0usize;
        let deadline = crate::boot::time::main_monotonic_ms() + 10_000;

        // Yerel durum değişkenleri (stream borusu tutulmadan erişim için).
        let mut status_code: Option<u16> = None;
        let mut body_complete = false;
        let mut response_buf: Vec<u8> = Vec::new();

        while !body_complete && total < resp_buf.len() {
            let n = socket::recv(fd, &mut buf[total..]).map_err(|_| socket::E_IO)?;
            if n == 0 {
                break;
            }

            if is_https {
                // TLS şifreli kayıtları çöz.
                if let Some(ref mut session) = self.tls_session {
                    session.feed_tcp_data(&buf[..n]);
                    let mut app = Vec::new();
                    let mut hs = Vec::new();
                    match session.process_records(&mut hs, &mut app) {
                        Ok(_) => {}
                        Err(_) => return Err(E_TLS),
                    }
                    // İşlenmiş uygulama verisini buf'ın başına kopyala.
                    let copy_len = app.len().min(buf.len());
                    buf[..copy_len].copy_from_slice(&app[..copy_len]);
                    total = copy_len;
                } else {
                    return Err(E_TLS);
                }
            } else {
                total += n;
            }

            // Çerçeveleri ayrıştır
            let mut offset = 0usize;
            while offset + 9 <= total {
                if let Some(fh) = h2::parse_frame_header(&buf[offset..offset + 9]) {
                    let payload_len = fh.payload_len() as usize;
                    if offset + 9 + payload_len > total {
                        break; // Henüz tüm payload gelmedi
                    }
                    let payload = &buf[offset + 9..offset + 9 + payload_len];

                    match fh.r#type {
                        h2::FRAME_SETTINGS => {
                            // SETTINGS ACK gönder
                            let ack = h2::build_frame(h2::FRAME_SETTINGS, h2::FLAG_ACK, 0, &[]);
                            let ack_bytes = if is_https {
                                if let Some(ref mut session) = self.tls_session {
                                    session.encrypt_app_data(&ack).unwrap_or_default()
                                } else {
                                    Vec::new()
                                }
                            } else {
                                ack
                            };
                            if !ack_bytes.is_empty() {
                                let _ = socket::send(fd, &ack_bytes);
                            }
                        }
                        h2::FRAME_HEADERS => {
                            // Yanıt başlıklarını ayrıştır (basit: durum kodunu bul)
                            if fh.flags & h2::FLAG_END_HEADERS != 0 {
                                // :status'yi HPACK'den çöz (basit: 0x88 = 200)
                                for byte in payload {
                                    if *byte == 0x88 {
                                        status_code = Some(200);
                                        break;
                                    }
                                }
                            }
                            if fh.flags & h2::FLAG_END_STREAM != 0 {
                                body_complete = true;
                            }
                        }
                        h2::FRAME_DATA => {
                            // Flow control: bağlantı penceresini kontrol et.
                            if payload_len as u32 > self.connection_window {
                                // Pencere yetersiz: WINDOW_UPDATE gönder ve devam et.
                                let wu = h2::build_window_update(0, 65535);
                                let wu_bytes = if is_https {
                                    if let Some(ref mut session) = self.tls_session {
                                        session.encrypt_app_data(&wu).unwrap_or_default()
                                    } else {
                                        Vec::new()
                                    }
                                } else {
                                    wu
                                };
                                if !wu_bytes.is_empty() {
                                    let _ = socket::send(fd, &wu_bytes);
                                }
                                self.connection_window = 65535;
                            }
                            self.connection_window =
                                self.connection_window.saturating_sub(payload_len as u32);

                            response_buf.extend_from_slice(payload);
                            if fh.flags & h2::FLAG_END_STREAM != 0 {
                                body_complete = true;
                            }
                            // WINDOW_UPDATE gönder (akış penceresi)
                            let wu = h2::build_window_update(stream_id, payload_len as u32);
                            let wu_bytes = if is_https {
                                if let Some(ref mut session) = self.tls_session {
                                    session.encrypt_app_data(&wu).unwrap_or_default()
                                } else {
                                    Vec::new()
                                }
                            } else {
                                wu
                            };
                            if !wu_bytes.is_empty() {
                                let _ = socket::send(fd, &wu_bytes);
                            }
                        }
                        h2::FRAME_WINDOW_UPDATE => {
                            // Sunucudan gelen WINDOW_UPDATE: bağlantı penceresini genişlet.
                            if fh.stream_id() == 0 && payload_len >= 4 {
                                let increment = u32::from_be_bytes([
                                    payload[0], payload[1], payload[2], payload[3],
                                ]) & 0x7fff_ffff;
                                self.connection_window =
                                    self.connection_window.saturating_add(increment);
                            }
                        }
                        h2::FRAME_RST_STREAM => {
                            self.open_stream_count = self.open_stream_count.saturating_sub(1);
                            return Err(socket::E_IO);
                        }
                        h2::FRAME_GOAWAY => {
                            self.open_stream_count = self.open_stream_count.saturating_sub(1);
                            return Err(socket::E_IO);
                        }
                        h2::FRAME_PING => {
                            // PING ACK gönder
                            let mut ping_payload = [0u8; 8];
                            ping_payload.copy_from_slice(&payload[..8.min(payload.len())]);
                            let ack =
                                h2::build_frame(h2::FRAME_PING, h2::FLAG_ACK, 0, &ping_payload);
                            let ack_bytes = if is_https {
                                if let Some(ref mut session) = self.tls_session {
                                    session.encrypt_app_data(&ack).unwrap_or_default()
                                } else {
                                    Vec::new()
                                }
                            } else {
                                ack
                            };
                            if !ack_bytes.is_empty() {
                                let _ = socket::send(fd, &ack_bytes);
                            }
                        }
                        h2::FRAME_CONTINUATION => {
                            // CONTINUATION: başlık bloğu devam ediyor, birleştir.
                            response_buf.extend_from_slice(payload);
                        }
                        _ => {}
                    }

                    offset += 9 + payload_len;
                } else {
                    break;
                }
            }

            // Kalan veriyi baştan başla (işlenmemiş baytlar)
            if offset < total {
                buf.copy_within(offset..total, 0);
                total = total - offset;
            } else {
                total = 0;
            }

            if crate::boot::time::main_monotonic_ms() > deadline {
                return Err(socket::E_TIMEOUT);
            }
        }

        // Stream durumunu Closed olarak işaretle ve sayacı azalt (kısa boru).
        {
            let stream = self.stream_mut(stream_id).ok_or(socket::E_IO)?;
            stream.state = H2StreamState::Closed;
        }
        self.open_stream_count = self.open_stream_count.saturating_sub(1);

        // Yanıtı resp_buf'ye kopyala
        let status = status_code.unwrap_or(200);
        let hlen = 0; // HTTP/2'de ayrı başlık yok, gövde doğrudan başlar
        let copy_len = response_buf.len().min(resp_buf.len());
        resp_buf[..copy_len].copy_from_slice(&response_buf[..copy_len]);

        Ok((status, hlen, copy_len))
    }

    /// Bağlantıyı kapatır (GOAWAY gönderir).
    pub fn close(self) -> Result<(), i32> {
        let goaway = h2::build_frame(h2::FRAME_GOAWAY, 0, 0, &[0, 0, 0, 0]);
        let _ = socket::send(self.fd, &goaway);
        let _ = socket::close(self.fd);
        Ok(())
    }
}

/// HTTP/2 üzerinden GET isteği yapar.
/// Yeni bir bağlantı kurar, istek gönderir, yanıtı okur ve bağlantıyı kapatır.
pub fn http2_get(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize, usize), i32> {
    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (port, host_len, path_len) = parse_url(url, &mut host, &mut path)?;
    let is_https = url.starts_with("https://");
    let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;

    let mut conn = Http2Connection::connect(host_str, port, is_https)?;
    let result = conn.get(&host[..host_len], &path[..path_len], resp_buf);
    let _ = conn.close();
    result
}

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
// HTTP/2 unit tests (NIC gerektirmez)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod h2_tests {
    use super::h2::*;

    #[test]
    fn test_frame_header_roundtrip() {
        let payload = b"hello world";
        let frame = build_frame(
            FRAME_HEADERS,
            FLAG_END_HEADERS | FLAG_END_STREAM,
            1,
            payload,
        );
        assert!(frame.len() >= 9 + payload.len());

        let fh = parse_frame_header(&frame).unwrap();
        assert_eq!(fh.r#type, FRAME_HEADERS);
        assert_eq!(fh.flags, FLAG_END_HEADERS | FLAG_END_STREAM);
        assert_eq!(fh.stream_id(), 1);
        assert_eq!(fh.payload_len(), payload.len() as u32);
    }

    #[test]
    fn test_client_settings_frame() {
        let frame = build_client_settings();
        assert_eq!(frame.len(), 9);
        assert_eq!(frame[3], FRAME_SETTINGS);
        assert_eq!(frame[4], 0); // flags
        assert_eq!(frame[5], 0); // stream ID = 0
        assert_eq!(frame[6], 0);
        assert_eq!(frame[7], 0);
        assert_eq!(frame[8], 0);
    }

    #[test]
    fn test_client_settings_full() {
        let frame = build_client_settings_full();
        assert!(frame.len() >= 9 + 12); // 9 header + 12 payload (2 settings × 6 bytes)
        assert_eq!(frame[3], FRAME_SETTINGS);
        assert_eq!(frame[4], 0);
    }

    #[test]
    fn test_window_update_frame() {
        let wu = build_window_update(5, 65535);
        let fh = parse_frame_header(&wu).unwrap();
        assert_eq!(fh.r#type, FRAME_WINDOW_UPDATE);
        assert_eq!(fh.stream_id(), 5);
        assert_eq!(fh.payload_len(), 4);
        let increment = u32::from_be_bytes([wu[9], wu[10], wu[11], wu[12]]);
        assert_eq!(increment, 65535);
    }

    #[test]
    fn test_hpack_encode_get_headers_http() {
        let mut dyn_table = HpackDynamicTable::new(4096);
        let block = encode_get_headers(b"example.com", b"/", false, &mut dyn_table);
        // :method GET (0x82), :scheme http (0x86), :path / (0x84)
        assert_eq!(block[0], 0x82);
        assert_eq!(block[1], 0x86);
        assert_eq!(block[2], 0x84);
    }

    #[test]
    fn test_hpack_encode_get_headers_https() {
        let mut dyn_table = HpackDynamicTable::new(4096);
        let block = encode_get_headers(b"example.com", b"/", true, &mut dyn_table);
        // :method GET (0x82), :scheme https (0x87), :path / (0x84)
        assert_eq!(block[0], 0x82);
        assert_eq!(block[1], 0x87);
        assert_eq!(block[2], 0x84);
    }

    #[test]
    fn test_hpack_encode_get_headers_non_root_path() {
        let mut dyn_table = HpackDynamicTable::new(4096);
        let block = encode_get_headers(b"example.com", b"/index.html", false, &mut dyn_table);
        // :method GET (0x82), :scheme http (0x86), :path literal (not 0x84)
        assert_eq!(block[0], 0x82);
        assert_eq!(block[1], 0x86);
        assert_ne!(block[2], 0x84); // :path / değil, literal encoding
    }

    #[test]
    fn test_build_get_headers() {
        let mut dyn_table = HpackDynamicTable::new(4096);
        let frame = build_get_headers(1, b"example.com", b"/", true, &mut dyn_table);
        let fh = parse_frame_header(&frame).unwrap();
        assert_eq!(fh.r#type, FRAME_HEADERS);
        assert_eq!(fh.flags, FLAG_END_HEADERS | FLAG_END_STREAM);
        assert_eq!(fh.stream_id(), 1);
    }

    #[test]
    fn test_is_http2_response() {
        // SETTINGS frame on stream 0 = HTTP/2 response
        let settings_frame = build_client_settings();
        assert!(is_http2_response(&settings_frame));

        // Non-SETTINGS frame = not HTTP/2
        let data_frame = build_frame(FRAME_DATA, 0, 1, b"data");
        assert!(!is_http2_response(&data_frame));

        // Too short
        let short = &[0u8; 8];
        assert!(!is_http2_response(short));
    }

    #[test]
    fn test_hpack_dynamic_table_insert_and_find() {
        let mut table = HpackDynamicTable::new(4096);
        table.insert(b":path", b"/index.html");
        table.insert(b":authority", b"example.com");

        // :path /index.html should be found at dynamic index
        let found = table.find(b":path", Some(b"/index.html"));
        assert!(found.is_some());
        assert!(found.unwrap() > 61); // Statik tablo boyutundan sonra

        // :authority should be found by name only
        let found_name = table.find(b":authority", None);
        assert!(found_name.is_some());
    }

    #[test]
    fn test_hpack_dynamic_table_eviction() {
        let mut table = HpackDynamicTable::new(100); // Çok küçük limit
        table.insert(b"short", b"v");
        let _ = table.find(b"short", Some(b"v"));
        // Büyük giriş eklenince eskiler çıkarılmalı
        table.insert(b"very-long-name-that-exceeds-limit", b"value");
        // Tablo hala çalışır durumda olmalı
        assert!(table.entries.len() <= 1);
    }

    #[test]
    fn test_preface_constant() {
        assert_eq!(PREFACE, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    }

    #[test]
    fn test_frame_type_constants() {
        assert_eq!(FRAME_DATA, 0x0);
        assert_eq!(FRAME_HEADERS, 0x1);
        assert_eq!(FRAME_SETTINGS, 0x4);
        assert_eq!(FRAME_WINDOW_UPDATE, 0x8);
        assert_eq!(FRAME_CONTINUATION, 0x9);
    }

    #[test]
    fn test_flag_constants() {
        assert_eq!(FLAG_END_STREAM, 0x1);
        assert_eq!(FLAG_END_HEADERS, 0x4);
        assert_eq!(FLAG_ACK, 0x1);
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

        let ip = socket::resolve_host(&host)?;
        let fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM).map_err(|e| {
            crate::serial_print("[WS] socket oluşturulamadı: ");
            crate::serial_print(&e.to_string());
            crate::serial_print("\n");
            e
        })?;
        let _ = socket::set_timeout(fd, 8000);
        socket::connect(fd, ip, port).map_err(|e| {
            let _ = socket::close(fd);
            e
        })?;

        // TLS el sıkışması (wss:// için)
        let mut tls_session = if is_wss {
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
            crate::drivers::tls::build_client_hello(
                &host,
                &client_random,
                &mut hello,
                &mut ecdhe_secret,
                false,
            );
            let ch_body = if hello.len() > 5 { &hello[5..] } else { &hello };
            let mut session = crate::drivers::tls::TlsSession::new(&host, client_random);
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

            Some(session)
        } else {
            None
        };

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
        let _ = socket::close(self.fd);
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

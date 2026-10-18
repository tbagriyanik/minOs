//! Minimal HTTP istemcisi (`curl`/`http_get`): HTTP/1.1 GET, Content-Length ve
//! `Connection: close` çerçeveleme, chunked transfer çözümü, 3xx yönlendirme
//! takibi (en fazla 3 atlama). TLS (`https://`) desteklenmez ve hata döner.
#![allow(dead_code, static_mut_refs)]

use crate::drivers::e1000;
use crate::drivers::netstack::Ipv4Address;
use crate::drivers::socket;

pub const E_URL: i32 = 1001;
pub const E_TLS: i32 = 1002;
pub const E_HTTP: i32 = 1003;
pub const E_REDIRECT: i32 = 1004;

pub const MAX_REDIRECTS: usize = 3;
pub const MAX_HOST: usize = 96;
pub const MAX_PATH: usize = 256;

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
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
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
    if !buf[sp + 1].is_ascii_digit() || !buf[sp + 2].is_ascii_digit() || !buf[sp + 3].is_ascii_digit() {
        return None;
    }
    Some(d(buf[sp + 1]) * 100 + d(buf[sp + 2]) * 10 + d(buf[sp + 3]))
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
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
                && key.iter().zip(name.iter()).all(|(a, b)| a.to_ascii_lowercase() == *b)
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
    // IP literal ise Host başlığına aynen yaz; DNS adı zaten çözüldü.
    let _ = ip;
    let parts: [&[u8]; 6] = [
        b"GET ",
        path,
        b" HTTP/1.1\r\nHost: ",
        host,
        b"\r\nUser-Agent: minOs-curl/1.0\r\nConnection: close\r\n\r\n",
        b"",
    ];
    let mut n = 0usize;
    for p in parts {
        if n + p.len() > req.len() {
            return None;
        }
        req[n..n + p.len()].copy_from_slice(p);
        n += p.len();
    }
    Some(n)
}

/// Tek bir HTTP GET turu yapar: yanıtın tamamını (başlık+gövde) `resp_buf`'a
/// yazar, `(durum_kodu, başlık_uzunluğu, toplam_uzunluk)` döner.
fn fetch_once(
    host: &[u8],
    port: u16,
    path: &[u8],
    ip: Ipv4Address,
    resp_buf: &mut [u8],
) -> Result<(u16, usize, usize), i32> {
    let fd = socket::socket(socket::AF_INET, socket::SOCK_STREAM)?;
    let _ = socket::set_timeout(fd, 4000);
    let res = (|| {
        socket::connect(fd, ip, port)?;
        let mut req = [0u8; 768];
        let reqlen = build_request(&mut req, host, path, ip).ok_or(socket::E_INVAL)?;
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
            match socket::recv(fd, &mut resp_buf[total..]) {
                Ok(0) => break, // FIN = EOF
                Ok(n) => {
                    total += n;
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
        if total < 12 {
            return Err(E_HTTP);
        }
        let status = parse_status(&resp_buf[..total]).ok_or(E_HTTP)?;
        let hlen = find_header_end(&resp_buf[..total]).ok_or(E_HTTP)?;
        Ok((status, hlen, total))
    })();
    socket::close(fd);
    res
}

/// `http://` URL'sini indirir; 3xx yönlendirmeleri takip eder.
/// Dönen `(durum, başlık_uzunluğu, toplam_uzunluk)`; gövde `resp_buf[hlen..total]`
/// aralığındadır (chunked ise çözülmüş hâliyle).
pub fn http_get(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize, usize), i32> {
    if !e1000::is_ready() {
        return Err(socket::E_NOTCONN);
    }
    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (mut port, mut host_len, mut path_len) = parse_url(url, &mut host, &mut path)?;
    let mut hops = 0;
    loop {
        let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;
        let path_str = core::str::from_utf8(&path[..path_len]).map_err(|_| E_URL)?;
        let ip = socket::resolve_host(host_str)?;
        let (status, hlen, total) =
            fetch_once(&host[..host_len], port, &path[..path_len], ip, resp_buf)?;
        if (300..400).contains(&status) && hops < MAX_REDIRECTS {
            let headers = &resp_buf[..hlen];
            if let Some(loc) = header_value(headers, b"location") {
                let loc_str = core::str::from_utf8(loc).map_err(|_| E_HTTP)?;
                if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
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
    crate::serial_print(if ok { "BASARILI\n" } else { "BASARISIZ\n" });
    ok
}

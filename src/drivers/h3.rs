//! HTTP/3 (RFC 9114) + QPACK (RFC 9204) istemci katmanı.
//!
//! QUIC bağlantısı (`QuicConn`) üzerinde çalışır. Kapsam:
//! - QPACK kodlayıcı/çözücü: statik tablo (indeksli alan satırları,
//!   isim referanslı ve literal isimli alan satırları), literal dizgeler
//!   (Huffman opsiyonel — kodlama tarafı düz metin üretir, çözme tarafı
//!   HPACK Huffman'ını yeniden kullanır), dinamik tablo YOK.
//! - H3 çerçeve kodlama/ayrıştırma: DATA, HEADERS, SETTINGS, PUSH_PROMISE,
//!   GOAWAY, MAX_PUSH_ID; bilinmeyen çerçeveler atlanır.
//! - İstemci kontrol akışı (SETTINGS) ve istek akışları.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::drivers::qpack::{qpack_encode, QpackDecoder};
use crate::drivers::quic::{decode_varint, encode_varint};
use crate::drivers::quic_conn::{ConnState, QuicConn};

// ---------------------------------------------------------------------------
// HTTP/3 çerçeveleri (RFC 9114 §7)
// ---------------------------------------------------------------------------

pub const FRAME_DATA: u64 = 0x00;
pub const FRAME_HEADERS: u64 = 0x01;
pub const FRAME_SETTINGS: u64 = 0x04;
pub const FRAME_PUSH_PROMISE: u64 = 0x05;
pub const FRAME_GOAWAY: u64 = 0x07;
pub const FRAME_MAX_PUSH_ID: u64 = 0x0d;

/// H3 çerçevesi: (type, payload).
pub struct H3Frame {
    pub frame_type: u64,
    pub payload: Vec<u8>,
}

/// `data` içindeki tüm tam çerçeveleri ayrıştırır; eksikse `None` yerine
/// mevcut olanları döner ve `consumed` bayt sayısını bildirir.
/// Basit yaklaşım: çağıran taraf tam çerçeve sınırını bilir (akış
/// tamponu). Burada tüm tam çerçeveleri döndürürüz.
pub fn parse_frames(data: &[u8]) -> Result<(Vec<H3Frame>, usize), String> {
    let mut frames = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let (ft, l1) = decode_varint(&data[pos..]).ok_or_else(|| String::from("h3_type"))?;
        pos += l1;
        let (flen, l2) = decode_varint(&data[pos..]).ok_or_else(|| String::from("h3_len"))?;
        pos += l2;
        let n = flen as usize;
        if pos + n > data.len() {
            // Eksik çerçeve — kalan baytlar sonraki çağrıya
            break;
        }
        frames.push(H3Frame {
            frame_type: ft,
            payload: data[pos..pos + n].to_vec(),
        });
        pos += n;
    }
    Ok((frames, pos))
}

/// Tek çerçeve kodlar.
pub fn encode_frame(frame_type: u64, payload: &[u8], out: &mut Vec<u8>) {
    encode_varint(frame_type, out);
    encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

/// SETTINGS çerçevesi (istemci reklamları).
pub fn client_settings() -> Vec<u8> {
    // SETTINGS_QPACK_MAX_TABLE_CAPACITY(0x01)=0, MAX_FIELD_SECTION_SIZE(0x06),
    // QPACK_BLOCKED_STREAMS(0x07), ENABLE_CONNECT_PROTOCOL(0x08),
    // H3_DATAGRAM(0x33)=1
    let mut p = Vec::new();
    encode_varint(0x06, &mut p);
    encode_varint(65_536, &mut p); // max_field_section_size
    encode_varint(0x07, &mut p);
    encode_varint(16, &mut p); // qpack_blocked_streams
    encode_varint(0x33, &mut p);
    encode_varint(1, &mut p); // enable_h3_datagram
    let mut f = Vec::new();
    encode_frame(FRAME_SETTINGS, &p, &mut f);
    f
}

// ---------------------------------------------------------------------------
// H3 istemci
// ---------------------------------------------------------------------------

/// Yanıt başına durum.
pub struct H3Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub stream_id: u64,
    pub done: bool,
}

/// QUIC bağlantısı üzerinde HTTP/3 istemcisi.
pub struct H3Client {
    conn: QuicConn,
    next_stream: u64,
    control_sent: bool,
    /// Yanıt akış tamponları: stream_id -> biriken ham baytlar.
    rx: Vec<(u64, Vec<u8>)>,
    /// stream_id -> kısmi H3 frame durumu (basit: tüm baytlar birikir,
    /// her poll'da yeniden ayrıştırılır).
    responses: Vec<H3Response>,
    qpack: QpackDecoder,
}

impl H3Client {
    /// Bağlı QUIC bağlantısını sarar; kontrol akışı SETTINGS'i gönderir.
    pub fn new(conn: QuicConn) -> Result<Self, String> {
        let mut client = Self {
            conn,
            next_stream: 0,
            control_sent: false,
            rx: Vec::new(),
            responses: Vec::new(),
            qpack: QpackDecoder::new(),
        };
        client.send_control_stream()?;
        Ok(client)
    }

    /// Kontrol akışı (istemci-initiated unidirectional, stream 2) SETTINGS.
    fn send_control_stream(&mut self) -> Result<(), String> {
        if self.control_sent {
            return Ok(());
        }
        let settings = client_settings();
        // Stream id 2 = client uni (bits: 0 initiator=client? QUIC: bit0
        // initiator (0=client), bit1 direction (1=unidirectional) → 2).
        self.conn
            .send_stream_data(2, &settings, false)
            .map_err(|e| alloc::format!("ctrl: {}", e))?;
        self.control_sent = true;
        Ok(())
    }

    /// GET benzeri basit istek gönderir (form submit yok — GET/HEAD).
    /// Dönen stream_id yanıt takibi için saklanır.
    pub fn send_request(
        &mut self,
        method: &str,
        path: &str,
        authority: &str,
        extra_headers: &[(String, String)],
    ) -> Result<u64, String> {
        if !self.conn.ready() {
            return Err(String::from("quic_not_ready"));
        }
        let stream_id = self.next_stream;
        self.next_stream += 4; // client bidi stream'leri 0,4,8...

        let mut headers: Vec<(String, String)> = alloc::vec![
            (String::from(":method"), String::from(method)),
            (String::from(":scheme"), String::from("https")),
            (String::from(":authority"), String::from(authority)),
            (String::from(":path"), String::from(path)),
        ];
        headers.extend(extra_headers.iter().cloned());

        let qpack_block = qpack_encode(&headers);
        let mut stream_data = Vec::new();
        // HEADERS frame: type 0x01, payload = QPACK bloğu
        // (QPACK encoded field section; Required Insert Count=0, Base=0
        // varsayımıyla ham blok — statik tablo kullandığımız için encode
        // edilmiş alan satırları doğrudan gelir)
        encode_frame(FRAME_HEADERS, &qpack_block, &mut stream_data);

        self.conn
            .send_stream_data(stream_id, &stream_data, false)
            .map_err(|e| alloc::format!("send: {}", e))?;
        self.track_response(stream_id);
        Ok(stream_id)
    }

    /// QUIC poll + gelen stream verilerini H3 yanıtlarına çevirir.
    /// `Ok(true)` = bağlantı canlı.
    pub fn poll(&mut self) -> Result<bool, String> {
        let alive = self.conn.poll()?;
        if !alive {
            return Ok(false);
        }
        // Takip edilen istek akışlarından gelen veriyi topla
        let ids: Vec<u64> = self.responses.iter().map(|r| r.stream_id).collect();
        let mut fins: Vec<u64> = Vec::new();
        for id in ids {
            if let Some((data, fin)) = self.conn.read_stream(id) {
                if fin {
                    fins.push(id);
                }
                if !data.is_empty() {
                    self.rx.push((id, data));
                }
            }
        }
        for id in fins {
            if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == id) {
                r.done = true;
            }
        }
        self.process_rx()?;
        Ok(alive && self.conn.state() != ConnState::Failed)
    }

    /// `send_request` sonrası stream_id'yi yanıt takibine ekler.
    pub fn track_response(&mut self, stream_id: u64) {
        if !self.responses.iter().any(|r| r.stream_id == stream_id) {
            self.responses.push(H3Response {
                status: 0,
                headers: Vec::new(),
                body: Vec::new(),
                stream_id,
                done: false,
            });
        }
    }

    fn process_rx(&mut self) -> Result<(), String> {
        // Her stream için biriken baytları H3 çerçevelerine ayır
        let pending = core::mem::take(&mut self.rx);
        let mut new_rx: Vec<(u64, Vec<u8>)> = Vec::new();
        for (sid, data) in pending {
            match parse_frames(&data) {
                Ok((frames, consumed)) => {
                    if consumed < data.len() {
                        // Eksik çerçeve kuyruğu — sakla
                        new_rx.push((sid, data[consumed..].to_vec()));
                    }
                    self.handle_frames(sid, frames)?;
                }
                Err(e) => {
                    if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == sid) {
                        r.done = true;
                    }
                    let _ = e;
                }
            }
        }
        self.rx = new_rx;
        Ok(())
    }

    fn handle_frames(&mut self, sid: u64, frames: Vec<H3Frame>) -> Result<(), String> {
        let Some(resp) = self.responses.iter_mut().find(|r| r.stream_id == sid) else {
            return Ok(());
        };
        for f in frames {
            match f.frame_type {
                FRAME_HEADERS => {
                    // QPACK Required Insert Count + Delta Base (varint×2)
                    // sonra alan satırları. Statik tablo kullandığımız için
                    // genellikle 0,0.
                    let mut pos = 0usize;
                    let (_ric, l1) =
                        decode_varint(&f.payload[..]).ok_or_else(|| String::from("qpack_ric"))?;
                    pos += l1;
                    let (_dbase, l2) = decode_varint(&f.payload[pos..])
                        .ok_or_else(|| String::from("qpack_dbase"))?;
                    pos += l2;
                    let headers = self.qpack.decode_block(&f.payload[pos..])?;
                    for (n, v) in headers {
                        if n == ":status" {
                            resp.status = v.parse().unwrap_or(0);
                        } else {
                            resp.headers.push((n, v));
                        }
                    }
                }
                FRAME_DATA => {
                    resp.body.extend_from_slice(&f.payload);
                }
                FRAME_PUSH_PROMISE | FRAME_GOAWAY | FRAME_MAX_PUSH_ID => {}
                _ => {} // bilinmeyen çerçeve — yoksay
            }
        }
        Ok(())
    }

    /// Belirli bir stream'in yanıtı hazır mı?
    pub fn response_ready(&self, stream_id: u64) -> bool {
        self.responses
            .iter()
            .any(|r| r.stream_id == stream_id && (r.status != 0 || r.done))
    }

    /// Yanıtı al (varsa). `done` bayrağı stream FIN ile set edilir.
    pub fn take_response(&mut self, stream_id: u64) -> Option<H3Response> {
        let idx = self
            .responses
            .iter()
            .position(|r| r.stream_id == stream_id)?;
        if !self.responses[idx].done {
            return None;
        }
        Some(self.responses.remove(idx))
    }

    /// QUIC bağlantısının 1-RTT aşamasında olup olmadığı.
    pub fn conn_ready(&self) -> bool {
        self.conn.ready()
    }

    /// Bağlantıyı kapatır.
    pub fn close(self) {
        self.conn.close();
    }
}

// ---------------------------------------------------------------------------
// Üst düzey HTTP/3 GET (http.rs/http2.rs ile aynı kullanım kalıbı)
// ---------------------------------------------------------------------------

/// `https://` URL'sini HTTP/3 (QUIC) üzerinden GET eder.
/// `resp_buf` gövdeyi alır; dönen: (status, gövde uzunluğu).
/// Yanıt başlıkları `resp_headers` doluysa `(ad, değer)` listesi olarak
/// yazılır (küçük harfli adlar).
pub fn http3_get_full(
    url: &str,
    resp_buf: &mut [u8],
    resp_headers: &mut Vec<(String, String)>,
) -> Result<(u16, usize), i32> {
    use crate::drivers::http::{parse_url, E_URL, MAX_HOST, MAX_PATH};
    use crate::drivers::socket;

    let mut host = [0u8; MAX_HOST];
    let mut path = [0u8; MAX_PATH];
    let (port, host_len, path_len) = parse_url(url, &mut host, &mut path)?;
    if !url.starts_with("https://") {
        return Err(E_URL);
    }
    let host_str = core::str::from_utf8(&host[..host_len]).map_err(|_| E_URL)?;
    let path_str = core::str::from_utf8(&path[..path_len]).map_err(|_| E_URL)?;
    let ip = socket::resolve_host(host_str)?;

    let mut conn = QuicConn::connect(ip, port, host_str).map_err(|_| socket::E_NOTCONN)?;
    conn.set_recv_timeout(1_500);
    let mut client = H3Client::new(conn).map_err(|_| socket::E_NOTCONN)?;

    // El sıkışmanın tamamlanmasını bekle (maks ~20 sn)
    let mut ready = false;
    for _ in 0..40 {
        if client.conn_ready() {
            ready = true;
            break;
        }
        if !client.poll().map_err(|_| socket::E_IO)? {
            break;
        }
    }
    if !ready {
        client.close();
        return Err(socket::E_TIMEOUT);
    }

    let authority = if port == 443 {
        host_str.to_string()
    } else {
        alloc::format!("{}:{}", host_str, port)
    };
    let stream_id = client
        .send_request("GET", path_str, &authority, &[])
        .map_err(|_| socket::E_IO)?;

    // Yanıtı bekle (maks ~30 sn)
    let mut result: Option<H3Response> = None;
    for _ in 0..60 {
        if let Some(r) = client.take_response(stream_id) {
            result = Some(r);
            break;
        }
        if !client.poll().map_err(|_| socket::E_IO)? {
            break;
        }
    }
    client.close();
    let resp = result.ok_or(socket::E_TIMEOUT)?;

    let blen = resp.body.len().min(resp_buf.len());
    resp_buf[..blen].copy_from_slice(&resp.body[..blen]);
    resp_headers.clear();
    resp_headers.extend(resp.headers.iter().cloned());
    Ok((resp.status, blen))
}

/// `http3_get_full` basitleştirilmiş sürümü (başlıklar döndürmez).
pub fn http3_get(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize), i32> {
    let mut headers = Vec::new();
    http3_get_full(url, resp_buf, &mut headers)
}

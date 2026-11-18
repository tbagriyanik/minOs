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
//!
#![allow(static_mut_refs)] // tek çekirdekli no_std: havuz deseni http.rs ile aynı

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
    // SETTINGS_QPACK_MAX_TABLE_CAPACITY(0x01)=4096,
    // MAX_FIELD_SECTION_SIZE(0x06)=65536,
    // QPACK_BLOCKED_STREAMS(0x07)=16,
    // ENABLE_CONNECT_PROTOCOL(0x08)=0 (gönderilmiyor),
    // H3_DATAGRAM(0x33)=1
    let mut p = Vec::new();
    encode_varint(0x01, &mut p);
    encode_varint(
        crate::drivers::qpack::QPACK_MAX_TABLE_CAPACITY as u64,
        &mut p,
    );
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
    /// Trailer HEADERS alanları — `:status` içermeyen HEADERS
    /// blokları (RFC 9114 §4.1: trailer'larda pseudo-header olmaz).
    pub trailers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub stream_id: u64,
    pub done: bool,
}

/// Sunucu-initiated unidirectional stream durumu (H3 stream type
/// öneki + biriken yük).
struct UniRx {
    id: u64,
    /// H3 stream type (0x00 control, 0x02 qpack encoder, 0x03 qpack
    /// decoder); henüz ayrıştırılmadıysa None.
    stream_type: Option<u64>,
    /// Type öneki düşüldükten sonraki biriken yük.
    buf: Vec<u8>,
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
    /// Sunucu uni-stream'leri (kontrol/QPACK encoder).
    uni: Vec<UniRx>,
    /// QPACK decoder stream (istemci uni stream 6) gönderim offset'i.
    dec_offset: u64,
    /// Insert Count Increment ile bildirilen son insert sayısı.
    dec_insert_ack: u64,
    /// Dinamik tablo verisi bekleyen HEADERS yükleri: (stream_id, payload).
    pending_qpack: Vec<(u64, Vec<u8>)>,
    /// Section Ack bekleyen tamamlanmış istek stream id'leri.
    ack_pending: Vec<u64>,
    /// Sunucu kontrol akışından GOAWAY alındı mı? (H3 bağlantı
    /// yeniden kullanımı için — GOAWAY sonrası conn havuza dönmez.)
    goaway: bool,
    /// GOAWAY'nin izin verdiği son stream id.
    goaway_stream_id: u64,
    /// Sunucunun MAX_PUSH_ID değeri (server push kullanılmıyor;
    /// kayıt için saklanır).
    max_push_id: u64,
    /// Sunucu SETTINGS parametreleri (id -> değer).
    peer_settings: Vec<(u64, u64)>,
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
            uni: Vec::new(),
            dec_offset: 0,
            dec_insert_ack: 0,
            pending_qpack: Vec::new(),
            ack_pending: Vec::new(),
            goaway: false,
            goaway_stream_id: 0,
            max_push_id: u64::MAX,
            peer_settings: Vec::new(),
        };
        client.send_control_stream()?;
        client.send_decoder_stream()?;
        Ok(client)
    }

    /// Kontrol akışı (istemci-initiated unidirectional, stream 2):
    /// H3 stream type 0x00 + SETTINGS çerçevesi.
    fn send_control_stream(&mut self) -> Result<(), String> {
        if self.control_sent {
            return Ok(());
        }
        let settings = client_settings();
        // Stream id 2 = client uni (bits: 0 initiator=client? QUIC: bit0
        // initiator (0=client), bit1 direction (1=unidirectional) → 2).
        // H3 uni stream'lerin ilk baytı stream type varint'idir.
        let mut payload = Vec::new();
        encode_varint(0x00, &mut payload); // stream type: control
        payload.extend_from_slice(&settings);
        self.conn
            .send_stream_data(2, &payload, false)
            .map_err(|e| alloc::format!("ctrl: {}", e))?;
        self.control_sent = true;
        Ok(())
    }

    /// QPACK decoder stream (istemci uni stream 6, type 0x03) açar;
    /// Section Acknowledgment / Insert Count Increment buradan gider.
    fn send_decoder_stream(&mut self) -> Result<(), String> {
        let mut payload = Vec::new();
        encode_varint(0x03, &mut payload); // stream type: qpack decoder
        self.conn
            .send_stream_data_off(6, 0, &payload, false)
            .map_err(|e| alloc::format!("dec: {}", e))?;
        self.dec_offset = payload.len() as u64;
        Ok(())
    }

    /// QPACK decoder stream'ine komut ekler (Section Ack / Insert
    /// Count Increment).
    fn qpack_feedback(&mut self, section_acks: &[u64], insert_total: u64) {
        let mut buf = Vec::new();
        for &sid in section_acks {
            // Section Acknowledgment: '1' + 7-bit önekli stream id
            crate::drivers::qpack::encode_prefixed_int(sid, 7, 0x80, &mut buf);
        }
        let delta = insert_total.saturating_sub(self.dec_insert_ack);
        if delta > 0 {
            // Insert Count Increment: '00' + 6-bit önekli increment
            crate::drivers::qpack::encode_prefixed_int(delta, 6, 0x00, &mut buf);
            self.dec_insert_ack = insert_total;
        }
        if buf.is_empty() {
            return;
        }
        let off = self.dec_offset;
        if self.conn.send_stream_data_off(6, off, &buf, false).is_ok() {
            self.dec_offset += buf.len() as u64;
        }
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
        self.send_request_body(method, path, authority, extra_headers, None)
    }

    /// İstek gönderir; `body` verilirse HEADERS'tan sonra DATA frame'i
    /// eklenir ve akış FIN ile kapatılır.
    pub fn send_request_body(
        &mut self,
        method: &str,
        path: &str,
        authority: &str,
        extra_headers: &[(String, String)],
        body: Option<&[u8]>,
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
        if let Some(b) = body {
            if !headers.iter().any(|(n, _)| n == "content-length") {
                headers.push((
                    String::from("content-length"),
                    alloc::format!("{}", b.len()),
                ));
            }
        }
        headers.extend(extra_headers.iter().cloned());

        let qpack_block = qpack_encode(&headers);
        let mut stream_data = Vec::new();
        // HEADERS frame: type 0x01, payload = QPACK bloğu
        // (QPACK encoded field section; Required Insert Count=0, Base=0
        // varsayımıyla ham blok — statik tablo kullandığımız için encode
        // edilmiş alan satırları doğrudan gelir)
        encode_frame(FRAME_HEADERS, &qpack_block, &mut stream_data);
        if let Some(b) = body {
            encode_frame(FRAME_DATA, b, &mut stream_data);
        }
        // Gövde olsun ya da olmasın istek akışını FIN ile kapat
        // (sunucu yanıtı aynı akışta döner; FIN göndermek server'ın
        // "istek sonu"nu anlamasını sağlar).
        self.conn
            .send_stream_data(stream_id, &stream_data, true)
            .map_err(|e| alloc::format!("send: {}", e))?;
        self.track_response(stream_id);
        Ok(stream_id)
    }

    /// Kısa canlılık yoklaması: soket timeout'unu geçici olarak kısaltıp
    /// tek poll çalıştırır, sonra eski timeout'u geri yükler.
    /// Havuzdan alınan bağlantının hâlâ yaşadığını anlamak için kullanılır.
    pub fn poll_quick(&mut self) -> bool {
        self.conn.set_recv_timeout(30);
        let alive = self.poll().unwrap_or(false);
        self.conn.set_recv_timeout(1_500);
        alive
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
                // HEADERS işlendi ise Section Ack gönder
                if r.status != 0 && !self.ack_pending.contains(&id) {
                    self.ack_pending.push(id);
                }
            }
        }
        // Sunucu uni-stream'lerini topla (kontrol / QPACK encoder)
        self.drain_uni_streams()?;
        // QPACK encoder stream'inden gelen insert'leri işle
        self.process_uni_streams()?;
        // Bekleyen HEADERS bloklarını tekrar dene
        self.retry_pending_qpack()?;
        self.process_rx()?;
        // Tamamlanan istek akışları için Section Ack + insert bildirimi
        let acks = core::mem::take(&mut self.ack_pending);
        self.qpack_feedback(&acks, self.qpack.total_inserts());
        Ok(alive && self.conn.state() != ConnState::Failed)
    }

    /// conn'deki uni-stream verilerini (id % 4 == 3) `uni` tamponlarına
    /// taşır; stream type varint'ini ayrıştırır.
    fn drain_uni_streams(&mut self) -> Result<(), String> {
        let ids = self.conn.stream_ids();
        for id in ids {
            if id % 4 != 3 {
                continue; // yalnızca sunucu-initiated uni
            }
            let Some((data, _fin)) = self.conn.read_stream(id) else {
                continue;
            };
            if data.is_empty() {
                continue;
            }
            let idx = match self.uni.iter().position(|u| u.id == id) {
                Some(i) => i,
                None => {
                    self.uni.push(UniRx {
                        id,
                        stream_type: None,
                        buf: Vec::new(),
                    });
                    self.uni.len() - 1
                }
            };
            self.uni[idx].buf.extend_from_slice(&data);
            // Stream type varint'ini çöz (henüz yoksa)
            if self.uni[idx].stream_type.is_none() {
                if let Some((t, l)) = decode_varint(&self.uni[idx].buf) {
                    self.uni[idx].stream_type = Some(t);
                    self.uni[idx].buf.drain(..l);
                }
            }
        }
        Ok(())
    }

    /// QPACK encoder stream (type 0x02) verisini decoder'a, kontrol
    /// stream'i (type 0x00) H3 çerçevelerini ilgili işleyicilere besler.
    fn process_uni_streams(&mut self) -> Result<(), String> {
        for i in 0..self.uni.len() {
            match self.uni[i].stream_type {
                Some(0x02) if !self.uni[i].buf.is_empty() => {
                    match self.qpack.decode_encoder_stream(&self.uni[i].buf) {
                        Ok(consumed) => {
                            self.uni[i].buf.drain(..consumed);
                        }
                        Err(_) => {
                            self.uni[i].buf.clear();
                        }
                    }
                }
                Some(0x00) if !self.uni[i].buf.is_empty() => {
                    self.process_control_stream(i)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Sunucu kontrol akışı (uni stream type 0x00) üzerindeki H3
    /// çerçevelerini işler: SETTINGS, GOAWAY, MAX_PUSH_ID
    /// (RFC 9114 §6.2). Eksik çerçeve kuyruktaki baytlarda kalır.
    fn process_control_stream(&mut self, idx: usize) -> Result<(), String> {
        let buf = core::mem::take(&mut self.uni[idx].buf);
        match parse_frames(&buf) {
            Ok((frames, consumed)) => {
                for f in frames {
                    match f.frame_type {
                        FRAME_SETTINGS => self.apply_peer_settings(&f.payload),
                        FRAME_GOAWAY => {
                            self.goaway = true;
                            if let Some((id, _)) = decode_varint(&f.payload) {
                                self.goaway_stream_id = id;
                            }
                        }
                        FRAME_MAX_PUSH_ID => {
                            if let Some((id, _)) = decode_varint(&f.payload) {
                                self.max_push_id = id;
                            }
                        }
                        _ => {} // CANCEL_PUSH / PRIORITY_UPDATE vb. — yoksay
                    }
                }
                let mut rest = buf;
                rest.drain(..consumed);
                self.uni[idx].buf = rest;
            }
            Err(_) => {
                // Bozuk/eksik başlık — daha fazla veri bekle.
                self.uni[idx].buf = buf;
            }
        }
        Ok(())
    }

    /// Sunucu SETTINGS parametrelerini (id, value varint çiftleri)
    /// `peer_settings`'e yazar; tekrar gelenler üzerine yazılır.
    fn apply_peer_settings(&mut self, payload: &[u8]) {
        let mut pos = 0usize;
        while pos < payload.len() {
            let Some((id, l1)) = decode_varint(&payload[pos..]) else {
                break;
            };
            pos += l1;
            let Some((val, l2)) = decode_varint(&payload[pos..]) else {
                break;
            };
            pos += l2;
            match self.peer_settings.iter_mut().find(|(k, _)| *k == id) {
                Some(e) => e.1 = val,
                None => self.peer_settings.push((id, val)),
            }
        }
    }

    /// Sunucunun bildirdiği bir SETTINGS parametresi (yoksa `None`).
    pub fn peer_setting(&self, id: u64) -> Option<u64> {
        self.peer_settings
            .iter()
            .find(|(k, _)| *k == id)
            .map(|(_, v)| *v)
    }

    /// Sunucu GOAWAY gönderdi mi veya bağlantı kapandı mı?
    /// `false` ise bağlantı havuza geri konabilir.
    pub fn is_closing(&self) -> bool {
        self.goaway || self.conn.state() == ConnState::Failed
    }

    /// `qpack_pending` ile ertelenen HEADERS yüklerini tekrar dener.
    fn retry_pending_qpack(&mut self) -> Result<(), String> {
        let pending = core::mem::take(&mut self.pending_qpack);
        for (sid, payload) in pending {
            match self.qpack.decode_block(&payload) {
                Ok(headers) => {
                    self.apply_header_block(sid, headers)?;
                }
                Err(ref e) if e == "qpack_pending" => {
                    self.pending_qpack.push((sid, payload));
                }
                Err(_) => {
                    if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == sid) {
                        r.done = true;
                    }
                }
            }
        }
        Ok(())
    }

    /// Çözülmüş başlıkları ilgili yanıta uygular.
    fn apply_headers(&mut self, sid: u64, headers: Vec<(String, String)>) -> Result<(), String> {
        let Some(resp) = self.responses.iter_mut().find(|r| r.stream_id == sid) else {
            return Ok(());
        };
        for (n, v) in headers {
            if n == ":status" {
                let new_status: u16 = v.parse().unwrap_or(0);
                // Interim 1xx yanıtı final yanıtla değiştirildiğinde
                // önceki (1xx) başlıkları at — RFC 9114 §4.1.
                if (100..200).contains(&resp.status) && !(100..200).contains(&new_status) {
                    resp.headers.clear();
                }
                resp.status = new_status;
            } else {
                resp.headers.push((n, v));
            }
        }
        Ok(())
    }

    /// Çözülmüş HEADERS bloğunu başlık ya da trailer olarak sınıflandırır:
    /// `:status` içeren bloklar yanıt başlıklarıdır (interim 1xx dahil),
    /// içermeyenler trailer'dır (RFC 9114 §4.1 — trailer'larda pseudo-header
    /// alanı bulunmaz).
    fn apply_header_block(
        &mut self,
        sid: u64,
        headers: Vec<(String, String)>,
    ) -> Result<(), String> {
        if headers.iter().any(|(n, _)| n == ":status") {
            return self.apply_headers(sid, headers);
        }
        if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == sid) {
            for (n, v) in headers {
                r.trailers.push((n, v));
            }
        }
        Ok(())
    }

    /// `send_request` sonrası stream_id'yi yanıt takibine ekler.
    pub fn track_response(&mut self, stream_id: u64) {
        if !self.responses.iter().any(|r| r.stream_id == stream_id) {
            self.responses.push(H3Response {
                status: 0,
                headers: Vec::new(),
                trailers: Vec::new(),
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
        if !self.responses.iter().any(|r| r.stream_id == sid) {
            return Ok(());
        }
        for f in frames {
            match f.frame_type {
                FRAME_HEADERS => {
                    // QPACK payload = prefix (RIC+Base) + alan satırları;
                    // decode_block prefix'i kendisi çözer.
                    match self.qpack.decode_block(&f.payload) {
                        Ok(headers) => {
                            self.apply_header_block(sid, headers)?;
                        }
                        Err(ref e) if e == "qpack_pending" => {
                            self.pending_qpack.push((sid, f.payload.clone()));
                        }
                        Err(_) => {
                            if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == sid)
                            {
                                r.done = true;
                            }
                        }
                    }
                }
                FRAME_DATA => {
                    if let Some(r) = self.responses.iter_mut().find(|r| r.stream_id == sid) {
                        r.body.extend_from_slice(&f.payload);
                    }
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
// H3 bağlantı havuzu: aynı host'a tekrar isteklerde QUIC bağlantısı
// yeniden kullanılır (TCP keep-alive havuzunun HTTP/3 karşılığı).
// ---------------------------------------------------------------------------

/// Havuzdaki bir H3 istemcisi ve ait olduğu yetki (authority).
struct PooledH3 {
    host: String,
    port: u16,
    client: H3Client,
    last_used: u64,
}

static mut H3_POOL: [Option<PooledH3>; 4] = [const { None }; 4];

/// Havuzda boşta kalabilecek en uzun süre — reklam edilen QUIC
/// `max_idle_timeout`'un (30 sn) altında tutulur.
const H3_POOL_IDLE_MS: u64 = 20_000;

/// `(host, port)` için havuzda bekleyen H3 istemcisini çıkarır.
/// Bayat girdiler düşürülür; bulunan istemcinin canlılığı çağıran
/// tarafından yoklanır.
fn h3_pool_acquire(host: &str, port: u16) -> Option<H3Client> {
    unsafe {
        let now = crate::monotonic_ms();
        for entry in H3_POOL.iter_mut() {
            if entry
                .as_ref()
                .is_some_and(|p| now.wrapping_sub(p.last_used) > H3_POOL_IDLE_MS)
            {
                entry.take(); // drop → soket kapanır
            }
        }
        for entry in H3_POOL.iter_mut() {
            if entry
                .as_ref()
                .is_some_and(|p| p.host == host && p.port == port)
            {
                return entry.take().map(|p| p.client);
            }
        }
    }
    None
}

/// İstemciyi havuza geri koyar. GOAWAY alınmış veya bağlantısı
/// kapanmış istemciler havuza dönmez (drop → soket kapanır).
fn h3_pool_release(host: &str, port: u16, client: H3Client) {
    if client.is_closing() {
        return;
    }
    unsafe {
        let now = crate::monotonic_ms();
        let mut empty = None;
        let mut oldest = 0usize;
        let mut oldest_t = u64::MAX;
        for (i, entry) in H3_POOL.iter().enumerate() {
            match entry {
                None => {
                    empty = Some(i);
                    break;
                }
                Some(p) => {
                    if p.last_used < oldest_t {
                        oldest_t = p.last_used;
                        oldest = i;
                    }
                }
            }
        }
        let idx = empty.unwrap_or(oldest);
        H3_POOL[idx] = Some(PooledH3 {
            host: String::from(host),
            port,
            client,
            last_used: now,
        });
    }
}

/// `livetest_insecure` cmdline token'ı var mı? (Yalnızca gömülü
/// test sunucusuna karşı sertifika doğrulamasını atlatmak için.)
#[cfg(not(test))]
fn livetest_insecure_enabled() -> bool {
    crate::boot::limine::limine_boot_cmdline()
        .map(|c| c.split_whitespace().any(|t| t == "livetest_insecure"))
        .unwrap_or(false)
}

#[cfg(test)]
fn livetest_insecure_enabled() -> bool {
    false
}

/// Yeni QUIC bağlantısı kurar, el sıkışmanın bitmesini bekler ve
/// H3 istemcisini döndürür. Kontrol akışı SETTINGS'i 1-RTT anahtarı
/// gerektirdiğinden `H3Client::new` ancak `conn.ready()` sonrası
/// çağrılabilir.
fn h3_connect(
    ip: crate::drivers::netstack::Ipv4Address,
    port: u16,
    host: &str,
) -> Result<H3Client, i32> {
    use crate::drivers::socket;
    let mut conn = QuicConn::connect(ip, port, host).map_err(|_| socket::E_NOTCONN)?;
    conn.set_recv_timeout(1_500);
    if livetest_insecure_enabled() {
        conn.tls_mut().set_skip_cert_validation(true);
    }
    let mut ready = false;
    for _ in 0..40 {
        if conn.ready() {
            ready = true;
            break;
        }
        if !conn.poll().map_err(|_| socket::E_IO)? {
            break;
        }
    }
    if !ready {
        conn.close();
        return Err(socket::E_TIMEOUT);
    }
    H3Client::new(conn).map_err(|_| socket::E_NOTCONN)
}

/// `https://` URL'sini HTTP/3 (QUIC) üzerinden istek yapar.
/// `resp_buf` gövdeyi alır; dönen: (status, gövde uzunluğu).
/// Yanıt başlıkları `resp_headers` doluysa `(ad, değer)` listesi olarak
/// yazılır (küçük harfli adlar). `body` verilirse DATA frame'i gönderilir.
/// Aynı host'a tekrar isteklerde QUIC bağlantısı havuzdan yeniden
/// kullanılır; GOAWAY/kapanan bağlantılar havuza dönmez.
pub fn http3_request_full(
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    req_headers: &[(String, String)],
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

    // Havuzdan al; yoksa ya da bağlantı öldüyse yeni kur.
    let mut client = match h3_pool_acquire(host_str, port) {
        Some(mut c) => {
            // Kısa canlılık yoklaması: sunucu bağlantıyı kapattıysa
            // (CONNECTION_CLOSE) burada yakalanır.
            if c.poll_quick() {
                c
            } else {
                c.close();
                h3_connect(ip, port, host_str)?
            }
        }
        None => h3_connect(ip, port, host_str)?,
    };

    let authority = if port == 443 {
        host_str.to_string()
    } else {
        alloc::format!("{}:{}", host_str, port)
    };
    let result = h3_do_request(
        &mut client,
        method,
        path_str,
        &authority,
        req_headers,
        body,
        resp_buf,
        resp_headers,
    );
    // Başarılı olsun olmasın bağlantıyı havuza geri ver —
    // release() GOAWAY/kapanma durumunu denetler.
    h3_pool_release(host_str, port, client);
    result
}

/// Tek bir H3 isteği gönderir ve yanıtı bekler; `client` üzerinde
/// çalışır (havuz tarafından edinilmiş/kurulmuş olmalı).
fn h3_do_request(
    client: &mut H3Client,
    method: &str,
    path: &str,
    authority: &str,
    req_headers: &[(String, String)],
    body: Option<&[u8]>,
    resp_buf: &mut [u8],
    resp_headers: &mut Vec<(String, String)>,
) -> Result<(u16, usize), i32> {
    use crate::drivers::socket;
    let stream_id = client
        .send_request_body(method, path, authority, req_headers, body)
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
    let resp = result.ok_or(socket::E_TIMEOUT)?;

    let blen = resp.body.len().min(resp_buf.len());
    resp_buf[..blen].copy_from_slice(&resp.body[..blen]);
    resp_headers.clear();
    resp_headers.extend(resp.headers.iter().cloned());
    Ok((resp.status, blen))
}

/// `https://` URL'sini HTTP/3 (QUIC) üzerinden GET eder.
pub fn http3_get_full(
    url: &str,
    resp_buf: &mut [u8],
    resp_headers: &mut Vec<(String, String)>,
) -> Result<(u16, usize), i32> {
    http3_request_full("GET", url, None, &[], resp_buf, resp_headers)
}

/// `http3_get_full` basitleştirilmiş sürümü (başlıklar döndürmez).
pub fn http3_get(url: &str, resp_buf: &mut [u8]) -> Result<(u16, usize), i32> {
    let mut headers = Vec::new();
    http3_get_full(url, resp_buf, &mut headers)
}

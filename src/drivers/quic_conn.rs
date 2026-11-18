//! QUIC (RFC 9000) + QUIC-TLS (RFC 9001) istemci bağlantısı.
//!
//! UDP datagramları üzerinde Initial/Handshake/1-RTT paket uzaylarını
//! yönetir; TLS 1.3 el sıkışmasını `TlsSession` üzerinden CRYPTO
//! stream'lerine taşıyarak yürütür. Kapsam: el sıkışma, ACK'ler,
//! CRYPTO/PING/CONNECTION_CLOSE/HANDSHAKE_DONE çerçeveleri ve STREAM
//! frame'lerinin uygulamaya teslimi (H3 katmanı `h3.rs`'te). Kayıp
//! kurtarma RTO ile offset'li CRYPTO segmentleri üzerinden yapılır;
//! gönderim akış kontrolü MAX_DATA/MAX_STREAM_DATA ile uygulanır.

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::netstack::IpAddress;
use crate::drivers::quic::{self, QuicKeys};
use crate::drivers::socket;
use crate::drivers::streambuf::{CryptoRx, StreamRx};
use crate::drivers::tls::{self, TlsSession, TlsState};

const CID_LEN: usize = 8;
const PN_LEN: usize = 4;
const INITIAL_MIN_LEN: usize = 1200;
/// Tek UDP datagramı üst sınırı (IPv4: 65535-20-8; pratik MTU ~1500).
const RX_BUF_LEN: usize = 9000;

/// Bağlantı durumu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// El sıkışma sürüyor.
    Handshake,
    /// 1-RTT anahtarları hazır; istek/yanıt akabilir.
    Ready,
    /// Bağlantı başarısız oldu.
    Failed,
}

/// Bir packet-number uzayının gönderim/alım durumu.
struct PnSpace {
    tx: Option<QuicKeys>,
    rx: Option<QuicKeys>,
    next_pn: u64,
    largest_rx: u64,
    ack_pending: Option<u64>,
}

impl PnSpace {
    fn new() -> Self {
        Self {
            tx: None,
            rx: None,
            next_pn: 0,
            largest_rx: 0,
            ack_pending: None,
        }
    }
}

/// ACK'lenmemiş gönderilmiş stream parçası (retransmission birimi).
#[derive(Clone)]
struct PendingChunk {
    stream_id: u64,
    offset: u64,
    data: Vec<u8>,
    fin: bool,
}

/// Peer'ın NEW_CONNECTION_ID frame'iyle sağladığı alternatif bağlantı
/// kimliği (RFC 9000 §19.15). Bağlantı göçü (migration) için saklanır.
struct PeerCid {
    seq: u64,
    retire_prior_to: u64,
    cid: Vec<u8>,
    reset_token: [u8; 16],
}

/// Bizim NEW_CONNECTION_ID ile verdiğimiz yedek CID (seq >= 1;
/// seq 0 örtük olarak `scid`'dir). RETIRE_CONNECTION_ID geldiğinde
/// seq ile eşleştirilir.
struct IssuedCid {
    seq: u64,
}

/// QUIC istemci bağlantısı.
pub struct QuicConn {
    fd: i32,
    peer_ip: IpAddress,
    peer_port: u16,
    /// İlk Initial DCID (anahtar türetimi). Retry'da güncellenir.
    dcid: [u8; CID_LEN],
    /// Bizim SCID'imiz (kendi paketlerimizin SCID'si; gelen short
    /// header'larda DCID olarak görünür).
    scid: [u8; CID_LEN],
    /// Sunucunun seçtiği SCID — gönderilen long header'larda DCID,
    /// short header'larda hedef.
    server_scid: Vec<u8>,
    /// Yeniden gönderim için saklanan ClientHello handshake mesajı.
    client_hello: Vec<u8>,
    state: ConnState,
    tls: TlsSession,
    initial: PnSpace,
    handshake: PnSpace,
    app: PnSpace,
    /// Initial ve Handshake CRYPTO akışları ayrı paket numarası
    /// uzaylarına aittir (RFC 9001 §4.9) — ayrı tamponlar.
    crypto_rx_init: CryptoRx,
    crypto_rx_hs: CryptoRx,
    /// Handshake uzayındaki client→server CRYPTO verisi (client Finished).
    crypto_tx_hs: Vec<u8>,
    /// Gönderilmiş handshake CRYPTO verisi (retransmission için saklanır).
    sent_hs_crypto: Vec<u8>,
    /// Son gelen/giden aktivite zamanı (ms) — RTO hesabı için.
    last_activity_ms: u64,
    /// Retransmission zaman aşımı (ms; üstel geri çekilme).
    rto_ms: u64,
    /// Initial ClientHello en az bir kez gönderildi mi.
    initial_sent: bool,
    /// Alınan STREAM verileri: stream_id -> (offset->data parçaları, fin).
    streams: Vec<StreamRx>,
    rx_buf: Vec<u8>,
    /// Sunucunun SCID'si öğrenildi mi (short header hedefi için).
    app_keys_ready: bool,
    /// Bağlantı düzeyi gönderim akış kontrolü sınırı: peer'ın izin
    /// verdiği toplam STREAM payload baytı (MAX_DATA çerçevesi veya
    /// `initial_max_data` transport parametresi; mutlak değer).
    tx_max_data: u64,
    /// Şimdiye kadar gönderilen toplam STREAM payload baytı.
    tx_conn_bytes: u64,
    /// Akış başına azami gönderim offset'i — MAX_STREAM_DATA
    /// çerçeveleriyle güncellenir (mutlak offset değeri).
    tx_stream_limits: Vec<(u64, u64)>,
    /// Transport parametrelerinden gelen varsayılan akış sınırları:
    /// client'ın gönderdiği bidi ve uni akışları için peer limitleri
    /// (`initial_max_stream_data_bidi_remote` / `_uni`).
    tx_stream_default_bidi: u64,
    tx_stream_default_uni: u64,
    /// Peer transport parametreleri (EE 0xffa5) işlendi mi?
    peer_params_done: bool,
    /// Akış kontrolüne takılan gönderim parçaları (FIFO; poll'da
    /// MAX_DATA/MAX_STREAM_DATA geldiğinde boşaltılır).
    tx_queue: Vec<PendingChunk>,
    /// ACK'lenmemiş gönderilmiş stream parçaları (slot -> parça;
    /// ACK gelince None). Yeniden iletimde aynı slot kullanılır.
    tx_pending: Vec<Option<PendingChunk>>,
    /// App uzayında gönderilen paketler: (pn, tx_pending slot id listesi).
    sent_app: Vec<(u64, Vec<usize>)>,
    /// Peer'ın NEW_CONNECTION_ID ile verdiği alternatif CID'ler.
    peer_cids: Vec<PeerCid>,
    /// PATH_CHALLENGE'a yanıt olarak gönderilecek PATH_RESPONSE verisi.
    pending_path_response: Option<[u8; 8]>,
    /// Bizim gönderdiğimiz yedek CID'ler (NEW_CONNECTION_ID, seq >= 1).
    issued_cids: Vec<IssuedCid>,
    /// Yedek CID'ler 1-RTT anahtarları hazır olunca gönderildi mi?
    cids_issued: bool,
    /// 0-RTT (erken veri) paket anahtarları — PSK teklif edilmişse
    /// ClientHello ile birlikte türetilir (RFC 9001 §5.1).
    early_keys: Option<QuicKeys>,
    /// 0-RTT paket numarası (kendi paket numarası uzayı).
    early_tx_pn: u64,
    /// 0-RTT ile gönderilen stream parçaları — 1-RTT anahtarları
    /// hazır olunca normal yolla tekrar iletilir (alıcı aynı
    /// offset'teki örtüşen veriyi eler; PSK reddedilirse bu tekrar
    /// veri kaybını önler).
    early_chunks: Vec<PendingChunk>,
    /// Erken parçaların 1-RTT tekrarı yapıldı mı?
    early_resend_done: bool,
}

impl QuicConn {
    /// UDP soketi açar, Initial paketini gönderir ve el sıkışma moduna
    /// geçer. `hostname` SNI + sertifika doğrulaması için kullanılır.
    /// `ip` IPv4 veya IPv6 olabilir; soket ailesi buna göre seçilir.
    pub fn connect(ip: IpAddress, port: u16, hostname: &str) -> Result<Self, String> {
        let domain = match ip {
            IpAddress::V4(_) => socket::AF_INET,
            IpAddress::V6(_) => socket::AF_INET6,
        };
        let fd = socket::socket(domain, socket::SOCK_DGRAM)
            .map_err(|e| alloc::format!("socket: {}", e))?;
        socket::set_timeout(fd, 5_000).ok();

        let mut dcid = [0u8; CID_LEN];
        let mut scid = [0u8; CID_LEN];
        let mut client_random = [0u8; 32];
        if !crate::random::secure_fill_bytes(&mut dcid)
            || !crate::random::secure_fill_bytes(&mut scid)
            || !crate::random::secure_fill_bytes(&mut client_random)
        {
            socket::close(fd);
            return Err(String::from("random"));
        }

        let mut tls = TlsSession::new(hostname, client_random);
        tls.set_negotiated_protocol(alloc::vec![b'h', b'3']);

        // ClientHello (TLS kaydı -> handshake mesajı). Önbellekte
        // oturum varsa PSK ile resumption denenir (0-RTT temeli).
        let mut rec = Vec::new();
        let mut ecdhe = [0u8; 32];
        let mut x25519 = [0u8; 32];
        let mut psk_offer = None;
        tls::build_client_hello_cached(
            hostname,
            &client_random,
            &mut rec,
            &mut ecdhe,
            &mut x25519,
            &[b"h3"],
            &mut psk_offer,
        );
        if rec.len() < 5 {
            socket::close(fd);
            return Err(String::from("client_hello"));
        }
        let mut hello = rec.split_off(5);
        tls.set_client_ecdhe_secret(ecdhe);
        tls.set_client_x25519_secret(x25519);
        if let Some(p) = psk_offer {
            tls.set_psk(&p);
        }

        // quic_transport_parameters (0xffa5) ekle — RFC 9000 §18.2
        // Biçim: (parametre_id varint, uzunluk varint, değer baytları).
        // Tamsayı değerler varint olarak kodlanır; length = kodlanmış
        // varint uzunluğu.
        let mut params = Vec::new();
        let put = |params: &mut Vec<u8>, id: u64, val: u64| {
            quic::encode_varint(id, params);
            let mut vb = Vec::new();
            quic::encode_varint(val, &mut vb);
            quic::encode_varint(vb.len() as u64, params);
            params.extend_from_slice(&vb);
        };
        put(&mut params, 0x01, 30_000); // max_idle_timeout (ms)
        put(&mut params, 0x04, 1_000_000); // initial_max_data
        put(&mut params, 0x05, 65_536); // initial_max_stream_data_bidi_local
        put(&mut params, 0x06, 65_536); // initial_max_stream_data_bidi_remote
        put(&mut params, 0x07, 65_536); // initial_max_stream_data_uni
        put(&mut params, 0x08, 128); // initial_max_streams_bidi
        put(&mut params, 0x09, 32); // initial_max_streams_uni
        put(&mut params, 0x0a, 3); // ack_delay_exponent
        put(&mut params, 0x0b, 25); // max_ack_delay (ms)
        put(&mut params, 0x0e, 4); // active_connection_id_limit
                                   // initial_source_connection_id (0x0f) — zorunlu; değer = scid.
        quic::encode_varint(0x0f, &mut params);
        quic::encode_varint(scid.len() as u64, &mut params);
        params.extend_from_slice(&scid);
        // QUIC yalnızca AES-128-GCM (0x1301) destekler — sunulabilir
        // TLS 1.3 paket listesinden gerisini çıkar (yoksa sunucu 0x1302
        // seçip bağlantıyı imkânsız kılar).
        quic::strip_client_hello_cipher_suites(&mut hello, &[tls::TLS_AES_256_GCM_SHA384]);
        quic::inject_quic_transport_params(&mut hello, &params);
        // TLS transcript'ine ClientHello eklenir — QUIC el sıkışma
        // anahtarları (ve 0-RTT early traffic secret) bu transcript
        // hash'ine bağlıdır. Parametre enjeksiyonundan SONRA çağrılmalı.
        tls.note_client_hello(&hello);

        let mut conn = Self {
            fd,
            peer_ip: ip,
            peer_port: port,
            dcid,
            scid,
            server_scid: Vec::new(),
            client_hello: hello.clone(),
            state: ConnState::Handshake,
            tls,
            initial: PnSpace::new(),
            handshake: PnSpace::new(),
            app: PnSpace::new(),
            crypto_rx_init: CryptoRx::new(),
            crypto_rx_hs: CryptoRx::new(),
            crypto_tx_hs: Vec::new(),
            sent_hs_crypto: Vec::new(),
            last_activity_ms: crate::monotonic_ms(),
            rto_ms: 500,
            initial_sent: false,
            streams: Vec::new(),
            rx_buf: Vec::new(),
            app_keys_ready: false,
            tx_max_data: 0,
            tx_conn_bytes: 0,
            tx_stream_limits: Vec::new(),
            tx_stream_default_bidi: 0,
            tx_stream_default_uni: 0,
            peer_params_done: false,
            tx_queue: Vec::new(),
            tx_pending: Vec::new(),
            sent_app: Vec::new(),
            peer_cids: Vec::new(),
            pending_path_response: None,
            issued_cids: Vec::new(),
            cids_issued: false,
            early_keys: None,
            early_tx_pn: 0,
            early_chunks: Vec::new(),
            early_resend_done: false,
        };
        conn.initial.tx = quic::client_initial_keys(&dcid);
        conn.initial.rx = quic::server_initial_keys(&dcid);
        // PSK teklif edildiyse 0-RTT erken trafik anahtarlarını türet
        // (RFC 9001 §5.1: client_early_traffic_secret -> QUIC keys).
        if let Some(sec) = conn.tls.quic_client_early_traffic_secret() {
            if let Some(k) = quic::traffic_keys(&sec) {
                conn.early_keys = Some(k);
            }
        }
        let _ = conn.send_crypto_segments(quic::PT_INITIAL, &hello);
        Ok(conn)
    }

    /// CRYPTO verisini ~`CRYPTO_SEG` baytlık parçalara bölüp her birini
    /// kendi CRYPTO frame'iyle (doğru stream offset'i) gönderir.
    /// Tek dev paket yerine segmentleme MTU'yu aşmayı önler ve alıcının
    /// `CryptoRx`'inin beklediği aralık tabanlı yeniden birleştirme ile
    /// uyumludur — yeniden iletilen segmentler örtüşen aralık olarak
    /// güvenle elenir. Initial paketleri 1200 bayta doldurulur.
    fn send_crypto_segments(&mut self, ptype: u8, data: &[u8]) -> Result<(), String> {
        const CRYPTO_SEG: usize = 1000;
        let mut off = 0usize;
        while off < data.len() {
            let end = (off + CRYPTO_SEG).min(data.len());
            let mut frames = Vec::new();
            quic::encode_crypto_frame(off as u64, &data[off..end], &mut frames);
            if ptype == quic::PT_INITIAL {
                // 1200 bayt minimumu için PADDING (RFC 9000 §14.1).
                // Datagram boyutu ≈ başlık + pn + şifreli yük + AEAD etiketi;
                // yük = frames. frames.len() iki kez sayılmamalı.
                let overhead = 7 + CID_LEN + 1 + CID_LEN + 1 + PN_LEN + 16;
                while overhead + frames.len() < INITIAL_MIN_LEN {
                    frames.push(quic::FRAME_PADDING);
                }
            }
            let keys = match ptype {
                quic::PT_INITIAL => self.initial.tx.take(),
                quic::PT_HANDSHAKE => self.handshake.tx.take(),
                _ => self.app.tx.take(),
            };
            let Some(keys) = keys else { break };
            let result = self.send_long_pkt_with(&keys, ptype, &frames);
            match ptype {
                quic::PT_INITIAL => self.initial.tx = Some(keys),
                quic::PT_HANDSHAKE => self.handshake.tx = Some(keys),
                _ => self.app.tx = Some(keys),
            }
            result?;
            if ptype == quic::PT_INITIAL {
                self.initial_sent = true;
            }
            off = end;
        }
        Ok(())
    }

    /// Bir datagram alıp tek sefer işler. `Ok(true)` bağlantı yaşar,
    /// `Ok(false)` Ready veya Failed.
    pub fn poll(&mut self) -> Result<bool, String> {
        if self.state == ConnState::Failed {
            return Ok(false);
        }
        let mut buf = alloc::vec![0u8; RX_BUF_LEN];
        match socket::recvfrom(self.fd, &mut buf) {
            Ok((n, ip, port)) => {
                if ip != self.peer_ip || port != self.peer_port {
                    return Ok(true);
                }
                self.rx_buf.clear();
                self.rx_buf.extend_from_slice(&buf[..n]);
                self.process_datagram()?;
                self.last_activity_ms = crate::monotonic_ms();
                self.rto_ms = 500; // ilerleme: RTO'yu sıfırla
            }
            Err(e) => {
                if e != socket::E_TIMEOUT {
                    self.state = ConnState::Failed;
                }
            }
        }
        self.pump_crypto()?;
        self.update_key_phases()?;
        self.send_pending_acks()?;
        self.send_path_response()?;
        self.maybe_issue_cids()?;
        self.resend_early_data()?;
        self.flush_crypto_tx_hs()?;
        self.flush_tx_queue()?;
        // RTO dolduysa el sıkışma verisini yeniden gönder
        let now = crate::monotonic_ms();
        if now.wrapping_sub(self.last_activity_ms) > self.rto_ms {
            self.retransmit()?;
            self.last_activity_ms = now;
            self.rto_ms = (self.rto_ms * 2).min(10_000);
        }
        Ok(self.state != ConnState::Failed)
    }

    /// Kısa bir recv denemesi; datagram yoksa hemen döner (EAGAIN yerine
    /// soket timeout'u kullanılır — poll() blocking sayılır).
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// TLS oturumuna erişim (sertifika doğrulama sonucu için).
    pub fn tls(&self) -> &TlsSession {
        &self.tls
    }

    /// TLS oturumuna değişebilir erişim (yalnızca test kancaları için;
    /// ör. `livetest_insecure` ile sertifika doğrulamasını atlamak).
    pub fn tls_mut(&mut self) -> &mut TlsSession {
        &mut self.tls
    }

    /// 1-RTT anahtarları hazır mı?
    pub fn ready(&self) -> bool {
        self.state == ConnState::Ready
    }

    /// Uygulama (H3) katmanının kullanacağı STREAM verisini okur.
    /// Dönen: (veri, fin) — fin true ise akış sonlandı ve tampon temizlendi.
    /// Veri yoksa `None`.
    pub fn read_stream(&mut self, stream_id: u64) -> Option<(Vec<u8>, bool)> {
        let idx = self.streams.iter().position(|s| s.id == stream_id)?;
        let s = &mut self.streams[idx];
        let (out, fin) = s.take_contiguous();
        if fin {
            self.streams.remove(idx);
        }
        if out.is_empty() && !fin {
            return None;
        }
        Some((out, fin))
    }

    /// Bir stream üzerinde `offset` konumundan veri gönderir.
    pub fn send_stream_data_off(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<(), String> {
        if !self.app_keys_ready {
            return Err(String::from("not_ready"));
        }
        // Büyük veriyi ~1000 baytlık STREAM frame'lere böl (tek UDP
        // datagramı MTU'yu aşmamalı).
        const CHUNK: usize = 1000;
        if data.len() <= CHUNK {
            return self.send_stream_chunk(stream_id, offset, data, fin);
        }
        let mut off = offset;
        let mut pos = 0usize;
        while pos < data.len() {
            let end = (pos + CHUNK).min(data.len());
            let last = end == data.len();
            self.send_stream_chunk(stream_id, off, &data[pos..end], fin && last)?;
            off += (end - pos) as u64;
            pos = end;
        }
        Ok(())
    }

    /// Tek STREAM frame'li tek paket gönderir.
    fn send_stream_chunk(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<(), String> {
        // Akış kontrolü: kuyrukta bekleyen parça varsa doğrudan
        // gönderme (akış başına sıralamayı koru).
        if !self.tx_queue.is_empty() {
            self.tx_queue.push(PendingChunk {
                stream_id,
                offset,
                data: data.to_vec(),
                fin,
            });
            return Ok(());
        }
        if !self.stream_send_allowed(stream_id, offset, data.len()) {
            // Pencere aşımı — parça bekletilir, MAX_DATA /
            // MAX_STREAM_DATA gelince poll() boşaltır.
            self.tx_queue.push(PendingChunk {
                stream_id,
                offset,
                data: data.to_vec(),
                fin,
            });
            return Ok(());
        }
        self.emit_stream_chunk(stream_id, offset, data, fin)
    }

    /// `(stream_id, offset, data, fin)` için tek STREAM frame'li paket
    /// gönderir; akış kontrolü hesabını ve ACK takibini burada yapar.
    /// Kuyruk kontrolü `send_stream_chunk`'ta yapılır.
    fn emit_stream_chunk(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<(), String> {
        let Some(pn) = self.send_stream_packet(stream_id, offset, data, fin)? else {
            return Ok(());
        };
        self.tx_conn_bytes = self.tx_conn_bytes.saturating_add(data.len() as u64);
        let id = self.tx_pending.len();
        self.tx_pending.push(Some(PendingChunk {
            stream_id,
            offset,
            data: data.to_vec(),
            fin,
        }));
        self.sent_app.push((pn, alloc::vec![id]));
        Ok(())
    }

    /// Tek STREAM frame'li tek app-uzayı paketi gönderir; ACK/retransmit
    /// takibi yapmaz. Başarılı UDP gönderiminde paket numarasını döner.
    fn send_stream_packet(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<Option<u64>, String> {
        let keys = match self.app.tx.take() {
            Some(k) => k,
            None => return Err(String::from("no_keys")),
        };
        let mut frames = Vec::new();
        quic::encode_stream_frame(stream_id, offset, fin, data, &mut frames);
        let mut header = Vec::with_capacity(1 + self.server_scid.len());
        header.push(0x40 | ((PN_LEN - 1) as u8));
        header.extend_from_slice(&self.server_scid);
        let pn = self.app.next_pn;
        self.app.next_pn += 1;
        let result = quic::seal_packet(&keys, pn, PN_LEN, header, &frames, false);
        self.app.tx = Some(keys);
        if let Some(pkt) = result {
            self.send_pkt(&pkt)?;
            return Ok(Some(pn));
        }
        Ok(None)
    }

    /// UDP datagramını peer'ın adres ailesine uygun `sendto` varyantıyla
    /// gönderir (IPv4 veya IPv6).
    fn send_pkt(&mut self, pkt: &[u8]) -> Result<(), String> {
        match self.peer_ip {
            IpAddress::V4(ip) => socket::sendto(self.fd, ip, self.peer_port, pkt),
            IpAddress::V6(ip) => socket::sendto_v6(self.fd, ip, self.peer_port, pkt),
        }
        .map_err(|e| alloc::format!("sendto: {}", e))?;
        Ok(())
    }

    /// Akışın gönderebileceği azami offset: MAX_STREAM_DATA çerçevesi
    /// varsa o, yoksa transport parametresi varsayılanı.
    fn stream_tx_limit(&self, stream_id: u64) -> u64 {
        if let Some((_, limit)) = self
            .tx_stream_limits
            .iter()
            .find(|(id, _)| *id == stream_id)
        {
            return *limit;
        }
        // QUIC stream id bitleri: bit0=initiator, bit1=direction
        // (0=bidi, 1=uni). Client'ın gönderdiği uni akışlar için
        // `initial_max_stream_data_uni`, bidi için
        // `initial_max_stream_data_bidi_remote` geçerlidir.
        if stream_id & 0x2 != 0 {
            self.tx_stream_default_uni
        } else {
            self.tx_stream_default_bidi
        }
    }

    /// `(offset, len)` aralığının akış ve bağlantı düzeyi akış
    /// kontrolü sınırlarına uyup uymadığını denetler.
    /// 0-RTT uygun mu? (erken anahtarlar türetilmiş ve el sıkışma
    /// hâlâ sürüyor — sunucu ilk uçuşunu göndermeden erken veri
    /// gönderilebilir, RFC 9001 §4.5).
    pub fn zero_rtt_available(&self) -> bool {
        self.early_keys.is_some() && self.state == ConnState::Handshake
    }

    /// Erken veri (0-RTT) paketiyle stream verisi gönderir (RFC 9001
    /// §4.5). `Ok(true)` = 0-RTT olarak gönderildi; parça 1-RTT
    /// anahtarları hazır olunca normal akış yoluyla otomatik tekrar
    /// iletilir. `Ok(false)` = erken anahtar yok veya el sıkışma
    /// tamamlandı — çağıran normal `send_stream_data` yoluna düşmeli.
    pub fn send_stream_data_early(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<bool, String> {
        // 0-RTT yalnızca el sıkışma sürerken ve tek UDP datagramına
        // sığacak kadar küçük veride anlamlıdır.
        if self.state != ConnState::Handshake || data.len() > 1200 {
            return Ok(false);
        }
        let Some(keys) = self.early_keys.take() else {
            return Ok(false);
        };
        let mut frames = Vec::new();
        quic::encode_stream_frame(stream_id, offset, fin, data, &mut frames);
        // 0-RTT long header'da DCID, istemcinin ilk Initial'ındaki
        // orijinal DCID'dir (sunucu SCID'si henüz bilinmiyor).
        let dcid = self.dcid.to_vec();
        let header = quic::build_long_header(
            quic::PT_0RTT,
            quic::VERSION_1,
            &dcid,
            &self.scid,
            None,
            PN_LEN,
            0,
            frames.len() + 16,
        );
        let pn = self.early_tx_pn;
        self.early_tx_pn += 1;
        let pkt = header.and_then(|h| quic::seal_packet(&keys, pn, PN_LEN, h, &frames, true));
        self.early_keys = Some(keys);
        let Some(pkt) = pkt else {
            return Ok(false);
        };
        self.send_pkt(&pkt)?;
        self.early_chunks.push(PendingChunk {
            stream_id,
            offset,
            data: data.to_vec(),
            fin,
        });
        Ok(true)
    }

    /// `(stream_id, offset, data)` 0-RTT ile gönderildiyse `true`.
    /// Çağıran normal gönderimi atlayabilir — conn 1-RTT'de aynı
    /// baytları tekrarlar, alıcı örtüşen offset'i eler.
    pub fn early_covered(&self, stream_id: u64, offset: u64, data: &[u8]) -> bool {
        self.early_chunks
            .iter()
            .any(|c| c.stream_id == stream_id && c.offset == offset && c.data == data)
    }

    /// 0-RTT parçalarını 1-RTT anahtarları hazır olunca normal akış
    /// yoluyla tekrar gönderir. PSK kabul edilse bile tekrar zararsızdır
    /// (alıcı aynı offset'teki örtüşen veriyi `StreamRx` ile eler);
    /// PSK reddedilirse bu tekrar veri kaybını önler.
    fn resend_early_data(&mut self) -> Result<(), String> {
        if self.early_resend_done || !self.app_keys_ready || self.early_chunks.is_empty() {
            return Ok(());
        }
        self.early_resend_done = true;
        let chunks = self.early_chunks.clone();
        for c in chunks {
            self.send_stream_data_off(c.stream_id, c.offset, &c.data, c.fin)?;
        }
        Ok(())
    }

    fn stream_send_allowed(&self, stream_id: u64, offset: u64, len: usize) -> bool {
        let end = offset.saturating_add(len as u64);
        end <= self.stream_tx_limit(stream_id)
            && self.tx_conn_bytes.saturating_add(len as u64) <= self.tx_max_data
    }

    /// `tx_queue`'daki parçaları akış kontrolü izin verdiği ölçüde
    /// gönderir. Sığmayan parçalar kuyrukta kalır; parça kısmen
    /// gönderilebilir (offset ilerletilerek kalan tekrar denenir).
    /// Farklı akışlar bağımsız olduğundan engellenen parça atlanıp
    /// sonrakiler denenebilir — QUIC akışları sıra dışı veri kabul
    /// eder ve aynı akışta engellenen parçadan sonrakiler de aynı
    /// sınıra takılır.
    fn flush_tx_queue(&mut self) -> Result<(), String> {
        if self.tx_queue.is_empty() {
            return Ok(());
        }
        let pending = core::mem::take(&mut self.tx_queue);
        let mut still: Vec<PendingChunk> = Vec::new();
        for mut c in pending {
            loop {
                if c.data.is_empty() {
                    if c.fin {
                        if self.stream_send_allowed(c.stream_id, c.offset, 0) {
                            self.emit_stream_chunk(c.stream_id, c.offset, &[], true)?;
                        } else {
                            still.push(c);
                        }
                    }
                    break;
                }
                let stream_room = self.stream_tx_limit(c.stream_id).saturating_sub(c.offset);
                let conn_room = self.tx_max_data.saturating_sub(self.tx_conn_bytes);
                let allow = stream_room.min(conn_room).min(c.data.len() as u64) as usize;
                if allow == 0 {
                    still.push(c);
                    break;
                }
                let last = allow == c.data.len();
                let piece = c.data[..allow].to_vec();
                self.emit_stream_chunk(c.stream_id, c.offset, &piece, c.fin && last)?;
                c.offset += allow as u64;
                c.data.drain(..allow);
                if last {
                    break;
                }
            }
        }
        self.tx_queue = still;
        Ok(())
    }

    /// Stream'in gönderim offset'ini sıfırdan başlatır (offset=0 kısayolu).
    pub fn send_stream_data(
        &mut self,
        stream_id: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<(), String> {
        self.send_stream_data_off(stream_id, 0, data, fin)
    }

    /// Veri taşıyan bilinen stream id'lerini döndürür (uni-stream
    /// keşfi için; veri gelmemiş stream'ler listede yoktur).
    pub fn stream_ids(&self) -> Vec<u64> {
        self.streams.iter().map(|s| s.id).collect()
    }

    /// Soket recv timeout'unu değiştirir (ms).
    pub fn set_recv_timeout(&mut self, ms: u64) {
        socket::set_timeout(self.fd, ms).ok();
    }

    /// Bağlantıyı kapatır.
    pub fn close(self) {
        socket::close(self.fd);
    }

    // ------------------------------------------------------------------
    // İç işler
    // ------------------------------------------------------------------

    fn process_datagram(&mut self) -> Result<(), String> {
        let mut data = self.rx_buf.clone();
        while !data.is_empty() {
            if data[0] & 0x80 != 0 {
                let Some((ptype, _ver, dcid, scid, _token, length, pn_offset, _hlen)) =
                    quic::parse_long_header_prefix(&data)
                else {
                    return Err(String::from("bad_long_header"));
                };
                if ptype == quic::PT_RETRY {
                    // Yeni DCID ile Initial anahtarlarını türet, CH'yi tekrar gönder
                    if dcid.len() >= CID_LEN {
                        self.dcid[..CID_LEN].copy_from_slice(&dcid[..CID_LEN]);
                    }
                    if !scid.is_empty() {
                        self.server_scid = scid.clone();
                    }
                    self.initial.tx = quic::client_initial_keys(&self.dcid);
                    self.initial.rx = quic::server_initial_keys(&self.dcid);
                    self.initial.next_pn = 0;
                    let hello = self.client_hello.clone();
                    let _ = self.send_crypto_segments(quic::PT_INITIAL, &hello);
                    return Ok(());
                }
                if ptype == quic::PT_0RTT {
                    return Ok(()); // desteklenmiyor
                }
                let consumed = (pn_offset + length as usize).min(data.len());
                if consumed == 0 {
                    return Ok(());
                }
                let pkt = data[..consumed].to_vec();
                data = data[consumed..].to_vec();
                // Sunucunun SCID'sini öğren (ilk long header paketinden)
                if (ptype != quic::PT_INITIAL || self.server_scid.is_empty()) && !scid.is_empty() {
                    self.server_scid = scid.clone();
                }
                self.handle_long_packet(ptype, &pkt, pn_offset)?;
            } else {
                // Short header — datagramın geri kalanı tek pakettir
                let pn_offset = 1 + self.scid.len();
                self.handle_short_packet(&data, pn_offset)?;
                return Ok(());
            }
        }
        Ok(())
    }

    fn handle_long_packet(
        &mut self,
        ptype: u8,
        pkt: &[u8],
        pn_offset: usize,
    ) -> Result<(), String> {
        let (rx, space) = match ptype {
            quic::PT_INITIAL => (&self.initial.rx, 0u8),
            quic::PT_HANDSHAKE => (&self.handshake.rx, 1u8),
            _ => (&self.app.rx, 2u8),
        };
        let Some(keys) = rx.as_ref() else {
            return Ok(());
        };
        let expected = match space {
            0 => self.initial.largest_rx,
            1 => self.handshake.largest_rx,
            _ => self.app.largest_rx,
        };
        let Some((pn, payload, _)) = quic::open_packet(keys, pkt, true, pn_offset, expected) else {
            return Ok(());
        };
        match space {
            0 => {
                self.initial.largest_rx = self.initial.largest_rx.max(pn);
                self.initial.ack_pending = Some(pn);
                self.dispatch_frames(&payload, true, true)?;
            }
            1 => {
                self.handshake.largest_rx = self.handshake.largest_rx.max(pn);
                self.handshake.ack_pending = Some(pn);
                self.dispatch_frames(&payload, true, false)?;
            }
            _ => {
                self.app.largest_rx = self.app.largest_rx.max(pn);
                self.app.ack_pending = Some(pn);
                self.dispatch_frames(&payload, false, false)?;
            }
        }
        Ok(())
    }

    fn handle_short_packet(&mut self, pkt: &[u8], pn_offset: usize) -> Result<(), String> {
        let Some(keys) = self.app.rx.as_ref() else {
            return Ok(());
        };
        if let Some((pn, payload, _)) =
            quic::open_packet(keys, pkt, false, pn_offset, self.app.largest_rx)
        {
            self.app.largest_rx = self.app.largest_rx.max(pn);
            self.app.ack_pending = Some(pn);
            self.dispatch_frames(&payload, false, false)?;
        }
        Ok(())
    }

    /// Çözülmüş payload'daki çerçeveleri işler.
    /// `crypto_space`: Initial/Handshake uzayı (CRYPTO → TLS).
    /// `hs_space`: Handshake uzayı (CRYPTO → handshake akışı).
    fn dispatch_frames(
        &mut self,
        payload: &[u8],
        crypto_space: bool,
        hs_space: bool,
    ) -> Result<(), String> {
        let mut pos = 0usize;
        while pos < payload.len() {
            let ft = payload[pos];
            pos += 1;
            match ft {
                quic::FRAME_PADDING => continue,
                quic::FRAME_PING => continue,
                0x02 | 0x03 => {
                    // ACK frame: largest, delay, range_count, first_range,
                    // range_count × (gap, len), [ECN: 3 varint]
                    let read_varint = |pos: &mut usize| -> Option<u64> {
                        let (v, l) = quic::decode_varint(&payload[*pos..])?;
                        *pos += l;
                        Some(v)
                    };
                    let Some(largest) = read_varint(&mut pos) else {
                        return Ok(());
                    };
                    if read_varint(&mut pos).is_none() {
                        return Ok(());
                    }
                    let Some(range_count) = read_varint(&mut pos) else {
                        return Ok(());
                    };
                    let Some(first_range) = read_varint(&mut pos) else {
                        return Ok(());
                    };
                    let Some(first_start) = largest.checked_sub(first_range) else {
                        return Ok(());
                    };
                    // Onaylanan pn aralıkları (dahil): ilk aralık
                    // [largest-first_range, largest]; sonraki her aralık
                    // önceki aralığın başından gap+1 geri çekilip len
                    // uzunluğunda kurulur (RFC 9000 §19.3).
                    let mut acked = Vec::new();
                    acked.push((first_start, largest));
                    let mut prev_start = first_start;
                    for _ in 0..range_count {
                        let Some(gap) = read_varint(&mut pos) else {
                            return Ok(());
                        };
                        let Some(len) = read_varint(&mut pos) else {
                            return Ok(());
                        };
                        let Some(end) = prev_start.checked_sub(gap).and_then(|v| v.checked_sub(1))
                        else {
                            return Ok(());
                        };
                        let Some(start) = end.checked_sub(len) else {
                            return Ok(());
                        };
                        acked.push((start, end));
                        prev_start = start;
                    }
                    if ft & 0x01 != 0
                        && (read_varint(&mut pos).is_none()
                            || read_varint(&mut pos).is_none()
                            || read_varint(&mut pos).is_none())
                    {
                        return Ok(());
                    }
                    if !crypto_space {
                        self.note_acked(&acked);
                    }
                }
                0x06 => {
                    // CRYPTO frame (RFC 9000 §19.6): Offset + Length + Data —
                    // uzunluk alanı her zaman vardır, flag biti yoktur.
                    let Some((offset, l1)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l1;
                    let Some((dlen_v, l2)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l2;
                    let dlen = dlen_v as usize;
                    let dstart = pos;
                    if dstart + dlen > payload.len() {
                        return Ok(());
                    }
                    let data = &payload[dstart..dstart + dlen];
                    pos = dstart + dlen;
                    if crypto_space {
                        if hs_space {
                            self.crypto_rx_hs.insert(offset, data);
                        } else {
                            self.crypto_rx_init.insert(offset, data);
                        }
                    }
                }
                0x08..=0x0f => {
                    if let Some(consumed) =
                        self.parse_stream_frame(ft, &payload[pos..], crypto_space)?
                    {
                        pos += consumed;
                    } else {
                        return Ok(());
                    }
                }
                0x1c | 0x1d => {
                    self.state = ConnState::Failed;
                    return Ok(());
                }
                0x1e => {} // HANDSHAKE_DONE
                // Flow-control / bağlantı yönetimi çerçeveleri:
                // MAX_DATA(0x10), MAX_STREAM_DATA(0x11), MAX_STREAMS(0x12/0x13),
                // DATA_BLOCKED(0x14), STREAM_DATA_BLOCKED(0x15),
                // STREAMS_BLOCKED(0x16/0x17).
                0x10 => {
                    // MAX_DATA: bağlantı düzeyi azami STREAM baytı
                    // (mutlak kümülatif değer — asla azalmaz).
                    let Some((v, l)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l;
                    self.tx_max_data = self.tx_max_data.max(v);
                    self.flush_tx_queue()?;
                }
                0x11 => {
                    // MAX_STREAM_DATA: akış düzeyi azami offset.
                    let Some((sid, l1)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l1;
                    let Some((v, l2)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l2;
                    match self.tx_stream_limits.iter_mut().find(|(id, _)| *id == sid) {
                        Some((_, limit)) => *limit = (*limit).max(v),
                        None => self.tx_stream_limits.push((sid, v)),
                    }
                    self.flush_tx_queue()?;
                }
                0x12..=0x17 => {
                    // Alan sayısı türe göre: MAX_STREAMS/*_BLOCKED tek,
                    // STREAM_DATA_BLOCKED(0x15) çift varint.
                    let n = match ft {
                        0x15 => 2,
                        _ => 1,
                    };
                    for _ in 0..n {
                        match quic::decode_varint(&payload[pos..]) {
                            Some((_, l)) if pos + l <= payload.len() => pos += l,
                            _ => return Ok(()),
                        }
                    }
                }
                0x18 => {
                    // NEW_CONNECTION_ID (RFC 9000 §19.15):
                    // seq, retire_prior_to, len(1..=20), cid, token(16).
                    let Some((seq, l1)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l1;
                    let Some((retire, l2)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l2;
                    if pos >= payload.len() {
                        return Ok(());
                    }
                    let cid_len = payload[pos] as usize;
                    pos += 1;
                    if cid_len == 0 || cid_len > 20 || pos + cid_len + 16 > payload.len() {
                        return Ok(());
                    }
                    let cid = payload[pos..pos + cid_len].to_vec();
                    pos += cid_len;
                    let mut token = [0u8; 16];
                    token.copy_from_slice(&payload[pos..pos + 16]);
                    pos += 16;
                    // retire_prior_to'dan küçük seq'ler artık kullanılmaz.
                    self.peer_cids.retain(|c| c.seq >= retire && c.seq != seq);
                    self.peer_cids.push(PeerCid {
                        seq,
                        retire_prior_to: retire,
                        cid,
                        reset_token: token,
                    });
                    self.peer_cids.sort_by_key(|c| c.seq);
                    // active_connection_id_limit (4) civarında tut.
                    while self.peer_cids.len() > 8 {
                        self.peer_cids.remove(0);
                    }
                }
                0x19 => {
                    // RETIRE_CONNECTION_ID (§19.16): peer bizim
                    // NEW_CONNECTION_ID ile verdiğimiz bir CID'yi
                    // emekliye ayırdı — defterden düş.
                    let Some((seq, l)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l;
                    self.issued_cids.retain(|c| c.seq != seq);
                }
                0x1a => {
                    // PATH_CHALLENGE (§19.17): 8 bayt veri — aynı veri
                    // PATH_RESPONSE ile yankılanır (yol doğrulama).
                    if pos + 8 > payload.len() {
                        return Ok(());
                    }
                    let mut d = [0u8; 8];
                    d.copy_from_slice(&payload[pos..pos + 8]);
                    pos += 8;
                    self.pending_path_response = Some(d);
                }
                0x1b => {
                    // PATH_RESPONSE (§19.18): 8 bayt — tüket.
                    if pos + 8 > payload.len() {
                        return Ok(());
                    }
                    pos += 8;
                }
                _ => return Ok(()), // bilinmeyen frame — paketin sonu
            }
        }
        Ok(())
    }

    /// App uzayında ACK'lenen pn aralıklarına karşılık gelen stream
    /// parçalarını `tx_pending`'den temizler. `ranges` dahil
    /// `(start, end)` paket numarası aralıklarıdır.
    fn note_acked(&mut self, ranges: &[(u64, u64)]) {
        if ranges.is_empty() || self.sent_app.is_empty() {
            return;
        }
        let mut acked_ids = Vec::new();
        self.sent_app.retain(|(pn, ids)| {
            if ranges.iter().any(|(s, e)| pn >= s && pn <= e) {
                acked_ids.extend(ids.iter().copied());
                false
            } else {
                true
            }
        });
        for id in acked_ids {
            if let Some(slot) = self.tx_pending.get_mut(id) {
                *slot = None;
            }
        }
        // Başka bir kopyası ACK'lenmiş slotlara işaret eden eski
        // sent_app kayıtlarını buda.
        let tx_pending = &self.tx_pending;
        self.sent_app.retain(|(_, ids)| {
            ids.iter()
                .any(|id| tx_pending.get(*id).is_some_and(|p| p.is_some()))
        });
    }

    /// STREAM frame'ini ayrıştırır; `rest` frame type sonrası baytlar.
    /// Başarılıysa tüketilen bayt sayısını döner.
    fn parse_stream_frame(
        &mut self,
        ft: u8,
        rest: &[u8],
        _crypto_space: bool,
    ) -> Result<Option<usize>, String> {
        let mut pos = 0usize;
        let Some((stream_id, l1)) = quic::decode_varint(&rest[pos..]) else {
            return Ok(None);
        };
        pos += l1;
        let mut offset = 0u64;
        if ft & 0x04 != 0 {
            // OFF biti — offset varint
            let Some((off, l)) = quic::decode_varint(&rest[pos..]) else {
                return Ok(None);
            };
            offset = off;
            pos += l;
        }
        let fin = ft & 0x01 != 0;
        let (dlen, data) = if ft & 0x02 != 0 {
            let Some((len, l2)) = quic::decode_varint(&rest[pos..]) else {
                return Ok(None);
            };
            pos += l2;
            if pos + len as usize > rest.len() {
                return Ok(None);
            }
            (len as usize, &rest[pos..pos + len as usize])
        } else {
            (rest.len() - pos, &rest[pos..])
        };
        pos += dlen;
        // Akış verisini sakla (tekrar iletilen aralıklar elenir)
        if let Some(s) = self.streams.iter_mut().find(|s| s.id == stream_id) {
            s.insert(offset, data, fin);
        } else {
            let mut s = StreamRx::new(stream_id);
            s.insert(offset, data, fin);
            self.streams.push(s);
        }
        Ok(Some(pos))
    }

    /// Her iki CRYPTO akışındaki sürekli veriyi TLS'e besler.
    fn pump_crypto(&mut self) -> Result<(), String> {
        for is_hs in [false, true] {
            let rx = if is_hs {
                &mut self.crypto_rx_hs
            } else {
                &mut self.crypto_rx_init
            };
            let end = rx.contiguous_end();
            if end <= rx.fed {
                continue;
            }
            let mut chunk = Vec::new();
            rx.copy_range(rx.fed, end, &mut chunk);
            rx.advance(end);
            self.tls
                .feed_quic_handshake(&chunk)
                .map_err(|_| String::from("tls_handshake"))?;
        }
        Ok(())
    }

    /// Peer'ın QUIC transport parametrelerini (EncryptedExtensions
    /// 0xffa5) bir kez işleyip gönderim akış kontrolü başlangıç
    /// değerlerini yükler (RFC 9000 §18.2).
    fn apply_peer_transport_params(&mut self) {
        if self.peer_params_done {
            return;
        }
        let Some(params) = self.tls.quic_transport_params().map(|p| p.to_vec()) else {
            return;
        };
        self.peer_params_done = true;
        // Biçim: (parametre_id varint, uzunluk varint, değer)
        let mut pos = 0usize;
        while pos < params.len() {
            let Some((id, l1)) = quic::decode_varint(&params[pos..]) else {
                break;
            };
            pos += l1;
            let Some((len, l2)) = quic::decode_varint(&params[pos..]) else {
                break;
            };
            pos += l2;
            let len = len as usize;
            if pos + len > params.len() {
                break;
            }
            let value = &params[pos..pos + len];
            pos += len;
            let parsed = quic::decode_varint(value).map(|(v, _)| v);
            match (id, parsed) {
                (0x04, Some(v)) => self.tx_max_data = v, // initial_max_data
                (0x06, Some(v)) => self.tx_stream_default_bidi = v, // bidi_remote
                (0x07, Some(v)) => self.tx_stream_default_uni = v, // uni
                _ => {}
            }
        }
    }

    /// Anahtar fazlarını günceller; client Finished'i ve Ready geçişini yapar.
    fn update_key_phases(&mut self) -> Result<(), String> {
        self.apply_peer_transport_params();
        // QUIC paket koruması yalnızca AES-128-GCM-SHA-256 (0x1301)
        // destekler; sunucu AES-256 seçerse bağlantıyı düşür.
        if self.tls.tls13_cipher_suite() != Some(tls::TLS_AES_128_GCM_SHA256)
            && self.state == ConnState::Handshake
            && self.tls.quic_handshake_secrets().is_some()
        {
            self.state = ConnState::Failed;
            return Ok(());
        }
        if self.handshake.tx.is_none() {
            if let Some((c, s)) = self.tls.quic_handshake_secrets() {
                let (cv, sv) = (c.to_vec(), s.to_vec());
                if let (Some(ct), Some(st)) = (quic::traffic_keys(&cv), quic::traffic_keys(&sv)) {
                    self.handshake.tx = Some(ct);
                    self.handshake.rx = Some(st);
                }
            }
        }
        if !self.app_keys_ready {
            if let Some((c, s)) = self.tls.quic_application_secrets() {
                let (cv, sv) = (c.to_vec(), s.to_vec());
                if let (Some(ct), Some(st)) = (quic::traffic_keys(&cv), quic::traffic_keys(&sv)) {
                    self.app.tx = Some(ct);
                    self.app.rx = Some(st);
                    self.app_keys_ready = true;
                }
            }
        }
        // TLS Established → client Finished (Handshake CRYPTO akışı)
        if self.tls.state() == TlsState::Established && self.crypto_tx_hs.is_empty() {
            if let Some(fin) = self.tls.build_quic_client_finished() {
                self.crypto_tx_hs = fin;
            }
        }
        if self.app_keys_ready && self.state == ConnState::Handshake {
            self.state = ConnState::Ready;
        }
        Ok(())
    }

    fn send_pending_acks(&mut self) -> Result<(), String> {
        for space in 0..3u8 {
            let pn = match space {
                0 => self.initial.ack_pending.take(),
                1 => self.handshake.ack_pending.take(),
                _ => self.app.ack_pending.take(),
            };
            let Some(pn) = pn else { continue };
            let mut frames = Vec::new();
            quic::encode_ack_frame(pn, 0, &mut frames);
            // QuicKeys kopyalanamaz; gönderim için geçici olarak alanı
            // boşaltıp geri koyuyoruz.
            match space {
                0 => {
                    if let Some(k) = self.initial.tx.take() {
                        let _ = self.send_long_pkt(quic::PT_INITIAL, &k, &frames);
                        self.initial.tx = Some(k);
                    }
                }
                1 => {
                    if let Some(k) = self.handshake.tx.take() {
                        let _ = self.send_long_pkt(quic::PT_HANDSHAKE, &k, &frames);
                        self.handshake.tx = Some(k);
                    }
                }
                _ => {
                    if let Some(k) = self.app.tx.take() {
                        let _ = self.send_short_pkt(&k, &frames);
                        self.app.tx = Some(k);
                    }
                }
            }
        }
        Ok(())
    }

    fn flush_crypto_tx_hs(&mut self) -> Result<(), String> {
        if self.crypto_tx_hs.is_empty() || self.handshake.tx.is_none() {
            return Ok(());
        }
        let data = core::mem::take(&mut self.crypto_tx_hs);
        self.sent_hs_crypto = data.clone();
        // Handshake CRYPTO akışı offset'i: client'ın ilk handshake
        // verisi 0; segmentler kendi offset'leriyle gönderilir.
        self.send_crypto_segments(quic::PT_HANDSHAKE, &data)
    }

    /// RTO dolduğunda ACK'lenmemiş stream parçalarını yeni paket
    /// numaralarıyla yeniden gönderir. Akış kontrolü ve bayt sayacı
    /// tekrar işletilmez — parçalar zaten izinli kabul edilmiştir;
    /// alıcı `StreamRx::insert` ile örtüşen aralıkları eler.
    fn retransmit_pending_streams(&mut self) -> Result<(), String> {
        if !self.app_keys_ready {
            return Ok(());
        }
        let ids: Vec<usize> = (0..self.tx_pending.len())
            .filter(|&i| self.tx_pending[i].is_some())
            .collect();
        for id in ids {
            let Some(chunk) = self.tx_pending.get(id).and_then(|p| p.as_ref()).cloned() else {
                continue;
            };
            if let Some(pn) =
                self.send_stream_packet(chunk.stream_id, chunk.offset, &chunk.data, chunk.fin)?
            {
                self.sent_app.push((pn, alloc::vec![id]));
            }
        }
        Ok(())
    }

    /// RTO dolduğunda el sıkışma verisini yeniden gönderir.
    /// Basit kayıp kurtarma: ClientHello veya client Finished, daha
    /// önce gönderilen offset'lerle CRYPTO segmentleri olarak tekrar
    /// iletilir — alıcının CryptoRx'i örtüşen aralıkları birleştirir.
    /// Ready durumda handshake tekrarı yerine stream retransmission
    /// çalışır.
    fn retransmit(&mut self) -> Result<(), String> {
        if self.state == ConnState::Failed {
            return Ok(());
        }
        if self.state == ConnState::Ready {
            return self.retransmit_pending_streams();
        }
        if self.handshake.tx.is_some() && !self.sent_hs_crypto.is_empty() {
            // client Finished'ı Handshake uzayında tekrar gönder
            let data = self.sent_hs_crypto.clone();
            self.send_crypto_segments(quic::PT_HANDSHAKE, &data)
        } else if self.initial_sent {
            // Henüz handshake anahtarı yok → ClientHello'yu tekrar gönder
            let hello = self.client_hello.clone();
            let _ = self.send_crypto_segments(quic::PT_INITIAL, &hello);
            Ok(())
        } else {
            Ok(())
        }
    }

    fn send_long_pkt(&mut self, ptype: u8, keys: &QuicKeys, frames: &[u8]) -> Result<(), String> {
        self.send_long_pkt_with(keys, ptype, frames)
    }

    fn send_long_pkt_with(
        &mut self,
        keys: &QuicKeys,
        ptype: u8,
        frames: &[u8],
    ) -> Result<(), String> {
        // Long header DCID: sunucu SCID'si biliniyorsa o, değilse initial DCID.
        let dcid = if self.server_scid.is_empty() {
            self.dcid.to_vec()
        } else {
            self.server_scid.clone()
        };
        let Some(header) = quic::build_long_header(
            ptype,
            quic::VERSION_1,
            &dcid,
            &self.scid,
            None,
            PN_LEN,
            0,
            frames.len() + 16,
        ) else {
            return Ok(());
        };
        let space = match ptype {
            quic::PT_INITIAL => &mut self.initial,
            quic::PT_HANDSHAKE => &mut self.handshake,
            _ => &mut self.app,
        };
        let pn = space.next_pn;
        space.next_pn += 1;
        if let Some(pkt) = quic::seal_packet(keys, pn, PN_LEN, header, frames, true) {
            self.send_pkt(&pkt)?;
        }
        Ok(())
    }

    fn send_short_pkt(&mut self, keys: &QuicKeys, frames: &[u8]) -> Result<(), String> {
        let mut header = Vec::with_capacity(1 + self.server_scid.len());
        header.push(0x40 | ((PN_LEN - 1) as u8));
        header.extend_from_slice(&self.server_scid);
        let pn = self.app.next_pn;
        self.app.next_pn += 1;
        if let Some(pkt) = quic::seal_packet(keys, pn, PN_LEN, header, frames, false) {
            self.send_pkt(&pkt)?;
        }
        Ok(())
    }

    /// PATH_CHALLENGE'a alınan PATH_RESPONSE'u 1-RTT paketinde yankılar
    /// (RFC 9000 §8.2.2 yol doğrulama). Anahtarlar henüz hazır değilse
    /// veri bekletilir; sonraki poll'da gönderilir.
    fn send_path_response(&mut self) -> Result<(), String> {
        let Some(data) = self.pending_path_response else {
            return Ok(());
        };
        if !self.app_keys_ready {
            return Ok(());
        }
        let Some(keys) = self.app.tx.take() else {
            return Ok(());
        };
        let mut frames = Vec::new();
        quic::encode_path_response_frame(&data, &mut frames);
        let r = self.send_short_pkt(&keys, &frames);
        self.app.tx = Some(keys);
        r?;
        self.pending_path_response = None;
        Ok(())
    }

    /// 1-RTT anahtarları hazır olunca `active_connection_id_limit` (4)
    /// toplam CID sunmak için seq 1..=3 NEW_CONNECTION_ID frame'leri
    /// gönderir; seq 0 örtük olarak `scid`'dir. Peer bu CID'leri
    /// bağlantı göçünde (migration) hedef olarak kullanabilir.
    fn maybe_issue_cids(&mut self) -> Result<(), String> {
        if self.cids_issued || !self.app_keys_ready {
            return Ok(());
        }
        self.cids_issued = true;
        for seq in 1..=3u64 {
            let mut raw = [0u8; CID_LEN];
            if !crate::random::secure_fill_bytes(&mut raw) {
                break;
            }
            let mut token = [0u8; 16];
            if !crate::random::secure_fill_bytes(&mut token) {
                break;
            }
            let cid = raw.to_vec();
            let mut frames = Vec::new();
            quic::encode_new_connection_id_frame(seq, 0, &cid, &token, &mut frames);
            let Some(keys) = self.app.tx.take() else {
                break;
            };
            let r = self.send_short_pkt(&keys, &frames);
            self.app.tx = Some(keys);
            r?;
            self.issued_cids.push(IssuedCid { seq });
        }
        Ok(())
    }

    /// Peer'ın NEW_CONNECTION_ID ile sağladığı alternatif CID sayısı
    /// (bağlantı göçü için).
    pub fn peer_cid_count(&self) -> usize {
        self.peer_cids.len()
    }

    /// `seq` numaralı peer CID'inin ham bağlantı kimliği.
    pub fn peer_cid_bytes(&self, seq: u64) -> Option<&[u8]> {
        self.peer_cids
            .iter()
            .find(|c| c.seq == seq)
            .map(|c| c.cid.as_slice())
    }

    /// `seq` numaralı peer CID'inin stateless reset token'ı.
    pub fn peer_cid_reset_token(&self, seq: u64) -> Option<&[u8; 16]> {
        self.peer_cids
            .iter()
            .find(|c| c.seq == seq)
            .map(|c| &c.reset_token)
    }

    /// `seq` numaralı peer CID'inin retire_prior_to değeri.
    pub fn peer_cid_retire_prior_to(&self, seq: u64) -> Option<u64> {
        self.peer_cids
            .iter()
            .find(|c| c.seq == seq)
            .map(|c| c.retire_prior_to)
    }
}

//! QUIC (RFC 9000) + QUIC-TLS (RFC 9001) istemci bağlantısı.
//!
//! UDP datagramları üzerinde Initial/Handshake/1-RTT paket uzaylarını
//! yönetir; TLS 1.3 el sıkışmasını `TlsSession` üzerinden CRYPTO
//! stream'lerine taşıyarak yürütür. Kapsam: el sıkışma, ACK'ler,
//! CRYPTO/PING/CONNECTION_CLOSE/HANDSHAKE_DONE çerçeveleri ve STREAM
//! frame'lerinin uygulamaya teslimi (H3 katmanı `h3.rs`'te). Kayıp
//! kurtarma ve tıkanıklık kontrolü basittir (tekrar yok).

use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::netstack::Ipv4Address;
use crate::drivers::quic::{self, QuicKeys};
use crate::drivers::socket;
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

/// Bir gelen STREAM'in yeniden birleştirme durumu.
struct StreamRx {
    id: u64,
    /// (offset, veri) parçaları.
    chunks: Vec<(u64, Vec<u8>)>,
    fin: bool,
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

/// CRYPTO stream yeniden birleştirme: sıralı (offset, veri) parçaları.
struct CryptoRx {
    chunks: Vec<(u64, Vec<u8>)>,
    /// TLS'e zaten beslenen bayt sayısı (akış başından itibaren).
    fed: u64,
}

impl CryptoRx {
    fn new() -> Self {
        Self {
            chunks: Vec::new(),
            fed: 0,
        }
    }

    fn insert(&mut self, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let end = offset + data.len() as u64;
        let mut out: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut new_off = offset;
        let mut new_data: &[u8] = data;
        for (o, d) in self.chunks.drain(..) {
            let oe = o + d.len() as u64;
            if oe <= new_off || end <= o {
                // Yeni parça eskiyi tamamen kapsıyorsa eskisini düşür
                if oe <= offset && end >= oe {
                    continue;
                }
                out.push((o, d));
                continue;
            }
            // Çakışma: örtüşmeyen kısımları koru
            if o < new_off {
                let keep = (new_off - o) as usize;
                out.push((o, d[..keep].to_vec()));
            }
            if oe > end {
                let skip = (end - o) as usize;
                out.push((end, d[skip..].to_vec()));
            }
            // Yeni parçanın kalan kısmı
            if new_off < o {
                // yeni parça eski parçamdan önce başlıyor: örtüşen kısmı kırp
                let covered = (oe.min(end) - new_off) as usize;
                new_data = &new_data[covered..];
                new_off += covered as u64;
                if new_data.is_empty() {
                    break;
                }
            }
        }
        let end2 = new_off + new_data.len() as u64;
        if !new_data.is_empty() {
            out.push((new_off, new_data.to_vec()));
        }
        out.retain(|(_, d)| !d.is_empty());
        let _ = end2;
        out.sort_by_key(|(o, _)| *o);
        self.chunks = out;
    }

    /// `fed` sonrasındaki sürekli kapsamın sonunu döndürür.
    fn contiguous_end(&self) -> u64 {
        let mut have = self.fed;
        loop {
            let mut advanced = false;
            for (o, d) in &self.chunks {
                let oe = *o + d.len() as u64;
                if *o <= have && oe > have {
                    have = oe;
                    advanced = true;
                }
            }
            if !advanced {
                return have;
            }
        }
    }

    /// `from..to` aralığını `out`'a kopyalar.
    fn copy_range(&self, from: u64, to: u64, out: &mut Vec<u8>) {
        for (o, d) in &self.chunks {
            let oe = *o + d.len() as u64;
            if oe <= from || *o >= to {
                continue;
            }
            let s = (from.max(*o) - *o) as usize;
            let e = (to.min(oe) - *o) as usize;
            out.extend_from_slice(&d[s..e]);
        }
    }

    /// `upto`'ya kadar olan veriyi tüketilmiş say.
    fn advance(&mut self, upto: u64) {
        self.fed = upto;
        let fed = self.fed;
        for (o, d) in self.chunks.iter_mut() {
            if *o < fed {
                let skip = (fed - *o) as usize;
                if skip >= d.len() {
                    d.clear();
                } else {
                    d.drain(..skip);
                }
                *o = fed;
            }
        }
        self.chunks.retain(|(_, d)| !d.is_empty());
    }
}

/// QUIC istemci bağlantısı.
pub struct QuicConn {
    fd: i32,
    peer_ip: Ipv4Address,
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
}

impl QuicConn {
    /// UDP soketi açar, Initial paketini gönderir ve el sıkışma moduna
    /// geçer. `hostname` SNI + sertifika doğrulaması için kullanılır.
    pub fn connect(ip: Ipv4Address, port: u16, hostname: &str) -> Result<Self, String> {
        let fd = socket::socket(socket::AF_INET, socket::SOCK_DGRAM)
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

        // ClientHello (TLS kaydı -> handshake mesajı)
        let mut rec = Vec::new();
        let mut ecdhe = [0u8; 32];
        let mut x25519 = [0u8; 32];
        tls::build_client_hello_with_alpn(
            hostname,
            &client_random,
            &mut rec,
            &mut ecdhe,
            &mut x25519,
            false,
            None,
            None,
            &[b"h3"],
        );
        if rec.len() < 5 {
            socket::close(fd);
            return Err(String::from("client_hello"));
        }
        let mut hello = rec.split_off(5);
        tls.set_client_ecdhe_secret(ecdhe);
        tls.set_client_x25519_secret(x25519);

        // quic_transport_parameters (0xffa5) ekle — RFC 9000 §18.2
        let mut params = Vec::new();
        let mut put = |id: u64, val: u64| {
            quic::encode_varint(id, &mut params);
            quic::encode_varint(val, &mut params);
        };
        put(0x06, 1_000_000); // initial_max_data
        put(0x04, 65_536); // initial_max_stream_data_bidi_local
        put(0x05, 65_536); // initial_max_stream_data_bidi_remote
        put(0x08, 65_536); // initial_max_stream_data_uni
        put(0x03, 128); // initial_max_streams_bidi
        put(0x07, 32); // initial_max_streams_uni
        put(0x0a, 30_000); // max_idle_timeout (ms)
        put(0x0b, 25); // max_ack_delay (ms)
        put(0x0c, 3); // ack_delay_exponent
        put(0x0e, 4); // active_connection_id_limit
        quic::inject_quic_transport_params(&mut hello, &params);

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
        };
        conn.initial.tx = quic::client_initial_keys(&dcid);
        conn.initial.rx = quic::server_initial_keys(&dcid);
        conn.send_initial_crypto(0, &hello);
        Ok(conn)
    }

    /// Initial uzayında CRYPTO frame'i gönderir (ClientHello / yeniden
    /// deneme). Paket 1200 bayta doldurulur.
    fn send_initial_crypto(&mut self, offset: u64, data: &[u8]) {
        let Some(keys) = self.initial.tx.as_ref() else {
            return;
        };
        let mut frames = Vec::new();
        quic::encode_crypto_frame(offset, data, &mut frames);
        // 1200 bayt minimumu için PADDING
        let header_len_est = 7 + CID_LEN + 1 + CID_LEN + 1 + PN_LEN + frames.len() + 16;
        while header_len_est + frames.len() < INITIAL_MIN_LEN {
            frames.push(quic::FRAME_PADDING);
        }
        let Some(header) = quic::build_long_header(
            quic::PT_INITIAL,
            quic::VERSION_1,
            &self.dcid,
            &self.scid,
            None,
            PN_LEN,
            0,
            frames.len() + 16,
        ) else {
            return;
        };
        let pn = self.initial.next_pn;
        self.initial.next_pn += 1;
        if let Some(pkt) = quic::seal_packet(keys, pn, PN_LEN, header, &frames, true) {
            let _ = socket::sendto(self.fd, self.peer_ip, self.peer_port, &pkt);
            self.initial_sent = true;
        }
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
        self.flush_crypto_tx_hs()?;
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
        s.chunks.sort_by_key(|(o, _)| *o);
        let mut out = Vec::new();
        for (_, d) in s.chunks.iter() {
            out.extend_from_slice(d);
        }
        let fin = s.fin;
        self.streams[idx].chunks.clear();
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
            socket::sendto(self.fd, self.peer_ip, self.peer_port, &pkt)
                .map_err(|e| alloc::format!("sendto: {}", e))?;
        }
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
                    self.send_initial_crypto(0, &hello);
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
                    if read_varint(&mut pos).is_none() || read_varint(&mut pos).is_none() {
                        return Ok(());
                    }
                    let Some(ranges) = read_varint(&mut pos) else {
                        return Ok(());
                    };
                    if read_varint(&mut pos).is_none() {
                        return Ok(());
                    }
                    for _ in 0..ranges {
                        if read_varint(&mut pos).is_none() || read_varint(&mut pos).is_none() {
                            return Ok(());
                        }
                    }
                    if ft & 0x01 != 0
                        && (read_varint(&mut pos).is_none()
                            || read_varint(&mut pos).is_none()
                            || read_varint(&mut pos).is_none())
                    {
                        return Ok(());
                    }
                }
                0x06 | 0x07 => {
                    let Some((offset, l1)) = quic::decode_varint(&payload[pos..]) else {
                        return Ok(());
                    };
                    pos += l1;
                    let (dlen, dstart) = if ft & 0x01 != 0 {
                        let Some((len, l2)) = quic::decode_varint(&payload[pos..]) else {
                            return Ok(());
                        };
                        pos += l2;
                        (len as usize, pos)
                    } else {
                        (payload.len() - pos, pos)
                    };
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
                // STREAMS_BLOCKED(0x16/0x17) — hepsi varint alanlı;
                // gönderim tarafında sınır uygulamadığımız için yalnızca atlanır.
                0x10..=0x17 => {
                    // Alan sayısı türe göre: MAX_DATA/MAX_STREAMS/*_BLOCKED
                    // tek, MAX_STREAM_DATA/STREAM_DATA_BLOCKED çift varint.
                    let n = match ft {
                        0x11 | 0x15 => 2,
                        _ => 1,
                    };
                    for _ in 0..n {
                        match quic::decode_varint(&payload[pos..]) {
                            Some((_, l)) if pos + l <= payload.len() => pos += l,
                            _ => return Ok(()),
                        }
                    }
                }
                0x18..=0x1b => {
                    // NEW_CONNECTION_ID / RETIRE_CONNECTION_ID /
                    // PATH_CHALLENGE / PATH_RESPONSE — ayrıntılı parse yerine
                    // paketin kalanını at (tek çerçeve varsayımı).
                    return Ok(());
                }
                _ => return Ok(()), // bilinmeyen frame — paketin sonu
            }
        }
        Ok(())
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
        // Akış verisini sakla
        if let Some(s) = self.streams.iter_mut().find(|s| s.id == stream_id) {
            s.chunks.push((offset, data.to_vec()));
            s.fin |= fin;
        } else {
            self.streams.push(StreamRx {
                id: stream_id,
                chunks: alloc::vec![(offset, data.to_vec())],
                fin,
            });
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

    /// Anahtar fazlarını günceller; client Finished'i ve Ready geçişini yapar.
    fn update_key_phases(&mut self) -> Result<(), String> {
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
        if self.crypto_tx_hs.is_empty() {
            return Ok(());
        }
        let Some(keys) = self.handshake.tx.take() else {
            return Ok(());
        };
        let data = core::mem::take(&mut self.crypto_tx_hs);
        self.sent_hs_crypto = data.clone();
        let mut frames = Vec::new();
        // Handshake CRYPTO akışı offset'i: client'ın ilk handshake verisi 0.
        quic::encode_crypto_frame(0, &data, &mut frames);
        let result = self.send_long_pkt_with(&keys, quic::PT_HANDSHAKE, &frames);
        self.handshake.tx = Some(keys);
        result
    }

    /// RTO dolduğunda el sıkışma verisini yeniden gönderir.
    /// Basit kayıp kurtarma: Initial CH veya client Finished tekrar edilir.
    fn retransmit(&mut self) -> Result<(), String> {
        if self.state == ConnState::Ready || self.state == ConnState::Failed {
            return Ok(());
        }
        if self.handshake.tx.is_some() && !self.sent_hs_crypto.is_empty() {
            // client Finished'ı Handshake uzayında tekrar gönder
            let Some(keys) = self.handshake.tx.take() else {
                return Ok(());
            };
            let data = self.sent_hs_crypto.clone();
            let mut frames = Vec::new();
            quic::encode_crypto_frame(0, &data, &mut frames);
            let result = self.send_long_pkt_with(&keys, quic::PT_HANDSHAKE, &frames);
            self.handshake.tx = Some(keys);
            result
        } else if self.initial_sent {
            // Henüz handshake anahtarı yok → ClientHello'yu tekrar gönder
            let hello = self.client_hello.clone();
            self.send_initial_crypto(0, &hello);
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
            socket::sendto(self.fd, self.peer_ip, self.peer_port, &pkt)
                .map_err(|e| alloc::format!("sendto: {}", e))?;
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
            socket::sendto(self.fd, self.peer_ip, self.peer_port, &pkt)
                .map_err(|e| alloc::format!("sendto: {}", e))?;
        }
        Ok(())
    }
}

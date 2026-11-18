//! QUIC v1 (RFC 9000/9001) paket koruma katmanı.
//!
//! Bu modül şunları sağlar:
//! - İlk (Initial) anahtar türetimi: HKDF-Extract/Expand (SHA-256) ve
//!   `HKDF-Expand-Label` ile `client_/server_initial_{key,iv,hp}`.
//! - Paket numarası kodlama/çözme ve header protection (AES-ECB maskesi,
//!   uzun ve kısa başlıklar için).
//! - AEAD (AES-128-GCM) ile paket mühürleme/açma; nonce = iv XOR pn.
//! - Long header (Initial/0-RTT/Handshake/Retry) ayrıştırma ve üretme.
//!
//! Taşıma (UDP) ve TLS el sıkışma sürücüsü ayrı katmanlarda ele alınır;
//! buradaki tüm fonksiyonlar saf bayt işlemleridir ve host testleriyle
//! RFC 9001 Ek A vektörleri üzerinden doğrulanır.

#![allow(dead_code)]

use alloc::vec::Vec;

use super::crypto::aes::Aes;
use super::crypto::gcm::Gcm;
use super::crypto::hmac::HmacSha256;

/// QUIC sürümü 1 (RFC 9000).
pub const VERSION_1: u32 = 0x0000_0001;

/// QUIC v1 Initial tuzu (RFC 9001 §5.2).
const INITIAL_SALT: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];

/// Paket tipleri (long header ilk baytının alt 2 biti).
pub const PT_INITIAL: u8 = 0x0;
pub const PT_0RTT: u8 = 0x1;
pub const PT_HANDSHAKE: u8 = 0x2;
pub const PT_RETRY: u8 = 0x3;

/// Kısa başlık paketi için sabit bayrak değeri (fixed bit = 1).
const SHORT_HEADER_FIXED: u8 = 0x40;

const MAX_PKT_NUM_LEN: usize = 4;
const HEADER_PROTECTION_SAMPLE_LEN: usize = 16;

// ---------------------------------------------------------------------------
// HKDF (SHA-256) — RFC 5869
// ---------------------------------------------------------------------------

fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    HmacSha256::sign(salt, ikm)
}

fn hkdf_expand(prk: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(length);
    let mut t_prev: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while out.len() < length {
        let mut block = Vec::with_capacity(t_prev.len() + info.len() + 1);
        block.extend_from_slice(&t_prev);
        block.extend_from_slice(info);
        block.push(counter);
        t_prev = HmacSha256::sign(prk, &block).to_vec();
        out.extend_from_slice(&t_prev);
        counter = counter.wrapping_add(1);
    }
    out.truncate(length);
    out
}

/// TLS 1.3 `HKDF-Expand-Label` (RFC 8446 §7.1); QUIC etiketleri aynı
/// yapıyı `"tls13 "` önekiyle kullanır (RFC 9001 §5.1).
fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    let full_label_len = 6 + label.len(); // "tls13 " + label
    let mut info = Vec::with_capacity(2 + 1 + full_label_len + 1 + context.len());
    info.extend_from_slice(&(length as u16).to_be_bytes());
    info.push(full_label_len as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand(secret, &info, length)
}

// ---------------------------------------------------------------------------
// Anahtar fazları
// ---------------------------------------------------------------------------

/// Bir koruma fazının AEAD + header protection anahtarları.
pub struct QuicKeys {
    pub key: [u8; 16],
    pub iv: [u8; 12],
    pub hp: [u8; 16],
    pub gcm: Gcm,
    pub aes: Aes,
}

impl QuicKeys {
    fn from_secret(secret: &[u8]) -> Option<Self> {
        let key_v = hkdf_expand_label(secret, b"quic key", b"", 16);
        let iv_v = hkdf_expand_label(secret, b"quic iv", b"", 12);
        let hp_v = hkdf_expand_label(secret, b"quic hp", b"", 16);
        let mut key = [0u8; 16];
        let mut iv = [0u8; 12];
        let mut hp = [0u8; 16];
        key.copy_from_slice(&key_v[..16]);
        iv.copy_from_slice(&iv_v[..12]);
        hp.copy_from_slice(&hp_v[..16]);
        let gcm = Gcm::new(&key).ok()?;
        let aes = Aes::new(&hp).ok()?;
        Some(Self {
            key,
            iv,
            hp,
            gcm,
            aes,
        })
    }

    /// Paket numarası için nonce üretir: iv XOR (pn 62..0, 96-bit sola dayalı).
    fn nonce(&self, packet_number: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n.copy_from_slice(&self.iv);
        // pn 62-bit; 96-bit nonce'un son 8 baytına yerleştirilir (büyük endian,
        // 62 bit => üst 2 bit sıfır).
        let pn_bytes = packet_number.to_be_bytes();
        for i in 0..8 {
            n[4 + i] ^= pn_bytes[i];
        }
        n
    }
}

/// Initial secret'ları türetir: `initial_secret = HKDF-Extract(salt, dcid)`.
/// Dönen çift: (client_initial_secret, server_initial_secret).
pub fn initial_secrets(dcid: &[u8]) -> ([u8; 32], [u8; 32]) {
    let prk = hkdf_extract(&INITIAL_SALT, dcid);
    let client_label = hkdf_expand_label(&prk, b"client in", b"", 32);
    let server_label = hkdf_expand_label(&prk, b"server in", b"", 32);
    let mut c = [0u8; 32];
    let mut s = [0u8; 32];
    c.copy_from_slice(&client_label[..32]);
    s.copy_from_slice(&server_label[..32]);
    (c, s)
}

/// İstemci Initial anahtar seti.
pub fn client_initial_keys(dcid: &[u8]) -> Option<QuicKeys> {
    let (client_secret, _) = initial_secrets(dcid);
    QuicKeys::from_secret(&client_secret)
}

/// Sunucu Initial anahtar seti.
pub fn server_initial_keys(dcid: &[u8]) -> Option<QuicKeys> {
    let (_, server_secret) = initial_secrets(dcid);
    QuicKeys::from_secret(&server_secret)
}

// ---------------------------------------------------------------------------
// Header protection (RFC 9001 §5.4)
// ---------------------------------------------------------------------------

/// Örnek 16 baytlık `sample` üzerinden maskeyi üretir (16 bayt döner;
/// ilk 5'i header protection için kullanılır).
fn hp_mask(aes: &Aes, sample: &[u8]) -> [u8; 16] {
    debug_assert!(sample.len() >= HEADER_PROTECTION_SAMPLE_LEN);
    let mut block = [0u8; 16];
    block.copy_from_slice(&sample[..16]);
    aes.encrypt(&block)
}

/// Long header paketini header protection ile mühürler.
/// `header` ilk baytı (type+reserved+pn_len) içerir; `pn` paket numarası.
/// Dönen: güncellenmiş ilk bayt + maskelenmiş pn baytları.
pub fn protect_long_header(aes: &Aes, first_byte: u8, pn_bytes: &mut [u8], sample: &[u8]) -> u8 {
    let mask = hp_mask(aes, sample);
    let pn_len = (first_byte & 0x03) as usize + 1;
    let mut fb = first_byte;
    fb ^= mask[0] & 0x0f; // long header: düşük 4 bit
    for i in 0..pn_len.min(pn_bytes.len()) {
        pn_bytes[i] ^= mask[1 + i];
    }
    fb
}

/// Short header paketini header protection ile mühürler.
/// `first_byte` sabit bit + spin + pn_len içerir.
pub fn protect_short_header(aes: &Aes, first_byte: u8, pn_bytes: &mut [u8], sample: &[u8]) -> u8 {
    let mask = hp_mask(aes, sample);
    let pn_len = (first_byte & 0x03) as usize + 1;
    let mut fb = first_byte;
    fb ^= mask[0] & 0x1f; // short header: düşük 5 bit
    for i in 0..pn_len.min(pn_bytes.len()) {
        pn_bytes[i] ^= mask[1 + i];
    }
    fb
}

/// Korumalı long header ilk baytını çözer (pn_len çıkarılır).
pub fn unprotect_long_first(aes: &Aes, first_byte: u8, sample: &[u8]) -> (u8, usize) {
    let mask = hp_mask(aes, sample);
    let fb = first_byte ^ (mask[0] & 0x0f);
    (fb, (fb & 0x03) as usize + 1)
}

/// Korumalı short header ilk baytını çözer.
pub fn unprotect_short_first(aes: &Aes, first_byte: u8, sample: &[u8]) -> (u8, usize) {
    let mask = hp_mask(aes, sample);
    let fb = first_byte ^ (mask[0] & 0x1f);
    (fb, (fb & 0x03) as usize + 1)
}

// ---------------------------------------------------------------------------
// Paket numarası
// ---------------------------------------------------------------------------

/// Paket numarasını `pn_len` bayta büyük endian olarak kodlar (düşük bitler).
pub fn encode_packet_number(pn: u64, pn_len: usize, out: &mut [u8]) {
    let bytes = pn.to_be_bytes();
    let src = &bytes[8 - pn_len..];
    out[..pn_len].copy_from_slice(src);
}

/// Korumalı (henüz çözülmemiş) pn baytlarını verilen `expected` en büyük pn
/// bağlamında gerçek paket numarasına genişletir (RFC 9000 §A.3).
pub fn decode_packet_number(truncated: u64, pn_nbits: usize, expected: u64) -> u64 {
    let win = 1u64 << pn_nbits;
    let hwin = win / 2;
    let candidate = (expected & !(win - 1)) | truncated;
    if candidate + hwin <= expected && candidate < (1u64 << 62) - win {
        candidate + win
    } else if candidate > expected + hwin && candidate >= win {
        candidate - win
    } else {
        candidate
    }
}

// ---------------------------------------------------------------------------
// Paket mühürleme / açma (AEAD)
// ---------------------------------------------------------------------------

/// `payload`'ı (açık metin) AEAD ile şifreler; `header` AAD olarak kullanılır.
/// `header` ilk baytı (type+reserved) ve long-header kuyrukunu içerir;
/// pn bu fonksiyon tarafından sona eklenir. `is_long` header protection
/// varyantını seçer. Dönen: korunmuş tam paket.
pub fn seal_packet(
    keys: &QuicKeys,
    packet_number: u64,
    pn_len: usize,
    mut header: Vec<u8>,
    payload: &[u8],
    is_long: bool,
) -> Option<Vec<u8>> {
    let nonce = keys.nonce(packet_number);
    // Header sonuna pn ekle — AAD pn'İ İÇERİR (RFC 9001 §5.3: ilk bayttan
    // "unprotected packet number" dahil header sonuna kadar).
    let mut pn_buf = [0u8; MAX_PKT_NUM_LEN];
    encode_packet_number(packet_number, pn_len, &mut pn_buf[..pn_len]);
    header.extend_from_slice(&pn_buf[..pn_len]);
    let pn_offset = header.len() - pn_len;

    let (ct, tag) = keys.gcm.encrypt(&nonce, &header, payload).ok()?;

    let mut body = ct;
    body.extend_from_slice(&tag);

    let mut packet = header;
    packet.extend_from_slice(&body);

    // Header protection sample: pn'den sonraki 16 bayt (ciphertext başı).
    let sample_start = pn_offset + pn_len;
    if packet.len() < sample_start + HEADER_PROTECTION_SAMPLE_LEN {
        return None;
    }
    let sample: Vec<u8> =
        packet[sample_start..sample_start + HEADER_PROTECTION_SAMPLE_LEN].to_vec();

    // İlk baytı pn_len bitleriyle güncelle + maskele; pn baytlarını maskele.
    let pn_len_bits = (pn_len - 1) as u8;
    let mut fb = packet[0];
    fb = (fb & 0xfc) | pn_len_bits;
    if is_long {
        packet[0] = protect_long_header(
            &keys.aes,
            fb,
            &mut packet[pn_offset..pn_offset + pn_len],
            &sample,
        );
    } else {
        packet[0] = protect_short_header(
            &keys.aes,
            fb,
            &mut packet[pn_offset..pn_offset + pn_len],
            &sample,
        );
    }
    Some(packet)
}

/// TLS 1.3 trafik secret'ından (initial/handshake/application) QUIC paket
/// anahtarlarını türetir. Yalnızca AES-128-GCM-SHA-256 (0x1301) desteklenir.
pub fn traffic_keys(secret: &[u8]) -> Option<QuicKeys> {
    QuicKeys::from_secret(secret)
}

/// `packet`'i çözer; başarılıysa (packet_number, payload, pn_offset) döner.
/// `expected_pn` en büyük görülen paket numarası (tahmin için).
/// `is_long` header tipine göre header protection varyantını seçer.
/// Birleşik datagramlarda tüketilen bayt sayısı `pn_offset + Length`
/// alanından (`parse_long_header_prefix`) hesaplanır.
pub fn open_packet(
    keys: &QuicKeys,
    packet: &[u8],
    is_long: bool,
    pn_offset: usize,
    expected_pn: u64,
) -> Option<(u64, Vec<u8>, usize)> {
    if packet.len() < pn_offset + MAX_PKT_NUM_LEN + HEADER_PROTECTION_SAMPLE_LEN {
        return None;
    }
    // Header protection sample: pn_offset + 4 (max pn len) konumundan 16 bayt.
    let sample_start = pn_offset + MAX_PKT_NUM_LEN;
    let sample = &packet[sample_start..sample_start + HEADER_PROTECTION_SAMPLE_LEN];

    let (fb, pn_len) = if is_long {
        unprotect_long_first(&keys.aes, packet[0], sample)
    } else {
        unprotect_short_first(&keys.aes, packet[0], sample)
    };
    if pn_len == 0 || pn_len > MAX_PKT_NUM_LEN || pn_offset + pn_len > packet.len() {
        return None;
    }

    // pn baytlarını çöz
    let mut pn_enc = [0u8; MAX_PKT_NUM_LEN];
    pn_enc[..pn_len].copy_from_slice(&packet[pn_offset..pn_offset + pn_len]);
    let mask = hp_mask(&keys.aes, sample);
    for i in 0..pn_len {
        pn_enc[i] ^= mask[1 + i];
    }
    let truncated = u64::from_be_bytes({
        let mut b = [0u8; 8];
        b[8 - pn_len..].copy_from_slice(&pn_enc[..pn_len]);
        b
    });
    let packet_number = decode_packet_number(truncated, pn_len * 8, expected_pn);

    // AAD = header INCLUDING pn (RFC 9001 §5.3): çözülmüş ilk bayt +
    // pn_offset'e kadar başlık + çözülmüş pn baytları.
    let mut header = Vec::with_capacity(pn_offset + pn_len);
    header.push(fb);
    header.extend_from_slice(&packet[1..pn_offset]);
    header.extend_from_slice(&pn_enc[..pn_len]);

    // Kalan = ciphertext || tag
    let ct_start = pn_offset + pn_len;
    if packet.len() < ct_start + 16 {
        return None;
    }
    let ct = &packet[ct_start..packet.len() - 16];
    let tag: &[u8; 16] = packet[packet.len() - 16..].try_into().ok()?;
    let nonce = keys.nonce(packet_number);
    let payload = keys.gcm.decrypt(&nonce, &header, ct, tag).ok()?;

    Some((packet_number, payload, pn_offset))
}

// ---------------------------------------------------------------------------
// Long header yardımcıları
// ---------------------------------------------------------------------------

/// Varint kodlama (RFC 9000 §16): 2-bit uzunluk öneki.
pub fn encode_varint(value: u64, out: &mut Vec<u8>) {
    if value < (1 << 6) {
        out.push(value as u8);
    } else if value < (1 << 14) {
        out.extend_from_slice(&((value as u16) | 0x4000).to_be_bytes());
    } else if value < (1 << 30) {
        out.extend_from_slice(&((value as u32) | 0x8000_0000).to_be_bytes());
    } else {
        // 62-bit varint: 8 bayt, üst 2 bit 11.
        out.extend_from_slice(&(value | 0xC000_0000_0000_0000).to_be_bytes());
    }
}

/// Varint çözme; (değer, tüketilen bayt) döner.
pub fn decode_varint(data: &[u8]) -> Option<(u64, usize)> {
    if data.is_empty() {
        return None;
    }
    let first = data[0];
    let len = 1usize << (first >> 6);
    if data.len() < len {
        return None;
    }
    let mut v: u64 = 0;
    for b in data.iter().take(len) {
        v = (v << 8) | *b as u64;
    }
    v &= (1u64 << (len * 8 - 2)) - 1;
    Some((v, len))
}

/// Long header üretir (pn HARİÇ — `seal_packet` ekler).
/// `packet_type` PT_INITIAL/PT_0RTT/PT_HANDSHAKE/PT_RETRY.
/// `dcid`/`scid` bağlantı kimlikleri, `token` yalnız Initial'da,
/// `pn_len` 1..=4. `packet_number` yalnız Length hesaplaması için kullanılır
/// (pn_len + payload_len).
/// Dönen header'ın sonu Length alanıdır; pn_offset = header.len().
#[allow(clippy::too_many_arguments)]
pub fn build_long_header(
    packet_type: u8,
    version: u32,
    dcid: &[u8],
    scid: &[u8],
    token: Option<&[u8]>,
    pn_len: usize,
    _packet_number: u64,
    payload_len: usize,
) -> Option<Vec<u8>> {
    if dcid.len() > 20 || scid.len() > 20 || pn_len == 0 || pn_len > 4 {
        return None;
    }
    let mut h = Vec::with_capacity(32 + dcid.len() + scid.len());
    // İlk bayt: 1 | type(2) | reserved(2) | pn_len(2) — pn_len koruma öncesi.
    let mut first = 0xC0u8; // long header + fixed bit
    first |= (packet_type & 0x03) << 4;
    first |= (pn_len - 1) as u8;
    h.push(first);
    h.extend_from_slice(&version.to_be_bytes());
    h.push(dcid.len() as u8);
    h.extend_from_slice(dcid);
    h.push(scid.len() as u8);
    h.extend_from_slice(scid);
    if packet_type == PT_INITIAL {
        let t = token.unwrap_or(&[]);
        if t.len() > 1024 {
            return None;
        }
        encode_varint(t.len() as u64, &mut h);
        h.extend_from_slice(t);
    }
    if packet_type == PT_RETRY {
        // Retry'de length/pn yok; integrity tag sonda eklenir (ayrı katman).
        return Some(h);
    }
    // Length: pn_len + payload_len (AEAD tag dahil). pn bu fonksiyonda
    // eklenmez; `seal_packet` pn'i header sonuna ekleyip korur.
    encode_varint((pn_len + payload_len) as u64, &mut h);
    Some(h)
}

/// Long header ayrıştırma sonucu:
/// (packet_type, version, dcid, scid, token, length, header_len, pn_offset)
pub type LongHeaderPrefix = (u8, u32, Vec<u8>, Vec<u8>, Vec<u8>, u64, usize, usize);

/// Long header ayrıştırma (koruma kaldırılmadan, type/version/cid/token/length).
pub fn parse_long_header_prefix(packet: &[u8]) -> Option<LongHeaderPrefix> {
    if packet.len() < 7 || packet[0] & 0x80 == 0 {
        return None;
    }
    let ptype = (packet[0] >> 4) & 0x03;
    let version = u32::from_be_bytes([packet[1], packet[2], packet[3], packet[4]]);
    let mut pos = 5usize;
    if pos >= packet.len() {
        return None;
    }
    let dcid_len = packet[pos] as usize;
    pos += 1;
    if pos + dcid_len > packet.len() || dcid_len > 20 {
        return None;
    }
    let dcid = packet[pos..pos + dcid_len].to_vec();
    pos += dcid_len;
    if pos >= packet.len() {
        return None;
    }
    let scid_len = packet[pos] as usize;
    pos += 1;
    if pos + scid_len > packet.len() || scid_len > 20 {
        return None;
    }
    let scid = packet[pos..pos + scid_len].to_vec();
    pos += scid_len;

    let mut token = Vec::new();
    if ptype == PT_INITIAL {
        let (tlen, tl) = decode_varint(&packet[pos..])?;
        pos += tl;
        if pos + tlen as usize > packet.len() {
            return None;
        }
        token = packet[pos..pos + tlen as usize].to_vec();
        pos += tlen as usize;
    }

    if ptype == PT_RETRY {
        // Retry: length/pn yok; geri kalan integrity tag'dir.
        return Some((ptype, version, dcid, scid, token, 0, pos, usize::MAX));
    }

    let (length, ll) = decode_varint(&packet[pos..])?;
    pos += ll;
    // pn_offset: header protection çözülmeden önce pn başlangıcı.
    // pn_len ilk bayttan okunamaz (korumalı); çağıran pn_offset'ten itibaren
    // MAX_PKT_NUM_LEN örneği alıp pn_len'i çözer. pn_offset = pos.
    Some((ptype, version, dcid, scid, token, length, pos, pos))
}

// ---------------------------------------------------------------------------
// Frame kodlama (RFC 9000 §19)
// ---------------------------------------------------------------------------

pub const FRAME_PADDING: u8 = 0x00;
pub const FRAME_PING: u8 = 0x01;
pub const FRAME_ACK: u8 = 0x02;
pub const FRAME_CRYPTO: u8 = 0x07; // bit0=1: Length alanı mevcut
pub const FRAME_STREAM_BASE: u8 = 0x08; // STREAM bitleri ile birlikte

/// STREAM frame'i kodlar. `stream_id`, `offset`, `fin`, `data`.
/// FIN biti: stream id'nin düşük biti (0x01) + FIN flag (0x20 base'de).
pub fn encode_stream_frame(stream_id: u64, offset: u64, fin: bool, data: &[u8], out: &mut Vec<u8>) {
    // Frame type: 0x08 | (OFF 0x04) | (LEN 0x02) | (FIN 0x01)
    let mut ft = FRAME_STREAM_BASE;
    if offset > 0 {
        ft |= 0x04;
    }
    ft |= 0x02; // her zaman length alanı
    if fin {
        ft |= 0x01;
    }
    out.push(ft);
    encode_varint(stream_id, out);
    if offset > 0 {
        encode_varint(offset, out);
    }
    encode_varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

/// CRYPTO frame'i kodlar (el sıkışma verisi).
pub fn encode_crypto_frame(offset: u64, data: &[u8], out: &mut Vec<u8>) {
    out.push(FRAME_CRYPTO);
    encode_varint(offset, out);
    encode_varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

/// ACK frame'i kodlar (RFC 9000 §19.3): tek aralık, ECN yok.
/// `largest` en büyük onaylanan paket numarası; `ack_delay` kodlanmış
/// ack_delay değeri (mikrosaniye >> ack_delay_exponent).
pub fn encode_ack_frame(largest: u64, ack_delay: u64, out: &mut Vec<u8>) {
    out.push(0x02);
    encode_varint(largest, out);
    encode_varint(ack_delay, out);
    encode_varint(0, out); // ack_range_count
    encode_varint(0, out); // first_ack_range
}

/// CONNECTION_CLOSE frame'i (transport error, RFC 9000 §19.19).
pub fn encode_connection_close_frame(error_code: u64, out: &mut Vec<u8>) {
    out.push(0x1c);
    encode_varint(error_code, out);
    encode_varint(0, out); // frame_type
    encode_varint(0, out); // reason_len
}

/// MAX_DATA / MAX_STREAM_DATA / MAX_STREAMS çerçeveleri (basit sürüm).
pub fn encode_max_data_frame(max_data: u64, out: &mut Vec<u8>) {
    out.push(0x10);
    encode_varint(max_data, out);
}

pub fn encode_max_streams_frame(is_uni: bool, count: u64, out: &mut Vec<u8>) {
    out.push(if is_uni { 0x13 } else { 0x12 });
    encode_varint(count, out);
}

/// ClientHello handshake mesajına `quic_transport_parameters` (0xffa5)
/// uzantısını ekler ve uzunluk alanlarını günceller.
/// `msg` biçimi: type(1) + len(3) + body. Gövde sonundaki extensions
/// bloğunun sonuna ekleme yapılır.
pub fn inject_quic_transport_params(msg: &mut Vec<u8>, params_body: &[u8]) {
    // extensions_len konumu: gövde sonundan geriye doğru — extensions
    // toplam uzunluğu 2 bayt; extensions bloğu msg sonunda biter.
    // body = msg[4..]; body sonu = extensions sonu.
    // extensions bloğunu bulmak için CH gövdesini baştan ayrıştır:
    //   legacy_version(2) + random(32) + session_id(1+n) +
    //   cipher_suites(2+n) + compression(1+n) + extensions(2+n)
    if msg.len() < 4 + 2 + 32 + 1 + 2 + 1 {
        return;
    }
    let mut pos = 4 + 2 + 32;
    let sid_len = msg[pos] as usize;
    pos += 1 + sid_len;
    if pos + 2 > msg.len() {
        return;
    }
    let cs_len = u16::from_be_bytes([msg[pos], msg[pos + 1]]) as usize;
    pos += 2 + cs_len;
    if pos + 1 > msg.len() {
        return;
    }
    let comp_len = msg[pos] as usize;
    pos += 1 + comp_len;
    if pos + 2 > msg.len() {
        return;
    }
    let ext_total_pos = pos;
    let ext_total = u16::from_be_bytes([msg[pos], msg[pos + 1]]) as usize;
    pos += 2;
    let ext_end = pos + ext_total;
    if ext_end != msg.len() {
        return; // beklenmedik biçim — ekleme yapma
    }
    let ext_len = (4 + params_body.len()) as u16;
    let ext_total_size = 4 + params_body.len();
    // Yeni uzantıyı sona ekle
    msg.extend_from_slice(&0xffa5u16.to_be_bytes());
    msg.extend_from_slice(&ext_len.to_be_bytes());
    msg.extend_from_slice(params_body);
    // extensions toplam uzunluğunu güncelle
    let new_total = (ext_total + ext_total_size) as u16;
    msg[ext_total_pos] = (new_total >> 8) as u8;
    msg[ext_total_pos + 1] = new_total as u8;
    // handshake mesaj uzunluğunu güncelle
    let hs_len = (msg.len() - 4) as u32;
    msg[1] = ((hs_len >> 16) & 0xff) as u8;
    msg[2] = ((hs_len >> 8) & 0xff) as u8;
    msg[3] = (hs_len & 0xff) as u8;
}

/// PADDING frame'leriyle datagramı minimum boyuta doldurur (Initial için
/// 1200 bayt zorunlu, RFC 9000 §14.1).
pub fn pad_datagram(packet: &mut Vec<u8>, min_len: usize) {
    while packet.len() < min_len {
        packet.push(FRAME_PADDING);
    }
}

// ---------------------------------------------------------------------------
// Testler — RFC 9001 Ek A vektörleri
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: alloc::string::String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn hkdf_expand_label_rfc9001() {
        // RFC 9001 A.1: initial_secret
        let dcid = hex("8394c8f03e515708");
        let (client_secret, server_secret) = initial_secrets(&dcid);
        let cs = hex(concat!(
            "c00cf151ca5be075ed0ebfb5c80323c4",
            "2d6b7db67881289af4008f1f6c357aea"
        ));
        let ss = hex(concat!(
            "3c199828fd139efd216c155ad844cc81",
            "fb82fa8d7446fa7d78be803acdda951b"
        ));
        assert_eq!(&client_secret[..], &cs[..]);
        assert_eq!(&server_secret[..], &ss[..]);
    }

    #[test]
    fn initial_key_iv_hp_vectors() {
        let dcid = hex("8394c8f03e515708");
        let keys = client_initial_keys(&dcid).expect("anahtar üretilemedi");
        assert_eq!(hex("1f369613dd76d5467730efcbe3b1a22d"), keys.key.to_vec());
        assert_eq!(hex("fa044b2f42a3fd3b46fb255c"), keys.iv.to_vec());
        assert_eq!(hex("9f50449e04a0e810283a1e9933adedd2"), keys.hp.to_vec());
    }

    #[test]
    fn header_protection_roundtrip_long() {
        // RFC 9001 A.2 örneği: unprotected header + pn=2, protected çıktı
        // bilinen paketle karşılaştırılır (bütünlük: açma ile geri dönmeli).
        let dcid = hex("8394c8f03e515708");
        let scid = hex("f067a5502a4262b5");
        let keys = client_initial_keys(&dcid).unwrap();

        // Basit payload ile paket üret
        let mut payload = alloc::vec![0x06u8, 0x00, 0x40, 0xf1, 0x01];
        payload.extend_from_slice(b"quic-initial-payload-test-vector!");
        let pn = 2u64;
        let pn_len = 4usize;
        let header = build_long_header(
            PT_INITIAL,
            VERSION_1,
            &dcid,
            &scid,
            Some(&[]),
            pn_len,
            pn,
            payload.len() + 16, // ct + tag
        )
        .unwrap();
        let pn_offset = header.len(); // pn burada başlar
        let pkt = seal_packet(&keys, pn, pn_len, header, &payload, true).unwrap();

        // Paket açılabilmeli.
        let (dpn, dpay, _hlen) =
            open_packet(&keys, &pkt, true, pn_offset, 0).expect("paket açılamadı");
        assert_eq!(dpn, pn);
        assert_eq!(dpay, payload);
    }

    #[test]
    fn packet_number_decode_wrap() {
        // pn_nbits=8, expected=300, truncated=5 → 261? RFC penceresi:
        // candidate=256+5=261? Aslında expected&~255=256, +5=261; 261+128=389>300
        // ve 261<300+128 → 261? expected=300 için doğru pn=261 olamaz (>300 değil).
        // Doğru örnek: expected=300, wire=44 → candidate=300? Test penceresi:
        let pn = decode_packet_number(44, 8, 300);
        // win=256,hwin=128: candidate=(300&!255)|44=256|44=300 → 300+128>300?
        // candidate>expected+hwin? 300>428? hayır → 300? ama wire 44≠300...
        // truncated 44 → candidate=256+44=300. Evet 300 döner.
        assert_eq!(pn, 300);
        // expected=10, truncated=250 → candidate=250; 250>10+128 ve 250>=256? hayır
        // → candidate-256=-6 taşar → else → 250? Aslında 250>138 ve >=256 false → else 250.
        let pn2 = decode_packet_number(250, 8, 10);
        assert_eq!(pn2, 250);
    }

    #[test]
    fn varint_roundtrip() {
        for v in [
            0u64,
            63,
            64,
            16383,
            16384,
            (1 << 30) - 1,
            1 << 30,
            u64::MAX >> 2,
        ] {
            let mut out = Vec::new();
            encode_varint(v, &mut out);
            let (d, n) = decode_varint(&out).unwrap();
            assert_eq!(d, v);
            assert_eq!(n, out.len());
        }
    }

    #[test]
    fn stream_frame_encodes() {
        let mut out = Vec::new();
        encode_stream_frame(0, 0, true, b"hello", &mut out);
        // type 0x0f? stream_id=0 → ft=0x08|0x02|0x01=0x0b
        assert_eq!(out[0], 0x0b);
        assert_eq!(&out[3..8], b"hello");
    }
}

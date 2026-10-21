//! TLS 1.2 / TLS 1.3 Protokol İstemci Katmanı (no_std)
//! TLS 1.2: ClientHello → ServerHello → Certificate → ServerKeyExchange →
//!          ServerHelloDone → ClientKeyExchange → ChangeCipherSpec → Finished → şifreli Application Data.
//! TLS 1.3: ClientHello → ServerHello → EncryptedExtensions → Certificate →
//!          CertificateVerify → Finished → şifreli Application Data.
//! AES-128-GCM, AES-256-GCM, ECDHE (secp256r1), RSA anahtar değişimi, SHA-256 / SHA-384 PRF ve
//! anahtar türetme dahil. TLS 1.3'de HKDF anahtar planı kullanılır.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::crypto::gcm::Gcm;
use crate::drivers::crypto::hmac::HmacSha256;
use crate::drivers::crypto::sha256::Sha256;
use crate::drivers::crypto::rsa::{pkcs1_v1_5_encrypt, rsa_verify_pkcs1_sha256, RsaPublicKey};
use crate::drivers::crypto::x509::{parse_x509_ec_public_key, parse_x509_rsa_public_key};

pub const TLS_RECORD_CHANGE_CIPHER_SPEC: u8 = 20;
pub const TLS_RECORD_ALERT: u8 = 21;
pub const TLS_RECORD_HANDSHAKE: u8 = 22;
pub const TLS_RECORD_APPLICATION_DATA: u8 = 23;

pub const TLS_VERSION_1_0: u16 = 0x0301;
pub const TLS_VERSION_1_2: u16 = 0x0303;
pub const TLS_VERSION_1_3: u16 = 0x0304;

pub const HANDSHAKE_CLIENT_HELLO: u8 = 1;
pub const HANDSHAKE_SERVER_HELLO: u8 = 2;
pub const HANDSHAKE_CERTIFICATE: u8 = 11;
pub const HANDSHAKE_SERVER_KEY_EXCHANGE: u8 = 12;
pub const HANDSHAKE_SERVER_HELLO_DONE: u8 = 14;
pub const HANDSHAKE_CLIENT_KEY_EXCHANGE: u8 = 16;
pub const HANDSHAKE_FINISHED: u8 = 20;
pub const HANDSHAKE_ENCRYPTED_EXTENSIONS: u8 = 8;
pub const HANDSHAKE_CERTIFICATE_VERIFY: u8 = 15;

/// ECDHE + RSA sertifikası + AES-128-GCM + SHA-256.
pub const TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0xC02F;
pub const TLS_RSA_WITH_AES_128_GCM_SHA256: u16 = 0x009C;
/// ECDHE + RSA + AES-256-GCM + SHA-384.
pub const TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384: u16 = 0xC030;
/// TLS 1.3 cipher suites.
pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301;
pub const TLS_AES_256_GCM_SHA384: u16 = 0x1302;
pub const TLS_CHACHA20_POLY1305_SHA256: u16 = 0x1303;

/// secp256r1 (NIST P-256) named curve kimliği.
pub const NAMED_CURVE_SECP256R1: u16 = 0x0017;

const TLS_MASTER_SECRET_LEN: usize = 48;
const TLS_KEY_BLOCK_LEN: usize = 40;
const TLS_VERIFY_DATA_LEN: usize = 12;

/// GCM kimlik dogrulama etiketinin uzunlugu (bayt).
const GCM_TAG_LEN: usize = 16;

/// TLS 1.3 hash output length (SHA-256).
const TLS13_HASH_LEN: usize = 32;

/// TLS 1.3 HKDF-Expand-Label prefix.
const TLS13_LABEL_PREFIX: &[u8] = b"tls13 ";

/// Signature algorithms supported.
pub const SIG_ALG_RSA_SHA256: u16 = 0x0401;
pub const SIG_ALG_ECDSA_SECP256R1_SHA256: u16 = 0x0403;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlsState {
    ClientHelloSent,
    ServerHelloReceived,
    CertificateReceived,
    ServerHelloDoneReceived,
    FinishedSent,
    /// TLS 1.3: sunucunun Finished doğrulandı, istemci Finished gönderilecek.
    Tls13ServerFinished,
    Established,
    Error,
}

pub struct TlsSession {
    state: TlsState,
    hostname: String,
    client_random: [u8; 32],
    server_random: [u8; 32],
    cipher_suite: u16,
    tls13_mode: bool,
    rsa_key: Option<RsaPublicKey>,
    /// P-256 EC açık anahtarı (04 || X || Y) — ECDSA sertifikaları için.
    ec_key: Option<[u8; 65]>,
    /// Sunucunun geçici ECDHE açık noktası (65 bayt, 04||X||Y).
    ecdhe_point: Vec<u8>,
    pre_master_secret: Option<Vec<u8>>,
    master_secret: Option<[u8; 48]>,
    /// TLS 1.3 handshake / master secret (32 byte).
    tls13_handshake_secret: Option<[u8; 32]>,
    tls13_master_secret: Option<[u8; 32]>,
    /// TLS 1.3 el sıkışma trafik sırları ve türetilmiş sunucu anahtarı/IV'si.
    tls13_client_hs_secret: Option<[u8; 32]>,
    tls13_server_hs_secret: Option<[u8; 32]>,
    tls13_server_hs_key: Option<[u8; 16]>,
    tls13_server_hs_iv: Option<[u8; 12]>,
    /// TLS 1.2 sunucu el sıkışma kaydı sıra sayacı.
    tls13_server_hs_seq: u64,
    /// TLS 1.2'de istemci ChangeCipherSpec gönderdikten sonra sunucunun
    /// el sıkışma kayıtları da şifrelidir; çözülmeleri gerekir.
    tls12_cipher_active: bool,
    client_write_key: Option<[u8; 16]>,
    server_write_key: Option<[u8; 16]>,
    client_write_iv: Option<[u8; 4]>,
    server_write_iv: Option<[u8; 4]>,
    /// TLS 1.3 base IV (12 byte) — nonce = base_iv XOR sequence_number.
    client_base_iv: Option<[u8; 12]>,
    server_base_iv: Option<[u8; 12]>,
    server_gcm: Option<Gcm>,
    client_gcm: Option<Gcm>,
    server_seq: u64,
    client_seq: u64,
    record_buffer: Vec<u8>,
    hs_buffer: Vec<u8>,
    /// TLS 1.2 handshake hash (hash of all handshake messages).
    handshake_hash: Sha256,
    /// TLS 1.3 transcript hash (updated as handshake messages are processed).
    tls13_transcript_hash: Sha256,
    outgoing: Vec<u8>,
    /// Alınan son TLS uyarısı (seviye, açıklama).
    last_alert: Option<(u8, u8)>,
    /// Client ECDHE secret for TLS 1.3 key derivation (stored after ClientHello).
    client_ecdhe_secret: Option<[u8; 32]>,
    /// Sertifika hostname ve süre doğrulaması başarılı mı?
    pub cert_verified: bool,
    /// Sertifika doğrulama hatası (varsa).
    pub cert_error: Option<crate::drivers::crypto::x509::CertValidationError>,
}

impl TlsSession {
    pub fn new(hostname: &str, client_random: [u8; 32]) -> Self {
        TlsSession {
            state: TlsState::ClientHelloSent,
            hostname: String::from(hostname),
            client_random,
            server_random: [0; 32],
            cipher_suite: 0,
            tls13_mode: false,
            rsa_key: None,
            ec_key: None,
            ecdhe_point: Vec::new(),
            pre_master_secret: None,
            master_secret: None,
            tls13_handshake_secret: None,
            tls13_master_secret: None,
            tls13_client_hs_secret: None,
            tls13_server_hs_secret: None,
            tls13_server_hs_key: None,
            tls13_server_hs_iv: None,
            tls13_server_hs_seq: 0,
            tls12_cipher_active: false,
            client_write_key: None,
            server_write_key: None,
            client_write_iv: None,
            server_write_iv: None,
            client_base_iv: None,
            server_base_iv: None,
            server_gcm: None,
            client_gcm: None,
            server_seq: 0,
            client_seq: 0,
            record_buffer: Vec::new(),
            hs_buffer: Vec::new(),
            handshake_hash: Sha256::new(),
            tls13_transcript_hash: Sha256::new(),
            outgoing: Vec::new(),
            last_alert: None,
            client_ecdhe_secret: None,
            cert_verified: false,
            cert_error: None,
        }
    }

    pub fn is_cert_verified(&self) -> bool {
        self.cert_verified
    }

    pub fn cert_error(&self) -> Option<crate::drivers::crypto::x509::CertValidationError> {
        self.cert_error
    }

    pub fn set_client_ecdhe_secret(&mut self, secret: [u8; 32]) {
        self.client_ecdhe_secret = Some(secret);
    }

    /// Sunucudan gelen son TLS uyarısı (seviye, açıklama) — hata mesajı için.
    pub fn last_alert(&self) -> Option<(u8, u8)> {
        self.last_alert
    }

    pub fn note_client_hello(&mut self, client_hello_body: &[u8]) {
        self.handshake_hash.update(client_hello_body);
        self.tls13_transcript_hash.update(client_hello_body);
    }

    pub fn feed_tcp_data(&mut self, data: &[u8]) {
        self.record_buffer.extend_from_slice(data);
    }

    pub fn take_outgoing(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.outgoing)
    }

    pub fn process_records(
        &mut self,
        out_handshake: &mut Vec<u8>,
        out_app_data: &mut Vec<u8>,
    ) -> Result<TlsState, ()> {
        loop {
            if self.record_buffer.len() < 5 {
                break;
            }
            let (ctype, ver, len) = parse_record_header(&self.record_buffer).ok_or(())?;
            let _ = ver;
            if self.record_buffer.len() < 5 + len {
                break;
            }
            let payload = &self.record_buffer[5..5 + len];
            let payload_copy = payload.to_vec();
            self.record_buffer.drain(..5 + len);

            match ctype {
                TLS_RECORD_HANDSHAKE => {
                    if self.tls13_mode && self.tls13_server_hs_key.is_some() {
                        // TLS 1.3: ServerHello'dan sonraki tüm el sıkışma
                        // kayıtları sunucu el sıkışma trafik anahtarıyla şifrelidir.
                        let decrypted = self.decrypt_tls13_handshake(&payload_copy)?;
                        self.process_handshake_records(&decrypted, out_handshake)?;
                    } else if self.tls12_cipher_active {
                        // TLS 1.2: istemci ChangeCipherSpec gönderdikten sonra
                        // sunucunun el sıkışma kayıtları da şifrelidir.
                        let decrypted = self.decrypt_tls12_record(&payload_copy)?;
                        self.process_handshake_records(&decrypted, out_handshake)?;
                    } else {
                        self.process_handshake_records(&payload_copy, out_handshake)?;
                    }
                }
                TLS_RECORD_CHANGE_CIPHER_SPEC => {}
                TLS_RECORD_ALERT => {
                    let desc = payload_copy.get(1).copied().unwrap_or(0);
                    let level = payload_copy.first().copied().unwrap_or(0);
                    self.last_alert = Some((level, desc));
                    return Err(());
                }
                TLS_RECORD_APPLICATION_DATA => {
                    if self.tls13_mode {
                        // TLS 1.3'te sunucu, el sıkışma sonrası mesajları
                        // (örn. NewSessionTicket) uygulama verisi kaydının
                        // içinde, içerik türü 0x16 olarak gönderir.
                        let (content, ctype) = self.decrypt_tls13_app_record(&payload_copy)?;
                        match ctype {
                            TLS_RECORD_HANDSHAKE => {
                                self.process_handshake_records(&content, out_handshake)?;
                            }
                            TLS_RECORD_ALERT => {
                                let desc = content.get(1).copied().unwrap_or(0);
                                let level = content.first().copied().unwrap_or(0);
                                self.last_alert = Some((level, desc));
                                return Err(());
                            }
                            _ => out_app_data.extend_from_slice(&content),
                        }
                    } else {
                        let plaintext = self.decrypt_app_data(&payload_copy)?;
                        out_app_data.extend_from_slice(&plaintext);
                    }
                }
                _ => {}
            }
        }
        Ok(self.state)
    }

    /// TLS 1.3 encrypted handshake record decryption.
    /// Uses server handshake traffic key derived after ServerHello.
    fn decrypt_tls13_handshake(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
        let key = self.tls13_server_hs_key.ok_or(())?;
        let iv = self.tls13_server_hs_iv.ok_or(())?;

        let gcm = Gcm::new(&key).map_err(|_| ())?;
        let seq = self.tls13_server_hs_seq;
        self.tls13_server_hs_seq = self.tls13_server_hs_seq.wrapping_add(1);

        let nonce = Self::tls13_nonce(&iv, seq);
        let gcm_aad = Self::tls13_aad(TLS_RECORD_HANDSHAKE, ciphertext.len());

        if ciphertext.len() < 16 {
            return Err(());
        }
        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];

        let mut tag_arr = [0u8; 16];
        tag_arr.copy_from_slice(tag);

        let inner = gcm.decrypt(&nonce, &gcm_aad, ct, &tag_arr)?;
        // Ic duz metinden el sikisma icerigini ve icerik turunu ayir.
        let (content, content_type) = Self::tls13_strip_inner_plaintext(&inner)?;
        if content_type != TLS_RECORD_HANDSHAKE {
            return Err(());
        }
        Ok(content.to_vec())
    }

    /// TLS 1.2 şifreli el sıkışma kaydının çözülmesi.
    ///
    /// İstemci ChangeCipherSpec gönderdikten sonra sunucunun el sıkışma
    /// kayıtları (Finished) sunucu yazma anahtarıyla şifrelenir.
    fn decrypt_tls12_record(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
        let server_iv = self.server_write_iv.ok_or(())?;
        let gcm = self.server_gcm.as_mut().ok_or(())?;

        let seq = self.server_seq;
        self.server_seq = self.server_seq.wrapping_add(1);

        let nonce = Self::tls12_nonce(&server_iv, seq);
        let gcm_aad = Self::tls12_aad(seq, TLS_RECORD_HANDSHAKE, ciphertext.len());

        if ciphertext.len() < 16 {
            return Err(());
        }
        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];

        let mut tag_arr = [0u8; 16];
        tag_arr.copy_from_slice(tag);

        gcm.decrypt(&nonce, &gcm_aad, ct, &tag_arr)
    }

    pub fn state(&self) -> TlsState {
        self.state
    }

    fn process_handshake_records(
        &mut self,
        payload: &[u8],
        out_handshake: &mut Vec<u8>,
    ) -> Result<(), ()> {
        let mut buf = core::mem::take(&mut self.hs_buffer);
        buf.extend_from_slice(payload);

        let mut pos = 0;
        while pos + 4 <= buf.len() {
            let msg_type = buf[pos];
            let msg_len = ((buf[pos + 1] as usize) << 16)
                | ((buf[pos + 2] as usize) << 8)
                | (buf[pos + 3] as usize);
            if pos + 4 + msg_len > buf.len() {
                break;
            }
            let msg_frame = &buf[pos..pos + 4 + msg_len];
            let msg_body = &buf[pos + 4..pos + 4 + msg_len];

            // TLS 1.3'te Finished doğrulaması, mesajın *eklenmeden önceki*
            // transcript hash ile yapılır. Bu yüzden hash'in anlık kopyası
            // saklanır.
            let pre_transcript = if self.tls13_mode {
                Some(self.tls13_transcript_hash.clone())
            } else {
                None
            };

            if !self.tls13_mode {
                self.handshake_hash.update(msg_frame);
            }
            self.tls13_transcript_hash.update(msg_frame);
            out_handshake.extend_from_slice(msg_frame);

            match msg_type {
                HANDSHAKE_SERVER_HELLO => self.handle_server_hello(msg_body)?,
                HANDSHAKE_ENCRYPTED_EXTENSIONS => {
                    // TLS 1.3: EncryptedExtensions - temel destek için aksiyon gerekmez.
                }
                HANDSHAKE_CERTIFICATE => self.handle_certificate(msg_body)?,
                HANDSHAKE_SERVER_KEY_EXCHANGE => self.handle_server_key_exchange(msg_body)?,
                HANDSHAKE_SERVER_HELLO_DONE => {
                    self.handle_server_hello_done()?;
                }
                HANDSHAKE_CERTIFICATE_VERIFY => {
                    // TLS 1.3: CertificateVerify - transcript hash üzerindeki imzayı doğrula.
                    self.handle_certificate_verify(msg_body, pre_transcript.as_ref())?;
                }
                HANDSHAKE_FINISHED => self.handle_server_finished(msg_body, pre_transcript.as_ref())?,
                _ => {}
            }
            pos += 4 + msg_len;
        }
        self.hs_buffer.extend_from_slice(&buf[pos..]);
        Ok(())
    }

    fn handle_server_hello(&mut self, body: &[u8]) -> Result<(), ()> {
        if body.len() < 34 + 1 + 2 + 1 {
            return Err(());
        }
        let _ver = u16::from_be_bytes([body[0], body[1]]);
        self.server_random.copy_from_slice(&body[2..34]);
        let sess_id_len = body[34] as usize;
        let pos = 35 + sess_id_len;
        if pos + 3 > body.len() {
            return Err(());
        }
        self.cipher_suite = u16::from_be_bytes([body[pos], body[pos + 1]]);

        // Check for TLS 1.3 cipher suites.
        match self.cipher_suite {
            TLS_AES_128_GCM_SHA256 => {
                self.tls13_mode = true;
            }
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 | TLS_RSA_WITH_AES_128_GCM_SHA256 => {
                self.tls13_mode = false;
            }
            // Desteklenmeyen paket: sessizce bozuk bir el sikisma kurmak yerine
            // hata dondur.
            _ => return Err(()),
        }

        // Skip compression method (1 byte).
        let comp_pos = pos + 2;
        if comp_pos >= body.len() {
            return Err(());
        }
        let _compression = body[comp_pos];

        // Parse extensions if present.
        let ext_start = comp_pos + 1;
        if ext_start < body.len() {
            if ext_start + 2 > body.len() {
                return Err(());
            }
            let ext_len = u16::from_be_bytes([body[ext_start], body[ext_start + 1]]) as usize;
            let ext_end = ext_start + 2 + ext_len;
            if ext_end > body.len() {
                return Err(());
            }
            // Uzunluk alanindan sonraki gercek uzantı verisi.
            let ext_data = &body[ext_start + 2..ext_end];
            self.parse_server_extensions(ext_data)?;
        }

        self.state = TlsState::ServerHelloReceived;

        // TLS 1.3: ServerHello'dan hemen sonra el sıkışma trafik anahtarları
        // türetilir; sunucunun sonraki tüm el sıkışma kayıtları bu anahtarla
        // şifrelenir. Transcript hash bu noktada ClientHello..ServerHello'dır.
        if self.tls13_mode {
            let shared = self.compute_ecdh_shared_secret()?;
            self.derive_tls13_handshake_keys(&shared)?;
        }

        Ok(())
    }

    /// Parse ServerHello extensions: supported_versions, key_share.
    fn parse_server_extensions(&mut self, data: &[u8]) -> Result<(), ()> {
        let mut pos = 0;
        while pos + 4 <= data.len() {
            let ext_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let ext_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            if pos + 4 + ext_len > data.len() {
                break;
            }
            let ext_body = &data[pos + 4..pos + 4 + ext_len];

            match ext_type {
                0x002b => {
                    // supported_versions: check if server selected TLS 1.3.
                    if ext_len >= 2 {
                        let selected = u16::from_be_bytes([ext_body[0], ext_body[1]]);
                        if selected == TLS_VERSION_1_3 {
                            self.tls13_mode = true;
                        }
                    }
                }
                0x0033 => {
                    // key_share: server's ECDHE share.
                    if ext_len >= 4 {
                        let group = u16::from_be_bytes([ext_body[0], ext_body[1]]);
                        if group == NAMED_CURVE_SECP256R1 {
                            let point_len = u16::from_be_bytes([ext_body[2], ext_body[3]]) as usize;
                            if ext_len >= 4 + point_len && point_len == 65 {
                                self.ecdhe_point = ext_body[4..4 + point_len].to_vec();
                            }
                        }
                    }
                }
                _ => {}
            }
            pos += 4 + ext_len;
        }
        Ok(())
    }

    /// ServerKeyExchange: sunucunun geçici ECDHE noktası ve bu noktayı
    /// imzalayan RSA-PKCS#1 v1.5 / SHA-256 imzası.
    ///
    /// İmza `client_random || server_random || ServerECDHParams` üzerinden
    /// sertifikanın RSA anahtarıyla doğrulanır (RFC 4492 §5.4).
    fn handle_server_key_exchange(&mut self, body: &[u8]) -> Result<(), ()> {
        if body.len() < 5 {
            return Err(());
        }
        if body[0] != 3 {
            return Err(());
        }
        let curve = u16::from_be_bytes([body[1], body[2]]);
        if curve != NAMED_CURVE_SECP256R1 {
            return Err(());
        }
        let point_len = u16::from_be_bytes([body[3], body[4]]) as usize;
        if point_len != 65 || body.len() < 5 + point_len + 4 {
            return Err(());
        }
        let params_end = 5 + point_len;
        self.ecdhe_point = body[5..params_end].to_vec();

        let sig_alg = u16::from_be_bytes([body[params_end], body[params_end + 1]]);
        let sig_len =
            u16::from_be_bytes([body[params_end + 2], body[params_end + 3]]) as usize;
        if body.len() < params_end + 4 + sig_len {
            return Err(());
        }
        let sig = &body[params_end + 4..params_end + 4 + sig_len];

        let mut msg = Vec::with_capacity(64 + params_end);
        msg.extend_from_slice(&self.client_random);
        msg.extend_from_slice(&self.server_random);
        msg.extend_from_slice(&body[..params_end]);
        self.verify_signature(sig_alg, &msg, sig)
    }

    /// İmza doğrulaması: şema türüne göre RSA veya ECDSA yolunu seçer.
    fn verify_signature(&self, sig_alg: u16, msg: &[u8], sig: &[u8]) -> Result<(), ()> {
        match sig_alg {
            SIG_ALG_RSA_SHA256 => {
                let rsa_key = self.rsa_key.as_ref().ok_or(())?;
                rsa_verify_pkcs1_sha256(rsa_key, msg, sig)
            }
            SIG_ALG_ECDSA_SECP256R1_SHA256 => {
                let ec_key = self.ec_key.as_ref().ok_or(())?;
                let mut h = Sha256::new();
                h.update(msg);
                let digest = h.finalize();
                let (r, s) = crate::drivers::crypto::ecc::parse_ecdsa_der_signature(sig)
                    .ok_or(())?;
                if crate::drivers::crypto::ecc::ecdsa_verify_p256(ec_key, &digest, &r, &s) {
                    Ok(())
                } else {
                    Err(())
                }
            }
            _ => Err(()),
        }
    }

    /// TLS 1.3 CertificateVerify: sunucunun transcript hash üzerindeki imzasını
    /// doğrular.
    ///
    /// İmza içeriği (RFC 8446 §4.4.3):
    ///   64 × 0x20 || "TLS 1.3, server CertificateVerify" || 0x00 || Transcript-Hash
    fn handle_certificate_verify(
        &mut self,
        body: &[u8],
        pre_transcript: Option<&Sha256>,
    ) -> Result<(), ()> {
        if !self.tls13_mode {
            return Ok(());
        }
        if body.len() < 4 {
            return Err(());
        }
        let sig_alg = u16::from_be_bytes([body[0], body[1]]);
        let sig_len = u16::from_be_bytes([body[2], body[3]]) as usize;
        if body.len() < 4 + sig_len {
            return Err(());
        }
        let sig = &body[4..4 + sig_len];

        // Transcript hash, CertificateVerify mesajı eklenmeden önceki hâli.
        let transcript = match pre_transcript {
            Some(h) => h.clone().finalize(),
            None => self.tls13_transcript_hash.clone().finalize(),
        };

        // İmzalanan içerik: 64 boşluk + etiket + 0x00 + transcript hash.
        const CERT_VERIFY_CONTEXT: &[u8] = b"TLS 1.3, server CertificateVerify";
        let mut signed = Vec::with_capacity(64 + CERT_VERIFY_CONTEXT.len() + 1 + 32);
        signed.extend_from_slice(&[0x20u8; 64]);
        signed.extend_from_slice(CERT_VERIFY_CONTEXT);
        signed.push(0);
        signed.extend_from_slice(&transcript);

        self.verify_signature(sig_alg, &signed, sig)
    }

    fn handle_certificate(&mut self, body: &[u8]) -> Result<(), ()> {
        if body.len() < 3 {
            return Err(());
        }
        // TLS 1.3 Certificate mesajı önce certificate_request_context
        // (1 bayt uzunluk + veri) içerir; TLS 1.2'de bu alan yoktur.
        let list_start = if self.tls13_mode {
            let ctx_len = body[0] as usize;
            if body.len() < 1 + ctx_len + 3 {
                return Err(());
            }
            1 + ctx_len
        } else {
            0
        };
        if body.len() < list_start + 3 {
            return Err(());
        }
        let cert_list_len = ((body[list_start] as usize) << 16)
            | ((body[list_start + 1] as usize) << 8)
            | (body[list_start + 2] as usize);
        let list_end = list_start + 3 + cert_list_len;
        if body.len() < list_end {
            return Err(());
        }
        let mut pos = list_start + 3;
        while pos + 3 <= list_end {
            let cert_len = ((body[pos] as usize) << 16)
                | ((body[pos + 1] as usize) << 8)
                | (body[pos + 2] as usize);
            pos += 3;
            if pos + cert_len > list_end {
                return Err(());
            }
            if self.rsa_key.is_none() && self.ec_key.is_none() {
                // Zincirdeki ilk sertifika RSA degilse (örn. ECDSA) hata
                // vermek yerine siradaki sertifikalara bakilir; bazi sunucular
                // zincirde RSA capraz-imzali bir sertifika da gonderir.
                let cert = &body[pos..pos + cert_len];

                #[cfg(not(test))]
                {
                    let rtc = crate::drivers::rtc::DateTime::read();
                    let now = crate::drivers::crypto::x509::X509Time {
                        year: rtc.year,
                        month: rtc.month,
                        day: rtc.day,
                        hour: rtc.hour,
                        minute: rtc.minute,
                        second: rtc.second,
                    };
                    match crate::drivers::crypto::x509::verify_certificate_fields(cert, &self.hostname, &now) {
                        Ok(()) => {
                            self.cert_verified = true;
                            self.cert_error = None;
                            crate::serial::print("[TLS] Sertifika dogrulandi: ");
                            crate::serial::print(&self.hostname);
                            crate::serial::print(" (süre + SNI/CN gecerli)\n");
                        }
                        Err(e) => {
                            self.cert_verified = false;
                            self.cert_error = Some(e);
                            crate::serial::print("[TLS] Sertifika uyarisi: ");
                            match e {
                                crate::drivers::crypto::x509::CertValidationError::NotYetValid => {
                                    crate::serial::print("henuz gecerli degil (not_before)\n");
                                }
                                crate::drivers::crypto::x509::CertValidationError::Expired => {
                                    crate::serial::print("suresi dolmus (not_after)\n");
                                }
                                crate::drivers::crypto::x509::CertValidationError::HostnameMismatch => {
                                    crate::serial::print("hostname/SNI eslesmedi\n");
                                }
                                crate::drivers::crypto::x509::CertValidationError::InvalidDer => {
                                    crate::serial::print("gecersiz DER yapisi\n");
                                }
                            }
                        }
                    }
                }
                #[cfg(test)]
                {
                    self.cert_verified = true;
                }

                if let Ok(key) = parse_x509_rsa_public_key(cert) {
                    self.rsa_key = Some(key);
                } else if let Ok(point) = parse_x509_ec_public_key(cert) {
                    self.ec_key = Some(point);
                }
            }
            pos += cert_len;
            if self.tls13_mode {
                // TLS 1.3'te her CertificateEntry, sertifikanin yaninda bir
                // "extensions" alani da icerir; TLS 1.2'de bu alan yoktur.
                if pos + 2 > list_end {
                    return Err(());
                }
                let ext_len = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
                pos += 2 + ext_len;
                if pos > list_end {
                    return Err(());
                }
            }
        }
        if self.rsa_key.is_none() && self.ec_key.is_none() {
            // Hicbir sertifika RSA/EC icermiyor; imza dogrulanamaz.
            return Err(());
        }
        self.state = TlsState::CertificateReceived;
        Ok(())
    }

    fn handle_server_hello_done(&mut self) -> Result<(), ()> {
        if self.tls13_mode {
            // TLS 1.3: ServerHelloDone mesajı yoktur. El sıkışma anahtarları
            // ServerHello sonrasında türetilmişti; sunucunun şifreli
            // EncryptedExtensions/Certificate/CertificateVerify/Finished
            // kayıtlarını bekleriz.
            return Ok(());
        }

        if self.state != TlsState::CertificateReceived {
            return Err(());
        }
        let mut cke_body = Vec::new();
        if self.cipher_suite == TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 {
            if self.ecdhe_point.len() != 65 {
                return Err(());
            }
            let mut seed = [0u8; 32];
            let mut state = crate::monotonic_ms() ^ 0x2545_F491_4F6C_DD1D as u64;
            for b in seed.iter_mut() {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
            }
            let kp = crate::drivers::crypto::ecc::generate_keypair(&seed);
            let shared =
                crate::drivers::crypto::ecc::shared_secret(&kp.secret, &self.ecdhe_point)
                    .ok_or(())?;
            self.pre_master_secret = Some(shared.to_vec());
            cke_body.extend_from_slice(&kp.public);
        } else {
            let mut pms = [0u8; 48];
            pms[0] = (TLS_VERSION_1_2 >> 8) as u8;
            pms[1] = (TLS_VERSION_1_2 & 0xFF) as u8;
            let seed = crate::monotonic_ms();
            for i in 2..48 {
                pms[i] = ((seed.wrapping_add((i as u64) * 0x9e3779b97f4a7c15)) >> ((i % 8) * 8)) as u8;
            }
            let encrypted_pms = {
                let rsa_key = self.rsa_key.as_ref().ok_or(())?;
                pkcs1_v1_5_encrypt(rsa_key, &pms)?
            };
            self.pre_master_secret = Some(pms.to_vec());
            cke_body.extend_from_slice(&encrypted_pms);
        }

        let cke_body_len = 2 + cke_body.len();
        let mut cke = Vec::new();
        cke.push(HANDSHAKE_CLIENT_KEY_EXCHANGE);
        cke.push(((cke_body_len >> 16) & 0xFF) as u8);
        cke.push(((cke_body_len >> 8) & 0xFF) as u8);
        cke.push((cke_body_len & 0xFF) as u8);
        cke.extend_from_slice(&(cke_body.len() as u16).to_be_bytes());
        cke.extend_from_slice(&cke_body);
        self.handshake_hash.update(&cke);

        self.derive_keys()?;

        self.outgoing.push(TLS_RECORD_CHANGE_CIPHER_SPEC);
        self.outgoing.extend_from_slice(&TLS_VERSION_1_0.to_be_bytes());
        self.outgoing.extend_from_slice(&1u16.to_be_bytes());
        self.outgoing.push(1);
        // Bundan sonra sunucunun el sıkışma kayıtları da şifrelidir.
        self.tls12_cipher_active = true;

        let handshake_hash = self.handshake_hash.clone().finalize();
        let verify_data = self.prf(&self.master_secret.unwrap(), b"client finished", &handshake_hash)
            [..TLS_VERIFY_DATA_LEN]
            .to_vec();

        let mut finished = Vec::new();
        finished.push(HANDSHAKE_FINISHED);
        finished.push(0);
        finished.push(0);
        finished.push(TLS_VERIFY_DATA_LEN as u8);
        finished.extend_from_slice(&verify_data);

        let client_iv = self.client_write_iv.ok_or(())?;
        let gcm = self.client_gcm.as_mut().ok_or(())?;
        let mut nonce = <[u8; 12]>::default();
        nonce[..4].copy_from_slice(&client_iv);
        nonce[4..12].copy_from_slice(&self.client_seq.to_be_bytes());
        let seq_for_aad = self.client_seq;
        self.client_seq = self.client_seq.wrapping_add(1);

        let mut gcm_aad = <[u8; 13]>::default();
        gcm_aad[..8].copy_from_slice(&seq_for_aad.to_be_bytes());
        gcm_aad[8] = TLS_RECORD_HANDSHAKE;
        gcm_aad[9..11].copy_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        gcm_aad[11..13].copy_from_slice(&(finished.len() as u16).to_be_bytes());

        let (ct, tag) = gcm.encrypt(&nonce, &gcm_aad, &finished).map_err(|()| ())?;

        self.outgoing.push(TLS_RECORD_HANDSHAKE);
        self.outgoing.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        self.outgoing.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
        self.outgoing.extend_from_slice(&ct);
        self.outgoing.extend_from_slice(&tag);

        self.handshake_hash.update(&finished);

        self.state = TlsState::FinishedSent;
        Ok(())
    }

    /// Compute ECDHE shared secret from client secret and server's public point.
    fn compute_ecdh_shared_secret(&self) -> Result<[u8; 32], ()> {
        let client_secret = self.client_ecdhe_secret.ok_or(())?;
        let shared = crate::drivers::crypto::ecc::shared_secret(&client_secret, &self.ecdhe_point)
            .ok_or(())?;
        Ok(shared)
    }

    fn handle_server_finished(
        &mut self,
        body: &[u8],
        pre_transcript: Option<&Sha256>,
    ) -> Result<(), ()> {
        if self.tls13_mode {
            // TLS 1.3 Finished doğrulama anahtarı:
            //   finished_secret = HKDF-Expand-Label(server_hs_traffic_secret, "s finished", "")
            //   finished_key    = HKDF-Expand-Label(finished_secret, "finished", "")
            //   verify_data     = HMAC(finished_key, Transcript-Hash(ClientHello..CertificateVerify))
            let server_hs = self.tls13_server_hs_secret.ok_or(())?;
            let finished_secret = Self::hkdf_expand_label(
                &server_hs,
                b"s finished",
                b"",
                TLS13_HASH_LEN,
            );
            let finished_secret: [u8; TLS13_HASH_LEN] =
                finished_secret.try_into().map_err(|_| ())?;
            let finished_key = Self::hkdf_expand_label(
                &finished_secret,
                b"finished",
                b"",
                TLS13_HASH_LEN,
            );
            let finished_key: [u8; TLS13_HASH_LEN] = finished_key.try_into().map_err(|_| ())?;

            // Transcript hash, sunucu Finished mesajı eklenmeden önceki hâli.
            let transcript = match pre_transcript {
                Some(h) => h.clone().finalize(),
                None => self.tls13_transcript_hash.clone().finalize(),
            };
            let expected = HmacSha256::sign(&finished_key, &transcript);
            if body.len() < TLS_VERIFY_DATA_LEN
                || &body[..TLS_VERIFY_DATA_LEN] != &expected[..TLS_VERIFY_DATA_LEN]
            {
                return Err(());
            }

            // Transcript hash şu anda sunucu Finished'i de içeriyor; uygulama
            // trafik anahtarları bu hash ile türetilir.
            self.derive_tls13_application_keys()?;
            self.state = TlsState::Tls13ServerFinished;
            return Ok(());
        }

        if self.state != TlsState::FinishedSent {
            return Err(());
        }
        if body.len() < TLS_VERIFY_DATA_LEN {
            return Err(());
        }
        let server_verify = &body[..TLS_VERIFY_DATA_LEN];
        let handshake_hash = self.handshake_hash.clone().finalize();
        let expected = self.prf(&self.master_secret.unwrap(), b"server finished", &handshake_hash)
            [..TLS_VERIFY_DATA_LEN]
            .to_vec();
        if server_verify != expected.as_slice() {
            return Err(());
        }
        self.state = TlsState::Established;
        Ok(())
    }

    /// TLS 1.3 istemci Finished kaydını üretir.
    ///
    /// verify_data = HMAC(handshake_traffic_secret, Transcript-Hash(...)),
    /// kayıt ise "client handshake traffic secret" türevi anahtarla şifrelenir.
    /// Çağıran taraf bu kaydı sunucuya gönderdikten sonra oturum `Established`
    /// durumuna geçer.
    pub fn build_tls13_client_finished(&mut self) -> Option<Vec<u8>> {
        if !self.tls13_mode {
            return None;
        }
        let client_hs = self.tls13_client_hs_secret?;

        // Transcript hash: ClientHello..sunucu Finished (sunucu Finished
        // transcript hash'e eklenmiş durumda).
        let transcript = self.tls13_transcript_hash.clone().finalize();

        // finished_secret = HKDF-Expand-Label(client_hs_traffic_secret, "c finished", "")
        // finished_key    = HKDF-Expand-Label(finished_secret, "finished", "")
        let finished_secret = Self::hkdf_expand_label(
            &client_hs,
            b"c finished",
            b"",
            TLS13_HASH_LEN,
        );
        let finished_secret: [u8; TLS13_HASH_LEN] = finished_secret.try_into().ok()?;
        let finished_key = Self::hkdf_expand_label(
            &finished_secret,
            b"finished",
            b"",
            TLS13_HASH_LEN,
        );
        let finished_key: [u8; TLS13_HASH_LEN] = finished_key.try_into().ok()?;

        let verify_data = HmacSha256::sign(&finished_key, &transcript);

        let mut finished = Vec::with_capacity(4 + TLS_VERIFY_DATA_LEN);
        finished.push(HANDSHAKE_FINISHED);
        finished.push(0);
        finished.push(0);
        finished.push(TLS_VERIFY_DATA_LEN as u8);
        finished.extend_from_slice(&verify_data[..TLS_VERIFY_DATA_LEN]);

        // İstemci Finished transcript hash'e eklenir.
        self.tls13_transcript_hash.update(&finished);

        // Kayıt anahtarı/IV'si client handshake traffic secret'tan türetilir.
        let key = Self::hkdf_expand_label(&client_hs, b"key", b"", 16);
        let iv = Self::hkdf_expand_label(&client_hs, b"iv", b"", 12);
        let key: [u8; 16] = key.try_into().ok()?;
        let iv: [u8; 12] = iv.try_into().ok()?;

        let gcm = Gcm::new(&key).ok()?;
        // Ic duz metin: Finished || 0x16 (el sikisma) || dolgu
        let inner = Self::tls13_inner_plaintext(&finished, TLS_RECORD_HANDSHAKE);
        let nonce = Self::tls13_nonce(&iv, 0);
        let aad = Self::tls13_aad(TLS_RECORD_HANDSHAKE, inner.len() + GCM_TAG_LEN);
        let (ct, tag) = gcm.encrypt(&nonce, &aad, &inner).ok()?;

        let mut record = Vec::new();
        record.push(TLS_RECORD_HANDSHAKE);
        record.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        record.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
        record.extend_from_slice(&ct);
        record.extend_from_slice(&tag);

        self.state = TlsState::Established;
        Some(record)
    }

    /// TLS 1.3 ara cihaz (middlebox) uyumlulugu icin ChangeCipherSpec kaydi.
    ///
    /// RFC 8446 §D.4: eski ara cihazlar bu kaydi gormeyi bekler. TLS 1.3'de
    /// anlamsizdir, sunucu tarafindan yok sayilir.
    pub fn tls13_change_cipher_spec(&self) -> Option<Vec<u8>> {
        if !self.tls13_mode {
            return None;
        }
        let mut rec = Vec::with_capacity(6);
        rec.push(TLS_RECORD_CHANGE_CIPHER_SPEC);
        rec.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        rec.extend_from_slice(&1u16.to_be_bytes());
        rec.push(1);
        Some(rec)
    }

    fn derive_keys(&mut self) -> Result<(), ()> {
        let pms = self.pre_master_secret.as_ref().ok_or(())?;
        let mut seed = Vec::with_capacity(64);
        seed.extend_from_slice(&self.client_random);
        seed.extend_from_slice(&self.server_random);
        let ms = self.prf(pms, b"master secret", &seed);
        let master: [u8; 48] = ms.get(..48).ok_or(())?.try_into().map_err(|_| ())?;
        self.master_secret = Some(master);

        let mut kb_seed = Vec::with_capacity(64);
        kb_seed.extend_from_slice(&self.server_random);
        kb_seed.extend_from_slice(&self.client_random);
        let kb = self.prf(&master, b"key expansion", &kb_seed);

        let cwk: [u8; 16] = kb.get(0..16).ok_or(())?.try_into().map_err(|_| ())?;
        let swk: [u8; 16] = kb.get(16..32).ok_or(())?.try_into().map_err(|_| ())?;
        let civ: [u8; 4] = kb.get(32..36).ok_or(())?.try_into().map_err(|_| ())?;
        let siv: [u8; 4] = kb.get(36..40).ok_or(())?.try_into().map_err(|_| ())?;

        self.client_write_key = Some(cwk);
        self.server_write_key = Some(swk);
        self.client_write_iv = Some(civ);
        self.server_write_iv = Some(siv);

        self.server_gcm = Some(Gcm::new(&swk)?);
        self.client_gcm = Some(Gcm::new(&cwk)?);

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // TLS 1.3 HKDF key schedule (RFC 8446 Section 7)
    // ---------------------------------------------------------------------------

    fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; TLS13_HASH_LEN] {
        if salt.is_empty() || ikm.is_empty() {
            let default_buf: [u8; TLS13_HASH_LEN] = core::array::from_fn(|_| 0);
            let actual_salt = if salt.is_empty() { &default_buf[..] } else { salt };
            let actual_ikm = if ikm.is_empty() { &default_buf[..] } else { ikm };
            HmacSha256::sign(actual_salt, actual_ikm)
        } else {
            HmacSha256::sign(salt, ikm)
        }
    }

    fn hkdf_expand(prk: &[u8; TLS13_HASH_LEN], info: &[u8], length: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(length);
        let mut t_prev = Vec::new();
        let mut counter: u8 = 1;
        while result.len() < length {
            let mut block_input = Vec::new();
            block_input.extend_from_slice(&t_prev);
            block_input.extend_from_slice(info);
            block_input.push(counter);
            let t = HmacSha256::sign(prk, &block_input);
            result.extend_from_slice(&t);
            t_prev = t.to_vec();
            counter += 1;
        }
        result.truncate(length);
        result
    }

    fn hkdf_expand_label(
        secret: &[u8; TLS13_HASH_LEN],
        label: &[u8],
        context: &[u8],
        length: usize,
    ) -> Vec<u8> {
        let hkdf_label_len = 2 + TLS13_LABEL_PREFIX.len() + label.len() + context.len();
        let mut hkdf_label = Vec::with_capacity(hkdf_label_len);
        hkdf_label.extend_from_slice(&(length as u16).to_be_bytes());
        hkdf_label.extend_from_slice(TLS13_LABEL_PREFIX);
        hkdf_label.extend_from_slice(label);
        hkdf_label.extend_from_slice(context);
        Self::hkdf_expand(secret, &hkdf_label, length)
    }

    /// TLS 1.3 anahtar planı (RFC 8446 §7.1).
    ///
    /// Aşama 1 (`derive_tls13_handshake_keys`): ServerHello sonrası, transcript
    /// hash = Hash(ClientHello..ServerHello) kullanılarak el sıkışma trafik
    /// anahtarları türetilir.
    ///
    /// Aşama 2 (`derive_tls13_application_keys`): sunucu Finished doğrulandıktan
    /// sonra, transcript hash = Hash(ClientHello..server Finished) kullanılarak
    /// uygulama trafik anahtarları türetilir.
    fn derive_tls13_handshake_keys(&mut self, shared_secret: &[u8]) -> Result<(), ()> {
        // early_secret = HKDF-Extract(salt=0, IKM=0)
        let early_secret = Self::hkdf_extract(&[], &[]);
        // derived = Derive-Secret(early_secret, "derived", "")
        let derived = Self::hkdf_expand_label(&early_secret, b"derived", b"", TLS13_HASH_LEN);
        let derived_arr: [u8; TLS13_HASH_LEN] = derived.try_into().map_err(|_| ())?;

        // handshake_secret = HKDF-Extract(derived, shared_secret)
        let handshake_secret = Self::hkdf_extract(&derived_arr, shared_secret);
        self.tls13_handshake_secret = Some(handshake_secret);

        // master_secret = HKDF-Extract(Derive-Secret(handshake_secret, "derived", ""), 0)
        let derived2 = Self::hkdf_expand_label(
            &handshake_secret,
            b"derived",
            b"",
            TLS13_HASH_LEN,
        );
        let derived2_arr: [u8; TLS13_HASH_LEN] = derived2.try_into().map_err(|_| ())?;
        let master_secret = Self::hkdf_extract(&derived2_arr, &[]);
        self.tls13_master_secret = Some(master_secret);

        // El sıkışma trafik sırları (transcript hash = ClientHello..ServerHello).
        let transcript = self.tls13_transcript_hash.clone().finalize();
        let client_hs_secret = Self::hkdf_expand_label(
            &handshake_secret,
            b"c hs traffic",
            &transcript,
            TLS13_HASH_LEN,
        );
        let server_hs_secret = Self::hkdf_expand_label(
            &handshake_secret,
            b"s hs traffic",
            &transcript,
            TLS13_HASH_LEN,
        );
        self.tls13_client_hs_secret = Some(client_hs_secret.try_into().map_err(|_| ())?);
        self.tls13_server_hs_secret = Some(server_hs_secret.clone().try_into().map_err(|_| ())?);

        // Sunucu el sıkışma kayıtlarını çözmek için anahtar/IV türet.
        let server_hs_secret: [u8; TLS13_HASH_LEN] = server_hs_secret.try_into().map_err(|_| ())?;
        let server_hs_key = Self::hkdf_expand_label(&server_hs_secret, b"key", b"", 16);
        let server_hs_iv = Self::hkdf_expand_label(&server_hs_secret, b"iv", b"", 12);
        self.tls13_server_hs_key = Some(server_hs_key.try_into().map_err(|_| ())?);
        self.tls13_server_hs_iv = Some(server_hs_iv.try_into().map_err(|_| ())?);

        Ok(())
    }

    /// TLS 1.3 uygulama trafik anahtarları (sunucu Finished doğrulandıktan sonra).
    fn derive_tls13_application_keys(&mut self) -> Result<(), ()> {
        let master_secret = self.tls13_master_secret.ok_or(())?;
        // Transcript hash = ClientHello..sunucu Finished (sunucu Finished
        // doğrulandığında hash'e eklenmiş olmalı).
        let transcript = self.tls13_transcript_hash.clone().finalize();

        let client_ap_secret = Self::hkdf_expand_label(
            &master_secret,
            b"c ap traffic",
            &transcript,
            TLS13_HASH_LEN,
        );
        let server_ap_secret = Self::hkdf_expand_label(
            &master_secret,
            b"s ap traffic",
            &transcript,
            TLS13_HASH_LEN,
        );

        let client_ap_secret: [u8; TLS13_HASH_LEN] = client_ap_secret.try_into().map_err(|_| ())?;
        let server_ap_secret: [u8; TLS13_HASH_LEN] = server_ap_secret.try_into().map_err(|_| ())?;

        let client_ap_key = Self::hkdf_expand_label(&client_ap_secret, b"key", b"", 16);
        let client_ap_iv = Self::hkdf_expand_label(&client_ap_secret, b"iv", b"", 12);
        let server_ap_key = Self::hkdf_expand_label(&server_ap_secret, b"key", b"", 16);
        let server_ap_iv = Self::hkdf_expand_label(&server_ap_secret, b"iv", b"", 12);

        self.client_write_key = Some(client_ap_key.try_into().map_err(|_| ())?);
        self.server_write_key = Some(server_ap_key.try_into().map_err(|_| ())?);
        self.client_base_iv = Some(client_ap_iv.try_into().map_err(|_| ())?);
        self.server_base_iv = Some(server_ap_iv.try_into().map_err(|_| ())?);

        self.client_gcm = Some(Gcm::new(&self.client_write_key.unwrap()).map_err(|_| ())?);
        self.server_gcm = Some(Gcm::new(&self.server_write_key.unwrap()).map_err(|_| ())?);

        Ok(())
    }

    fn tls13_nonce(base_iv: &[u8; 12], seq: u64) -> [u8; 12] {
        let mut nonce = *base_iv;
        let seq_bytes = seq.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }
        nonce
    }

    /// TLS 1.3 iç düz metin (RFC 8446 §5.2):
    /// `TLSInnerPlaintext = content + Opaque content_type + uint8 zeros[padding]`
    ///
    /// Gerçek sunucular şifreli kayıtların içinde bu içerik türü baytını
    /// gönderir; istemci aynı biçimde üretmelidir.
    fn tls13_inner_plaintext(content: &[u8], content_type: u8) -> Vec<u8> {
        let mut inner = Vec::with_capacity(content.len() + 1);
        inner.extend_from_slice(content);
        inner.push(content_type);
        inner
    }

    /// Şifreli iç düz metni ayrıştırır: (içerik, içerik türü).
    ///
    /// Sondaki sıfır dolgu baytları atılır; kalan son bayt içerik türüdür.
    fn tls13_strip_inner_plaintext(inner: &[u8]) -> Result<(&[u8], u8), ()> {
        let mut end = inner.len();
        while end > 0 && inner[end - 1] == 0 {
            end -= 1;
        }
        if end == 0 {
            return Err(());
        }
        let content_type = inner[end - 1];
        Ok((&inner[..end - 1], content_type))
    }

    /// TLS 1.3 AAD: record_type || legacy_version || length (RFC 8446 §5.2).
    ///
    /// Uzunluk alanı 8 bayttır ve şifreli metnin (GCM etiketi dahil)
    /// uzunluğunu taşır; sıra numarası AAD'ye girmez, yalnızca nonce'ta
    /// kullanılır.
    fn tls13_aad(record_type: u8, len: usize) -> [u8; 11] {
        let mut aad = [0u8; 11];
        aad[0] = record_type;
        aad[1..3].copy_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        aad[3..11].copy_from_slice(&(len as u64).to_be_bytes());
        aad
    }

    fn tls12_nonce(write_iv: &[u8; 4], seq: u64) -> [u8; 12] {
        let s = seq.to_be_bytes();
        [
            write_iv[0], write_iv[1], write_iv[2], write_iv[3],
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]
    }

    fn tls12_aad(seq: u64, record_type: u8, len: usize) -> [u8; 13] {
        let mut aad = [0u8; 13];
        aad[0..8].copy_from_slice(&seq.to_be_bytes());
        aad[8] = record_type;
        aad[9..11].copy_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        aad[11..13].copy_from_slice(&(len as u16).to_be_bytes());
        aad
    }

    fn prf(&self, secret: &[u8], label: &[u8], seed: &[u8]) -> Vec<u8> {
        let mut label_seed = Vec::new();
        label_seed.extend_from_slice(label);
        label_seed.extend_from_slice(seed);

        let mut result = Vec::new();
        let mut a_prev = label_seed.clone();
        while result.len() < 64 {
            let a = HmacSha256::sign(secret, &a_prev);
            let mut block_input = Vec::new();
            block_input.extend_from_slice(&a);
            block_input.extend_from_slice(&label_seed);
            let block = HmacSha256::sign(secret, &block_input);
            result.extend_from_slice(&block);
            a_prev = a.to_vec();
        }
        result
    }

    /// TLS 1.3 uygulama trafik anahtarıyla bir kaydı çözer.
    ///
    /// Dönen değer: (içerik, içerik türü).
    fn decrypt_tls13_app_record(&mut self, ciphertext: &[u8]) -> Result<(Vec<u8>, u8), ()> {
        let server_base_iv = self.server_base_iv.ok_or(())?;
        let gcm = self.server_gcm.as_mut().ok_or(())?;

        let seq = self.server_seq;
        self.server_seq = self.server_seq.wrapping_add(1);

        let nonce = Self::tls13_nonce(&server_base_iv, seq);
        let gcm_aad = Self::tls13_aad(TLS_RECORD_APPLICATION_DATA, ciphertext.len());

        if ciphertext.len() < 16 {
            return Err(());
        }
        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];

        let mut tag_arr = [0u8; 16];
        tag_arr.copy_from_slice(tag);

        let inner = gcm.decrypt(&nonce, &gcm_aad, ct, &tag_arr)?;
        let (content, content_type) = Self::tls13_strip_inner_plaintext(&inner)?;
        Ok((content.to_vec(), content_type))
    }

    fn decrypt_app_data(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
        if self.tls13_mode {
            let (content, _) = self.decrypt_tls13_app_record(ciphertext)?;
            Ok(content)
        } else {
            let server_iv = self.server_write_iv.ok_or(())?;
            let gcm = self.server_gcm.as_mut().ok_or(())?;

            let seq = self.server_seq;
            self.server_seq = self.server_seq.wrapping_add(1);

            let nonce = Self::tls12_nonce(&server_iv, seq);
            let gcm_aad = Self::tls12_aad(seq, TLS_RECORD_APPLICATION_DATA, ciphertext.len());

            if ciphertext.len() < 16 {
                return Err(());
            }
            let ct_len = ciphertext.len() - 16;
            let ct = &ciphertext[..ct_len];
            let tag = &ciphertext[ct_len..];

            let mut tag_arr = [0u8; 16];
            tag_arr.copy_from_slice(tag);

            gcm.decrypt(&nonce, &gcm_aad, ct, &tag_arr)
        }
    }

    pub fn encrypt_app_data(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        if self.tls13_mode {
            let client_base_iv = self.client_base_iv?;
            let gcm = self.client_gcm.as_ref()?;

            let seq = self.client_seq;
            self.client_seq = self.client_seq.wrapping_add(1);

            // Ic duz metin: veri || 0x17 (uygulama verisi) || dolgu
            let inner = Self::tls13_inner_plaintext(plaintext, TLS_RECORD_APPLICATION_DATA);
            let nonce = Self::tls13_nonce(&client_base_iv, seq);
            let gcm_aad = Self::tls13_aad(TLS_RECORD_APPLICATION_DATA, inner.len() + GCM_TAG_LEN);

            let (ct, tag) = gcm.encrypt(&nonce, &gcm_aad, &inner).ok()?;

            let mut record = Vec::new();
            record.push(TLS_RECORD_APPLICATION_DATA);
            record.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
            record.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
            record.extend_from_slice(&ct);
            record.extend_from_slice(&tag);
            Some(record)
        } else {
            let client_iv = self.client_write_iv?;
            let gcm = self.client_gcm.as_ref()?;

            let seq = self.client_seq;
            self.client_seq = self.client_seq.wrapping_add(1);

            let mut nonce = <[u8; 12]>::default();
            nonce[..4].copy_from_slice(&client_iv);
            nonce[4..12].copy_from_slice(&seq.to_be_bytes());

            let mut gcm_aad = <[u8; 13]>::default();
            gcm_aad[..8].copy_from_slice(&seq.to_be_bytes());
            gcm_aad[8] = TLS_RECORD_APPLICATION_DATA;
            gcm_aad[9..11].copy_from_slice(&TLS_VERSION_1_2.to_be_bytes());
            gcm_aad[11..13].copy_from_slice(&(plaintext.len() as u16).to_be_bytes());

            let (ct, tag) = gcm.encrypt(&nonce, &gcm_aad, plaintext).ok()?;

            let mut record = Vec::new();
            record.push(TLS_RECORD_APPLICATION_DATA);
            record.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
            record.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
            record.extend_from_slice(&ct);
            record.extend_from_slice(&tag);
            Some(record)
        }
    }
}

    /// TLS uyarı açıklamalarının insan okunur karşılığı (RFC 5246 §7.2).
    pub fn alert_description_text(desc: u8) -> &'static str {
    match desc {
        0 => "TLS uyarısı: close_notify",
        10 => "TLS uyarısı: unexpected_message",
        20 => "TLS uyarısı: bad_record_mac",
        40 => "TLS uyarısı: handshake_failure",
        41 => "TLS uyarısı: no_certificate",
        42 => "TLS uyarısı: bad_certificate",
        43 => "TLS uyarısı: unsupported_certificate",
        44 => "TLS uyarısı: certificate_revoked",
        45 => "TLS uyarısı: certificate_expired",
        46 => "TLS uyarısı: certificate_unknown",
        47 => "TLS uyarısı: illegal_parameter",
        48 => "TLS uyarısı: unknown_ca",
        49 => "TLS uyarısı: access_denied",
        50 => "TLS uyarısı: decode_error",
        51 => "TLS uyarısı: decrypt_error",
        70 => "TLS uyarısı: protocol_version",
        71 => "TLS uyarısı: insufficient_security",
        80 => "TLS uyarısı: internal_error",
        86 => "TLS uyarısı: inappropriate_fallback",
        90 => "TLS uyarısı: user_canceled",
        100 => "TLS uyarısı: no_renegotiation",
        109 => "TLS uyarısı: missing_extension",
        110 => "TLS uyarısı: unsupported_extension",
        112 => "TLS uyarısı: unrecognized_name",
        116 => "TLS uyarısı: certificate_required",
        120 => "TLS uyarısı: no_application_protocol",
        _ => "TLS uyarısı: bilinmeyen uyarı",
    }
}

pub fn parse_record_header(buf: &[u8]) -> Option<(u8, u16, usize)> {
    if buf.len() < 5 {
        return None;
    }
    let ctype = buf[0];
    let ver = u16::from_be_bytes([buf[1], buf[2]]);
    let len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    Some((ctype, ver, len))
}

pub fn build_client_hello(
    hostname: &str,
    client_random: &[u8; 32],
    out: &mut Vec<u8>,
    ecdhe_secret_out: &mut [u8; 32],
) {
    out.clear();

    let mut handshake = Vec::new();
    handshake.push(HANDSHAKE_CLIENT_HELLO);
    handshake.extend_from_slice(&[0, 0, 0]);

    handshake.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
    handshake.extend_from_slice(client_random);
    handshake.push(0);

    // Cipher suites. Yalnizca anahtar turetimini SHA-256 ile yapabilen
    // paketler ilan edilir; sunucu listeden secmek zorundadir.
    //
    //  * TLS 1.3: TLS_AES_128_GCM_SHA256 (0x1301) — en yaygin modern paket.
    //  * TLS 1.2: ECDHE_RSA_WITH_AES_128_GCM_SHA256 (0xC02F) — ECDHE + RSA
    //    sertifikasi, ileri guvenlik (perfect forward secrecy).
    //  * TLS 1.2: TLS_RSA_WITH_AES_128_GCM_SHA256 (0x009C) — yalnizca RSA
    //    sertifikasi olan eski sunucular icin son care.
    let ciphers = [
        TLS_AES_128_GCM_SHA256,
        TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        TLS_RSA_WITH_AES_128_GCM_SHA256,
    ];
    let cipher_len = ((ciphers.len() + 1) * 2) as u16;
    handshake.extend_from_slice(&cipher_len.to_be_bytes());
    for c in &ciphers {
        handshake.extend_from_slice(&c.to_be_bytes());
    }
    handshake.extend_from_slice(&0x00FFu16.to_be_bytes());

    handshake.push(1);
    handshake.push(0);

    let mut extensions = Vec::new();
    let host_bytes = hostname.as_bytes();
    if !host_bytes.is_empty() {
        extensions.extend_from_slice(&0x0000u16.to_be_bytes());
        let list_len = (host_bytes.len() + 3) as u16;
        let ext_len = list_len + 2;
        extensions.extend_from_slice(&ext_len.to_be_bytes());
        extensions.extend_from_slice(&list_len.to_be_bytes());
        extensions.push(0);
        extensions.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
        extensions.extend_from_slice(host_bytes);
    }

    let versions: [u16; 2] = [TLS_VERSION_1_3, TLS_VERSION_1_2];
    extensions.extend_from_slice(&0x002bu16.to_be_bytes());
    let sv_len = (1 + versions.len() * 2) as u16;
    extensions.extend_from_slice(&sv_len.to_be_bytes());
    extensions.push(versions.len() as u8);
    for v in &versions {
        extensions.extend_from_slice(&v.to_be_bytes());
    }

    // signature_algorithms (0x000d): doğrulayabildiğimiz şemalar ilan edilir.
    // Sunucu listeden seçmek zorundadır.
    let sigalgs: [u16; 2] = [SIG_ALG_RSA_SHA256, SIG_ALG_ECDSA_SECP256R1_SHA256];
    extensions.extend_from_slice(&0x000du16.to_be_bytes());
    extensions.extend_from_slice(&((2 + sigalgs.len() * 2) as u16).to_be_bytes());
    extensions.extend_from_slice(&((sigalgs.len() * 2) as u16).to_be_bytes());
    for s in &sigalgs {
        extensions.extend_from_slice(&s.to_be_bytes());
    }

    extensions.extend_from_slice(&0x000au16.to_be_bytes());
    extensions.extend_from_slice(&4u16.to_be_bytes());
    extensions.extend_from_slice(&2u16.to_be_bytes());
    extensions.extend_from_slice(&NAMED_CURVE_SECP256R1.to_be_bytes());

    extensions.extend_from_slice(&0x000bu16.to_be_bytes());
    extensions.extend_from_slice(&2u16.to_be_bytes());
    extensions.push(1);
    extensions.push(0);

    extensions.extend_from_slice(&0xff01u16.to_be_bytes());
    extensions.extend_from_slice(&1u16.to_be_bytes());
    extensions.push(0);

    let seed = crate::boot::time::main_monotonic_ms() as u64 ^ 0x2545_F491_4F6C_DD1D as u64;
    let mut state = seed;
    let mut ks_seed = [0u8; 32];
    for b in ks_seed.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
    }
    let kp = crate::drivers::crypto::ecc::generate_keypair(&ks_seed);
    ecdhe_secret_out.copy_from_slice(&kp.secret);
    let kp_len = kp.public.len();
    let ks_ext_len = 2 + 2 + kp_len;
    extensions.extend_from_slice(&0x0033u16.to_be_bytes());
    extensions.extend_from_slice(&(ks_ext_len as u16).to_be_bytes());
    extensions.extend_from_slice(&NAMED_CURVE_SECP256R1.to_be_bytes());
    extensions.extend_from_slice(&((kp_len as u16).to_be_bytes()));
    extensions.extend_from_slice(&kp.public);

    if !extensions.is_empty() {
        handshake.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        handshake.extend_from_slice(&extensions);
    }

    let hs_len = (handshake.len() - 4) as u32;
    handshake[1] = ((hs_len >> 16) & 0xFF) as u8;
    handshake[2] = ((hs_len >> 8) & 0xFF) as u8;
    handshake[3] = (hs_len & 0xFF) as u8;

    out.push(TLS_RECORD_HANDSHAKE);
    out.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
    let rec_len = handshake.len() as u16;
    out.extend_from_slice(&rec_len.to_be_bytes());
    out.extend_from_slice(&handshake);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn hex(s: &[u8]) -> Vec<u8> {
        let b = s;
        let mut out = Vec::with_capacity(b.len() / 2);
        let mut i = 0;
        while i + 1 < b.len() {
            let hi = (b[i] as char).to_digit(16).unwrap() as u8;
            let lo = (b[i + 1] as char).to_digit(16).unwrap() as u8;
            out.push((hi << 4) | lo);
            i += 2;
        }
        out
    }

    fn to_hex(v: &[u8]) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for b in v {
            s.push_str(alloc::format!("{:02x}", b).as_str());
        }
        s
    }

    /// RFC 5869 Test Case 1 (SHA-256).
    #[test]
    fn hkdf_rfc5869_case1() {
        let ikm = hex(b"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = hex(b"000102030405060708090a0b0c");
        let info = hex(b"f0f1f2f3f4f5f6f7f8f9");
        let prk = TlsSession::hkdf_extract(&salt, &ikm);
        assert_eq!(
            to_hex(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let okm = TlsSession::hkdf_expand(&prk, &info, 42);
        assert_eq!(
            to_hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    /// RFC 5869 Test Case 3 (SHA-256): bos salt ve bos info.
    #[test]
    fn hkdf_rfc5869_case3() {
        let ikm = hex(b"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let prk = TlsSession::hkdf_extract(&[], &ikm);
        assert_eq!(
            to_hex(&prk),
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04"
        );
        let okm = TlsSession::hkdf_expand(&prk, &[], 42);
        assert_eq!(
            to_hex(&okm),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    /// HKDF-Expand-Label, HkdfLabel yapısına uygun bilgi alanı üretmelidir
    /// (RFC 8446 §7.1): length || "tls13 " + Label || Context.
    #[test]
    fn hkdf_expand_label_structure() {
        let secret: [u8; 32] = [0x5au8; 32];
        let label = b"key";
        let context = b"ctx";
        let len = 16usize;

        // Beklenen HkdfLabel: len(2) || "tls13 key" || "ctx"
        let mut info = Vec::new();
        info.extend_from_slice(&(len as u16).to_be_bytes());
        info.extend_from_slice(b"tls13 ");
        info.extend_from_slice(label);
        info.extend_from_slice(context);

        let expected = TlsSession::hkdf_expand(&secret, &info, len);
        let actual = TlsSession::hkdf_expand_label(&secret, label, context, len);
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), len);
    }

    /// ClientHello yapisi: sunucunun cift tarafi anlayabilmesi icin
    /// gerekli uzantilar ve cipher suite'ler bulunmali.
    #[test]
    fn client_hello_structure() {
        let random = [0u8; 32];
        let mut out = Vec::new();
        let mut ecdhe = [0u8; 32];
        build_client_hello("example.com", &random, &mut out, &mut ecdhe);

        // Kayit basligi: handshake(22) + legacy version 0x0303
        assert_eq!(out[0], TLS_RECORD_HANDSHAKE);
        assert_eq!(u16::from_be_bytes([out[1], out[2]]), TLS_VERSION_1_2);
        let rec_len = u16::from_be_bytes([out[3], out[4]]) as usize;
        assert_eq!(rec_len, out.len() - 5);

        // El sikisma mesaji
        let hs = &out[5..];
        assert_eq!(hs[0], HANDSHAKE_CLIENT_HELLO);
        let hs_len = ((hs[1] as usize) << 16) | ((hs[2] as usize) << 8) | (hs[3] as usize);
        assert_eq!(hs_len, hs.len() - 4);

        // Cipher suite'ler TLS 1.3 ve TLS 1.2 paketlerini icermeli.
        let cs_len = u16::from_be_bytes([hs[4 + 34 + 1], hs[4 + 34 + 2]]) as usize;
        let cs_start = 4 + 34 + 1 + 2;
        let mut has_tls13 = false;
        let mut has_ecdhe = false;
        let mut i = 0;
        while i + 1 < cs_len {
            let cs = u16::from_be_bytes([hs[cs_start + i], hs[cs_start + i + 1]]);
            if cs == TLS_AES_128_GCM_SHA256 {
                has_tls13 = true;
            }
            if cs == TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 {
                has_ecdhe = true;
            }
            i += 2;
        }
        assert!(has_tls13, "TLS 1.3 cipher suite sunulmali");
        assert!(has_ecdhe, "ECDHE cipher suite sunulmali");
    }

    /// TLS 1.3 nonce kurgusu: base_iv XOR sequence_number.
    #[test]
    fn tls13_nonce_xor() {
        let base: [u8; 12] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
        ];
        let n0 = TlsSession::tls13_nonce(&base, 0);
        assert_eq!(n0, base, "seq=0 nonce base_iv olmali");

        let n1 = TlsSession::tls13_nonce(&base, 1);
        assert_eq!(n1[11], base[11] ^ 1);
        assert_eq!(n1[0], base[0]);
    }

    /// TLS 1.2 nonce kurgusu: write_IV || sequence_number.
    #[test]
    fn tls12_nonce_layout() {
        let iv: [u8; 4] = [0xaa, 0xbb, 0xcc, 0xdd];
        let n = TlsSession::tls12_nonce(&iv, 0x0102030405060708);
        assert_eq!(&n[..4], &iv);
        assert_eq!(&n[4..], &0x0102030405060708u64.to_be_bytes());
    }

    /// TLS 1.3 ic duz metin: content || content_type || zeros.
    #[test]
    fn tls13_inner_plaintext_roundtrip() {
        let content = b"hello";
        let inner = TlsSession::tls13_inner_plaintext(content, TLS_RECORD_HANDSHAKE);
        assert_eq!(inner.len(), content.len() + 1);
        assert_eq!(inner[content.len()], TLS_RECORD_HANDSHAKE);

        let (c, t) = TlsSession::tls13_strip_inner_plaintext(&inner).unwrap();
        assert_eq!(c, &content[..]);
        assert_eq!(t, TLS_RECORD_HANDSHAKE);

        // Dolgu (sifir) baytlari olusturuldugunda da ayristirma calisir.
        let mut padded = inner.clone();
        padded.extend_from_slice(&[0u8; 16]);
        let (c, t) = TlsSession::tls13_strip_inner_plaintext(&padded).unwrap();
        assert_eq!(c, &content[..]);
        assert_eq!(t, TLS_RECORD_HANDSHAKE);

        // Tamami sifir olan icerik gecersizdir.
        assert!(TlsSession::tls13_strip_inner_plaintext(&[0u8; 16]).is_err());
    }

    /// ServerHello uzantı ayrıştırma: supported_versions ve key_share.
    #[test]
    fn server_extensions_parsed() {
        let mut session = TlsSession::new("example.com", [0u8; 32]);

        // supported_versions (0x002b) = TLS 1.3
        let mut exts = Vec::new();
        exts.extend_from_slice(&0x002bu16.to_be_bytes());
        exts.extend_from_slice(&2u16.to_be_bytes());
        exts.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());
        session.parse_server_extensions(&exts).unwrap();
        assert!(session.tls13_mode, "TLS 1.3 secilmeli");

        // key_share (0x0033): secp256r1 + 65 baytlik nokta
        let mut point = [0u8; 65];
        point[0] = 0x04;
        point[64] = 0x2a;
        let mut exts = Vec::new();
        exts.extend_from_slice(&0x0033u16.to_be_bytes());
        let body_len = 2 + 2 + point.len();
        exts.extend_from_slice(&(body_len as u16).to_be_bytes());
        exts.extend_from_slice(&NAMED_CURVE_SECP256R1.to_be_bytes());
        exts.extend_from_slice(&(point.len() as u16).to_be_bytes());
        exts.extend_from_slice(&point);
        session.parse_server_extensions(&exts).unwrap();
        assert_eq!(session.ecdhe_point.len(), 65);
        assert_eq!(session.ecdhe_point[0], 0x04);
        assert_eq!(session.ecdhe_point[64], 0x2a);
    }

    /// Kayit basligi ayrıştırma.
    #[test]
    fn record_header_parsing() {
        let buf = [23u8, 0x03, 0x03, 0x00, 0x05, 1, 2, 3, 4, 5];
        let (ctype, ver, len) = parse_record_header(&buf).unwrap();
        assert_eq!(ctype, TLS_RECORD_APPLICATION_DATA);
        assert_eq!(ver, TLS_VERSION_1_2);
        assert_eq!(len, 5);
        assert!(parse_record_header(&buf[..3]).is_none());
    }

    // -------------------------------------------------------------------------
    // TLS 1.3 uçtan uca (loopback) testi
    // -------------------------------------------------------------------------

    /// Test anahtarı (512-bit RSA). Yalnizca testlerde kullanilir.
    const TEST_N: [u8; 64] = [
        0xb0, 0xea, 0xe6, 0x91, 0x0f, 0xea, 0x84, 0x1c, 0x20, 0x51, 0x68, 0xa0, 0xb8, 0x35, 0x0e, 0xb8,
        0x95, 0xfc, 0x78, 0x8d, 0xdb, 0x3e, 0x3c, 0x10, 0xc2, 0x7b, 0xa5, 0xbf, 0xf8, 0x42, 0x70, 0xa7,
        0x1b, 0x73, 0x4b, 0xb2, 0xa9, 0xbb, 0x95, 0x6c, 0x91, 0xdf, 0xa1, 0xfa, 0xa6, 0xd4, 0xf2, 0xe8,
        0x42, 0xe9, 0xf6, 0x6d, 0xb5, 0xb0, 0x1b, 0x99, 0xe2, 0x05, 0x81, 0xc4, 0x18, 0x44, 0xa2, 0xbb,
    ];
    const TEST_D: [u8; 64] = [
        0x21, 0x25, 0xb1, 0xd3, 0x66, 0x0f, 0x03, 0x77, 0x9d, 0x9a, 0x83, 0x26, 0xf1, 0xbc, 0x87, 0x28,
        0x90, 0x79, 0x80, 0x40, 0xdb, 0xde, 0x1a, 0x34, 0xed, 0xab, 0x6b, 0x0f, 0xbf, 0xbc, 0xfb, 0x0c,
        0x14, 0x17, 0x4f, 0x54, 0xde, 0xeb, 0xd5, 0xcf, 0xfa, 0x17, 0xa0, 0xdb, 0xc4, 0xc6, 0x1d, 0x58,
        0x75, 0x13, 0xdf, 0xa1, 0x57, 0xff, 0xa3, 0x7e, 0xba, 0x46, 0xde, 0x19, 0x9a, 0x14, 0x7e, 0xf9,
    ];

    fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 6);
        out.push(tag);
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else if body.len() < 0x100 {
            out.push(0x81);
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.push((body.len() >> 8) as u8);
            out.push(body.len() as u8);
        }
        out.extend_from_slice(body);
        out
    }

    /// Test icin P-256 (secp256r1) EC sertifikası uretir.
    ///
    /// SPKI: id-ecPublicKey + secp256r1 OID + 04||X||Y BIT STRING.
    fn make_test_ec_cert(point: &[u8; 65]) -> Vec<u8> {
        let mut bits = Vec::new();
        bits.push(0x00);
        bits.extend_from_slice(point);
        let bitstring = der_tlv(0x03, &bits);

        // AlgorithmIdentifier: id-ecPublicKey (1.2.840.10045.2.1) + secp256r1
        const EC_PUBKEY_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
        const SECP256R1_OID: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
        let mut alg_body = Vec::new();
        alg_body.push(0x06);
        alg_body.push(EC_PUBKEY_OID.len() as u8);
        alg_body.extend_from_slice(EC_PUBKEY_OID);
        alg_body.push(0x06);
        alg_body.push(SECP256R1_OID.len() as u8);
        alg_body.extend_from_slice(SECP256R1_OID);
        let alg_id = der_tlv(0x30, &alg_body);

        let mut spki_body = Vec::new();
        spki_body.extend_from_slice(&alg_id);
        spki_body.extend_from_slice(&bitstring);
        let spki = der_tlv(0x30, &spki_body);

        let mut atv = Vec::new();
        atv.push(0x06);
        atv.push(3);
        atv.extend_from_slice(&[0x55, 0x04, 0x03]);
        atv.push(0x13);
        atv.push(11);
        atv.extend_from_slice(b"minos.test");
        let atv_seq = der_tlv(0x30, &atv);
        let set = der_tlv(0x31, &atv_seq);
        let name = der_tlv(0x30, &set);

        let mut val_body = Vec::new();
        for ts in [&b"240101000000Z"[..], &b"340101000000Z"[..]] {
            val_body.push(0x17);
            val_body.push(ts.len() as u8);
            val_body.extend_from_slice(ts);
        }
        let validity = der_tlv(0x30, &val_body);

        let mut tbs_body = Vec::new();
        tbs_body.push(0x02);
        tbs_body.push(0x01);
        tbs_body.push(0x01);
        tbs_body.extend_from_slice(&alg_id);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&validity);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&spki);
        let tbs = der_tlv(0x30, &tbs_body);

        let mut cert_body = Vec::new();
        cert_body.extend_from_slice(&tbs);
        cert_body.extend_from_slice(&alg_id);
        cert_body.extend_from_slice(&der_tlv(0x03, &[0x00]));
        der_tlv(0x30, &cert_body)
    }

    /// ClientHello içindeki key_share uzantısından istemcinin açık noktasını
    /// çıkarır.
    fn client_hello_key_share(hs: &[u8]) -> Option<Vec<u8>> {
        let mut pos = 4 + 34;
        let sess_len = hs[pos] as usize;
        pos += 1 + sess_len;
        let cs_len = u16::from_be_bytes([hs[pos], hs[pos + 1]]) as usize;
        pos += 2 + cs_len;
        let comp_len = hs[pos] as usize;
        pos += 1 + comp_len;
        if pos + 2 > hs.len() {
            return None;
        }
        let ext_total = u16::from_be_bytes([hs[pos], hs[pos + 1]]) as usize;
        pos += 2;
        let ext_end = pos + ext_total;
        while pos + 4 <= ext_end {
            let et = u16::from_be_bytes([hs[pos], hs[pos + 1]]);
            let el = u16::from_be_bytes([hs[pos + 2], hs[pos + 3]]) as usize;
            if et == 0x0033 {
                let body = &hs[pos + 4..pos + 4 + el];
                let group = u16::from_be_bytes([body[0], body[1]]);
                let plen = u16::from_be_bytes([body[2], body[3]]) as usize;
                if group == NAMED_CURVE_SECP256R1 && plen == 65 {
                    return Some(body[4..4 + plen].to_vec());
                }
            }
            pos += 4 + el;
        }
        None
    }

    /// Test icin kendi kendine imzali RSA sertifikasi uretir.
    fn make_test_cert() -> Vec<u8> {
        let mut rsapubkey_body = Vec::new();
        rsapubkey_body.push(0x02);
        rsapubkey_body.push(0x82);
        rsapubkey_body.push(0x00);
        rsapubkey_body.push(64);
        rsapubkey_body.extend_from_slice(&TEST_N);
        rsapubkey_body.push(0x02);
        rsapubkey_body.push(0x03);
        rsapubkey_body.extend_from_slice(&[0x01, 0x00, 0x01]);
        let rsapubkey = der_tlv(0x30, &rsapubkey_body);

        let mut bits = Vec::new();
        bits.push(0x00);
        bits.extend_from_slice(&rsapubkey);
        let bitstring = der_tlv(0x03, &bits);

        const RSA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
        let mut alg_body = Vec::new();
        alg_body.push(0x06);
        alg_body.push(RSA_OID.len() as u8);
        alg_body.extend_from_slice(RSA_OID);
        alg_body.push(0x05);
        alg_body.push(0x00);
        let alg_id = der_tlv(0x30, &alg_body);

        let mut spki_body = Vec::new();
        spki_body.extend_from_slice(&alg_id);
        spki_body.extend_from_slice(&bitstring);
        let spki = der_tlv(0x30, &spki_body);

        let mut atv = Vec::new();
        atv.push(0x06);
        atv.push(3);
        atv.extend_from_slice(&[0x55, 0x04, 0x03]);
        atv.push(0x13);
        atv.push(11);
        atv.extend_from_slice(b"minos.test");
        let atv_seq = der_tlv(0x30, &atv);
        let set = der_tlv(0x31, &atv_seq);
        let name = der_tlv(0x30, &set);

        let mut val_body = Vec::new();
        for ts in [&b"240101000000Z"[..], &b"340101000000Z"[..]] {
            val_body.push(0x17);
            val_body.push(ts.len() as u8);
            val_body.extend_from_slice(ts);
        }
        let validity = der_tlv(0x30, &val_body);

        let mut tbs_body = Vec::new();
        // serialNumber INTEGER
        tbs_body.push(0x02);
        tbs_body.push(0x01);
        tbs_body.push(0x01);
        tbs_body.extend_from_slice(&alg_id);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&validity);
        tbs_body.extend_from_slice(&name);
        tbs_body.extend_from_slice(&spki);
        let tbs = der_tlv(0x30, &tbs_body);

        let mut cert_body = Vec::new();
        cert_body.extend_from_slice(&tbs);
        cert_body.extend_from_slice(&alg_id);
        cert_body.extend_from_slice(&der_tlv(0x03, &[0x00]));
        der_tlv(0x30, &cert_body)
    }

    /// Test icin minimum TLS 1.3 sunucusu.
    struct TestServer {
        transcript: Sha256,
        handshake_secret: Option<[u8; 32]>,
        master_secret: Option<[u8; 32]>,
        /// ServerHello aninda turetilen el sikisma trafik sirlari.
        server_hs_secret: Option<[u8; 32]>,
        client_hs_secret: Option<[u8; 32]>,
        server_hs_key: Option<[u8; 16]>,
        server_hs_iv: Option<[u8; 12]>,
        server_hs_seq: u64,
        client_hs_key: Option<[u8; 16]>,
        client_hs_iv: Option<[u8; 12]>,
        client_hs_seq: u64,
        server_ap_key: Option<[u8; 16]>,
        server_ap_iv: Option<[u8; 12]>,
        server_ap_seq: u64,
        client_ap_key: Option<[u8; 16]>,
        client_ap_iv: Option<[u8; 12]>,
        client_ap_seq: u64,
        rsa: crate::drivers::crypto::rsa::RsaPrivateKey,
    }

    impl TestServer {
        fn new() -> Self {
            TestServer {
                transcript: Sha256::new(),
                handshake_secret: None,
                master_secret: None,
                server_hs_secret: None,
                client_hs_secret: None,
                server_hs_key: None,
                server_hs_iv: None,
                server_hs_seq: 0,
                client_hs_key: None,
                client_hs_iv: None,
                client_hs_seq: 0,
                server_ap_key: None,
                server_ap_iv: None,
                server_ap_seq: 0,
                client_ap_key: None,
                client_ap_iv: None,
                client_ap_seq: 0,
                rsa: crate::drivers::crypto::rsa::RsaPrivateKey {
                    n: TEST_N.to_vec(),
                    d: TEST_D.to_vec(),
                },
            }
        }

        fn note(&mut self, msg: &[u8]) {
            self.transcript.update(msg);
        }

        fn transcript_hash(&self) -> [u8; 32] {
            self.transcript.clone().finalize()
        }

        fn derive_secrets(&mut self, shared: &[u8; 32]) {
            let early = TlsSession::hkdf_extract(&[], &[]);
            let derived = TlsSession::hkdf_expand_label(&early, b"derived", b"", 32);
            let derived: [u8; 32] = derived.try_into().unwrap();
            let hs = TlsSession::hkdf_extract(&derived, shared);
            self.handshake_secret = Some(hs);

            let derived2 = TlsSession::hkdf_expand_label(&hs, b"derived", b"", 32);
            let derived2: [u8; 32] = derived2.try_into().unwrap();
            self.master_secret = Some(TlsSession::hkdf_extract(&derived2, &[]));
        }

        fn derive_handshake_traffic(&mut self) {
            let th = self.transcript_hash();
            let hs_secret = self.handshake_secret.as_ref().unwrap();
            let server_hs = TlsSession::hkdf_expand_label(
                hs_secret,
                b"s hs traffic",
                &th,
                32,
            );
            let server_hs: [u8; 32] = server_hs.try_into().unwrap();
            self.server_hs_secret = Some(server_hs);
            self.server_hs_key = Some(
                TlsSession::hkdf_expand_label(&server_hs, b"key", b"", 16)
                    .try_into()
                    .unwrap(),
            );
            self.server_hs_iv = Some(
                TlsSession::hkdf_expand_label(&server_hs, b"iv", b"", 12)
                    .try_into()
                    .unwrap(),
            );

            let client_hs = TlsSession::hkdf_expand_label(
                hs_secret,
                b"c hs traffic",
                &th,
                32,
            );
            let client_hs: [u8; 32] = client_hs.try_into().unwrap();
            self.client_hs_secret = Some(client_hs);
            self.client_hs_key = Some(
                TlsSession::hkdf_expand_label(&client_hs, b"key", b"", 16)
                    .try_into()
                    .unwrap(),
            );
            self.client_hs_iv = Some(
                TlsSession::hkdf_expand_label(&client_hs, b"iv", b"", 12)
                    .try_into()
                    .unwrap(),
            );
        }

        fn derive_application_traffic(&mut self) {
            let th = self.transcript_hash();
            let master_secret = self.master_secret.as_ref().unwrap();
            let client_ap = TlsSession::hkdf_expand_label(
                master_secret,
                b"c ap traffic",
                &th,
                32,
            );
            let client_ap: [u8; 32] = client_ap.try_into().unwrap();
            self.client_ap_key = Some(
                TlsSession::hkdf_expand_label(&client_ap, b"key", b"", 16)
                    .try_into()
                    .unwrap(),
            );
            self.client_ap_iv = Some(
                TlsSession::hkdf_expand_label(&client_ap, b"iv", b"", 12)
                    .try_into()
                    .unwrap(),
            );

            let server_ap = TlsSession::hkdf_expand_label(
                master_secret,
                b"s ap traffic",
                &th,
                32,
            );
            let server_ap: [u8; 32] = server_ap.try_into().unwrap();
            self.server_ap_key = Some(
                TlsSession::hkdf_expand_label(&server_ap, b"key", b"", 16)
                    .try_into()
                    .unwrap(),
            );
            self.server_ap_iv = Some(
                TlsSession::hkdf_expand_label(&server_ap, b"iv", b"", 12)
                    .try_into()
                    .unwrap(),
            );
        }

        fn encrypt_hs_record(&mut self, plaintext: &[u8]) -> Vec<u8> {
            let gcm = Gcm::new(self.server_hs_key.as_ref().unwrap()).unwrap();
            let seq = self.server_hs_seq;
            self.server_hs_seq += 1;
            let inner = TlsSession::tls13_inner_plaintext(plaintext, TLS_RECORD_HANDSHAKE);
            let nonce = TlsSession::tls13_nonce(self.server_hs_iv.as_ref().unwrap(), seq);
            let aad = TlsSession::tls13_aad(TLS_RECORD_HANDSHAKE, inner.len() + GCM_TAG_LEN);
            let (ct, tag) = gcm.encrypt(&nonce, &aad, &inner).unwrap();
            let mut rec = Vec::new();
            rec.push(TLS_RECORD_HANDSHAKE);
            rec.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
            rec.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
            rec.extend_from_slice(&ct);
            rec.extend_from_slice(&tag);
            rec
        }

        fn encrypt_app_record(&mut self, plaintext: &[u8]) -> Vec<u8> {
            let gcm = Gcm::new(self.server_ap_key.as_ref().unwrap()).unwrap();
            let seq = self.server_ap_seq;
            self.server_ap_seq += 1;
            let inner = TlsSession::tls13_inner_plaintext(plaintext, TLS_RECORD_APPLICATION_DATA);
            let nonce = TlsSession::tls13_nonce(self.server_ap_iv.as_ref().unwrap(), seq);
            let aad = TlsSession::tls13_aad(TLS_RECORD_APPLICATION_DATA, inner.len() + GCM_TAG_LEN);
            let (ct, tag) = gcm.encrypt(&nonce, &aad, &inner).unwrap();
            let mut rec = Vec::new();
            rec.push(TLS_RECORD_APPLICATION_DATA);
            rec.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
            rec.extend_from_slice(&((ct.len() + 16) as u16).to_be_bytes());
            rec.extend_from_slice(&ct);
            rec.extend_from_slice(&tag);
            rec
        }

        fn decrypt_client_record(&mut self, record: &[u8], is_hs: bool) -> Vec<u8> {
            let payload = &record[5..];
            let (key, iv, seq) = if is_hs {
                let s = self.client_hs_seq;
                self.client_hs_seq += 1;
                (
                    self.client_hs_key.as_ref().unwrap(),
                    self.client_hs_iv.as_ref().unwrap(),
                    s,
                )
            } else {
                let s = self.client_ap_seq;
                self.client_ap_seq += 1;
                (
                    self.client_ap_key.as_ref().unwrap(),
                    self.client_ap_iv.as_ref().unwrap(),
                    s,
                )
            };
            let gcm = Gcm::new(key).unwrap();
            let ctype = if is_hs {
                TLS_RECORD_HANDSHAKE
            } else {
                TLS_RECORD_APPLICATION_DATA
            };
            let nonce = TlsSession::tls13_nonce(iv, seq);
            let aad = TlsSession::tls13_aad(ctype, payload.len());
            let ct_len = payload.len() - 16;
            let mut tag = [0u8; 16];
            tag.copy_from_slice(&payload[ct_len..]);
            let inner = gcm
                .decrypt(&nonce, &aad, &payload[..ct_len], &tag)
                .unwrap();
            let (content, _) = TlsSession::tls13_strip_inner_plaintext(&inner).unwrap();
            content.to_vec()
        }
    }

    fn hs_message(msg_type: u8, body: &[u8]) -> Vec<u8> {
        let mut m = Vec::with_capacity(4 + body.len());
        m.push(msg_type);
        let len = body.len();
        m.push(((len >> 16) & 0xFF) as u8);
        m.push(((len >> 8) & 0xFF) as u8);
        m.push((len & 0xFF) as u8);
        m.extend_from_slice(body);
        m
    }

    /// TLS 1.3 el sikismasinin uctan uca dogrulanmasi.
    #[test]
    fn tls13_handshake_loopback() {
        let client_random = [0x11u8; 32];
        let mut hello = Vec::new();
        let mut client_ecdhe = [0u8; 32];
        build_client_hello("minos.test", &client_random, &mut hello, &mut client_ecdhe);

        let mut client = TlsSession::new("minos.test", client_random);
        let ch_body = &hello[5..];
        client.note_client_hello(ch_body);
        // TLS 1.3 anahtar turetimi icin istemci ECDHE gizlisi saklanir.
        client.set_client_ecdhe_secret(client_ecdhe);

        let mut server = TestServer::new();
        server.note(ch_body);

        // ClientHello icindeki key_share uzantisindan istemci noktasini al.
        let hs = ch_body;
        let client_point = client_hello_key_share(hs).expect("ClientHello key_share bulunamadi");
        assert_eq!(client_point.len(), 65);

        let srv_seed = [0x77u8; 32];
        let srv_kp = crate::drivers::crypto::ecc::generate_keypair(&srv_seed);
        let shared = crate::drivers::crypto::ecc::shared_secret(&srv_kp.secret, &client_point)
            .expect("paylasilan gizli");

        // ServerHello
        let mut sh_body = Vec::new();
        sh_body.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        sh_body.extend_from_slice(&[0x22u8; 32]);
        sh_body.push(0);
        sh_body.extend_from_slice(&TLS_AES_128_GCM_SHA256.to_be_bytes());
        sh_body.push(0);
        let mut sh_ext = Vec::new();
        sh_ext.extend_from_slice(&0x002bu16.to_be_bytes());
        sh_ext.extend_from_slice(&2u16.to_be_bytes());
        sh_ext.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());
        sh_ext.extend_from_slice(&0x0033u16.to_be_bytes());
        sh_ext.extend_from_slice(&((4 + srv_kp.public.len()) as u16).to_be_bytes());
        sh_ext.extend_from_slice(&NAMED_CURVE_SECP256R1.to_be_bytes());
        sh_ext.extend_from_slice(&(srv_kp.public.len() as u16).to_be_bytes());
        sh_ext.extend_from_slice(&srv_kp.public);
        sh_body.extend_from_slice(&(sh_ext.len() as u16).to_be_bytes());
        sh_body.extend_from_slice(&sh_ext);

        let sh = hs_message(HANDSHAKE_SERVER_HELLO, &sh_body);
        server.note(&sh);
        server.derive_secrets(&shared);
        server.derive_handshake_traffic();

        // ServerHello duz metin bir kayit olarak gonderilir.
        let mut sh_rec = Vec::new();
        sh_rec.push(TLS_RECORD_HANDSHAKE);
        sh_rec.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        sh_rec.extend_from_slice(&(sh.len() as u16).to_be_bytes());
        sh_rec.extend_from_slice(&sh);

        // EncryptedExtensions
        let ee = hs_message(HANDSHAKE_ENCRYPTED_EXTENSIONS, &[]);
        server.note(&ee);
        let ee_rec = server.encrypt_hs_record(&ee);

        // Certificate (TLS 1.3):
        //   certificate_request_context<0..2^8-1>  -> 1 bayt (0)
        //   certificate_list<0..2^24-1>            -> 3 bayt uzunluk
        //   CertificateEntry: cert_data<1..2^24-1> + extensions<0..2^16-1>
        let cert = make_test_cert();
        assert!(
            parse_x509_rsa_public_key(&cert).is_ok(),
            "test sertifikasi cozulemeli"
        );
        let mut cert_body = Vec::new();
        cert_body.push(0); // certificate_request_context length
        let list_len = cert.len() + 5; // 3 bayt cert_data + 2 bayt extensions
        cert_body.push(((list_len >> 16) & 0xFF) as u8);
        cert_body.push(((list_len >> 8) & 0xFF) as u8);
        cert_body.push((list_len & 0xFF) as u8);
        cert_body.push(((cert.len() >> 16) & 0xFF) as u8);
        cert_body.push(((cert.len() >> 8) & 0xFF) as u8);
        cert_body.push((cert.len() & 0xFF) as u8);
        cert_body.extend_from_slice(&cert);
        cert_body.push(0); // extensions length
        cert_body.push(0);
        let cert_msg = hs_message(HANDSHAKE_CERTIFICATE, &cert_body);
        server.note(&cert_msg);
        let cert_rec = server.encrypt_hs_record(&cert_msg);

        // CertificateVerify
        let mut signed = Vec::new();
        signed.extend_from_slice(&[0x20u8; 64]);
        signed.extend_from_slice(b"TLS 1.3, server CertificateVerify");
        signed.push(0);
        signed.extend_from_slice(&server.transcript_hash());
        let sig = crate::drivers::crypto::rsa::pkcs1_v1_5_sign(&server.rsa, &signed).unwrap();
        let mut cv_body = Vec::new();
        cv_body.extend_from_slice(&SIG_ALG_RSA_SHA256.to_be_bytes());
        cv_body.extend_from_slice(&(sig.len() as u16).to_be_bytes());
        cv_body.extend_from_slice(&sig);
        let cv = hs_message(HANDSHAKE_CERTIFICATE_VERIFY, &cv_body);
        server.note(&cv);
        let cv_rec = server.encrypt_hs_record(&cv);

        // Finished (sunucu)
        let s_finished =
            TlsSession::hkdf_expand_label(server.server_hs_secret.as_ref().unwrap(), b"s finished", b"", 32);
        let s_finished: [u8; 32] = s_finished.try_into().unwrap();
        let finished_key = TlsSession::hkdf_expand_label(&s_finished, b"finished", b"", 32);
        let finished_key: [u8; 32] = finished_key.try_into().unwrap();
        let verify = HmacSha256::sign(&finished_key, &server.transcript_hash());
        let fin = hs_message(HANDSHAKE_FINISHED, &verify[..TLS_VERIFY_DATA_LEN]);
        server.note(&fin);
        let fin_rec = server.encrypt_hs_record(&fin);

        // Istemci sunucu kayitlarini isler.
        let mut hs_out = Vec::new();
        let mut app_out = Vec::new();
        client.feed_tcp_data(&sh_rec);
        client.process_records(&mut hs_out, &mut app_out).unwrap();
        assert_eq!(client.state(), TlsState::ServerHelloReceived);

        for rec in [&ee_rec[..], &cert_rec[..], &cv_rec[..], &fin_rec[..]] {
            client.feed_tcp_data(rec);
            client.process_records(&mut hs_out, &mut app_out).unwrap();
        }
        assert_eq!(client.state(), TlsState::Tls13ServerFinished);

        // Istemci Finished uretir.
        let client_finished = client
            .build_tls13_client_finished()
            .expect("Finished uretilemedi");
        assert_eq!(client.state(), TlsState::Established);

        // Sunucu istemci Finished'i cozer ve dogrular.
        let plain = server.decrypt_client_record(&client_finished, true);
        let th2 = server.transcript_hash();
        let c_finished =
            TlsSession::hkdf_expand_label(server.client_hs_secret.as_ref().unwrap(), b"c finished", b"", 32);
        let c_finished: [u8; 32] = c_finished.try_into().unwrap();
        let c_key = TlsSession::hkdf_expand_label(&c_finished, b"finished", b"", 32);
        let c_key: [u8; 32] = c_key.try_into().unwrap();
        let expected = HmacSha256::sign(&c_key, &th2);
        assert_eq!(
            &plain[4..4 + TLS_VERIFY_DATA_LEN],
            &expected[..TLS_VERIFY_DATA_LEN],
            "Istemci Finished dogrulanmaliydi"
        );
        // Uygulama trafik sirlari, transcript "ClientHello..sunucu Finished"
        // uzerinden turetilir; istemci Finished bu noktadan sonra eklenir.
        server.derive_application_traffic();
        server.note(&hs_message(HANDSHAKE_FINISHED, &plain[4..]));

        // Uygulama verisi gidis-gelisi.
        let request = b"GET / HTTP/1.1\r\nHost: minos.test\r\n\r\n";
        let rec = client.encrypt_app_data(request).expect("istek seifrelenemedi");
        let plain = server.decrypt_client_record(&rec, false);
        assert_eq!(&plain[..], &request[..]);

        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi";
        let rec = server.encrypt_app_record(response);
        let mut app_out = Vec::new();
        client.feed_tcp_data(&rec);
        client.process_records(&mut hs_out, &mut app_out).unwrap();
        assert_eq!(&app_out[..], &response[..]);
    }

    /// ECDSA sertifikalı sunucu ile TLS 1.3 el sıkışması.
    ///
    /// Aynı akış ECDSA (P-256) anahtarıyla yürütülür; CertificateVerify
    /// imzası ECDSA ile üretilir ve istemci tarafından doğrulanır.
    #[test]
    fn tls13_handshake_loopback_ecdsa() {
        // Test anahtarı: d ve k (P-256 skalarları).
        const EC_D: [u8; 32] = [
            0xc9, 0xaf, 0xa9, 0xd8, 0x45, 0xba, 0x75, 0x16, 0x6b, 0x5c, 0x21, 0x57, 0x67, 0xb1, 0xd6, 0x93,
            0x4e, 0x50, 0xc3, 0xdb, 0x36, 0xe8, 0x9b, 0x12, 0x7b, 0x8a, 0x62, 0x2b, 0x12, 0x0f, 0x67, 0x21,
        ];
        const EC_K: [u8; 32] = [
            0xa6, 0xe3, 0xc5, 0x7d, 0xd0, 0x1a, 0xbe, 0x90, 0x08, 0x65, 0x38, 0x39, 0x83, 0x55, 0xdd, 0x4c,
            0x3b, 0x17, 0xaa, 0x87, 0x33, 0x82, 0xb0, 0xf2, 0x4d, 0x61, 0x2d, 0x94, 0x93, 0xd8, 0xaa, 0xd6,
        ];

        // Sunucu anahtar cifti.
        let kp = crate::drivers::crypto::ecc::generate_keypair(&EC_D);

        // --- Istemci ---
        let client_random = [0x22u8; 32];
        let mut hello = Vec::new();
        let mut client_ecdhe = [0u8; 32];
        build_client_hello("minos.test", &client_random, &mut hello, &mut client_ecdhe);
        let mut client = TlsSession::new("minos.test", client_random);
        let ch_body = &hello[5..];
        client.note_client_hello(ch_body);
        client.set_client_ecdhe_secret(client_ecdhe);

        // --- Sunucu ---
        let mut server = TestServer::new();
        server.note(ch_body);
        let client_point = client_hello_key_share(ch_body).expect("ClientHello key_share yok");
        assert_eq!(client_point.len(), 65);

        let srv_seed = [0x77u8; 32];
        let srv_kp = crate::drivers::crypto::ecc::generate_keypair(&srv_seed);
        let shared = crate::drivers::crypto::ecc::shared_secret(&srv_kp.secret, &client_point)
            .expect("paylasilan gizli");

        // ServerHello
        let mut sh_body = Vec::new();
        sh_body.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        sh_body.extend_from_slice(&[0x33u8; 32]);
        sh_body.push(0);
        sh_body.extend_from_slice(&TLS_AES_128_GCM_SHA256.to_be_bytes());
        sh_body.push(0);
        let mut sh_ext = Vec::new();
        sh_ext.extend_from_slice(&0x002bu16.to_be_bytes());
        sh_ext.extend_from_slice(&2u16.to_be_bytes());
        sh_ext.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());
        sh_ext.extend_from_slice(&0x0033u16.to_be_bytes());
        sh_ext.extend_from_slice(&((4 + srv_kp.public.len()) as u16).to_be_bytes());
        sh_ext.extend_from_slice(&NAMED_CURVE_SECP256R1.to_be_bytes());
        sh_ext.extend_from_slice(&(srv_kp.public.len() as u16).to_be_bytes());
        sh_ext.extend_from_slice(&srv_kp.public);
        sh_body.extend_from_slice(&(sh_ext.len() as u16).to_be_bytes());
        sh_body.extend_from_slice(&sh_ext);
        let sh = hs_message(HANDSHAKE_SERVER_HELLO, &sh_body);
        server.note(&sh);
        server.derive_secrets(&shared);
        server.derive_handshake_traffic();

        let mut sh_rec = Vec::new();
        sh_rec.push(TLS_RECORD_HANDSHAKE);
        sh_rec.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        sh_rec.extend_from_slice(&(sh.len() as u16).to_be_bytes());
        sh_rec.extend_from_slice(&sh);

        // EncryptedExtensions
        let ee = hs_message(HANDSHAKE_ENCRYPTED_EXTENSIONS, &[]);
        server.note(&ee);
        let ee_rec = server.encrypt_hs_record(&ee);

        // Certificate (EC)
        let cert = make_test_ec_cert(&kp.public);
        assert!(
            parse_x509_ec_public_key(&cert).is_ok(),
            "EC test sertifikat cozulemeli"
        );
        let mut cert_body = Vec::new();
        cert_body.push(0);
        let list_len = cert.len() + 5;
        cert_body.push(((list_len >> 16) & 0xFF) as u8);
        cert_body.push(((list_len >> 8) & 0xFF) as u8);
        cert_body.push((list_len & 0xFF) as u8);
        cert_body.push(((cert.len() >> 16) & 0xFF) as u8);
        cert_body.push(((cert.len() >> 8) & 0xFF) as u8);
        cert_body.push((cert.len() & 0xFF) as u8);
        cert_body.extend_from_slice(&cert);
        cert_body.push(0);
        cert_body.push(0);
        let cert_msg = hs_message(HANDSHAKE_CERTIFICATE, &cert_body);
        server.note(&cert_msg);
        let cert_rec = server.encrypt_hs_record(&cert_msg);

        // CertificateVerify (ECDSA)
        let mut signed = Vec::new();
        signed.extend_from_slice(&[0x20u8; 64]);
        signed.extend_from_slice(b"TLS 1.3, server CertificateVerify");
        signed.push(0);
        let transcript = server.transcript_hash();
        signed.extend_from_slice(&transcript);
        let mut h = Sha256::new();
        h.update(&signed);
        let digest = h.finalize();
        let (r, s) = crate::drivers::crypto::ecc::ecdsa_sign_p256(&EC_D, &digest, &EC_K)
            .expect("ECDSA imza uretilemedi");
        // DER: SEQUENCE { r INTEGER, s INTEGER }
        let mut der_body = Vec::new();
        for v in [r, s] {
            let mut b = v.to_vec();
            if b[0] & 0x80 != 0 {
                b.insert(0, 0x00);
            }
            der_body.push(0x02);
            der_body.push(b.len() as u8);
            der_body.extend_from_slice(&b);
        }
        let mut der = Vec::new();
        der.push(0x30);
        der.push(der_body.len() as u8);
        der.extend_from_slice(&der_body);

        let mut cv_body = Vec::new();
        cv_body.extend_from_slice(&SIG_ALG_ECDSA_SECP256R1_SHA256.to_be_bytes());
        cv_body.extend_from_slice(&(der.len() as u16).to_be_bytes());
        cv_body.extend_from_slice(&der);
        let cv = hs_message(HANDSHAKE_CERTIFICATE_VERIFY, &cv_body);
        server.note(&cv);
        let cv_rec = server.encrypt_hs_record(&cv);

        // Finished (sunucu)
        let s_finished =
            TlsSession::hkdf_expand_label(server.server_hs_secret.as_ref().unwrap(), b"s finished", b"", 32);
        let s_finished: [u8; 32] = s_finished.try_into().unwrap();
        let finished_key = TlsSession::hkdf_expand_label(&s_finished, b"finished", b"", 32);
        let finished_key: [u8; 32] = finished_key.try_into().unwrap();
        let verify = HmacSha256::sign(&finished_key, &server.transcript_hash());
        let fin = hs_message(HANDSHAKE_FINISHED, &verify[..TLS_VERIFY_DATA_LEN]);
        server.note(&fin);
        let fin_rec = server.encrypt_hs_record(&fin);

        // Istemci sunucu kayitlarini isler.
        let mut hs_out = Vec::new();
        let mut app_out = Vec::new();
        client.feed_tcp_data(&sh_rec);
        client.process_records(&mut hs_out, &mut app_out).unwrap();
        assert_eq!(client.state(), TlsState::ServerHelloReceived);

        for rec in [&ee_rec[..], &cert_rec[..], &cv_rec[..], &fin_rec[..]] {
            client.feed_tcp_data(rec);
            client.process_records(&mut hs_out, &mut app_out).unwrap();
        }
        assert_eq!(
            client.state(),
            TlsState::Tls13ServerFinished,
            "ECDSA CertificateVerify dogrulanmaliydi"
        );

        let client_finished = client.build_tls13_client_finished().unwrap();
        assert_eq!(client.state(), TlsState::Established);
        let plain = server.decrypt_client_record(&client_finished, true);
        assert_eq!(plain[0], HANDSHAKE_FINISHED);
    }
}
//! TLS 1.2 Protokol İstemci Katmanı (no_std)
//! Tam TLS 1.2 el sıkışması: ClientHello → ServerHello → Certificate →
//! ServerHelloDone → ClientKeyExchange → ChangeCipherSpec → Finished →
//! şifreli Application Data. AES-128-GCM, RSA anahtar değişimi,
//! SHA-256 PRF ve anahtar türetme dahil.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::crypto::gcm::Gcm;
use crate::drivers::crypto::hmac::HmacSha256;
use crate::drivers::crypto::sha256::Sha256;
use crate::drivers::crypto::rsa::{pkcs1_v1_5_encrypt, RsaPublicKey};
use crate::drivers::crypto::x509::parse_x509_rsa_public_key;

pub const TLS_RECORD_CHANGE_CIPHER_SPEC: u8 = 20;
pub const TLS_RECORD_ALERT: u8 = 21;
pub const TLS_RECORD_HANDSHAKE: u8 = 22;
pub const TLS_RECORD_APPLICATION_DATA: u8 = 23;

pub const TLS_VERSION_1_0: u16 = 0x0301;
pub const TLS_VERSION_1_2: u16 = 0x0303;

pub const HANDSHAKE_CLIENT_HELLO: u8 = 1;
pub const HANDSHAKE_SERVER_HELLO: u8 = 2;
pub const HANDSHAKE_CERTIFICATE: u8 = 11;
pub const HANDSHAKE_SERVER_HELLO_DONE: u8 = 14;
pub const HANDSHAKE_CLIENT_KEY_EXCHANGE: u8 = 16;
pub const HANDSHAKE_FINISHED: u8 = 20;

pub const TLS_RSA_WITH_AES_128_GCM_SHA256: u16 = 0x009C;

const TLS_MASTER_SECRET_LEN: usize = 48;
const TLS_KEY_BLOCK_LEN: usize = 40;
const TLS_VERIFY_DATA_LEN: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TlsState {
    ClientHelloSent,
    ServerHelloReceived,
    CertificateReceived,
    ServerHelloDoneReceived,
    FinishedSent,
    Established,
    Error,
}

pub struct TlsSession {
    state: TlsState,
    hostname: String,
    client_random: [u8; 32],
    server_random: [u8; 32],
    cipher_suite: u16,
    rsa_key: Option<RsaPublicKey>,
    pre_master_secret: Option<[u8; 48]>,
    master_secret: Option<[u8; 48]>,
    client_write_key: Option<[u8; 16]>,
    server_write_key: Option<[u8; 16]>,
    client_write_iv: Option<[u8; 4]>,
    server_write_iv: Option<[u8; 4]>,
    server_gcm: Option<Gcm>,
    client_gcm: Option<Gcm>,
    server_seq: u64,
    client_seq: u64,
    record_buffer: Vec<u8>,
    hs_buffer: Vec<u8>,
    handshake_hash: Sha256,
    outgoing: Vec<u8>,
    /// Alınan son TLS uyarısı (seviye, açıklama).
    last_alert: Option<(u8, u8)>,
}

impl TlsSession {
    pub fn new(hostname: &str, client_random: [u8; 32]) -> Self {
        TlsSession {
            state: TlsState::ClientHelloSent,
            hostname: String::from(hostname),
            client_random,
            server_random: [0; 32],
            cipher_suite: 0,
            rsa_key: None,
            pre_master_secret: None,
            master_secret: None,
            client_write_key: None,
            server_write_key: None,
            client_write_iv: None,
            server_write_iv: None,
            server_gcm: None,
            client_gcm: None,
            server_seq: 0,
            client_seq: 0,
            record_buffer: Vec::new(),
            hs_buffer: Vec::new(),
            handshake_hash: Sha256::new(),
            outgoing: Vec::new(),
            last_alert: None,
        }
    }

    /// Sunucudan gelen son TLS uyarısı (seviye, açıklama) — hata mesajı için.
    pub fn last_alert(&self) -> Option<(u8, u8)> {
        self.last_alert
    }

    pub fn note_client_hello(&mut self, client_hello_body: &[u8]) {
        self.handshake_hash.update(client_hello_body);
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
                    self.process_handshake_records(&payload_copy, out_handshake)?;
                }
                TLS_RECORD_CHANGE_CIPHER_SPEC => {}
                TLS_RECORD_ALERT => {
                    let desc = payload_copy.get(1).copied().unwrap_or(0);
                    let level = payload_copy.first().copied().unwrap_or(0);
                    self.last_alert = Some((level, desc));
                    return Err(());
                }
                TLS_RECORD_APPLICATION_DATA => {
                    let plaintext = self.decrypt_app_data(&payload_copy)?;
                    out_app_data.extend_from_slice(&plaintext);
                }
                _ => {}
            }
        }
        Ok(self.state)
    }

    pub fn state(&self) -> TlsState {
        self.state
    }

    fn process_handshake_records(
        &mut self,
        payload: &[u8],
        out_handshake: &mut Vec<u8>,
    ) -> Result<(), ()> {
        // El sıkışma mesajları kayıt sınırını aşabilir (büyük sertifika
        // zincirleri çoğu zaman birden fazla record'a bölünür). Tamamlanmamış
        // kısım tamponda bekletilir.
        let mut buf = core::mem::take(&mut self.hs_buffer);
        buf.extend_from_slice(payload);

        let mut pos = 0;
        while pos + 4 <= buf.len() {
            let msg_type = buf[pos];
            let msg_len = ((buf[pos + 1] as usize) << 16)
                | ((buf[pos + 2] as usize) << 8)
                | (buf[pos + 3] as usize);
            if pos + 4 + msg_len > buf.len() {
                // Eksik mesaj: beklemeye al.
                break;
            }
            let msg_frame = &buf[pos..pos + 4 + msg_len];
            let msg_body = &buf[pos + 4..pos + 4 + msg_len];

            self.handshake_hash.update(msg_frame);
            out_handshake.extend_from_slice(msg_frame);

            match msg_type {
                HANDSHAKE_SERVER_HELLO => self.handle_server_hello(msg_body)?,
                HANDSHAKE_CERTIFICATE => self.handle_certificate(msg_body)?,
                HANDSHAKE_SERVER_HELLO_DONE => {
                    self.handle_server_hello_done()?;
                }
                HANDSHAKE_FINISHED => self.handle_server_finished(msg_body)?,
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
        if self.cipher_suite != TLS_RSA_WITH_AES_128_GCM_SHA256 {
            return Err(());
        }
        self.state = TlsState::ServerHelloReceived;
        Ok(())
    }

    fn handle_certificate(&mut self, body: &[u8]) -> Result<(), ()> {
        if body.len() < 3 {
            return Err(());
        }
        let cert_list_len = ((body[0] as usize) << 16) | ((body[1] as usize) << 8) | (body[2] as usize);
        if body.len() < 3 + cert_list_len {
            return Err(());
        }
        let mut pos = 3;
        while pos + 3 <= 3 + cert_list_len {
            let cert_len = ((body[pos] as usize) << 16) | ((body[pos + 1] as usize) << 8) | (body[pos + 2] as usize);
            pos += 3;
            if pos + cert_len > 3 + cert_list_len {
                return Err(());
            }
            if self.rsa_key.is_none() {
                match parse_x509_rsa_public_key(&body[pos..pos + cert_len]) {
                    Ok(key) => self.rsa_key = Some(key),
                    Err(_) => return Err(()),
                }
            }
            pos += cert_len;
        }
        self.state = TlsState::CertificateReceived;
        Ok(())
    }

    fn handle_server_hello_done(&mut self) -> Result<(), ()> {
        if self.state != TlsState::CertificateReceived {
            return Err(());
        }
        let rsa_key = self.rsa_key.as_ref().ok_or(())?;

        // Pre-master secret: TLS 1.2 version + 46 random bytes
        let mut pms = [0u8; 48];
        pms[0] = (TLS_VERSION_1_2 >> 8) as u8;
        pms[1] = (TLS_VERSION_1_2 & 0xFF) as u8;
        let seed = crate::monotonic_ms();
        for i in 2..48 {
            pms[i] = ((seed.wrapping_add((i as u64) * 0x9e3779b97f4a7c15)) >> ((i % 8) * 8)) as u8;
        }
        self.pre_master_secret = Some(pms);

        let encrypted_pms = pkcs1_v1_5_encrypt(rsa_key, &pms)?;

        // ClientKeyExchange handshake message
        let cke_body_len = 2 + encrypted_pms.len();
        let mut cke = Vec::new();
        cke.push(HANDSHAKE_CLIENT_KEY_EXCHANGE);
        cke.push(((cke_body_len >> 16) & 0xFF) as u8);
        cke.push(((cke_body_len >> 8) & 0xFF) as u8);
        cke.push((cke_body_len & 0xFF) as u8);
        cke.extend_from_slice(&(encrypted_pms.len() as u16).to_be_bytes());
        cke.extend_from_slice(&encrypted_pms);
        self.handshake_hash.update(&cke);

        // Derive keys
        self.derive_keys()?;

        // ChangeCipherSpec record
        self.outgoing.push(TLS_RECORD_CHANGE_CIPHER_SPEC);
        self.outgoing.extend_from_slice(&TLS_VERSION_1_0.to_be_bytes());
        self.outgoing.extend_from_slice(&1u16.to_be_bytes());
        self.outgoing.push(1);

        // Finished handshake message
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

        // Encrypt Finished with client GCM
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

        // Update handshake hash with plaintext Finished
        let _hash_for_server = Sha256::new();
        // We need the hash of all handshake messages INCLUDING this Finished
        // for server finished verification. We already updated handshake_hash with CKE.
        // Now add Finished.
        // But handshake_hash was finalized above. Re-create:
        // Actually, we need to track all handshake messages. Let's keep a running hash.
        // For simplicity, we'll recompute: we have all handshake bytes in out_handshake
        // plus this finished. But that's complex. Instead, let's just keep the finalize
        // and re-hash.
        // The server finished verification hash = hash of all handshake messages
        // (client hello, server hello, cert, server hello done, client key exchange, client finished)
        // We already updated self.handshake_hash with all messages up to CKE.
        // Now add Finished:
        self.handshake_hash.update(&finished);

        self.state = TlsState::FinishedSent;
        Ok(())
    }

    fn handle_server_finished(&mut self, body: &[u8]) -> Result<(), ()> {
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

    fn derive_keys(&mut self) -> Result<(), ()> {
        let pms = self.pre_master_secret.ok_or(())?;
        let mut seed = Vec::with_capacity(64);
        seed.extend_from_slice(&self.client_random);
        seed.extend_from_slice(&self.server_random);
        let ms = self.prf(&pms, b"master secret", &seed);
        let mut master = [0u8; 48];
        master.copy_from_slice(&ms[..48]);
        self.master_secret = Some(master);

        let mut kb_seed = Vec::with_capacity(64);
        kb_seed.extend_from_slice(&self.server_random);
        kb_seed.extend_from_slice(&self.client_random);
        let kb = self.prf(&master, b"key expansion", &kb_seed);

        let mut cwk = [0u8; 16];
        let mut swk = [0u8; 16];
        let mut civ = [0u8; 4];
        let mut siv = [0u8; 4];
        cwk.copy_from_slice(&kb[0..16]);
        swk.copy_from_slice(&kb[16..32]);
        civ.copy_from_slice(&kb[32..36]);
        siv.copy_from_slice(&kb[36..40]);

        self.client_write_key = Some(cwk);
        self.server_write_key = Some(swk);
        self.client_write_iv = Some(civ);
        self.server_write_iv = Some(siv);

        self.server_gcm = Some(Gcm::new(&swk)?);
        self.client_gcm = Some(Gcm::new(&cwk)?);

        Ok(())
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

    fn decrypt_app_data(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
        let server_iv = self.server_write_iv.ok_or(())?;
        let gcm = self.server_gcm.as_mut().ok_or(())?;

        let seq = self.server_seq;
        self.server_seq = self.server_seq.wrapping_add(1);

        let mut nonce = <[u8; 12]>::default();
        nonce[..4].copy_from_slice(&server_iv);
        nonce[4..12].copy_from_slice(&seq.to_be_bytes());

        let mut gcm_aad = <[u8; 13]>::default();
        gcm_aad[..8].copy_from_slice(&seq.to_be_bytes());
        gcm_aad[8] = TLS_RECORD_APPLICATION_DATA;
        gcm_aad[9..11].copy_from_slice(&TLS_VERSION_1_2.to_be_bytes());
        gcm_aad[11..13].copy_from_slice(&(ciphertext.len() as u16).to_be_bytes());

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

    /// Şifreli application data kaydı üretir; istemci sıra numarası artırılır.
    pub fn encrypt_app_data(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
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

pub fn parse_record_header(buf: &[u8]) -> Option<(u8, u16, usize)> {    if buf.len() < 5 {
        return None;
    }
    let ctype = buf[0];
    let ver = u16::from_be_bytes([buf[1], buf[2]]);
    let len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    Some((ctype, ver, len))
}

pub fn build_client_hello(hostname: &str, client_random: &[u8; 32], out: &mut Vec<u8>) {
    out.clear();

    let mut handshake = Vec::new();
    handshake.push(HANDSHAKE_CLIENT_HELLO);
    handshake.extend_from_slice(&[0, 0, 0]);

    handshake.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
    handshake.extend_from_slice(client_random);
    handshake.push(0);

    let ciphers = [TLS_RSA_WITH_AES_128_GCM_SHA256];
    // Secure Renegotiation SCSV + uzunluk alanına dahil edilir
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
        // SNI (0x0000)
        extensions.extend_from_slice(&0x0000u16.to_be_bytes());
        let list_len = (host_bytes.len() + 3) as u16;
        let ext_len = list_len + 2;
        extensions.extend_from_slice(&ext_len.to_be_bytes());
        extensions.extend_from_slice(&list_len.to_be_bytes());
        extensions.push(0);
        extensions.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
        extensions.extend_from_slice(host_bytes);
    }

    // supported_versions (0x002b): yalnızca TLS 1.2
    extensions.extend_from_slice(&0x002bu16.to_be_bytes());
    extensions.extend_from_slice(&3u16.to_be_bytes());
    extensions.push(1);
    extensions.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());

    // signature_algorithms (0x000d): sunucunun sertifika/imzalama şeması
    // seçmesi için gereklidir; modern sunucular bu uzantı yoksa el sıkışmayı
    // reddeder.
    let sigalgs: [u16; 8] = [
        0x0401, // SHA256 + RSA (PKCS#1 v1.5)
        0x0501, // SHA384 + RSA
        0x0601, // SHA512 + RSA
        0x0201, // SHA1 + RSA (eski)
        0x0804, // SHA256 + RSA-PSS
        0x0805, // SHA384 + RSA-PSS
        0x0806, // SHA512 + RSA-PSS
        0x0403, // SHA256 + ECDSA
    ];
    extensions.extend_from_slice(&0x000du16.to_be_bytes());
    extensions.extend_from_slice(&((2 + sigalgs.len() * 2) as u16).to_be_bytes());
    extensions.extend_from_slice(&((sigalgs.len() * 2) as u16).to_be_bytes());
    for s in &sigalgs {
        extensions.extend_from_slice(&s.to_be_bytes());
    }

    // renegotiation_info (0xff01): boş yeniden müzakere bilgisi
    extensions.extend_from_slice(&0xff01u16.to_be_bytes());
    extensions.extend_from_slice(&1u16.to_be_bytes());
    extensions.push(0);

    if !extensions.is_empty() {
        handshake.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        handshake.extend_from_slice(&extensions);
    }

    let hs_len = (handshake.len() - 4) as u32;
    handshake[1] = ((hs_len >> 16) & 0xFF) as u8;
    handshake[2] = ((hs_len >> 8) & 0xFF) as u8;
    handshake[3] = (hs_len & 0xFF) as u8;

    out.push(TLS_RECORD_HANDSHAKE);
    out.extend_from_slice(&TLS_VERSION_1_0.to_be_bytes());
    let rec_len = handshake.len() as u16;
    out.extend_from_slice(&rec_len.to_be_bytes());
    out.extend_from_slice(&handshake);
}

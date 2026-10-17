//! Minimal TLS 1.2 / 1.3 Protokol İstemci Katmanı (no_std)
//!
//! Modern web sunucularına bağlanmak için gereken TLS Record ve Handshake
//! çerçevelemesini (ClientHello, SNI - Server Name Indication, TLS Record Layer)
//! sağlar. `socket.rs` üzerindeki ham TCP akışını şifreli/güvenli oturum
//! katmanına hazırlar.
#![allow(dead_code)]

use alloc::vec::Vec;

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

// Standart Güvenli Şifreleme Süitleri (Cipher Suites)
pub const TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0xC02F;
pub const TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384: u16 = 0xC030;
pub const TLS_RSA_WITH_AES_128_GCM_SHA256: u16 = 0x009C;
pub const TLS_RSA_WITH_AES_256_GCM_SHA384: u16 = 0x009D;
pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301; // TLS 1.3
pub const TLS_AES_256_GCM_SHA384: u16 = 0x1302; // TLS 1.3

/// Standart bir TLS 1.2/1.3 ClientHello paketi üretir (SNI destekli).
pub fn build_client_hello(hostname: &str, out: &mut Vec<u8>) {
    out.clear();

    // Rastgele 32-byte istemci dizisi
    let mut client_random = [0u8; 32];
    let seed = crate::monotonic_ms();
    for (i, b) in client_random.iter_mut().enumerate() {
        *b = ((seed.wrapping_add((i as u64) * 0x9e3779b97f4a7c15)) >> ((i % 8) * 8)) as u8;
    }

    // Handshake gövdesi (ClientHello)
    let mut handshake = Vec::new();
    handshake.push(HANDSHAKE_CLIENT_HELLO);
    // Uzunluk için 3 bayt yer tutucu
    handshake.extend_from_slice(&[0, 0, 0]);

    // Client Version (TLS 1.2 - 0x0303)
    handshake.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
    // Random (32 bytes)
    handshake.extend_from_slice(&client_random);
    // Session ID (0 length)
    handshake.push(0);

    // Cipher Suites
    let ciphers = [
        TLS_AES_128_GCM_SHA256,
        TLS_AES_256_GCM_SHA384,
        TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        TLS_RSA_WITH_AES_128_GCM_SHA256,
    ];
    let cipher_len = (ciphers.len() * 2) as u16;
    handshake.extend_from_slice(&cipher_len.to_be_bytes());
    for c in &ciphers {
        handshake.extend_from_slice(&c.to_be_bytes());
    }

    // Compression Methods (1 byte: 0x00 = null)
    handshake.push(1);
    handshake.push(0);

    // Extensions: SNI (Server Name Indication)
    let mut extensions = Vec::new();
    let host_bytes = hostname.as_bytes();
    if !host_bytes.is_empty() {
        // Extension: server_name (0x0000)
        extensions.extend_from_slice(&0x0000u16.to_be_bytes());
        let list_len = (host_bytes.len() + 3) as u16;
        let ext_len = list_len + 2;
        extensions.extend_from_slice(&ext_len.to_be_bytes());
        extensions.extend_from_slice(&list_len.to_be_bytes());
        extensions.push(0); // NameType: host_name
        extensions.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
        extensions.extend_from_slice(host_bytes);

        // Extension: supported_versions (0x002b) -> TLS 1.3 + TLS 1.2
        extensions.extend_from_slice(&0x002bu16.to_be_bytes());
        extensions.extend_from_slice(&5u16.to_be_bytes()); // length
        extensions.push(4); // supported versions bytes length
        extensions.extend_from_slice(&TLS_VERSION_1_3.to_be_bytes());
        extensions.extend_from_slice(&TLS_VERSION_1_2.to_be_bytes());
    }

    if !extensions.is_empty() {
        handshake.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        handshake.extend_from_slice(&extensions);
    }

    // Handshake uzunluğunu doldur (ilk 4 byte hariç)
    let hs_len = (handshake.len() - 4) as u32;
    handshake[1] = ((hs_len >> 16) & 0xFF) as u8;
    handshake[2] = ((hs_len >> 8) & 0xFF) as u8;
    handshake[3] = (hs_len & 0xFF) as u8;

    // TLS Record Header:
    // Type (1 byte) + Version (2 bytes) + Length (2 bytes)
    out.push(TLS_RECORD_HANDSHAKE);
    out.extend_from_slice(&TLS_VERSION_1_0.to_be_bytes()); // Uyumlu 0x0301 rekor başlığı
    let rec_len = handshake.len() as u16;
    out.extend_from_slice(&rec_len.to_be_bytes());
    out.extend_from_slice(&handshake);
}

/// Gelen TLS kaydının başlığını (Record Header) ayrıştırır.
/// Dönen: `(content_type, version, record_payload_len)`
pub fn parse_record_header(buf: &[u8]) -> Option<(u8, u16, usize)> {
    if buf.len() < 5 {
        return None;
    }
    let ctype = buf[0];
    let ver = u16::from_be_bytes([buf[1], buf[2]]);
    let len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    Some((ctype, ver, len))
}

//! HTTP/2 istemcisi (RFC 7540) ve HPACK kodlayıcı/çözücü (RFC 7541).
//!
//! `drivers::http` içindeki ilk HPACK denemesinin üzerine kurulu tam
//! uygulamadır: dinamik tablo (kodlama + çözme yönleri), statik tablo,
//! Huffman kod çözme, ALPN ile `h2`/`http/1.1` seçimi ve TLS kayıtları
//! üzerinden şifreli çerçeve taşıması içerir. ALPN `h2` seçmezse aynı
//! bağlantı üzerinden HTTP/1.1 GET'e düşer (geri dönüş).
#![allow(dead_code)]

use super::http::{
    decode_chunked, header_value, parse_url, E_HTTP, E_TLS, E_URL, MAX_HOST, MAX_PATH,
};
use crate::drivers::e1000;
use crate::drivers::socket;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

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
        build_frame(FRAME_SETTINGS, 0, 0, &[])
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

    /// HPACK dinamik tablo: en son eklenen girdiler en düşük dinamik
    /// indeksleri alır (62 = en yeni). `entries` en eskiden en yeniye
    /// sıralıdır; indeksleme sondan yapılır.
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

        /// İndisle (1-based, 62 = en yeni dinamik girdi) ara.
        fn get_by_index(&self, index: usize) -> Option<(usize, &[u8], &[u8])> {
            // Dinamik tablo indeksleri statik tablo boyutundan sonra başlar.
            // Statik tablo boyutu = 61 (RFC 7541 §2.3.1); indeks 62 en yeni
            // girdidir, bu yüzden sondan sayılır.
            const STATIC_TABLE_SIZE: usize = 61;
            if index <= STATIC_TABLE_SIZE {
                return None; // Statik tablo, burada işlenmez.
            }
            let dyn_idx = index - STATIC_TABLE_SIZE;
            let pos = self.entries.len().checked_sub(dyn_idx)?;
            self.entries
                .get(pos)
                .map(|e| (index, e.name.as_slice(), e.value.as_slice()))
        }

        /// İsim-değer çiftini ara; bulunursa dinamik tablo indeksi (62 = en yeni) döner.
        fn find(&self, name: &[u8], value: Option<&[u8]>) -> Option<usize> {
            const STATIC_TABLE_SIZE: usize = 61;
            for (i, e) in self.entries.iter().rev().enumerate() {
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

        pub fn set_max_size(&mut self, size: usize) {
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

    // ------------------------------------------------------------------
    // HPACK çözme tarafı: statik tablo + dinamik tablo + Huffman (RFC 7541)
    // ------------------------------------------------------------------

    /// HPACK statik tablosu (RFC 7541 Ek A) — 61 girdi, indeks 1'den başlar.
    pub const STATIC_TABLE: &[(&[u8], &[u8])] = &[
        (b":authority", b""),
        (b":method", b"GET"),
        (b":method", b"POST"),
        (b":path", b"/"),
        (b":path", b"/index.html"),
        (b":scheme", b"http"),
        (b":scheme", b"https"),
        (b":status", b"200"),
        (b":status", b"204"),
        (b":status", b"206"),
        (b":status", b"304"),
        (b":status", b"400"),
        (b":status", b"404"),
        (b":status", b"500"),
        (b"accept-charset", b""),
        (b"accept-encoding", b"gzip, deflate"),
        (b"accept-language", b""),
        (b"accept-ranges", b""),
        (b"accept", b""),
        (b"access-control-allow-origin", b""),
        (b"age", b""),
        (b"allow", b""),
        (b"authorization", b""),
        (b"cache-control", b""),
        (b"content-disposition", b""),
        (b"content-encoding", b""),
        (b"content-language", b""),
        (b"content-length", b""),
        (b"content-location", b""),
        (b"content-range", b""),
        (b"content-type", b""),
        (b"cookie", b""),
        (b"date", b""),
        (b"etag", b""),
        (b"expect", b""),
        (b"expires", b""),
        (b"from", b""),
        (b"host", b""),
        (b"if-match", b""),
        (b"if-modified-since", b""),
        (b"if-none-match", b""),
        (b"if-range", b""),
        (b"if-unmodified-since", b""),
        (b"last-modified", b""),
        (b"link", b""),
        (b"location", b""),
        (b"max-forwards", b""),
        (b"proxy-authenticate", b""),
        (b"proxy-authorization", b""),
        (b"range", b""),
        (b"referer", b""),
        (b"refresh", b""),
        (b"retry-after", b""),
        (b"server", b""),
        (b"set-cookie", b""),
        (b"strict-transport-security", b""),
        (b"transfer-encoding", b""),
        (b"user-agent", b""),
        (b"vary", b""),
        (b"via", b""),
        (b"www-authenticate", b""),
    ];

    /// Statik tablo girdisini indeksle (1-based) getirir.
    fn static_entry(index: usize) -> Option<(&'static [u8], &'static [u8])> {
        if index == 0 || index > STATIC_TABLE.len() {
            return None;
        }
        Some(STATIC_TABLE[index - 1])
    }

    /// HPACK Huffman kod tablosu (RFC 7541 Ek B): sembol → (kod, bit uzunluğu).
    /// `code`, kod bitlerinin tamsayı değeridir; en anlamlı bitten okunur.
    /// Son girdi EOS (256)'dır.
    const HUFFMAN: &[(u32, u8)] = &[
        (0x1ff8, 13),
        (0x7fffd8, 23),
        (0xfffffe2, 28),
        (0xfffffe3, 28),
        (0xfffffe4, 28),
        (0xfffffe5, 28),
        (0xfffffe6, 28),
        (0xfffffe7, 28),
        (0xfffffe8, 28),
        (0xffffea, 24),
        (0x3ffffffc, 30),
        (0xfffffe9, 28),
        (0xfffffea, 28),
        (0x3ffffffd, 30),
        (0xfffffeb, 28),
        (0xfffffec, 28),
        (0xfffffed, 28),
        (0xfffffee, 28),
        (0xfffffef, 28),
        (0xffffff0, 28),
        (0xffffff1, 28),
        (0xffffff2, 28),
        (0x3ffffffe, 30),
        (0xffffff3, 28),
        (0xffffff4, 28),
        (0xffffff5, 28),
        (0xffffff6, 28),
        (0xffffff7, 28),
        (0xffffff8, 28),
        (0xffffff9, 28),
        (0xffffffa, 28),
        (0xffffffb, 28),
        (0x14, 6),
        (0x3f8, 10),
        (0x3f9, 10),
        (0xffa, 12),
        (0x1ff9, 13),
        (0x15, 6),
        (0xf8, 8),
        (0x7fa, 11),
        (0x3fa, 10),
        (0x3fb, 10),
        (0xf9, 8),
        (0x7fb, 11),
        (0xfa, 8),
        (0x16, 6),
        (0x17, 6),
        (0x18, 6),
        (0x0, 5),
        (0x1, 5),
        (0x2, 5),
        (0x19, 6),
        (0x1a, 6),
        (0x1b, 6),
        (0x1c, 6),
        (0x1d, 6),
        (0x1e, 6),
        (0x1f, 6),
        (0x5c, 7),
        (0xfb, 8),
        (0x7ffc, 15),
        (0x20, 6),
        (0xffb, 12),
        (0x3fc, 10),
        (0x1ffa, 13),
        (0x21, 6),
        (0x5d, 7),
        (0x5e, 7),
        (0x5f, 7),
        (0x60, 7),
        (0x61, 7),
        (0x62, 7),
        (0x63, 7),
        (0x64, 7),
        (0x65, 7),
        (0x66, 7),
        (0x67, 7),
        (0x68, 7),
        (0x69, 7),
        (0x6a, 7),
        (0x6b, 7),
        (0x6c, 7),
        (0x6d, 7),
        (0x6e, 7),
        (0x6f, 7),
        (0x70, 7),
        (0x71, 7),
        (0x72, 7),
        (0xfc, 8),
        (0x73, 7),
        (0xfd, 8),
        (0x1ffb, 13),
        (0x7fff0, 19),
        (0x1ffc, 13),
        (0x3ffc, 14),
        (0x22, 6),
        (0x7ffd, 15),
        (0x3, 5),
        (0x23, 6),
        (0x4, 5),
        (0x24, 6),
        (0x5, 5),
        (0x25, 6),
        (0x26, 6),
        (0x27, 6),
        (0x6, 5),
        (0x74, 7),
        (0x75, 7),
        (0x28, 6),
        (0x29, 6),
        (0x2a, 6),
        (0x7, 5),
        (0x2b, 6),
        (0x76, 7),
        (0x2c, 6),
        (0x8, 5),
        (0x9, 5),
        (0x2d, 6),
        (0x77, 7),
        (0x78, 7),
        (0x79, 7),
        (0x7a, 7),
        (0x7b, 7),
        (0x7ffe, 15),
        (0x7fc, 11),
        (0x3ffd, 14),
        (0x1ffd, 13),
        (0xffffffc, 28),
        (0xfffe6, 20),
        (0x3fffd2, 22),
        (0xfffe7, 20),
        (0xfffe8, 20),
        (0x3fffd3, 22),
        (0x3fffd4, 22),
        (0x3fffd5, 22),
        (0x7fffd9, 23),
        (0x3fffd6, 22),
        (0x7fffda, 23),
        (0x7fffdb, 23),
        (0x7fffdc, 23),
        (0x7fffdd, 23),
        (0x7fffde, 23),
        (0xffffeb, 24),
        (0x7fffdf, 23),
        (0xffffec, 24),
        (0xffffed, 24),
        (0x3fffd7, 22),
        (0x7fffe0, 23),
        (0xffffee, 24),
        (0x7fffe1, 23),
        (0x7fffe2, 23),
        (0x7fffe3, 23),
        (0x7fffe4, 23),
        (0x1fffdc, 21),
        (0x3fffd8, 22),
        (0x7fffe5, 23),
        (0x3fffd9, 22),
        (0x7fffe6, 23),
        (0x7fffe7, 23),
        (0xffffef, 24),
        (0x3fffda, 22),
        (0x1fffdd, 21),
        (0xfffe9, 20),
        (0x3fffdb, 22),
        (0x3fffdc, 22),
        (0x7fffe8, 23),
        (0x7fffe9, 23),
        (0x1fffde, 21),
        (0x7fffea, 23),
        (0x3fffdd, 22),
        (0x3fffde, 22),
        (0xfffff0, 24),
        (0x1fffdf, 21),
        (0x3fffdf, 22),
        (0x7fffeb, 23),
        (0x7fffec, 23),
        (0x1fffe0, 21),
        (0x1fffe1, 21),
        (0x3fffe0, 22),
        (0x1fffe2, 21),
        (0x7fffed, 23),
        (0x3fffe1, 22),
        (0x7fffee, 23),
        (0x7fffef, 23),
        (0xfffea, 20),
        (0x3fffe2, 22),
        (0x3fffe3, 22),
        (0x3fffe4, 22),
        (0x7ffff0, 23),
        (0x3fffe5, 22),
        (0x3fffe6, 22),
        (0x7ffff1, 23),
        (0x3ffffe0, 26),
        (0x3ffffe1, 26),
        (0xfffeb, 20),
        (0x7fff1, 19),
        (0x3fffe7, 22),
        (0x7ffff2, 23),
        (0x3fffe8, 22),
        (0x1ffffec, 25),
        (0x3ffffe2, 26),
        (0x3ffffe3, 26),
        (0x3ffffe4, 26),
        (0x7ffffde, 27),
        (0x7ffffdf, 27),
        (0x3ffffe5, 26),
        (0xfffff1, 24),
        (0x1ffffed, 25),
        (0x7fff2, 19),
        (0x1fffe3, 21),
        (0x3ffffe6, 26),
        (0x7ffffe0, 27),
        (0x7ffffe1, 27),
        (0x3ffffe7, 26),
        (0x7ffffe2, 27),
        (0xfffff2, 24),
        (0x1fffe4, 21),
        (0x1fffe5, 21),
        (0x3ffffe8, 26),
        (0x3ffffe9, 26),
        (0xffffffd, 28),
        (0x7ffffe3, 27),
        (0x7ffffe4, 27),
        (0x7ffffe5, 27),
        (0xfffec, 20),
        (0xfffff3, 24),
        (0xfffed, 20),
        (0x1fffe6, 21),
        (0x3fffe9, 22),
        (0x1fffe7, 21),
        (0x1fffe8, 21),
        (0x7ffff3, 23),
        (0x3fffea, 22),
        (0x3fffeb, 22),
        (0x1ffffee, 25),
        (0x1ffffef, 25),
        (0xfffff4, 24),
        (0xfffff5, 24),
        (0x3ffffea, 26),
        (0x7ffff4, 23),
        (0x3ffffeb, 26),
        (0x7ffffe6, 27),
        (0x3ffffec, 26),
        (0x3ffffed, 26),
        (0x7ffffe7, 27),
        (0x7ffffe8, 27),
        (0x7ffffe9, 27),
        (0x7ffffea, 27),
        (0x7ffffeb, 27),
        (0xfffffe, 28),
        (0x7ffffec, 27),
        (0x7ffffed, 27),
        (0x7ffffee, 27),
        (0x7ffffef, 27),
        (0x7fffff0, 27),
        (0x3ffffee, 26),
        (0x3fffffff, 30), // EOS (256)
    ];

    /// Huffman kodu `data` içinden çözer; semboller MSB-öncelikli okunur.
    /// Girdi bitleri EOS önekiyle (hepsi 1) doldurulur; kalan dolgulu bitler
    /// yok sayılır. `index`, `HuffmanDecoder::new` tarafından kurulan
    /// `(bit_uzunlugu << 32) | kod` anahtarlı sıralı dizidir.
    fn huffman_decode(index: &[(u64, u16)], data: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut acc: u64 = 0;
        let mut len: u64 = 0;
        for &byte in data {
            for bit in (0..8).rev() {
                acc = (acc << 1) | ((byte >> bit) & 1) as u64;
                len += 1;
                if len > 30 {
                    return None;
                }
                let key = (len << 32) | acc;
                if let Ok(i) = index.binary_search_by_key(&key, |e| e.0) {
                    let sym = index[i].1;
                    if sym == 256 {
                        return None; // EOS kodu dizge içinde geçersizdir.
                    }
                    out.push(sym as u8);
                    acc = 0;
                    len = 0;
                }
            }
        }
        Some(out)
    }

    /// HPACK çözme hatası (biçim hatalı blok, geçersiz indeks, bozuk
    /// Huffman dizgesi vb.).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct HpackError;

    /// Çözülmüş başlık alanı listesi: `(ad, değer)` çiftleri.
    pub type HeaderList = Vec<(Vec<u8>, Vec<u8>)>;

    /// HPACK başlık bloğu çözücü: statik + dinamik tablo, artımlı indeksleme,
    /// tablo boyutu güncellemesi ve Huffman dizge çözme (RFC 7541 §6).
    pub struct HpackDecoder {
        table: HpackDynamicTable,
        /// Karşı tarafın kodlayıcılarının kullanabileceği azami tablo boyutu
        /// (bizim SETTINGS_HEADER_TABLE_SIZE reklamımız; varsayılan 4096).
        max_table_size: usize,
        /// `(len << 32) | code` → sembol, ikili arama için sıralı.
        huff_index: Vec<(u64, u16)>,
    }

    impl HpackDecoder {
        pub fn new() -> Self {
            let mut huff_index = Vec::with_capacity(HUFFMAN.len());
            for (sym, &(code, bits)) in HUFFMAN.iter().enumerate() {
                huff_index.push((((bits as u64) << 32) | code as u64, sym as u16));
            }
            huff_index.sort_by_key(|e| e.0);
            Self {
                table: HpackDynamicTable::new(4096),
                max_table_size: 4096,
                huff_index,
            }
        }

        /// Dinamik tablo boyutu güncellemesi (bizim decoder'ımızın limiti).
        /// RFC 7541 §6.3: değer reklam ettiğimiz sınırı aşamaz.
        fn table_size_update(&mut self, new_size: usize) -> Result<(), HpackError> {
            if new_size > self.max_table_size {
                return Err(HpackError);
            }
            self.table.set_max_size(new_size);
            Ok(())
        }

        /// N-bit önekli tamsayıyı çöz (RFC 7541 §5.1).
        /// `first`, önek baytları zaten okunmuş ilk oktettir.
        fn decode_int(
            &self,
            first: u8,
            data: &[u8],
            pos: &mut usize,
            prefix_bits: u8,
        ) -> Result<usize, HpackError> {
            let max = (1usize << prefix_bits) - 1;
            let mut value = (first as usize) & max;
            if value < max {
                return Ok(value);
            }
            value = max;
            let mut shift = 0u32;
            loop {
                let b = *data.get(*pos).ok_or(HpackError)?;
                *pos += 1;
                value = value
                    .checked_add(((b & 0x7f) as usize) << shift)
                    .ok_or(HpackError)?;
                if b & 0x80 == 0 {
                    return Ok(value);
                }
                shift += 7;
                if shift > 28 {
                    return Err(HpackError);
                }
            }
        }

        /// Huffman bayraklı dizge sabitini çöz (RFC 7541 §5.2).
        fn decode_string(&self, data: &[u8], pos: &mut usize) -> Result<Vec<u8>, HpackError> {
            let first = *data.get(*pos).ok_or(HpackError)?;
            *pos += 1;
            let huffman = first & 0x80 != 0;
            let len = self.decode_int(first & 0x7f, data, pos, 7)?;
            let bytes = data.get(*pos..*pos + len).ok_or(HpackError)?;
            *pos += len;
            if huffman {
                huffman_decode(&self.huff_index, bytes).ok_or(HpackError)
            } else {
                Ok(bytes.to_vec())
            }
        }

        /// İndeksli adı çöz: indeks 0 ise yeni literal ad okunur.
        fn resolve_name(
            &self,
            index: usize,
            data: &[u8],
            pos: &mut usize,
        ) -> Result<Vec<u8>, HpackError> {
            if index == 0 {
                return self.decode_string(data, pos);
            }
            if let Some((n, _v)) = static_entry(index) {
                return Ok(n.to_vec());
            }
            if let Some((_i, n, _v)) = self.table.get_by_index(index) {
                return Ok(n.to_vec());
            }
            Err(HpackError)
        }

        /// Tam (ad, değer) çiftini indeksle çöz.
        fn resolve_pair(&self, index: usize) -> Result<(Vec<u8>, Vec<u8>), HpackError> {
            if let Some((n, v)) = static_entry(index) {
                return Ok((n.to_vec(), v.to_vec()));
            }
            if let Some((_i, n, v)) = self.table.get_by_index(index) {
                return Ok((n.to_vec(), v.to_vec()));
            }
            Err(HpackError)
        }

        /// Bir başlık bloğunu (HEADERS/CONTINUATION payload'larının
        /// birleştirilmiş HPACK verisi) çözer; `(ad, değer)` listesi döner.
        pub fn decode_block(&mut self, data: &[u8]) -> Result<HeaderList, HpackError> {
            let mut out = Vec::new();
            let mut pos = 0usize;
            while pos < data.len() {
                let b = data[pos];
                pos += 1;
                if b & 0x80 != 0 {
                    // Indexed Header Field (7-bit önek).
                    let index = self.decode_int(b & 0x7f, data, &mut pos, 7)?;
                    out.push(self.resolve_pair(index)?);
                } else if b & 0xc0 == 0x40 {
                    // Literal with Incremental Indexing (6-bit önek).
                    let index = self.decode_int(b & 0x3f, data, &mut pos, 6)?;
                    let name = self.resolve_name(index, data, &mut pos)?;
                    let value = self.decode_string(data, &mut pos)?;
                    self.table.insert(&name, &value);
                    out.push((name, value));
                } else if b & 0xe0 == 0x20 {
                    // Dynamic Table Size Update (5-bit önek).
                    let new_size = self.decode_int(b & 0x1f, data, &mut pos, 5)?;
                    self.table_size_update(new_size)?;
                } else {
                    // Literal without Indexing (0000xxxx) veya
                    // Literal Never Indexed (0001xxxx) — 4-bit önek.
                    // İkisi de aynı şekilde çözülür; asla ekleme yapılmaz.
                    let index = self.decode_int(b & 0x0f, data, &mut pos, 4)?;
                    let name = self.resolve_name(index, data, &mut pos)?;
                    let value = self.decode_string(data, &mut pos)?;
                    out.push((name, value));
                }
            }
            Ok(out)
        }
    }

    impl Default for HpackDecoder {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(test)]
    mod h2_tests {
        use super::*;
        use crate::drivers::http2::is_http2_response;

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

            // En yeni girdi (:authority) indeks 62'yi alır.
            let found = table.find(b":authority", Some(b"example.com"));
            assert_eq!(found, Some(62));
            // :path /index.html bir sonraki en yeni girdi → indeks 63.
            let found_path = table.find(b":path", Some(b"/index.html"));
            assert_eq!(found_path, Some(63));

            // get_by_index: 62 → en yeni (:authority).
            let (idx, n, v) = table.get_by_index(62).unwrap();
            assert_eq!(idx, 62);
            assert_eq!(n, b":authority");
            assert_eq!(v, b"example.com");
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

        // -- HPACK çözücü testleri (RFC 7541 Ek C vektörleri) --

        fn hex_to_vec(s: &str) -> Vec<u8> {
            let cleaned: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < cleaned.len() {
                let hi = (cleaned[i] as char).to_digit(16).unwrap() as u8;
                let lo = (cleaned[i + 1] as char).to_digit(16).unwrap() as u8;
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        #[test]
        fn test_hpack_decode_indexed_static() {
            // Tek başlık: :status: 200 (statik indeks 8 → 0x88)
            let mut dec = HpackDecoder::new();
            let headers = dec.decode_block(&[0x88]).unwrap();
            assert_eq!(headers.len(), 1);
            assert_eq!(headers[0].0, b":status");
            assert_eq!(headers[0].1, b"200");
        }

        #[test]
        fn test_hpack_decode_rfc_c51_first_response() {
            // RFC 7541 C.5.1 — Huffman'siz ilk yanıt.
            let block = hex_to_vec(
                "4803 3330 32 58 07 70726976617465 61 1d \
             4d6f6e2c203231204f637420323031332032303a31333a323120474d54 \
             6e 17 68747470733a2f2f7777772e6578616d706c652e636f6d",
            );
            let mut dec = HpackDecoder::new();
            let headers = dec.decode_block(&block).unwrap();
            assert_eq!(headers.len(), 4);
            assert_eq!(headers[0], (b":status".to_vec(), b"302".to_vec()));
            assert_eq!(headers[1], (b"cache-control".to_vec(), b"private".to_vec()));
            assert_eq!(headers[2].0, b"date");
            assert_eq!(headers[2].1, b"Mon, 21 Oct 2013 20:13:21 GMT");
            assert_eq!(headers[3].0, b"location");
            assert_eq!(headers[3].1, b"https://www.example.com");
        }

        #[test]
        fn test_hpack_decode_rfc_c61_huffman_response() {
            // RFC 7541 C.6.1 — Huffman kodlu ilk yanıt (aynı başlıklar).
            let block = hex_to_vec(
                "4882 6402 5885 aec3 771a 4b61 96d0 7abe \
             9410 54d4 44a8 2005 9504 0b81 66e0 82a6 \
             2d1b ff6e 919d 29ad 1718 63c7 8f0b 97c8 \
             e9ae 82ae 43d3",
            );
            let mut dec = HpackDecoder::new();
            let headers = dec.decode_block(&block).unwrap();
            assert_eq!(headers.len(), 4);
            assert_eq!(headers[0], (b":status".to_vec(), b"302".to_vec()));
            assert_eq!(headers[1], (b"cache-control".to_vec(), b"private".to_vec()));
            assert_eq!(headers[2].1, b"Mon, 21 Oct 2013 20:13:21 GMT");
            assert_eq!(headers[3].1, b"https://www.example.com");
        }

        #[test]
        fn test_hpack_decode_dynamic_table_reuse() {
            // C.6.1 bloğunu çözünce dinamik tablo dolar; ikinci blok
            // (C.6.2) dinamik indekslerle aynı başlıkları üretmelidir.
            let block1 = hex_to_vec(
                "4882 6402 5885 aec3 771a 4b61 96d0 7abe \
             9410 54d4 44a8 2005 9504 0b81 66e0 82a6 \
             2d1b ff6e 919d 29ad 1718 63c7 8f0b 97c8 \
             e9ae 82ae 43d3",
            );
            let block2 = hex_to_vec("4883 640e ffc1 c0bf");
            let mut dec = HpackDecoder::new();
            let h1 = dec.decode_block(&block1).unwrap();
            let h2 = dec.decode_block(&block2).unwrap();
            assert_eq!(h1.len(), 4);
            assert_eq!(h2.len(), 4);
            assert_eq!(h2[0], (b":status".to_vec(), b"307".to_vec()));
            assert_eq!(h2[1], h1[1]); // cache-control: private (dinamik)
            assert_eq!(h2[2], h1[2]); // date (dinamik)
            assert_eq!(h2[3], h1[3]); // location (dinamik)
        }

        #[test]
        fn test_hpack_decode_huffman_string() {
            // Huffman kodlu "302": 0x64 0x02 (RFC C.6.1 :status değeri).
            // Tek başlık olarak: literal without indexing, indexed name :status (idx 8).
            // 0x08 → name index 8, sonra string: H=1, len=2 → 0x82 0x64 0x02
            let mut dec = HpackDecoder::new();
            let headers = dec.decode_block(&[0x08, 0x82, 0x64, 0x02]).unwrap();
            assert_eq!(headers[0], (b":status".to_vec(), b"302".to_vec()));
        }
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
    socket::send(fd, h2::PREFACE).map_err(|_| socket::E_IO)?;
    socket::send(fd, &h2::build_client_settings_full()).map_err(|_| socket::E_IO)?;
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
    /// HEADERS + CONTINUATION'dan toplanan HPACK başlık bloğu parçası.
    header_block: Vec<u8>,
    /// END_HEADERS alındıysa çözümlenmiş başlık listesi.
    headers: h2::HeaderList,
    /// Yanıt gövdesi (DATA çerçeveleri birleştirilmiş).
    body: Vec<u8>,
    /// Yanıt gövdesi bitti mi (END_STREAM alındı mı).
    body_complete: bool,
    /// Durum kodu (HPACK :status'ten çözülmüş).
    status_code: Option<u16>,
    /// Akış düzeyi flow-control penceresi.
    stream_window: u32,
}

impl H2Stream {
    fn new(stream_id: u32, initial_window: u32) -> Self {
        Self {
            stream_id,
            state: H2StreamState::Idle,
            header_block: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
            body_complete: false,
            status_code: None,
            stream_window: initial_window,
        }
    }
}

/// HTTP/2 (veya ALPN geri dönüşünde HTTP/1.1) istemci bağlantısı.
pub struct Http2Connection {
    fd: i32,
    host: String,
    port: u16,
    is_https: bool,
    tls_session: Option<crate::drivers::tls::TlsSession>,
    /// ALPN `h2` seçtiyse true; http/1.1'e düşüldüyse false.
    h2_enabled: bool,
    next_stream_id: u32,
    streams: alloc::vec::Vec<Option<H2Stream>>,
    /// Akış ID'sine göre stream index'i.
    stream_map: alloc::collections::BTreeMap<u32, usize>,
    /// Bağlantı düzeyi flow-control penceresi.
    connection_window: u32,
    /// Sunucudan gelen SETTINGS MAX_CONCURRENT_STREAMS değeri.
    max_concurrent_streams: u32,
    /// Sunucudan gelen SETTINGS_INITIAL_WINDOW_SIZE değeri.
    initial_window_size: u32,
    /// Şu anda açık akış sayısı.
    open_stream_count: u32,
    /// HPACK kodlayıcı dinamik tablosu (istemci → sunucu yönü).
    dyn_table: h2::HpackDynamicTable,
    /// HPACK çözücü (sunucu → istemci yönü; kendi dinamik tablosu var).
    hpack_dec: h2::HpackDecoder,
    /// Sunucu SETTINGS'ından arta kalan çözülmüş baytlar (sonraki
    /// çerçeveler bunların devamıdır).
    rx_pending: Vec<u8>,
}

/// TLS el sıkışması için tekrar eden istemci rastgeleliği üretir.
fn make_client_random() -> [u8; 32] {
    let mut client_random = [0u8; 32];
    let mut state = crate::boot::time::main_monotonic_ms() as u64 ^ 0x9e37_79b9_7f4a_7c15;
    for b in client_random.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
    }
    client_random
}

impl Http2Connection {
    /// Yeni bir bağlantı kurar: TCP bağlan, TLS el sıkışması (https ise),
    /// ALPN ile protokol seç, HTTP/2 ise preface + SETTINGS el sıkışması yap.
    pub fn connect(host: &str, port: u16, is_https: bool) -> Result<Self, i32> {
        if !e1000::is_ready() {
            return Err(socket::E_NOTCONN);
        }

        let (fd, tls_session) =
            (|| -> Result<(i32, Option<crate::drivers::tls::TlsSession>), i32> {
                // TLS el sıkışması yardımcı closure (IPv6 ve IPv4 için ortak).
                let tls_handshake =
                    |fd: i32, host: &str| -> Result<Option<crate::drivers::tls::TlsSession>, i32> {
                        if !is_https {
                            return Ok(None);
                        }
                        let client_random = make_client_random();

                        let mut hello = Vec::new();
                        let mut ecdhe_secret = [0u8; 32];
                        crate::drivers::tls::build_client_hello_with_alpn(
                            host,
                            &client_random,
                            &mut hello,
                            &mut ecdhe_secret,
                            false,
                            None,
                            None,
                            &[b"h2", b"http/1.1"],
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

                            if session.state() == crate::drivers::tls::TlsState::Tls13ServerFinished
                            {
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
                        Ok(Some(session))
                    };

                // IPv6 (AAAA) öncelikli, IPv4'e otomatik geri düşme — bağlantı/TLS
                // hatasında IPv4 yoluna düşülür (dual-stack fallback).
                if let Ok(v6) = socket::resolve_host_aaaa(host) {
                    let fd6 =
                        socket::socket(socket::AF_INET6, socket::SOCK_STREAM).inspect_err(|e| {
                            crate::serial_print("[H2v6] socket oluşturulamadı: ");
                            crate::serial_print(&e.to_string());
                            crate::serial_print("\n");
                        })?;
                    let _ = socket::set_timeout(fd6, 8000);
                    if socket::connect_v6(fd6, v6, port).is_ok() {
                        match tls_handshake(fd6, host) {
                            Ok(tls) => return Ok((fd6, tls)),
                            Err(e) => {
                                let _ = socket::close(fd6);
                                return Err(e);
                            }
                        }
                    }
                    let _ = socket::close(fd6);
                }

                // IPv4 yolu
                let ip = socket::resolve_host(host)?;
                let fd4 =
                    socket::socket(socket::AF_INET, socket::SOCK_STREAM).inspect_err(|e| {
                        crate::serial_print("[H2] socket oluşturulamadı: ");
                        crate::serial_print(&e.to_string());
                        crate::serial_print("\n");
                    })?;
                let _ = socket::set_timeout(fd4, 8000);
                socket::connect(fd4, ip, port).inspect_err(|_| {
                    socket::close(fd4);
                })?;
                let tls = tls_handshake(fd4, host)?;
                Ok((fd4, tls))
            })()?;

        // ALPN sonucu: h2 seçildiyse HTTP/2, değilse HTTP/1.1'e düş.
        // Şifresiz http:// için prior-knowledge HTTP/2 varsayılır.
        let h2_enabled = if is_https {
            tls_session
                .as_ref()
                .and_then(|s| s.negotiated_protocol())
                .is_some_and(|p| p == b"h2")
        } else {
            true
        };

        let mut conn = Self {
            fd,
            host: String::from(host),
            port,
            is_https,
            tls_session,
            h2_enabled,
            next_stream_id: 1, // İstemci akış ID'leri tek sayıdır
            streams: alloc::vec::Vec::new(),
            stream_map: alloc::collections::BTreeMap::new(),
            connection_window: 65535,
            max_concurrent_streams: 100,
            initial_window_size: 65535,
            open_stream_count: 0,
            dyn_table: h2::HpackDynamicTable::new(4096), // RFC 7541 varsayılanı
            hpack_dec: h2::HpackDecoder::new(),
            rx_pending: Vec::new(),
        };

        if h2_enabled {
            // HTTP/2 bağlantı önyüzü + boş SETTINGS (TLS ile şifrelenir).
            let mut preface_data = Vec::new();
            preface_data.extend_from_slice(h2::PREFACE);
            preface_data.extend_from_slice(&h2::build_client_settings_full());
            conn.send_app_data(&preface_data)?;

            // Sunucu SETTINGS'ını oku; MAX_CONCURRENT_STREAMS,
            // INITIAL_WINDOW_SIZE ve HEADER_TABLE_SIZE uygulanır.
            conn.read_server_settings()?;
        }

        Ok(conn)
    }

    /// Uygulama verisini gönderir: TLS oturumu varsa şifreler, değilse düz.
    /// Kısmi gönderimleri de tamamlar.
    fn send_app_data(&mut self, data: &[u8]) -> Result<(), i32> {
        let mut off = 0usize;
        while off < data.len() {
            let sent = if self.tls_session.is_some() {
                // TLS kaydı başına en fazla 8 KiB düz metin (kayıt boyutu sınırı).
                let end = (off + 8192).min(data.len());
                let rec = self
                    .tls_session
                    .as_mut()
                    .and_then(|s| s.encrypt_app_data(&data[off..end]))
                    .ok_or(E_TLS)?;
                let n = socket::send(self.fd, &rec).map_err(|_| socket::E_IO)?;
                if n == 0 {
                    return Err(socket::E_IO);
                }
                end - off
            } else {
                let n = socket::send(self.fd, &data[off..]).map_err(|_| socket::E_IO)?;
                if n == 0 {
                    return Err(socket::E_IO);
                }
                n
            };
            off += sent;
        }
        Ok(())
    }

    /// Soketten bir parça okuyup çözülen uygulama verisini `pending`'e ekler.
    /// TLS oturumu varsa kayıtları çözer. 0 bayt = karşı taraf kapattı.
    fn recv_app_data(&mut self, pending: &mut Vec<u8>) -> Result<usize, i32> {
        let mut raw = [0u8; 4096];
        let n = socket::recv(self.fd, &mut raw).map_err(|_| socket::E_IO)?;
        if n == 0 {
            return Ok(0);
        }
        if let Some(ref mut session) = self.tls_session {
            session.feed_tcp_data(&raw[..n]);
            let mut app = Vec::new();
            let mut hs = Vec::new();
            session
                .process_records(&mut hs, &mut app)
                .map_err(|_| E_TLS)?;
            // El sıkışma sonrası çıkan kayıtlar (örn. session ticket) gönder.
            let outgoing = session.take_outgoing();
            if !outgoing.is_empty() {
                socket::send(self.fd, &outgoing).map_err(|_| socket::E_IO)?;
            }
            pending.extend_from_slice(&app);
            // Soketten okunan bayt sayısını döndür: yalnızca el sıkışma
            // kaydı çözülmüşse app boş olabilir ama bağlantı açık demektir.
            Ok(n)
        } else {
            pending.extend_from_slice(&raw[..n]);
            Ok(n)
        }
    }

    /// Sunucunun ilk SETTINGS çerçevesini okur ve parametreleri uygular.
    fn read_server_settings(&mut self) -> Result<(), i32> {
        let mut pending: Vec<u8> = Vec::new();
        let deadline = crate::boot::time::main_monotonic_ms() + 8_000;
        loop {
            // Tam bir SETTINGS çerçevesi (9 + payload) birikene kadar oku.
            if pending.len() >= 9 {
                if let Some(fh) = h2::parse_frame_header(&pending[..9]) {
                    let frame_len = 9 + fh.payload_len() as usize;
                    if pending.len() >= frame_len {
                        if fh.r#type == h2::FRAME_SETTINGS && fh.flags & h2::FLAG_ACK == 0 {
                            self.apply_settings(&pending[9..frame_len]);
                            // SETTINGS sonrası gelen baytlar (örn. ilk
                            // WINDOW_UPDATE) bir sonraki okumaya devredilir.
                            self.rx_pending = pending.split_off(frame_len);
                            return Ok(());
                        }
                        // ACK veya beklenmedik çerçeve: atla.
                        pending.drain(..frame_len);
                        continue;
                    }
                }
            }
            if pending.len() > 1 << 20 {
                return Err(socket::E_IO);
            }
            if crate::boot::time::main_monotonic_ms() > deadline {
                return Err(socket::E_TIMEOUT);
            }
            if self.recv_app_data(&mut pending)? == 0 {
                return Err(socket::E_IO);
            }
        }
    }

    /// SETTINGS payload'unu uygular (RFC 7540 §6.5.2).
    fn apply_settings(&mut self, payload: &[u8]) {
        let mut off = 0usize;
        while off + 6 <= payload.len() {
            let id = u16::from_be_bytes([payload[off], payload[off + 1]]);
            let value = u32::from_be_bytes([
                payload[off + 2],
                payload[off + 3],
                payload[off + 4],
                payload[off + 5],
            ]);
            match id {
                h2::SETTINGS_MAX_CONCURRENT_STREAMS => {
                    self.max_concurrent_streams = value.max(1);
                }
                h2::SETTINGS_INITIAL_WINDOW_SIZE => {
                    self.initial_window_size = value.min(0x7fff_ffff);
                }
                h2::SETTINGS_HEADER_TABLE_SIZE => {
                    // Sunucunun kodlayıcı tablosu üst sınırı: bizim
                    // HPACK kodlayıcımıza uygulanır.
                    self.dyn_table.set_max_size(value as usize);
                }
                _ => {}
            }
            off += 6;
        }
    }

    /// Yeni bir akış açar ve stream_id döndürür.
    fn open_stream(&mut self) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2; // İstemci: 1, 3, 5, ...
        let stream = H2Stream::new(stream_id, self.initial_window_size);
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

    /// HTTP GET isteği gönderir ve yanıtı `resp_buf`'a yazar.
    /// ALPN `h2` ise HTTP/2, değilse HTTP/1.1 kullanılır.
    /// Dönen: (durum_kodu, başlık_uzunluğu, toplam_uzunluk)
    pub fn get(
        &mut self,
        authority: &[u8],
        path: &[u8],
        resp_buf: &mut [u8],
    ) -> Result<(u16, usize, usize), i32> {
        if self.h2_enabled {
            self.get_h2(authority, path, resp_buf)
        } else {
            self.get_http11(authority, path, resp_buf)
        }
    }

    /// HTTP/2 GET: HEADERS çerçevesi gönder, HEADERS/DATA çerçevelerini topla.
    fn get_h2(
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

        // HEADERS çerçevesi oluştur (HPACK ile).
        let headers_block = h2::encode_get_headers(authority, path, is_https, &mut self.dyn_table);
        let headers_frame = h2::build_frame(
            h2::FRAME_HEADERS,
            h2::FLAG_END_HEADERS | h2::FLAG_END_STREAM,
            stream_id,
            &headers_block,
        );
        self.send_app_data(&headers_frame)?;

        {
            let stream = self.stream_mut(stream_id).ok_or(socket::E_IO)?;
            stream.state = H2StreamState::HalfClosedLocal;
        }

        // Çözülmüş uygulama baytları; TLS kayıt sınırlarından bağımsız olarak
        // çerçeve hizası korunur (kayıt sınırında bölünen çerçeveler dahil).
        // Sunucu SETTINGS'ından arta kalan baytlarla başla.
        let mut pending: Vec<u8> = core::mem::take(&mut self.rx_pending);
        let mut header_block: Vec<u8> = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        let mut status_code: Option<u16> = None;
        let mut body_complete = false;
        // HEADERS END_STREAM taşıyıp END_HEADERS taşımıyorsa gövde,
        // CONTINUATION'lar bitince sonlanır.
        let mut headers_end_stream = false;
        let deadline = crate::boot::time::main_monotonic_ms() + 10_000;

        while !body_complete {
            // Tam çerçeveleri `pending` içinden işle.
            let mut offset = 0usize;
            let mut progressed = false;
            while offset + 9 <= pending.len() {
                let fh = match h2::parse_frame_header(&pending[offset..offset + 9]) {
                    Some(fh) => fh,
                    None => break,
                };
                let payload_len = fh.payload_len() as usize;
                if offset + 9 + payload_len > pending.len() {
                    break; // Çerçevenin tamamı henüz gelmedi.
                }
                let payload = &pending[offset + 9..offset + 9 + payload_len];
                progressed = true;

                match fh.r#type {
                    h2::FRAME_SETTINGS if fh.flags & h2::FLAG_ACK == 0 => {
                        self.apply_settings(payload);
                        let ack = h2::build_frame(h2::FRAME_SETTINGS, h2::FLAG_ACK, 0, &[]);
                        self.send_app_data(&ack)?;
                    }
                    h2::FRAME_SETTINGS => {}
                    h2::FRAME_HEADERS => {
                        if fh.stream_id() == stream_id {
                            let frag = self.strip_frame_padding(payload, fh.flags, true);
                            header_block.extend_from_slice(frag);
                            if fh.flags & h2::FLAG_END_STREAM != 0 {
                                headers_end_stream = true;
                            }
                            if fh.flags & h2::FLAG_END_HEADERS != 0 {
                                let headers = self.decode_headers(&header_block)?;
                                status_code = headers
                                    .iter()
                                    .find(|(n, _)| n == b":status")
                                    .and_then(|(_, v)| parse_status_code(v));
                                if let Some(s) = self.stream_mut(stream_id) {
                                    s.headers = headers;
                                }
                                if headers_end_stream {
                                    body_complete = true;
                                }
                            }
                        }
                    }
                    h2::FRAME_CONTINUATION => {
                        if fh.stream_id() == stream_id {
                            header_block.extend_from_slice(payload);
                            if fh.flags & h2::FLAG_END_HEADERS != 0 {
                                let headers = self.decode_headers(&header_block)?;
                                status_code = headers
                                    .iter()
                                    .find(|(n, _)| n == b":status")
                                    .and_then(|(_, v)| parse_status_code(v));
                                if let Some(s) = self.stream_mut(stream_id) {
                                    s.headers = headers;
                                }
                                if headers_end_stream {
                                    body_complete = true;
                                }
                            }
                        }
                    }
                    h2::FRAME_DATA => {
                        if fh.stream_id() == stream_id {
                            let data = self.strip_frame_padding(payload, fh.flags, false);
                            let data_len = data.len() as u32;
                            // Akış + bağlantı penceresi takibi.
                            if let Some(s) = self.stream_mut(stream_id) {
                                s.stream_window = s.stream_window.saturating_sub(data_len);
                            }
                            self.connection_window =
                                self.connection_window.saturating_sub(data_len);

                            if body.len() < resp_buf.len() {
                                let room = resp_buf.len() - body.len();
                                body.extend_from_slice(&data[..data.len().min(room)]);
                            }
                            if fh.flags & h2::FLAG_END_STREAM != 0 {
                                body_complete = true;
                            }
                            // Pencere daraldıysa WINDOW_UPDATE gönder.
                            if self.connection_window < 16384 {
                                let wu = h2::build_window_update(0, 65535);
                                self.send_app_data(&wu)?;
                                self.connection_window =
                                    self.connection_window.saturating_add(65535);
                            }
                            if let Some(s) = self.stream_mut(stream_id) {
                                if s.stream_window < 16384 {
                                    let wu = h2::build_window_update(stream_id, 65535);
                                    self.send_app_data(&wu)?;
                                    if let Some(s2) = self.stream_mut(stream_id) {
                                        s2.stream_window = s2.stream_window.saturating_add(65535);
                                    }
                                }
                            }
                        }
                    }
                    h2::FRAME_WINDOW_UPDATE => {
                        if fh.stream_id() == 0 && payload_len >= 4 {
                            let increment = u32::from_be_bytes([
                                payload[0], payload[1], payload[2], payload[3],
                            ]) & 0x7fff_ffff;
                            self.connection_window =
                                self.connection_window.saturating_add(increment);
                        } else if fh.stream_id() == stream_id && payload_len >= 4 {
                            let increment = u32::from_be_bytes([
                                payload[0], payload[1], payload[2], payload[3],
                            ]) & 0x7fff_ffff;
                            if let Some(s) = self.stream_mut(stream_id) {
                                s.stream_window = s.stream_window.saturating_add(increment);
                            }
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
                        let mut ping_payload = [0u8; 8];
                        let n = payload.len().min(8);
                        ping_payload[..n].copy_from_slice(&payload[..n]);
                        let ack = h2::build_frame(h2::FRAME_PING, h2::FLAG_ACK, 0, &ping_payload);
                        self.send_app_data(&ack)?;
                    }
                    _ => {}
                }

                offset += 9 + payload_len;
            }
            if offset > 0 {
                pending.drain(..offset);
            }
            if progressed {
                continue; // Aynı tamponda başka çerçeve olabilir.
            }

            if body.len() >= resp_buf.len() && !resp_buf.is_empty() {
                break; // Yanıt tamponu doldu.
            }
            if crate::boot::time::main_monotonic_ms() > deadline {
                return Err(socket::E_TIMEOUT);
            }
            if self.recv_app_data(&mut pending)? == 0 {
                break; // Karşı taraf kapattı.
            }
        }

        // Stream durumunu kapat.
        if let Some(s) = self.stream_mut(stream_id) {
            s.state = H2StreamState::Closed;
        }
        self.open_stream_count = self.open_stream_count.saturating_sub(1);

        // İşlenmemiş çerçeve baytlarını bir sonraki get() için sakla.
        self.rx_pending = pending;

        // Gövdeyi resp_buf'ye kopyala.
        let status = status_code.unwrap_or(200);
        let copy_len = body.len().min(resp_buf.len());
        resp_buf[..copy_len].copy_from_slice(&body[..copy_len]);

        Ok((status, 0, copy_len))
    }

    /// HEADERS/DATA çerçeve payload'ından padding'i ve HEADERS durumunda
    /// priority alanını ayıklar; işlenmiş dilimi döner.
    fn strip_frame_padding<'a>(&self, payload: &'a [u8], flags: u8, is_headers: bool) -> &'a [u8] {
        let mut data = payload;
        let mut pad_len = 0usize;
        if flags & h2::FLAG_PADDED != 0 && !data.is_empty() {
            pad_len = data[0] as usize;
            data = &data[1..];
        }
        if is_headers && flags & h2::FLAG_PRIORITY != 0 && data.len() >= 5 {
            data = &data[5..];
        }
        // Padding uzunluğu kalan baytları da kapsar.
        if flags & h2::FLAG_PADDED != 0 && data.len() > pad_len {
            data = &data[..data.len() - pad_len];
        }
        data
    }

    /// HPACK başlık bloğunu çözüp `(ad, değer)` listesi döner.
    fn decode_headers(&mut self, block: &[u8]) -> Result<h2::HeaderList, i32> {
        self.hpack_dec.decode_block(block).map_err(|_| socket::E_IO)
    }

    /// ALPN `http/1.1` (veya h2'siz) bağlantıda klasik HTTP/1.1 GET yapar.
    /// `Connection: close` gönderilir; yanıt başlıkları + gövde ayrıştırılır.
    fn get_http11(
        &mut self,
        authority: &[u8],
        path: &[u8],
        resp_buf: &mut [u8],
    ) -> Result<(u16, usize, usize), i32> {
        let mut req = Vec::with_capacity(256 + path.len() + authority.len());
        req.extend_from_slice(b"GET ");
        req.extend_from_slice(path);
        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        req.extend_from_slice(authority);
        req.extend_from_slice(
            b"\r\nUser-Agent: minOs/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        );
        self.send_app_data(&req)?;

        // Yanıtı topla: başlık sonu + gövde.
        let mut raw: Vec<u8> = Vec::new();
        let deadline = crate::boot::time::main_monotonic_ms() + 10_000;
        loop {
            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                // Başlık sonu bulunduysa Content-Length'e göre erken çıkabiliriz;
                // chunked/close-delimited için bağlantı kapanışını bekleriz.
                if let Some(hlen) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = &raw[..hlen + 4];
                    let body_have = raw.len() - (hlen + 4);
                    if let Some(cl) = header_value(headers, b"content-length") {
                        if let Ok(cl) = core::str::from_utf8(cl)
                            .unwrap_or("")
                            .trim()
                            .parse::<usize>()
                        {
                            if body_have >= cl {
                                break;
                            }
                        }
                    }
                }
            }
            if raw.len() >= resp_buf.len() + 8192 {
                break; // Yeterli veri toplandı.
            }
            if crate::boot::time::main_monotonic_ms() > deadline {
                break;
            }
            let mut tmp: Vec<u8> = Vec::new();
            match self.recv_app_data(&mut tmp) {
                Ok(0) => break,
                Ok(_) => raw.extend_from_slice(&tmp),
                Err(_) => break,
            }
        }

        // Başlık/gövde ayrımı.
        let hpos = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .ok_or(E_HTTP)?;
        let headers = &raw[..hpos];
        let status = parse_http11_status(headers).unwrap_or(200);
        let mut body = raw[hpos..].to_vec();

        // Chunked gövdeyi çöz.
        let is_chunked = header_value(headers, b"transfer-encoding")
            .map(|v| {
                core::str::from_utf8(v)
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("chunked")
            })
            .unwrap_or(false);
        if is_chunked {
            let mut cbuf = body.clone();
            if let Some(dlen) = decode_chunked(&mut cbuf, body.len()) {
                body = cbuf[..dlen].to_vec();
            }
        }

        let copy_len = body.len().min(resp_buf.len());
        resp_buf[..copy_len].copy_from_slice(&body[..copy_len]);
        Ok((status, 0, copy_len))
    }

    /// Bağlantıyı kapatır (h2 ise GOAWAY gönderir).
    pub fn close(mut self) -> Result<(), i32> {
        if self.h2_enabled {
            let goaway = h2::build_frame(h2::FRAME_GOAWAY, 0, 0, &[0, 0, 0, 0]);
            let _ = self.send_app_data(&goaway);
        }
        socket::close(self.fd);
        Ok(())
    }
}

/// HTTP/1.1 durum satırından durum kodunu çözer.
fn parse_http11_status(headers: &[u8]) -> Option<u16> {
    if headers.len() < 12 || &headers[..5] != b"HTTP/" {
        return None;
    }
    let sp = headers.iter().position(|&b| b == b' ')?;
    if headers.len() < sp + 4 {
        return None;
    }
    let d = |c: u8| (c as char).to_digit(10).unwrap_or(0) as u16;
    if !headers[sp + 1].is_ascii_digit()
        || !headers[sp + 2].is_ascii_digit()
        || !headers[sp + 3].is_ascii_digit()
    {
        return None;
    }
    Some(d(headers[sp + 1]) * 100 + d(headers[sp + 2]) * 10 + d(headers[sp + 3]))
}

/// HPACK `:status` değerini ("200", "404", ...) u16'ya çevirir.
fn parse_status_code(v: &[u8]) -> Option<u16> {
    if v.len() < 3 || v.len() > 5 {
        return None;
    }
    let mut n: u16 = 0;
    for &c in v {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((c - b'0') as u16)?;
    }
    Some(n)
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

// ---------------------------------------------------------------------------
// HTTP/2 birim testleri (NIC/TLS gerektirmez)
// ---------------------------------------------------------------------------

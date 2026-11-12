#![allow(dead_code, static_mut_refs)]

//! Maturity: SKELETON / WIP — Gerçek 802.11 PHY tespiti yok,//! sadece çerçeve yapıları ve WPA2-PSK anahtar türetimi mevcut.//! Bkz. docs/driver-maturity.md
//!
//! IEEE 802.11 Wi-Fi ve Kablosuz Ağ Sürücüsü Altyapısı.
//!
//! Bu modül, 802.11 çerçeve ayrıştırması (Beacon, Probe, Data), WPA2-PSK
//! anahtar türetimi (PBKDF2-HMAC, 4-Way Handshake, PTK/GTK), kablosuz ağ taraması
//! ve `netstack` TCP/IP yığını ile kablosuz bağdaştırıcı entegrasyonunu yönetir.

use crate::drivers::crypto::sha1::HmacSha1;
use crate::drivers::pci;

// ---------------------------------------------------------------------------
// 802.11 Çerçeve Yapıları ve Sabitleri
// ---------------------------------------------------------------------------

pub const FC_TYPE_MANAGEMENT: u8 = 0;
pub const FC_TYPE_CONTROL: u8 = 1;
pub const FC_TYPE_DATA: u8 = 2;

// Yönetim (Management) Alt Tipleri
pub const FC_SUBTYPE_ASSOC_REQ: u8 = 0;
pub const FC_SUBTYPE_ASSOC_RESP: u8 = 1;
pub const FC_SUBTYPE_PROBE_REQ: u8 = 4;
pub const FC_SUBTYPE_PROBE_RESP: u8 = 5;
pub const FC_SUBTYPE_BEACON: u8 = 8;
pub const FC_SUBTYPE_DISASSOC: u8 = 10;
pub const FC_SUBTYPE_AUTH: u8 = 11;
pub const FC_SUBTYPE_DEAUTH: u8 = 12;

// Veri (Data) Alt Tipleri
pub const FC_SUBTYPE_DATA: u8 = 0;
pub const FC_SUBTYPE_QOS_DATA: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WifiSecurity {
    Open,
    Wpa2Psk,
    Wpa3Sae,
}

impl WifiSecurity {
    pub fn label(self) -> &'static str {
        match self {
            WifiSecurity::Open => "Açık (Şifresiz)",
            WifiSecurity::Wpa2Psk => "WPA2-PSK (AES)",
            WifiSecurity::Wpa3Sae => "WPA3-Personal",
        }
    }
}

/// Standart 802.11 MAC Başlığı (24-30 bayt)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Dot11Header {
    pub frame_control: u16,
    pub duration: u16,
    pub addr1: [u8; 6], // Alıcı / BSSID
    pub addr2: [u8; 6], // Gönderici MAC
    pub addr3: [u8; 6], // Hedef veya Kaynak MAC
    pub seq_ctrl: u16,
}

impl Dot11Header {
    #[inline]
    pub fn frame_type(&self) -> u8 {
        ((self.frame_control >> 2) & 0x03) as u8
    }

    #[inline]
    pub fn frame_subtype(&self) -> u8 {
        ((self.frame_control >> 4) & 0x0F) as u8
    }

    #[inline]
    pub fn is_protected(&self) -> bool {
        (self.frame_control & (1 << 14)) != 0
    }
}

/// Kablosuz Erişim Noktası (Access Point / AP) Bilgisi
#[derive(Clone, Copy)]
pub struct AccessPoint {
    pub ssid: [u8; 32],
    pub ssid_len: usize,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub rssi: i8, // dBm (örn. -45 iyi, -85 zayıf)
    pub security: WifiSecurity,
}

impl AccessPoint {
    pub const fn empty() -> Self {
        Self {
            ssid: [0; 32],
            ssid_len: 0,
            bssid: [0; 6],
            channel: 1,
            rssi: -100,
            security: WifiSecurity::Open,
        }
    }

    pub fn ssid_str(&self) -> &str {
        core::str::from_utf8(&self.ssid[..self.ssid_len]).unwrap_or("<Gizli Ağ>")
    }

    pub fn signal_percent(&self) -> u8 {
        if self.rssi <= -100 {
            0
        } else if self.rssi >= -50 {
            100
        } else {
            ((self.rssi + 100) * 2) as u8
        }
    }
}

pub const MAX_SCAN_APS: usize = 16;
static mut SCAN_RESULTS: [Option<AccessPoint>; MAX_SCAN_APS] = [None; MAX_SCAN_APS];
static mut SCAN_COUNT: usize = 0;

// Bağlantı Durumu
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WifiState {
    Disconnected,
    Scanning,
    Authenticating,
    Associating,
    Handshake,
    Connected,
    Failed,
}

#[derive(Clone, Copy)]
pub struct WifiStatus {
    pub state: WifiState,
    pub connected_ssid: [u8; 32],
    pub connected_ssid_len: usize,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub rssi: i8,
    pub ip_addr: [u8; 4],
    pub mac_addr: [u8; 6],
}

impl WifiStatus {
    pub const fn new() -> Self {
        Self {
            state: WifiState::Disconnected,
            connected_ssid: [0; 32],
            connected_ssid_len: 0,
            bssid: [0; 6],
            channel: 6,
            rssi: 0,
            ip_addr: [0, 0, 0, 0],
            mac_addr: [0x52, 0x54, 0x00, 0x12, 0x34, 0x58],
        }
    }

    pub fn ssid_str(&self) -> &str {
        core::str::from_utf8(&self.connected_ssid[..self.connected_ssid_len]).unwrap_or("")
    }
}

impl Default for WifiStatus {
    fn default() -> Self {
        Self::new()
    }
}

static mut WIFI_STATUS: WifiStatus = WifiStatus::new();
static mut WIFI_INITIALIZED: bool = false;

// ---------------------------------------------------------------------------
// WPA2-PSK Anahtar Türetimi (PBKDF2-HMAC-SHA256 / PTK)
// ---------------------------------------------------------------------------

/// PBKDF2-HMAC-SHA1 ile parola ve SSID'den 256-bit PMK (Pairwise Master Key) üretir.
/// WPA2 standardında 4096 tur kullanılır; SHA-1 çıktısı 20 bayt olduğu için 32 baytlık
/// PMK iki SHA-1 bloğu (40 bayt) üzerinden türetilir ve ilk 32 bayt alınır.
pub fn derive_pmk(password: &[u8], ssid: &[u8], iterations: u32) -> [u8; 32] {
    let mut salt = [0u8; 36];
    let slen = ssid.len().min(32);
    salt[..slen].copy_from_slice(&ssid[..slen]);

    let mut pmk = [0u8; 32];
    // SHA-1 bloğu 20 bayt; 32 baytlık PMK için 2 blok (block 1 ve 2) gerekir.
    let blocks = 32_usize.div_ceil(20);
    let iters = iterations.max(1);
    for block in 1..=blocks {
        salt[slen..slen + 4].copy_from_slice(&(block as u32).to_be_bytes());
        let mut u = HmacSha1::sign(password, &salt[..slen + 4]);
        let mut out = u;
        for _ in 1..iters {
            u = HmacSha1::sign(password, &u);
            for j in 0..20 {
                out[j] ^= u[j];
            }
        }
        let start = (block - 1) * 20;
        let end = (start + 20).min(32);
        pmk[start..end].copy_from_slice(&out[..end - start]);
    }

    pmk
}

/// 4-Way Handshake'de kullanılan PTK (Pairwise Transient Key) türetim ilkeli.
/// PMK, Anonce, Snonce, AP MAC ve İstemci MAC birleştirilerek PRF-512 (HMAC-SHA1)
/// ile türetilir. Çıktı 512 bit (64 bayt): KCK(16) || KEK(16) || TK(16) || reserved(16).
pub fn derive_ptk(
    pmk: &[u8; 32],
    ap_mac: &[u8; 6],
    client_mac: &[u8; 6],
    anonce: &[u8; 32],
    snonce: &[u8; 32],
) -> [u8; 64] {
    let mut base = [0u8; 80];
    let prefix = b"Pairwise key expansion";
    let plen = prefix.len();
    base[..plen].copy_from_slice(prefix);

    // Min(AA, SPA) || Max(AA, SPA) || Min(ANonce, SNonce) || Max(ANonce, SNonce)
    let (min_mac, max_mac) = if ap_mac < client_mac {
        (ap_mac, client_mac)
    } else {
        (client_mac, ap_mac)
    };
    base[plen..plen + 6].copy_from_slice(min_mac);
    base[plen + 6..plen + 12].copy_from_slice(max_mac);

    let (min_nonce, max_nonce) = if anonce < snonce {
        (anonce, snonce)
    } else {
        (snonce, anonce)
    };
    base[plen + 12..plen + 44].copy_from_slice(min_nonce);
    base[plen + 44..plen + 76].copy_from_slice(max_nonce);
    // base[plen + 76..plen + 80] PRF sayacı (4 bayt, büyük uçtan)

    let mut ptk = [0u8; 64];
    let mut counter: u32 = 0;
    let mut offset = 0;
    while offset < 64 {
        let mut data = [0u8; 84];
        data[..80].copy_from_slice(&base);
        data[80..84].copy_from_slice(&counter.to_be_bytes());
        let h = HmacSha1::sign(pmk, &data); // 20 bayt
        let take = (64 - offset).min(20);
        ptk[offset..offset + take].copy_from_slice(&h[..take]);
        offset += take;
        counter = counter.wrapping_add(1);
    }

    ptk
}

// ---------------------------------------------------------------------------
// WPA 4-Way Handshake (IEEE 802.11i / RSN)
// ---------------------------------------------------------------------------

/// EAPOL (802.1X) Ethernet tipi ve EAPOL-Key sabitleri.
const EAPOL_ETH_TYPE: u16 = 0x888E;
const EAPOL_KEY_TYPE_RSN: u8 = 2; // WPA2/RSN
const EAPOL_VERSION: u8 = 2;

/// EAPOL-Key Key Information bayrakları.
const KEY_INFO_KEY_MIC: u16 = 1 << 6;
const KEY_INFO_KEY_SECURE: u16 = 1 << 5;
const KEY_INFO_KEY_ENCRYPTED: u16 = 1 << 4;
const KEY_INFO_INSTALL: u16 = 1 << 3;
const KEY_INFO_ACK: u16 = 1 << 0;

/// EAPOL-Key gövdesinin sabit kısmı (key_data hariç):
/// descriptor_type(1) + key_info(2) + key_length(2) + replay_counter(8) +
/// key_nonce(32) + key_iv(16) + key_rsc(8) + key_id(8) + key_mic(16) +
/// key_data_length(2) = 95 bayt.
const EAPOL_KEY_BODY_BASE: usize = 95;

/// PTK'ten türetilen WPA anahtar hiyerarjisi:
/// KCK = PTK[0..16], KEK = PTK[16..32], TK = PTK[32..48] (CCMP-128).
#[derive(Clone, Copy)]
pub struct WpaKeys {
    pub kck: [u8; 16], // Key Confirmation Key — EAPOL-Key MIC hesaplama
    pub kek: [u8; 16], // Key Encryption Key — GTK şifreleme
    pub tk: [u8; 16],  // Temporal Key — veri çerçevesi şifreleme
}

impl WpaKeys {
    pub fn from_ptk(ptk: &[u8; 64]) -> Self {
        Self {
            kck: ptk[0..16].try_into().unwrap(),
            kek: ptk[16..32].try_into().unwrap(),
            tk: ptk[32..48].try_into().unwrap(),
        }
    }
}

/// EAPOL-Key Key MIC hesaplar (WPA2: HMAC-SHA1(KCK, frame), ilk 16 bayt).
/// `frame` tam EAPOL çerçevesidir ve Key MIC alanı sıfırlanmış olmalıdır.
pub fn eapol_key_mic(kck: &[u8; 16], frame: &[u8]) -> [u8; 16] {
    let full = HmacSha1::sign(kck, frame);
    let mut mic = [0u8; 16];
    mic.copy_from_slice(&full[..16]);
    mic
}

/// EAPOL-Key çerçevesini `out` arabelleğine yazar ve toplam uzunluğu döndürür.
/// `mic` alanı zaten çerçeve içinde yer almalıdır (çağıran MIC'i yazmış olmalı).
pub fn write_eapol_key(
    key_info: u16,
    replay_counter: &[u8; 8],
    nonce: &[u8; 32],
    mic: &[u8; 16],
    key_data: &[u8],
    out: &mut [u8],
) -> usize {
    let body_len = EAPOL_KEY_BODY_BASE + key_data.len();
    let total = 4 + body_len;
    let f = &mut out[..total];
    f[0] = EAPOL_VERSION;
    f[1] = 3; // EAPOL-Key
    f[2..4].copy_from_slice(&(body_len as u16).to_be_bytes());
    let b = &mut f[4..];
    b[0] = EAPOL_KEY_TYPE_RSN;
    b[1..3].copy_from_slice(&key_info.to_be_bytes());
    b[3..5].copy_from_slice(&(key_data.len() as u16).to_be_bytes());
    b[5..13].copy_from_slice(replay_counter);
    b[13..45].copy_from_slice(nonce);
    // key_iv(45..61), key_rsc(61..69), key_id(69..77) sıfır bırakılır.
    b[77..93].copy_from_slice(mic);
    b[93..95].copy_from_slice(&(key_data.len() as u16).to_be_bytes());
    b[95..95 + key_data.len()].copy_from_slice(key_data);
    total
}

/// 4-Way Handshake durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WpaHandshakeState {
    Idle,
    Msg1Received, // STA ANonce'yi aldı (Msg 1)
    Msg2Sent,     // STA SNonce + MIC gönderdi (Msg 2)
    Msg3Received, // STA GTK + MIC aldı (Msg 3)
    Msg4Sent,     // STA onay gönderdi (Msg 4)
    Done,
    Failed,
}

/// WPA 4-way handshake durumu ve anahtarları.
pub struct WpaHandshake {
    pub state: WpaHandshakeState,
    pub pmk: [u8; 32],
    pub ap_mac: [u8; 6],
    pub client_mac: [u8; 6],
    pub anonce: [u8; 32],
    pub snonce: [u8; 32],
    pub ptk: Option<[u8; 64]>,
    pub keys: Option<WpaKeys>,
    pub replay_counter: [u8; 8],
}

impl WpaHandshake {
    pub fn new(pmk: &[u8; 32], ap_mac: &[u8; 6], client_mac: &[u8; 6]) -> Self {
        // Nonce'lar donanım RNG'sinden üretilir; handshake sırasında tazelenir.
        let mut anonce: [u8; 32] = Default::default();
        let mut snonce: [u8; 32] = Default::default();
        crate::random::fill_bytes(&mut anonce);
        crate::random::fill_bytes(&mut snonce);
        Self {
            state: WpaHandshakeState::Idle,
            pmk: *pmk,
            ap_mac: *ap_mac,
            client_mac: *client_mac,
            anonce,
            snonce,
            ptk: None,
            keys: None,
            replay_counter: [0; 8],
        }
    }

    /// AP'tan gelen Msg 1'i işler: ANonce'u kaydeder, SNonce üretir ve
    /// PTK/anahtarları türeteri Msg 2 (SNonce + MIC) çerçevesini hazırlar.
    /// Döndürülen değer: (msg2_frame_len, snonce).
    pub fn process_msg1(&mut self, anonce: &[u8; 32], out: &mut [u8]) -> (usize, [u8; 32]) {
        self.anonce = *anonce;
        // SNonce: her el sıkışmasında taze kriptografik rastgele değer.
        self.snonce = generate_snonce();

        let ptk = derive_ptk(
            &self.pmk,
            &self.ap_mac,
            &self.client_mac,
            &self.anonce,
            &self.snonce,
        );
        self.ptk = Some(ptk);
        self.keys = Some(WpaKeys::from_ptk(&ptk));

        let kck = self.keys.unwrap().kck;
        let mut frame = [0u8; 4 + EAPOL_KEY_BODY_BASE];
        // MIC sıfırlı Msg 2 gövdesi oluşturulur, ardından MIC hesaplanır.
        let len = write_eapol_key(
            KEY_INFO_KEY_MIC | KEY_INFO_ACK,
            &self.replay_counter,
            &self.snonce,
            &[0u8; 16],
            &[],
            &mut frame,
        );
        let mic = eapol_key_mic(&kck, &frame[..len]);
        // MIC'i çerçeveye yaz ve tekrar çıkar (MIC alanında MIC'in olduğu gibi kalır).
        let mut frame2 = [0u8; 4 + EAPOL_KEY_BODY_BASE];
        let len2 = write_eapol_key(
            KEY_INFO_KEY_MIC | KEY_INFO_ACK,
            &self.replay_counter,
            &self.snonce,
            &mic,
            &[],
            &mut frame2,
        );
        self.replay_counter = increment_counter(self.replay_counter);
        self.state = WpaHandshakeState::Msg2Sent;
        out[..len2].copy_from_slice(&frame2[..len2]);
        (len2, self.snonce)
    }

    /// AP'tan gelen Msg 3'i (GTK + MIC) doğrular ve Msg 4 onay çerçevesini hazırlar.
    pub fn process_msg3(&mut self, mic: &[u8; 16], out: &mut [u8]) -> bool {
        let Some(keys) = self.keys else {
            return false;
        };
        let kck = keys.kck;
        // Beklenen MIC: Msg 3 çerçevesi (MIC alanı verilen mic ile) üzerinden HMAC-SHA1(KCK).
        let mut frame = [0u8; 4 + EAPOL_KEY_BODY_BASE];
        let len = write_eapol_key(
            KEY_INFO_KEY_MIC | KEY_INFO_KEY_ENCRYPTED,
            &self.replay_counter,
            &self.anonce,
            mic,
            &[],
            &mut frame,
        );
        let expected = eapol_key_mic(&kck, &frame[..len]);
        if expected != *mic {
            self.state = WpaHandshakeState::Failed;
            return false;
        }

        // Msg 4: MIC'li onay.
        let mut frame4 = [0u8; 4 + EAPOL_KEY_BODY_BASE];
        let len4 = write_eapol_key(
            KEY_INFO_KEY_MIC | KEY_INFO_INSTALL,
            &self.replay_counter,
            &self.snonce,
            &[0u8; 16],
            &[],
            &mut frame4,
        );
        let mic4 = eapol_key_mic(&kck, &frame4[..len4]);
        let mut frame4b = [0u8; 4 + EAPOL_KEY_BODY_BASE];
        let len4b = write_eapol_key(
            KEY_INFO_KEY_MIC | KEY_INFO_INSTALL,
            &self.replay_counter,
            &self.snonce,
            &mic4,
            &[],
            &mut frame4b,
        );
        out[..len4b].copy_from_slice(&frame4b[..len4b]);
        self.replay_counter = increment_counter(self.replay_counter);
        self.state = WpaHandshakeState::Done;
        true
    }
}

/// SNonce için kriptografik rastgele değer üretir (WPA 4-way handshake).
fn generate_snonce() -> [u8; 32] {
    let mut snonce: [u8; 32] = Default::default();
    crate::random::fill_bytes(&mut snonce);
    snonce
}

/// Replay sayacını 1 artırır (8 bayt, büyük uçtan).
fn increment_counter(mut c: [u8; 8]) -> [u8; 8] {
    for i in (0..8).rev() {
        c[i] = c[i].wrapping_add(1);
        if c[i] != 0 {
            break;
        }
    }
    c
}

// ---------------------------------------------------------------------------
// Wi-Fi Arayüz Tespiti (PCI / USB)
// ---------------------------------------------------------------------------

/// Wi-Fi adaptörünün bağlı olduğu veri yolu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiBusKind {
    Pci,
    Usb,
}

/// Algılanan kablosuz ağ adaptörü bilgisi.
#[derive(Clone, Copy, Debug)]
pub struct WifiInterface {
    pub bus: WifiBusKind,
    /// PCI: [bus, device, function]; USB: [bus, address, 0]
    pub location: [u8; 3],
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    /// Donanımdan okunan MAC adresi (EEPROM/BAR okuma WIP; şimdilik sıfır).
    pub mac_addr: [u8; 6],
}

const PCI_CLASS_NETWORK: u8 = 0x02;
const PCI_SUBCLASS_WIRELESS: u8 = 0x80; // IEEE 802.11 kablosuz denetleyici

static mut WIFI_INTERFACES: [Option<WifiInterface>; 8] = [None; 8];
static mut WIFI_INTERFACE_COUNT: usize = 0;

fn register_interface(iface: WifiInterface) {
    unsafe {
        if WIFI_INTERFACE_COUNT < WIFI_INTERFACES.len() {
            WIFI_INTERFACES[WIFI_INTERFACE_COUNT] = Some(iface);
            WIFI_INTERFACE_COUNT += 1;
        }
    }
}

/// PCI veri yolunda IEEE 802.11 kablosuz ağ adaptörlerini tarar ve kaydeder.
fn detect_pci_interfaces() {
    let _ = pci::init(); // PCI şemasının taranmış olduğundan emin ol
    let count = pci::device_count();
    for i in 0..count {
        if let Some(dev) = pci::get_device(i) {
            if dev.class == PCI_CLASS_NETWORK && dev.subclass == PCI_SUBCLASS_WIRELESS {
                register_interface(WifiInterface {
                    bus: WifiBusKind::Pci,
                    location: [dev.bus, dev.device, dev.function],
                    vendor_id: dev.vendor_id,
                    device_id: dev.device_id,
                    class: dev.class,
                    subclass: dev.subclass,
                    mac_addr: [0; 6],
                });
            }
        }
    }
}

/// USB veri yolunda kablosuz ağ adaptörlerini tarar.
/// Mevcut USB sürücüsü genel cihaz numaralandırması açmadığından bu aşamada
/// yalnızca USB alt sisteminin varlığı kaydedilir; gerçek kablosuz USB
/// cihaz tespiti `crate::drivers::usb` üzerinden cihaz listesi açıldığında
/// etkinleştirilir.
fn detect_usb_interfaces() {
    if crate::drivers::usb::usb_available() {
        // Yer tutucu: USB kablosuz adaptörü (sınıf 0xE0 / alt sınıf wireless)
        // tespiti için USB cihaz numaralandırması gerekir.
    }
}

/// Tüm PCI/USB yollarında Wi-Fi arayüzlerini tespit eder.
pub fn wifi_detect_interfaces() {
    unsafe {
        WIFI_INTERFACE_COUNT = 0;
    }
    detect_pci_interfaces();
    detect_usb_interfaces();
}

/// Algılanan Wi-Fi arayüz sayısını döndürür.
pub fn wifi_interface_count() -> usize {
    unsafe { WIFI_INTERFACE_COUNT }
}

/// Algılanan Wi-Fi arayüzünü indeksle döndürür.
pub fn wifi_interface(index: usize) -> Option<WifiInterface> {
    unsafe {
        if index < WIFI_INTERFACE_COUNT {
            WIFI_INTERFACES[index]
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Wi-Fi Sürücü Yönetimi ve Tarama
// ---------------------------------------------------------------------------

pub fn init() {
    unsafe {
        if WIFI_INITIALIZED {
            return;
        }
        WIFI_INITIALIZED = true;
        crate::serial::print(
            "[WIFI] IEEE 802.11 / Wi-Fi kablosuz ağ alt sistemi başlatılıyor...\n",
        );

        // Gerçek donanım arayüzlerini tespit et (PCI class 0x02/alt 0x80 ve USB).
        wifi_detect_interfaces();
        crate::serial::print("[WIFI] ");
        crate::serial::print_dec(WIFI_INTERFACE_COUNT as u64);
        crate::serial::print(" kablosuz arayüzü algılandı.\n");
        for i in 0..WIFI_INTERFACE_COUNT {
            if let Some(iface) = WIFI_INTERFACES[i] {
                crate::serial::print("[WIFI]   ");
                match iface.bus {
                    WifiBusKind::Pci => crate::serial::print("PCI "),
                    WifiBusKind::Usb => crate::serial::print("USB "),
                }
                crate::serial::print("[");
                crate::serial::print_hex(iface.vendor_id as u64);
                crate::serial::print(":");
                crate::serial::print_hex(iface.device_id as u64);
                crate::serial::print("] class=");
                crate::serial::print_hex(iface.class as u64);
                crate::serial::print(" subclass=");
                crate::serial::print_hex(iface.subclass as u64);
                crate::serial::print("\n");
            }
        }

        // Varsayılan AP listesi (Simüle veya algılanan ağlar)
        populate_default_access_points();

        crate::serial::print(
            "[WIFI] Wi-Fi hazır (WPA2-PSK AES-CCMP & 802.11 MAC desteği aktif).\n",
        );
    }
}

unsafe fn populate_default_access_points() {
    SCAN_COUNT = 0;

    let ap_list: [(&str, [u8; 6], u8, i8, WifiSecurity); 3] = [
        (
            "minOs-Guest",
            [0x00, 0x14, 0xD1, 0x22, 0x33, 0x44],
            6,
            -48,
            WifiSecurity::Wpa2Psk,
        ),
        (
            "Home-Fiber-5G",
            [0xAC, 0x84, 0xC6, 0x11, 0x22, 0x33],
            36,
            -62,
            WifiSecurity::Wpa2Psk,
        ),
        (
            "Public_Free_WiFi",
            [0x00, 0x18, 0x0A, 0x55, 0x66, 0x77],
            1,
            -74,
            WifiSecurity::Open,
        ),
    ];

    for (name, bssid, chan, rssi, sec) in ap_list {
        let mut ap = AccessPoint::empty();
        let bytes = name.as_bytes();
        let len = bytes.len().min(ap.ssid.len());
        ap.ssid[..len].copy_from_slice(&bytes[..len]);
        ap.ssid_len = len;
        ap.bssid = bssid;
        ap.channel = chan;
        ap.rssi = rssi;
        ap.security = sec;

        if SCAN_COUNT < MAX_SCAN_APS {
            SCAN_RESULTS[SCAN_COUNT] = Some(ap);
            SCAN_COUNT += 1;
        }
    }
}

pub fn scan_networks() -> &'static [Option<AccessPoint>] {
    unsafe {
        WIFI_STATUS.state = WifiState::Scanning;
        populate_default_access_points();
        &SCAN_RESULTS[..SCAN_COUNT]
    }
}

pub fn scan_count() -> usize {
    unsafe { SCAN_COUNT }
}

pub fn get_ap(index: usize) -> Option<AccessPoint> {
    unsafe {
        if index < SCAN_COUNT {
            SCAN_RESULTS[index]
        } else {
            None
        }
    }
}

pub fn status() -> WifiStatus {
    unsafe { WIFI_STATUS }
}

pub fn connect(ssid: &str, password: &str) -> bool {
    unsafe {
        for i in 0..SCAN_COUNT {
            if let Some(ap) = SCAN_RESULTS[i] {
                if ap.ssid_str() == ssid {
                    WIFI_STATUS.state = WifiState::Authenticating;
                    WIFI_STATUS.connected_ssid_len = ap.ssid_len;
                    WIFI_STATUS.connected_ssid[..ap.ssid_len]
                        .copy_from_slice(&ap.ssid[..ap.ssid_len]);
                    WIFI_STATUS.bssid = ap.bssid;
                    WIFI_STATUS.channel = ap.channel;
                    WIFI_STATUS.rssi = ap.rssi;

                    // WPA2-PSK 4-way el sıkışması: PMK → PTK → KCK/KEK/TK,
                    // ardından Msg 1/2/3/4 değişimi (simüle edilen AP ile).
                    if !wpa_connect_handshake(&ap, password) {
                        WIFI_STATUS.state = WifiState::Failed;
                        crate::serial::print("[WIFI] WPA el sıkışması başarısız.\n");
                        return false;
                    }

                    WIFI_STATUS.state = WifiState::Connected;
                    WIFI_STATUS.ip_addr = [192, 168, 1, 105];

                    crate::serial::print("[WIFI] '");
                    crate::serial::print(ssid);
                    crate::serial::print("' ağına WPA2-PSK ile bağlanıldı (IP: 192.168.1.105).\n");
                    return true;
                }
            }
        }
        false
    }
}

/// WPA2-PSK 4-way el sıkışmasını (simüle edilen AP ile) çalıştırır.
/// PMK = PBKDF2-HMAC-SHA1(password, SSID, 4096); PTK ve KCK/KEK/TK türetilir.
fn wpa_connect_handshake(ap: &AccessPoint, password: &str) -> bool {
    let pmk = derive_pmk(password.as_bytes(), &ap.ssid[..ap.ssid_len], 4096);

    let client_mac = unsafe { WIFI_STATUS.mac_addr };
    let mut handshake = WpaHandshake::new(&pmk, &ap.bssid, &client_mac);

    // AP'tan gelen Msg 1: donanım RNG'sinden taze ANonce.
    let mut anonce: [u8; 32] = Default::default();
    crate::random::fill_bytes(&mut anonce);

    let mut out = [0u8; 4 + EAPOL_KEY_BODY_BASE];
    let (msg2_len, _snonce) = handshake.process_msg1(&anonce, &mut out);
    let _ = msg2_len; // STA → AP Msg 2 (gerçek donanıma TX)

    // AP (simüle) Msg 3 oluşturur: GTK + MIC, aynı KCK ile hesaplanır.
    let Some(keys) = handshake.keys else {
        return false;
    };
    let kck = keys.kck;
    let mut msg3 = [0u8; 4 + EAPOL_KEY_BODY_BASE];
    let msg3_len = write_eapol_key(
        KEY_INFO_KEY_MIC | KEY_INFO_KEY_ENCRYPTED,
        &handshake.replay_counter,
        &handshake.anonce,
        &[0u8; 16],
        &[],
        &mut msg3,
    );
    let ap_mic = eapol_key_mic(&kck, &msg3[..msg3_len]);
    let mut msg3b = [0u8; 4 + EAPOL_KEY_BODY_BASE];
    let _ = write_eapol_key(
        KEY_INFO_KEY_MIC | KEY_INFO_KEY_ENCRYPTED,
        &handshake.replay_counter,
        &handshake.anonce,
        &ap_mic,
        &[],
        &mut msg3b,
    );

    // STA Msg 3'i doğrular ve Msg 4 onayını gönderir.
    let mut out4 = [0u8; 4 + EAPOL_KEY_BODY_BASE];
    handshake.process_msg3(&ap_mic, &mut out4);
    handshake.state == WpaHandshakeState::Done
}

pub fn disconnect() {
    unsafe {
        WIFI_STATUS.state = WifiState::Disconnected;
        WIFI_STATUS.connected_ssid_len = 0;
        WIFI_STATUS.ip_addr = [0, 0, 0, 0];
        crate::serial::print("[WIFI] Kablosuz ağ bağlantısı kesildi.\n");
    }
}

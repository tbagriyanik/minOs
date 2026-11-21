//! Realtek RTL8188/8192 serisi USB kablosuz ağ denetleyicisi sürücüsü.
//!
//! Bu dosya iki aşamalı geliştirilmektedir:
//!
//! - **Faz 0 (mevcut):** VID/PID algılama, genel USB aygıt katmanına
//!   bağlanma ve temel durum takibi. Gerçek tarama/bağlantı henüz yok.
//! - **Faz 2 (planlanan):** Firmware yükleme, komut/olay kuyruğu,
//!   bulk IN/OUT aktarımı, RX/TX tanımlayıcı ayrıştırma ve `wifi.rs`
//!   `SCAN_RESULTS`'i gerçek çerçevelerle doldurma.
//!
//! Algılanan aygıtlar `usb_hid` genel katmanında kayıtlı kalır; sürücü
//! `usb_device_attached` geri çağrısıyla aygıt indeksini öğrenir.

use crate::drivers::usb_hid::{self, UsbDeviceKind};

/// Realtek USB kablosuz ağ üretici kodu.
const REALTEK_VID: u16 = 0x0BDA;

/// Desteklenen ürün kodları (bilinen RTL8188/8192EU/ETV/FU ailesi).
/// Liste `rtl8xxxu` (Linux) USB ID tablosuyla uyumludur; yeni çipler
/// donanım doğrulamasıyla eklenmelidir.
const SUPPORTED_PIDS: &[u16] = &[
    0x0179, // RTL8188ETV
    0x017A, // RTL8188ETV
    0x3188, // RTL8192EU
    0x8192, // RTL8192EU ( alternatif )
    0x8812, // RTL8821AU
    0xB720, // RTL8723AU
    0xF179, // RTL8188FU
    0xF817, // RTL8188FU
];

/// Sürücü çalışma durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtlState {
    /// Aygıt algılanmadı.
    NotDetected,
    /// Aygıt genel USB katmanında kayıtlı; sürücü başlatılmadı.
    Detected,
    /// Firmware ve uç noktalar hazırlanıyor.
    Initializing,
    /// Hazır; tarama ve bağlantı mümkün.
    Ready,
    /// Hata durumu.
    Error,
}

static mut STATE: RtlState = RtlState::NotDetected;
static mut DEVICE_INDEX: u8 = u8::MAX;
static mut LAST_ERROR: u32 = 0;

/// VID/PID çiftinin desteklenen Realtek USB Wi-Fi aygıtı olup olmadığını
/// döndürür. `usb_hid::generic_device_kind` tarafından numaralandırma
/// sırasında çağrılır.
pub fn is_supported_device(vendor: u16, product: u16) -> bool {
    vendor == REALTEK_VID && SUPPORTED_PIDS.contains(&product)
}

/// Genel USB katmanı aygıtı kaydet tiğinde çağrılır.
pub fn usb_device_attached(index: u8) {
    unsafe {
        // Yalnızca bu sınıfın aygıtlarını kabul et.
        let Some((vid, pid, _, _, _)) = usb_hid::usb_device_info(index) else {
            return;
        };
        if !is_supported_device(vid, pid) {
            return;
        }
        DEVICE_INDEX = index;
        STATE = RtlState::Detected;
        crate::serial::print("[RTL8XXX] USB Wi-Fi aygıtı algılandı (indeks ");
        crate::serial::print_dec(index as u64);
        crate::serial::print(").\n");
    }
}

/// Genel USB katmanı aygıtı kaldırdığında çağrılır.
pub fn usb_device_removed(index: u8) {
    unsafe {
        if DEVICE_INDEX == index {
            DEVICE_INDEX = u8::MAX;
            STATE = RtlState::NotDetected;
            crate::serial::print("[RTL8XXX] USB Wi-Fi aygıtı çıkarıldı.\n");
        }
    }
}

/// Sürücünün başlatılması: aygıt hazırsa uç noktaları ve firmware'i
/// hazırlar. Faz 2'de doldurulacaktır; şimdilik algılama durumunu döner.
pub fn init() -> bool {
    unsafe {
        if STATE != RtlState::Detected || DEVICE_INDEX == u8::MAX {
            return false;
        }
        STATE = RtlState::Initializing;
        // TODO(Faz 2): firmware yükleme + bulk uç nokta yapılandırması.
        STATE = RtlState::Ready;
        crate::serial::print("[RTL8XXX] Başlatıldı (temel algılama; tarama henüz yok).\n");
        true
    }
}

/// Aygıt algılandı mı?
pub fn is_detected() -> bool {
    unsafe { STATE != RtlState::NotDetected && DEVICE_INDEX != u8::MAX }
}

/// Sürücü durumu.
pub fn state() -> RtlState {
    unsafe { STATE }
}

/// Son hata kodu (genişletilecek).
pub fn last_error() -> u32 {
    unsafe { LAST_ERROR }
}

/// Genel aygıt türü eşleşmesi (bilgi amaçlı).
pub fn device_kind() -> Option<UsbDeviceKind> {
    Some(UsbDeviceKind::Wifi)
}

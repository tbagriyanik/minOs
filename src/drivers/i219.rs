#![allow(dead_code, unused_variables, static_mut_refs)]

//! Intel I219 / I220 / I225 / I226 Ethernet kimlik tablosu.
//!
//! Bu denetleyiciler (cloverpoint ve rainbow aileleri) e1000/e1000e kayıt
//! düzenini kullanır: RDBAL=0x2800, TDBAL=0x3800, RAL0=0x5400, CTRL=0x0000,
//! RCTL=0x0100, TCTL=0x0400, EERD=0x0014. Veri yolu bu nedenle
//! `drivers::e1000` sürücüsünde uygulanır; bu modül yalnızca aygıt kimliği
//! eşlemesini ve arayüz adlarını tutar.
//!
//! Daha önce bu modül kendi (hiç başlatılmayan) halka implementasyonunu
//! taşıyor ve boot yolundan hiç çağrılmıyordu; bu yüzden gerçek donanımda
//! Intel I219 kartları hiç algılanmıyordu. Artık kimlik tablosu `e1000`
//! PCI taraması tarafından doğrudan kullanılır.

use crate::drivers::e1000;

/// Intel PCI vendor kimliği.
pub const INTEL_VENDOR: u16 = 0x8086;

/// I217/I218/I219/I220 (cloverpoint) ve I225/I226 (rainbow) aygıt kimlikleri.
pub const I219_DEVICES: [u32; 24] = [
    0x125C, // I225-LM
    0x125D, // I226-LM / I226-V
    0x152A, // I217-LM
    0x152B, // I217-V
    0x154C, // I218-LM
    0x154D, // I218-V
    0x156F, // I219-V
    0x15B3, // I219-LM (eski revizyon)
    0x15B7, // I219-LM
    0x15B8, // I219-V
    0x15B9, // I219-LM
    0x15BB, // I219-V
    0x15BC, // I219-V
    0x15BD, // I219-V
    0x15BE, // I219-V
    0x15D6, // I219-LM
    0x15D7, // I219-LM
    0x15D8, // I219-V
    0x15DF, // I219-V
    0x15E0, // I219-V
    0x15E3, // I219-VM
    0x15F6, // I219-LM
    0x15F7, // I219-V
    0x15FA, // I220-V
];

/// Verilen aygıt kimliği Intel modern Ethernet ailesine mi ait?
pub fn matches_device(device_id: u32) -> bool {
    I219_DEVICES.contains(&device_id)
}

/// Aygıt sürücü tarafından tanındı mı? (e1000 sürücüsü bu aileyi sürer.)
pub fn is_detected() -> bool {
    e1000::is_ready() && e1000::nic_type() == e1000::NicType::IntelE1000
}

/// Sürücünün hazır olup olmadığı.
pub fn is_ready() -> bool {
    e1000::is_ready()
}

/// MAC adresi (e1000 sürücüsünden okunur).
pub fn mac_address() -> [u8; 6] {
    unsafe { e1000::MAC_ADDR }
}

/// Aygıt tespiti. Asıl tespit `drivers::e1000::init` içinde yapılır; bu
/// işlev yalnızca "Intel ailesi mi" sorusunu yanıtlar.
pub fn detect() -> bool {
    is_detected()
}

/// Başlatma. Bu aile `drivers::e1000` tarafından sürüldüğü için burada ek bir
/// işlem yapılmaz; sürücünün durumu döndürülür.
pub fn init() -> bool {
    e1000::is_ready()
}

/// Sürücü adı (İngilizce).
pub fn name() -> &'static str {
    "Intel I219-V"
}

/// Sürücü adı (Türkçe).
pub fn name_tr() -> &'static str {
    "Intel I219-V Ethernet"
}

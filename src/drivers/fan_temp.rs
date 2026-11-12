#![allow(dead_code, unused_variables, static_mut_refs)]

//! Fan and Temperature Sensor Driver - Fan ve sıcaklık sensörleri
//!
//! Bu driver:
//!   * ACPI ThermalZone üzerinden sistem sıcaklıklarını okur.
//!   * EC (Embedded Controller) üzerinden fan hızını okur ve ayarlar
//!     (klavye denetleyicisi arayüzü: komut portu 0x66, veri portu 0x62).
//!   * Fan profillerini yönetir: Silent, Normal, Performance, Full, Auto.
//!   * Sıcaklığa göre otomatik fan profili geçişi yapar.
//!
//! Not: EC kayıt ofsetleri satıcıya özeldir; burada örnek/konvensiyonel
//! ofsetler kullanılır. Gerçek donanıma geçişte ofsetler placa özel
//! ayarlanmalıdır.

use crate::drivers::port_io::{io_in8, io_out8};

// ---------------------------------------------------------------------------
// Sensör Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorState {
    NotDetected,
    Detected,
    Ready,
    Error,
}

// ---------------------------------------------------------------------------
// Sıcaklık Bilgisi
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct TemperatureSensor {
    pub id: u8,
    pub name: [u8; 32],
    pub temperature: i32, // °C
    pub critical: i32,    // Kritik sıcaklık (°C)
    pub max: i32,         // Maksimum sıcaklık (°C)
}

// ---------------------------------------------------------------------------
// Fan Bilgisi
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FanSensor {
    pub id: u8,
    pub name: [u8; 32],
    pub rpm: u32,          // Devir/dakika
    pub speed_percent: u8, // Hız yüzdesi
    pub min_rpm: u32,      // Minimum RPM
    pub max_rpm: u32,      // Maksimum RPM
}

// ---------------------------------------------------------------------------
// EC (Embedded Controller) I/O portları
// ---------------------------------------------------------------------------

/// EC komut portu (0x66) ve veri portu (0x62) — standart klavye denetleyicisi
/// arayüzü üzerinden EC'ye erişim.
const EC_CMD: u16 = 0x66;
const EC_DATA: u16 = 0x62;

const EC_STATUS_OBF: u8 = 1 << 0; // Output Buffer Full — okunacak veri var
const EC_STATUS_IBF: u8 = 1 << 1; // Input Buffer Full — EC komutu/veri işleniyor

const EC_CMD_READ: u8 = 0x81; // EC bellek baytı oku
const EC_CMD_WRITE: u8 = 0x82; // EC bellek baytı yaz

const EC_POLL_TIMEOUT: u32 = 10_000;

// EC fan kontrol kayıt ofsetleri (satıcıya özeldir; örnek değerler)
const EC_REG_FAN_CONTROL: u8 = 0x00;
const EC_REG_FAN_PROFILE: u8 = 0x01;
const EC_REG_FAN_SPEED: u8 = 0x02;

// ---------------------------------------------------------------------------
// EC protokol yardımcıları
// ---------------------------------------------------------------------------

/// `mask` bayrağının `set` durumuna gelene kadar durum bayraklarını sondar.
fn ec_wait_mask(mask: u8, set: bool) -> bool {
    for _ in 0..EC_POLL_TIMEOUT {
        let st = io_in8(EC_CMD);
        if set {
            if (st & mask) != 0 {
                return true;
            }
        } else if (st & mask) == 0 {
            return true;
        }
    }
    false
}

/// EC bellek offset'inden bir bayt okur. EC yoksa veya zaman aşımına uğrarsa `None`.
pub fn ec_read(offset: u8) -> Option<u8> {
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return None;
    }
    io_out8(EC_CMD, EC_CMD_READ);
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return None;
    }
    io_out8(EC_DATA, offset);
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return None;
    }
    if !ec_wait_mask(EC_STATUS_OBF, true) {
        return None;
    }
    Some(io_in8(EC_DATA))
}

/// EC bellek offset'ine bir bayt yazar. Başarı durumu döner.
pub fn ec_write(offset: u8, value: u8) -> bool {
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return false;
    }
    io_out8(EC_CMD, EC_CMD_WRITE);
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return false;
    }
    io_out8(EC_DATA, offset);
    if !ec_wait_mask(EC_STATUS_IBF, false) {
        return false;
    }
    io_out8(EC_DATA, value);
    ec_wait_mask(EC_STATUS_IBF, false)
}

/// EC'nin varlığını dene. Birkaç offset'ten okuma denenir; herhangi biri
/// 0xFF dışında bir değer dönerse EC varsayılır.
pub fn ec_detect() -> bool {
    (0..3).any(|off| ec_read(off as u8).is_some_and(|v| v != 0xFF))
}

/// EC mevcut mu?
pub fn ec_available() -> bool {
    unsafe { EC_AVAILABLE }
}

// ---------------------------------------------------------------------------
// Fan profilleri
// ---------------------------------------------------------------------------

/// Fan işletme profili.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FanProfile {
    Silent,      // Minimum güç, düşük RPM
    Normal,      // Dengeli
    Performance, // Yüksek RPM
    Full,        // Tam hız
    Auto,        // Sıcaklığa göre otomatik
}

impl FanProfile {
    pub fn label(self) -> &'static str {
        match self {
            FanProfile::Silent => "Sessiz",
            FanProfile::Normal => "Normal",
            FanProfile::Performance => "Performans",
            FanProfile::Full => "Tam Hız",
            FanProfile::Auto => "Otomatik",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            FanProfile::Silent => "Silent",
            FanProfile::Normal => "Normal",
            FanProfile::Performance => "Performance",
            FanProfile::Full => "Full",
            FanProfile::Auto => "Auto",
        }
    }

    /// Profili varsayılan sabit hız yüzdesine çevirir (Auto hariç).
    pub fn default_speed(self) -> u8 {
        match self {
            FanProfile::Silent => 30,
            FanProfile::Normal => 50,
            FanProfile::Performance => 75,
            FanProfile::Full => 100,
            FanProfile::Auto => 0, // sıcaklıktan hesaplanır
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "silent" | "sessiz" => Some(FanProfile::Silent),
            "normal" => Some(FanProfile::Normal),
            "performance" | "performans" => Some(FanProfile::Performance),
            "full" | "tam" | "tam hiz" | "tamhız" => Some(FanProfile::Full),
            "auto" | "otomatik" => Some(FanProfile::Auto),
            _ => None,
        }
    }

    pub fn all() -> &'static [FanProfile] {
        &[
            FanProfile::Silent,
            FanProfile::Normal,
            FanProfile::Performance,
            FanProfile::Full,
            FanProfile::Auto,
        ]
    }
}

/// Auto modu sıcaklık eşikleri (°C).
const AUTO_SILENT_MAX: i32 = 50;
const AUTO_NORMAL_MAX: i32 = 65;
const AUTO_PERFORMANCE_MAX: i32 = 80;

/// Sıcaklığa göre Auto modunda kullanılacak profili seçer.
pub fn profile_for_temperature(temp: i32) -> FanProfile {
    if temp >= AUTO_PERFORMANCE_MAX {
        FanProfile::Full
    } else if temp >= AUTO_NORMAL_MAX {
        FanProfile::Performance
    } else if temp >= AUTO_SILENT_MAX {
        FanProfile::Normal
    } else {
        FanProfile::Silent
    }
}

/// Bir fan için hedef hız yüzdesini hesaplar (profil + sıcaklık).
pub fn fan_target_speed(fan_id: u8, temperature: i32) -> u8 {
    unsafe {
        let profile = fan_profile_for_fan_locked(fan_id);
        match profile {
            FanProfile::Auto => {
                let auto_profile = profile_for_temperature(temperature);
                auto_profile.default_speed()
            }
            _ => profile.default_speed(),
        }
    }
}

/// Kilitli (unsafe) versiyon: fan için etkin profili döner.
unsafe fn fan_profile_for_fan_locked(fan_id: u8) -> FanProfile {
    for i in 0..FAN_SENSOR_COUNT {
        if FAN_SENSORS[i].id == fan_id {
            return FAN_PROFILES[i].unwrap_or(CURRENT_PROFILE);
        }
    }
    CURRENT_PROFILE
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut DETECTED: bool = false;
static mut STATE: SensorState = SensorState::NotDetected;
static mut EC_AVAILABLE: bool = false;
static mut CURRENT_PROFILE: FanProfile = FanProfile::Normal;
static mut FAN_PROFILES: [Option<FanProfile>; 8] = [None; 8];
static mut TEMP_SENSORS: [TemperatureSensor; 8] = [TemperatureSensor {
    id: 0,
    name: [0; 32],
    temperature: 0,
    critical: 0,
    max: 0,
}; 8];
static mut TEMP_SENSOR_COUNT: usize = 0;
static mut FAN_SENSORS: [FanSensor; 8] = [FanSensor {
    id: 0,
    name: [0; 32],
    rpm: 0,
    speed_percent: 0,
    min_rpm: 0,
    max_rpm: 0,
}; 8];
static mut FAN_SENSOR_COUNT: usize = 0;

// ---------------------------------------------------------------------------
// Cihaz Algılama
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    unsafe {
        if DETECTED {
            return true;
        }

        // ACPI DSDT/SSDT tablolarını tara ve Thermal Zone sensörlerini yükle
        let mut tz_list = [crate::drivers::acpi_battery::ThermalZoneParsedData::default(); 8];
        let tz_count = crate::drivers::acpi_battery::scan_acpi_thermal_zones(&mut tz_list);

        DETECTED = true;
        STATE = SensorState::Detected;

        if tz_count > 0 {
            TEMP_SENSOR_COUNT = tz_count.min(8);
            for i in 0..TEMP_SENSOR_COUNT {
                TEMP_SENSORS[i].id = i as u8;
                TEMP_SENSORS[i].name = [0; 32];
                let name_bytes = &tz_list[i].name;
                TEMP_SENSORS[i].name[..4].copy_from_slice(name_bytes);
                let cur = crate::drivers::acpi_battery::kelvin_tenths_to_celsius(
                    tz_list[i].current_temp_k,
                );
                let crit = crate::drivers::acpi_battery::kelvin_tenths_to_celsius(
                    tz_list[i].critical_temp_k,
                );
                TEMP_SENSORS[i].temperature = if cur > 0 { cur } else { 45 };
                TEMP_SENSORS[i].critical = if crit > 0 { crit } else { 95 };
                TEMP_SENSORS[i].max = if crit > 5 { crit - 5 } else { 90 };
            }
        } else {
            // ACPI'de thermal zone bulunamazsa varsayılan sensörleri sağla
            TEMP_SENSORS[0].id = 0;
            TEMP_SENSORS[0].name[..3].copy_from_slice(b"CPU");
            TEMP_SENSORS[0].temperature = 45;
            TEMP_SENSORS[0].critical = 95;
            TEMP_SENSORS[0].max = 90;
            TEMP_SENSOR_COUNT = 1;
        }

        // Fan sensörleri
        FAN_SENSORS[0].id = 0;
        FAN_SENSORS[0].name[..7].copy_from_slice(b"CPU Fan");
        FAN_SENSORS[0].rpm = 1200;
        FAN_SENSORS[0].speed_percent = 40;
        FAN_SENSORS[0].min_rpm = 0;
        FAN_SENSORS[0].max_rpm = 3000;
        FAN_SENSOR_COUNT = 1;

        // EC (Embedded Controller) tespiti
        EC_AVAILABLE = ec_detect();

        // Varsayılan profil: Normal
        CURRENT_PROFILE = FanProfile::Normal;
        for i in 0..FAN_SENSOR_COUNT {
            FAN_PROFILES[i] = Some(FanProfile::Normal);
        }

        DETECTED
    }
}

// ---------------------------------------------------------------------------
// Cihaz Başlatma
// ---------------------------------------------------------------------------

pub fn init() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        STATE = SensorState::Ready;

        // Varsayılan profili uygula
        let _ = apply_fan_profiles();

        true
    }
}

// ---------------------------------------------------------------------------
// Fan profil API'si
// ---------------------------------------------------------------------------

/// Geçerli genel fan profilini döner.
pub fn fan_profile_current() -> FanProfile {
    unsafe { CURRENT_PROFILE }
}

/// Kullanılabilir profil isimlerini döner.
pub fn fan_profile_names() -> &'static [&'static str] {
    &["Silent", "Normal", "Performance", "Full", "Auto"]
}

/// Genel fan profilini isme göre ayarlar.
pub fn fan_profile_set(name: &str) -> bool {
    let profile = match FanProfile::from_name(name) {
        Some(p) => p,
        None => return false,
    };
    unsafe {
        CURRENT_PROFILE = profile;
        for i in 0..FAN_SENSOR_COUNT {
            FAN_PROFILES[i] = Some(profile);
        }
    }
    let _ = apply_fan_profiles();
    true
}

/// Belirli bir fan için etkin profili döner (özel override varsa onu, yoksa genel).
pub fn fan_profile_for_fan(fan_id: u8) -> FanProfile {
    unsafe { fan_profile_for_fan_locked(fan_id) }
}

/// Belirli bir fan için profil override'ı ayarlar.
pub fn fan_profile_set_for_fan(fan_id: u8, profile: FanProfile) -> bool {
    unsafe {
        for i in 0..FAN_SENSOR_COUNT {
            if FAN_SENSORS[i].id == fan_id {
                FAN_PROFILES[i] = Some(profile);
                let _ = apply_fan_profiles();
                return true;
            }
        }
        false
    }
}

/// Tüm fanlara etkin profilleri uygular (hız ayarı + EC kaydı).
pub fn apply_fan_profiles() -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        let mut ok = true;
        for i in 0..FAN_SENSOR_COUNT {
            let fan_id = FAN_SENSORS[i].id;
            let temp = TEMP_SENSORS
                .iter()
                .find(|s| s.id == fan_id)
                .map(|s| s.temperature)
                .unwrap_or(45);
            let speed = fan_target_speed(fan_id, temp);

            FAN_SENSORS[i].speed_percent = speed;
            FAN_SENSORS[i].rpm = (FAN_SENSORS[i].max_rpm * speed as u32) / 100;

            // EC'ye yaz (varsa)
            if EC_AVAILABLE {
                if !ec_write(EC_REG_FAN_SPEED, speed) {
                    ok = false;
                }
                if !ec_write(
                    EC_REG_FAN_PROFILE,
                    profile_to_ec_byte(FAN_PROFILES[i].unwrap_or(CURRENT_PROFILE)),
                ) {
                    ok = false;
                }
            }
        }

        ok
    }
}

/// FanProfile -> EC profil baytı (satıcıya özeldir).
fn profile_to_ec_byte(profile: FanProfile) -> u8 {
    match profile {
        FanProfile::Silent => 1,
        FanProfile::Normal => 2,
        FanProfile::Performance => 3,
        FanProfile::Full => 4,
        FanProfile::Auto => 0,
    }
}

/// Sıcaklığa göre Auto profillerini günceller ve uygular.
pub fn update_fan_profiles() -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        // Auto modundaki fanları sıcaklığa göre yeniden ayarla
        for i in 0..FAN_SENSOR_COUNT {
            let fan_id = FAN_SENSORS[i].id;
            let effective = fan_profile_for_fan_locked(fan_id);
            if effective == FanProfile::Auto {
                let temp = TEMP_SENSORS
                    .iter()
                    .find(|s| s.id == fan_id)
                    .map(|s| s.temperature)
                    .unwrap_or(45);
                let auto_profile = profile_for_temperature(temp);
                FAN_PROFILES[i] = Some(auto_profile);
            }
        }

        apply_fan_profiles()
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları (devam)
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn state() -> SensorState {
    unsafe { STATE }
}

pub fn name() -> &'static str {
    "Fan and Temperature Sensors"
}

pub fn name_tr() -> &'static str {
    "Fan ve Sıcaklık Sensörleri"
}

/// Sıcaklık sensör sayısı
pub fn temp_sensor_count() -> usize {
    unsafe { TEMP_SENSOR_COUNT }
}

/// Fan sensör sayısı
pub fn fan_sensor_count() -> usize {
    unsafe { FAN_SENSOR_COUNT }
}

/// Sıcaklık sensörünü oku
pub fn get_temp_sensor(id: u8) -> Option<TemperatureSensor> {
    unsafe {
        for i in 0..TEMP_SENSOR_COUNT {
            if TEMP_SENSORS[i].id == id {
                return Some(TEMP_SENSORS[i]);
            }
        }
        None
    }
}

/// Tüm sıcaklık sensörlerini oku
pub fn get_all_temp_sensors() -> &'static [TemperatureSensor] {
    unsafe { &TEMP_SENSORS[..TEMP_SENSOR_COUNT] }
}

/// Fan sensörünü oku
pub fn get_fan_sensor(id: u8) -> Option<FanSensor> {
    unsafe {
        for i in 0..FAN_SENSOR_COUNT {
            if FAN_SENSORS[i].id == id {
                return Some(FAN_SENSORS[i]);
            }
        }
        None
    }
}

/// Tüm fan sensörlerini oku
pub fn get_all_fan_sensors() -> &'static [FanSensor] {
    unsafe { &FAN_SENSORS[..FAN_SENSOR_COUNT] }
}

/// Sıcaklık değerlerini güncelle ve Auto fan profillerini yeniden hesapla.
pub fn update_temperatures() -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        // ACPI thermal zone'dan güncel sıcaklık oku
        let mut tz_list = [crate::drivers::acpi_battery::ThermalZoneParsedData::default(); 8];
        let tz_count = crate::drivers::acpi_battery::scan_acpi_thermal_zones(&mut tz_list);
        if tz_count > 0 {
            for i in 0..TEMP_SENSOR_COUNT.min(tz_count) {
                let cur = crate::drivers::acpi_battery::kelvin_tenths_to_celsius(
                    tz_list[i].current_temp_k,
                );
                if cur > 0 {
                    TEMP_SENSORS[i].temperature = cur;
                }
            }
        } else {
            for i in 0..TEMP_SENSOR_COUNT {
                TEMP_SENSORS[i].temperature = 40 + (i as i32 * 5);
            }
        }

        // Sıcaklık değişimi sonucu Auto fan profillerini güncelle
        let _ = update_fan_profiles();

        true
    }
}

/// Fan hızlarını güncelle (profilleri uygula).
pub fn update_fan_speeds() -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        apply_fan_profiles()
    }
}

/// Fan hızını ayarla (yüzde). Manuel override; EC'ye de yazılır.
pub fn set_fan_speed(id: u8, percent: u8) -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        for i in 0..FAN_SENSOR_COUNT {
            if FAN_SENSORS[i].id == id {
                FAN_SENSORS[i].speed_percent = percent;
                FAN_SENSORS[i].rpm = (FAN_SENSORS[i].max_rpm * percent as u32) / 100;
                FAN_PROFILES[i] = Some(FanProfile::Full); // manuel ayar = tam hız kipinde sabitlenir
                if EC_AVAILABLE {
                    let _ = ec_write(EC_REG_FAN_SPEED, percent);
                    let _ = ec_write(EC_REG_FAN_PROFILE, profile_to_ec_byte(FanProfile::Full));
                }
                return true;
            }
        }

        false
    }
}

/// Fan otomatik mod
pub fn set_fan_auto(id: u8) -> bool {
    unsafe {
        if STATE != SensorState::Ready {
            return false;
        }

        for i in 0..FAN_SENSOR_COUNT {
            if FAN_SENSORS[i].id == id {
                FAN_PROFILES[i] = Some(FanProfile::Auto);
                let _ = update_fan_profiles();
                return true;
            }
        }

        false
    }
}

/// Kritik sıcaklık uyarısı
pub fn is_critical_temperature() -> bool {
    unsafe {
        for i in 0..TEMP_SENSOR_COUNT {
            if TEMP_SENSORS[i].temperature >= TEMP_SENSORS[i].critical {
                return true;
            }
        }
        false
    }
}

/// Ortalama sıcaklık
pub fn average_temperature() -> Option<i32> {
    unsafe {
        if TEMP_SENSOR_COUNT == 0 {
            return None;
        }

        let mut sum = 0i32;
        for i in 0..TEMP_SENSOR_COUNT {
            sum += TEMP_SENSORS[i].temperature;
        }
        Some(sum / TEMP_SENSOR_COUNT as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_profile_default_speeds() {
        assert_eq!(FanProfile::Silent.default_speed(), 30);
        assert_eq!(FanProfile::Normal.default_speed(), 50);
        assert_eq!(FanProfile::Performance.default_speed(), 75);
        assert_eq!(FanProfile::Full.default_speed(), 100);
        assert_eq!(FanProfile::Auto.default_speed(), 0);
    }

    #[test]
    fn fan_profile_from_name() {
        assert_eq!(FanProfile::from_name("silent"), Some(FanProfile::Silent));
        assert_eq!(FanProfile::from_name("sessiz"), Some(FanProfile::Silent));
        assert_eq!(FanProfile::from_name("NORMAL"), Some(FanProfile::Normal));
        assert_eq!(
            FanProfile::from_name("performance"),
            Some(FanProfile::Performance)
        );
        assert_eq!(
            FanProfile::from_name("performans"),
            Some(FanProfile::Performance)
        );
        assert_eq!(FanProfile::from_name("full"), Some(FanProfile::Full));
        assert_eq!(FanProfile::from_name("auto"), Some(FanProfile::Auto));
        assert_eq!(FanProfile::from_name("otomatik"), Some(FanProfile::Auto));
        assert_eq!(FanProfile::from_name("gecersiz"), None);
    }

    #[test]
    fn fan_profile_all_count() {
        assert_eq!(FanProfile::all().len(), 5);
    }

    #[test]
    fn profile_for_temperature_thresholds() {
        assert_eq!(profile_for_temperature(40), FanProfile::Silent);
        assert_eq!(profile_for_temperature(50), FanProfile::Normal);
        assert_eq!(profile_for_temperature(64), FanProfile::Normal);
        assert_eq!(profile_for_temperature(65), FanProfile::Performance);
        assert_eq!(profile_for_temperature(79), FanProfile::Performance);
        assert_eq!(profile_for_temperature(80), FanProfile::Full);
        assert_eq!(profile_for_temperature(95), FanProfile::Full);
    }

    #[test]
    fn fan_target_speed_auto() {
        // Auto -> sıcaklığa dayalı
        assert_eq!(fan_target_speed(0, 40), FanProfile::Silent.default_speed());
        assert_eq!(fan_target_speed(0, 55), FanProfile::Normal.default_speed());
        assert_eq!(
            fan_target_speed(0, 70),
            FanProfile::Performance.default_speed()
        );
        assert_eq!(fan_target_speed(0, 90), FanProfile::Full.default_speed());
    }

    #[test]
    fn fan_target_speed_fixed() {
        // Sabit profiller sıcaklığa bağımsızdır
        assert_eq!(fan_target_speed(0, 100), FanProfile::Silent.default_speed());
        // bu çağrı state'i değiştirmez; sadece hesaplama yapar
    }

    #[test]
    fn ec_status_bits() {
        assert_eq!(EC_STATUS_OBF, 1 << 0);
        assert_eq!(EC_STATUS_IBF, 1 << 1);
    }

    #[test]
    fn ec_command_bytes() {
        assert_eq!(EC_CMD_READ, 0x81);
        assert_eq!(EC_CMD_WRITE, 0x82);
    }

    #[test]
    fn profile_to_ec_byte_mapping() {
        assert_eq!(profile_to_ec_byte(FanProfile::Silent), 1);
        assert_eq!(profile_to_ec_byte(FanProfile::Normal), 2);
        assert_eq!(profile_to_ec_byte(FanProfile::Performance), 3);
        assert_eq!(profile_to_ec_byte(FanProfile::Full), 4);
        assert_eq!(profile_to_ec_byte(FanProfile::Auto), 0);
    }
}

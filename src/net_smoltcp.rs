//! smoltcp tabanlı ağ yapılandırması kaldırıldı; çekirdek ağ yığını
//! (`drivers/netstack.rs`) sıfır harici bağımlılıkla çalışır.
//!
//! Bu modül yalnızca protokol katmanlarının adlarını/sabitlerini tanımlayan
//! bir referanstır; cihaz G/Ç işlemleri NIC sürücülerinde kalır.

pub const HTTP_PORT: u16 = 80;
pub const HTTPS_PORT: u16 = 443;
pub const DNS_PORT: u16 = 53;

pub struct Protocols;
impl Protocols {
    pub const fn enabled() -> bool {
        true
    }
    pub const fn summary() -> &'static str {
        "DHCP IPv4/IPv6 TCP UDP DNS HTTP"
    }
}

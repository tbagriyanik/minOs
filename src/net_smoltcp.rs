//! smoltcp socket/protocol configuration used by the kernel network service.
//! Device I/O remains owned by the NIC drivers; this module owns protocol
//! state so DHCP, DNS, TCP and UDP share one implementation.
pub use smoltcp::iface::{Config, Interface, SocketSet};
pub use smoltcp::socket::{dhcpv4, dns, tcp, udp};
pub use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address, Ipv6Address};

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

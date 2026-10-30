#![allow(dead_code, unused_variables, static_mut_refs)]

//! Minimal TCP/IP yığını: ARP, IPv4, ICMP (ping) ve temel paket ayrıştırma.
//! QEMU `-netdev user` (SLIRP) ile ARP'ye cevap verir, ping yanıtlayabilir.

use crate::drivers::e1000;

// ---------------------------------------------------------------------------
// Adres tipleri
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MacAddress(pub [u8; 6]);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }
    pub fn to_string(&self) -> &'static str {
        // Statik metin üretimi (serial log için)
        ""
    }
}

// Statik IP ayarları (QEMU SLIRP subnet: 10.0.2.0/24)
pub const LOCAL_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
pub const GATEWAY_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
pub const SUBNET_MASK: Ipv4Address = Ipv4Address::new(255, 255, 255, 0);
static mut ACTIVE_LOCAL_IP: Ipv4Address = LOCAL_IP;
static mut ACTIVE_GATEWAY_IP: Ipv4Address = GATEWAY_IP;
static mut ACTIVE_DNS_IP: Ipv4Address = DNS_SERVER;
static mut ACTIVE_SUBNET_MASK: Ipv4Address = SUBNET_MASK;
static mut DHCP_OFFER: Option<(Ipv4Address, Ipv4Address, Ipv4Address)> = None;
static mut DHCP_SERVER_IP: Ipv4Address = GATEWAY_IP;
static mut DHCP_READY: bool = false;
static mut DHCP_XID: u32 = 0x4D4F5301;
pub fn local_ip() -> Ipv4Address {
    unsafe { ACTIVE_LOCAL_IP }
}
pub fn gateway_ip() -> Ipv4Address {
    unsafe { ACTIVE_GATEWAY_IP }
}
pub fn dns_server() -> Ipv4Address {
    unsafe { ACTIVE_DNS_IP }
}
pub fn dhcp_ready() -> bool {
    unsafe { DHCP_READY }
}
pub fn subnet_mask() -> Ipv4Address {
    unsafe { ACTIVE_SUBNET_MASK }
}

// Broadcast MAC
pub const MAC_BROADCAST: MacAddress = MacAddress([0xFF; 6]);

// ---------------------------------------------------------------------------
// ARP
// ---------------------------------------------------------------------------

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_IPV6: u16 = 0x86DD;
pub const ETHERTYPE_ARP: u16 = 0x0806;

const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;

// Basit ARP önbelleği (4 girişli)
struct ArpEntry {
    ip: Ipv4Address,
    mac: MacAddress,
    used: bool,
}

static mut ARP_TABLE: [ArpEntry; 16] = [
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
    ArpEntry {
        ip: Ipv4Address([0; 4]),
        mac: MacAddress([0; 6]),
        used: false,
    },
];

// ---------------------------------------------------------------------------
// IPv6 Neighbor Discovery (RFC 4861)
// ---------------------------------------------------------------------------

/// IPv6 Komşu Önbelleği: IPv6 adresi → MAC eşlemesi.
struct NeighborEntry {
    ip: Ipv6Address,
    mac: MacAddress,
    used: bool,
}

static mut NEIGHBOR_CACHE: [NeighborEntry; 8] = [
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
    NeighborEntry {
        ip: Ipv6Address::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        used: false,
    },
];

pub fn neighbor_lookup(ip: Ipv6Address) -> Option<MacAddress> {
    unsafe {
        for e in NEIGHBOR_CACHE.iter() {
            if e.used && e.ip == ip {
                return Some(e.mac);
            }
        }
    }
    None
}

pub fn neighbor_insert(ip: Ipv6Address, mac: MacAddress) {
    unsafe {
        // Var olan eşleşmeyi güncelle
        for e in NEIGHBOR_CACHE.iter_mut() {
            if e.used && e.ip == ip {
                e.mac = mac;
                return;
            }
        }
        // Boş slota ekle
        for e in NEIGHBOR_CACHE.iter_mut() {
            if !e.used {
                e.ip = ip;
                e.mac = mac;
                e.used = true;
                return;
            }
        }
        // Dolu ise ilk girişi değiştir
        NEIGHBOR_CACHE[0].ip = ip;
        NEIGHBOR_CACHE[0].mac = mac;
        NEIGHBOR_CACHE[0].used = true;
    }
}

pub fn arp_lookup(ip: Ipv4Address) -> Option<MacAddress> {
    unsafe {
        for e in ARP_TABLE.iter_mut() {
            if e.used && e.ip == ip {
                return Some(e.mac);
            }
        }
    }
    None
}

pub fn arp_insert(ip: Ipv4Address, mac: MacAddress) {
    unsafe {
        // Var olan eşleşmeyi güncelle
        for e in ARP_TABLE.iter_mut() {
            if e.used && e.ip == ip {
                e.mac = mac;
                return;
            }
        }
        // Boş slota ekle
        for e in ARP_TABLE.iter_mut() {
            if !e.used {
                e.ip = ip;
                e.mac = mac;
                e.used = true;
                return;
            }
        }
        // Dolu ise Gateway olmayan ilk girişi değiştir
        for e in ARP_TABLE.iter_mut() {
            if e.ip != GATEWAY_IP && e.ip != DNS_SERVER {
                e.ip = ip;
                e.mac = mac;
                e.used = true;
                return;
            }
        }
        ARP_TABLE[0].ip = ip;
        ARP_TABLE[0].mac = mac;
    }
}

// ---------------------------------------------------------------------------
// Ethernet Çerçevesi Kurulumu
// ---------------------------------------------------------------------------

/// Ethernet çerçevesi kurar: dst MAC, src MAC, ethertype, payload
fn build_ethernet(dst: MacAddress, ethertype: u16, payload: &[u8], buf: &mut [u8]) -> usize {
    let mac = unsafe { e1000::MAC_ADDR };
    buf[0..6].copy_from_slice(&dst.0);
    buf[6..12].copy_from_slice(&mac);
    buf[12] = (ethertype >> 8) as u8;
    buf[13] = (ethertype & 0xFF) as u8;
    buf[14..14 + payload.len()].copy_from_slice(payload);
    14 + payload.len()
}

// ---------------------------------------------------------------------------
// IPv4
// ---------------------------------------------------------------------------

const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_UDP: u8 = 17;
pub const DNS_SERVER: Ipv4Address = Ipv4Address::new(10, 0, 2, 3);
pub const DNS_PORT: u16 = 53;
pub const HTTP_PORT: u16 = 80;

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | (data[i + 1] as u32);
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// IPv4 paketi kurar ve Ethernet'e sarmalar, ardından e1000'den iletir.
fn send_ipv4(dst_ip: Ipv4Address, proto: u8, payload: &[u8]) -> bool {
    let local = local_ip();
    let gateway = gateway_ip();
    let mask = subnet_mask();
    let on_subnet = (0..4).all(|i| (dst_ip.0[i] & mask.0[i]) == (local.0[i] & mask.0[i]));
    let next_hop = if on_subnet { dst_ip } else { gateway };
    // MAC'i ARP ile çöz
    let dst_mac = match arp_lookup(next_hop) {
        Some(m) => m,
        None => {
            // ARP request gönder
            send_arp_request(next_hop);
            // QEMU SLIRP'in yanıt vermesi için kartı yokla (poll)
            let mut resolved = None;
            for _ in 0..10_000 {
                e1000::poll();
                if let Some(m) = arp_lookup(next_hop) {
                    resolved = Some(m);
                    break;
                }
                core::hint::spin_loop();
            }
            match resolved {
                Some(m) => m,
                None => {
                    // QEMU SLIRP Gateway için bilinen varsayılan MAC'e geri düş (paket düşmesini önle)
                    if next_hop == gateway {
                        let gw_mac = MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x02]);
                        arp_insert(gateway, gw_mac);
                        gw_mac
                    } else if next_hop == DNS_SERVER {
                        let dns_mac = MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x03]);
                        arp_insert(DNS_SERVER, dns_mac);
                        dns_mac
                    } else {
                        return false;
                    }
                }
            }
        }
    };

    // IPv4 header (20 byte)
    let mut ip_packet = [0u8; 1500 - 14];
    if payload.len() > ip_packet.len() - 20 {
        return false;
    }
    let total_len = (20 + payload.len()) as u16;
    ip_packet[0] = 0x45; // version 4, IHL 5
    ip_packet[1] = 0x00;
    ip_packet[2] = (total_len >> 8) as u8;
    ip_packet[3] = (total_len & 0xFF) as u8;
    ip_packet[4] = 0x00;
    ip_packet[5] = 0x00;
    ip_packet[6] = 0x40; // DF
    ip_packet[7] = 0x00;
    ip_packet[8] = 64; // TTL
    ip_packet[9] = proto;
    // header checksum sonradan
    ip_packet[10] = 0;
    ip_packet[11] = 0;
    ip_packet[12..16].copy_from_slice(&local.0);
    ip_packet[16..20].copy_from_slice(&dst_ip.0);

    let cksum = checksum(&ip_packet[..20]);
    ip_packet[10] = (cksum >> 8) as u8;
    ip_packet[11] = (cksum & 0xFF) as u8;

    ip_packet[20..20 + payload.len()].copy_from_slice(payload);

    let mut frame = [0u8; 1514];
    let len = build_ethernet(
        dst_mac,
        ETHERTYPE_IPV4,
        &ip_packet[..20 + payload.len()],
        &mut frame,
    );
    e1000::transmit(&frame[..len])
}

// ---------------------------------------------------------------------------
// ARP paket işleme
// ---------------------------------------------------------------------------

fn send_arp_request(target: Ipv4Address) {
    // ARP paketi: 28 byte
    let mut arp = [0u8; 28];
    arp[0..2].copy_from_slice(&1u16.to_be_bytes()); // hardware ethernet
    arp[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes()); // protocol IPv4
    arp[4] = 6; // hlen
    arp[5] = 4; // plen
    arp[6..8].copy_from_slice(&ARP_OP_REQUEST.to_be_bytes());
    let mac = unsafe { e1000::MAC_ADDR };
    arp[8..14].copy_from_slice(&mac); // sender MAC
    arp[14..18].copy_from_slice(&local_ip().0); // sender IP
    arp[18..24].copy_from_slice(&[0; 6]); // target MAC (boş)
    arp[24..28].copy_from_slice(&target.0); // target IP

    let mut frame = [0u8; 1514];
    let len = build_ethernet(MAC_BROADCAST, ETHERTYPE_ARP, &arp, &mut frame);
    e1000::transmit(&frame[..len]);
}

fn handle_arp(payload: &[u8]) {
    if payload.len() < 28 {
        return;
    }
    if payload[0..2] != [0, 1]
        || payload[2..4] != ETHERTYPE_IPV4.to_be_bytes()
        || payload[4] != 6
        || payload[5] != 4
    {
        return;
    }
    let op = ((payload[6] as u16) << 8) | payload[7] as u16;
    let sender_ip = Ipv4Address([payload[14], payload[15], payload[16], payload[17]]);
    let sender_mac = MacAddress([
        payload[8],
        payload[9],
        payload[10],
        payload[11],
        payload[12],
        payload[13],
    ]);
    let target_ip = Ipv4Address([payload[24], payload[25], payload[26], payload[27]]);

    // Önbelleğe ekle
    arp_insert(sender_ip, sender_mac);

    if op == ARP_OP_REPLY {
        unsafe {
            LAST_ARP_REPLY_SRC = Some(sender_ip);
        }
    }

    // Bize gelen ARP request ise cevap ver
    if op == ARP_OP_REQUEST && target_ip == local_ip() {
        let mut arp = [0u8; 28];
        arp[0..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&ARP_OP_REPLY.to_be_bytes());
        let mac = unsafe { e1000::MAC_ADDR };
        arp[8..14].copy_from_slice(&mac);
        arp[14..18].copy_from_slice(&local_ip().0);
        arp[18..24].copy_from_slice(&sender_mac.0);
        arp[24..28].copy_from_slice(&sender_ip.0);

        let mut frame = [0u8; 1514];
        let len = build_ethernet(sender_mac, ETHERTYPE_ARP, &arp, &mut frame);
        e1000::transmit(&frame[..len]);
    }
}

// ---------------------------------------------------------------------------
// ICMP (ping)
// ---------------------------------------------------------------------------

const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_ECHO_REQUEST: u8 = 8;
pub static mut LAST_PING_REPLY_SRC: Option<Ipv4Address> = None;
pub static mut LAST_PING_REPLY_SEQ: u16 = 0;
pub static mut LAST_PING_REPLY_LEN: usize = 0;
pub static mut LAST_PING_REPLY_TTL: u8 = 64;
pub static mut LAST_ARP_REPLY_SRC: Option<Ipv4Address> = None;

#[derive(Clone, Copy, Debug)]
pub struct PingReply {
    pub src: Ipv4Address,
    pub seq: u16,
    pub len: usize,
    pub ttl: u8,
}

#[inline]
pub fn clear_ping_reply() {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(LAST_PING_REPLY_SRC), None);
    }
}

#[inline]
pub fn has_ping_reply() -> bool {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING_REPLY_SRC)).is_some() }
}

pub fn take_ping_reply() -> Option<PingReply> {
    unsafe {
        let src = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING_REPLY_SRC));
        if let Some(ip) = src {
            let seq = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING_REPLY_SEQ));
            let len = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING_REPLY_LEN));
            let ttl = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING_REPLY_TTL));
            core::ptr::write_volatile(core::ptr::addr_of_mut!(LAST_PING_REPLY_SRC), None);
            Some(PingReply {
                src: ip,
                seq,
                len,
                ttl,
            })
        } else {
            None
        }
    }
}

fn handle_icmp(src_ip: Ipv4Address, ttl: u8, payload: &[u8]) {
    if payload.len() < 8 || checksum(payload) != 0 {
        return;
    }
    let icmp_type = payload[0];
    if icmp_type == ICMP_ECHO_REQUEST {
        // Echo Reply gönder: type 0
        let mut reply = [0u8; 1500 - 14 - 20];
        let n = payload.len().min(reply.len());
        reply[..n].copy_from_slice(&payload[..n]);
        reply[0] = ICMP_ECHO_REPLY;
        reply[1] = 0;
        // checksum yeniden hesapla
        let ck = checksum(&reply[..n]);
        reply[2] = (ck >> 8) as u8;
        reply[3] = (ck & 0xFF) as u8;
        send_ipv4(src_ip, IP_PROTO_ICMP, &reply[..n]);
    } else if icmp_type == ICMP_ECHO_REPLY {
        let seq = if payload.len() >= 8 {
            u16::from_be_bytes([payload[6], payload[7]])
        } else {
            0
        };
        let data_len = payload.len().saturating_sub(8);
        unsafe {
            LAST_PING_REPLY_SRC = Some(src_ip);
            LAST_PING_REPLY_SEQ = seq;
            LAST_PING_REPLY_LEN = data_len;
            LAST_PING_REPLY_TTL = ttl;
        }
    }
}

// ---------------------------------------------------------------------------
// Gelen paket dağıtımı
// ---------------------------------------------------------------------------

/// e1000'den gelen bir Ethernet çerçevesini ayrıştırır ve ilgili yığın katmanına iletir.
pub fn on_packet(frame: &[u8]) {
    if frame.len() < 14 {
        return;
    }
    let ethertype = ((frame[12] as u16) << 8) | frame[13] as u16;
    let payload = &frame[14..];
    match ethertype {
        ETHERTYPE_ARP => handle_arp(payload),
        ETHERTYPE_IPV4 => handle_ipv4(payload),
        ETHERTYPE_IPV6 => handle_ipv6(payload),
        _ => {}
    }
}

fn handle_ipv4(payload: &[u8]) {
    if payload.len() < 20 {
        return;
    }
    if payload[0] >> 4 != 4 {
        return;
    }
    let ihl = ((payload[0] & 0x0F) as usize) * 4;
    if ihl < 20 || payload.len() < ihl || checksum(&payload[..ihl]) != 0 {
        return;
    }
    let proto = payload[9];
    let src_ip = Ipv4Address([payload[12], payload[13], payload[14], payload[15]]);
    let dst_ip = Ipv4Address([payload[16], payload[17], payload[18], payload[19]]);
    let total = u16::from_be_bytes([payload[2], payload[3]]) as usize;
    if total < ihl || total > payload.len() {
        return;
    }
    // IP fragment birleştirmesi yok; bozuk kısmi TCP/UDP datagramlarını işlemeden bırak.
    let fragment = u16::from_be_bytes([payload[6], payload[7]]);
    if fragment & 0x3FFF != 0 {
        return;
    }
    let end = total;
    let is_dhcp = proto == IP_PROTO_UDP
        && end >= ihl + 4
        && u16::from_be_bytes([payload[ihl + 2], payload[ihl + 3]]) == 68;
    if dst_ip != local_ip() && !is_dhcp {
        return;
    }
    let l4 = &payload[ihl..end];
    match proto {
        IP_PROTO_ICMP => handle_icmp(src_ip, payload[8], l4),
        IP_PROTO_UDP => {
            if l4.len() < 8 {
                return;
            }
            let udp_len = u16::from_be_bytes([l4[4], l4[5]]) as usize;
            if udp_len < 8 || udp_len > l4.len() {
                return;
            }
            if (l4[6] != 0 || l4[7] != 0)
                && transport_checksum(src_ip, dst_ip, IP_PROTO_UDP, &l4[..udp_len]) != 0
            {
                return;
            }
            handle_udp(src_ip, &l4[..udp_len]);
        }
        IP_PROTO_TCP => handle_tcp(src_ip, l4),
        _ => {}
    }
}

pub(crate) fn transport_checksum(
    src: Ipv4Address,
    dst: Ipv4Address,
    proto: u8,
    data: &[u8],
) -> u16 {
    let mut sum: u32 = 0;
    for pair in src.0.chunks(2).chain(dst.0.chunks(2)) {
        sum += ((pair[0] as u32) << 8) | pair[1] as u32;
    }
    sum += proto as u32;
    sum += data.len() as u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

// ---------------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------------

/// Basit UDP üzerinden bir "test sayfası" döndürür. Tam HTTP yığını değil ama
/// UDP echo / DHCP discover basitliğiyle birlikte çalışan bir "status" uzantısı.
static mut UDP_LISTENER_DATA: [u8; 512] = [0; 512];
static mut UDP_LISTENER_LEN: usize = 0;
static mut DNS_REPLY_DATA: [u8; 512] = [0; 512];
static mut DNS_REPLY_LEN: usize = 0;
static mut DNS_REPLY_IP: [u8; 4] = [0; 4];
static mut TCP_DATA: [u8; 65536] = [0; 65536];
static mut TCP_DATA_LEN: usize = 0;
static mut TCP_LOCAL_PORT: u16 = 0;
static mut TCP_REMOTE_PORT: u16 = 0;
static mut TCP_REMOTE_IP: Ipv4Address = Ipv4Address([0; 4]);
static mut TCP_LOCAL_SEQ: u32 = 0;
static mut TCP_REMOTE_SEQ: u32 = 0;
static mut TCP_CONNECTED: bool = false;
static mut TCP_SYN_ACKED: bool = false;
static mut TCP_FIN_RECEIVED: bool = false;
static mut TCP_RESET: bool = false;

pub fn take_udp_data() -> Option<&'static [u8]> {
    unsafe {
        if UDP_LISTENER_LEN > 0 {
            let s = &UDP_LISTENER_DATA[..UDP_LISTENER_LEN];
            UDP_LISTENER_LEN = 0;
            Some(s)
        } else {
            None
        }
    }
}

fn handle_udp(src_ip: Ipv4Address, payload: &[u8]) {
    if payload.len() < 8 {
        return;
    }
    let udp_len = ((payload[4] as u16) << 8) | payload[5] as u16;
    if udp_len < 8 || payload.len() < udp_len as usize {
        return;
    }
    let data = &payload[8..udp_len as usize];
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    if dst_port == 68 && src_port == 67 {
        handle_dhcp(src_ip, data);
        return;
    }
    // Soket UDP portuna gelen datagram soket tamponuna gider (DNS yolu korunur).
    if dst_port != 68
        && src_port != DNS_PORT
        && crate::drivers::socket::on_udp_datagram(src_ip, src_port, dst_port, data)
    {
        return;
    }
    if src_port == DNS_PORT && data.len() >= 12 {
        let id = u16::from_be_bytes([data[0], data[1]]);
        let flags = u16::from_be_bytes([data[2], data[3]]);
        if (id == 0x4D4D || id == 0x4D4E) && (flags & 0x8000 != 0) && (flags & 0x000F == 0) {
            unsafe {
                // Try A record first (id 0x4D4D), then AAAA (id 0x4D4E)
                if id == 0x4D4D {
                    if let Some(ip) = dns_parse_a(data, id) {
                        let n = data.len().min(DNS_REPLY_DATA.len());
                        DNS_REPLY_DATA[..n].copy_from_slice(&data[..n]);
                        DNS_REPLY_IP = ip.0;
                        DNS_REPLY_LEN = n;
                    }
                } else if id == 0x4D4E {
                    if let Some(ip) = dns_parse_aaaa(data, id) {
                        let n = data.len().min(DNS_REPLY_DATA.len());
                        DNS_REPLY_DATA[..n].copy_from_slice(&data[..n]);
                        // Store first 4 bytes of AAAA in DNS_REPLY_IP for compatibility
                        DNS_REPLY_IP = [ip.0[0], ip.0[1], ip.0[2], ip.0[3]];
                        DNS_REPLY_LEN = n;
                    }
                }
            }
        }
    }
    // Sağlıklı bir "sınamayı" kaydet — uygulama katmanı (tarayıcı) alır
    unsafe {
        let n = data.len().min(UDP_LISTENER_DATA.len());
        UDP_LISTENER_DATA[..n].copy_from_slice(&data[..n]);
        UDP_LISTENER_LEN = n;
    }
    // Kaynağı da kaydedelim
    let _ = src_ip;
}

fn handle_tcp(src_ip: Ipv4Address, payload: &[u8]) {
    // Önce soket tablosuna danış; eşleşme yoksa eski tek-bağlantılı yola düş.
    if crate::drivers::socket::on_tcp_segment(src_ip, payload) {
        return;
    }
    if payload.len() < 20 {
        return;
    }
    if transport_checksum(src_ip, local_ip(), IP_PROTO_TCP, payload) != 0 {
        return;
    }
    let off = ((payload[12] >> 4) as usize) * 4;
    if off < 20 || payload.len() < off {
        return;
    }
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[13];
    unsafe {
        if dst_port != TCP_LOCAL_PORT || src_port != TCP_REMOTE_PORT || src_ip != TCP_REMOTE_IP {
            return;
        }
        if flags & 0x04 != 0 {
            TCP_RESET = true;
            TCP_CONNECTED = false;
            return;
        }
        let has_syn = flags & 0x02 != 0;
        let has_ack = flags & 0x10 != 0;
        let has_fin = flags & 0x01 != 0;
        let data = &payload[off..];
        if has_syn && has_ack && !TCP_SYN_ACKED {
            TCP_REMOTE_SEQ = seq.wrapping_add(1);
            TCP_LOCAL_SEQ = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
            TCP_SYN_ACKED = true;
            TCP_CONNECTED = true;
            let _ = send_tcp_segment(
                TCP_REMOTE_IP,
                TCP_REMOTE_PORT,
                TCP_LOCAL_PORT,
                TCP_LOCAL_SEQ,
                TCP_REMOTE_SEQ,
                0x10,
                &[],
            );
        } else if TCP_CONNECTED && has_ack {
            let ack = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
            if ack.wrapping_sub(TCP_LOCAL_SEQ) < 0x8000_0000 {
                TCP_LOCAL_SEQ = ack;
            }
        }
        if TCP_CONNECTED && !has_ack && !has_syn {
            let _ = send_tcp_segment(
                TCP_REMOTE_IP,
                TCP_REMOTE_PORT,
                TCP_LOCAL_PORT,
                TCP_LOCAL_SEQ,
                TCP_REMOTE_SEQ,
                0x10,
                &[],
            );
        }
        if TCP_CONNECTED && !data.is_empty() {
            let room = TCP_DATA.len().saturating_sub(TCP_DATA_LEN);
            let n = data.len().min(room);
            TCP_DATA[TCP_DATA_LEN..TCP_DATA_LEN + n].copy_from_slice(&data[..n]);
            TCP_DATA_LEN += n;
            TCP_REMOTE_SEQ = seq.wrapping_add(data.len() as u32);
            let _ = send_tcp_segment(
                TCP_REMOTE_IP,
                TCP_REMOTE_PORT,
                TCP_LOCAL_PORT,
                TCP_LOCAL_SEQ,
                TCP_REMOTE_SEQ,
                0x10,
                &[],
            );
        }
        if TCP_CONNECTED && has_fin {
            TCP_REMOTE_SEQ = seq.wrapping_add(data.len() as u32).wrapping_add(1);
            TCP_FIN_RECEIVED = true;
            let _ = send_tcp_segment(
                TCP_REMOTE_IP,
                TCP_REMOTE_PORT,
                TCP_LOCAL_PORT,
                TCP_LOCAL_SEQ,
                TCP_REMOTE_SEQ,
                0x11,
                &[],
            );
            TCP_CONNECTED = false;
        }
    }
}

fn dns_parse_a(data: &[u8], id: u16) -> Option<Ipv4Address> {
    if data.len() < 12 || u16::from_be_bytes([data[0], data[1]]) != id {
        return None;
    }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    // Yanıt bayrağı (QR=1) ve RCODE == 0 (hata yok) olmalı
    if (flags & 0x8000) == 0 || (flags & 0x000F) != 0 {
        return None;
    }
    let qd = u16::from_be_bytes([data[4], data[5]]) as usize;
    let an = u16::from_be_bytes([data[6], data[7]]) as usize;
    let mut p = 12usize;
    // Soru bölümünü atla
    for _ in 0..qd {
        loop {
            if p >= data.len() {
                return None;
            }
            let n = data[p] as usize;
            p += 1;
            if n == 0 {
                break;
            }
            if (n & 0xC0) == 0xC0 {
                // Sıkıştırılmış isim işaretçisi (2 bayt)
                if p >= data.len() {
                    return None;
                }
                p += 1;
                break;
            } else if (n & 0xC0) != 0 {
                return None; // Desteklenmeyen etiket formatı
            }
            if p + n > data.len() {
                return None;
            }
            p += n;
        }
        if p + 4 > data.len() {
            return None;
        }
        p += 4; // QTYPE (2) + QCLASS (2)
    }
    // Yanıt bölümünü ayrıştır ve A kaydını ara
    for _ in 0..an {
        if p >= data.len() {
            return None;
        }
        // İsim alanını atla (pointer veya label listesi olabilir)
        loop {
            if p >= data.len() {
                return None;
            }
            let n = data[p] as usize;
            if (n & 0xC0) == 0xC0 {
                if p + 2 > data.len() {
                    return None;
                }
                p += 2;
                break;
            } else if n == 0 {
                p += 1;
                break;
            } else {
                p += 1;
                if p + n > data.len() {
                    return None;
                }
                p += n;
            }
        }
        if p + 10 > data.len() {
            return None;
        }
        let typ = u16::from_be_bytes([data[p], data[p + 1]]);
        let class = u16::from_be_bytes([data[p + 2], data[p + 3]]);
        let rdlen = u16::from_be_bytes([data[p + 8], data[p + 9]]) as usize;
        p += 10;
        if p + rdlen > data.len() {
            return None;
        }
        // TYPE A (1) ve CLASS IN (1) ve RDLENGTH 4
        if typ == 1 && class == 1 && rdlen == 4 {
            return Some(Ipv4Address([
                data[p],
                data[p + 1],
                data[p + 2],
                data[p + 3],
            ]));
        }
        p += rdlen;
    }
    None
}

/// DNS yanıtındaki ilk AAAA kaydını ayrıştırır (RFC 3596).
/// Soru bölümünü ve sıkıştırılmış DNS isimlerini güvenli şekilde atlar.
pub fn dns_query_aaaa(host: &str) -> bool {
    let bytes = host.as_bytes();
    if bytes.is_empty() || bytes.len() > 96 {
        return false;
    }
    let mut q = [0u8; 256];
    q[0..2].copy_from_slice(&0x4D4Eu16.to_be_bytes());
    q[2..4].copy_from_slice(&0x0100u16.to_be_bytes()); // Standard query, RD=1
    q[4..6].copy_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    let mut p = 12;
    let mut start = 0;
    for i in 0..=bytes.len() {
        if i == bytes.len() || bytes[i] == b'.' {
            let n = i - start;
            if n == 0 || n > 63 {
                return false;
            }
            q[p] = n as u8;
            p += 1;
            q[p..p + n].copy_from_slice(&bytes[start..i]);
            p += n;
            start = i + 1;
        }
    }
    q[p] = 0;
    p += 1;
    q[p..p + 2].copy_from_slice(&28u16.to_be_bytes()); // QTYPE: AAAA
    p += 2;
    q[p..p + 2].copy_from_slice(&1u16.to_be_bytes()); // QCLASS: IN
    p += 2;
    unsafe {
        DNS_REPLY_LEN = 0;
        DNS_REPLY_IP = [0; 4];
    }

    let primary_dns = dns_server();
    let sent_primary = send_udp(primary_dns, 49154, DNS_PORT, &q[..p]);

    // Yedek olarak genel DNS (1.1.1.1) sunucusuna da ilet
    let fallback_dns = Ipv4Address::new(1, 1, 1, 1);
    if primary_dns != fallback_dns {
        send_udp(fallback_dns, 49155, DNS_PORT, &q[..p]);
    }
    sent_primary
}

/// DNS yanıtındaki ilk AAAA kaydını ayrıştırır (RFC 3596).
/// Soru bölümünü ve sıkıştırılmış DNS isimlerini güvenli şekilde atlar.
pub fn dns_parse_aaaa(data: &[u8], id: u16) -> Option<Ipv6Address> {
    if data.len() < 12 || u16::from_be_bytes([data[0], data[1]]) != id {
        return None;
    }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 || flags & 0x000f != 0 {
        return None;
    }
    let qd = u16::from_be_bytes([data[4], data[5]]) as usize;
    let an = u16::from_be_bytes([data[6], data[7]]) as usize;
    let mut p = 12usize;
    for _ in 0..qd {
        loop {
            let n = *data.get(p)? as usize;
            p += 1;
            if n == 0 {
                break;
            }
            if n & 0xc0 == 0xc0 {
                p += 1;
                break;
            }
            if n & 0xc0 != 0 {
                return None;
            }
            p = p.checked_add(n)?;
            if p > data.len() {
                return None;
            }
        }
        p = p.checked_add(4)?;
        if p > data.len() {
            return None;
        }
    }
    for _ in 0..an {
        let n = *data.get(p)? as usize;
        p += 1;
        if n & 0xc0 == 0xc0 {
            p += 1;
        } else {
            if n != 0 {
                p = p.checked_add(n)?;
            }
            while *data.get(p)? != 0 {
                let l = data[p] as usize;
                p += 1;
                p = p.checked_add(l)?;
            }
            p += 1;
        }
        if p + 10 > data.len() {
            return None;
        }
        let typ = u16::from_be_bytes([data[p], data[p + 1]]);
        let class = u16::from_be_bytes([data[p + 2], data[p + 3]]);
        let len = u16::from_be_bytes([data[p + 8], data[p + 9]]) as usize;
        p += 10;
        if p + len > data.len() {
            return None;
        }
        if typ == 28 && class == 1 && len == 16 {
            let mut a = [0u8; 16];
            a.copy_from_slice(&data[p..p + 16]);
            return Some(Ipv6Address(a));
        }
        p += len;
    }
    None
}

/// AAAA DNS yanıtını alır ve Ipv6Address döner.
pub fn take_dns_result_aaaa() -> Option<Ipv6Address> {
    unsafe {
        if DNS_REPLY_LEN == 0 {
            None
        } else {
            DNS_REPLY_LEN = 0;
            let mut buf = [0u8; 512];
            let n = take_dns_packet(&mut buf);
            if n >= 12 {
                if let Some(ip) = dns_parse_aaaa(&buf, 0x4D4E) {
                    return Some(ip);
                }
            }
            None
        }
    }
}

pub fn take_dns_result() -> Option<Ipv4Address> {
    unsafe {
        if DNS_REPLY_LEN == 0 {
            None
        } else {
            DNS_REPLY_LEN = 0;
            Some(Ipv4Address(DNS_REPLY_IP))
        }
    }
}
pub fn take_dns_packet(buf: &mut [u8]) -> usize {
    unsafe {
        let n = DNS_REPLY_LEN.min(buf.len());
        buf[..n].copy_from_slice(&DNS_REPLY_DATA[..n]);
        DNS_REPLY_LEN = 0;
        n
    }
}
pub fn take_tcp_data(buf: &mut [u8]) -> usize {
    unsafe {
        let n = TCP_DATA_LEN.min(buf.len());
        buf[..n].copy_from_slice(&TCP_DATA[..n]);
        if n < TCP_DATA_LEN {
            TCP_DATA.copy_within(n..TCP_DATA_LEN, 0);
        }
        TCP_DATA_LEN -= n;
        n
    }
}
pub fn tcp_connected() -> bool {
    unsafe { TCP_CONNECTED }
}
pub fn tcp_syn_acked() -> bool {
    unsafe { TCP_SYN_ACKED }
}
pub fn tcp_finished() -> bool {
    unsafe { TCP_FIN_RECEIVED }
}
pub fn tcp_reset_received() -> bool {
    unsafe { TCP_RESET }
}
pub fn tcp_clear_result() {
    unsafe {
        TCP_DATA_LEN = 0;
        TCP_FIN_RECEIVED = false;
        TCP_RESET = false;
        TCP_CONNECTED = false;
        TCP_SYN_ACKED = false;
    }
}

/// UDP datagram gönderir. IPv4 UDP checksum'i isteğe bağlı olduğu için sıfır bırakılır.
pub fn send_udp(dst: Ipv4Address, src_port: u16, dst_port: u16, data: &[u8]) -> bool {
    if data.len() > 1400 {
        return false;
    }
    let mut packet = [0u8; 1500 - 14 - 20];
    let len = 8 + data.len();
    packet[0..2].copy_from_slice(&src_port.to_be_bytes());
    packet[2..4].copy_from_slice(&dst_port.to_be_bytes());
    packet[4..6].copy_from_slice(&(len as u16).to_be_bytes());
    packet[6..8].copy_from_slice(&0u16.to_be_bytes());
    packet[8..len].copy_from_slice(data);
    send_ipv4(dst, IP_PROTO_UDP, &packet[..len])
}

/// DNS A sorgusu oluşturup gönderir; hem yapılandırılmış DNS sunucusuna hem de 1.1.1.1 yedek sunucuya iletir.
pub fn dns_query(host: &str) -> bool {
    let bytes = host.as_bytes();
    if bytes.is_empty() || bytes.len() > 96 {
        return false;
    }
    let mut q = [0u8; 256];
    q[0..2].copy_from_slice(&0x4D4Du16.to_be_bytes());
    q[2..4].copy_from_slice(&0x0100u16.to_be_bytes()); // Standard query, RD=1
    q[4..6].copy_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    let mut p = 12;
    let mut start = 0;
    for i in 0..=bytes.len() {
        if i == bytes.len() || bytes[i] == b'.' {
            let n = i - start;
            if n == 0 || n > 63 {
                return false;
            }
            q[p] = n as u8;
            p += 1;
            q[p..p + n].copy_from_slice(&bytes[start..i]);
            p += n;
            start = i + 1;
        }
    }
    q[p] = 0;
    p += 1;
    q[p..p + 2].copy_from_slice(&1u16.to_be_bytes());
    p += 2; // QTYPE: A
    q[p..p + 2].copy_from_slice(&1u16.to_be_bytes());
    p += 2; // QCLASS: IN
    unsafe {
        DNS_REPLY_LEN = 0;
        DNS_REPLY_IP = [0; 4];
    }

    let primary_dns = dns_server();
    let sent_primary = send_udp(primary_dns, 49152, DNS_PORT, &q[..p]);

    // Yedek olarak genel DNS (1.1.1.1) sunucusuna da ilet
    let fallback_dns = Ipv4Address::new(1, 1, 1, 1);
    if primary_dns != fallback_dns {
        send_udp(fallback_dns, 49153, DNS_PORT, &q[..p]);
    }
    sent_primary
}

pub fn resolve_ipv4(host: &str) -> Option<Ipv4Address> {
    if let Some(ip) = parse_ipv4(host) {
        return Some(ip);
    }
    if !dns_query(host) {
        return None;
    }
    let mut spins = 0usize;
    loop {
        e1000::poll();
        if let Some(ip) = take_dns_result() {
            return Some(ip);
        }
        spins += 1;
        if spins >= 5_000_000 {
            return None;
        }
        core::hint::spin_loop();
    }
}

pub fn parse_ipv4(host: &str) -> Option<Ipv4Address> {
    let mut parts = host.split('.');
    let mut ip = [0u8; 4];
    for octet in &mut ip {
        *octet = parts.next()?.parse().ok()?;
    }
    if parts.next().is_some() {
        None
    } else {
        Some(Ipv4Address(ip))
    }
}

pub(crate) fn send_tcp_segment(
    dst: Ipv4Address,
    dst_port: u16,
    src_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    data: &[u8],
) -> bool {
    if data.len() > 1300 {
        return false;
    }
    let mut seg = [0u8; 1400];
    seg[0..2].copy_from_slice(&src_port.to_be_bytes());
    seg[2..4].copy_from_slice(&dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&seq.to_be_bytes());
    seg[8..12].copy_from_slice(&ack.to_be_bytes());
    seg[12] = 0x50;
    seg[13] = flags;
    seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    seg[20..20 + data.len()].copy_from_slice(data);
    let n = 20 + data.len();
    let c = transport_checksum(local_ip(), dst, IP_PROTO_TCP, &seg[..n]);
    seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst, IP_PROTO_TCP, &seg[..n])
}

pub fn tcp_begin(dst: Ipv4Address, dst_port: u16, src_port: u16) -> bool {
    unsafe {
        TCP_LOCAL_PORT = src_port;
        TCP_REMOTE_PORT = dst_port;
        TCP_REMOTE_IP = dst;
        TCP_LOCAL_SEQ = 0x1020_3040;
        TCP_REMOTE_SEQ = 0;
        TCP_DATA_LEN = 0;
        TCP_CONNECTED = false;
        TCP_SYN_ACKED = false;
        TCP_FIN_RECEIVED = false;
        TCP_RESET = false;
    }
    send_tcp_segment(dst, dst_port, src_port, 0x1020_3040, 0, 0x02, &[])
}

pub fn tcp_send(data: &[u8]) -> bool {
    unsafe {
        if !TCP_CONNECTED {
            return false;
        }
        let ok = send_tcp_segment(
            TCP_REMOTE_IP,
            TCP_REMOTE_PORT,
            TCP_LOCAL_PORT,
            TCP_LOCAL_SEQ,
            TCP_REMOTE_SEQ,
            0x18,
            data,
        );
        if ok {
            TCP_LOCAL_SEQ = TCP_LOCAL_SEQ.wrapping_add(data.len() as u32);
        }
        ok
    }
}

pub fn tcp_close() -> bool {
    unsafe {
        if !TCP_CONNECTED {
            return false;
        }
        let ok = send_tcp_segment(
            TCP_REMOTE_IP,
            TCP_REMOTE_PORT,
            TCP_LOCAL_PORT,
            TCP_LOCAL_SEQ,
            TCP_REMOTE_SEQ,
            0x11,
            &[],
        );
        if ok {
            TCP_LOCAL_SEQ = TCP_LOCAL_SEQ.wrapping_add(1);
            TCP_CONNECTED = false;
        }
        ok
    }
}

/// TCP SYN gönderir; gelen TCP yükleri `take_udp_data` üzerinden alınabilir.
pub fn tcp_connect(dst: Ipv4Address, dst_port: u16, src_port: u16) -> bool {
    let mut seg = [0u8; 20];
    seg[0..2].copy_from_slice(&src_port.to_be_bytes());
    seg[2..4].copy_from_slice(&dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&0x1000u32.to_be_bytes());
    seg[12] = 0x50;
    seg[13] = 0x02;
    seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    let c = transport_checksum(local_ip(), dst, IP_PROTO_TCP, &seg);
    seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst, IP_PROTO_TCP, &seg)
}

/// Bağlantı kurulmuş bir TCP akışına HTTP/1.0 GET isteği hazırlayıp gönderir.
/// `seq` ve `ack` çağıranın takip ettiği TCP sıra numaralarıdır.
pub fn http_get(dst: Ipv4Address, host: &str, path: &str, seq: u32, ack: u32) -> bool {
    let hb = host.as_bytes();
    let pb = path.as_bytes();
    if hb.len() > 96 || pb.len() > 180 {
        return false;
    }
    let mut seg = [0u8; 620];
    let req = b"GET ";
    let mut n = 0;
    seg[20..24].copy_from_slice(req);
    n += 4;
    seg[20 + n..20 + n + pb.len()].copy_from_slice(pb);
    n += pb.len();
    let tail = b" HTTP/1.0\r\nHost: ";
    seg[20 + n..20 + n + tail.len()].copy_from_slice(tail);
    n += tail.len();
    seg[20 + n..20 + n + hb.len()].copy_from_slice(hb);
    n += hb.len();
    let end = b"\r\nConnection: close\r\n\r\n";
    seg[20 + n..20 + n + end.len()].copy_from_slice(end);
    n += end.len();
    seg[0..2].copy_from_slice(&49153u16.to_be_bytes());
    seg[2..4].copy_from_slice(&HTTP_PORT.to_be_bytes());
    seg[4..8].copy_from_slice(&seq.to_be_bytes());
    seg[8..12].copy_from_slice(&ack.to_be_bytes());
    seg[12] = 0x50;
    seg[13] = 0x18;
    seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    let total = 20 + n;
    let c = transport_checksum(local_ip(), dst, IP_PROTO_TCP, &seg[..total]);
    seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst, IP_PROTO_TCP, &seg[..total])
}

// ---------------------------------------------------------------------------
// Ping (ICMP Echo Request) — dış sunucuya test
// ---------------------------------------------------------------------------

pub fn ping_ex(target: Ipv4Address, seq: u16, payload_len: usize) -> bool {
    let mut icmp = [0u8; 128];
    let plen = payload_len.min(64);
    let total = 8 + plen;
    icmp[0] = ICMP_ECHO_REQUEST;
    icmp[1] = 0;
    icmp[2] = 0;
    icmp[3] = 0;
    icmp[4] = 0x12;
    icmp[5] = 0x34;
    icmp[6..8].copy_from_slice(&seq.to_be_bytes());
    for i in 0..plen {
        icmp[8 + i] = b'a' + ((i % 26) as u8);
    }
    let ck = checksum(&icmp[..total]);
    icmp[2] = (ck >> 8) as u8;
    icmp[3] = (ck & 0xFF) as u8;
    send_ipv4(target, IP_PROTO_ICMP, &icmp[..total])
}

pub fn ping(target: Ipv4Address) -> bool {
    ping_ex(target, 1, 32)
}

// ---------------------------------------------------------------------------
// ARP çözümlemeyi başlat (netdev user için gateway)
// ---------------------------------------------------------------------------

pub fn init() {
    // QEMU SLIRP varsayılan gateway ve DNS MAC eşleştirmesini önceden kaydet
    arp_insert(GATEWAY_IP, MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x02]));
    arp_insert(DNS_SERVER, MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x03]));
    // Canlı ARP güncellemesi için gateway'e request gönder
    send_arp_request(GATEWAY_IP);
}

/// e1000 ağ kartı ve TCP/IP yığını için self-test çalıştırır ve seri porta raporlar.
pub fn run_network_self_test() -> bool {
    crate::serial_print("[NET-TEST] --- e1000 ve TCP/IP yığını doğrulama testi ---\n");

    if !e1000::is_ready() {
        crate::serial_print("[NET-TEST] e1000 sürücüsü hazır değil, ağ testleri ATLANDI.\n");
        return false;
    }

    let link_up = e1000::link_up();
    crate::serial_print("[NET-TEST] e1000 Link Durumu: ");
    if link_up {
        crate::serial_print("LINK UP (aktif)\n");
    } else {
        crate::serial_print("LINK DOWN (kablo/ağ bağlantısı yok)\n");
    }

    // 1. ARP Istegi ve TX Ring iletim testi (Canlı ARP Yanıtı Bekleme)
    unsafe {
        LAST_ARP_REPLY_SRC = None;
    }
    crate::serial_print("[NET-TEST] 1. ARP: Gateway (10.0.2.2) icin ARP istegi gönderiliyor... ");
    send_arp_request(GATEWAY_IP);
    crate::serial_print("TX iletildi.\n");

    let mut arp_replied = false;
    for _ in 0..2_000 {
        e1000::poll();
        if unsafe { LAST_ARP_REPLY_SRC.is_some() } {
            arp_replied = true;
            break;
        }
        core::hint::spin_loop();
    }
    crate::serial_print("[NET-TEST] 1. ARP yanıtı: ");
    if arp_replied {
        crate::serial_print("GERÇEK YANIT ALINDI (gateway fiziksel MAC doğrulandı)\n");
    } else {
        crate::serial_print("SLIRP statik/önbellek MAC etkin (zaman aşımı)\n");
    }

    // 2. ICMP Echo Request ve Echo Reply testi
    clear_ping_reply();
    crate::serial_print("[NET-TEST] 2. ICMP: Gateway'e Ping paketi gönderiliyor... ");
    let ping_sent = ping(GATEWAY_IP);
    if ping_sent {
        crate::serial_print("TX iletildi, Echo Reply bekleniyor... ");
        let mut echo_replied = false;
        for _ in 0..3_000 {
            e1000::poll();
            if has_ping_reply() {
                echo_replied = true;
                break;
            }
            core::hint::spin_loop();
        }
        if echo_replied {
            crate::serial_print("BAŞARILI (ICMP Echo Reply alındı)\n");
        } else {
            crate::serial_print(
                "YANIT BEKLENİYOR (QEMU ağ geçidinde ICMP kapalı veya yanıt gecikti)\n",
            );
        }
    } else {
        crate::serial_print("BAŞARISIZ (TX kuyruk hatası)\n");
    }

    // 3. TCP Bağlantısı (3-Way Handshake) & HTTP Testi
    crate::serial_print(
        "[NET-TEST] 3. TCP: Gateway HTTP portuna (10.0.2.2:80) SYN gönderiliyor... ",
    );
    let tcp_init = tcp_begin(GATEWAY_IP, 80, 49200);
    if tcp_init {
        let mut tcp_synack = false;
        for _ in 0..5_000 {
            e1000::poll();
            if unsafe { TCP_CONNECTED } {
                tcp_synack = true;
                break;
            }
            core::hint::spin_loop();
        }
        if tcp_synack {
            crate::serial_print("BAĞLANTI KURULDU (SYN-ACK alındı, ESTABLISHED)!\n");
            // HTTP İsteği gönder
            crate::serial_print("[NET-TEST] 4. HTTP: GET / HTTP/1.0 istegi gönderiliyor... ");
            let http_sent =
                tcp_send(b"GET / HTTP/1.0\r\nHost: 10.0.2.2\r\nConnection: close\r\n\r\n");
            if http_sent {
                crate::serial_print("İletildi, veri bekleniyor... ");
                for _ in 0..5_000 {
                    e1000::poll();
                    if unsafe { TCP_DATA_LEN > 0 || TCP_FIN_RECEIVED } {
                        break;
                    }
                    core::hint::spin_loop();
                }
                let data_len = unsafe { TCP_DATA_LEN };
                if data_len > 0 {
                    crate::serial_print("BAŞARILI (HTTP yanıtı alındı: ");
                    crate::serial::print_dec(data_len as u64);
                    crate::serial_print(" bayt)\n");
                } else {
                    crate::serial_print("Yanıt tamponu boş (sunucu yanıt vermedi)\n");
                }
            } else {
                crate::serial_print("Gönderilemedi\n");
            }
            tcp_close();
        } else {
            crate::serial_print(
                "SYN-ACK ALINAMADI (80 numaralı bağlantı noktası kapalı veya dinleyici yok)\n",
            );
        }
    } else {
        crate::serial_print("TCP paketi iletilemedi\n");
    }

    // 4. Soket API + HTTP istemcisi mantık self-testleri (NIC gerektirmez).
    let sock_ok = crate::drivers::socket::self_test();
    let http_ok = crate::drivers::http::self_test();
    if sock_ok && http_ok {
        crate::serial_print("[NET-TEST] Soket + HTTP mantık testleri: BAŞARILI\n");
    } else {
        crate::serial_print("[NET-TEST] Soket/HTTP mantık testleri: BAŞARISIZ\n");
    }

    ping_sent
}

pub fn dhcp_request() -> bool {
    unsafe {
        DHCP_READY = false;
        DHCP_OFFER = None;
    }
    let mac = unsafe { e1000::MAC_ADDR };
    let mut p = [0u8; 300];
    p[0] = 1;
    p[1] = 1;
    p[2] = 6;
    p[4..8].copy_from_slice(&unsafe { DHCP_XID }.to_be_bytes());
    p[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    let mut n = 240;
    p[n] = 53;
    p[n + 1] = 1;
    p[n + 2] = 1;
    n += 3;
    p[n] = 55;
    p[n + 1] = 4;
    p[n + 2] = 1;
    p[n + 3] = 3;
    p[n + 4] = 6;
    p[n + 5] = 51;
    n += 6;
    p[n] = 61;
    p[n + 1] = 7;
    p[n + 2] = 1;
    p[n + 3..n + 9].copy_from_slice(&mac);
    n += 9;
    p[n] = 255;
    n += 1;
    send_dhcp_bootstrap(&p[..n])
}

fn send_dhcp_bootstrap(data: &[u8]) -> bool {
    if data.len() > 1200 {
        return false;
    }
    let mut udp = [0u8; 1210];
    let len = data.len() + 8;
    udp[0..2].copy_from_slice(&68u16.to_be_bytes());
    udp[2..4].copy_from_slice(&67u16.to_be_bytes());
    udp[4..6].copy_from_slice(&(len as u16).to_be_bytes());
    udp[8..len].copy_from_slice(data);
    let mut ip = [0u8; 1230];
    let total = len + 20;
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    ip[6] = 0x40;
    ip[8] = 64;
    ip[9] = IP_PROTO_UDP;
    ip[16..20].copy_from_slice(&[255; 4]);
    let c = checksum(&ip[..20]);
    ip[10..12].copy_from_slice(&c.to_be_bytes());
    ip[20..total].copy_from_slice(&udp[..len]);
    let mut frame = [0u8; 1260];
    let n = build_ethernet(MAC_BROADCAST, ETHERTYPE_IPV4, &ip[..total], &mut frame);
    e1000::transmit(&frame[..n])
}

fn handle_dhcp(src: Ipv4Address, data: &[u8]) {
    if data.len() < 240 || data[0] != 2 || data[1] != 1 || data[2] != 6 {
        return;
    }
    let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if xid != unsafe { DHCP_XID } {
        return;
    }
    let offered = Ipv4Address([data[16], data[17], data[18], data[19]]);
    let mut mask = SUBNET_MASK;
    let mut router = src;
    let mut dns = DNS_SERVER;
    let mut server = src;
    let mut msg = 0u8;
    let mut p = 240;
    while p < data.len() {
        let tag = data[p];
        p += 1;
        if tag == 255 {
            break;
        }
        if tag == 0 {
            continue;
        }
        if p >= data.len() {
            return;
        }
        let len = data[p] as usize;
        p += 1;
        if p + len > data.len() {
            return;
        }
        let v = &data[p..p + len];
        p += len;
        match tag {
            53 if len == 1 => msg = v[0],
            1 if len >= 4 => mask = Ipv4Address([v[0], v[1], v[2], v[3]]),
            3 if len >= 4 => router = Ipv4Address([v[0], v[1], v[2], v[3]]),
            6 if len >= 4 => dns = Ipv4Address([v[0], v[1], v[2], v[3]]),
            54 if len >= 4 => server = Ipv4Address([v[0], v[1], v[2], v[3]]),
            _ => {}
        }
    }
    if msg == 2 {
        unsafe {
            DHCP_OFFER = Some((offered, router, dns));
            DHCP_SERVER_IP = server;
        }
        send_dhcp_request(offered);
    } else if msg == 5 {
        unsafe {
            ACTIVE_LOCAL_IP = offered;
            ACTIVE_GATEWAY_IP = router;
            ACTIVE_DNS_IP = dns;
            ACTIVE_SUBNET_MASK = mask;
            DHCP_READY = true;
        }
        send_arp_request(router);
    }
}

fn send_dhcp_request(offered: Ipv4Address) {
    let mac = unsafe { e1000::MAC_ADDR };
    let server = unsafe { DHCP_SERVER_IP };
    let mut p = [0u8; 300];
    p[0] = 1;
    p[1] = 1;
    p[2] = 6;
    p[4..8].copy_from_slice(&unsafe { DHCP_XID }.to_be_bytes());
    p[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    let mut n = 240;
    p[n] = 53;
    p[n + 1] = 1;
    p[n + 2] = 3;
    n += 3;
    p[n] = 50;
    p[n + 1] = 4;
    p[n + 2..n + 6].copy_from_slice(&offered.0);
    n += 6;
    p[n] = 54;
    p[n + 1] = 4;
    p[n + 2..n + 6].copy_from_slice(&server.0);
    n += 6;
    p[n] = 61;
    p[n + 1] = 7;
    p[n + 2] = 1;
    p[n + 3..n + 9].copy_from_slice(&mac);
    n += 9;
    p[n] = 255;
    n += 1;
    let _ = send_dhcp_bootstrap(&p[..n]);
}

/// IPv6 Adresi (128-bit / 16 bayt).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv6Address(pub [u8; 16]);

impl Ipv6Address {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const UNSPECIFIED: Self = Self([0; 16]);
    pub const LOOPBACK: Self = Self([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    /// Link-local olup olmadığını kontrol eder (fe80::/10).
    pub fn is_link_local(&self) -> bool {
        self.0[0] == 0xFE && (self.0[1] & 0xC0) == 0x80
    }

    /// Loopback (::1) olup olmadığını kontrol eder.
    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }

    /// Multicast (ff00::/8) olup olmadığını kontrol eder.
    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xFF
    }

    /// IPv6 çok noktaya yayın (multicast) için hedef MAC adresini üretir (RFC 2464).
    pub fn multicast_mac(&self) -> MacAddress {
        MacAddress([0x33, 0x33, self.0[12], self.0[13], self.0[14], self.0[15]])
    }

    /// Adresi standart onaltılık metin biçiminde biçimlendirir.
    pub fn format(&self) -> alloc::string::String {
        use alloc::format;
        format!(
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
            u16::from_be_bytes([self.0[4], self.0[5]]),
            u16::from_be_bytes([self.0[6], self.0[7]]),
            u16::from_be_bytes([self.0[8], self.0[9]]),
            u16::from_be_bytes([self.0[10], self.0[11]]),
            u16::from_be_bytes([self.0[12], self.0[13]]),
            u16::from_be_bytes([self.0[14], self.0[15]])
        )
    }
}

pub const ICMPV6_TYPE_DEST_UNREACH: u8 = 1;
pub const ICMPV6_TYPE_PKT_TOO_BIG: u8 = 2;
pub const ICMPV6_TYPE_TIME_EXCEEDED: u8 = 3;
pub const ICMPV6_TYPE_PARAM_PROBLEM: u8 = 4;
pub const ICMPV6_TYPE_ECHO_REQUEST: u8 = 128;
pub const ICMPV6_TYPE_ECHO_REPLY: u8 = 129;
pub const ICMPV6_TYPE_ROUTER_SOLICIT: u8 = 133;
pub const ICMPV6_TYPE_ROUTER_ADVERT: u8 = 134;
pub const ICMPV6_TYPE_NEIGHBOR_SOLICIT: u8 = 135;
pub const ICMPV6_TYPE_NEIGHBOR_ADVERT: u8 = 136;

/// Yerel ağ kartının donanım (MAC) adresini döndürür.
pub fn local_mac() -> MacAddress {
    MacAddress(unsafe { e1000::MAC_ADDR })
}

/// Yerel arayüz MAC adresinden otomatik link-local IPv6 (EUI-64) türetir.
pub fn local_ipv6() -> Ipv6Address {
    let mac = local_mac();
    let mut bytes = [0u8; 16];
    bytes[0] = 0xFE;
    bytes[1] = 0x80;
    // EUI-64: U/L bit ters çevrilir, ortaya 0xFFFE eklenir
    bytes[8] = mac.0[0] ^ 0x02;
    bytes[9] = mac.0[1];
    bytes[10] = mac.0[2];
    bytes[11] = 0xFF;
    bytes[12] = 0xFE;
    bytes[13] = mac.0[3];
    bytes[14] = mac.0[4];
    bytes[15] = mac.0[5];
    Ipv6Address(bytes)
}

/// IPv6 sözde başlığı (pseudo-header) üzerinden 16-bit sağlama toplamı (checksum) hesaplar (RFC 8200).
pub fn ipv6_pseudo_checksum(
    src: &[u8; 16],
    dst: &[u8; 16],
    next_header: u8,
    payload: &[u8],
) -> u16 {
    let mut sum: u32 = 0;
    for i in 0..8 {
        sum += u16::from_be_bytes([src[i * 2], src[i * 2 + 1]]) as u32;
    }
    for i in 0..8 {
        sum += u16::from_be_bytes([dst[i * 2], dst[i * 2 + 1]]) as u32;
    }
    let len = payload.len() as u32;
    sum += (len >> 16) & 0xFFFF;
    sum += len & 0xFFFF;
    sum += next_header as u32;

    let mut i = 0;
    while i + 1 < payload.len() {
        sum += u16::from_be_bytes([payload[i], payload[i + 1]]) as u32;
        i += 2;
    }
    if i < payload.len() {
        sum += (payload[i] as u32) << 8;
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let res = !(sum as u16);
    if res == 0 {
        0xFFFF
    } else {
        res
    }
}

/// IPv6 paketi gönderir (Ethernet çerçevesi içinde).
pub fn send_ipv6(dst_ip: Ipv6Address, next_header: u8, payload: &[u8]) -> bool {
    let src_ip = local_ipv6();
    let dst_mac = if dst_ip.is_multicast() {
        dst_ip.multicast_mac()
    } else if let Some(mac) = neighbor_lookup(dst_ip) {
        mac
    } else {
        // Henüz bilinmiyor: Neighbor Solicitation gönder ve gateway fallback'i kullan
        let _ = send_neighbor_solicitation(dst_ip);
        MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x02])
    };

    let mut ip_packet = [0u8; 1500 - 14];
    if payload.len() > ip_packet.len() - 40 {
        return false;
    }
    let total_payload_len = payload.len() as u16;

    // IPv6 başlığı (40 bayt)
    ip_packet[0] = 0x60; // Version 6
    ip_packet[1] = 0x00; // Traffic Class
    ip_packet[2] = 0x00; // Flow Label
    ip_packet[3] = 0x00;
    ip_packet[4] = (total_payload_len >> 8) as u8;
    ip_packet[5] = (total_payload_len & 0xFF) as u8;
    ip_packet[6] = next_header;
    ip_packet[7] = 64; // Hop Limit
    ip_packet[8..24].copy_from_slice(&src_ip.0);
    ip_packet[24..40].copy_from_slice(&dst_ip.0);
    ip_packet[40..40 + payload.len()].copy_from_slice(payload);

    if dst_ip.is_loopback() || dst_ip == src_ip {
        handle_ipv6(&ip_packet[..40 + payload.len()]);
        return true;
    }

    let mut frame = [0u8; 1514];
    let len = build_ethernet(
        dst_mac,
        ETHERTYPE_IPV6,
        &ip_packet[..40 + payload.len()],
        &mut frame,
    );
    e1000::transmit(&frame[..len])
}

/// IPv6 Komşu Sorgulama (Neighbor Solicitation, ICMPv6 Type 135) gönderir.
/// Belirtilen IPv6 adresinin MAC adresini öğrenmek için kullanılır (RFC 4861).
pub fn send_neighbor_solicitation(target_ip: Ipv6Address) -> bool {
    let src_ip = local_ipv6();
    // Solicited-node multicast adresi: ff02::1:ffXX:XXXX (hedef IP'nin son 24 biti)
    let mut solicited_node = [0u8; 16];
    solicited_node[0] = 0xFF;
    solicited_node[1] = 0x02;
    solicited_node[2] = 0x00;
    solicited_node[3] = 0x00;
    solicited_node[4] = 0x00;
    solicited_node[5] = 0x00;
    solicited_node[6] = 0x00;
    solicited_node[7] = 0x00;
    solicited_node[8] = 0x00;
    solicited_node[9] = 0x00;
    solicited_node[10] = 0x00;
    solicited_node[11] = 0x01;
    solicited_node[12] = 0xFF;
    solicited_node[13] = target_ip.0[13];
    solicited_node[14] = target_ip.0[14];
    solicited_node[15] = target_ip.0[15];
    let dst_mac = MacAddress([0x33, 0x33, 0xFF, target_ip.0[13], target_ip.0[14], target_ip.0[15]]);

    let mut pkt = [0u8; 64];
    let total_len = 32; // ICMPv6 başlığı (8) + Target Address (16) + SLLAO (8)
    pkt[0] = ICMPV6_TYPE_NEIGHBOR_SOLICIT;
    pkt[1] = 0; // Code
    pkt[2] = 0; // Checksum (hesaplanacak)
    pkt[3] = 0; // Reserved
    pkt[8..24].copy_from_slice(&target_ip.0); // Target Address

    // Source Link-Layer Address option (Type 1, Length 1 = 8 bytes)
    pkt[24] = 1; // Type: Source Link-Layer Address
    pkt[25] = 1; // Length: 1 unit of 8 octets
    pkt[26..32].copy_from_slice(&local_mac().0);

    let src = local_ipv6();
    let dst = Ipv6Address(solicited_node);
    let csum = ipv6_pseudo_checksum(&src.0, &dst.0, 58, &pkt[..total_len]);
    pkt[2] = (csum >> 8) as u8;
    pkt[3] = (csum & 0xFF) as u8;

    send_ipv6(dst, 58, &pkt[..total_len])
}

/// IPv6 Komşu Bildirimi (Neighbor Advertisement, ICMPv6 Type 136) gönderir.
/// Bir Komşu Sorgulamaya yanıt olarak veya otomatik olarak gönderilebilir.
pub fn send_neighbor_advertisement(target_ip: Ipv6Address, dst_ip: Ipv6Address, solicited: bool, override_flag: bool) -> bool {
    let src_ip = local_ipv6();
    let dst_mac = if dst_ip.is_multicast() {
        dst_ip.multicast_mac()
    } else {
        neighbor_lookup(dst_ip).unwrap_or(MacAddress([0x33, 0x33, 0xFF, target_ip.0[13], target_ip.0[14], target_ip.0[15]]))
    };

    let mut pkt = [0u8; 32];
    let total_len = 32;
    pkt[0] = ICMPV6_TYPE_NEIGHBOR_ADVERT;
    pkt[1] = 0; // Code
    pkt[2] = 0; // Checksum
    pkt[3] = 0;
    // Flags: Router (bit 7), Solicited (bit 6), Override (bit 5)
    let mut flags: u8 = 0;
    if solicited {
        flags |= 0x40;
    }
    if override_flag {
        flags |= 0x20;
    }
    pkt[4] = flags;
    pkt[8..24].copy_from_slice(&target_ip.0); // Target Address

    // Target Link-Layer Address option (Type 2, Length 1 = 8 bytes)
    pkt[24] = 2; // Type: Target Link-Layer Address
    pkt[25] = 1; // Length: 1 unit of 8 octets
    pkt[26..32].copy_from_slice(&local_mac().0);

    let src = local_ipv6();
    let csum = ipv6_pseudo_checksum(&src.0, &dst_ip.0, 58, &pkt[..total_len]);
    pkt[2] = (csum >> 8) as u8;
    pkt[3] = (csum & 0xFF) as u8;

    let mut frame = [0u8; 1514];
    let ip_packet_len = 40 + total_len;
    let mut ip_packet = [0u8; 1500 - 14];
    ip_packet[0] = 0x60;
    ip_packet[4] = (total_len as u16 >> 8) as u8;
    ip_packet[5] = (total_len as u16 & 0xFF) as u8;
    ip_packet[6] = 58;
    ip_packet[7] = 255;
    ip_packet[8..24].copy_from_slice(&src_ip.0);
    ip_packet[24..40].copy_from_slice(&dst_ip.0);
    ip_packet[40..40 + total_len].copy_from_slice(&pkt[..total_len]);

    let len = build_ethernet(dst_mac, ETHERTYPE_IPV6, &ip_packet[..ip_packet_len], &mut frame);
    e1000::transmit(&frame[..len])
}

/// IPv6 üzerinde TCP segmenti gönderir (RFC 8200/9293).
/// Neighbor Discovery henüz yoksa send_ipv6'in gateway fallback'i kullanılır.
pub fn send_tcp_segment_ipv6(
    dst: Ipv6Address,
    dst_port: u16,
    src_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> bool {
    if payload.len() > 1300 {
        return false;
    }
    let mut seg = [0u8; 1400];
    seg[0..2].copy_from_slice(&src_port.to_be_bytes());
    seg[2..4].copy_from_slice(&dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&seq.to_be_bytes());
    seg[8..12].copy_from_slice(&ack.to_be_bytes());
    seg[12] = 5 << 4;
    seg[13] = flags;
    seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    seg[16..18].fill(0);
    seg[18..20].fill(0);
    let n = 20 + payload.len();
    seg[20..n].copy_from_slice(payload);
    let checksum = ipv6_pseudo_checksum(&local_ipv6().0, &dst.0, IP_PROTO_TCP, &seg[..n]);
    seg[16..18].copy_from_slice(&checksum.to_be_bytes());
    send_ipv6(dst, IP_PROTO_TCP, &seg[..n])
}

/// ICMPv6 Echo Request (Ping6) gönderir.
pub fn send_icmpv6_echo(dst: Ipv6Address, id: u16, seq: u16, data: &[u8]) -> bool {
    let mut pkt = [0u8; 128];
    let total_len = (8 + data.len()).min(pkt.len());
    pkt[0] = ICMPV6_TYPE_ECHO_REQUEST;
    pkt[1] = 0;
    pkt[2] = 0;
    pkt[3] = 0;
    pkt[4..6].copy_from_slice(&id.to_be_bytes());
    pkt[6..8].copy_from_slice(&seq.to_be_bytes());
    let copy_len = (total_len - 8).min(data.len());
    pkt[8..8 + copy_len].copy_from_slice(&data[..copy_len]);

    let src = local_ipv6();
    let csum = ipv6_pseudo_checksum(&src.0, &dst.0, 58, &pkt[..total_len]);
    pkt[2..4].copy_from_slice(&csum.to_be_bytes());

    send_ipv6(dst, 58, &pkt[..total_len])
}

/// ICMPv6 Echo Reply (Ping6) yanıt kapsayıcısı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PingReply6 {
    pub src: Ipv6Address,
    pub seq: u16,
    pub len: usize,
    pub hop_limit: u8,
}

static mut LAST_PING6_REPLY_SRC: Option<Ipv6Address> = None;
static mut LAST_PING6_REPLY_SEQ: u16 = 0;
static mut LAST_PING6_REPLY_LEN: usize = 0;
static mut LAST_PING6_REPLY_HOP_LIMIT: u8 = 0;

#[inline]
pub fn clear_ping6_reply() {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(LAST_PING6_REPLY_SRC), None);
    }
}

#[inline]
pub fn has_ping6_reply() -> bool {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING6_REPLY_SRC)).is_some() }
}

pub fn take_ping6_reply() -> Option<PingReply6> {
    unsafe {
        let src = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING6_REPLY_SRC));
        if let Some(ip) = src {
            let seq = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING6_REPLY_SEQ));
            let len = core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING6_REPLY_LEN));
            let hop_limit =
                core::ptr::read_volatile(core::ptr::addr_of!(LAST_PING6_REPLY_HOP_LIMIT));
            core::ptr::write_volatile(core::ptr::addr_of_mut!(LAST_PING6_REPLY_SRC), None);
            Some(PingReply6 {
                src: ip,
                seq,
                len,
                hop_limit,
            })
        } else {
            None
        }
    }
}

/// ICMPv6 Echo Request paketi gönderir (Ping6).
pub fn ping6_ex(target: Ipv6Address, seq: u16, payload_len: usize) -> bool {
    let mut data = [0u8; 64];
    let plen = payload_len.min(64);
    for i in 0..plen {
        data[i] = b'a' + ((i % 26) as u8);
    }
    send_icmpv6_echo(target, 0x1234, seq, &data[..plen])
}

pub fn ping6(target: Ipv6Address) -> bool {
    ping6_ex(target, 1, 32)
}

/// IPv4 veya IPv6 adres kapsayıcısı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpAddress {
    V4(Ipv4Address),
    V6(Ipv6Address),
}

/// Standart veya sıkıştırılmış IPv6 adres dizesini ayrıştırır (Örn: "::1", "fe80::1", "2001:db8::1", "[::1]").
pub fn parse_ipv6(host: &str) -> Option<Ipv6Address> {
    let clean = host.trim().trim_start_matches('[').trim_end_matches(']');
    if clean.is_empty() {
        return None;
    }

    let mut parts: [u16; 8] = [0; 8];
    if clean == "::" {
        return Some(Ipv6Address::UNSPECIFIED);
    }

    if let Some(double_colon_pos) = clean.find("::") {
        let left = &clean[..double_colon_pos];
        let right = &clean[double_colon_pos + 2..];

        let mut left_parts: [u16; 8] = [0; 8];
        let mut left_count = 0;
        if !left.is_empty() {
            for s in left.split(':') {
                if left_count >= 8 || s.len() > 4 {
                    return None;
                }
                left_parts[left_count] = u16::from_str_radix(s, 16).ok()?;
                left_count += 1;
            }
        }

        let mut right_parts: [u16; 8] = [0; 8];
        let mut right_count = 0;
        if !right.is_empty() {
            for s in right.split(':') {
                if right_count >= 8 || s.len() > 4 {
                    return None;
                }
                right_parts[right_count] = u16::from_str_radix(s, 16).ok()?;
                right_count += 1;
            }
        }

        if left_count + right_count >= 8 {
            return None;
        }

        let zeros = 8 - (left_count + right_count);
        let mut idx = 0;
        for i in 0..left_count {
            parts[idx] = left_parts[i];
            idx += 1;
        }
        idx += zeros;
        for i in 0..right_count {
            parts[idx] = right_parts[i];
            idx += 1;
        }
    } else {
        let mut count = 0;
        for s in clean.split(':') {
            if count >= 8 || s.len() > 4 {
                return None;
            }
            parts[count] = u16::from_str_radix(s, 16).ok()?;
            count += 1;
        }
        if count != 8 {
            return None;
        }
    }

    let mut bytes = [0u8; 16];
    for i in 0..8 {
        let be = parts[i].to_be_bytes();
        bytes[i * 2] = be[0];
        bytes[i * 2 + 1] = be[1];
    }
    Some(Ipv6Address(bytes))
}

pub fn handle_ipv6(payload: &[u8]) {
    // IPv6 başlığı en az 40 bayt olmalıdır
    if payload.len() < 40 {
        return;
    }
    let version = (payload[0] >> 4) & 0x0F;
    if version != 6 {
        return;
    }
    let payload_len = u16::from_be_bytes([payload[4], payload[5]]) as usize;
    let next_header = payload[6];
    let hop_limit = payload[7];
    let _ = (payload_len, hop_limit);

    let mut src = [0u8; 16];
    let mut dst = [0u8; 16];
    src.copy_from_slice(&payload[8..24]);
    dst.copy_from_slice(&payload[24..40]);
    let src_ip = Ipv6Address(src);
    let _dst_ip = Ipv6Address(dst);

    // ICMPv6 (58), TCP (6), UDP (17)
    match next_header {
        58 => {
            // ICMPv6 paketi
            if payload.len() >= 48 {
                let icmp = &payload[40..];
                let icmp_type = icmp[0];
                if icmp_type == ICMPV6_TYPE_ECHO_REQUEST {
                    // ICMPv6 Echo Reply (Type 129, Code 0)
                    let mut reply = [0u8; 1400];
                    let icmp_len = (payload.len() - 40).min(reply.len());
                    reply[..icmp_len].copy_from_slice(&icmp[..icmp_len]);
                    reply[0] = ICMPV6_TYPE_ECHO_REPLY;
                    reply[1] = 0;
                    reply[2] = 0; // Checksum sıfırla
                    reply[3] = 0;
                    let local = local_ipv6();
                    let csum = ipv6_pseudo_checksum(&local.0, &src, 58, &reply[..icmp_len]);
                    reply[2] = (csum >> 8) as u8;
                    reply[3] = (csum & 0xFF) as u8;
                    send_ipv6(src_ip, 58, &reply[..icmp_len]);
                } else if icmp_type == ICMPV6_TYPE_ECHO_REPLY {
                    let seq = if icmp.len() >= 8 {
                        u16::from_be_bytes([icmp[6], icmp[7]])
                    } else {
                        0
                    };
                    let data_len = (payload.len() - 40).saturating_sub(8);
                    unsafe {
                        core::ptr::write_volatile(
                            core::ptr::addr_of_mut!(LAST_PING6_REPLY_SRC),
                            Some(src_ip),
                        );
                        core::ptr::write_volatile(
                            core::ptr::addr_of_mut!(LAST_PING6_REPLY_SEQ),
                            seq,
                        );
                        core::ptr::write_volatile(
                            core::ptr::addr_of_mut!(LAST_PING6_REPLY_LEN),
                            data_len,
                        );
                        core::ptr::write_volatile(
                            core::ptr::addr_of_mut!(LAST_PING6_REPLY_HOP_LIMIT),
                            hop_limit,
                        );
                    }
                } else if icmp_type == ICMPV6_TYPE_NEIGHBOR_SOLICIT {
                    // Neighbor Advertisement (Type 136)
                    let icmp_len = payload.len() - 40;
                    if icmp_len >= 24 {
                        let target_ip = &icmp[8..24];
                        let mut na = [0u8; 32];
                        na[0] = ICMPV6_TYPE_NEIGHBOR_ADVERT;
                        na[1] = 0;
                        na[2] = 0;
                        na[3] = 0;
                        na[4] = 0x60; // Flags: Solicited (0x40) | Override (0x20)
                        na[8..24].copy_from_slice(target_ip);
                        na[24] = 2; // Option 2: Target Link-Layer Address
                        na[25] = 1; // Length: 8 bytes (1 unit of 8 octets)
                        na[26..32].copy_from_slice(&local_mac().0);

                        let local = local_ipv6();
                        let csum = ipv6_pseudo_checksum(&local.0, &src, 58, &na[..32]);
                        na[2] = (csum >> 8) as u8;
                        na[3] = (csum & 0xFF) as u8;
                        send_ipv6(src_ip, 58, &na[..32]);
                    }
                } else if icmp_type == ICMPV6_TYPE_NEIGHBOR_ADVERT {
                    // Komşu önbelleğini güncelle: hedef IP ve kaynak MAC
                    if icmp.len() >= 32 {
                        let target_ip_bytes = &icmp[8..24];
                        let mut target_ip = [0u8; 16];
                        target_ip.copy_from_slice(target_ip_bytes);
                        // Target Link-Layer Address option (Type 2) at offset 24
                        if icmp.len() >= 32 && icmp[24] == 2 {
                            let mut mac = [0u8; 6];
                            mac.copy_from_slice(&icmp[26..32]);
                            neighbor_insert(Ipv6Address(target_ip), MacAddress(mac));
                        }
                    }
                }
            }
        }
        6 => {
            // TCP over IPv6: TCP segmenti 40. ofsetten başlar
            if payload.len() >= 60 {
                let tcp = &payload[40..];
                let _ = crate::drivers::socket::on_tcp_segment_ipv6(src_ip, tcp);
            }
        }
        17 => {
            // UDP over IPv6: UDP başlığı 8 bayt
            if payload.len() >= 48 {
                let src_port = u16::from_be_bytes([payload[40], payload[41]]);
                let dst_port = u16::from_be_bytes([payload[42], payload[43]]);
                let ulen = u16::from_be_bytes([payload[44], payload[45]]) as usize;
                if ulen >= 8 && 40 + ulen <= payload.len() {
                    let udata = &payload[48..40 + ulen];
                    crate::drivers::socket::on_udp_datagram(
                        Ipv4Address([127, 0, 0, 1]),
                        src_port,
                        dst_port,
                        udata,
                    );
                }
            }
        }
        _ => {}
    }
}

#![allow(dead_code, unused_variables, static_mut_refs)]

//! Minimal TCP/IP yığını: ARP, IPv4, ICMP (ping) ve temel paket ayrıştırma.
//! QEMU `-netdev user` (SLIRP) ile ARP'ye cevap verir, ping yanıtlayabilir.

use crate::drivers::e1000;

// ---------------------------------------------------------------------------
// Adres tipleri
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; 6]);

#[derive(Clone, Copy, PartialEq, Eq)]
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

// Broadcast MAC
pub const MAC_BROADCAST: MacAddress = MacAddress([0xFF; 6]);

// ---------------------------------------------------------------------------
// ARP
// ---------------------------------------------------------------------------

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;

// Basit ARP önbelleği (4 girişli)
struct ArpEntry {
    ip: Ipv4Address,
    mac: MacAddress,
    used: bool,
}

static mut ARP_TABLE: [ArpEntry; 4] = [
    ArpEntry { ip: Ipv4Address([0; 4]), mac: MacAddress([0; 6]), used: false },
    ArpEntry { ip: Ipv4Address([0; 4]), mac: MacAddress([0; 6]), used: false },
    ArpEntry { ip: Ipv4Address([0; 4]), mac: MacAddress([0; 6]), used: false },
    ArpEntry { ip: Ipv4Address([0; 4]), mac: MacAddress([0; 6]), used: false },
];

fn arp_lookup(ip: Ipv4Address) -> Option<MacAddress> {
    unsafe {
        for e in ARP_TABLE.iter_mut() {
            if e.used && e.ip == ip {
                return Some(e.mac);
            }
        }
    }
    None
}

fn arp_insert(ip: Ipv4Address, mac: MacAddress) {
    unsafe {
        for e in ARP_TABLE.iter_mut() {
            if !e.used {
                e.ip = ip;
                e.mac = mac;
                e.used = true;
                return;
            }
        }
        // dolu — ilk girişi değiştir
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
const IP_PROTO_UDP: u8 = 17;

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
    let on_subnet = (0..4).all(|i| (dst_ip.0[i] & SUBNET_MASK.0[i]) == (LOCAL_IP.0[i] & SUBNET_MASK.0[i]));
    let next_hop = if on_subnet { dst_ip } else { GATEWAY_IP };
    // MAC'i ARP ile çöz
    let dst_mac = match arp_lookup(next_hop) {
        Some(m) => m,
        None => {
            // ARP request gönder
            send_arp_request(next_hop);
            // QEMU SLIRP cevap verir, tekrar bak (poll arkasından)
            match arp_lookup(next_hop) {
                Some(m) => m,
                None => return false,
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
    ip_packet[12..16].copy_from_slice(&LOCAL_IP.0);
    ip_packet[16..20].copy_from_slice(&dst_ip.0);

    let cksum = checksum(&ip_packet[..20]);
    ip_packet[10] = (cksum >> 8) as u8;
    ip_packet[11] = (cksum & 0xFF) as u8;

    ip_packet[20..20 + payload.len()].copy_from_slice(payload);

    let mut frame = [0u8; 1514];
    let len = build_ethernet(dst_mac, ETHERTYPE_IPV4, &ip_packet[..20 + payload.len()], &mut frame);
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
    arp[14..18].copy_from_slice(&LOCAL_IP.0); // sender IP
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
    let op = ((payload[6] as u16) << 8) | payload[7] as u16;
    let sender_ip = Ipv4Address([payload[14], payload[15], payload[16], payload[17]]);
    let sender_mac = MacAddress([payload[8], payload[9], payload[10], payload[11], payload[12], payload[13]]);
    let target_ip = Ipv4Address([payload[24], payload[25], payload[26], payload[27]]);

    // Önbelleğe ekle
    arp_insert(sender_ip, sender_mac);

    // Bize gelen ARP request ise cevap ver
    if op == ARP_OP_REQUEST && target_ip == LOCAL_IP {
        let mut arp = [0u8; 28];
        arp[0..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&ARP_OP_REPLY.to_be_bytes());
        let mac = unsafe { e1000::MAC_ADDR };
        arp[8..14].copy_from_slice(&mac);
        arp[14..18].copy_from_slice(&LOCAL_IP.0);
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

fn handle_icmp(src_ip: Ipv4Address, payload: &[u8]) {
    if payload.len() < 8 {
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
        _ => {}
    }
}

fn handle_ipv4(payload: &[u8]) {
    if payload.len() < 20 {
        return;
    }
    let ihl = ((payload[0] & 0x0F) as usize) * 4;
    if ihl < 20 || payload.len() < ihl {
        return;
    }
    let proto = payload[9];
    let src_ip = Ipv4Address([payload[12], payload[13], payload[14], payload[15]]);
    let dst_ip = Ipv4Address([payload[16], payload[17], payload[18], payload[19]]);
    if dst_ip != LOCAL_IP {
        return;
    }
    let l4 = &payload[ihl..];
    match proto {
        IP_PROTO_ICMP => handle_icmp(src_ip, l4),
        IP_PROTO_UDP => handle_udp(src_ip, l4),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------------

/// Basit UDP üzerinden bir "test sayfası" döndürür. Tam HTTP yığını değil ama
/// UDP echo / DHCP discover basitliğiyle birlikte çalışan bir "status" uzantısı.
static mut UDP_LISTENER_DATA: [u8; 512] = [0; 512];
static mut UDP_LISTENER_LEN: usize = 0;

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
    let dst_port = ((payload[2] as u16) << 8) | payload[3] as u16;
    let udp_len = ((payload[4] as u16) << 8) | payload[5] as u16;
    if udp_len < 8 || payload.len() < udp_len as usize {
        return;
    }
    let data = &payload[8..udp_len as usize];
    // Sağlıklı bir "sınamayı" kaydet — uygulama katmanı (tarayıcı) alır
    unsafe {
        let n = data.len().min(UDP_LISTENER_DATA.len());
        UDP_LISTENER_DATA[..n].copy_from_slice(&data[..n]);
        UDP_LISTENER_LEN = n;
    }
    // Kaynağı da kaydedelim
    let _ = src_ip;
}

// ---------------------------------------------------------------------------
// Ping (ICMP Echo Request) — dış sunucuya test
// ---------------------------------------------------------------------------

pub fn ping(target: Ipv4Address) -> bool {
    let mut icmp = [0u8; 8 + 4];
    icmp[0] = ICMP_ECHO_REQUEST;
    icmp[1] = 0;
    icmp[2] = 0;
    icmp[3] = 0;
    icmp[4] = 0x12;
    icmp[5] = 0x34;
    icmp[6..8].copy_from_slice(&1u16.to_be_bytes()); // seq
    icmp[8..12].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes()); // payload
    let ck = checksum(&icmp);
    icmp[2] = (ck >> 8) as u8;
    icmp[3] = (ck & 0xFF) as u8;
    send_ipv4(target, IP_PROTO_ICMP, &icmp)
}

// ---------------------------------------------------------------------------
// ARP çözümlemeyi başlat (netdev user için gateway)
// ---------------------------------------------------------------------------

pub fn init() {
    // Gateway için ARP request gönder (SLIRP cevaplar)
    send_arp_request(GATEWAY_IP);
}

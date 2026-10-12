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
static mut ACTIVE_LOCAL_IP: Ipv4Address = LOCAL_IP;
static mut ACTIVE_GATEWAY_IP: Ipv4Address = GATEWAY_IP;
static mut ACTIVE_DNS_IP: Ipv4Address = DNS_SERVER;
static mut ACTIVE_SUBNET_MASK: Ipv4Address = SUBNET_MASK;
static mut DHCP_OFFER: Option<(Ipv4Address, Ipv4Address, Ipv4Address)> = None;
static mut DHCP_SERVER_IP: Ipv4Address = GATEWAY_IP;
static mut DHCP_READY: bool = false;
static mut DHCP_XID: u32 = 0x4D4F5301;
pub fn local_ip() -> Ipv4Address { unsafe { ACTIVE_LOCAL_IP } }
pub fn gateway_ip() -> Ipv4Address { unsafe { ACTIVE_GATEWAY_IP } }
pub fn dns_server() -> Ipv4Address { unsafe { ACTIVE_DNS_IP } }
pub fn dhcp_ready() -> bool { unsafe { DHCP_READY } }
pub fn subnet_mask() -> Ipv4Address { unsafe { ACTIVE_SUBNET_MASK } }

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
pub const IP_PROTO_TCP: u8 = 6;
const IP_PROTO_UDP: u8 = 17;
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
    let local = local_ip(); let gateway = gateway_ip();
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
                    return false;
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
    let op = ((payload[6] as u16) << 8) | payload[7] as u16;
    let sender_ip = Ipv4Address([payload[14], payload[15], payload[16], payload[17]]);
    let sender_mac = MacAddress([payload[8], payload[9], payload[10], payload[11], payload[12], payload[13]]);
    let target_ip = Ipv4Address([payload[24], payload[25], payload[26], payload[27]]);

    // Önbelleğe ekle
    arp_insert(sender_ip, sender_mac);

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
    let total = u16::from_be_bytes([payload[2], payload[3]]) as usize;
    let end = total.min(payload.len());
    if end < ihl { return; }
    let is_dhcp = proto == IP_PROTO_UDP && end >= ihl + 4 && u16::from_be_bytes([payload[ihl+2],payload[ihl+3]]) == 68;
    if dst_ip != local_ip() && !is_dhcp {
        return;
    }
    let l4 = &payload[ihl..end];
    match proto {
        IP_PROTO_ICMP => handle_icmp(src_ip, l4),
        IP_PROTO_UDP => handle_udp(src_ip, l4),
        IP_PROTO_TCP => handle_tcp(src_ip, l4),
        _ => {}
    }
}

fn transport_checksum(src: Ipv4Address, dst: Ipv4Address, proto: u8, data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for pair in src.0.chunks(2).chain(dst.0.chunks(2)) {
        sum += ((pair[0] as u32) << 8) | pair[1] as u32;
    }
    sum += proto as u32;
    sum += data.len() as u32;
    let mut i = 0;
    while i + 1 < data.len() { sum += ((data[i] as u32) << 8) | data[i + 1] as u32; i += 2; }
    if i < data.len() { sum += (data[i] as u32) << 8; }
    while sum >> 16 != 0 { sum = (sum & 0xffff) + (sum >> 16); }
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
static mut TCP_DATA: [u8; 4096] = [0; 4096];
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
    let dst_port = ((payload[2] as u16) << 8) | payload[3] as u16;
    let udp_len = ((payload[4] as u16) << 8) | payload[5] as u16;
    if udp_len < 8 || payload.len() < udp_len as usize {
        return;
    }
    let data = &payload[8..udp_len as usize];
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    if dst_port==68 && src_port==67 { handle_dhcp(src_ip,data); return; }
    if src_port == DNS_PORT && data.len() >= 12 {
        let id = u16::from_be_bytes([data[0], data[1]]);
        let flags = u16::from_be_bytes([data[2], data[3]]);
        if id == 0x4D4D && flags & 0x8000 != 0 && flags & 0x000F == 0 {
            unsafe {
                let n = data.len().min(DNS_REPLY_DATA.len());
                DNS_REPLY_DATA[..n].copy_from_slice(&data[..n]);
                DNS_REPLY_LEN = n;
                if let Some(ip) = dns_parse_a(&data[..n], id) { DNS_REPLY_IP = ip.0; }
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
    if payload.len() < 20 { return; }
    let off = ((payload[12] >> 4) as usize) * 4;
    if off < 20 || payload.len() < off { return; }
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[13];
    unsafe {
        if dst_port != TCP_LOCAL_PORT || src_port != TCP_REMOTE_PORT || src_ip != TCP_REMOTE_IP { return; }
        if flags & 0x04 != 0 { TCP_RESET = true; TCP_CONNECTED = false; return; }
        let has_syn = flags & 0x02 != 0;
        let has_ack = flags & 0x10 != 0;
        let has_fin = flags & 0x01 != 0;
        let data = &payload[off..];
            if has_syn && has_ack && !TCP_SYN_ACKED {
            TCP_REMOTE_SEQ = seq.wrapping_add(1);
            TCP_LOCAL_SEQ = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
            TCP_SYN_ACKED = true;
            TCP_CONNECTED = true;
            let _ = send_tcp_segment(TCP_REMOTE_IP, TCP_REMOTE_PORT, TCP_LOCAL_PORT, TCP_LOCAL_SEQ, TCP_REMOTE_SEQ, 0x10, &[]);
        } else if TCP_CONNECTED && has_ack {
            let ack = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
            if ack.wrapping_sub(TCP_LOCAL_SEQ) < 0x8000_0000 { TCP_LOCAL_SEQ = ack; }
        }
            if TCP_CONNECTED && !has_ack && !has_syn {
                let _ = send_tcp_segment(TCP_REMOTE_IP, TCP_REMOTE_PORT, TCP_LOCAL_PORT, TCP_LOCAL_SEQ, TCP_REMOTE_SEQ, 0x10, &[]);
            }
        if TCP_CONNECTED && !data.is_empty() {
            let room = TCP_DATA.len().saturating_sub(TCP_DATA_LEN);
            let n = data.len().min(room);
            TCP_DATA[TCP_DATA_LEN..TCP_DATA_LEN+n].copy_from_slice(&data[..n]);
            TCP_DATA_LEN += n;
            TCP_REMOTE_SEQ = seq.wrapping_add(data.len() as u32);
            let _ = send_tcp_segment(TCP_REMOTE_IP, TCP_REMOTE_PORT, TCP_LOCAL_PORT, TCP_LOCAL_SEQ, TCP_REMOTE_SEQ, 0x10, &[]);
        }
        if TCP_CONNECTED && has_fin {
            TCP_REMOTE_SEQ = seq.wrapping_add(data.len() as u32).wrapping_add(1);
            TCP_FIN_RECEIVED = true;
            let _ = send_tcp_segment(TCP_REMOTE_IP, TCP_REMOTE_PORT, TCP_LOCAL_PORT, TCP_LOCAL_SEQ, TCP_REMOTE_SEQ, 0x11, &[]);
            TCP_CONNECTED = false;
        }
    }
}

fn dns_parse_a(data: &[u8], id: u16) -> Option<Ipv4Address> {
    if data.len() < 12 || u16::from_be_bytes([data[0], data[1]]) != id { return None; }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 || flags & 0x000F != 0 { return None; }
    let qd = u16::from_be_bytes([data[4], data[5]]) as usize;
    let an = u16::from_be_bytes([data[6], data[7]]) as usize;
    let mut p = 12usize;
    for _ in 0..qd {
        loop { if p >= data.len() { return None; } let n=data[p] as usize; p+=1; if n==0 { break; } if n&0xC0==0 || p+n>data.len(){return None;} p+=n; }
        if p+4>data.len(){return None;} p+=4;
    }
    for _ in 0..an {
        if p+2>data.len(){return None;}
        if data[p]&0xC0==0xC0 { p+=2; } else { loop { if p>=data.len(){return None;} let n=data[p] as usize; p+=1; if n==0{break;} if p+n>data.len(){return None;} p+=n; } }
        if p+10>data.len(){return None;}
        let typ=u16::from_be_bytes([data[p],data[p+1]]); let class=u16::from_be_bytes([data[p+2],data[p+3]]); let rdlen=u16::from_be_bytes([data[p+8],data[p+9]]) as usize; p+=10;
        if p+rdlen>data.len(){return None;}
        if typ==1 && class==1 && rdlen==4 { return Some(Ipv4Address([data[p],data[p+1],data[p+2],data[p+3]])); }
        p+=rdlen;
    }
    None
}

pub fn take_dns_result() -> Option<Ipv4Address> { unsafe { if DNS_REPLY_LEN==0 { None } else { DNS_REPLY_LEN=0; Some(Ipv4Address(DNS_REPLY_IP)) } } }
pub fn take_dns_packet(buf: &mut [u8]) -> usize { unsafe { let n=DNS_REPLY_LEN.min(buf.len()); buf[..n].copy_from_slice(&DNS_REPLY_DATA[..n]); DNS_REPLY_LEN=0; n } }
pub fn take_tcp_data(buf: &mut [u8]) -> usize { unsafe { let n=TCP_DATA_LEN.min(buf.len()); buf[..n].copy_from_slice(&TCP_DATA[..n]); if n<TCP_DATA_LEN { TCP_DATA.copy_within(n..TCP_DATA_LEN,0); } TCP_DATA_LEN-=n; n } }
pub fn tcp_connected() -> bool { unsafe { TCP_CONNECTED } }
pub fn tcp_syn_acked() -> bool { unsafe { TCP_SYN_ACKED } }
pub fn tcp_finished() -> bool { unsafe { TCP_FIN_RECEIVED } }
pub fn tcp_reset_received() -> bool { unsafe { TCP_RESET } }
pub fn tcp_clear_result() { unsafe { TCP_DATA_LEN=0; TCP_FIN_RECEIVED=false; TCP_RESET=false; TCP_CONNECTED=false; TCP_SYN_ACKED=false; } }

/// UDP datagram gönderir. IPv4 UDP checksum'i isteğe bağlı olduğu için sıfır bırakılır.
pub fn send_udp(dst: Ipv4Address, src_port: u16, dst_port: u16, data: &[u8]) -> bool {
    if data.len() > 1400 { return false; }
    let mut packet = [0u8; 1500 - 14 - 20];
    let len = 8 + data.len();
    packet[0..2].copy_from_slice(&src_port.to_be_bytes());
    packet[2..4].copy_from_slice(&dst_port.to_be_bytes());
    packet[4..6].copy_from_slice(&(len as u16).to_be_bytes());
    packet[6..8].copy_from_slice(&0u16.to_be_bytes());
    packet[8..len].copy_from_slice(data);
    send_ipv4(dst, IP_PROTO_UDP, &packet[..len])
}

/// DNS A sorgusu oluşturup gönderir; yanıt `take_udp_data` ile okunabilir.
pub fn dns_query(host: &str) -> bool {
    let bytes = host.as_bytes();
    if bytes.is_empty() || bytes.len() > 96 { return false; }
    let mut q = [0u8; 256];
    q[0..2].copy_from_slice(&0x4D4Du16.to_be_bytes());
    q[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
    q[4..6].copy_from_slice(&1u16.to_be_bytes());
    let mut p = 12; let mut start = 0;
    for i in 0..=bytes.len() { if i == bytes.len() || bytes[i] == b'.' { let n=i-start; if n==0 || n>63 { return false; } q[p]=n as u8; p+=1; q[p..p+n].copy_from_slice(&bytes[start..i]); p+=n; start=i+1; } }
    q[p]=0; p+=1; q[p..p+2].copy_from_slice(&1u16.to_be_bytes()); p+=2; q[p..p+2].copy_from_slice(&1u16.to_be_bytes()); p+=2;
    unsafe { DNS_REPLY_LEN=0; DNS_REPLY_IP=[0;4]; }
    send_udp(dns_server(), 49152, DNS_PORT, &q[..p])
}

pub fn resolve_ipv4(host: &str) -> Option<Ipv4Address> {
    if let Some(ip)=parse_ipv4(host) { return Some(ip); }
    if !dns_query(host) { return None; }
    let mut spins=0usize;
    loop {
        e1000::poll();
        if let Some(ip)=take_dns_result() { return Some(ip); }
        spins+=1; if spins>=5_000_000 { return None; }
        core::hint::spin_loop();
    }
}

fn parse_ipv4(host: &str) -> Option<Ipv4Address> {
    let mut parts=host.split('.'); let mut ip=[0u8;4];
    for octet in &mut ip { *octet=parts.next()?.parse().ok()?; }
    if parts.next().is_some(){None}else{Some(Ipv4Address(ip))}
}

fn send_tcp_segment(dst: Ipv4Address, dst_port:u16, src_port:u16, seq:u32, ack:u32, flags:u8, data:&[u8]) -> bool {
    if data.len()>1300{return false;}
    let mut seg=[0u8;1400];
    seg[0..2].copy_from_slice(&src_port.to_be_bytes()); seg[2..4].copy_from_slice(&dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&seq.to_be_bytes()); seg[8..12].copy_from_slice(&ack.to_be_bytes());
    seg[12]=0x50; seg[13]=flags; seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    seg[20..20+data.len()].copy_from_slice(data);
    let n=20+data.len(); let c=transport_checksum(local_ip(),dst,IP_PROTO_TCP,&seg[..n]); seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst,IP_PROTO_TCP,&seg[..n])
}

pub fn tcp_begin(dst: Ipv4Address, dst_port:u16, src_port:u16) -> bool {
    unsafe { TCP_LOCAL_PORT=src_port; TCP_REMOTE_PORT=dst_port; TCP_REMOTE_IP=dst; TCP_LOCAL_SEQ=0x1020_3040; TCP_REMOTE_SEQ=0; TCP_DATA_LEN=0; TCP_CONNECTED=false; TCP_SYN_ACKED=false; TCP_FIN_RECEIVED=false; TCP_RESET=false; }
    send_tcp_segment(dst,dst_port,src_port,0x1020_3040,0,0x02,&[])
}

pub fn tcp_send(data:&[u8]) -> bool { unsafe { if !TCP_CONNECTED{return false;} let ok=send_tcp_segment(TCP_REMOTE_IP,TCP_REMOTE_PORT,TCP_LOCAL_PORT,TCP_LOCAL_SEQ,TCP_REMOTE_SEQ,0x18,data); if ok { TCP_LOCAL_SEQ=TCP_LOCAL_SEQ.wrapping_add(data.len() as u32); } ok } }

pub fn tcp_close() -> bool { unsafe { if !TCP_CONNECTED{return false;} let ok=send_tcp_segment(TCP_REMOTE_IP,TCP_REMOTE_PORT,TCP_LOCAL_PORT,TCP_LOCAL_SEQ,TCP_REMOTE_SEQ,0x11,&[]); if ok { TCP_LOCAL_SEQ=TCP_LOCAL_SEQ.wrapping_add(1); TCP_CONNECTED=false; } ok } }

/// TCP SYN gönderir; gelen TCP yükleri `take_udp_data` üzerinden alınabilir.
pub fn tcp_connect(dst: Ipv4Address, dst_port: u16, src_port: u16) -> bool {
    let mut seg = [0u8; 20];
    seg[0..2].copy_from_slice(&src_port.to_be_bytes()); seg[2..4].copy_from_slice(&dst_port.to_be_bytes());
    seg[4..8].copy_from_slice(&0x1000u32.to_be_bytes()); seg[12] = 0x50; seg[13] = 0x02;
    seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    let c = transport_checksum(local_ip(), dst, IP_PROTO_TCP, &seg); seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst, IP_PROTO_TCP, &seg)
}

/// Bağlantı kurulmuş bir TCP akışına HTTP/1.0 GET isteği hazırlayıp gönderir.
/// `seq` ve `ack` çağıranın takip ettiği TCP sıra numaralarıdır.
pub fn http_get(dst: Ipv4Address, host: &str, path: &str, seq: u32, ack: u32) -> bool {
    let hb = host.as_bytes(); let pb = path.as_bytes();
    if hb.len() > 96 || pb.len() > 180 { return false; }
    let mut seg = [0u8; 620];
    let req = b"GET "; let mut n = 0;
    seg[20..24].copy_from_slice(req); n += 4;
    seg[20+n..20+n+pb.len()].copy_from_slice(pb); n += pb.len();
    let tail = b" HTTP/1.0\r\nHost: "; seg[20+n..20+n+tail.len()].copy_from_slice(tail); n += tail.len();
    seg[20+n..20+n+hb.len()].copy_from_slice(hb); n += hb.len();
    let end = b"\r\nConnection: close\r\n\r\n"; seg[20+n..20+n+end.len()].copy_from_slice(end); n += end.len();
    seg[0..2].copy_from_slice(&49153u16.to_be_bytes()); seg[2..4].copy_from_slice(&HTTP_PORT.to_be_bytes());
    seg[4..8].copy_from_slice(&seq.to_be_bytes()); seg[8..12].copy_from_slice(&ack.to_be_bytes());
    seg[12] = 0x50; seg[13] = 0x18; seg[14..16].copy_from_slice(&64240u16.to_be_bytes());
    let total = 20+n; let c = transport_checksum(local_ip(), dst, IP_PROTO_TCP, &seg[..total]); seg[16..18].copy_from_slice(&c.to_be_bytes());
    send_ipv4(dst, IP_PROTO_TCP, &seg[..total])
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
    // QEMU SLIRP varsayılan gateway ve DNS MAC eşleştirmesini önceden kaydet
    arp_insert(GATEWAY_IP, MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x02]));
    arp_insert(DNS_SERVER, MacAddress([0x52, 0x55, 0x0A, 0x00, 0x02, 0x03]));
    // Canlı ARP güncellemesi için gateway'e request gönder
    send_arp_request(GATEWAY_IP);
}

pub fn dhcp_request() -> bool {
    unsafe { DHCP_READY=false; DHCP_OFFER=None; }
    let mac=unsafe{e1000::MAC_ADDR}; let mut p=[0u8;300];
    p[0]=1;p[1]=1;p[2]=6; p[4..8].copy_from_slice(&unsafe{DHCP_XID}.to_be_bytes()); p[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99,130,83,99]); let mut n=240;
    p[n]=53;p[n+1]=1;p[n+2]=1;n+=3;
    p[n]=55;p[n+1]=4;p[n+2]=1;p[n+3]=3;p[n+4]=6;p[n+5]=51;n+=6;
    p[n]=61;p[n+1]=7;p[n+2]=1;p[n+3..n+9].copy_from_slice(&mac);n+=9;
    p[n]=255;n+=1;
    send_dhcp_bootstrap(&p[..n])
}

fn send_dhcp_bootstrap(data:&[u8])->bool {
    if data.len()>1200{return false;}
    let mut udp=[0u8;1210];let len=data.len()+8;udp[0..2].copy_from_slice(&68u16.to_be_bytes());udp[2..4].copy_from_slice(&67u16.to_be_bytes());udp[4..6].copy_from_slice(&(len as u16).to_be_bytes());udp[8..len].copy_from_slice(data);
    let mut ip=[0u8;1230];let total=len+20;ip[0]=0x45;ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());ip[6]=0x40;ip[8]=64;ip[9]=IP_PROTO_UDP;ip[16..20].copy_from_slice(&[255;4]);let c=checksum(&ip[..20]);ip[10..12].copy_from_slice(&c.to_be_bytes());ip[20..total].copy_from_slice(&udp[..len]);
    let mut frame=[0u8;1260];let n=build_ethernet(MAC_BROADCAST,ETHERTYPE_IPV4,&ip[..total],&mut frame);e1000::transmit(&frame[..n])
}

fn handle_dhcp(src:Ipv4Address,data:&[u8]) {
    if data.len()<240 || data[0]!=2 || data[1]!=1 || data[2]!=6 {return;}
    let xid=u32::from_be_bytes([data[4],data[5],data[6],data[7]]); if xid!=unsafe{DHCP_XID} {return;}
    let offered=Ipv4Address([data[16],data[17],data[18],data[19]]);
    let mut mask=SUBNET_MASK; let mut router=src; let mut dns=DNS_SERVER; let mut server=src; let mut msg=0u8; let mut p=240;
    while p<data.len() { let tag=data[p];p+=1;if tag==255{break;}if tag==0{continue;}if p>=data.len(){return;}let len=data[p] as usize;p+=1;if p+len>data.len(){return;}let v=&data[p..p+len];p+=len;match tag {53 if len==1=>msg=v[0],1 if len>=4=>mask=Ipv4Address([v[0],v[1],v[2],v[3]]),3 if len>=4=>router=Ipv4Address([v[0],v[1],v[2],v[3]]),6 if len>=4=>dns=Ipv4Address([v[0],v[1],v[2],v[3]]),54 if len>=4=>server=Ipv4Address([v[0],v[1],v[2],v[3]]),_=>{}}}
    if msg==2 { unsafe{DHCP_OFFER=Some((offered,router,dns));DHCP_SERVER_IP=server;} send_dhcp_request(offered); }
    else if msg==5 { unsafe{ACTIVE_LOCAL_IP=offered;ACTIVE_GATEWAY_IP=router;ACTIVE_DNS_IP=dns;ACTIVE_SUBNET_MASK=mask;DHCP_READY=true;} send_arp_request(router); }
}

fn send_dhcp_request(offered:Ipv4Address) {
    let mac=unsafe{e1000::MAC_ADDR};let server=unsafe{DHCP_SERVER_IP};let mut p=[0u8;300];p[0]=1;p[1]=1;p[2]=6;p[4..8].copy_from_slice(&unsafe{DHCP_XID}.to_be_bytes());p[10..12].copy_from_slice(&0x8000u16.to_be_bytes());p[28..34].copy_from_slice(&mac);p[236..240].copy_from_slice(&[99,130,83,99]);let mut n=240;p[n]=53;p[n+1]=1;p[n+2]=3;n+=3;p[n]=50;p[n+1]=4;p[n+2..n+6].copy_from_slice(&offered.0);n+=6;p[n]=54;p[n+1]=4;p[n+2..n+6].copy_from_slice(&server.0);n+=6;p[n]=61;p[n+1]=7;p[n+2]=1;p[n+3..n+9].copy_from_slice(&mac);n+=9;p[n]=255;n+=1;let _=send_dhcp_bootstrap(&p[..n]);
}

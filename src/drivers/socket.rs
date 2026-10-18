//! BSD tarzı socket API: `socket()` / `connect()` / `send()` / `recv()` / `close()`
//! + UDP `sendto()` / `recvfrom()` + DNS önbellekli `resolve_host()`.
//!
//! TCP, yığın tarafında soket başına durum makinesi olarak çalışır: her soketin
//! kendi sıra numaraları (ISS/snd_nxt/rcv_nxt), RX tamponu ve yeniden iletim
//! (retransmit) zamanlayıcısı vardır. Yeniden iletimler hem bloklayan çağrıların
//! içindeki bekleme döngülerinde hem de ana döngüden çağrılan `poll_timeouts()`
//! içinde ateşlenir. Gelen TCP segmentleri önce buraya (`on_tcp_segment`)
//! danışılır, eşleşme yoksa eski tek-bağlantılı yola bırakılır.
#![allow(dead_code, static_mut_refs)]

use crate::drivers::e1000;
use crate::drivers::netstack::{self, Ipv4Address};

pub const AF_INET: i32 = 2;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;

// BSD errno benzeri negatif kodlar (dönen Result'un Err tarafı).
pub const E_BADFD: i32 = 9;
pub const E_INVAL: i32 = 22;
pub const E_NOMEM: i32 = 12;
pub const E_NOTCONN: i32 = 107;
pub const E_CONNREFUSED: i32 = 111;
pub const E_TIMEOUT: i32 = 110;
pub const E_NOSUPPORT: i32 = 95;
pub const E_BUSY: i32 = 16;

pub const MAX_SOCKETS: usize = 8;
pub const RX_BUF: usize = 8192;
pub const MSS: usize = 1300;
pub const DEFAULT_TIMEOUT_MS: u64 = 8000;
// Eski tek-bağlantılı yolun kullandığı portlardan uzak dur.
const EPHEMERAL_BASE: u16 = 50000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SockType {
    Stream,
    Dgram,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SockState {
    Free,
    SynSent,
    Established,
    /// FIN gönderildi, karşı tarafın ACK/FIN'i bekleniyor.
    FinWait,
    /// Karşı taraf FIN gönderdi, ACK'ledik; okuma EOF verecek.
    CloseWait,
    Closed,
    Error,
}

struct Retry {
    kind: u8, // 0=yok, 1=SYN, 2=veri, 3=FIN
    at_ms: u64,
    interval_ms: u64,
}

pub struct Socket {
    used: bool,
    sock_type: SockType,
    state: SockState,
    local_port: u16,
    remote_port: u16,
    remote_ip: Ipv4Address,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    rx: [u8; RX_BUF],
    rx_len: usize,
    // Onay bekleyen son gönderilen parça (stop-and-wait).
    unacked: [u8; MSS],
    unacked_len: usize,
    unacked_seq: u32,
    fin_seq: u32,
    // UDP için son datagramın kaynağı.
    udp_peer_ip: Ipv4Address,
    udp_peer_port: u16,
    udp_pending: bool,
    timeout_ms: u64,
    retry: Retry,
    error: i32,
}

impl Socket {
    const fn empty() -> Self {
        Self {
            used: false,
            sock_type: SockType::Stream,
            state: SockState::Free,
            local_port: 0,
            remote_port: 0,
            remote_ip: Ipv4Address([0; 4]),
            iss: 0,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            rx: [0; RX_BUF],
            rx_len: 0,
            unacked: [0; MSS],
            unacked_len: 0,
            unacked_seq: 0,
            fin_seq: 0,
            udp_peer_ip: Ipv4Address([0; 4]),
            udp_peer_port: 0,
            udp_pending: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            retry: Retry { kind: 0, at_ms: 0, interval_ms: 500 },
            error: 0,
        }
    }
}

static mut SOCKETS: [Socket; MAX_SOCKETS] = [
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
    Socket::empty(),
];
static mut NEXT_EPHEMERAL: u16 = EPHEMERAL_BASE;

fn sock_mut(fd: i32) -> Option<&'static mut Socket> {
    if fd < 0 || (fd as usize) >= MAX_SOCKETS {
        return None;
    }
    unsafe {
        let s = &mut SOCKETS[fd as usize];
        if s.used {
            Some(s)
        } else {
            None
        }
    }
}

fn alloc_ephemeral() -> u16 {
    unsafe {
        for _ in 0..1024 {
            let p = NEXT_EPHEMERAL;
            NEXT_EPHEMERAL = if NEXT_EPHEMERAL >= 60999 {
                EPHEMERAL_BASE
            } else {
                NEXT_EPHEMERAL + 1
            };
            let mut clash = false;
            for s in SOCKETS.iter() {
                if s.used && s.local_port == p {
                    clash = true;
                    break;
                }
            }
            if !clash {
                return p;
            }
        }
        0
    }
}

/// `socket(AF_INET, SOCK_STREAM|SOCK_DGRAM)` — boş bir tanımlayıcı döner.
pub fn socket(_domain: i32, stype: i32) -> Result<i32, i32> {
    let t = match stype {
        SOCK_STREAM => SockType::Stream,
        SOCK_DGRAM => SockType::Dgram,
        _ => return Err(E_NOSUPPORT),
    };
    unsafe {
        for (i, s) in SOCKETS.iter_mut().enumerate() {
            if !s.used {
                *s = Socket::empty();
                s.used = true;
                s.sock_type = t;
                s.state = SockState::Closed;
                return Ok(i as i32);
            }
        }
    }
    Err(E_NOMEM)
}

/// Soket bekleme süresini değiştirir (varsayılan 8 sn).
pub fn set_timeout(fd: i32, ms: u64) -> Result<(), i32> {
    match sock_mut(fd) {
        Some(s) => {
            s.timeout_ms = ms.max(200);
            Ok(())
        }
        None => Err(E_BADFD),
    }
}

fn arm_retry(s: &mut Socket, kind: u8) {
    let now = crate::monotonic_ms();
    s.retry.kind = kind;
    s.retry.interval_ms = 500;
    s.retry.at_ms = now + 500;
}

/// Süresi gelen yeniden iletimi ateşler. Bloklayan döngülerden ve
/// `poll_timeouts()` içinden çağrılır.
fn fire_retry(fd: usize) {
    let now = crate::monotonic_ms();
    unsafe {
        let s = &mut SOCKETS[fd];
        if !s.used || s.retry.kind == 0 || now < s.retry.at_ms {
            return;
        }
        match s.retry.kind {
            1 => {
                netstack::send_tcp_segment(
                    s.remote_ip,
                    s.remote_port,
                    s.local_port,
                    s.iss,
                    0,
                    0x02,
                    &[],
                );
            }
            2 => {
                let buf = s.unacked;
                let len = s.unacked_len;
                let seq = s.unacked_seq;
                let ack = s.rcv_nxt;
                netstack::send_tcp_segment(
                    s.remote_ip,
                    s.remote_port,
                    s.local_port,
                    seq,
                    ack,
                    0x18,
                    &buf[..len],
                );
            }
            3 => {
                netstack::send_tcp_segment(
                    s.remote_ip,
                    s.remote_port,
                    s.local_port,
                    s.fin_seq,
                    s.rcv_nxt,
                    0x11,
                    &[],
                );
            }
            _ => return,
        }
        s.retry.interval_ms = (s.retry.interval_ms * 2).min(4000);
        s.retry.at_ms = now + s.retry.interval_ms;
    }
}

/// Tüm soketlerde süresi gelen yeniden iletimleri ateşler.
/// Ana olay döngüsünden (`e1000::poll()` yanından) çağrılır.
pub fn poll_timeouts() {
    for i in 0..MAX_SOCKETS {
        fire_retry(i);
    }
}

pub fn pump_all() {
    e1000::poll();
    poll_timeouts();
}

fn pump(fd: usize) {
    e1000::poll();
    fire_retry(fd);
}

/// `connect(fd, ip, port)` — TCP 3'lü el sıkışması (bloklayan).
pub fn connect(fd: i32, ip: Ipv4Address, port: u16) -> Result<(), i32> {
    if !e1000::is_ready() {
        return Err(E_NOTCONN);
    }
    let s = sock_mut(fd).ok_or(E_BADFD)?;
    if s.sock_type != SockType::Stream {
        return Err(E_INVAL);
    }
    if s.state == SockState::Established {
        return Ok(());
    }
    let lport = alloc_ephemeral();
    if lport == 0 {
        return Err(E_BUSY);
    }
    let iss =
        (crate::monotonic_ms() as u32).wrapping_mul(2654435761).wrapping_add((fd as u32).wrapping_mul(7919) | 0x1000);
    s.local_port = lport;
    s.remote_ip = ip;
    s.remote_port = port;
    s.iss = iss;
    s.snd_una = iss;
    s.snd_nxt = iss.wrapping_add(1);
    s.rcv_nxt = 0;
    s.rx_len = 0;
    s.unacked_len = 0;
    s.error = 0;
    s.state = SockState::SynSent;
    arm_retry(s, 1);
    netstack::send_tcp_segment(ip, port, lport, iss, 0, 0x02, &[]);

    let deadline = crate::monotonic_ms().wrapping_add(s.timeout_ms);
    loop {
        pump(fd as usize);
        let s = sock_mut(fd).ok_or(E_BADFD)?;
        match s.state {
            SockState::Established => {
                s.retry.kind = 0;
                return Ok(());
            }
            SockState::Error => return Err(s.error),
            _ => {}
        }
        if crate::monotonic_ms() >= deadline {
            s.state = SockState::Closed;
            s.retry.kind = 0;
            return Err(E_TIMEOUT);
        }
        core::hint::spin_loop();
    }
}

/// `send(fd, data)` — tamamını göndermeye çalışır, gönderilen baytı döner.
pub fn send(fd: i32, data: &[u8]) -> Result<usize, i32> {
    let total = data.len();
    if total == 0 {
        return Ok(0);
    }
    let mut done = 0;
    while done < total {
        let n = send_chunk(fd, &data[done..])?;
        done += n;
        if n == 0 {
            break;
        }
    }
    Ok(done)
}

fn send_chunk(fd: i32, data: &[u8]) -> Result<usize, i32> {
    let idx = fd as usize;
    let s = sock_mut(fd).ok_or(E_BADFD)?;
    if s.sock_type != SockType::Stream {
        return Err(E_INVAL);
    }
    if s.state == SockState::Error {
        return Err(s.error);
    }
    if s.state != SockState::Established {
        return Err(E_NOTCONN);
    }
    let n = data.len().min(MSS);
    s.unacked[..n].copy_from_slice(&data[..n]);
    s.unacked_len = n;
    s.unacked_seq = s.snd_nxt;
    let end = s.snd_nxt.wrapping_add(n as u32);
    netstack::send_tcp_segment(
        s.remote_ip,
        s.remote_port,
        s.local_port,
        s.snd_nxt,
        s.rcv_nxt,
        0x18,
        &data[..n],
    );
    arm_retry(sock_mut(fd).ok_or(E_BADFD)?, 2);

    let deadline = crate::monotonic_ms().wrapping_add(sock_mut(fd).ok_or(E_BADFD)?.timeout_ms);
    loop {
        pump(idx);
        let s = sock_mut(fd).ok_or(E_BADFD)?;
        if s.state == SockState::Error {
            return Err(s.error);
        }
        if s.state == SockState::CloseWait || s.state == SockState::Closed {
            return Err(E_NOTCONN);
        }
        // Parçanın tamamı onaylandı mı?
        if end.wrapping_sub(s.snd_una) > 0x8000_0000 {
            s.snd_nxt = end;
            s.unacked_len = 0;
            s.retry.kind = 0;
            return Ok(n);
        }
        if crate::monotonic_ms() >= deadline {
            return Err(E_TIMEOUT);
        }
        core::hint::spin_loop();
    }
}

/// `recv(fd, buf)` — veri gelene kadar bekler; `Ok(0)` = karşı taraf kapattı (EOF).
pub fn recv(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    let idx = fd as usize;
    let s = sock_mut(fd).ok_or(E_BADFD)?;
    if s.sock_type != SockType::Stream {
        return Err(E_INVAL);
    }
    if buf.is_empty() {
        return Ok(0);
    }
    let deadline = sock_mut(fd).ok_or(E_BADFD)?.timeout_ms;
    let end = crate::monotonic_ms().wrapping_add(deadline);
    loop {
        pump(idx);
        let s = sock_mut(fd).ok_or(E_BADFD)?;
        if s.sock_type != SockType::Stream {
            return Err(E_INVAL);
        }
        if s.state == SockState::Error {
            return Err(s.error);
        }
        if s.rx_len > 0 {
            let n = s.rx_len.min(buf.len());
            buf[..n].copy_from_slice(&s.rx[..n]);
            s.rx.copy_within(n..s.rx_len, 0);
            s.rx_len -= n;
            return Ok(n);
        }
        if s.state == SockState::CloseWait || s.state == SockState::Closed {
            return Ok(0);
        }
        if crate::monotonic_ms() >= end {
            return Err(E_TIMEOUT);
        }
        core::hint::spin_loop();
    }
}

/// `close(fd)` — FIN gönderir, soketi serbest bırakır.
pub fn close(fd: i32) {
    let Some(s) = sock_mut(fd) else { return };
    if s.sock_type == SockType::Stream
        && (s.state == SockState::Established || s.state == SockState::CloseWait)
    {
        s.fin_seq = s.snd_nxt;
        netstack::send_tcp_segment(
            s.remote_ip,
            s.remote_port,
            s.local_port,
            s.fin_seq,
            s.rcv_nxt,
            0x11,
            &[],
        );
        s.snd_nxt = s.snd_nxt.wrapping_add(1);
        s.state = SockState::FinWait;
        arm_retry(s, 3);
        let deadline = crate::monotonic_ms().wrapping_add(2000);
        while crate::monotonic_ms() < deadline {
            e1000::poll();
            fire_retry(fd as usize);
            let done = unsafe { !SOCKETS[fd as usize].used }
                || matches!(
                    unsafe { SOCKETS[fd as usize].state },
                    SockState::Closed | SockState::Error
                );
            if done {
                break;
            }
            core::hint::spin_loop();
        }
    }
    unsafe {
        SOCKETS[fd as usize] = Socket::empty();
    }
}

// ---------------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------------

/// `sendto(fd, ip, port, data)` — bağlantısız UDP gönderimi.
pub fn sendto(fd: i32, ip: Ipv4Address, port: u16, data: &[u8]) -> Result<usize, i32> {
    let s = sock_mut(fd).ok_or(E_BADFD)?;
    if s.sock_type != SockType::Dgram {
        return Err(E_INVAL);
    }
    if s.local_port == 0 {
        let p = alloc_ephemeral();
        if p == 0 {
            return Err(E_BUSY);
        }
        s.local_port = p;
    }
    if netstack::send_udp(ip, s.local_port, port, data) {
        Ok(data.len())
    } else {
        Err(E_NOTCONN)
    }
}

/// `recvfrom(fd, buf)` — gönderen adresiyle birlikte bir datagram döner.
pub fn recvfrom(fd: i32, buf: &mut [u8]) -> Result<(usize, Ipv4Address, u16), i32> {
    let idx = fd as usize;
    let s = sock_mut(fd).ok_or(E_BADFD)?;
    if s.sock_type != SockType::Dgram {
        return Err(E_INVAL);
    }
    let deadline = crate::monotonic_ms().wrapping_add(s.timeout_ms);
    loop {
        pump(idx);
        let s = sock_mut(fd).ok_or(E_BADFD)?;
        if s.udp_pending {
            let avail = s.rx_len.min(buf.len());
            buf[..avail].copy_from_slice(&s.rx[..avail]);
            s.udp_pending = false;
            s.rx_len = 0;
            return Ok((avail, s.udp_peer_ip, s.udp_peer_port));
        }
        if crate::monotonic_ms() >= deadline {
            return Err(E_TIMEOUT);
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// DNS (önbellekli çözümleme)
// ---------------------------------------------------------------------------

const DNS_CACHE_N: usize = 4;
struct DnsCacheEntry {
    name: [u8; 64],
    name_len: usize,
    ip: Ipv4Address,
    used: bool,
}
static mut DNS_CACHE: [DnsCacheEntry; DNS_CACHE_N] = [
    DnsCacheEntry { name: [0; 64], name_len: 0, ip: Ipv4Address([0; 4]), used: false },
    DnsCacheEntry { name: [0; 64], name_len: 0, ip: Ipv4Address([0; 4]), used: false },
    DnsCacheEntry { name: [0; 64], name_len: 0, ip: Ipv4Address([0; 4]), used: false },
    DnsCacheEntry { name: [0; 64], name_len: 0, ip: Ipv4Address([0; 4]), used: false },
];

fn dns_cache_lookup(host: &str) -> Option<Ipv4Address> {
    unsafe {
        for e in DNS_CACHE.iter() {
            if e.used && e.name_len == host.len() && &e.name[..e.name_len] == host.as_bytes() {
                return Some(e.ip);
            }
        }
    }
    None
}

fn dns_cache_store(host: &str, ip: Ipv4Address) {
    if host.len() > 64 {
        return;
    }
    unsafe {
        for e in DNS_CACHE.iter_mut() {
            if !e.used {
                e.name[..host.len()].copy_from_slice(host.as_bytes());
                e.name_len = host.len();
                e.ip = ip;
                e.used = true;
                return;
            }
        }
        DNS_CACHE[0].name[..host.len()].copy_from_slice(host.as_bytes());
        DNS_CACHE[0].name_len = host.len();
        DNS_CACHE[0].ip = ip;
    }
}

/// Alan adını IPv4'e çözer: önce önbellek, sonra dotted-quad, sonra DNS (5 sn).
pub fn resolve_host(host: &str) -> Result<Ipv4Address, i32> {
    if host.is_empty() || host.len() > 96 {
        return Err(E_INVAL);
    }
    if let Some(ip) = dns_cache_lookup(host) {
        return Ok(ip);
    }
    if let Some(ip) = netstack::parse_ipv4(host) {
        return Ok(ip);
    }
    if !e1000::is_ready() {
        return Err(E_NOTCONN);
    }
    if !netstack::dns_query(host) {
        return Err(E_NOTCONN);
    }
    let deadline = crate::monotonic_ms().wrapping_add(5000);
    loop {
        e1000::poll();
        if let Some(ip) = netstack::take_dns_result() {
            dns_cache_store(host, ip);
            return Ok(ip);
        }
        if crate::monotonic_ms() >= deadline {
            return Err(E_TIMEOUT);
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Gelen paket dağıtımı (netstack'ten çağrılır)
// ---------------------------------------------------------------------------

fn send_ack(s: &Socket) {
    netstack::send_tcp_segment(
        s.remote_ip,
        s.remote_port,
        s.local_port,
        s.snd_nxt,
        s.rcv_nxt,
        0x10,
        &[],
    );
}

/// Gelen bir TCP segmentini soket tablosuna dağıtır. Tüketildiyse `true`.
pub(crate) fn on_tcp_segment(src_ip: Ipv4Address, payload: &[u8]) -> bool {
    if payload.len() < 20 {
        return false;
    }
    if netstack::transport_checksum(src_ip, netstack::local_ip(), netstack::IP_PROTO_TCP, payload) != 0 {
        return false;
    }
    let off = ((payload[12] >> 4) as usize) * 4;
    if off < 20 || payload.len() < off {
        return false;
    }
    let src_port = u16::from_be_bytes([payload[0], payload[1]]);
    let dst_port = u16::from_be_bytes([payload[2], payload[3]]);
    let seq = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let ack = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
    let flags = payload[13];
    let data = &payload[off..];

    unsafe {
        let slot = SOCKETS.iter_mut().find(|s| {
            s.used
                && s.sock_type == SockType::Stream
                && s.local_port == dst_port
                && (s.state == SockState::SynSent
                    || (s.remote_port == src_port && s.remote_ip == src_ip))
        });
        let Some(s) = slot else { return false };
        if flags & 0x04 != 0 {
            s.state = SockState::Error;
            s.error = E_CONNREFUSED;
            s.retry.kind = 0;
            return true;
        }
        match s.state {
            SockState::SynSent => {
                if flags & 0x02 != 0 && flags & 0x10 != 0 && ack == s.iss.wrapping_add(1) {
                    s.rcv_nxt = seq.wrapping_add(1);
                    s.snd_una = ack;
                    s.snd_nxt = ack;
                    s.remote_ip = src_ip;
                    s.remote_port = src_port;
                    s.state = SockState::Established;
                    s.retry.kind = 0;
                    send_ack(s);
                }
                true
            }
            SockState::Established | SockState::FinWait | SockState::CloseWait => {
                // ACK ilerlemesi (veri + FIN onayları dahil): ack, gönderilip
                // henüz onaylanmamış [snd_una, snd_nxt] aralığındaysa ilerlet.
                if flags & 0x10 != 0 {
                    let forward = ack.wrapping_sub(s.snd_una);
                    let outstanding = s.snd_nxt.wrapping_sub(s.snd_una);
                    if outstanding < 0x8000_0000 && forward <= outstanding {
                        s.snd_una = ack;
                        if s.retry.kind == 2 && s.unacked_len > 0 {
                            let end = s.unacked_seq.wrapping_add(s.unacked_len as u32);
                            if end.wrapping_sub(ack) > 0x8000_0000 {
                                s.unacked_len = 0;
                                s.retry.kind = 0;
                            }
                        }
                        if s.state == SockState::FinWait && ack == s.snd_nxt {
                            s.retry.kind = 0;
                            s.state = SockState::Closed;
                            return true;
                        }
                    }
                }
                if s.state == SockState::FinWait {
                    return true;
                }
                // Sıralı veri: tampona al, onayla.
                if !data.is_empty() && seq == s.rcv_nxt {
                    let room = RX_BUF.saturating_sub(s.rx_len);
                    let n = data.len().min(room);
                    if n > 0 {
                        s.rx[s.rx_len..s.rx_len + n].copy_from_slice(&data[..n]);
                        s.rx_len += n;
                        s.rcv_nxt = s.rcv_nxt.wrapping_add(n as u32);
                    }
                    send_ack(s);
                } else if !data.is_empty() {
                    // Sıra dışı: yinelenen ACK ile göndericiyi uyar.
                    send_ack(s);
                }
                if flags & 0x01 != 0 {
                    // FIN: veriden sonra bir sıra numarası tüketir.
                    if seq.wrapping_add(data.len() as u32) == s.rcv_nxt {
                        s.rcv_nxt = s.rcv_nxt.wrapping_add(1);
                        send_ack(s);
                        s.state = SockState::CloseWait;
                    }
                }
                true
            }
            _ => true,
        }
    }
}

/// Gelen bir UDP datagramını ilgili sokete dağıtır. Tüketildiyse `true`.
pub(crate) fn on_udp_datagram(
    src_ip: Ipv4Address,
    src_port: u16,
    dst_port: u16,
    data: &[u8],
) -> bool {
    unsafe {
        let slot = SOCKETS.iter_mut().find(|s| {
            s.used && s.sock_type == SockType::Dgram && s.local_port == dst_port && !s.udp_pending
        });
        let Some(s) = slot else { return false };
        let n = data.len().min(RX_BUF);
        s.rx[..n].copy_from_slice(&data[..n]);
        s.rx_len = n;
        s.udp_peer_ip = src_ip;
        s.udp_peer_port = src_port;
        s.udp_pending = true;
        true
    }
}

// ---------------------------------------------------------------------------
// Mantık self-testi (NIC gerektirmez)
// ---------------------------------------------------------------------------

pub fn self_test() -> bool {
    // 1. Tahsis: 8 soket açılır, 9. başarısız olur.
    let mut fds = [0i32; MAX_SOCKETS];
    let mut ok = true;
    for (k, slot) in fds.iter_mut().enumerate() {
        match socket(AF_INET, if k % 2 == 0 { SOCK_STREAM } else { SOCK_DGRAM }) {
            Ok(fd) => *slot = fd,
            Err(_) => ok = false,
        }
    }
    if socket(AF_INET, SOCK_STREAM).is_ok() {
        ok = false;
    }
    // 2. Kısa ömürlü portlar çakışmaz.
    let mut ports = [0u16; MAX_SOCKETS];
    for (k, fd) in fds.iter().enumerate() {
        unsafe {
            let p = alloc_ephemeral();
            SOCKETS[*fd as usize].local_port = p;
            ports[k] = p;
        }
    }
    for i in 0..MAX_SOCKETS {
        for j in (i + 1)..MAX_SOCKETS {
            if ports[i] == ports[j] {
                ok = false;
            }
        }
    }
    // 3. Geçersiz fd reddedilir; UDP sokette connect/send karıştırma reddedilir.
    if socket(0, 99).is_ok() || recv(99, &mut []).is_ok() {
        ok = false;
    }
    if connect(fds[1], Ipv4Address([1, 2, 3, 4]), 80).is_ok() {
        ok = false;
    }
    for fd in fds {
        close(fd);
    }
    // 4. Hepsi kapandıktan sonra yeniden tahsis dolar.
    let mut fds2 = [0i32; MAX_SOCKETS];
    for slot in fds2.iter_mut() {
        match socket(AF_INET, SOCK_STREAM) {
            Ok(fd) => *slot = fd,
            Err(_) => ok = false,
        }
    }
    for fd in fds2 {
        close(fd);
    }
    crate::serial_print("[SOCK-TEST] socket tahsis/port/self-check: ");
    crate::serial_print(if ok { "BASARILI\n" } else { "BASARISIZ\n" });
    ok
}

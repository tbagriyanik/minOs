#[derive(Clone, Copy)]
pub struct MacAddress(pub [u8; 6]);

#[derive(Clone, Copy)]
pub struct Ipv4Address(pub [u8; 4]);

pub struct EthernetPacket<'a> {
    pub src_mac: MacAddress,
    pub dst_mac: MacAddress,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

pub struct NetworkInterface {
    pub mac: MacAddress,
    pub ip: Ipv4Address,
    pub subnet: Ipv4Address,
    pub gateway: Ipv4Address,
    pub packets_rx: usize,
    pub packets_tx: usize,
}

impl NetworkInterface {
    pub const fn new() -> Self {
        Self {
            mac: MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]), // QEMU standart MAC
            ip: Ipv4Address([192, 168, 1, 100]),
            subnet: Ipv4Address([255, 255, 255, 0]),
            gateway: Ipv4Address([192, 168, 1, 1]),
            packets_rx: 0,
            packets_tx: 0,
        }
    }

    pub fn receive_packet(&mut self, _data: &[u8]) {
        self.packets_rx += 1;
    }

    pub fn transmit_packet(&mut self, _data: &[u8]) {
        self.packets_tx += 1;
    }
}

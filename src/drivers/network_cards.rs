//! PCI identification for legacy QEMU-compatible Ethernet adapters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkCard {
    IntelPro1000,
    RealtekRtl8139,
    AmdPcnet,
}

pub fn identify(vendor: u16, device: u16) -> Option<NetworkCard> {
    match (vendor, device) {
        (0x8086, 0x100e | 0x100f | 0x1004 | 0x10d3) => Some(NetworkCard::IntelPro1000),
        (0x10ec, 0x8139) => Some(NetworkCard::RealtekRtl8139),
        (0x1022, 0x2000) => Some(NetworkCard::AmdPcnet),
        _ => None,
    }
}

pub fn name(card: NetworkCard) -> &'static str {
    match card {
        NetworkCard::IntelPro1000 => "Intel PRO/1000",
        NetworkCard::RealtekRtl8139 => "Realtek RTL8139",
        NetworkCard::AmdPcnet => "AMD PCnet",
    }
}

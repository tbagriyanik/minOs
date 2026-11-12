//! Small, allocation-free primitives shared by drivers and protocol code.

use heapless::spsc::Queue;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

bitflags::bitflags! {
    /// Common Ethernet packet metadata carried between IRQ and network tasks.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PacketFlags: u8 {
        const CHECKSUM_VALID = 1 << 0;
        const BROADCAST = 1 << 1;
        const NEEDS_TX = 1 << 2;
    }
}

pub const NETWORK_QUEUE_CAPACITY: usize = 64;
pub type NetworkQueue<T> = Queue<T, NETWORK_QUEUE_CAPACITY>;

/// Ethernet II header. `zerocopy` keeps parsing aligned and bounds checked at
/// the call site while retaining the wire representation.
#[repr(C, packed)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct EthernetHeader {
    pub destination: [u8; 6],
    pub source: [u8; 6],
    pub ethertype: u16,
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut digest = crc32fast::Hasher::new();
    digest.update(data);
    digest.finalize()
}

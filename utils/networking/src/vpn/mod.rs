pub mod common;
pub mod error;
pub mod network;
mod packet;

pub use common::MAX_FRAME_SIZE;
pub use error::Error;
pub use network::{Network, Networks};
pub use packet::{ArpField, ArpPacket};
pub use packet::{EtherField, EtherFrame, EtherType, PeekPacket};
pub use packet::{IpPacket, IpV4Packet, IpV6Packet, Ipv4Field, Ipv6Field};

/// IP sub-protocol identifiers
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[non_exhaustive]
#[repr(u8)]
pub enum Protocol {
    HopByHop = 0,
    Icmp = 1,
    Igmp = 2,
    Tcp = 6,
    Egp = 8,
    Igp = 9,
    Udp = 17,
    Rdp = 27,
    Dccp = 33,
    Ipv6Tun = 41,
    Sdrp = 42,
    Ipv6Route = 43,
    Ipv6Frag = 44,
    Ipv6Icmp = 58,
    Ipv6NoNxt = 59,
    Ipv6Opts = 60,
    Ipcv = 71,
    IpIp = 94,
    IpComp = 108,
    Smp = 121,
    Sctp = 132,
    Ethernet = 143,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

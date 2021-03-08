use crate::vpn::Error;
use std::convert::TryFrom;
use std::ops::Deref;

#[non_exhaustive]
pub enum EtherFrame {
    /// EtherType IP
    Ip(Box<[u8]>),
    /// EtherType ARP
    Arp(Box<[u8]>),
}

impl EtherFrame {
    pub fn payload(&self) -> &[u8] {
        &self[14..]
    }

    pub fn reply(&self, mut payload: Vec<u8>) -> Vec<u8> {
        let frame: &Box<[u8]> = self.deref();
        payload.reserve(14);
        payload.splice(0..0, frame[12..14].iter().cloned());
        payload.splice(0..0, frame[0..6].iter().cloned());
        payload.splice(0..0, frame[6..12].iter().cloned());
        payload
    }
}

impl Deref for EtherFrame {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ip(b) | Self::Arp(b) => b,
        }
    }
}

impl Into<Vec<u8>> for EtherFrame {
    fn into(self) -> Vec<u8> {
        match self {
            Self::Ip(b) | Self::Arp(b) => b.into_vec(),
        }
    }
}

impl TryFrom<Vec<u8>> for EtherFrame {
    type Error = Error;

    #[inline]
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from(value.into_boxed_slice())
    }
}

impl TryFrom<Box<[u8]>> for EtherFrame {
    type Error = Error;

    fn try_from(value: Box<[u8]>) -> Result<Self, Self::Error> {
        const HEADER_SIZE: usize = 14;

        if value.len() < HEADER_SIZE {
            return Err(Error::PacketMalformed("Ethernet: frame too short".into()));
        }

        let protocol = &value[12..14];

        log::warn!("Frame: 0x{:02x?}", value);

        match protocol {
            &[0x08, 0x00] => {
                log::warn!("IP");
                IpPacket::peek(&value[HEADER_SIZE..])?;
                Ok(EtherFrame::Ip(value))
            }
            &[0x08, 0x06] => {
                log::warn!("ARP");
                ArpPacket::peek(&value[HEADER_SIZE..])?;
                Ok(EtherFrame::Arp(value))
            }
            _ => Err(Error::ProtocolNotSupported(format!("0x{:02x?}", protocol))),
        }
    }
}

impl std::fmt::Display for EtherFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EtherFrame::Ip(_) => write!(f, "IP"),
            EtherFrame::Arp(_) => write!(f, "ARP"),
        }
    }
}

pub trait EtherType<'a> {
    fn peek(data: &'a [u8]) -> Result<(), Error>;
    fn packet(data: &'a [u8]) -> Self;
}

pub enum IpPacket<'a> {
    V4(IpV4Packet<'a>),
    V6(IpV6Packet<'a>),
}

impl<'a> IpPacket<'a> {
    pub fn dst_address(&self) -> &'a [u8] {
        match self {
            Self::V4(ip) => ip.dst_address,
            Self::V6(ip) => ip.dst_address,
        }
    }

    pub fn is_broadcast(&self) -> bool {
        match self {
            Self::V4(ip) => &ip.dst_address[0..4] == &[255, 255, 255, 255],
            Self::V6(_) => false,
        }
    }
}

impl<'a> EtherType<'a> for IpPacket<'a> {
    fn peek(data: &'a [u8]) -> Result<(), Error> {
        match data[0] >> 4 {
            4 => IpV4Packet::peek(data),
            6 => IpV6Packet::peek(data),
            _ => Err(Error::PacketMalformed("IP: invalid version".into())),
        }
    }

    fn packet(data: &'a [u8]) -> Self {
        if data[0] >> 4 == 4 {
            Self::V4(IpV4Packet::packet(data))
        } else {
            Self::V6(IpV6Packet::packet(data))
        }
    }
}

pub struct IpV4Packet<'a> {
    pub dst_address: &'a [u8],
}

impl<'a> EtherType<'a> for IpV4Packet<'a> {
    fn peek(data: &'a [u8]) -> Result<(), Error> {
        const HEADER_SIZE: usize = 20;

        if data.len() < HEADER_SIZE {
            return Err(Error::PacketMalformed("IPv4: header too short".into()));
        }

        let len = u16::from_be_bytes([data[2], data[3]]) as usize;
        if data.len() < len {
            return Err(Error::PacketMalformed("IPv4: payload too short".into()));
        }

        Ok(())
    }

    fn packet(data: &'a [u8]) -> Self {
        Self {
            dst_address: &data[16..20],
        }
    }
}

pub struct IpV6Packet<'a> {
    pub dst_address: &'a [u8],
}

impl<'a> EtherType<'a> for IpV6Packet<'a> {
    fn peek(data: &'a [u8]) -> Result<(), Error> {
        const HEADER_SIZE: usize = 40;

        if data.len() < HEADER_SIZE {
            return Err(Error::PacketMalformed("IPv6: header too short".into()));
        }

        let len = HEADER_SIZE + u16::from_be_bytes([data[4], data[5]]) as usize;
        if data.len() < len as usize {
            return Err(Error::PacketMalformed("IPv6: payload too short".into()));
        } else if len == HEADER_SIZE {
            return Err(Error::ProtocolNotSupported("IPv6: jumbogram".into()));
        }

        Ok(())
    }

    fn packet(data: &'a [u8]) -> Self {
        Self {
            dst_address: &data[24..40],
        }
    }
}

pub struct ArpPacket<'a> {
    /// Hardware type
    pub htype: &'a [u8],
    /// Protocol type
    pub ptype: &'a [u8],
    /// Hardware length
    pub hlen: u8,
    /// Protocol length
    pub plen: u8,
    /// Operation
    pub op: &'a [u8],
    /// Sender hardware address
    pub sha: &'a [u8],
    /// Sender protocol address
    pub spa: &'a [u8],
    /// Target hardware address
    pub tha: &'a [u8],
    /// Target protocol address
    pub tpa: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for ArpPacket<'a> {
    type Error = Error;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        Self::peek(data)?;
        Ok(Self {
            htype: &data[0..2],
            ptype: &data[2..4],
            hlen: data[4],
            plen: data[5],
            op: &data[6..8],
            sha: &data[8..14],
            spa: &data[14..18],
            tha: &data[18..24],
            tpa: &data[24..28],
        })
    }
}

impl<'a> EtherType<'a> for ArpPacket<'a> {
    fn peek(data: &'a [u8]) -> Result<(), Error> {
        if data.len() < 28 {
            return Err(Error::PacketMalformed("ARP: packet too short".into()));
        }
        Ok(())
    }

    fn packet(data: &'a [u8]) -> Self {
        Self::try_from(data).ok().unwrap()
    }
}

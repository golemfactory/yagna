#![allow(unused)]

use crate::vpn::packet::field::*;
use crate::vpn::Error;
use std::convert::TryFrom;
use std::ops::Deref;

mod field {
    pub type Field = std::ops::Range<usize>;
    pub type Rest = std::ops::RangeFrom<usize>;
}

pub struct EtherField;
impl EtherField {
    pub const DST_MAC: Field = 0..6;
    pub const SRC_MAC: Field = 6..12;
    pub const ETHER_TYPE: Field = 12..14;
    pub const PAYLOAD: Rest = 14..;
}

#[non_exhaustive]
pub enum EtherFrame {
    /// EtherType IP
    Ip(Box<[u8]>),
    /// EtherType ARP
    Arp(Box<[u8]>),
}

impl EtherFrame {
    pub fn get_field(&self, field: Field) -> &[u8] {
        &self.deref()[field]
    }

    pub fn payload(&self) -> &[u8] {
        &self[EtherField::PAYLOAD]
    }

    pub fn reply(&self, mut payload: Vec<u8>) -> Vec<u8> {
        let frame: &Box<[u8]> = self.deref();
        payload.reserve(EtherField::PAYLOAD.start);
        payload.splice(0..0, frame[EtherField::ETHER_TYPE].iter().cloned());
        payload.splice(0..0, frame[EtherField::DST_MAC].iter().cloned());
        payload.splice(0..0, frame[EtherField::SRC_MAC].iter().cloned());
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

impl TryFrom<Box<[u8]>> for EtherFrame {
    type Error = Error;

    fn try_from(value: Box<[u8]>) -> Result<Self, Self::Error> {
        const HEADER_SIZE: usize = 14;

        if value.len() < HEADER_SIZE {
            return Err(Error::PacketMalformed("Ethernet: frame too short".into()));
        }

        let protocol = &value[EtherField::ETHER_TYPE];
        match protocol {
            &[0x08, 0x00] => {
                IpPacket::peek(&value[HEADER_SIZE..])?;
                Ok(EtherFrame::Ip(value))
            }
            &[0x08, 0x06] => {
                ArpPacket::peek(&value[HEADER_SIZE..])?;
                Ok(EtherFrame::Arp(value))
            }
            _ => Err(Error::ProtocolNotSupported(format!("0x{:02x?}", protocol))),
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

impl Into<Vec<u8>> for EtherFrame {
    fn into(self) -> Vec<u8> {
        match self {
            Self::Ip(b) | Self::Arp(b) => b.into_vec(),
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

pub struct Ipv4Field;
impl Ipv4Field {
    pub const HDR_SIZE: Field = 2..4;
    pub const DST_ADDR: Field = 16..20;
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

        let mut field = [0u8; 2];
        field.copy_from_slice(&data[Ipv4Field::HDR_SIZE]);
        let len = u16::from_be_bytes(field) as usize;

        if data.len() < len {
            return Err(Error::PacketMalformed("IPv4: payload too short".into()));
        }
        Ok(())
    }

    fn packet(data: &'a [u8]) -> Self {
        Self {
            dst_address: &data[Ipv4Field::DST_ADDR],
        }
    }
}

pub struct Ipv6Field;
impl Ipv6Field {
    pub const HDR_SIZE: Field = 4..6;
    pub const DST_ADDR: Field = 24..40;
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

        let mut field = [0u8; 2];
        field.copy_from_slice(&data[Ipv6Field::HDR_SIZE]);
        let len = HEADER_SIZE + u16::from_be_bytes(field) as usize;

        if data.len() < len as usize {
            return Err(Error::PacketMalformed("IPv6: payload too short".into()));
        } else if len == HEADER_SIZE {
            return Err(Error::ProtocolNotSupported("IPv6: jumbogram".into()));
        }
        Ok(())
    }

    fn packet(data: &'a [u8]) -> Self {
        Self {
            dst_address: &data[Ipv6Field::DST_ADDR],
        }
    }
}

pub struct ArpField;
impl ArpField {
    /// Hardware type
    pub const HTYPE: Field = 0..2;
    /// Protocol type
    pub const PTYPE: Field = 2..4;
    /// Hardware length
    pub const HLEN: Field = 4..5;
    /// Protocol length
    pub const PLEN: Field = 5..6;
    /// Operation
    pub const OP: Field = 6..8;
    /// Sender hardware address
    pub const SHA: Field = 8..14;
    /// Sender protocol address
    pub const SPA: Field = 14..18;
    /// Target hardware address
    pub const THA: Field = 18..24;
    /// Target protocol address
    pub const TPA: Field = 24..28;
}

pub struct ArpPacket<'a> {
    inner: &'a [u8],
}

impl<'a> ArpPacket<'a> {
    pub fn get_field(&self, field: Field) -> &[u8] {
        &self.inner[field]
    }
}

impl<'a> ArpPacket<'a> {
    pub fn mirror(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(ArpField::TPA.end);
        buf.extend(self.inner[0..ArpField::OP.start].iter().cloned());
        buf.extend(self.mirror_op().iter().cloned());
        buf.extend(self.get_field(ArpField::THA).iter().cloned());
        buf.extend(self.get_field(ArpField::TPA).iter().cloned());
        buf.extend(self.get_field(ArpField::SHA).iter().cloned());
        buf.extend(self.get_field(ArpField::SPA).iter().cloned());
        buf
    }

    fn mirror_op(&self) -> [u8; 2] {
        let op = self.get_field(ArpField::OP);
        // request
        if op == &[0x00, 0x01] {
            // reply
            [0x00, 0x02]
        } else if op == &[0x00, 0x02] {
            [0x00, 0x01]
        } else {
            let mut ret = [0u8; 2];
            ret.copy_from_slice(op);
            ret
        }
    }
}

impl<'a> TryFrom<&'a [u8]> for ArpPacket<'a> {
    type Error = Error;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        Self::peek(data)?;
        Ok(Self { inner: data })
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

pub struct ArpPacketMut<'a> {
    inner: &'a mut [u8],
}

impl<'a> ArpPacketMut<'a> {
    pub fn set_field(&mut self, field: Field, value: &[u8]) {
        let value = &value[..field.end];
        self.inner[field].copy_from_slice(value);
    }

    pub fn freeze(self) -> ArpPacket<'a> {
        ArpPacket { inner: self.inner }
    }
}

impl<'a> TryFrom<&'a mut [u8]> for ArpPacketMut<'a> {
    type Error = Error;

    fn try_from(data: &'a mut [u8]) -> Result<Self, Self::Error> {
        if data.len() < 28 {
            return Err(Error::PacketMalformed("ARP: packet too short".into()));
        }
        Ok(Self { inner: data })
    }
}

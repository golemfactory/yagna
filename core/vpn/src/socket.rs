use managed::ManagedSlice;
use smoltcp::socket::*;
use smoltcp::wire::{IpProtocol, IpVersion};
use std::time::Duration;
use ya_utils_networking::vpn::MAX_FRAME_SIZE;

pub const TCP_CONN_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_KEEP_ALIVE: Duration = Duration::from_secs(60);

pub fn tcp_socket<'a>() -> TcpSocket<'a> {
    let rx_buf = TcpSocketBuffer::new(vec![0; MAX_FRAME_SIZE * 4]);
    let tx_buf = TcpSocketBuffer::new(vec![0; MAX_FRAME_SIZE * 4]);
    let mut socket = TcpSocket::new(rx_buf, tx_buf);
    socket.set_keep_alive(Some(TCP_KEEP_ALIVE.into()));
    socket
}

pub fn udp_socket<'a>() -> UdpSocket<'a> {
    let rx_buf = UdpSocketBuffer::new(meta_storage(), payload_storage());
    let tx_buf = UdpSocketBuffer::new(meta_storage(), payload_storage());
    UdpSocket::new(rx_buf, tx_buf)
}

pub fn icmp_socket<'a>() -> IcmpSocket<'a> {
    let rx_buf = IcmpSocketBuffer::new(meta_storage(), payload_storage());
    let tx_buf = IcmpSocketBuffer::new(meta_storage(), payload_storage());
    IcmpSocket::new(rx_buf, tx_buf)
}

pub fn raw_socket<'a>(ip_version: IpVersion, ip_protocol: IpProtocol) -> RawSocket<'a> {
    let rx_buf = RawSocketBuffer::new(meta_storage(), payload_storage());
    let tx_buf = RawSocketBuffer::new(meta_storage(), payload_storage());
    RawSocket::new(ip_version, ip_protocol, rx_buf, tx_buf)
}

fn meta_storage<'a, T: Clone>() -> ManagedSlice<'a, T> {
    ManagedSlice::Owned(Vec::new())
}

fn payload_storage<T: Default + Clone>() -> Vec<T> {
    vec![Default::default(); MAX_FRAME_SIZE]
}

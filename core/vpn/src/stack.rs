use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use smoltcp::iface::Route;
use smoltcp::socket::*;
use smoltcp::time::Instant;
use smoltcp::wire::{IpAddress, IpCidr, IpEndpoint, IpProtocol, IpVersion};

use ya_utils_networking::vpn::{Error, Protocol};

use crate::interface::*;
use crate::message::ConnectionMeta;
use crate::port;
use crate::socket::*;

pub type Result<T> = std::result::Result<T, Error>;

pub struct Stack<'a> {
    iface: Rc<RefCell<CaptureInterface<'a>>>,
    sockets: Rc<RefCell<SocketSet<'a>>>,
    ports: Rc<RefCell<port::Allocator>>,
}

impl<'a> Stack<'a> {
    pub fn new(net_ip: IpCidr, net_route: Route) -> Self {
        let sockets = SocketSet::new(Vec::with_capacity(8));
        let mut iface = default_iface();
        add_iface_route(&mut iface, net_ip, net_route);

        Self {
            iface: Rc::new(RefCell::new(iface)),
            sockets: Rc::new(RefCell::new(sockets)),
            ports: Default::default(),
        }
    }

    pub(crate) fn iface(&self) -> Rc<RefCell<CaptureInterface<'a>>> {
        self.iface.clone()
    }

    pub(crate) fn sockets(&self) -> Rc<RefCell<SocketSet<'a>>> {
        self.sockets.clone()
    }

    pub(crate) fn poll(&self) -> Result<()> {
        let mut iface = self.iface.borrow_mut();
        let mut sockets = self.sockets.borrow_mut();
        iface
            .poll(&mut (*sockets), Instant::now())
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    #[allow(unused)]
    pub fn bind(&self, protocol: Protocol, local: IpEndpoint) -> Result<SocketHandle> {
        let mut sockets = self.sockets.borrow_mut();
        let handle = match protocol {
            Protocol::Tcp => sockets.add(tcp_socket()),
            Protocol::Udp => sockets.add(udp_socket()),
            Protocol::Icmp => sockets.add(icmp_socket()),
            _ => {
                let ip_version = match local.addr {
                    IpAddress::Ipv4(_) => IpVersion::Ipv4,
                    IpAddress::Ipv6(_) => IpVersion::Ipv6,
                    _ => return Err(Error::Other(format!("Invalid address: {}", local.addr))),
                };

                sockets.add(raw_socket(ip_version, map_protocol(protocol)?))
            }
        };
        Ok(handle)
    }

    pub fn connect(&self, remote: IpEndpoint) -> Result<Connect<'a>> {
        let mut sockets = self.sockets.borrow_mut();
        let mut ports = self.ports.borrow_mut();

        let ip = self.address()?.address();
        let port = ports.next(Protocol::Tcp)?;
        let local: IpEndpoint = (ip, port).into();
        let handle = sockets.add(tcp_socket());

        if let Err(e) = {
            let mut socket = sockets.get::<TcpSocket>(handle);
            socket.connect(remote, local)
        } {
            sockets.remove(handle);
            ports.free(Protocol::Tcp, port);
            return Err(Error::ConnectionError(e.to_string()));
        }

        Ok(Connect {
            meta: ConnectionMeta {
                handle,
                protocol: Protocol::Tcp,
                remote,
            },
            local,
            sockets: self.sockets.clone(),
        })
    }

    pub fn disconnect(&self, protocol: Protocol, handle: SocketHandle) -> Result<()> {
        let mut sockets = self.sockets.borrow_mut();
        let mut ports = self.ports.borrow_mut();

        let port = match protocol {
            Protocol::Tcp => {
                let socket = sockets.get::<TcpSocket>(handle);
                socket.local_endpoint().port
            }
            Protocol::Udp => {
                let socket = sockets.get::<UdpSocket>(handle);
                socket.endpoint().port
            }
            _ => 0 as u16,
        };

        sockets.remove(handle);
        ports.free(protocol, port);

        Ok(())
    }

    pub fn send<F: Fn() + 'static>(&self, data: Vec<u8>, meta: ConnectionMeta, f: F) -> Send<'a> {
        Send {
            data,
            offset: 0,
            meta,
            sockets: self.sockets.clone(),
            sent: Box::new(f),
        }
    }

    pub fn receive_phy(&self, data: Vec<u8>) {
        let mut iface = self.iface.borrow_mut();
        iface.device_mut().phy_rx(data)
    }

    pub fn addresses(&self) -> Vec<IpCidr> {
        self.iface.borrow().ip_addrs().to_vec()
    }

    pub fn address(&self) -> Result<IpCidr> {
        {
            let iface = self.iface.borrow();
            iface.ip_addrs().iter().next().cloned()
        }
        .ok_or_else(|| Error::NetEmpty)
    }

    pub fn add_address(&self, address: IpCidr) {
        let mut iface = self.iface.borrow_mut();
        add_iface_address(&mut (*iface), address);
    }
}

pub struct Connect<'a> {
    pub meta: ConnectionMeta,
    local: IpEndpoint,
    sockets: Rc<RefCell<SocketSet<'a>>>,
}

impl<'a> Future for Connect<'a> {
    type Output = Result<IpEndpoint>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sockets_rfc = self.sockets.clone();
        let mut sockets = sockets_rfc.borrow_mut();
        let mut socket = sockets.get::<TcpSocket>(self.meta.handle);

        if !socket.is_open() {
            Poll::Ready(Err(Error::ConnectionError("socket closed".into())))
        } else if socket.can_send() {
            Poll::Ready(Ok(self.local))
        } else {
            socket.register_send_waker(cx.waker());
            Poll::Pending
        }
    }
}

pub struct Send<'a> {
    data: Vec<u8>,
    offset: usize,
    meta: ConnectionMeta,
    sockets: Rc<RefCell<SocketSet<'a>>>,
    sent: Box<dyn Fn()>,
}

impl<'a> Future for Send<'a> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = {
            let sockets_rfc = self.sockets.clone();
            let mut sockets = sockets_rfc.borrow_mut();
            let meta = &self.meta;

            match meta.protocol {
                Protocol::Tcp => {
                    let mut socket = sockets.get::<TcpSocket>(meta.handle);
                    let result = socket.send_slice(&self.data[self.offset..]);
                    (*self.sent)();

                    return match result {
                        Ok(count) => {
                            self.offset += count;
                            if self.offset >= self.data.len() {
                                Poll::Ready(Ok(()))
                            } else {
                                socket.register_send_waker(cx.waker());
                                Poll::Pending
                            }
                        }
                        Err(smoltcp::Error::Exhausted) => {
                            socket.register_send_waker(cx.waker());
                            Poll::Pending
                        }
                        Err(err) => Poll::Ready(Err(Error::Other(err.to_string()))),
                    };
                }
                Protocol::Udp => sockets
                    .get::<UdpSocket>(meta.handle)
                    .send_slice(&self.data, meta.remote),
                Protocol::Icmp => sockets
                    .get::<IcmpSocket>(meta.handle)
                    .send_slice(&self.data, meta.remote.addr),
                _ => sockets.get::<RawSocket>(meta.handle).send_slice(&self.data),
            }
        };

        (*self.sent)();

        match result {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(Error::Other(err.to_string()))),
        }
    }
}

fn map_protocol(protocol: Protocol) -> Result<IpProtocol> {
    match protocol {
        Protocol::HopByHop => Ok(IpProtocol::HopByHop),
        Protocol::Icmp => Ok(IpProtocol::Icmp),
        Protocol::Igmp => Ok(IpProtocol::Igmp),
        Protocol::Tcp => Ok(IpProtocol::Tcp),
        Protocol::Udp => Ok(IpProtocol::Udp),
        Protocol::Ipv6Route => Ok(IpProtocol::Ipv6Route),
        Protocol::Ipv6Frag => Ok(IpProtocol::Ipv6Frag),
        Protocol::Ipv6Icmp => Ok(IpProtocol::Icmpv6),
        Protocol::Ipv6NoNxt => Ok(IpProtocol::Ipv6NoNxt),
        Protocol::Ipv6Opts => Ok(IpProtocol::Ipv6Opts),
        _ => Err(Error::ProtocolNotSupported(protocol.to_string())),
    }
}

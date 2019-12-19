use crate::crypto::PeerId;

pub use parity_multiaddr::Multiaddr;
use parity_multiaddr::Protocol;

use std::net::{IpAddr, SocketAddr};

pub trait MultiaddrExt {
    fn quic_peer(sock: SocketAddr, peer_id: PeerId) -> Multiaddr {
        let mut maddr = Multiaddr::from(sock.ip());
        maddr.push(Protocol::Udp(sock.port()));
        maddr.push(Protocol::Quic);
        maddr.push(Protocol::P2p(peer_id.into_inner()));

        maddr
    }

    fn to_socket_addr(&self) -> SocketAddr;

    fn contains_socket_addr(&self) -> bool;

    fn is_quic(&self) -> bool;
}

impl MultiaddrExt for Multiaddr {
    /// # panic
    ///
    /// Panic on invalid multiaddr
    fn to_socket_addr(&self) -> SocketAddr {
        use Protocol::*;

        let mut ip: Option<IpAddr> = None;
        let mut port = None;

        for proto in self.iter() {
            match proto {
                Ip4(i) => ip = Some(IpAddr::from(i)),
                Ip6(i) => ip = Some(IpAddr::from(i)),
                Udp(p) => port = Some(p),
                _ => (),
            }
        }

        if let (Some(ip), Some(port)) = (ip, port) {
            SocketAddr::new(ip, port)
        } else {
            panic!("invalid multiaddr address");
        }
    }

    fn contains_socket_addr(&self) -> bool {
        use Protocol::*;

        let mut has_ip = false;
        let mut has_port = false;

        for proto in self.iter() {
            match proto {
                Ip4(_) => has_ip = true,
                Ip6(_) => has_ip = true,
                Udp(_) => has_port = true,
                Tcp(_) => has_port = true,
                _ => (),
            }
        }

        if has_ip && has_port {
            true
        } else {
            false
        }
    }

    fn is_quic(&self) -> bool {
        use Protocol::*;

        for proto in self.iter() {
            if let Quic = proto {
                return true;
            }
        }

        false
    }
}

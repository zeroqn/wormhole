use crate::PeerId;

pub use parity_multiaddr::Multiaddr;
use parity_multiaddr::Protocol;

use std::net::SocketAddr;

pub trait MultiaddrExt {
    fn quic_from_sock_addr(sock: SocketAddr) -> Multiaddr {
        let mut addr = Multiaddr::from(sock.ip());
        addr.push(Protocol::Udp(sock.port()));
        addr.push(Protocol::Quic);

        addr
    }

    fn push_peer_id(&mut self, peer_id: PeerId);
}

impl MultiaddrExt for Multiaddr {
    fn push_peer_id(&mut self, peer_id: PeerId) {
        self.push(Protocol::P2p(peer_id.0))
    }
}

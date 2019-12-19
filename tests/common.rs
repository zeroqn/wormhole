use anyhow::Error;
use wormhole::crypto::{PrivateKey, PublicKey};
use wormhole::multiaddr::{Multiaddr, MultiaddrExt};
use wormhole::transport::{QuicListener, QuicTransport, Transport};

use std::net::ToSocketAddrs;

#[derive(thiserror::Error, Debug)]
pub enum CommonError {
    #[error("no socket address")]
    NoSocketAddress,
    #[error("no message")]
    NoMessage,
}

pub fn random_keypair() -> (PrivateKey, PublicKey) {
    let privkey = (0..32).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    let privkey =
        PrivateKey::from_slice(privkey.as_slice()).expect("impossible, random private key fail");

    let pubkey = privkey.pubkey();

    (privkey, pubkey)
}

pub async fn make_xenovox<A: ToSocketAddrs>(addr: A) -> Result<(QuicTransport, QuicListener, Multiaddr, PublicKey), Error> {
    let (sk, pk) = random_keypair();

    let mut xenovox = QuicTransport::make(&sk, pk.clone())?;
    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let maddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    let listener = xenovox.listen(maddr.clone()).await?;
    Ok((xenovox, listener, maddr, pk))
}

use crate::{PeerId, PublicKey};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use parity_multiaddr::Multiaddr;
use futures::{io::{AsyncRead, AsyncWrite}, Stream};
use bytes::Bytes;

use std::net::SocketAddr;

#[async_trait]
pub trait MuxedStream: AsyncRead + AsyncWrite + Stream<Item = Bytes> {
    async fn close(&mut self) -> Result<(), Error>;

    fn reset(&mut self);
}

#[async_trait]
pub trait CapableConn: Sync + Send + Clone {
    type MuxedStream;
    type Transport: Sync + Send + Clone;

    async fn open_stream(&self) -> Result<Self::MuxedStream, Error>;

    async fn accept_stream(&self) -> Result<Self::MuxedStream, Error>;

    fn is_closed() -> bool;

    async fn close(&self) -> Result<(), Error>;

    fn local_peer(&self) -> &PeerId;

    fn remote_peer(&self) -> PeerId;

    fn remote_public_key(&self) -> PublicKey;

    fn local_multiaddr(&self) -> &Multiaddr;

    fn remote_multiaddr(&self) -> &Multiaddr;

    fn transport(&self) -> Self::Transport;
}

#[async_trait]
pub trait Listener: Send {
    type CapableConn;

    async fn accept(&self) -> Result<Self::CapableConn, Error>;
    
    async fn close(&self) -> Result<(), Error>;

    fn addr(&self) -> SocketAddr;

    fn multiaddr(&self) -> &Multiaddr;
}

#[async_trait]
pub trait Transport: Sync + Send + Clone {
    type CapableConn;
    type Listener;

    async fn dial(&self, ctx: Context, raddr: Multiaddr, peer_id: PeerId) -> Result<Self::CapableConn, Error>;

    fn can_dial(&self, raddr: &Multiaddr) -> bool;

    async fn listen(&self, laddr: Multiaddr) -> Result<Self::Listener, Error>;
}

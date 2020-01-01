pub mod capable_conn;
pub mod config;
pub mod listener;
pub mod muxed_stream;
pub mod transport;

pub use capable_conn::QuinnConnectionExt;
pub use config::QuicConfig;
pub use transport::QuicTransport;

use capable_conn::QuicConn;
use listener::QuicListener;
use muxed_stream::QuicMuxedStream;

use crate::crypto::{PeerId, PublicKey};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use dyn_clone::DynClone;
use futures::prelude::{AsyncRead, AsyncWrite, Stream};
use parity_multiaddr::Multiaddr;

use std::net::SocketAddr;

pub const RESET_ERR_CODE: u32 = 0;

#[async_trait]
pub trait MuxedStream: AsyncRead + AsyncWrite + Stream<Item = Bytes> + Unpin + Send {
    async fn close(&mut self) -> Result<(), Error>;

    fn reset(&mut self);
}

pub trait ConnSecurity {
    fn local_peer(&self) -> PeerId;

    fn remote_peer(&self) -> PeerId;

    fn remote_public_key(&self) -> PublicKey;
}

pub trait ConnMultiaddr {
    fn local_multiaddr(&self) -> Multiaddr;

    fn remote_multiaddr(&self) -> Multiaddr;
}

#[async_trait]
pub trait CapableConn: ConnSecurity + ConnMultiaddr + Sync + Send + DynClone {
    async fn open_stream(&self) -> Result<Box<dyn MuxedStream>, Error>;

    async fn accept_stream(&self) -> Result<Box<dyn MuxedStream>, Error>;

    fn is_closed(&self) -> bool;

    async fn close(&self) -> Result<(), Error>;

    fn transport(&self) -> Box<dyn Transport>;
}

impl<C: CapableConn + 'static> From<C> for Box<dyn CapableConn> {
    fn from(conn: C) -> Self {
        Box::new(conn) as Box<dyn CapableConn>
    }
}

#[async_trait]
pub trait Listener: Send {
    async fn accept(&mut self) -> Result<Box<dyn CapableConn>, Error>;

    fn close(&mut self) -> Result<(), Error>;

    fn addr(&self) -> SocketAddr;

    fn multiaddr(&self) -> Multiaddr;
}

impl<L: Listener + 'static> From<L> for Box<dyn Listener> {
    fn from(listener: L) -> Self {
        Box::new(listener) as Box<dyn Listener>
    }
}

#[async_trait]
pub trait Transport: Sync + Send + DynClone {
    async fn dial(
        &self,
        ctx: Context,
        raddr: Multiaddr,
        peer_id: PeerId,
    ) -> Result<Box<dyn CapableConn>, Error>;

    fn can_dial(&self, raddr: &Multiaddr) -> bool;

    async fn listen(&mut self, laddr: Multiaddr) -> Result<Box<dyn Listener>, Error>;

    fn local_multiaddr(&self) -> Option<Multiaddr>;
}

impl<T: Transport + 'static> From<T> for Box<dyn Transport> {
    fn from(transport: T) -> Self {
        Box::new(transport) as Box<dyn Transport>
    }
}

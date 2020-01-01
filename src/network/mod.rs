pub mod conn;
pub mod conn_pool;
pub mod dialer;
pub mod r#impl;
pub mod stream;

pub use r#impl::QuicNetwork;

use conn::NetworkConn;
use conn_pool::NetworkConnPool;
use dialer::NetworkDialer;
use stream::NetworkStream;

use crate::{
    crypto::PeerId,
    multiaddr::Multiaddr,
    peer_store::PeerStore,
    transport::{ConnMultiaddr, ConnSecurity},
};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use derive_more::Display;
use dyn_clone::DynClone;
use futures::io::{AsyncRead, AsyncWrite};

use std::{fmt, ops::Deref};

pub enum NetworkEvent {
    Listen(Box<dyn Network>, Multiaddr),
    ListenClose(Box<dyn Network>, Multiaddr),
    Connected(Box<dyn Network>, Box<dyn Conn>),
    Disconnected(Box<dyn Network>, Box<dyn Conn>),
    OpenedStream(Box<dyn Network>, Box<dyn Stream>),
    ClosedStream(Box<dyn Network>, Box<dyn Stream>),
}

#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]
pub enum Connectedness {
    #[display(fmt = "not connected before")]
    NotConnected,
    #[display(fmt = "connected")]
    Connected,
    #[display(fmt = "can connect")]
    CanConnect,
    #[display(fmt = "unable to connect")]
    CannotConnect,
}
#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]

pub enum Direction {
    #[display(fmt = "inbound")]
    Inbound,
    #[display(fmt = "outbound")]
    Outbound,
}

#[derive(Display, PartialEq, Eq, Hash, Clone, Copy)]
#[display(fmt = "{}", _0)]
pub struct ProtocolId(u64);

impl Deref for ProtocolId {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Into<ProtocolId> for u64 {
    fn into(self) -> ProtocolId {
        ProtocolId(self)
    }
}

#[derive(Display, PartialEq, Eq, Hash, Clone, Copy)]
#[display(fmt = "protocol {} => {}", id, name)]
pub struct Protocol {
    pub(crate) id: ProtocolId,
    pub(crate) name: &'static str,
}

impl Protocol {
    pub fn new(id: u64, name: &'static str) -> Self {
        Protocol {
            id: id.into(),
            name,
        }
    }
}

impl fmt::Debug for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

// TODO: Item should be protocol message
#[async_trait]
pub trait Stream:
    AsyncWrite + AsyncRead + futures::stream::Stream<Item = Bytes> + DynClone + Send + Sync + Unpin
{
    fn protocol(&self) -> Option<Protocol>;

    fn set_protocol(&mut self, proto: Protocol);

    fn direction(&self) -> Direction;

    fn conn(&self) -> Box<dyn Conn>;

    async fn close(&mut self) -> Result<(), Error>;

    async fn reset(&mut self);
}

impl<S: Stream + 'static> From<S> for Box<dyn Stream> {
    fn from(stream: S) -> Box<dyn Stream> {
        Box::new(stream) as Box<dyn Stream>
    }
}

dyn_clone::clone_trait_object!(Stream);

#[async_trait]
pub trait Conn: ConnSecurity + ConnMultiaddr + DynClone + Send + Sync {
    async fn new_stream(&self, proto: Protocol) -> Result<Box<dyn Stream>, Error>;

    async fn streams(&self) -> Vec<Box<dyn Stream>>;

    fn direction(&self) -> Direction;

    fn is_closed(&self) -> bool;

    async fn close(&self) -> Result<(), Error>;
}

impl<C: Conn + 'static> From<C> for Box<dyn Conn> {
    fn from(conn: C) -> Box<dyn Conn> {
        Box::new(conn) as Box<dyn Conn>
    }
}

dyn_clone::clone_trait_object!(Conn);

#[async_trait]
impl<C> Conn for C
where
    C: Deref<Target = dyn Conn> + ConnSecurity + ConnMultiaddr + DynClone + Send + Sync,
{
    async fn new_stream(&self, proto: Protocol) -> Result<Box<dyn Stream>, Error> {
        self.deref().new_stream(proto).await
    }

    async fn streams(&self) -> Vec<Box<dyn Stream>> {
        self.deref().streams().await
    }

    fn direction(&self) -> Direction {
        self.deref().direction()
    }

    fn is_closed(&self) -> bool {
        self.deref().is_closed()
    }

    async fn close(&self) -> Result<(), Error> {
        self.deref().close().await
    }
}

#[async_trait]
pub trait Dialer {
    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Box<dyn Conn>, Error>;

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    fn peer_store(&self) -> PeerStore;

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn peers(&self) -> Vec<PeerId>;

    async fn conns(&self) -> Vec<Box<dyn Conn>>;

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>>;
}

#[async_trait]
pub trait Network: Send + Sync + DynClone {
    async fn close(&self) -> Result<(), Error>;

    async fn new_stream(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        proto: Protocol,
    ) -> Result<Box<dyn Stream>, Error>;

    async fn listen(&mut self, laddr: Multiaddr) -> Result<(), Error>;
}

impl<N: Network + 'static> From<N> for Box<dyn Network> {
    fn from(network: N) -> Box<dyn Network> {
        Box::new(network) as Box<dyn Network>
    }
}

dyn_clone::clone_trait_object!(Network);

#[async_trait]
pub trait RemoteConnHandler: Send + Sync + DynClone {
    async fn handle(&self, conn: Box<dyn Conn>);
}

dyn_clone::clone_trait_object!(RemoteConnHandler);

#[async_trait]
pub trait RemoteStreamHandler: Send + Sync + DynClone {
    async fn handle(&self, stream: Box<dyn Stream>);
}

dyn_clone::clone_trait_object!(RemoteStreamHandler);

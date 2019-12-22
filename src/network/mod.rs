pub mod conn;
pub mod conn_pool;
pub mod dialer;
pub mod r#impl;
pub mod stream;
pub use conn::QuicConn;
pub(crate) use conn_pool::QuicConnPool;
pub use dialer::QuicDialer;
pub use r#impl::QuicNetwork;
pub use stream::QuicStream;

use crate::{
    crypto::PeerId,
    multiaddr::Multiaddr,
    transport::{ConnMultiaddr, ConnSecurity},
};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use derive_more::Display;
use futures::io::{AsyncRead, AsyncWrite};

pub enum NetworkEvent<Network, Conn, Stream> {
    Listen(Network, Multiaddr),
    ListenClose(Network, Multiaddr),
    Connected(Network, Conn),
    Disconnected(Network, Conn),
    OpenedStream(Network, Stream),
    ClosedStream(Network, Stream),
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

#[derive(Debug, Display, PartialEq, Eq, Hash, Clone, Copy)]
#[display(fmt = "protocol {} => {}", id, name)]
pub struct ProtocolId {
    id: u64,
    name: &'static str,
}

impl ProtocolId {
    pub fn new(id: u64, name: &'static str) -> Self {
        ProtocolId { id, name }
    }
}

// TODO: Item should be protocol message
#[async_trait]
pub trait Stream: AsyncWrite + AsyncRead + futures::stream::Stream<Item = Bytes> + Clone {
    type Conn: Clone + Send;

    fn protocol(&self) -> Option<ProtocolId>;

    fn set_protocol(&mut self, proto_id: ProtocolId);

    fn direction(&self) -> Direction;

    fn conn(&self) -> Self::Conn;

    async fn close(&mut self) -> Result<(), Error>;

    async fn reset(&mut self);
}

#[async_trait]
pub trait Conn: ConnSecurity + ConnMultiaddr + Clone + Send {
    type Stream;

    async fn new_stream(&self, proto_id: ProtocolId) -> Result<Self::Stream, Error>;

    async fn streams(&self) -> Vec<Self::Stream>;

    fn direction(&self) -> Direction;

    fn is_closed(&self) -> bool;

    async fn close(&self) -> Result<(), Error>;
}

#[async_trait]
pub trait Dialer {
    type Conn;
    type PeerStore;

    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Self::Conn, Error>;

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    fn peer_store(&self) -> Self::PeerStore;

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn peers(&self) -> Vec<PeerId>;

    async fn conns(&self) -> Vec<Self::Conn>;

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Self::Conn>;
}

#[async_trait]
pub trait Network: Send + Sync + Clone {
    type Stream;

    async fn close(&self) -> Result<(), Error>;

    async fn new_stream(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        proto_id: ProtocolId,
    ) -> Result<Self::Stream, Error>;

    async fn listen(&mut self, laddr: Multiaddr) -> Result<(), Error>;
}

#[async_trait]
pub trait RemoteConnHandler: Send + Sync + Clone {
    type Conn;

    async fn handle(&self, conn: Self::Conn);
}

#[async_trait]
pub trait RemoteStreamHandler: Send + Sync + Clone {
    type Stream;

    async fn handle(&self, stream: Self::Stream);
}

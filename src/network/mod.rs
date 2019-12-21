pub mod conn;
pub mod stream;
pub use conn::QuicConn;

use crate::{
    transport::{CapableConn, MuxedStream},
    multiaddr::Multiaddr,
    crypto::PeerId,
};

use anyhow::Error;
use creep::Context;
use async_trait::async_trait;
use derive_more::Display;

pub enum NetworkEvent<Network, Conn, Stream> {
    Listen(Network, Multiaddr),
    ListenClose(Network, Multiaddr),
    Connected(Network, Conn),
    Disconnected(Network, Conn),
    OpenedStream(Network, Stream),
    ClosedStream(Network, Stream),
}

#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]
pub enum Connectdness {
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

// TODO: Item should be protocol message
#[async_trait]
pub trait Stream: MuxedStream {
    type Conn: Clone + Send;

    fn protocol(&self) -> Option<ProtocolId>;

    fn set_protocol(&mut self, id: ProtocolId);

    fn direction(&self) -> Direction;

    fn conn(&self) -> Self::Conn;
}

#[async_trait]
pub trait Conn: CapableConn + Clone + Send {
    type Stream;

    async fn new_stream(&self, proto_id: &ProtocolId) -> Result<Self::Stream, Error>;

    async fn streams(&self) -> &[Self::Stream];

    fn direction(&self) -> Direction;
}

#[async_trait]
pub trait Dialer {
    type Conn;
    type PeerStore;

    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Self::Conn, Error>;

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    fn peer_store(&self) -> Self::PeerStore;

    fn connectedness(&self, peer_id: &PeerId) -> Connectdness;

    fn peers(&self) -> &[PeerId];

    fn conns(&self) -> &[Self::Conn];

    fn conn_to_peer(&self, peer_id: &PeerId) -> Self::Conn;
}

#[async_trait]
pub trait Network {
    type Stream;

    fn set_remote_conn_handler(&self, handler: impl RemoteConnHandler);

    fn set_remote_stream_handler(&self, handler: impl RemoteStreamHandler);

    fn close(&self) -> Result<(), Error>;

    async fn new_stream(&self, ctx: Context, peer_id: &PeerId, proto_id: ProtocolId) -> Result<Self::Stream, Error>;

    async fn listen(laddr: Multiaddr) -> Result<(), Error>;
}

#[async_trait]
pub trait RemoteConnHandler {
    type Conn;

    async fn handle(&self, conn: Self::Conn);
}

#[async_trait]
pub trait RemoteStreamHandler {
    type Stream;

    async fn handle(&self, stream: Self::Stream);
}

use super::{Connectedness, Direction, Protocol};
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
use dyn_clone::DynClone;
use futures::io::{AsyncRead, AsyncWrite};

use std::ops::Deref;

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

    fn peer_store(&self) -> Box<dyn PeerStore>;

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn peers(&self) -> Vec<PeerId>;

    async fn conns(&self) -> Vec<Box<dyn Conn>>;

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>>;
}

#[async_trait]
pub trait Network: Send + Sync + DynClone {
    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Box<dyn Conn>, Error>;

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error>;

    async fn peers(&self) -> Vec<PeerId>;

    async fn conns(&self) -> Vec<Box<dyn Conn>>;

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>>;

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

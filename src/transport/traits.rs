use crate::crypto::{PeerId, PublicKey};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use dyn_clone::DynClone;
use futures::prelude::{AsyncRead, AsyncWrite, Stream};
use parity_multiaddr::Multiaddr;

use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
};

#[async_trait]
pub trait MuxedStream: AsyncRead + AsyncWrite + Stream<Item = Bytes> + Unpin + Send {
    async fn close(&mut self) -> Result<(), Error>;

    fn reset(&mut self);
}

#[async_trait]
impl<S> MuxedStream for S
where
    S: DerefMut<Target = dyn MuxedStream>
        + Send
        + Unpin
        + Stream<Item = Bytes>
        + AsyncWrite
        + AsyncRead,
{
    async fn close(&mut self) -> Result<(), Error> {
        self.deref_mut().close().await
    }

    fn reset(&mut self) {
        self.deref_mut().reset()
    }
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

dyn_clone::clone_trait_object!(CapableConn);

#[async_trait]
impl<C> CapableConn for C
where
    C: Deref<Target = dyn CapableConn> + Sync + Send + DynClone + ConnSecurity + ConnMultiaddr,
{
    async fn open_stream(&self) -> Result<Box<dyn MuxedStream>, Error> {
        self.deref().open_stream().await
    }

    async fn accept_stream(&self) -> Result<Box<dyn MuxedStream>, Error> {
        self.deref().accept_stream().await
    }

    fn is_closed(&self) -> bool {
        self.deref().is_closed()
    }

    async fn close(&self) -> Result<(), Error> {
        self.deref().close().await
    }

    fn transport(&self) -> Box<dyn Transport> {
        self.deref().transport()
    }
}

impl<C> ConnSecurity for C
where
    C: Deref<Target = dyn CapableConn>,
{
    fn local_peer(&self) -> PeerId {
        self.deref().local_peer()
    }

    fn remote_peer(&self) -> PeerId {
        self.deref().remote_peer()
    }

    fn remote_public_key(&self) -> PublicKey {
        self.deref().remote_public_key()
    }
}

impl<C> ConnMultiaddr for C
where
    C: Deref<Target = dyn CapableConn>,
{
    fn local_multiaddr(&self) -> Multiaddr {
        self.deref().local_multiaddr()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.deref().remote_multiaddr()
    }
}

#[async_trait]
pub trait Listener: Send {
    async fn accept(&mut self) -> Result<Box<dyn CapableConn>, Error>;

    fn close(&mut self) -> Result<(), Error>;

    fn addr(&self) -> SocketAddr;

    fn multiaddr(&self) -> Multiaddr;
}

#[async_trait]
impl<L> Listener for L
where
    L: DerefMut<Target = dyn Listener> + Send,
{
    async fn accept(&mut self) -> Result<Box<dyn CapableConn>, Error> {
        self.deref_mut().accept().await
    }

    fn close(&mut self) -> Result<(), Error> {
        self.deref_mut().close()
    }

    fn addr(&self) -> SocketAddr {
        self.deref().addr()
    }

    fn multiaddr(&self) -> Multiaddr {
        self.deref().multiaddr()
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

    async fn local_multiaddr(&self) -> Option<Multiaddr>;
}

#[async_trait]
impl<T> Transport for T
where
    T: DerefMut<Target = dyn Transport + Send> + Sync + Send + DynClone,
{
    async fn dial(
        &self,
        ctx: Context,
        raddr: Multiaddr,
        peer_id: PeerId,
    ) -> Result<Box<dyn CapableConn>, Error> {
        self.deref().dial(ctx, raddr, peer_id).await
    }

    fn can_dial(&self, raddr: &Multiaddr) -> bool {
        self.deref().can_dial(raddr)
    }

    async fn listen(&mut self, laddr: Multiaddr) -> Result<Box<dyn Listener>, Error> {
        self.deref_mut().listen(laddr).await
    }

    async fn local_multiaddr(&self) -> Option<Multiaddr> {
        self.deref().local_multiaddr().await
    }
}

dyn_clone::clone_trait_object!(Transport);

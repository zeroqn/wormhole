pub mod switch;
pub mod r#impl;
pub mod framed_stream;
pub use switch::DefaultSwitch;
pub use framed_stream::FramedStream;
pub use r#impl::{DefaultHost, DefaultStreamHandler};

use crate::{network::{self, Protocol, ProtocolId, NetworkEvent, Connectedness}, crypto::{PeerId, PublicKey}, multiaddr::Multiaddr};

use futures::{channel::mpsc, io::{AsyncRead, AsyncWrite}};
use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use dyn_clone::DynClone;

use std::{collections::HashSet};

#[async_trait]
pub trait ResetStream {
    async fn reset(&mut self);
}

#[async_trait]
impl<T> ResetStream for T
where
    T: network::Stream + Send + Unpin
{
    async fn reset(&mut self) {
        self.reset().await
    }
}

impl<T> RawStream for T
where
    T: network::Stream + Send + Unpin
{}

pub trait RawStream: AsyncRead + AsyncWrite + ResetStream + Send + Unpin {}

pub trait MatchProtocol<'a>: Send + DynClone {
    fn r#match(&self, name: &'a str) -> bool;
}

#[async_trait]
pub trait ProtocolHandler: Send + Sync + DynClone {
    fn proto_id(&self) -> ProtocolId;

    fn proto_name(&self) -> &'static str;

    async fn handle(&self, stream: &mut FramedStream);
}

#[async_trait]
pub trait Switch: Sync + Send + DynClone {
    async fn add_handler(&self, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    // Match protocol name
    async fn add_match_handler(&self, r#match: impl for<'a> MatchProtocol<'a> + 'static, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    async fn remove_handler(&self, proto_id: ProtocolId);

    async fn negotiate(&self, stream: &mut FramedStream) -> Result<Box<dyn ProtocolHandler>, Error>;

    async fn handle(&self, mut stream: FramedStream) {
        let proto_handler = match self.negotiate(&mut stream).await {
            Ok(handler) => handler,
            Err(_) => {
                // Reset stream
                return stream.reset().await;
            }
        };

        proto_handler.handle(&mut stream).await
    }
}

#[async_trait]
pub trait PeerStore: Sync + Send {
    async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey>;

    async fn set_pubkey(&self, peer_id: PeerId, pubkey: PublicKey);

    async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn set_connectedness(&self, peer_id: PeerId, connectedness: Connectedness);

    async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<HashSet<Multiaddr>>;

    async fn add_multiaddr(&self, peer_id: PeerId, addr: Multiaddr);
}

#[async_trait]
pub trait Host {
    type Switch;
    type Network: network::Network;
    type PeerStore;

    fn peer_id(&self) -> &PeerId;

    fn peer_store(&self) -> Self::PeerStore;

    async fn add_handler(&self, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    // Match protocol name
    async fn add_match_handler(&self, r#match: impl for<'a> MatchProtocol<'a> + 'static, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    async fn remove_handler(&self, proto_id: ProtocolId);

    async fn connect(&self, ctx: Context, peer_id: &PeerId, raddr: Option<&Multiaddr>) -> Result<(), Error>;

    async fn new_stream(&self, ctx: Context, peer_id: &PeerId, protocol: Protocol) -> Result<FramedStream, Error>;

    async fn close(&self) -> Result<(), Error>;

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent<Self::Network>>;
}

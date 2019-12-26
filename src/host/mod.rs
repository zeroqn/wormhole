pub mod switch;
pub use switch::DefaultSwitch;

use crate::{network::{self, Protocol, ProtocolId, NetworkEvent, Connectedness}, crypto::{PeerId, PublicKey}, multiaddr::Multiaddr};

use bytes::Bytes;
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

pub trait RawStream: AsyncRead + AsyncWrite + ResetStream + futures::stream::Stream<Item = Bytes> + Send + Unpin {}

pub trait MatchProtocol<'a>: Send + DynClone {
    fn r#match(&self, name: &'a str) -> bool;
}

#[async_trait]
pub trait ProtocolHandler: Send + Sync + DynClone {
    fn proto_id(&self) -> ProtocolId;

    fn proto_name(&self) -> &'static str;

    async fn handle(&self, stream: &mut dyn RawStream);
}

#[async_trait]
pub trait Switch: Sync + Send + DynClone {
    async fn add_handler(&self, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    // Match protocol name
    async fn add_match_handler(&self, r#match: impl for<'a> MatchProtocol<'a> + 'static, handler: impl ProtocolHandler + 'static) -> Result<(), Error>;

    async fn remove_handler(&self, proto_id: ProtocolId);

    async fn negotiate(&self, stream: &mut dyn RawStream) -> Result<Box<dyn ProtocolHandler>, Error>;

    async fn handle(&self, mut stream: Box<dyn RawStream>) {
        let proto_handler = match self.negotiate(&mut *stream).await {
            Ok(handler) => handler,
            Err(_) => {
                // Reset stream
                return stream.reset().await;
            }
        };

        proto_handler.handle(&mut *stream).await
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
    type Switch: Switch;
    type Network: network::Network;
    type PeerStore: PeerStore;

    fn peer_id(&self) -> &PeerId;

    fn peer_store(&self) -> Self::PeerStore;

    async fn connect(&self, ctx: Context, peer_id: &PeerId, raddr: Option<&Multiaddr>) -> Result<(), Error>;

    async fn new_stream(&self, ctx: Context, peer_id: &PeerId, proto: Protocol) -> Result<<Self::Network as network::Network>::Stream, Error>;

    async fn close(&self) -> Result<(), Error>;

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent<Self::Network>>;
}

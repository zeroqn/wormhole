use crate::{network::{self, ProtocolId, NetworkEvent, Connectedness}, crypto::{PeerId, PublicKey}, multiaddr::Multiaddr};

use bytes::Bytes;
use futures::{channel::mpsc, io::{AsyncRead, AsyncWrite}};
use anyhow::Error;
use async_trait::async_trait;
use creep::Context;

use std::{pin::Pin, collections::HashSet};

pub trait RawStream: AsyncRead + AsyncWrite + futures::stream::Stream<Item = Bytes> {}

#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    fn id(&self) -> ProtocolId;
    
    fn string(&self) -> &'static str;

    async fn handle(&self, stream: Pin<&mut dyn RawStream>) -> Result<(), Error>;
}

#[async_trait]
pub trait Switch: Sync + Send + Clone {
    fn add_handler(&self, handler: impl ProtocolHandler) -> Result<(), Error>;
    
    fn add_match_handler(&self, matcher: impl Fn(ProtocolId) -> bool, handler: impl ProtocolHandler) -> Result<(), Error>;

    fn remove_handler(&self, id: ProtocolId);

    async fn negotiate(&self, stream: Pin<&mut dyn RawStream>) -> Result<Box<dyn ProtocolHandler>, Error>;
}

#[async_trait]
pub trait PeerStore: Sync + Send + Clone {
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

    async fn connect(&self, ctx: Context, peer_id: &PeerId, raddr: Option<&Multiaddr>) -> Result<(), Error>;

    async fn new_stream(&self, ctx: Context, peer_id: &PeerId, proto_id: ProtocolId) -> Result<<Self::Network as network::Network>::Stream, Error>;

    async fn close(&self) -> Result<(), Error>;

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent<Self::Network>>;
}

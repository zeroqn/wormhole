use super::FramedStream;
use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network::{Connectedness, NetworkEvent, Protocol, ProtocolId},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use dyn_clone::DynClone;
use futures::channel::mpsc;
use tracing::{debug, error};

use std::collections::HashSet;

pub trait MatchProtocol: Send + DynClone {
    fn r#match<'a>(&self, name: &'a str) -> bool;
}

dyn_clone::clone_trait_object!(MatchProtocol);

#[async_trait]
pub trait ProtocolHandler: Send + Sync + DynClone {
    fn proto_id(&self) -> ProtocolId;

    fn proto_name(&self) -> &'static str;

    async fn handle(&self, stream: FramedStream);
}

dyn_clone::clone_trait_object!(ProtocolHandler);

#[async_trait]
pub trait Switch: Sync + Send + DynClone {
    async fn add_handler(&self, handler: Box<dyn ProtocolHandler>) -> Result<(), Error>;

    // Match protocol name
    async fn add_match_handler(
        &self,
        r#match: Box<dyn MatchProtocol>,
        handler: Box<dyn ProtocolHandler>,
    ) -> Result<(), Error>;

    async fn remove_handler(&self, proto_id: ProtocolId);

    async fn negotiate(&self, stream: &mut FramedStream)
        -> Result<Box<dyn ProtocolHandler>, Error>;

    async fn handle(&self, mut stream: FramedStream) {
        let proto_handler = match self.negotiate(&mut stream).await {
            Ok(handler) => handler,
            Err(err) => {
                // Reset stream
                error!("negotiate: {}", err);
                return stream.reset().await;
            }
        };

        debug!("accept protocol {}", proto_handler.proto_name());

        proto_handler.handle(stream).await
    }
}

dyn_clone::clone_trait_object!(Switch);

#[async_trait]
pub trait PeerStore: Sync + Send + DynClone {
    async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey>;

    async fn set_pubkey(&self, peer_id: PeerId, pubkey: PublicKey);

    async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn set_connectedness(&self, peer_id: PeerId, connectedness: Connectedness);

    async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<HashSet<Multiaddr>>;

    async fn add_multiaddr(&self, peer_id: PeerId, addr: Multiaddr);
}

dyn_clone::clone_trait_object!(PeerStore);

#[async_trait]
pub trait Host: Sync + Send + DynClone {
    fn peer_id(&self) -> &PeerId;

    fn peer_store(&self) -> crate::peer_store::PeerStore;

    async fn add_handler(&self, handler: Box<dyn ProtocolHandler>) -> Result<(), Error>;

    // Match protocol name
    async fn add_match_handler(
        &self,
        r#match: Box<dyn MatchProtocol>,
        handler: Box<dyn ProtocolHandler>,
    ) -> Result<(), Error>;

    async fn remove_handler(&self, proto_id: ProtocolId);

    async fn connect(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        raddr: Option<&Multiaddr>,
    ) -> Result<(), Error>;

    async fn new_stream(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        protocol: Protocol,
    ) -> Result<FramedStream, Error>;

    async fn close(&self) -> Result<(), Error>;

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent>;
}

dyn_clone::clone_trait_object!(Host);

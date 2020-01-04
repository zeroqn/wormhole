use super::{
    switch::{Offer, Use},
    DefaultSwitch, FramedStream, Host, MatchProtocol, ProtocolHandler, Switch,
};
use crate::{
    crypto::{PeerId, PrivateKey, PublicKey},
    multiaddr::Multiaddr,
    network::{
        Conn, Network, NetworkEvent, Protocol, ProtocolId, QuicNetwork, RemoteConnHandler,
        RemoteStreamHandler, Stream,
    },
    peer_store::PeerStore,
};

use anyhow::{Context as AnyHowContext, Error};
use async_trait::async_trait;
use bytes::BytesMut;
use creep::Context;
use futures::{channel::mpsc, SinkExt, TryStreamExt};
use prost::Message;
use tracing::{debug, trace_span};

use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum HostError {
    #[error("no protocol use message returned")]
    NoProtocolUse,
}

#[async_trait]
impl RemoteConnHandler for () {
    async fn handle(&self, _conn: Box<dyn Conn>) {}
}

#[derive(Clone)]
pub struct DefaultStreamHandler {
    switch: Arc<DefaultSwitch>,
}

impl DefaultStreamHandler {
    pub fn new(switch: Arc<DefaultSwitch>) -> Self {
        DefaultStreamHandler { switch }
    }
}

#[async_trait]
impl RemoteStreamHandler for DefaultStreamHandler {
    async fn handle(&self, stream: Box<dyn Stream>) {
        self.switch.handle(FramedStream::new(stream)).await
    }
}

pub type DefaultNetwork = QuicNetwork<(), DefaultStreamHandler>;

#[derive(Clone)]
pub struct QuicHost {
    network: DefaultNetwork,
    switch: Arc<DefaultSwitch>,
    peer_store: PeerStore,

    _pubkey: PublicKey,
    peer_id: PeerId,
}

impl QuicHost {
    pub fn make(host_privkey: &PrivateKey, peer_store: PeerStore) -> Result<Self, Error> {
        let switch = Arc::new(DefaultSwitch::default());
        let stream_handler = DefaultStreamHandler::new(Arc::clone(&switch));

        let network = QuicNetwork::make(host_privkey, peer_store.clone(), (), stream_handler)?;

        let pubkey = host_privkey.pubkey();
        let peer_id = pubkey.peer_id();

        let host = QuicHost {
            network,
            switch,
            peer_store,

            _pubkey: pubkey,
            peer_id,
        };

        Ok(host)
    }

    pub async fn listen(&mut self, multiaddr: Multiaddr) -> Result<(), Error> {
        Ok(self.network.listen(multiaddr).await?)
    }

    // TODO: multiple protocols support
    async fn try_protocol(
        _ctx: Context,
        framed_stream: &mut FramedStream,
        protocol: Protocol,
    ) -> Result<(), Error> {
        use HostError::*;

        let span = trace_span!("try protocol");
        let _guard = span.enter();

        debug!("offer {}", protocol);
        let offer = Offer::with_names(vec![protocol.name.to_owned()]);
        let mut offer_data = BytesMut::new();
        offer.encode(&mut offer_data)?;

        framed_stream.send(offer_data.freeze()).await?;
        debug!("offer sent");

        let maybe_use = framed_stream.try_next().await?.ok_or(NoProtocolUse)?;
        let r#use = Use::decode(maybe_use).context("decode protocol Use message")?;
        debug!("got use {:?}", r#use);

        Ok(())
    }
}

#[async_trait]
impl Host for QuicHost {
    fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    fn peer_store(&self) -> PeerStore {
        self.peer_store.clone()
    }

    async fn add_handler(&self, handler: Box<dyn ProtocolHandler>) -> Result<(), Error> {
        Ok(self.switch.add_handler(handler).await?)
    }

    // Match protocol name
    async fn add_match_handler(
        &self,
        r#match: Box<dyn MatchProtocol>,
        handler: Box<dyn ProtocolHandler>,
    ) -> Result<(), Error> {
        Ok(self.switch.add_match_handler(r#match, handler).await?)
    }

    async fn remove_handler(&self, proto_id: ProtocolId) {
        self.switch.remove_handler(proto_id).await
    }

    async fn connect(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        raddr: Option<&Multiaddr>,
    ) -> Result<(), Error> {
        if let Some(raddr) = raddr {
            self.peer_store
                .set_multiaddr(peer_id, raddr.to_owned())
                .await;
        }

        Ok(self.network.connect(ctx, peer_id).await?)
    }

    async fn new_stream(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        protocol: Protocol,
    ) -> Result<FramedStream, Error> {
        let raw_stream = self
            .network
            .new_stream(ctx.clone(), peer_id, protocol)
            .await?;
        let mut framed_stream = FramedStream::new(raw_stream);

        if let Err(err) = Self::try_protocol(ctx, &mut framed_stream, protocol).await {
            framed_stream.reset().await;
            return Err(err);
        }

        Ok(framed_stream)
    }

    async fn close(&self) -> Result<(), Error> {
        Ok(self.network.close().await?)
    }

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent> {
        todo!()
    }
}

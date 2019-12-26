use super::{DefaultSwitch, Host, Switch, FramedStream, switch::{Offer, Use}, ProtocolHandler, MatchProtocol};
use crate::{
    crypto::{PeerId, PublicKey, PrivateKey},
    multiaddr::Multiaddr,
    network::{QuicConn, QuicNetwork, QuicStream, RemoteConnHandler, RemoteStreamHandler, Protocol, ProtocolId, NetworkEvent, Network},
    peer_store::PeerStore,
};

use bytes::BytesMut;
use prost::Message;
use anyhow::{Error, Context as AnyHowContext};
use async_trait::async_trait;
use creep::Context;
use futures::{channel::mpsc, TryStreamExt, SinkExt};

use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum HostError {
    #[error("no protocol use message returned")]
    NoProtocolUse,
}

#[async_trait]
impl RemoteConnHandler for () {
    type Conn = QuicConn;

    async fn handle(&self, _conn: Self::Conn) {}
}

#[derive(Clone)]
pub struct DefaultStreamHandler {
    switch: Arc<DefaultSwitch>,
}

impl DefaultStreamHandler {
    pub fn new(switch: Arc<DefaultSwitch>) -> Self {
        DefaultStreamHandler {
            switch,
        }
    }
}

#[async_trait]
impl RemoteStreamHandler for DefaultStreamHandler {
    type Stream = QuicStream;

    async fn handle(&self, stream: Self::Stream) {
        self.switch.handle(FramedStream::new(Box::new(stream))).await
    }
}

pub type DefaultNetwork = QuicNetwork<(), DefaultStreamHandler>;

pub struct DefaultHost {
    network: DefaultNetwork,
    switch: Arc<DefaultSwitch>,
    peer_store: PeerStore,

    _pubkey: PublicKey,
    peer_id: PeerId,
}

impl DefaultHost {
    pub fn make(host_privkey: &PrivateKey, peer_store: PeerStore) -> Result<Self, Error> {
        let switch = Arc::new(DefaultSwitch::default());
        let stream_handler = DefaultStreamHandler::new(Arc::clone(&switch));

        let network = QuicNetwork::make(host_privkey, peer_store.clone(), (), stream_handler)?;

        let pubkey = host_privkey.pubkey();
        let peer_id = pubkey.peer_id();

        let host = DefaultHost {
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
    async fn negotiate(_ctx: Context, framed_stream: &mut FramedStream, protocol: Protocol) -> Result<(), Error> {
        use HostError::*;

        let offer = Offer::with_names(vec![protocol.name.to_owned()]);
        let mut offer_data = BytesMut::new();
        offer.encode(&mut offer_data)?;

        framed_stream.send(offer_data.freeze()).await?;

        let maybe_use = framed_stream.try_next().await?.ok_or(NoProtocolUse)?;
        Use::decode(maybe_use).context("decode protocol Use message")?;

        Ok(())
    }
}

#[async_trait]
impl Host for DefaultHost {
    type Switch = DefaultSwitch;
    type Network = DefaultNetwork;
    type PeerStore = PeerStore;

    fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    fn peer_store(&self) -> Self::PeerStore {
        self.peer_store.clone()
    }

    async fn add_handler(&self, handler: impl ProtocolHandler + 'static) -> Result<(), Error> {
        Ok(self.switch.add_handler(handler).await?)
    }

    // Match protocol name
    async fn add_match_handler(&self, r#match: impl for<'a> MatchProtocol<'a> + 'static, handler: impl ProtocolHandler + 'static) -> Result<(), Error> {
        Ok(self.switch.add_match_handler(r#match, handler).await?)
    }

    async fn remove_handler(&self, proto_id: ProtocolId) {
        self.switch.remove_handler(proto_id).await
    }

    async fn connect(&self, ctx: Context, peer_id: &PeerId, raddr: Option<&Multiaddr>) -> Result<(), Error> {
        if let Some(raddr) = raddr {
            self.peer_store.set_multiaddr(peer_id, raddr.to_owned()).await;
        }

        Ok(self.network.connect(ctx, peer_id).await?)
    }

    async fn new_stream(&self, ctx: Context, peer_id: &PeerId, protocol: Protocol) -> Result<FramedStream, Error> {
        let raw_stream = self.network.new_stream(ctx.clone(), peer_id, protocol).await?;
        let mut framed_stream = FramedStream::new(Box::new(raw_stream));

        if let Err(err) = Self::negotiate(ctx, &mut framed_stream, protocol).await {
            framed_stream.reset().await;
            return Err(err);
        }

        Ok(framed_stream)
    }

    async fn close(&self) -> Result<(), Error> {
        Ok(self.network.close().await?)
    }

    async fn subscribe(&self) -> mpsc::Receiver<NetworkEvent<Self::Network>> {
        todo!()
    }
}

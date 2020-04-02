use crate::{
    crypto::PeerId,
    host::{FramedStream, ProtocolHandler},
    multiaddr::Multiaddr,
    network::{Direction, Protocol, ProtocolId},
    peer_store::PeerStore,
};

use anyhow::{Context as ErrorContext, Error};
use async_trait::async_trait;
use bytes::BytesMut;
use creep::Context;
use derive_more::Display;
use dyn_clone::DynClone;
use futures::{channel::mpsc::Sender, SinkExt, TryStreamExt};
use prost::Message;
use tokio::time::delay_for;
use tracing::{debug, error};

use std::{convert::TryFrom, ops::Deref, time::Duration};

/// Protocol name, version appended
const PROTO_NAME: &str = "/bootstrap/1.0";

#[async_trait]
pub trait BootstrapPeerStore: PeerStore {
    async fn register(&self, peers: Vec<(PeerId, Multiaddr)>);
    async fn contains(&self, peer_id: &PeerId) -> bool;
    async fn choose(&self, max: usize) -> Vec<(PeerId, Multiaddr)>;
}

dyn_clone::clone_trait_object!(BootstrapPeerStore);

#[async_trait]
impl<S> BootstrapPeerStore for S
where
    S: Deref<Target = dyn BootstrapPeerStore> + DynClone + Send + Sync + PeerStore,
{
    async fn register(&self, peers: Vec<(PeerId, Multiaddr)>) {
        self.deref().register(peers).await
    }

    async fn contains(&self, peer_id: &PeerId) -> bool {
        self.deref().contains(peer_id).await
    }

    async fn choose(&self, max: usize) -> Vec<(PeerId, Multiaddr)> {
        self.deref().choose(max).await
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BootstrapError {
    #[error("no new arrive")]
    NoNewArrive,
}

#[derive(Debug, Display)]
pub enum Event {
    #[display(fmt = "got new peer info from bootstrap peer")]
    NewArrived,
}

#[derive(Debug, Display, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[display(fmt = "publisher mode")]
    Publisher,
    #[display(fmt = "subscriber mode")]
    Subscriber,
}

#[derive(Message)]
struct PeerInfo {
    #[prost(bytes, tag = "1")]
    peer_id: Vec<u8>,
    #[prost(bytes, tag = "2")]
    multiaddr: Vec<u8>,
}

impl PeerInfo {
    pub fn new(peer_addr: (PeerId, Multiaddr)) -> Self {
        PeerInfo {
            peer_id: peer_addr.0.into_inner().to_vec(),
            multiaddr: peer_addr.1.to_vec(),
        }
    }

    pub fn into_pair(self) -> Result<(PeerId, Multiaddr), Error> {
        let peer_id = PeerId::from_bytes(self.peer_id).context("decode peer id")?;
        let multiaddr = Multiaddr::try_from(self.multiaddr).context("decode multiaddr")?;

        Ok((peer_id, multiaddr))
    }
}

#[derive(Message)]
struct DiscoverPeers {
    #[prost(message, repeated, tag = "1")]
    data: Vec<PeerInfo>,
}

// TODO: shutdown logic
#[derive(Clone)]
pub struct BootstrapProtocol {
    proto_id: ProtocolId,

    mode: Mode,

    peer_store: Box<dyn BootstrapPeerStore>,
    evt_tx: Sender<Event>,
}

impl BootstrapProtocol {
    pub fn new(
        proto_id: ProtocolId,
        mode: Mode,
        peer_store: impl BootstrapPeerStore + 'static,
        evt_tx: Sender<Event>,
    ) -> Self {
        BootstrapProtocol {
            proto_id,

            mode,

            peer_store: Box::new(peer_store),
            evt_tx,
        }
    }

    pub fn protocol(proto_id: ProtocolId) -> Protocol {
        Protocol::new(*proto_id, PROTO_NAME)
    }

    // TODO: pass timeout through ctx
    pub async fn publish_ourself(
        _ctx: Context,
        peer_id: PeerId,
        local_multiaddr: Multiaddr,
        w_stream: &mut FramedStream,
    ) -> Result<(), Error> {
        debug!("publish ourself {}", local_multiaddr);

        let our_info = PeerInfo::new((peer_id, local_multiaddr));

        let encoded_ourself = {
            let ourself = DiscoverPeers {
                data: vec![our_info],
            };
            let mut encoded_ourself = BytesMut::new();
            ourself
                .encode(&mut encoded_ourself)
                .context("encode ourself")?;

            encoded_ourself.freeze()
        };

        w_stream
            .send(encoded_ourself)
            .await
            .context("publish ourself")?;
        Ok(())
    }

    pub async fn publish_peers(
        _ctx: Context,
        peer_store: &dyn BootstrapPeerStore,
        w_stream: &mut FramedStream,
    ) -> Result<(), Error> {
        let peer_infos = peer_store
            .deref()
            .choose(40)
            .await
            .into_iter()
            .map(PeerInfo::new)
            .collect();

        let encoded_peers = {
            let peers = DiscoverPeers { data: peer_infos };
            let mut encoded_peers = BytesMut::new();
            peers
                .encode(&mut encoded_peers)
                .context("encode push peers")?;

            encoded_peers.freeze()
        };

        w_stream.send(encoded_peers).await.context("push peers")?;
        Ok(())
    }

    // TODO: set interval through ctx
    pub async fn interval_publish_peers(
        ctx: Context,
        peer_store: Box<dyn BootstrapPeerStore>,
        w_stream: &mut FramedStream,
    ) -> Result<(), Error> {
        loop {
            Self::publish_peers(ctx.clone(), peer_store.deref(), w_stream).await?;

            for _ in 0..20 {
                // TODO: better shutdown check
                delay_for(Duration::from_secs(2)).await
            }
        }
    }

    pub async fn new_arrived(
        _ctx: Context,
        peer_store: &dyn BootstrapPeerStore,
        r_stream: &mut FramedStream,
        evt_tx: &mut Sender<Event>,
    ) -> Result<(), Error> {
        let encoded_peers = r_stream
            .try_next()
            .await?
            .ok_or(BootstrapError::NoNewArrive)?;

        let discover_peers: DiscoverPeers =
            prost::Message::decode(encoded_peers).context("decode new arrived peers")?;

        let peer_pairs = discover_peers
            .data
            .into_iter()
            .map(PeerInfo::into_pair)
            .collect::<Result<Vec<_>, Error>>()?;

        debug!("new arrived, register now");
        peer_store.register(peer_pairs).await;

        evt_tx
            .try_send(Event::NewArrived)
            .context("impossible, try send new arrived event")?;

        Ok(())
    }
}

#[async_trait]
impl ProtocolHandler for BootstrapProtocol {
    fn proto_id(&self) -> ProtocolId {
        self.proto_id
    }

    fn proto_name(&self) -> &'static str {
        PROTO_NAME
    }

    async fn handle(&self, stream: FramedStream) {
        let direction = stream.conn().direction();
        let mode = self.mode;
        let mut w_stream = stream.clone();

        let mut stream_reset = false;

        match (mode, direction) {
            (Mode::Publisher, Direction::Inbound) => {
                let peer_store = self.peer_store.clone();

                tokio::spawn(async move {
                    if let Err(err) =
                        Self::interval_publish_peers(Context::new(), peer_store, &mut w_stream)
                            .await
                    {
                        error!("publish peer {}", err);
                    }
                });
            }
            (Mode::Publisher, Direction::Outbound) | (Mode::Subscriber, Direction::Outbound) => {
                let our_id = w_stream.conn().local_peer();
                let our_multiaddr = w_stream.conn().local_multiaddr();

                tokio::spawn(async move {
                    if let Err(err) =
                        Self::publish_ourself(Context::new(), our_id, our_multiaddr, &mut w_stream)
                            .await
                    {
                        error!("publish ourself {}", err);
                    }
                });
            }
            _ => {
                // Publisher should not dial out to collect peers. Subscribe
                // should close incoming stream for peer query.
                stream_reset = true;
                w_stream.reset().await;
            }
        }

        if !stream_reset {
            let our_id = stream.conn().local_peer();
            let mut r_stream = stream;
            let mut evt_tx = self.evt_tx.clone();

            loop {
                if let Err(err) = Self::new_arrived(
                    Context::new(),
                    self.peer_store.deref(),
                    &mut r_stream,
                    &mut evt_tx,
                )
                .await
                {
                    error!("{} new arrived: {}", our_id, err);
                    break;
                }
            }
        }
    }
}

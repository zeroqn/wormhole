use crate::{
    crypto::PeerId,
    host::{FramedStream, ProtocolHandler},
    multiaddr::Multiaddr,
    network::{Connectedness, Direction, Protocol, ProtocolId},
    peer_store::{
        simple_store::{self, SimplePeerStore},
        PeerStore,
    },
};

use anyhow::{Context as ErrorContext, Error};
use async_trait::async_trait;
use bytes::BytesMut;
use creep::Context;
use derive_more::Display;
use futures::{channel::mpsc::Sender, SinkExt, TryStreamExt};
use prost::Message;
use tokio::time::delay_for;
use tracing::{debug, error};

use std::{convert::TryFrom, time::Duration};

/// Protocol name, version appended
const PROTO_NAME: &str = "/bootstrap/1.0";

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

    pub fn into_store_info(self) -> Result<simple_store::PeerInfo, Error> {
        let peer_id = PeerId::from_bytes(self.peer_id).context("decode peer id")?;
        let multiaddr = Multiaddr::try_from(self.multiaddr).context("decode multiaddr")?;

        Ok(simple_store::PeerInfo::with_addr(peer_id, multiaddr))
    }
}

#[derive(Message)]
struct Peers {
    #[prost(message, repeated, tag = "1")]
    data: Vec<PeerInfo>,
}

// TODO: shutdown logic
#[derive(Clone)]
pub struct BootstrapProtocol {
    proto_id: ProtocolId,

    mode: Mode,

    peer_store: SimplePeerStore,
    evt_tx: Sender<Event>,
}

impl BootstrapProtocol {
    pub fn new(
        proto_id: ProtocolId,
        mode: Mode,
        peer_store: SimplePeerStore,
        evt_tx: Sender<Event>,
    ) -> Self {
        BootstrapProtocol {
            proto_id,

            mode,

            peer_store,
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
            let ourself = Peers {
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
        peer_store: &SimplePeerStore,
        w_stream: &mut FramedStream,
    ) -> Result<(), Error> {
        let peer_infos = peer_store
            .choose(40)
            .await
            .into_iter()
            .map(PeerInfo::new)
            .collect();

        let encoded_peers = {
            let peers = Peers { data: peer_infos };
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
        peer_store: &SimplePeerStore,
        w_stream: &mut FramedStream,
    ) -> Result<(), Error> {
        loop {
            Self::publish_peers(ctx.clone(), peer_store, w_stream).await?;

            for _ in 0..20 {
                // TODO: better shutdown check
                delay_for(Duration::from_secs(2)).await
            }
        }
    }

    pub async fn new_arrived(
        _ctx: Context,
        peer_store: &SimplePeerStore,
        r_stream: &mut FramedStream,
        evt_tx: &mut Sender<Event>,
    ) -> Result<(), Error> {
        let encoded_peers = r_stream
            .try_next()
            .await?
            .ok_or(BootstrapError::NoNewArrive)?;

        let decoded_peers: Peers =
            prost::Message::decode(encoded_peers).context("decode new arrived peers")?;
        let store_peer_infos = decoded_peers
            .data
            .into_iter()
            .map(PeerInfo::into_store_info)
            .collect::<Result<Vec<_>, Error>>()?;

        debug!("new arrived");

        for mut peer_info in store_peer_infos.into_iter() {
            debug!("new arrived {}", peer_info.peer_id());

            if !peer_store.contains(peer_info.peer_id()).await {
                peer_info.set_connectedness(Connectedness::CanConnect);
                peer_store.register(peer_info).await
            } else {
                let peer_id = peer_info.peer_id().to_owned();
                if let Some(multiaddr) = peer_info.into_multiaddr() {
                    peer_store.add_multiaddr(&peer_id, multiaddr).await
                }
            }
        }

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
                        Self::interval_publish_peers(Context::new(), &peer_store, &mut w_stream)
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
                if let Err(err) =
                    Self::new_arrived(Context::new(), &self.peer_store, &mut r_stream, &mut evt_tx)
                        .await
                {
                    error!("{} new arrived: {}", our_id, err);
                    break;
                }
            }
        }
    }
}

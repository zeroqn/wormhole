use crate::{
    crypto::PeerId,
    host::{FramedStream, ProtocolHandler},
    multiaddr::Multiaddr,
    network::{Connectedness, Protocol, ProtocolId},
    peer_store::{self, PeerStore},
};

use anyhow::{Context as ErrorContext, Error};
use async_trait::async_trait;
use bytes::BytesMut;
use creep::Context;
use derive_more::Display;
use futures::{channel::mpsc::Sender, lock::BiLock, SinkExt, TryStreamExt};
use prost::Message;
use tokio::time::delay_for;
use tracing::{debug, error};

use std::{
    convert::TryFrom,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

type ReadStream = BiLock<FramedStream>;
type WriteStream = BiLock<FramedStream>;

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

    pub fn into_store_info(self) -> Result<peer_store::PeerInfo, Error> {
        let peer_id = PeerId::from_bytes(self.peer_id).context("decode peer id")?;
        let multiaddr = Multiaddr::try_from(self.multiaddr).context("decode multiaddr")?;

        Ok(peer_store::PeerInfo::with_addr(peer_id, multiaddr))
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
    peer_store: PeerStore,

    peer_id: PeerId,
    local_multiaddr: Multiaddr,

    // TODO: mask bits
    // If we aren't server, just publish ourself then listen on new arrived.
    is_server: Arc<AtomicBool>,

    event_tx: Sender<Event>,
}

impl BootstrapProtocol {
    pub fn new(
        peer_store: PeerStore,
        peer_id: PeerId,
        local_multiaddr: Multiaddr,
        is_server: bool,
        event_tx: Sender<Event>,
    ) -> Self {
        BootstrapProtocol {
            peer_store,

            peer_id,
            local_multiaddr,

            is_server: Arc::new(AtomicBool::new(is_server)),

            event_tx,
        }
    }

    pub fn protocol() -> Protocol {
        Protocol::new(1, "bootstrap")
    }

    // TODO: pass timeout through ctx
    pub async fn publish_ourself(
        _ctx: Context,
        peer_id: PeerId,
        local_multiaddr: Multiaddr,
        w_stream: WriteStream,
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
            .lock()
            .await
            .send(encoded_ourself)
            .await
            .context("publish ourself")?;
        Ok(())
    }

    pub async fn push_peers(
        _ctx: Context,
        peer_store: PeerStore,
        w_stream: &mut WriteStream,
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

        w_stream
            .lock()
            .await
            .send(encoded_peers)
            .await
            .context("push peers")?;
        Ok(())
    }

    // TODO: set interval through ctx
    pub async fn interval_push_peers(
        ctx: Context,
        peer_store: PeerStore,
        mut w_stream: WriteStream,
    ) -> Result<(), Error> {
        loop {
            Self::push_peers(ctx.clone(), peer_store.clone(), &mut w_stream).await?;

            for _ in 0..20 {
                delay_for(Duration::from_secs(2)).await
            }
        }
    }

    pub async fn new_arrived(
        _ctx: Context,
        peer_store: PeerStore,
        r_stream: ReadStream,
        mut ev_tx: Sender<Event>,
    ) -> Result<(), Error> {
        let encoded_peers = r_stream
            .lock()
            .await
            .try_next()
            .await?
            .ok_or(BootstrapError::NoNewArrive)?;

        let decoded_peers: Peers =
            prost::Message::decode(encoded_peers).context("decode new arrived peers")?;
        let store_peer_infos = decoded_peers
            .data
            .into_iter()
            .map(|b_pi| b_pi.into_store_info())
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
                    peer_store.set_multiaddr(&peer_id, multiaddr).await
                }
            }
        }

        ev_tx
            .try_send(Event::NewArrived)
            .context("impossible, try send new arrived event")?;

        Ok(())
    }
}

#[async_trait]
impl ProtocolHandler for BootstrapProtocol {
    fn proto_id(&self) -> ProtocolId {
        1.into()
    }

    fn proto_name(&self) -> &'static str {
        "bootstrap"
    }

    async fn handle(&self, stream: FramedStream) {
        let (w_stream, r_stream) = BiLock::new(stream);

        if !self.is_server.load(Ordering::SeqCst) {
            let peer_id = self.peer_id.clone();
            let local_multiaddr = self.local_multiaddr.clone();

            tokio::spawn(async move {
                if let Err(err) =
                    Self::publish_ourself(Context::new(), peer_id, local_multiaddr, w_stream).await
                {
                    error!("publish ourself: {}", err);
                }
            });
        } else {
            let peer_store = self.peer_store.clone();

            tokio::spawn(async move {
                if let Err(err) =
                    Self::interval_push_peers(Context::new(), peer_store, w_stream).await
                {
                    error!("push peers: {}", err);
                }
            });
        }

        if let Err(err) = Self::new_arrived(
            Context::new(),
            self.peer_store.clone(),
            r_stream,
            self.event_tx.clone(),
        )
        .await
        {
            error!("{} new arrived: {}", self.peer_id, err);
        }
    }
}

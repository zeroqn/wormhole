use super::{Connectedness, Dialer, QuicConn, Conn, Direction};
use crate::{crypto::PeerId, peer_store::PeerStore, transport::{Transport, QuicTransport}};

use log::error;
use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use futures::lock::Mutex;

use std::{collections::HashSet, borrow::Borrow, hash::{Hasher, Hash}, sync::Arc};

#[derive(thiserror::Error, Debug)]
pub enum DialerError {
    #[error("no address found for peer {0}")]
    NoAddress(PeerId),
}

pub struct PeerConn {
    peer_id: PeerId,
    conn: QuicConn,
}

impl Borrow<PeerId> for PeerConn {
    fn borrow(&self) -> &PeerId {
        &self.peer_id
    }
}

impl PartialEq for PeerConn {
    fn eq(&self, other: &PeerConn) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for PeerConn {}

impl Hash for PeerConn {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.peer_id.hash(hasher)
    }
}

#[derive(Clone)]
pub struct QuicDialer {
    peer_store: PeerStore,
    conn_pool: Arc<Mutex<HashSet<PeerConn>>>,
    transport: QuicTransport,
}

impl QuicDialer {
    pub fn new(transport: QuicTransport) -> Self {
        QuicDialer {
            peer_store: Default::default(),
            conn_pool: Default::default(),
            transport,
        }
    }

    pub(crate) async fn close(&self) -> Result<(), Error> {
        let peer_conns = {
            let mut pool = self.conn_pool.lock().await;
            pool.drain().collect::<Vec<_>>()
        };

        for peer_conn in peer_conns.into_iter() {
            let store = self.peer_store.clone();

            tokio::spawn(Self::close_peer_conn(peer_conn, store));
        }

        Ok(())
    }

  // TODO: graceful error handle?
    async fn close_peer_conn(peer_conn: PeerConn, peer_store: PeerStore) {
        let PeerConn {peer_id, conn} = peer_conn;

        if let Err(err) = conn.close().await {
            error!("close {} connection: {}", peer_id, err);
        }

        peer_store.set_connectedness(&peer_id, Connectedness::CanConnect).await;
    }
}

#[async_trait]
impl Dialer for QuicDialer {
    type Conn = QuicConn;
    type PeerStore = PeerStore;

    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Self::Conn, Error> {
        if let Some(addrs) = self.peer_store.get_multiaddrs(peer_id).await {
            // TODO: simply use first address right now
            if let Some(addr) = addrs.into_iter().next() {
                let conn = self.transport.dial(ctx, addr, peer_id.to_owned()).await?;

                self.peer_store.set_connectedness(peer_id, Connectedness::Connected).await;

                return Ok(QuicConn::new(conn, Direction::Outbound));
            }
        }

        Err(DialerError::NoAddress(peer_id.to_owned()).into())
    }

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error> {
        let opt_conn = {
            self.conn_pool.lock().await.take(peer_id)
        };

        if let Some(peer_conn) = opt_conn {
            Self::close_peer_conn(peer_conn, self.peer_store.clone()).await;
        }

        Ok(())
    }

    fn peer_store(&self) -> Self::PeerStore {
        self.peer_store.clone()
    }

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.peer_store.get_connectedness(peer_id).await
    }

    async fn peers(&self) -> Vec<PeerId> {
        self.conn_pool.lock().await.iter().map(|pc| pc.peer_id.clone()).collect()
    }

    async fn conns(&self) -> Vec<Self::Conn> {
        self.conn_pool.lock().await.iter().map(|pc| pc.conn.clone()).collect()
    }

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Self::Conn> {
        self.conn_pool.lock().await.get(peer_id).map(|pc| pc.conn.clone())
    }
}

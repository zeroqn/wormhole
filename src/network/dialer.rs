use super::{Connectedness, Dialer, QuicConn, Conn, Direction};
use crate::{crypto::PeerId, peer_store::PeerStore, transport::{Transport, QuicTransport}};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;

use std::{collections::HashSet, borrow::Borrow, hash::{Hasher, Hash}};

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

pub struct QuicDialer {
    peer_store: PeerStore,
    conn_pool: HashSet<PeerConn>,
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

                return Ok(QuicConn::new(conn, Direction::Outbound));
            }
        }

        Err(DialerError::NoAddress(peer_id.to_owned()).into())
    }

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error> {
        if let Some(conn) = self.conn_to_peer(peer_id) {
            return conn.close().await;
        }

        Ok(())
    }

    fn peer_store(&self) -> Self::PeerStore {
        self.peer_store.clone()
    }

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.peer_store.get_connectedness(peer_id).await
    }

    fn peers(&self) -> Vec<PeerId> {
        self.conn_pool.iter().map(|pc| pc.peer_id.clone()).collect()
    }

    fn conns(&self) -> Vec<Self::Conn> {
        self.conn_pool.iter().map(|pc| pc.conn.clone()).collect()
    }

    fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Self::Conn> {
        self.conn_pool.get(peer_id).map(|pc| pc.conn.clone())
    }
}

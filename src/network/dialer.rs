use super::{Conn, Connectedness, Dialer, Direction, QuicConn, QuicConnPool};
use crate::{
    crypto::PeerId,
    peer_store::PeerStore,
    transport::{QuicTransport, Transport},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use tracing::{error, debug};

#[derive(thiserror::Error, Debug)]
pub enum DialerError {
    #[error("no address found for peer {0}")]
    NoAddress(PeerId),
}

#[derive(Clone)]
pub struct QuicDialer {
    peer_store: PeerStore,
    conn_pool: QuicConnPool,
    transport: QuicTransport,
}

impl QuicDialer {
    pub(crate) fn new(
        peer_store: PeerStore,
        conn_pool: QuicConnPool,
        transport: QuicTransport,
    ) -> Self {
        QuicDialer {
            peer_store,
            conn_pool,
            transport,
        }
    }

    pub(crate) async fn close(&self) -> Result<(), Error> {
        let peer_conns = self.conn_pool.drain().await;

        for peer_conn in peer_conns.into_iter() {
            let store = self.peer_store.clone();

            tokio::spawn(async move {
                let peer_id = peer_conn.0.clone();

                if let Err(err) = Self::close_peer_conn(peer_conn, store).await {
                    error!("close {} connection: {}", peer_id, err);
                }
            });
        }

        Ok(())
    }

    async fn close_peer_conn(
        peer_conn: (PeerId, QuicConn),
        peer_store: PeerStore,
    ) -> Result<(), Error> {
        let (peer_id, conn) = peer_conn;

        peer_store
            .set_connectedness(&peer_id, Connectedness::CanConnect)
            .await;

        if conn.is_closed() {
            Ok(())
        } else {
            conn.close().await
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
                debug!("dial peer {}", addr);

                let conn = self.transport.dial(ctx, addr, peer_id.to_owned()).await?;

                self.peer_store
                    .set_connectedness(peer_id, Connectedness::Connected)
                    .await;

                debug!("connected to peer {}", peer_id);

                return Ok(QuicConn::new(conn, Direction::Outbound));
            }
        }

        Err(DialerError::NoAddress(peer_id.to_owned()).into())
    }

    async fn close_peer(&self, peer_id: &PeerId) -> Result<(), Error> {
        if let Some(conn) = self.conn_pool.take(peer_id).await {
            debug!("close peer {}", peer_id);

            Self::close_peer_conn((peer_id.to_owned(), conn), self.peer_store.clone()).await?;

            debug!("disconnected to peer {}", peer_id);
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
        self.conn_pool.peers().await
    }

    async fn conns(&self) -> Vec<Self::Conn> {
        self.conn_pool.conns().await
    }

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Self::Conn> {
        self.conn_pool.conn_to_peer(peer_id).await
    }
}

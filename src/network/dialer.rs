use super::{Conn, Connectedness, Dialer, Direction, NetworkConn, NetworkConnPool};
use crate::{crypto::PeerId, peer_store::PeerStore, transport::Transport};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use tracing::{debug, error};

#[derive(thiserror::Error, Debug)]
pub enum DialerError {
    #[error("no address found for peer {0}")]
    NoAddress(PeerId),
}

#[derive(Clone)]
pub struct NetworkDialer {
    peer_store: Box<dyn PeerStore>,
    conn_pool: NetworkConnPool,
    transport: Box<dyn Transport>,
}

impl NetworkDialer {
    pub(crate) fn new(
        peer_store: impl PeerStore + 'static,
        conn_pool: NetworkConnPool,
        transport: impl Transport + 'static,
    ) -> Self {
        let transport: Box<dyn Transport> = Box::new(transport);
        let peer_store: Box<dyn PeerStore> = Box::new(peer_store);

        NetworkDialer {
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
        peer_conn: (PeerId, Box<dyn Conn>),
        peer_store: impl PeerStore,
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
impl Dialer for NetworkDialer {
    async fn dial_peer(&self, ctx: Context, peer_id: &PeerId) -> Result<Box<dyn Conn>, Error> {
        if let Some(conn) = self.conn_pool.conn_to_peer(peer_id).await {
            debug!("reuse exist peer {} connection", peer_id);
            return Ok(conn);
        }

        if let Some(addrs) = self.peer_store.get_multiaddrs(peer_id).await {
            // TODO: simply use first address right now
            if let Some(addr) = addrs.into_iter().next() {
                debug!("dial peer {}", addr);

                let conn = self.transport.dial(ctx, addr, peer_id.to_owned()).await?;
                let conn = NetworkConn::new(conn, Direction::Outbound);
                debug!("connected to peer {}", peer_id);

                self.peer_store
                    .set_connectedness(peer_id, Connectedness::Connected)
                    .await;
                self.conn_pool
                    .insert(peer_id.to_owned(), conn.clone())
                    .await;

                return Ok(Box::new(conn) as Box<dyn Conn>);
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

    fn peer_store(&self) -> Box<dyn PeerStore> {
        self.peer_store.clone()
    }

    async fn connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.peer_store.get_connectedness(peer_id).await
    }

    async fn peers(&self) -> Vec<PeerId> {
        self.conn_pool.peers().await
    }

    async fn conns(&self) -> Vec<Box<dyn Conn>> {
        self.conn_pool.conns().await
    }

    async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>> {
        self.conn_pool.conn_to_peer(peer_id).await
    }
}

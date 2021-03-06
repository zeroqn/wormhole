use super::Conn;
use crate::crypto::PeerId;

use futures::lock::Mutex;

use std::{
    borrow::Borrow,
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};

struct PeerConn {
    peer_id: PeerId,
    conn: Box<dyn Conn>,
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
pub(crate) struct NetworkConnPool(Arc<Mutex<HashSet<PeerConn>>>);

impl Default for NetworkConnPool {
    fn default() -> Self {
        NetworkConnPool(Default::default())
    }
}

impl NetworkConnPool {
    pub async fn peers(&self) -> Vec<PeerId> {
        self.0
            .lock()
            .await
            .iter()
            .map(|pc| pc.peer_id.clone())
            .collect()
    }

    pub async fn conns(&self) -> Vec<Box<dyn Conn>> {
        self.0
            .lock()
            .await
            .iter()
            .map(|pc| pc.conn.clone())
            .collect()
    }

    pub async fn conn_to_peer(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>> {
        self.0.lock().await.get(peer_id).map(|pc| pc.conn.clone())
    }

    pub async fn insert(&self, peer_id: PeerId, conn: impl Conn + 'static) {
        let conn: Box<dyn Conn> = Box::new(conn);

        self.0.lock().await.insert(PeerConn { peer_id, conn });
    }

    pub async fn take(&self, peer_id: &PeerId) -> Option<Box<dyn Conn>> {
        self.0.lock().await.take(peer_id).map(|pc| pc.conn)
    }

    pub async fn drain(&self) -> Vec<(PeerId, Box<dyn Conn>)> {
        self.0
            .lock()
            .await
            .drain()
            .map(|pc| (pc.peer_id, pc.conn))
            .collect()
    }
}

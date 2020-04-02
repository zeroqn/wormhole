use super::PeerStore;
use crate::{
    bootstrap::BootstrapPeerStore,
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network::Connectedness,
};

use anyhow::Error;
use async_trait::async_trait;
use futures::lock::Mutex;

use std::{
    borrow::Borrow,
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(thiserror::Error, Debug)]
pub enum SimplePeerStoreError {
    #[error("{peer_id} is not derive from {pubkey}")]
    PeerIdNotMatchPubKey { peer_id: PeerId, pubkey: PublicKey },
}

#[derive(Clone)]
pub struct PeerInfo {
    peer_id: PeerId,
    pubkey: Option<PublicKey>,
    connectedness: Connectedness,
    multiaddrs: HashSet<Multiaddr>,
}

impl PeerInfo {
    pub fn new(peer_id: PeerId) -> Self {
        PeerInfo {
            peer_id,
            pubkey: None,
            connectedness: Connectedness::NotConnected,
            multiaddrs: HashSet::new(),
        }
    }

    pub fn with_all(pubkey: PublicKey, connectedness: Connectedness, addr: Multiaddr) -> Self {
        let mut addr_set = HashSet::new();
        addr_set.insert(addr);

        PeerInfo {
            peer_id: pubkey.peer_id(),
            pubkey: Some(pubkey),
            connectedness,
            multiaddrs: addr_set,
        }
    }

    pub fn with_addr(peer_id: PeerId, addr: Multiaddr) -> Self {
        let mut addr_set = HashSet::new();
        addr_set.insert(addr);

        PeerInfo {
            peer_id,
            pubkey: None,
            connectedness: Connectedness::NotConnected,
            multiaddrs: addr_set,
        }
    }

    pub fn with_pubkey(peer_id: PeerId, pubkey: PublicKey) -> Self {
        PeerInfo {
            peer_id,
            pubkey: Some(pubkey),
            connectedness: Connectedness::NotConnected,
            multiaddrs: HashSet::new(),
        }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn pubkey(&self) -> Option<&PublicKey> {
        self.pubkey.as_ref()
    }

    pub fn into_multiaddr(self) -> Option<Multiaddr> {
        self.multiaddrs.into_iter().next()
    }

    pub fn set_connectedness(&mut self, connectedness: Connectedness) {
        self.connectedness = connectedness;
    }

    pub fn set_pubkey(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        use SimplePeerStoreError::*;

        if pubkey.peer_id() != self.peer_id {
            Err(PeerIdNotMatchPubKey {
                peer_id: self.peer_id.clone(),
                pubkey,
            }
            .into())
        } else {
            self.pubkey = Some(pubkey);

            Ok(())
        }
    }
}

impl Borrow<PeerId> for PeerInfo {
    fn borrow(&self) -> &PeerId {
        &self.peer_id
    }
}

impl PartialEq for PeerInfo {
    fn eq(&self, other: &PeerInfo) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for PeerInfo {}

impl Hash for PeerInfo {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.peer_id.hash(hasher)
    }
}

#[derive(Clone)]
pub struct SimplePeerStore {
    book: Arc<Mutex<HashSet<PeerInfo>>>,
}

impl Default for SimplePeerStore {
    fn default() -> Self {
        SimplePeerStore {
            book: Default::default(),
        }
    }
}

impl SimplePeerStore {
    pub async fn contains(&self, peer_id: &PeerId) -> bool {
        self.book.lock().await.contains(peer_id)
    }

    pub async fn register(&self, peer_info: PeerInfo) {
        self.book.lock().await.insert(peer_info);
    }

    pub async fn choose(&self, size: usize) -> Vec<(PeerId, Multiaddr)> {
        let book = self.book.lock().await;

        book.iter()
            .filter(|pi| !pi.multiaddrs.is_empty())
            .take(size)
            .map(|pi| {
                (
                    pi.peer_id.clone(),
                    pi.multiaddrs.iter().next().expect("impossible").clone(),
                )
            })
            .collect()
    }
}

#[async_trait]
impl PeerStore for SimplePeerStore {
    async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.pubkey.clone())
            .flatten()
    }

    async fn set_pubkey(&self, peer_id: &PeerId, pubkey: PublicKey) {
        if peer_id != &pubkey.peer_id() {
            return;
        }

        let mut book = self.book.lock().await;
        if !book.contains(peer_id) {
            book.insert(PeerInfo::with_pubkey(peer_id.to_owned(), pubkey));
        }
    }

    async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.connectedness)
            .unwrap_or_else(|| Connectedness::NotConnected)
    }

    async fn set_connectedness(&self, peer_id: &PeerId, connectedness: Connectedness) {
        let mut book = self.book.lock().await;

        if let Some(mut peer_info) = book.take(peer_id) {
            peer_info.connectedness = connectedness;
            book.insert(peer_info);
        } else {
            book.insert(PeerInfo::new(peer_id.to_owned()));
        }
    }

    async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.multiaddrs.iter().cloned().collect())
    }

    // TODO: check peer id in multiaddr match given peer id.
    // But we need to take care of relay address.
    async fn add_multiaddr(&self, peer_id: &PeerId, addr: Multiaddr) {
        let mut book = self.book.lock().await;

        if let Some(mut peer_info) = book.take(peer_id) {
            peer_info.multiaddrs.insert(addr);
            book.insert(peer_info);
        } else {
            book.insert(PeerInfo::with_addr(peer_id.to_owned(), addr));
        }
    }
}

#[async_trait]
impl BootstrapPeerStore for SimplePeerStore {
    async fn register(&self, peers: Vec<(PeerId, Multiaddr)>) {
        for (peer_id, multiaddr) in peers.into_iter() {
            let info = PeerInfo::with_addr(peer_id, multiaddr);
            self.register(info).await
        }
    }

    async fn contains(&self, peer_id: &PeerId) -> bool {
        self.contains(peer_id).await
    }

    async fn choose(&self, max: usize) -> Vec<(PeerId, Multiaddr)> {
        self.choose(max).await
    }
}

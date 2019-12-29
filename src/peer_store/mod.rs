use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network::Connectedness,
};

use anyhow::Error;
use futures::lock::Mutex;

use std::{
    borrow::Borrow,
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(thiserror::Error, Debug)]
pub enum PeerStoreError {
    #[error("{peer_id} is not derive from {pubkey}")]
    PeerIdNotMatchPubKey { peer_id: PeerId, pubkey: PublicKey },
}

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

    pub fn into_multiaddr(self) -> Option<Multiaddr> {
        self.multiaddrs.into_iter().next()
    }

    pub fn set_connectedness(&mut self, connectedness: Connectedness) {
        self.connectedness = connectedness;
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
pub struct PeerStore {
    book: Arc<Mutex<HashSet<PeerInfo>>>,
}

impl Default for PeerStore {
    fn default() -> Self {
        PeerStore {
            book: Default::default(),
        }
    }
}

impl PeerStore {
    pub async fn contains(&self, peer_id: &PeerId) -> bool {
        self.book.lock().await.contains(peer_id)
    }

    pub async fn register(&self, peer_info: PeerInfo) {
        self.book.lock().await.insert(peer_info);
    }
    
    pub async fn choose(&self, size: usize) -> Vec<(PeerId, Multiaddr)> {
        let book = self.book.lock().await;

        book.iter().filter(|pi| !pi.multiaddrs.is_empty()).take(size).map(|pi| (pi.peer_id.clone(), pi.multiaddrs.iter().next().expect("impossible").clone())).collect()
    }

    pub async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.pubkey.clone())
            .flatten()
    }

    pub async fn set_pubkey(&self, peer_id: &PeerId, pubkey: PublicKey) -> Result<(), Error> {
        if peer_id != &pubkey.peer_id() {
            return Err(PeerStoreError::PeerIdNotMatchPubKey {
                peer_id: peer_id.to_owned(),
                pubkey,
            }
            .into());
        }

        let mut book = self.book.lock().await;

        if let Some(mut peer_info) = book.take(peer_id) {
            peer_info.pubkey = Some(pubkey);
            book.insert(peer_info);
        } else {
            book.insert(PeerInfo::with_pubkey(peer_id.to_owned(), pubkey));
        }

        Ok(())
    }

    pub async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.connectedness)
            .unwrap_or_else(|| Connectedness::NotConnected)
    }

    pub async fn set_connectedness(&self, peer_id: &PeerId, connectedness: Connectedness) {
        let mut book = self.book.lock().await;

        if let Some(mut peer_info) = book.take(peer_id) {
            peer_info.connectedness = connectedness;
            book.insert(peer_info);
        } else {
            book.insert(PeerInfo::new(peer_id.to_owned()));
        }
    }

    pub async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<HashSet<Multiaddr>> {
        self.book
            .lock()
            .await
            .get(peer_id)
            .map(|pi| pi.multiaddrs.clone())
    }

    // TODO: check peer id in multiaddr match given peer id.
    // But we need to take care of relay address.
    pub async fn set_multiaddr(&self, peer_id: &PeerId, addr: Multiaddr) {
        let mut book = self.book.lock().await;

        if let Some(mut peer_info) = book.take(peer_id) {
            peer_info.multiaddrs.insert(addr);
            book.insert(peer_info);
        } else {
            book.insert(PeerInfo::with_addr(peer_id.to_owned(), addr));
        }
    }
}

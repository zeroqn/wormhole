use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network::Connectedness,
};

use async_trait::async_trait;
use dyn_clone::DynClone;

use std::ops::Deref;

#[async_trait]
pub trait PeerStore: Sync + Send + DynClone {
    async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey>;

    async fn set_pubkey(&self, peer_id: &PeerId, pubkey: PublicKey);

    async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness;

    async fn set_connectedness(&self, peer_id: &PeerId, connectedness: Connectedness);

    async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>>;

    async fn add_multiaddr(&self, peer_id: &PeerId, addr: Multiaddr);
}

dyn_clone::clone_trait_object!(PeerStore);

#[async_trait]
impl<S> PeerStore for S
where
    S: Deref<Target = dyn PeerStore> + Sync + Send + DynClone,
{
    async fn get_pubkey(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.deref().get_pubkey(peer_id).await
    }

    async fn set_pubkey(&self, peer_id: &PeerId, pubkey: PublicKey) {
        self.deref().set_pubkey(peer_id, pubkey).await
    }

    async fn get_connectedness(&self, peer_id: &PeerId) -> Connectedness {
        self.deref().get_connectedness(peer_id).await
    }

    async fn set_connectedness(&self, peer_id: &PeerId, connectedness: Connectedness) {
        self.deref().set_connectedness(peer_id, connectedness).await
    }

    async fn get_multiaddrs(&self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        self.deref().get_multiaddrs(peer_id).await
    }

    async fn add_multiaddr(&self, peer_id: &PeerId, addr: Multiaddr) {
        self.deref().add_multiaddr(peer_id, addr).await
    }
}

pub mod multiaddr;
pub mod transport;

use bytes::Bytes;
use parity_multihash::Multihash;

pub struct PeerId(Multihash);

// TODO: replace with ophelia trait
#[derive(Clone)]
pub struct PublicKey(Bytes);

impl PublicKey {
    fn peer_id(&self) -> PeerId {
        unimplemented!()
    }
}

pub mod transport;

use bytes::Bytes;
use parity_multihash::Multihash;

pub struct PeerId(Multihash);
// TODO: replace with ophelia trait
pub struct PublicKey(Bytes);

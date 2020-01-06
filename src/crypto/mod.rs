use anyhow::Error;
use bytes::Bytes;
use ophelia_hasher::HashValue;
use ophelia_secp256k1::{Secp256k1PrivateKey, Secp256k1PublicKey, Secp256k1Signature};
use parity_multihash::{self as multihash, Multihash};

use std::{convert::TryFrom, fmt};

pub struct Signature(Secp256k1Signature);

impl Signature {
    pub fn from_slice(data: &[u8]) -> Result<Self, Error> {
        // Note: compact formate
        Ok(Signature(Secp256k1Signature::try_from(data)?))
    }

    pub fn to_bytes(&self) -> Bytes {
        use ophelia::Signature;

        self.0.to_bytes()
    }
}

pub struct PrivateKey(Secp256k1PrivateKey);

impl PrivateKey {
    pub fn from_slice(data: &[u8]) -> Result<Self, Error> {
        Ok(PrivateKey(Secp256k1PrivateKey::try_from(data)?))
    }

    pub fn sign(&self, msg: &HashValue) -> Result<Signature, Error> {
        use ophelia::PrivateKey;

        Ok(Signature(self.0.sign_message(msg)))
    }

    pub fn pubkey(&self) -> PublicKey {
        use ophelia::ToPublicKey;

        PublicKey(self.0.pub_key())
    }
}

#[derive(Clone)]
pub struct PublicKey(Secp256k1PublicKey);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.string())
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.string())
    }
}

impl PublicKey {
    pub fn from_slice(data: &[u8]) -> Result<Self, Error> {
        Ok(PublicKey(Secp256k1PublicKey::try_from(data)?))
    }

    pub fn to_bytes(&self) -> Bytes {
        use ophelia::PublicKey;

        self.0.to_bytes()
    }

    pub fn string(&self) -> String {
        bs58::encode(self.to_bytes().as_ref()).into_string()
    }

    pub fn verify(&self, msg: &HashValue, sig: &Signature) -> Result<(), Error> {
        use ophelia::SignatureVerify;

        Ok(sig.0.verify(msg, &self.0)?)
    }

    pub fn peer_id(&self) -> PeerId {
        let hash = keccak256_hash(&self.to_bytes().as_ref());
        let mhash = multihash::encode(multihash::Hash::Keccak256, hash.as_ref())
            .expect("impossible, keccak256 length cannot exceed u32::MAX");

        PeerId(mhash)
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct PeerId(Multihash);

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.string())
    }
}

impl PeerId {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        let mhash = Multihash::from_bytes(bytes)?;

        Ok(PeerId(mhash))
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn string(&self) -> String {
        bs58::encode(self.as_slice()).into_string()
    }

    pub fn into_inner(self) -> Multihash {
        self.0
    }
}

pub fn keccak256_hash(obj: &[u8]) -> HashValue {
    use ophelia_hasher::Hasher;

    ophelia_hasher_keccak256::Keccak256.digest(obj)
}

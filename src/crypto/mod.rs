use anyhow::Error;
use lazy_static::lazy_static;
use parity_multihash::{self as multihash, Multihash};
use secp256k1::Secp256k1;

use std::fmt;

lazy_static! {
    static ref SECP256K1: Secp256k1<secp256k1::All> = Secp256k1::new();
}

#[derive(thiserror::Error, Debug)]
pub enum CryptoError {
    #[error("invalid private key {0}")]
    InvalidPrivateKey(Error),

    #[error("invalid public key {0}")]
    InvalidPublicKey(Error),

    #[error("invalid signature {0}")]
    InvalidSignature(Error),

    #[error("unexpect error {0}")]
    UnexpectedError(Error),
}

impl From<secp256k1::Error> for CryptoError {
    fn from(err: secp256k1::Error) -> Self {
        use secp256k1::Error::*;

        match err {
            InvalidSecretKey => Self::InvalidPrivateKey(err.into()),
            InvalidPublicKey => Self::InvalidPublicKey(err.into()),
            IncorrectSignature | InvalidSignature => Self::InvalidSignature(err.into()),
            _ => Self::UnexpectedError(err.into()),
        }
    }
}

pub struct Signature([u8; 64]);

impl Signature {
    pub fn from_slice(data: &[u8]) -> Result<Self, CryptoError> {
        let sig = secp256k1::Signature::from_compact(data)?;

        Ok(Signature(sig.serialize_compact()))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

pub struct PrivateKey(secp256k1::SecretKey);

impl PrivateKey {
    pub fn from_slice(data: &[u8]) -> Result<Self, CryptoError> {
        Ok(PrivateKey(secp256k1::SecretKey::from_slice(data)?))
    }

    pub fn sign(&self, msg: &[u8]) -> Result<Signature, CryptoError> {
        let msg = secp256k1::Message::from_slice(msg)?;
        let sig = SECP256K1.sign(&msg, &self.0);

        Ok(Signature(sig.serialize_compact()))
    }

    pub fn pubkey(&self) -> PublicKey {
        PublicKey(secp256k1::PublicKey::from_secret_key(&SECP256K1, &self.0).serialize())
    }
}

// TODO: replace with ophelia trait
#[derive(Clone)]
pub struct PublicKey([u8; 33]);

impl PublicKey {
    pub fn from_slice(data: &[u8]) -> Result<Self, CryptoError> {
        Ok(PublicKey(
            secp256k1::PublicKey::from_slice(data)?.serialize(),
        ))
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Result<(), CryptoError> {
        let msg = secp256k1::Message::from_slice(msg)?;
        let sig = secp256k1::Signature::from_compact(sig.as_slice())?;
        let pubkey = secp256k1::PublicKey::from_slice(&self.0)?;

        Ok(SECP256K1.verify(&msg, &sig, &pubkey)?)
    }

    pub fn peer_id(&self) -> PeerId {
        let hash = keccak256_hash(&self.0);
        let mhash = multihash::encode(multihash::Hash::Keccak256, &hash)
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
        bs58::encode(self.as_slice())
            .with_alphabet(bs58::alphabet::FLICKR)
            .into_string()
    }

    pub fn into_inner(self) -> Multihash {
        self.0
    }
}

pub fn keccak256_hash(obj: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;

    let mut hasher = tiny_keccak::Keccak::v256();
    let mut output = [0u8; 32];

    hasher.update(obj);
    hasher.finalize(&mut output);

    output
}

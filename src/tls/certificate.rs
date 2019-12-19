use crate::crypto::{self, CryptoError};

use anyhow::Error;
use derive_more::Display;
use prost::Message;
use rcgen::{CertificateParams, CustomExtension};

const CERT_P2P_EXT_PK_PREFIX: &str = "libp2p-tls-handshake";
const CERT_P2P_EXT_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];

#[derive(thiserror::Error, Debug)]
pub enum DerCertificateError {
    #[error("fail to parse: {0}")]
    ParseError(String),
    #[error("signed key decode error: {0}")]
    SignedKeyDecodeError(#[from] prost::DecodeError),
    #[error("host public key not found")]
    NoHostPublicKey,
    #[error("unsupport host public key type {0}")]
    UnsupportedHostPublicKeyType(i32),
    #[error("invalid host public key {0}")]
    InvalidHostPublicKey(Error),
    #[error("no proof found in certificate")]
    NoProof,
    #[error("invalid proof {0}")]
    InvalidProof(Error),
    #[error("unexpected internal error {0}")]
    UnexpectedError(Error),
}

impl From<CryptoError> for DerCertificateError {
    fn from(err: CryptoError) -> Self {
        use CryptoError::*;

        match err {
            InvalidPublicKey(err) => Self::InvalidHostPublicKey(err.into()),
            InvalidSignature(err) => Self::InvalidProof(err.into()),
            _ => Self::UnexpectedError(err.into()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
enum InternalError {
    #[error("certificate public key take {real}, larger than {max}")]
    CertificatePublicKeyTooLarge { max: usize, real: usize },
}

impl From<InternalError> for DerCertificateError {
    fn from(err: InternalError) -> DerCertificateError {
        DerCertificateError::UnexpectedError(err.into())
    }
}

#[derive(Debug, Display, Clone, PartialEq, Eq, prost::Enumeration)]
enum KeyType {
    #[display(fmt = "rsa")]
    RSA = 0,
    #[display(fmt = "ed25519")]
    ED25519 = 1,
    #[display(fmt = "secp256k1")]
    Secp256k1 = 2,
    #[display(fmt = "ecdsa")]
    ECDSA = 3,
}

#[derive(Clone, PartialEq, Eq, Message)]
struct PublicKey {
    #[prost(enumeration = "KeyType", tag = "1")]
    pub key_type: i32,
    #[prost(bytes, tag = "2")]
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct SignedKey {
    #[prost(message, tag = "1")]
    pub public_key: Option<PublicKey>,
    #[prost(bytes, tag = "2")]
    pub signature: Vec<u8>,
}

impl SignedKey {
    pub fn new(host_pk: &[u8], cert_proof: &[u8]) -> Self {
        let pk = PublicKey {
            key_type: KeyType::Secp256k1 as i32,
            data: Vec::from(host_pk),
        };

        SignedKey {
            public_key: Some(pk),
            signature: Vec::from(cert_proof),
        }
    }
}

struct P2POid;

impl P2POid {
    pub fn matched(oid: std::slice::Iter<'_, u64>) -> bool {
        for (ln, rn) in oid.zip(CERT_P2P_EXT_OID) {
            if ln != rn {
                return false;
            }
        }

        true
    }
}

pub struct P2PSelfSignedCertificate {
    der_cert: rcgen::Certificate,
}

impl P2PSelfSignedCertificate {
    pub fn from_host(
        host_privkey: &crypto::PrivateKey,
        host_pubkey: &crypto::PublicKey,
    ) -> Result<Self, Error> {
        // Generate random certificate keypair
        let cert_keypair = rcgen::KeyPair::generate(&rcgen::PKCS_ECDSA_P384_SHA384)?;
        let cert_pubkey = cert_keypair.public_key_raw();

        // Now we need to produce this proof extension
        let cert_proof = Self::gen_proof(cert_pubkey, host_privkey)?;
        let signed_key = SignedKey::new(host_pubkey.as_slice(), cert_proof.as_slice());

        let mut encoded_key = Vec::with_capacity(signed_key.encoded_len());
        signed_key.encode(&mut encoded_key)?;

        let p2p_ext = CustomExtension::from_oid_content(CERT_P2P_EXT_OID, encoded_key);

        // Now we're ready to produce our self-signed certificate
        let peer_id = host_pubkey.peer_id();
        let mut cert_params = CertificateParams::default();
        // Note: use peer id as dns name isn't defined in spec
        cert_params.subject_alt_names = vec![rcgen::SanType::DnsName(peer_id.string())];
        cert_params.custom_extensions = vec![p2p_ext];
        cert_params.is_ca = rcgen::IsCa::SelfSignedOnly;
        cert_params.alg = &rcgen::PKCS_ECDSA_P384_SHA384;
        cert_params.key_pair = Some(cert_keypair);

        let cert = rcgen::Certificate::from_params(cert_params)?;

        Ok(P2PSelfSignedCertificate { der_cert: cert })
    }

    pub fn serialize_der(&self) -> Result<Vec<u8>, Error> {
        Ok(self.der_cert.serialize_der()?)
    }

    // FIXME: zero
    pub fn serialize_private_key_der(&self) -> Vec<u8> {
        self.der_cert.serialize_private_key_der()
    }

    pub fn verify_cert_ext(data: &[u8]) -> Result<crypto::PublicKey, DerCertificateError> {
        use DerCertificateError::*;

        let (_, cert) =
            x509_parser::parse_x509_der(data).map_err(|err| ParseError(format!("{:?}", err)))?;

        let cert = cert.tbs_certificate;
        let exts = cert.extensions;

        let cert_pubkey = cert.subject_pki.subject_public_key.data;

        for ext in exts.iter() {
            if P2POid::matched(ext.oid.iter()) {
                let signed_key = SignedKey::decode(ext.value)?;
                let pubkey_in_ext = signed_key.public_key.ok_or(NoHostPublicKey)?;

                if pubkey_in_ext.key_type != KeyType::Secp256k1 as i32 {
                    return Err(UnsupportedHostPublicKeyType(pubkey_in_ext.key_type));
                }

                let host_pubkey = crypto::PublicKey::from_slice(&pubkey_in_ext.data)?;
                let sig = crypto::Signature::from_slice(signed_key.signature.as_slice())?;

                Self::verify_proof(&sig, &cert_pubkey, &host_pubkey)?;
                return Ok(host_pubkey);
            }
        }

        Err(NoProof)
    }

    fn salt_cert_pubkey(cert_pk: &[u8], buf: &mut [u8]) -> Result<usize, InternalError> {
        let prefix = CERT_P2P_EXT_PK_PREFIX;
        let spk_len = prefix.len() + cert_pk.len();

        if spk_len > buf.len() {
            return Err(InternalError::CertificatePublicKeyTooLarge {
                max: buf.len(),
                real: spk_len,
            })?;
        }

        buf[..prefix.len()].copy_from_slice(prefix.as_bytes());
        buf[prefix.len()..spk_len].copy_from_slice(cert_pk);

        Ok(spk_len)
    }

    fn gen_proof(
        cert_pubkey: &[u8],
        host_privkey: &crypto::PrivateKey,
    ) -> Result<crypto::Signature, Error> {
        let mut buf = [0u8; 1000];
        let len = Self::salt_cert_pubkey(cert_pubkey, &mut buf)?;
        let salted_pk = &buf[..len];

        let msg = crypto::keccak256_hash(salted_pk);
        Ok(host_privkey.sign(&msg)?)
    }

    fn verify_proof(
        proof: &crypto::Signature,
        cert_pubkey: &[u8],
        host_pubkey: &crypto::PublicKey,
    ) -> Result<(), DerCertificateError> {
        let mut buf = [0u8; 1000];
        let len = Self::salt_cert_pubkey(cert_pubkey, &mut buf)?;
        let salted_pk = &buf[..len];

        let msg = crypto::keccak256_hash(salted_pk);
        Ok(host_pubkey.verify(&msg, proof)?)
    }
}

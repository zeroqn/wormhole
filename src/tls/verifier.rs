use super::certificate::{DerCertificateError, P2PSelfSignedCertificate};

use anyhow::Error;
use rustls::{
    ClientCertVerified, ClientCertVerifier, DistinguishedNames, ServerCertVerified,
    ServerCertVerifier, TLSError,
};

#[derive(thiserror::Error, Debug)]
pub enum PeerCertificateVerifierError {
    #[error("wrong number of certs in chain, expect 1, got {0}")]
    MoreThanOneCertificate(usize),
}

impl From<PeerCertificateVerifierError> for TLSError {
    fn from(err: PeerCertificateVerifierError) -> TLSError {
        TLSError::General(err.to_string())
    }
}

impl From<DerCertificateError> for TLSError {
    fn from(err: DerCertificateError) -> TLSError {
        TLSError::General(err.to_string())
    }
}

// TODO: Normal TLS certificate verification processdure
struct PeerCertVerifier;

impl PeerCertVerifier {
    pub fn verify(certs: &[rustls::Certificate]) -> Result<(), TLSError> {
        use PeerCertificateVerifierError::*;

        if certs.len() != 1 {
            return Err(MoreThanOneCertificate(certs.len()))?;
        }

        let root_cert = &certs[0].as_ref();
        P2PSelfSignedCertificate::verify_cert_ext(root_cert)?;

        Ok(())
    }
}

pub struct ClientAuth;

impl ClientCertVerifier for ClientAuth {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn client_auth_root_subjects(&self) -> DistinguishedNames {
        DistinguishedNames::new()
    }

    fn verify_client_cert(
        &self,
        certs: &[rustls::Certificate],
    ) -> Result<ClientCertVerified, TLSError> {
        PeerCertVerifier::verify(certs)?;

        Ok(ClientCertVerified::assertion())
    }
}

pub struct ServerAuth;

impl ServerCertVerifier for ServerAuth {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        certs: &[rustls::Certificate],
        _hostname: webpki::DNSNameRef<'_>,
        _ocsp: &[u8],
    ) -> Result<ServerCertVerified, TLSError> {
        PeerCertVerifier::verify(certs)?;

        Ok(ServerCertVerified::assertion())
    }
}

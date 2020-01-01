use crate::crypto;
use crate::tls::{
    certificate::P2PSelfSignedCertificate,
    verifier::{ClientAuth, ServerAuth},
};

use anyhow::Error;
use quinn::{
    ClientConfig, ClientConfigBuilder, ServerConfig, ServerConfigBuilder, TransportConfig,
};

use std::{convert::TryFrom, sync::Arc};

#[derive(Clone)]
pub struct QuicConfig {
    host_pubkey: crypto::PublicKey,

    cert: rustls::Certificate,
    cert_privkey: rustls::PrivateKey,
}

impl QuicConfig {
    pub fn make(
        host_pubkey: crypto::PublicKey,
        host_privkey: &crypto::PrivateKey,
    ) -> Result<Self, Error> {
        let cert = P2PSelfSignedCertificate::from_host(host_privkey, &host_pubkey)?;
        let cert_privkey = rustls::PrivateKey(cert.serialize_private_key_der());

        let config = QuicConfig {
            host_pubkey: host_pubkey,
            cert: rustls::Certificate::try_from(cert)?,
            cert_privkey,
        };

        Ok(config)
    }

    pub fn make_server_config(&self) -> Result<ServerConfig, Error> {
        let mut tls_cfg = rustls::ServerConfig::new(Arc::new(ClientAuth));
        // Force tls 1.3, no fallback
        tls_cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
        tls_cfg.max_early_data_size = u32::max_value();
        tls_cfg.set_single_cert(vec![self.cert.clone()], self.cert_privkey.clone())?;

        let server_config = quinn::ServerConfig {
            transport: Arc::new(TransportConfig {
                // No uni stream
                stream_window_uni: 0,
                ..Default::default()
            }),
            crypto: Arc::new(tls_cfg),
            ..Default::default()
        };

        Ok(ServerConfigBuilder::new(server_config).build())
    }

    pub fn make_client_config(&self) -> Result<ClientConfig, Error> {
        let mut tls_cfg = rustls::ClientConfig::new();
        // Force tls 1.3, no fallback
        tls_cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
        tls_cfg.set_single_client_cert(vec![self.cert.clone()], self.cert_privkey.clone());
        // Through rustls "dangerous_configuration" feature gate
        tls_cfg
            .dangerous()
            .set_certificate_verifier(Arc::new(ServerAuth));

        let client_config = ClientConfig {
            transport: Arc::new(TransportConfig {
                // No uni stream
                stream_window_uni: 0,
                ..Default::default()
            }),
            crypto: Arc::new(tls_cfg),
            ..Default::default()
        };

        Ok(ClientConfigBuilder::new(client_config).build())
    }
}

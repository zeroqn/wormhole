use super::{QuicConn, QuicListener, Transport, QuicConfig};
use crate::{multiaddr::{Multiaddr, MultiaddrExt}, crypto::{PeerId, PublicKey, PrivateKey}};
use crate::tls::certificate::P2PSelfSignedCertificate;

use log::warn;
use quinn::{Endpoint, ServerConfig, ClientConfig, EndpointBuilder, NewConnection};
use quinn_proto::EndpointConfig;
use async_trait::async_trait;
use anyhow::Error;
use creep::Context;

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("transport isn't listen on any socket addr")]
    NoListen,

    #[error("transport doesn't support this multiaddr `{0}`")]
    UndialableMultiaddr(Multiaddr),

    #[error("wrong number of certs in chain, expect 1, got {0}")]
    MoreThanOneCertificate(usize),

    #[error("connection peer id mismatch: expect {target}, got {connected}")]
    PeerMismatch{
        target: PeerId,
        connected: PeerId,
    }
}

#[derive(Clone)]
pub struct QuicTransport {
    config: QuicConfig,
    server_config: ServerConfig,
    client_config: ClientConfig,

    endpoint: Option<Endpoint>,

    local_pubkey: PublicKey,
    local_multiaddr: Option<Multiaddr>,
}

impl QuicTransport {
    pub fn make(host_privkey: &PrivateKey, host_pubkey: PublicKey) -> Result<Self, Error> {
        let config = QuicConfig::make(host_pubkey.clone(), host_privkey)?;
        let server_config = config.make_server_config()?;
        let client_config = config.make_client_config()?;

        let transport = QuicTransport {
            config,
            server_config,
            client_config,

            endpoint: None,

            local_pubkey: host_pubkey,
            local_multiaddr: None,
        };

        Ok(transport)
    }
}

#[async_trait]
impl Transport for QuicTransport {
    type CapableConn = QuicConn;
    type Listener = QuicListener;

    async fn dial(
        &self,
        _ctx: Context,
        raddr: Multiaddr,
        peer_id: PeerId,
    ) -> Result<Self::CapableConn, Error> {
        use TransportError::*;

        if self.endpoint.is_none() {
            return Err(NoListen)?;
        }

        if !self.can_dial(&raddr) {
            return Err(UndialableMultiaddr(raddr))?;
        }

        let sock_addr = raddr.to_socket_addr();
        let endpoint = self.endpoint.as_ref().expect("impossible no listen");

        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = endpoint.connect(&sock_addr, "p2p")?.await?;

        let peer_certs = connection.peer_der_certificates().expect("impossible, pass cert verifier without valid certificate");

        if peer_certs.len() > 1 {
            return Err(MoreThanOneCertificate(peer_certs.len()))?;
        }

        let peer_pubkey = P2PSelfSignedCertificate::recover_peer_pubkey(peer_certs[0].as_slice())?;

        let connected_peer_id = peer_pubkey.peer_id();
        if connected_peer_id != peer_id {
            return Err(PeerMismatch {
                target: peer_id,
                connected: connected_peer_id,
            })?;
        }

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("connection driver err {}", err);
            }
        });

        Ok(QuicConn::new(connection, bi_streams, self.clone(), self.local_pubkey.clone(), peer_pubkey, raddr))
    }

    fn can_dial(&self, raddr: &Multiaddr) -> bool {
        raddr.contains_socket_addr() && raddr.is_quic()
    }

    async fn listen(&mut self, laddr: Multiaddr) -> Result<Self::Listener, Error> {
        let sock_addr = laddr.to_socket_addr();

        let mut builder = EndpointBuilder::new(EndpointConfig::default());
        builder.listen(self.server_config.clone());
        builder.default_client_config(self.client_config.clone());

        let (driver, endpoint, incoming) = builder.bind(&sock_addr)?;

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("endpoint driver err {}", err);
            }
        });

        self.endpoint = Some(endpoint);
        self.local_multiaddr = Some(laddr.clone());

        Ok(QuicListener::new(incoming, self.local_pubkey.clone(), self.clone()))
    }

    fn local_multiaddr(&self) -> Option<Multiaddr> {
        self.local_multiaddr.clone()
    }
}

use super::{QuicConfig, QuicConn, QuicListener, QuinnConnectionExt, Transport};
use crate::{
    crypto::{PeerId, PrivateKey, PublicKey},
    multiaddr::{Multiaddr, MultiaddrExt},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use tracing::{info, debug};
use quinn::{ClientConfig, Endpoint, NewConnection, ServerConfig};
use futures::lock::Mutex;

use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("transport isn't listen on any socket addr")]
    NoListen,

    #[error("transport doesn't support this multiaddr `{0}`")]
    UndialableMultiaddr(Multiaddr),

    #[error("connection peer id mismatch: expect {target}, got {connected}")]
    PeerMismatch { target: PeerId, connected: PeerId },
}

#[derive(Clone)]
pub struct QuicTransport {
    config: QuicConfig,
    server_config: ServerConfig,
    client_config: ClientConfig,

    endpoint: Arc<Mutex<Option<Endpoint>>>,

    local_pubkey: PublicKey,
    local_multiaddr: Option<Multiaddr>,
}

impl QuicTransport {
    pub fn make(host_privkey: &PrivateKey) -> Result<Self, Error> {
        let host_pubkey = host_privkey.pubkey();
        let config = QuicConfig::make(host_pubkey.clone(), host_privkey)?;
        let server_config = config.make_server_config()?;
        let client_config = config.make_client_config()?;

        let transport = QuicTransport {
            config,
            server_config,
            client_config,

            endpoint: Arc::new(Mutex::new(None)),

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

        {
            if self.endpoint.lock().await.is_none() {
                return Err(NoListen)?;
            }
        }

        if !self.can_dial(&raddr) {
            return Err(UndialableMultiaddr(raddr))?;
        }

        let sock_addr = raddr.to_socket_addr();
        let endpoint = {
            self.endpoint.lock().await.clone().expect("impossible no listen")
        };

        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = endpoint.connect(&sock_addr, "p2p")?.await?;

        let peer_pubkey = connection.peer_pubkey()?;

        let connected_peer_id = peer_pubkey.peer_id();
        if connected_peer_id != peer_id {
            return Err(PeerMismatch {
                target: peer_id,
                connected: connected_peer_id,
            })?;
        }

        debug!("create new connection to {}", raddr);

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                info!("dial connection driver: {}", err);
            }
        });

        Ok(QuicConn::new(
            connection,
            bi_streams,
            self.clone(),
            self.local_pubkey.clone(),
            peer_pubkey,
            raddr,
        ))
    }

    fn can_dial(&self, raddr: &Multiaddr) -> bool {
        raddr.contains_socket_addr() && raddr.is_quic()
    }

    async fn listen(&mut self, laddr: Multiaddr) -> Result<Self::Listener, Error> {
        let sock_addr = laddr.to_socket_addr();

        let mut builder = Endpoint::builder();
        builder.default_client_config(self.client_config.clone());
        builder.listen(self.server_config.clone());

        let (driver, endpoint, incoming) = builder.bind(&sock_addr)?;

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                info!("endpoint driver: {}", err);
            }
        });

        {
            self.endpoint.lock().await.replace(endpoint);
        }

        self.local_multiaddr = Some(laddr.clone());

        debug!("listen on {}", laddr);

        Ok(QuicListener::new(
            incoming,
            self.local_pubkey.clone(),
            self.clone(),
        ))
    }

    fn local_multiaddr(&self) -> Option<Multiaddr> {
        self.local_multiaddr.clone()
    }
}

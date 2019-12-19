use super::{QuicConn, QuicListener, Transport};
use crate::{multiaddr::{Multiaddr, MultiaddrExt}, crypto::{PeerId, PublicKey}};

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
}

#[derive(Clone)]
pub struct QuicTransport {
    config: EndpointConfig,
    server_config: ServerConfig,
    client_config: ClientConfig,

    endpoint: Option<Endpoint>,

    local_pubkey: PublicKey,
    local_multiaddr: Multiaddr,
}

#[async_trait]
impl Transport for QuicTransport {
    type CapableConn = QuicConn;
    type Listener = QuicListener;
    
    async fn dial(
        &self,
        ctx: Context,
        raddr: Multiaddr,
        peer_id: PeerId,
    ) -> Result<Self::CapableConn, Error> {
        if self.endpoint.is_none() {
            return Err(TransportError::NoListen)?;
        }

        if !self.can_dial(&raddr) {
            return Err(TransportError::UndialableMultiaddr(raddr))?;
        }

        let sock_addr = raddr.to_socket_addr();
        let endpoint = self.endpoint.expect("impossible no listen");

        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = endpoint.connect(&sock_addr, "p2p")?.await?;

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("connection driver err {}", err);
            }
        });

        Ok(QuicConn::new(connection, bi_streams, self.clone(), self.local_pubkey))
    }

    fn can_dial(&self, raddr: &Multiaddr) -> bool {
        raddr.contains_socket_addr() && raddr.is_quic()
    }

    async fn listen(&self, laddr: Multiaddr) -> Result<Self::Listener, Error> {
        let sock_addr = laddr.to_socket_addr();

        let mut builder = EndpointBuilder::new(self.config.clone());
        builder.listen(self.server_config.clone());
        builder.default_client_config(self.client_config.clone());
        
        let (driver, endpoint, incoming) = builder.bind(&sock_addr)?;

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("endpoint driver err {}", err);
            }
        });

        self.endpoint = Some(endpoint);
        self.local_multiaddr = laddr;

        Ok(QuicListener::new(incoming, laddr, self.local_pubkey.clone(), self.clone()))
    }
    
    fn local_multiaddr(&self) -> Multiaddr {
        self.local_multiaddr.clone()
    }
}

use super::{Listener, QuicConn, QuicTransport, QuinnConnectionExt, Transport};
use crate::{
    crypto::PublicKey,
    multiaddr::{Multiaddr, MultiaddrExt},
};

use anyhow::Error;
use async_trait::async_trait;
use futures::stream::StreamExt;
use log::warn;
use quinn::{Incoming, NewConnection};

use std::net::SocketAddr;

#[derive(thiserror::Error, Debug)]
pub enum ListenerError {
    #[error("listner was closed or lost driver")]
    ClosedOrDriverLost,
}

pub struct QuicListener {
    incoming: Option<Incoming>,
    pubkey: PublicKey,
    transport: QuicTransport,
}

impl QuicListener {
    pub fn new(incoming: Incoming, local_pubkey: PublicKey, transport: QuicTransport) -> Self {
        QuicListener {
            incoming: Some(incoming),
            pubkey: local_pubkey,
            transport,
        }
    }
}

#[async_trait]
impl Listener for QuicListener {
    type CapableConn = QuicConn;

    async fn accept(&mut self) -> Result<Self::CapableConn, Error> {
        if self.incoming.is_none() {
            return Err(ListenerError::ClosedOrDriverLost)?;
        }

        let incoming = self.incoming.as_mut().expect("impossible no incoming");

        let connecting = incoming
            .next()
            .await
            .ok_or(ListenerError::ClosedOrDriverLost)?;

        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = connecting.await?;

        let remote_pubkey = connection.peer_pubkey()?;
        let remote_peer_id = remote_pubkey.peer_id();
        let remote_multiaddr = Multiaddr::quic_peer(connection.remote_address(), remote_peer_id);

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("connection driver err {}", err);
            }
        });

        Ok(QuicConn::new(
            connection,
            bi_streams,
            self.transport.clone(),
            self.pubkey.clone(),
            remote_pubkey,
            remote_multiaddr,
        ))
    }

    fn close(&mut self) -> Result<(), Error> {
        drop(self.incoming.take());

        Ok(())
    }

    fn addr(&self) -> SocketAddr {
        self.transport
            .local_multiaddr()
            .expect("impossible, no multiaddr after listen")
            .to_socket_addr()
    }

    fn multiaddr(&self) -> Multiaddr {
        self.transport
            .local_multiaddr()
            .expect("impossible, no multiaddr after listen")
    }
}

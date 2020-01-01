use super::{CapableConn, Listener, QuicConn, QuicTransport, QuinnConnectionExt, Transport};
use crate::{
    crypto::PublicKey,
    multiaddr::{Multiaddr, MultiaddrExt},
};

use anyhow::Error;
use async_trait::async_trait;
use futures::stream::StreamExt;
use quinn::{Incoming, NewConnection};
use tracing::{debug, info};

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

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
    async fn accept(&mut self) -> Result<Box<dyn CapableConn>, Error> {
        if self.incoming.is_none() {
            return Err(ListenerError::ClosedOrDriverLost)?;
        }

        let incoming = self.incoming.as_mut().expect("impossible no incoming");

        let connecting = incoming
            .next()
            .await
            .ok_or(ListenerError::ClosedOrDriverLost)?;

        debug!(
            "got incoming connection attampt from {}",
            connecting.remote_address()
        );

        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = connecting.await?;

        let remote_pubkey = connection.peer_pubkey()?;
        let remote_peer_id = remote_pubkey.peer_id();
        let remote_multiaddr = Multiaddr::quic_peer(connection.remote_address(), remote_peer_id);

        debug!("accept connection from {}", remote_multiaddr);

        let is_closed = Arc::new(AtomicBool::new(false));
        let is_closed_by_driver = Arc::clone(&is_closed);

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                info!("accepted connection driver: {}", err);
            }

            is_closed_by_driver.store(true, Ordering::SeqCst);
        });

        Ok(QuicConn::new(
            connection,
            bi_streams,
            is_closed,
            self.transport.clone(),
            self.pubkey.clone(),
            remote_pubkey,
            remote_multiaddr,
        )
        .into())
    }

    fn close(&mut self) -> Result<(), Error> {
        debug!(
            "close transprt listener {:?}",
            self.transport.local_multiaddr()
        );
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

use super::{Listener, QuicConn, QuicTransport, Transport};
use crate::{multiaddr::{Multiaddr, MultiaddrExt}, PublicKey};

use log::warn;
use futures::stream::StreamExt;
use anyhow::Error;
use async_trait::async_trait;
use quinn::{Incoming, NewConnection};

use std::net::SocketAddr;

#[derive(thiserror::Error, Debug)]
pub enum ListenerError {
    #[error("listner was closed or lost driver")]
    ClosedOrDriverLost,
}

pub struct QuicListener {
    incoming: Option<Incoming>,
    multiaddr: Multiaddr,
    pubkey: PublicKey,
    transport: QuicTransport,
}

impl QuicListener {
    pub fn new(incoming: Incoming, local_multiaddr: Multiaddr, local_pubkey: PublicKey, transport: QuicTransport) -> Self {
        QuicListener {
            incoming: Some(incoming),
            multiaddr: local_multiaddr,
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

        let connecting = incoming.next().await.ok_or(ListenerError::ClosedOrDriverLost)?;
        let NewConnection {
            driver,
            connection,
            bi_streams,
            ..
        } = connecting.await?;

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                warn!("connection driver err {}", err);
            }
        });

        Ok(QuicConn::new(connection, bi_streams, self.transport, self.pubkey))
    }

    async fn close(&mut self) -> Result<(), Error> {
        drop(self.incoming.take());

        Ok(())
    }

    fn addr(&self) -> SocketAddr {
        self.transport.local_multiaddr().to_socket_addr()
    }

    fn multiaddr(&self) -> Multiaddr {
        self.transport.local_multiaddr()
    }
}

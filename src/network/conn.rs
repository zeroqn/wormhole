use super::{Direction, NetworkStream, Protocol};
use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network::{Conn, Stream},
    transport::{self, CapableConn, ConnSecurity},
};

use anyhow::Error;
use async_trait::async_trait;
use futures::lock::Mutex;
use tracing::{debug, error};

use std::sync::Arc;

#[derive(Clone)]
pub struct NetworkConn {
    inner: Box<dyn CapableConn>,
    direction: Direction,
    streams: Arc<Mutex<Vec<NetworkStream>>>,
}

impl NetworkConn {
    pub fn new(conn: impl CapableConn + 'static, direction: Direction) -> Self {
        let inner: Box<dyn CapableConn> = Box::new(conn);

        NetworkConn {
            inner,
            direction,
            streams: Default::default(),
        }
    }

    pub(crate) async fn accept(&self) -> Result<NetworkStream, Error> {
        let muxed_stream = self.inner.accept_stream().await?;
        let new_stream = NetworkStream::new(muxed_stream, Direction::Inbound, self.clone());
        debug!("accepted new stream from peer {}", self.remote_peer());

        {
            self.streams.lock().await.push(new_stream.clone());
        }

        Ok(new_stream)
    }
}

impl transport::ConnSecurity for NetworkConn {
    fn local_peer(&self) -> PeerId {
        self.inner.local_peer()
    }

    fn remote_peer(&self) -> PeerId {
        self.inner.remote_peer()
    }

    fn remote_public_key(&self) -> PublicKey {
        self.inner.remote_public_key()
    }
}

impl transport::ConnMultiaddr for NetworkConn {
    fn local_multiaddr(&self) -> Multiaddr {
        self.inner.local_multiaddr()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.inner.remote_multiaddr()
    }
}

#[async_trait]
impl Conn for NetworkConn {
    async fn new_stream(&self, proto: Protocol) -> Result<Box<dyn Stream>, Error> {
        let muxed_stream = self.inner.open_stream().await?;
        let mut new_stream = NetworkStream::new(muxed_stream, self.direction, self.clone());
        new_stream.set_protocol(proto);

        debug!(
            "create new stream to peer {} using proto {}",
            self.remote_peer(),
            proto
        );

        {
            self.streams.lock().await.push(new_stream.clone());
        }

        Ok(Box::new(new_stream))
    }

    async fn streams(&self) -> Vec<Box<dyn Stream>> {
        self.streams
            .lock()
            .await
            .iter()
            .map(|s| Box::new(s.clone()) as Box<dyn Stream>)
            .collect()
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    async fn close(&self) -> Result<(), Error> {
        debug!("close connection to peer {}", self.remote_peer());

        let streams = { self.streams.lock().await.drain(..).collect::<Vec<_>>() };

        for mut stream in streams.into_iter() {
            let peer_id = self.inner.remote_peer();

            tokio::spawn(async move {
                if let Err(err) = stream.close().await {
                    error!(
                        "close {} protocol {:?} stream: {}",
                        peer_id.clone(),
                        stream.protocol(),
                        err
                    );
                }
            });
        }

        self.inner.close().await
    }
}

use super::{Direction, ProtocolId, QuicStream};
use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network,
    transport::{self, CapableConn},
};

use anyhow::Error;
use async_trait::async_trait;
use futures::lock::Mutex;
use tracing::error;

use std::sync::Arc;

#[derive(Clone)]
pub struct QuicConn {
    inner: transport::QuicConn,
    direction: Direction,
    streams: Arc<Mutex<Vec<QuicStream>>>,
}

impl QuicConn {
    pub fn new(conn: transport::QuicConn, direction: Direction) -> Self {
        QuicConn {
            inner: conn,
            direction,
            streams: Default::default(),
        }
    }

    pub(crate) async fn accept(&self) -> Result<QuicStream, Error> {
        let muxed_stream = self.inner.accept_stream().await?;
        let new_stream = QuicStream::new(muxed_stream, Direction::Inbound, self.clone());

        {
            self.streams.lock().await.push(new_stream.clone());
        }

        Ok(new_stream)
    }
}

impl transport::ConnSecurity for QuicConn {
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

impl transport::ConnMultiaddr for QuicConn {
    fn local_multiaddr(&self) -> Multiaddr {
        self.inner.local_multiaddr()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.inner.remote_multiaddr()
    }
}

#[async_trait]
impl network::Conn for QuicConn {
    type Stream = QuicStream;

    async fn new_stream(&self, proto_id: ProtocolId) -> Result<Self::Stream, Error> {
        use network::Stream;

        let muxed_stream = self.inner.open_stream().await?;
        let mut new_stream = QuicStream::new(muxed_stream, self.direction, self.clone());
        new_stream.set_protocol(proto_id);

        {
            self.streams.lock().await.push(new_stream.clone());
        }

        Ok(new_stream)
    }

    async fn streams(&self) -> Vec<Self::Stream> {
        self.streams.lock().await.clone()
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    async fn close(&self) -> Result<(), Error> {
        use network::Stream;
        use transport::ConnSecurity;

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

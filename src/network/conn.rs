use super::{Direction, QuicStream, ProtocolId};
use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    network, transport::{CapableConn, self},
};

use anyhow::Error;
use async_trait::async_trait;
use futures::lock::Mutex;

use std::sync::Arc;

#[derive(Clone)]
pub struct QuicConn {
    inner: transport::QuicConn,
    direction: Direction,
    streams: Arc<Mutex<Vec<QuicStream>>>,
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

    async fn new_stream(&self, _proto_id: ProtocolId) -> Result<Self::Stream, Error> {
        let muxed_stream = self.inner.open_stream().await?;

        // TODO: negotiate protocol through these stream
        let stream = QuicStream::new(muxed_stream, self.direction, self.clone());
        Ok(stream)
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
        self.inner.close().await
    }
}

use super::{CapableConn, QuicMuxedStream, RESET_ERR_CODE, Transport};
use crate::{PeerId, PublicKey, multiaddr::{MultiaddrExt, Multiaddr}};

use anyhow::Error;
use async_trait::async_trait;
use futures::{lock::Mutex, stream::StreamExt};
use quinn::{Connection, ConnectionError, IncomingBiStreams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[derive(Clone)]
pub struct QuicConn<Transport> {
    conn: Connection,
    bi_streams: Arc<Mutex<IncomingBiStreams>>,
    is_closed: Arc<AtomicBool>,
    transport: Transport,
    
    local_pubkey: PublicKey,
    remote_pubkey: PublicKey,
}

#[async_trait]
impl<T: Transport> CapableConn for QuicConn<T> {
    type MuxedStream = QuicMuxedStream;
    type Transport = T;

    async fn open_stream(&self) -> Result<Self::MuxedStream, Error> {
        let (send, read) = self.conn.open_bi().await?;

        Ok(QuicMuxedStream::new(read, send))
    }

    async fn accept_stream(&self) -> Result<Self::MuxedStream, Error> {
        let bi_streams = &mut self.bi_streams.lock().await;

        let opt_stream = bi_streams.next().await;
        if opt_stream.is_none() {
            self.is_closed.store(true, Ordering::SeqCst);
        }

        let (send, read) = opt_stream.ok_or(ConnectionError::LocallyClosed)??;

        Ok(QuicMuxedStream::new(read, send))
    }

    fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }

    async fn close(&self) -> Result<(), Error> {
        self.is_closed.store(true, Ordering::SeqCst);
        self.conn.close(RESET_ERR_CODE.into(), b"close");
        
        Ok(())
    }

    fn local_peer(&self) -> PeerId {
        self.local_pubkey.peer_id()
    }

    fn remote_peer(&self) -> PeerId {
        self.remote_pubkey.peer_id()
    }

    fn remote_public_key(&self) -> PublicKey {
        self.remote_pubkey.clone()
    }

    fn local_multiaddr(&self) -> Multiaddr {
        self.transport.local_multiaddr()
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        let sock_addr = self.conn.remote_address();
        let mut maddr = Multiaddr::quic_from_sock_addr(sock_addr);

        maddr.push_peer_id(self.remote_peer());
        maddr
    }

    fn transport(&self) -> Self::Transport {
        self.transport.clone()
    }
}

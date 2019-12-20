use super::{CapableConn, QuicMuxedStream, QuicTransport, Transport, RESET_ERR_CODE};
use crate::{
    crypto::{PeerId, PublicKey},
    multiaddr::Multiaddr,
    tls::certificate::P2PSelfSignedCertificate,
};

use anyhow::Error;
use async_trait::async_trait;
use futures::{lock::Mutex, stream::StreamExt};
use quinn::{Connection, ConnectionError, IncomingBiStreams};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(thiserror::Error, Debug)]
enum ConnectionInternalError {
    #[error("wrong number of certs in chain, expect 1, got {0}")]
    MoreThanOneCertificate(usize),
}

pub trait QuinnConnectionExt {
    fn peer_pubkey(&self) -> Result<PublicKey, Error>;
}

impl QuinnConnectionExt for Connection {
    fn peer_pubkey(&self) -> Result<PublicKey, Error> {
        use ConnectionInternalError::*;

        let peer_certs = self
            .peer_der_certificates()
            .expect("impossible, pass cert verifier without valid certificate");

        if peer_certs.len() > 1 {
            return Err(MoreThanOneCertificate(peer_certs.len()))?;
        }

        Ok(P2PSelfSignedCertificate::recover_peer_pubkey(
            peer_certs[0].as_slice(),
        )?)
    }
}

#[derive(Clone)]
pub struct QuicConn {
    conn: Connection,
    bi_streams: Arc<Mutex<IncomingBiStreams>>,
    is_closed: Arc<AtomicBool>,
    transport: QuicTransport,

    local_pubkey: PublicKey,
    remote_pubkey: PublicKey,
    remote_multiaddr: Multiaddr,
}

impl QuicConn {
    pub fn new(
        conn: Connection,
        bi_streams: IncomingBiStreams,
        transport: QuicTransport,
        local_pubkey: PublicKey,
        remote_pubkey: PublicKey,
        remote_multiaddr: Multiaddr,
    ) -> Self {
        QuicConn {
            conn,
            bi_streams: Arc::new(Mutex::new(bi_streams)),
            is_closed: Arc::new(AtomicBool::new(false)),
            transport,

            local_pubkey,
            remote_pubkey,
            remote_multiaddr,
        }
    }
}

#[async_trait]
impl CapableConn for QuicConn {
    type MuxedStream = QuicMuxedStream;
    type Transport = QuicTransport;

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
        self.transport
            .local_multiaddr()
            .expect("impossible, got connection without listen")
    }

    fn remote_multiaddr(&self) -> Multiaddr {
        self.remote_multiaddr.clone()
    }

    fn transport(&self) -> Self::Transport {
        self.transport.clone()
    }
}

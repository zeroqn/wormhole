use super::{
    Conn, Dialer, Network, ProtocolId, QuicDialer, QuicStream, RemoteConnHandler,
    RemoteStreamHandler,
};
use crate::{
    crypto::{PeerId, PrivateKey},
    multiaddr::Multiaddr,
    transport::{Listener, QuicListener, QuicTransport, Transport},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use futures::lock::Mutex;

use std::{net::SocketAddr, sync::Arc};

#[derive(thiserror::Error, Debug)]
pub enum NetworkError {
    #[error("already listen on {0}")]
    AlreadyListen(SocketAddr),
}

#[derive(Clone)]
pub struct QuicNetwork<ConnHandler, StreamHandler> {
    dialer: QuicDialer,
    transport: QuicTransport,
    listener: Arc<Mutex<Option<QuicListener>>>,
    conn_handler: Arc<ConnHandler>,
    stream_handler: Arc<StreamHandler>,
}

impl<CH, SH> QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler + 'static,
    SH: RemoteStreamHandler + 'static,
{
    pub fn make(
        host_privkey: &PrivateKey,
        conn_handler: CH,
        stream_handler: SH,
    ) -> Result<Self, Error> {
        let transport = QuicTransport::make(host_privkey)?;
        let dialer = QuicDialer::new(transport.clone());

        Ok(QuicNetwork {
            dialer,
            transport,
            listener: Arc::new(Mutex::new(None)),
            conn_handler: Arc::new(conn_handler),
            stream_handler: Arc::new(stream_handler),
        })
    }
}

#[async_trait]
impl<CH, SH> Network for QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler + 'static,
    SH: RemoteStreamHandler + 'static,
{
    type Stream = QuicStream;

    async fn close(&self) -> Result<(), Error> {
        self.dialer.close().await?;

        {
            // Note: no rwlock please :)
            if let Some(listener) = &mut *self.listener.lock().await {
                listener.close().await?;
            }
        }

        Ok(())
    }

    async fn new_stream(
        &self,
        ctx: Context,
        peer_id: &PeerId,
        proto_id: ProtocolId,
    ) -> Result<Self::Stream, Error> {
        let conn = match self.dialer.conn_to_peer(peer_id).await {
            Some(conn) => conn,
            None => self.dialer.dial_peer(ctx, peer_id).await?,
        };

        conn.new_stream(proto_id).await
    }

    async fn listen(&mut self, laddr: Multiaddr) -> Result<(), Error> {
        let self_listener = &mut *self.listener.lock().await;
        if let Some(listener) = self_listener {
            return Err(NetworkError::AlreadyListen(listener.addr()).into());
        }

        let new_listener = self.transport.listen(laddr).await?;
        *self_listener = Some(new_listener);

        Ok(())
    }
}

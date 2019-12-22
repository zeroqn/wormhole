use super::{
    Conn, Connectedness, Dialer, Direction, Network, ProtocolId, QuicConn, QuicConnPool,
    QuicDialer, QuicStream, RemoteConnHandler, RemoteStreamHandler,
};
use crate::{
    crypto::{PeerId, PrivateKey},
    multiaddr::Multiaddr,
    peer_store::{PeerInfo, PeerStore},
    transport::{ConnMultiaddr, ConnSecurity},
    transport::{Listener, QuicListener, QuicTransport, Transport},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use futures::{channel::oneshot, future::Future, lock::Mutex, ready};
use tracing::error;

use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context as TaskContext, Poll},
};

#[derive(thiserror::Error, Debug)]
pub enum NetworkError {
    #[error("already listen on {0}")]
    AlreadyListen(Multiaddr),
    #[error("lost closed signal")]
    LostClosedSignal,
}

struct QuicListenerDriver<ConnHandler, StreamHandler> {
    listener: QuicListener,
    peer_store: PeerStore,
    conn_pool: QuicConnPool,

    conn_handler: ConnHandler,
    stream_handler: StreamHandler,

    closing: Arc<AtomicBool>,
    close_done: Option<oneshot::Sender<()>>,
}

impl<CH, SH> QuicListenerDriver<CH, SH>
where
    CH: RemoteConnHandler<Conn = QuicConn> + 'static + Unpin,
    SH: RemoteStreamHandler<Stream = QuicStream> + 'static + Unpin,
{
    fn new(network: QuicNetwork<CH, SH>, listener: QuicListener) -> (Self, QuicListenerDriverRef) {
        let closing = Arc::new(AtomicBool::new(false));
        let (close_tx, close_rx) = oneshot::channel();

        let driver_ref = QuicListenerDriverRef {
            closing: Arc::clone(&closing),
            close_done: Arc::new(Mutex::new(close_rx)),
            local_multiaddr: listener.multiaddr(),
        };

        let driver = QuicListenerDriver {
            listener,
            peer_store: network.peer_store.clone(),
            conn_pool: network.conn_pool.clone(),
            conn_handler: network.conn_handler.clone(),
            stream_handler: network.stream_handler.clone(),
            closing,
            close_done: Some(close_tx),
        };

        (driver, driver_ref)
    }

    async fn drive_conn(
        conn: QuicConn,
        peer_store: PeerStore,
        conn_pool: QuicConnPool,
        conn_handler: CH,
        stream_handler: SH,
    ) {
        let peer_id = conn.remote_peer();
        let peer_maddr = conn.remote_multiaddr();

        if !peer_store.contains(&peer_id).await {
            let pubkey = conn.remote_public_key();

            let peer_info = PeerInfo::with_all(pubkey, Connectedness::Connected, peer_maddr);

            peer_store.register(peer_info).await;
        } else {
            peer_store
                .set_connectedness(&peer_id, Connectedness::Connected)
                .await;
            peer_store.set_multiaddr(&peer_id, peer_maddr).await;
        }

        conn_pool.insert(peer_id.clone(), conn.clone()).await;

        conn_handler.handle(conn.clone()).await;

        loop {
            let stream = match conn.accept().await {
                Err(err) => {
                    error!("accept peer {} stream: {}", peer_id.clone(), err);

                    break;
                }
                Ok(stream) => stream,
            };

            let stream_handler = stream_handler.clone();
            tokio::spawn(async move {
                stream_handler.handle(stream).await;
            });
        }

        if let Some(conn) = conn_pool.take(&peer_id).await {
            if let Err(err) = conn.close().await {
                error!("close {} connection in driver: {}", peer_id, err);
            }
        }
    }
}

impl<CH, SH> Future for QuicListenerDriver<CH, SH>
where
    CH: RemoteConnHandler<Conn = QuicConn> + 'static + Unpin,
    SH: RemoteStreamHandler<Stream = QuicStream> + 'static + Unpin,
{
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext) -> Poll<Self::Output> {
        if self.closing.load(Ordering::SeqCst) {
            let driver = self.get_mut();
            // Simply drop should not cause error
            if let Err(_) = driver.listener.close() {
                unreachable!()
            }

            let close_done = driver.close_done.take().expect("impossible, repeat close");

            if let Err(_) = close_done.send(()) {
                return Poll::Ready(Err(NetworkError::LostClosedSignal.into()));
            } else {
                return Poll::Ready(Ok(()));
            }
        }

        let driver = self.get_mut();
        let new_conn = match ready!(Future::poll(Pin::new(&mut driver.listener.accept()), cx)) {
            Err(err) => return Poll::Ready(Err(err)),
            Ok(conn) => conn,
        };

        let conn = QuicConn::new(new_conn, Direction::Inbound);
        let peer_store = driver.peer_store.clone();
        let conn_pool = driver.conn_pool.clone();
        let conn_handler = driver.conn_handler.clone();
        let stream_handler = driver.stream_handler.clone();

        tokio::spawn(Self::drive_conn(
            conn,
            peer_store,
            conn_pool,
            conn_handler,
            stream_handler,
        ));

        Poll::Pending
    }
}

#[derive(Clone)]
pub struct QuicListenerDriverRef {
    closing: Arc<AtomicBool>,
    close_done: Arc<Mutex<oneshot::Receiver<()>>>,
    local_multiaddr: Multiaddr,
}

impl QuicListenerDriverRef {
    pub async fn close(&self) -> Result<(), Error> {
        self.closing.store(true, Ordering::SeqCst);

        Ok((&mut *self.close_done.lock().await).await?)
    }

    pub fn local_multiaddr(&self) -> Multiaddr {
        self.local_multiaddr.clone()
    }
}

#[derive(Clone)]
pub struct QuicNetwork<ConnHandler, StreamHandler> {
    dialer: QuicDialer,
    peer_store: PeerStore,
    conn_pool: QuicConnPool,
    transport: QuicTransport,
    listener_driver_ref: Arc<Mutex<Option<QuicListenerDriverRef>>>,
    conn_handler: ConnHandler,
    stream_handler: StreamHandler,
}

impl<CH, SH> QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler<Conn = QuicConn> + 'static + Unpin,
    SH: RemoteStreamHandler<Stream = QuicStream> + 'static + Unpin,
{
    pub fn make(
        host_privkey: &PrivateKey,
        peer_store: PeerStore,
        conn_handler: CH,
        stream_handler: SH,
    ) -> Result<Self, Error> {
        let transport = QuicTransport::make(host_privkey)?;
        let conn_pool = QuicConnPool::default();
        let dialer = QuicDialer::new(peer_store.clone(), conn_pool.clone(), transport.clone());

        Ok(QuicNetwork {
            dialer,
            peer_store,
            conn_pool,
            transport,
            listener_driver_ref: Default::default(),
            conn_handler: conn_handler,
            stream_handler: stream_handler,
        })
    }
}

#[async_trait]
impl<CH, SH> Network for QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler<Conn = QuicConn> + 'static + Unpin,
    SH: RemoteStreamHandler<Stream = QuicStream> + 'static + Unpin,
{
    type Stream = QuicStream;

    async fn close(&self) -> Result<(), Error> {
        self.dialer.close().await?;

        let opt_driver_ref = { self.listener_driver_ref.lock().await.take() };

        if let Some(driver_ref) = opt_driver_ref {
            driver_ref.close().await?
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
        let opt_driver_ref = { self.listener_driver_ref.lock().await.clone() };

        if let Some(driver_ref) = opt_driver_ref {
            return Err(NetworkError::AlreadyListen(driver_ref.local_multiaddr()).into());
        }

        let new_listener = self.transport.listen(laddr.clone()).await?;
        let (driver, driver_ref) = QuicListenerDriver::new(self.clone(), new_listener);

        tokio::spawn(async move {
            if let Err(err) = driver.await {
                error!("listener driver: {}", err);
            }
        });

        self.listener_driver_ref.lock().await.replace(driver_ref);

        Ok(())
    }
}

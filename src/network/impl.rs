use super::{
    Connectedness, Dialer, Direction, Network, NetworkConn, NetworkConnPool, NetworkDialer,
    Protocol, RemoteConnHandler, RemoteStreamHandler, Stream,
};
use crate::{
    crypto::{PeerId, PrivateKey},
    multiaddr::Multiaddr,
    peer_store::{PeerInfo, PeerStore},
    transport::{ConnMultiaddr, ConnSecurity},
    transport::{Listener, QuicTransport, Transport},
};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use futures::{channel::oneshot, lock::Mutex};
use tracing::{debug, error};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(thiserror::Error, Debug)]
pub enum NetworkError {
    #[error("already listen on {0}")]
    AlreadyListen(Multiaddr),
    #[error("lost closed signal")]
    LostClosedSignal,
}

struct QuicListenerDriver<ConnHandler, StreamHandler> {
    listener: Box<dyn Listener>,
    peer_store: PeerStore,
    conn_pool: NetworkConnPool,

    conn_handler: ConnHandler,
    stream_handler: StreamHandler,

    closing: Arc<AtomicBool>,
    close_done: Option<oneshot::Sender<()>>,
}

impl<CH, SH> QuicListenerDriver<CH, SH>
where
    CH: RemoteConnHandler + 'static + Unpin + Clone,
    SH: RemoteStreamHandler + 'static + Unpin + Clone,
{
    fn new(
        network: QuicNetwork<CH, SH>,
        listener: impl Listener + 'static,
    ) -> (Self, QuicListenerDriverRef) {
        let closing = Arc::new(AtomicBool::new(false));
        let (close_tx, close_rx) = oneshot::channel();

        let driver_ref = QuicListenerDriverRef {
            closing: Arc::clone(&closing),
            close_done: Arc::new(Mutex::new(close_rx)),
            local_multiaddr: listener.multiaddr(),
        };

        let listener: Box<dyn Listener> = Box::new(listener);
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

    async fn accept(&mut self) -> Result<(), Error> {
        loop {
            if self.closing.load(Ordering::SeqCst) {
                // Simply drop should not cause error
                if let Err(_) = self.listener.close() {
                    unreachable!()
                }

                let close_done = self.close_done.take().expect("impossible, repeat close");

                if let Err(_) = close_done.send(()) {
                    return Err(NetworkError::LostClosedSignal.into());
                } else {
                    return Ok(());
                }
            }

            let conn = NetworkConn::new(self.listener.accept().await?, Direction::Inbound);
            let peer_store = self.peer_store.clone();
            let conn_pool = self.conn_pool.clone();
            let conn_handler = dyn_clone::clone(&self.conn_handler);
            let stream_handler = dyn_clone::clone(&self.stream_handler);

            tokio::spawn(Self::drive_conn(
                conn,
                peer_store,
                conn_pool,
                conn_handler,
                stream_handler,
            ));
        }
    }

    async fn drive_conn(
        conn: NetworkConn,
        peer_store: PeerStore,
        conn_pool: NetworkConnPool,
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

        conn_handler.handle(dyn_clone::clone_box(&conn)).await;

        loop {
            let stream = match conn.accept().await {
                Err(err) => {
                    error!("accept peer {} stream: {}", peer_id.clone(), err);

                    break;
                }
                Ok(stream) => stream,
            };

            let stream_handler = dyn_clone::clone_box(&stream_handler);
            tokio::spawn(async move {
                stream_handler.handle(dyn_clone::clone_box(&stream)).await;
            });
        }

        if let Some(conn) = conn_pool.take(&peer_id).await {
            if let Err(err) = conn.close().await {
                error!("close {} connection in driver: {}", peer_id, err);
            }
        }
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
    dialer: NetworkDialer,
    peer_store: PeerStore,
    conn_pool: NetworkConnPool,
    transport: QuicTransport,
    listener_driver_ref: Arc<Mutex<Option<QuicListenerDriverRef>>>,
    conn_handler: ConnHandler,
    stream_handler: StreamHandler,
}

impl<CH, SH> QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler + 'static + Unpin,
    SH: RemoteStreamHandler + 'static + Unpin,
{
    pub fn make(
        host_privkey: &PrivateKey,
        peer_store: PeerStore,
        conn_handler: CH,
        stream_handler: SH,
    ) -> Result<Self, Error> {
        let transport = QuicTransport::make(host_privkey)?;
        let conn_pool = NetworkConnPool::default();
        let dialer = NetworkDialer::new(peer_store.clone(), conn_pool.clone(), transport.clone());

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

    pub async fn connect(&self, ctx: Context, peer_id: &PeerId) -> Result<(), Error> {
        self.dialer.dial_peer(ctx, peer_id).await?;

        Ok(())
    }
}

#[async_trait]
impl<CH, SH> Network for QuicNetwork<CH, SH>
where
    CH: RemoteConnHandler + 'static + Unpin + Clone,
    SH: RemoteStreamHandler + 'static + Unpin + Clone,
{
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
        proto: Protocol,
    ) -> Result<Box<dyn Stream>, Error> {
        debug!("new stream to {} using protocol {}", peer_id, proto);

        let conn = match self.dialer.conn_to_peer(peer_id).await {
            Some(conn) => conn,
            None => self.dialer.dial_peer(ctx, peer_id).await?,
        };

        conn.new_stream(proto).await
    }

    async fn listen(&mut self, laddr: Multiaddr) -> Result<(), Error> {
        let opt_driver_ref = { self.listener_driver_ref.lock().await.clone() };

        if let Some(driver_ref) = opt_driver_ref {
            return Err(NetworkError::AlreadyListen(driver_ref.local_multiaddr()).into());
        }

        let new_listener = self.transport.listen(laddr.clone()).await?;
        let (mut driver, driver_ref) =
            QuicListenerDriver::new(dyn_clone::clone(&self), new_listener);

        tokio::spawn(async move {
            if let Err(err) = driver.accept().await {
                error!("listener driver: {}", err);
            }
        });

        self.listener_driver_ref.lock().await.replace(driver_ref);

        Ok(())
    }
}

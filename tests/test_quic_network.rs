mod common;
use common::{random_keypair, CommonError};

use anyhow::Error;
use async_trait::async_trait;
use creep::Context;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::stream::StreamExt;
use wormhole::{
    crypto::PublicKey,
    multiaddr::{Multiaddr, MultiaddrExt},
    network::{
        Connectedness, Network, ProtocolId, QuicConn, QuicNetwork, QuicStream, RemoteConnHandler,
        RemoteStreamHandler,
    },
    peer_store::{PeerInfo, PeerStore},
};

use std::net::ToSocketAddrs;

#[derive(Clone)]
pub struct NoopConnHandler;

#[async_trait]
impl RemoteConnHandler for NoopConnHandler {
    type Conn = QuicConn;

    async fn handle(&self, _conn: Self::Conn) {}
}

#[derive(Clone)]
pub struct EchoStreamHandler(UnboundedSender<String>);

#[async_trait]
impl RemoteStreamHandler for EchoStreamHandler {
    type Stream = QuicStream;

    async fn handle(&self, mut stream: Self::Stream) {
        let mut msg = String::new();

        stream
            .read_to_string(&mut msg)
            .await
            .expect("read message error");

        self.0.unbounded_send(msg).expect("send message error");
    }
}

pub async fn make_xenovox<A: ToSocketAddrs>(
    addr: A,
    peer_store: PeerStore,
) -> Result<
    (
        QuicNetwork<NoopConnHandler, EchoStreamHandler>,
        PublicKey,
        UnboundedReceiver<String>,
    ),
    Error,
> {
    let (sk, pk) = random_keypair();
    let (msg_tx, msg_rx) = unbounded();

    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let maddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    let peer_info = PeerInfo::with_all(pk.clone(), Connectedness::CanConnect, maddr.clone());
    peer_store.register(peer_info).await;

    let mut xenovox =
        QuicNetwork::make(&sk, peer_store, NoopConnHandler, EchoStreamHandler(msg_tx))?;

    xenovox.listen(maddr).await?;
    Ok((xenovox, pk, msg_rx))
}

#[tokio::test]
async fn test_quic_network_with_remote_peer() -> Result<(), Error> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let peer_store = PeerStore::default();
    let proto_id = ProtocolId::new(1, "drama");

    let msg = "watch 20-12-2019";

    let (_geralt_xenovox, geralt_pubkey, mut msg_rx) =
        make_xenovox(("127.0.0.1", 2077), peer_store.clone()).await?;
    let (ciri_xenovox, ..) = make_xenovox(("127.0.0.1", 2020), peer_store.clone()).await?;

    let mut ciri_stream = ciri_xenovox
        .new_stream(Context::new(), &geralt_pubkey.peer_id(), proto_id)
        .await?;

    ciri_stream.write_all(msg.as_bytes()).await?;
    ciri_stream.close().await?;

    assert_eq!(&msg_rx.next().await.ok_or(CommonError::NoMessage)?, &msg);

    Ok::<(), Error>(())
}

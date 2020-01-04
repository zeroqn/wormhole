mod common;
use common::{random_keypair, CommonError};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt, TryStreamExt,
};
use tracing::{debug, error};

use wormhole::{
    bootstrap::{self, BootstrapProtocol},
    crypto::PublicKey,
    host::{FramedStream, Host, ProtocolHandler, QuicHost},
    multiaddr::{Multiaddr, MultiaddrExt},
    network::{Connectedness, Protocol, ProtocolId},
    peer_store::{PeerInfo, PeerStore},
};

use std::net::ToSocketAddrs;

const BOOTSTRAP_PROTO_ID: u64 = 1;
const ECHO_PROTO_ID: u64 = 2;

#[derive(Clone)]
pub struct EchoProtocol;

impl EchoProtocol {
    fn proto() -> Protocol {
        Protocol::new(ECHO_PROTO_ID, "echo")
    }

    async fn echo(stream: &mut FramedStream) -> Result<(), Error> {
        let msg = stream.try_next().await?.expect("impossible");
        stream.send(msg).await?;

        Ok(())
    }
}

#[async_trait]
impl ProtocolHandler for EchoProtocol {
    fn proto_id(&self) -> ProtocolId {
        ECHO_PROTO_ID.into()
    }

    fn proto_name(&self) -> &'static str {
        "echo"
    }

    async fn handle(&self, mut stream: FramedStream) {
        if let Err(err) = Self::echo(&mut stream).await {
            error!("echo {}", err);
        }
    }
}

async fn make_xenovox<A: ToSocketAddrs>(
    addr: A,
    peer_store: PeerStore,
    bt_mode: bootstrap::Mode,
) -> Result<
    (
        QuicHost,
        PublicKey,
        Multiaddr,
        BootstrapProtocol,
        Receiver<bootstrap::Event>,
    ),
    Error,
> {
    let (sk, pk) = random_keypair();
    debug!("peer id {}", pk.peer_id());

    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let multiaddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    // Register our self
    let peer_info = PeerInfo::with_all(pk.clone(), Connectedness::Connected, multiaddr.clone());
    peer_store.register(peer_info).await;

    let (bt_evt_tx, bt_evt_rx) = channel(100);
    // TieDing
    let bt_x_proto = BootstrapProtocol::new(
        BOOTSTRAP_PROTO_ID.into(),
        pk.peer_id(),
        multiaddr.clone(),
        bt_mode,
        peer_store.clone(),
        bt_evt_tx,
    );

    let mut host = QuicHost::make(&sk, peer_store.clone())?;
    if bt_mode == bootstrap::Mode::Publisher {
        host.add_handler(Box::new(bt_x_proto.clone())).await?;
    }
    host.add_handler(Box::new(EchoProtocol)).await?;
    host.listen(multiaddr.clone()).await?;

    Ok((host, pk, multiaddr, bt_x_proto, bt_evt_rx))
}

#[tokio::test]
async fn test_bootstrap_protocol() -> Result<(), Error> {
    use bootstrap::Mode;

    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let geralt_store = PeerStore::default();
    let ciri_store = PeerStore::default();
    let yene_store = PeerStore::default();
    let msg = "watch 20-12-2019";

    let (_geralt_xenovox, geralt_pubkey, geralt_maddr, .., _geralt_bt_evt_rx) =
        make_xenovox(("127.0.0.1", 2077), geralt_store, Mode::Publisher).await?;
    let (ciri_xenovox, ciri_pubkey, _, ciri_bt, .., _ciri_bt_evt_rx) =
        make_xenovox(("127.0.0.1", 2020), ciri_store.clone(), Mode::Subscriber).await?;
    let (yennefer_xenovox, .., yene_bt, mut yene_bt_evt_rx) =
        make_xenovox(("127.0.0.1", 2021), yene_store.clone(), Mode::Subscriber).await?;

    // Register geralt info, he is bootstrap
    let geralt_peer_id = geralt_pubkey.peer_id();
    let geralt_info = PeerInfo::with_addr(geralt_peer_id.clone(), geralt_maddr);
    ciri_store.register(geralt_info.clone()).await;
    yene_store.register(geralt_info).await;

    let ciri_bt_stream = ciri_xenovox
        .new_stream(
            Context::new(),
            &geralt_peer_id,
            BootstrapProtocol::protocol(BOOTSTRAP_PROTO_ID.into()),
        )
        .await?;
    tokio::spawn(async move { ciri_bt.handle(ciri_bt_stream).await });

    let yene_bt_stream = yennefer_xenovox
        .new_stream(
            Context::new(),
            &geralt_peer_id,
            BootstrapProtocol::protocol(BOOTSTRAP_PROTO_ID.into()),
        )
        .await?;
    tokio::spawn(async move { yene_bt.handle(yene_bt_stream).await });

    // After new arrival, yene should have ciri info
    let event = yene_bt_evt_rx.next().await.ok_or(CommonError::NoMessage)?;
    debug!("event {}", event);

    let mut yene_stream = yennefer_xenovox
        .new_stream(
            Context::new(),
            &ciri_pubkey.peer_id(),
            EchoProtocol::proto(),
        )
        .await?;

    yene_stream.send(Bytes::from(msg)).await?;
    let echoed = yene_stream
        .try_next()
        .await?
        .ok_or(CommonError::NoMessage)?;

    assert_eq!(&echoed, &msg);

    Ok::<(), Error>(())
}

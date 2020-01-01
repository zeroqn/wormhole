mod common;
use common::{random_keypair, CommonError};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use futures::{
    channel::mpsc::{channel, Receiver},
    lock::BiLock,
    SinkExt, StreamExt, TryStreamExt,
};
use tracing::{debug, error};

use wormhole::{
    bootstrap::{BootstrapProtocol, Event as BtEvent},
    crypto::PublicKey,
    host::{DefaultHost, FramedStream, Host, ProtocolHandler},
    multiaddr::{Multiaddr, MultiaddrExt},
    network::{Connectedness, Protocol, ProtocolId},
    peer_store::{PeerInfo, PeerStore},
};

use std::net::ToSocketAddrs;

#[derive(Clone)]
pub struct EchoProtocol;

impl EchoProtocol {
    fn proto() -> Protocol {
        Protocol::new(1, "echo")
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
        2.into()
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
    is_server: bool,
) -> Result<(DefaultHost, PublicKey, Multiaddr, Receiver<BtEvent>), Error> {
    let (sk, pk) = random_keypair();

    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let maddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    let peer_info = PeerInfo::with_all(pk.clone(), Connectedness::CanConnect, maddr.clone());
    peer_store.register(peer_info).await;

    debug!("peer id {}", pk.peer_id());

    let (ev_tx, ev_rx) = channel(5);
    // TieDing
    let bt_x_proto = BootstrapProtocol::new(
        peer_store.clone(),
        pk.peer_id(),
        maddr.clone(),
        is_server,
        ev_tx,
    );

    let mut host = DefaultHost::make(&sk, peer_store.clone())?;
    host.add_handler(Box::new(bt_x_proto)).await?;
    host.add_handler(Box::new(EchoProtocol)).await?;
    host.listen(maddr.clone()).await?;

    Ok((host, pk, maddr, ev_rx))
}

#[tokio::test]
async fn test_bootstrap_protocol() -> Result<(), Error> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let peer_store = PeerStore::default();
    let msg = "watch 20-12-2019";

    let (_geralt_xenovox, geralt_pubkey, geralt_maddr, ..) =
        make_xenovox(("127.0.0.1", 2077), peer_store.clone(), true).await?;
    let (ciri_xenovox, ciri_pubkey, ciri_maddr, ..) =
        make_xenovox(("127.0.0.1", 2020), peer_store.clone(), false).await?;

    let (ev_tx, _ev_rx) = channel(10);
    let ciri_stream = ciri_xenovox
        .new_stream(
            Context::new(),
            &geralt_pubkey.peer_id(),
            BootstrapProtocol::protocol(),
        )
        .await?;
    let (w_stream, r_stream) = BiLock::new(ciri_stream);
    BootstrapProtocol::publish_ourself(
        Context::new(),
        ciri_pubkey.peer_id(),
        ciri_maddr.clone(),
        w_stream,
    )
    .await?;
    BootstrapProtocol::new_arrived(Context::new(), peer_store.clone(), r_stream, ev_tx).await?;

    let yene_store = PeerStore::default();
    let geralt_info = PeerInfo::with_addr(geralt_pubkey.peer_id(), geralt_maddr);
    yene_store.register(geralt_info).await;

    let (yennefer_xenovox, yene_pubkey, yene_maddr, ..) =
        make_xenovox(("127.0.0.1", 2021), yene_store.clone(), false).await?;

    let yene_stream = yennefer_xenovox
        .new_stream(
            Context::new(),
            &geralt_pubkey.peer_id(),
            BootstrapProtocol::protocol(),
        )
        .await?;

    let (ev_tx, mut ev_rx) = channel(10);
    let (w_stream, r_stream) = BiLock::new(yene_stream);
    BootstrapProtocol::publish_ourself(
        Context::new(),
        yene_pubkey.peer_id(),
        yene_maddr.clone(),
        w_stream,
    )
    .await?;
    BootstrapProtocol::new_arrived(Context::new(), yene_store.clone(), r_stream, ev_tx).await?;

    let event = ev_rx.next().await.ok_or(CommonError::NoMessage)?;
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

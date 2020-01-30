mod common;
use common::{random_keypair, CommonError};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use creep::Context;
use futures::{SinkExt, TryStreamExt};
use tracing::error;

use wormhole::{
    crypto::PublicKey,
    host::{FramedStream, Host, ProtocolHandler, QuicHost},
    multiaddr::{Multiaddr, MultiaddrExt},
    network::{Connectedness, Protocol, ProtocolId},
    peer_store::simple_store::{PeerInfo, SimplePeerStore},
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
        1.into()
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
    peer_store: SimplePeerStore,
) -> Result<(QuicHost, PublicKey), Error> {
    let (sk, pk) = random_keypair();

    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let maddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    let peer_info = PeerInfo::with_all(pk.clone(), Connectedness::CanConnect, maddr.clone());
    peer_store.register(peer_info).await;

    let mut host = QuicHost::make(&sk, peer_store.clone())?;
    host.add_handler(Box::new(EchoProtocol)).await?;
    host.listen(maddr).await?;

    Ok((host, pk))
}

#[tokio::test]
async fn test_default_host() -> Result<(), Error> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let peer_store = SimplePeerStore::default();
    let msg = "watch 20-12-2019";

    let (_geralt_xenovox, geralt_pubkey) =
        make_xenovox(("127.0.0.1", 2077), peer_store.clone()).await?;
    let (ciri_xenovox, ..) = make_xenovox(("127.0.0.1", 2020), peer_store.clone()).await?;

    let mut ciri_stream = ciri_xenovox
        .new_stream(
            Context::new(),
            &geralt_pubkey.peer_id(),
            EchoProtocol::proto(),
        )
        .await?;

    ciri_stream.send(Bytes::from(msg)).await?;
    let echoed = ciri_stream
        .try_next()
        .await?
        .ok_or(CommonError::NoMessage)?;

    assert_eq!(&echoed, &msg);

    Ok::<(), Error>(())
}

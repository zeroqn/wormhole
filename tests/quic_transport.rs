mod common;
use common::{CommonError, random_keypair};

use anyhow::Error;
use creep::Context;
use futures::channel::mpsc::unbounded;
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::stream::StreamExt;
use wormhole::{
    transport::{CapableConn, Listener, Transport, QuicTransport, QuicListener},
    multiaddr::{Multiaddr, MultiaddrExt},
    crypto::PublicKey,
};

use std::net::ToSocketAddrs;

pub async fn make_xenovox<A: ToSocketAddrs>(
    addr: A,
) -> Result<(QuicTransport, QuicListener, Multiaddr, PublicKey), Error> {
    let (sk, pk) = random_keypair();

    let mut xenovox = QuicTransport::make(&sk)?;
    let mut sock_addr = addr.to_socket_addrs()?;
    let sock_addr = sock_addr.next().ok_or(CommonError::NoSocketAddress)?;
    let maddr = Multiaddr::quic_peer(sock_addr, pk.peer_id());

    let listener = xenovox.listen(maddr.clone()).await?;
    Ok((xenovox, listener, maddr, pk))
}

#[tokio::test]
async fn should_able_to_communicate_with_remote_peer() -> Result<(), Error> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let msg = "watch 20-12-2019";
    let (msg_tx, mut msg_rx) = unbounded();

    let (_geralt_xenovox, mut geralt_listener, geralt_maddr, geralt_pubkey) =
        make_xenovox(("127.0.0.1", 2077)).await?;
    let (ciri_xenovox, ..) = make_xenovox(("0.0.0.0", 2020)).await?;

    tokio::spawn(async move {
        let conn_from_ciri = geralt_listener.accept().await?;
        let mut stream_from_ciri = conn_from_ciri.accept_stream().await?;

        let mut ciri_msg = String::new();
        stream_from_ciri.read_to_string(&mut ciri_msg).await?;

        msg_tx.unbounded_send(ciri_msg)?;
        Ok::<(), Error>(())
    });

    let ciri_conn = ciri_xenovox
        .dial(Context::new(), geralt_maddr, geralt_pubkey.peer_id())
        .await?;
    let mut ciri_stream = ciri_conn.open_stream().await?;

    ciri_stream.write_all(msg.as_bytes()).await?;
    ciri_stream.close().await?;
    assert_eq!(&msg_rx.next().await.ok_or(CommonError::NoMessage)?, &msg);

    Ok::<(), Error>(())
}

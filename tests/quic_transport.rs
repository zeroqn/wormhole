mod common;
use common::{make_xenovox, CommonError};

use creep::Context;
use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::channel::mpsc::unbounded;
use futures::stream::StreamExt;
use anyhow::Error;
use wormhole::transport::{Listener, CapableConn, Transport};

#[tokio::test]
async fn should_able_to_communicate_with_remote_peer() -> Result<(), Error> {
    env_logger::init();

    let msg = "watch 20-12-2019";
    let (msg_tx, mut msg_rx) = unbounded();

    let (_geralt_xenovox, mut geralt_listener, geralt_maddr, geralt_pubkey) = make_xenovox(("127.0.0.1", 2077)).await?;
    let (ciri_xenovox, ..) = make_xenovox(("0.0.0.0", 2020)).await?;

    tokio::spawn(async move {
        let conn_from_ciri = geralt_listener.accept().await?;
        let mut stream_from_ciri = conn_from_ciri.accept_stream().await?;

        let mut ciri_msg = String::new();
        stream_from_ciri.read_to_string(&mut ciri_msg).await?;

        msg_tx.unbounded_send(ciri_msg)?;
        Ok::<(), Error>(())
    });

    let ciri_conn = ciri_xenovox.dial(Context::new(), geralt_maddr, geralt_pubkey.peer_id()).await?;
    let mut ciri_stream = ciri_conn.open_stream().await?;

    ciri_stream.write_all(msg.as_bytes()).await?;
    ciri_stream.close().await?;
    assert_eq!(&msg_rx.next().await.ok_or(CommonError::NoMessage)?, &msg);

    Ok::<(), Error>(())
}

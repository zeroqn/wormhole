use super::{Direction, ProtocolId, QuicConn};
use crate::{network, transport};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{
    io,
    prelude::{AsyncRead, AsyncWrite},
};

use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub struct QuicStream {
    inner: transport::QuicMuxedStream,

    proto: Option<ProtocolId>,
    direction: Direction,
    conn: QuicConn,
}

impl AsyncRead for QuicStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().inner), cx, buf)
    }
}

impl AsyncWrite for QuicStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().inner), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().inner), cx)
    }
}

// TODO: should be decoded protocol message bytes
impl futures::stream::Stream for QuicStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

#[async_trait]
impl transport::MuxedStream for QuicStream {
    async fn close(&mut self) -> Result<(), Error> {
        self.inner.close().await
    }

    fn reset(&mut self) {
        self.inner.reset()
    }
}

impl network::Stream for QuicStream {
    type Conn = QuicConn;

    fn protocol(&self) -> Option<ProtocolId> {
        self.proto.clone()
    }

    fn set_protocol(&mut self, id: ProtocolId) {
        self.proto = Some(id);
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn conn(&self) -> Self::Conn {
        self.conn.clone()
    }
}

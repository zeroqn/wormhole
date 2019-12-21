use super::{Direction, ProtocolId, QuicConn};
use crate::{
    network,
    transport::{self, MuxedStream},
};

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{
    io,
    lock::Mutex,
    pin_mut,
    prelude::{AsyncRead, AsyncWrite, Future},
    ready,
};

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

#[derive(Clone)]
pub struct QuicStream {
    inner: Arc<Mutex<transport::QuicMuxedStream>>,
    proto: Option<ProtocolId>,
    direction: Direction,
    conn: QuicConn,
}

impl QuicStream {
    pub fn new(
        muxed_stream: transport::QuicMuxedStream,
        direction: Direction,
        conn: QuicConn,
    ) -> Self {
        QuicStream {
            inner: Arc::new(Mutex::new(muxed_stream)),
            proto: None,
            direction,
            conn,
        }
    }
}

impl AsyncRead for QuicStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut().inner.lock();
        pin_mut!(inner);

        let mut stream = ready!(inner.poll(cx));

        AsyncRead::poll_read(Pin::new(&mut *stream), cx, buf)
    }
}

impl AsyncWrite for QuicStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut().inner.lock();
        pin_mut!(inner);

        let mut stream = ready!(inner.poll(cx));

        AsyncWrite::poll_write(Pin::new(&mut *stream), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let inner = self.get_mut().inner.lock();
        pin_mut!(inner);

        let mut stream = ready!(inner.poll(cx));

        AsyncWrite::poll_flush(Pin::new(&mut *stream), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let inner = self.get_mut().inner.lock();
        pin_mut!(inner);

        let mut stream = ready!(inner.poll(cx));

        AsyncWrite::poll_close(Pin::new(&mut *stream), cx)
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

    async fn close(&mut self) -> Result<(), Error> {
        self.inner.lock().await.close().await
    }

    async fn reset(&mut self) {
        self.inner.lock().await.reset()
    }
}

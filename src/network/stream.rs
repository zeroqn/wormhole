use super::{Conn, Direction, NetworkConn, Protocol, Stream};
use crate::transport::{ConnSecurity, MuxedStream};

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
use tracing::debug;

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

#[derive(Clone)]
pub struct NetworkStream {
    inner: Arc<Mutex<Box<dyn MuxedStream>>>,
    proto: Option<Protocol>,
    direction: Direction,
    conn: NetworkConn,
}

impl NetworkStream {
    pub fn new(
        muxed_stream: impl MuxedStream + 'static,
        direction: Direction,
        conn: NetworkConn,
    ) -> Self {
        let muxed_stream: Box<dyn MuxedStream> = Box::new(muxed_stream);

        NetworkStream {
            inner: Arc::new(Mutex::new(muxed_stream)),
            proto: None,
            direction,
            conn,
        }
    }
}

macro_rules! stream_poll_ready {
    ($stream:expr, $cx:expr) => {{
        let inner = $stream.inner.lock();
        pin_mut!(inner);

        ready!(inner.poll($cx))
    }};
}

impl AsyncRead for NetworkStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let stream = self.get_mut();
        let mut muxed_stream = stream_poll_ready!(stream, cx);
        debug!(
            "poll_read on peer stream {} using proto {:?}",
            stream.conn.remote_peer(),
            stream.proto
        );

        AsyncRead::poll_read(Pin::new(&mut *muxed_stream), cx, buf)
    }
}

impl AsyncWrite for NetworkStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let stream = self.get_mut();
        let mut muxed_stream = stream_poll_ready!(stream, cx);
        debug!(
            "poll_write {} bytes to peer stream {} using proto {:?}",
            buf.len(),
            stream.conn.remote_peer(),
            stream.proto
        );

        AsyncWrite::poll_write(Pin::new(&mut *muxed_stream), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let stream = self.get_mut();
        let mut muxed_stream = stream_poll_ready!(stream, cx);
        debug!(
            "poll_flush to peer stream {} using proto {:?}",
            stream.conn.remote_peer(),
            stream.proto
        );

        AsyncWrite::poll_flush(Pin::new(&mut *muxed_stream), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let stream = self.get_mut();
        let mut muxed_stream = stream_poll_ready!(stream, cx);
        debug!(
            "poll close to peer stream {} using proto {:?}",
            stream.conn.remote_peer(),
            stream.proto
        );

        AsyncWrite::poll_close(Pin::new(&mut *muxed_stream), cx)
    }
}

// TODO: should be decoded protocol message bytes
impl futures::stream::Stream for NetworkStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

#[async_trait]
impl Stream for NetworkStream {
    fn protocol(&self) -> Option<Protocol> {
        self.proto.clone()
    }

    fn set_protocol(&mut self, proto: Protocol) {
        debug!(
            "set peer stream {} to proto {}",
            self.conn.remote_peer(),
            proto
        );
        self.proto = Some(proto);
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn conn(&self) -> Box<dyn Conn> {
        dyn_clone::clone_box(&self.conn)
    }

    async fn close(&mut self) -> Result<(), Error> {
        debug!("close peer stream {}", self.conn.remote_peer());
        self.inner.lock().await.close().await
    }

    async fn reset(&mut self) {
        debug!("reset peer stream {}", self.conn.remote_peer());
        self.inner.lock().await.reset()
    }
}

use super::{super::traits::MuxedStream, RESET_ERR_CODE};

use async_trait::async_trait;
use bytes::Bytes;
use derive_more::Constructor;
use futures::{
    io,
    prelude::{AsyncRead, AsyncWrite, Stream},
};
use quinn::{RecvStream, SendStream};
use tracing::debug;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Constructor)]
pub struct QuicMuxedStream {
    read: RecvStream,
    send: SendStream,
}

impl AsyncRead for QuicMuxedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().read), cx, buf)
    }
}

impl AsyncWrite for QuicMuxedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}

// TODO: decode polled bytes with tokio LengthDelimitedCodec
impl Stream for QuicMuxedStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<Self::Item>> {
        unimplemented!()
    }
}

#[async_trait]
impl MuxedStream for QuicMuxedStream {
    async fn close(&mut self) -> Result<(), anyhow::Error> {
        debug!("close send stream");
        Ok(self.send.finish().await?)
    }

    fn reset(&mut self) {
        self.send.reset(RESET_ERR_CODE.into());
        if let Err(_unknown_stream) = self.read.stop(RESET_ERR_CODE.into()) {
            debug!("muxed stream: reset got unknown stream");
        }
    }
}

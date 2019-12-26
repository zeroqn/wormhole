use super::RawStream;

use futures::{
    Sink, Stream,
};
use futures_codec::{Framed, LengthCodec, Encoder};

use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub struct FramedStream<S: RawStream> {
    inner: Framed<S, LengthCodec>,
}

impl<S: RawStream> FramedStream<S> {
    pub fn new(stream: S) -> Self {
        FramedStream {
            inner: Framed::new(stream, LengthCodec),
        }
    }

    pub async fn reset(self) {
        let (mut stream, _codec) = self.inner.release();

        stream.reset().await;
    }
}

impl<S: RawStream> Stream for FramedStream<S> {
    type Item = <Framed<S, LengthCodec> as Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.get_mut().inner), cx)
    }
}

impl<S: RawStream> Sink<<LengthCodec as Encoder>::Item> for FramedStream<S> {
    type Error = <LengthCodec as Encoder>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn start_send(self: Pin<&mut Self>, item: <LengthCodec as Encoder>::Item) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.get_mut().inner), item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.get_mut().inner), cx)
    }
}

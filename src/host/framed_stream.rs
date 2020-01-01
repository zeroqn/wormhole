use crate::network;

use futures::{Sink, Stream};
use futures_codec::{Encoder, Framed, LengthCodec};

use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub struct FramedStream {
    inner: Framed<Box<dyn network::Stream>, LengthCodec>,
}

impl FramedStream {
    pub fn new(stream: Box<dyn network::Stream>) -> Self {
        FramedStream {
            inner: Framed::new(stream, LengthCodec),
        }
    }

    pub async fn reset(self) {
        let (mut stream, _codec) = self.inner.release();

        stream.reset().await;
    }
}

impl Stream for FramedStream {
    type Item = <Framed<Box<dyn network::Stream>, LengthCodec> as Stream>::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.get_mut().inner), cx)
    }
}

impl Sink<<LengthCodec as Encoder>::Item> for FramedStream {
    type Error = <LengthCodec as Encoder>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn start_send(
        self: Pin<&mut Self>,
        item: <LengthCodec as Encoder>::Item,
    ) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.get_mut().inner), item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.get_mut().inner), cx)
    }
}

pub mod conn;
pub mod conn_pool;
pub mod dialer;
pub mod quic_impl;
pub mod stream;
pub mod traits;

pub use quic_impl::QuicNetwork;
pub use traits::{Conn, Dialer, Network, RemoteConnHandler, RemoteStreamHandler, Stream};

use conn::NetworkConn;
use conn_pool::NetworkConnPool;
use dialer::NetworkDialer;
use stream::NetworkStream;

use crate::multiaddr::Multiaddr;

use derive_more::Display;

use std::{fmt, ops::Deref};

pub enum NetworkEvent {
    Listen(Box<dyn Network>, Multiaddr),
    ListenClose(Box<dyn Network>, Multiaddr),
    Connected(Box<dyn Network>, Box<dyn Conn>),
    Disconnected(Box<dyn Network>, Box<dyn Conn>),
    OpenedStream(Box<dyn Network>, Box<dyn Stream>),
    ClosedStream(Box<dyn Network>, Box<dyn Stream>),
}

const CONNECTEDNESS_MASK: usize = 0b1110;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Display)]
#[repr(usize)]
pub enum Connectedness {
    #[display(fmt = "not connected")]
    NotConnected = 0 << 1,

    #[display(fmt = "can connect")]
    CanConnect = 1 << 1,

    #[display(fmt = "connected")]
    Connected = 2 << 1,

    #[display(fmt = "unconnectable")]
    CannotConnect = 3 << 1,

    #[display(fmt = "connecting")]
    Connecting = 4 << 1,
}

impl From<usize> for Connectedness {
    fn from(src: usize) -> Connectedness {
        use self::Connectedness::*;

        debug_assert!(
            src == NotConnected as usize
                || src == CanConnect as usize
                || src == Connected as usize
                || src == CannotConnect as usize
                || src == Connecting as usize
        );

        unsafe { ::std::mem::transmute(src) }
    }
}

impl From<Connectedness> for usize {
    fn from(src: Connectedness) -> usize {
        let v = src as usize;
        debug_assert!(v & CONNECTEDNESS_MASK == v);
        v
    }
}

#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]
pub enum Direction {
    #[display(fmt = "inbound")]
    Inbound,
    #[display(fmt = "outbound")]
    Outbound,
}

#[derive(Display, PartialEq, Eq, Hash, Clone, Copy)]
#[display(fmt = "{}", _0)]
pub struct ProtocolId(u64);

impl Deref for ProtocolId {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Into<ProtocolId> for u64 {
    fn into(self) -> ProtocolId {
        ProtocolId(self)
    }
}

#[derive(Display, PartialEq, Eq, Hash, Clone, Copy)]
#[display(fmt = "protocol {} => {}", id, name)]
pub struct Protocol {
    pub(crate) id: ProtocolId,
    pub(crate) name: &'static str,
}

impl Protocol {
    pub fn new(id: u64, name: &'static str) -> Self {
        Protocol {
            id: id.into(),
            name,
        }
    }
}

impl fmt::Debug for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

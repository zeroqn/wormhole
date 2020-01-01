pub mod conn;
pub mod conn_pool;
pub mod dialer;
pub mod r#impl;
pub mod stream;
pub mod traits;

pub use r#impl::QuicNetwork;
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

#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]
pub enum Connectedness {
    #[display(fmt = "not connected before")]
    NotConnected,
    #[display(fmt = "connected")]
    Connected,
    #[display(fmt = "can connect")]
    CanConnect,
    #[display(fmt = "unable to connect")]
    CannotConnect,
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

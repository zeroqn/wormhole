pub mod quic;
pub mod traits;

pub use quic::config::QuicConfig;
pub use quic::transport::QuicTransport;
pub use traits::{CapableConn, ConnMultiaddr, ConnSecurity, Listener, MuxedStream, Transport};

pub const RESET_ERR_CODE: u32 = 0;

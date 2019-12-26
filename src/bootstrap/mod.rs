use crate::{
    crypto::PeerId,
    host::{FramedStream, ProtocolHandler},
    multiaddr::Multiaddr,
    peer_store::PeerStore,
    network::ProtocolId
};

use tokio::time::delay_for;
use tracing::{error, warn};
use async_trait::async_trait;
use bytes::BytesMut;
use futures::SinkExt;
use prost::Message;

use std::time::Duration;

#[derive(Message)]
struct PeerInfo {
    #[prost(bytes, tag = "1")]
    peer_id: Vec<u8>,
    #[prost(bytes, tag = "2")]
    multiaddr: Vec<u8>,
}

impl PeerInfo {
    pub fn new(peer_addr: (PeerId, Multiaddr)) -> Self {
        PeerInfo {
            peer_id: peer_addr.0.into_inner().to_vec(),
            multiaddr: peer_addr.1.to_vec()
        }
    }
}

#[derive(Message)]
struct Peers {
    #[prost(message, repeated, tag = "1")]
    data: Vec<PeerInfo>,
}

#[derive(Clone)]
pub struct BootstrapProtocol {
    peer_store: PeerStore,
}

#[async_trait]
impl ProtocolHandler for BootstrapProtocol {
    fn proto_id(&self) -> ProtocolId {
        1.into()
    }

    fn proto_name(&self) -> &'static str {
        "bootstrap"
    }

    async fn handle(&self, stream: &mut FramedStream) {
        loop {
            loop {
                let peer_infos = self.peer_store.take(40).await.into_iter().map(PeerInfo::new).collect();

                let peers = Peers {
                    data: peer_infos
                };
                let mut peers_data = BytesMut::new();
                match peers.encode(&mut peers_data) {
                    Err(e) => {
                        error!("encode peers {}", e);
                        break;
                    }
                    Ok(p) => p,
                };

                if let Err(e) = stream.send(peers_data.freeze()).await {
                    warn!("send peers {}", e);
                }

                break;
            }

            delay_for(Duration::from_secs(40)).await;
        }
    }
}

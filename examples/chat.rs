use async_trait::async_trait;
use creep::Context;
use structopt::{self, StructOpt};
use wormhole::{
    multiaddr::{Multiaddr, MultiaddrExt},
    crypto::{PrivateKey, PublicKey, PeerId},
    host::{ProtocolHandler, FramedStream, QuicHost, Host},
    network::{Protocol, ProtocolId, Connectedness},
    peer_store::{PeerStore, PeerInfo},
};
use futures::{TryStreamExt, SinkExt};
use anyhow::Result as AnyResult;
use tokio::{sync::broadcast, io::{stdin, BufReader, AsyncBufReadExt}};
use tracing::{error, debug, warn};

use std::net::SocketAddr;

#[derive(StructOpt, Debug)]
#[structopt(name = "chat")]
struct Opt {
    /// Remote peer address
    #[structopt(short = "a", long)]
    remote_addr: Option<SocketAddr>,

    /// Remote nick name
    #[structopt(short = "r", long)]
    remote_nickname: Option<String>,

    /// Our listen address
    #[structopt(short = "l", long)]
    listen: SocketAddr,

    /// Our nickname
    #[structopt(short = "n", long, default_value = "bot")]
    nickname: String,
}

#[derive(Debug)]
struct Persona {
    nickname: String,
    pubkey: PublicKey,
    peer_id: PeerId,
    multiaddr: Multiaddr,
}

impl Persona {
    fn new(nickname: String, sockaddr: SocketAddr) -> Self {
        let pubkey = Self::make_privkey_from_nickname(&nickname).pubkey();
        let peer_id = pubkey.peer_id();
        let multiaddr = Multiaddr::quic_peer(sockaddr, peer_id.clone());

        Persona {
            nickname,
            pubkey,
            peer_id,
            multiaddr
        }
    }

    fn to_info(&self, connectedness: Connectedness) -> PeerInfo {
        PeerInfo::with_all(self.pubkey.clone(), connectedness, self.multiaddr.clone())
    }

    fn make_privkey_from_nickname(nickname: &str) -> PrivateKey {
        use ophelia::Hasher;
        use ophelia_hasher_keccak256::Keccak256;

        let hashed = Keccak256.digest(nickname.as_bytes()).to_bytes();
        PrivateKey::from_slice(&hashed).expect("make private key")
    }
}

#[derive(Clone)]
struct ChatProtocol {
    local_nickname: String,
    local_msg_subscriber: broadcast::Sender<String>,
}

impl ChatProtocol {
    fn new(local_nickname: String, local_msg_subscriber: broadcast::Sender<String>) -> Self {
        ChatProtocol {
            local_nickname,
            local_msg_subscriber,
        }
    }

    fn protocol() -> Protocol {
        Protocol::new(77, "chat")
    }
}

#[async_trait]
impl ProtocolHandler for ChatProtocol {
    fn proto_id(&self) -> ProtocolId {
        77.into()
    }

    fn proto_name(&self) -> &'static str {
        "chat"
    }

    async fn handle(&self, stream: FramedStream) {
        let peer_id = stream.conn().remote_peer();
        let mut local_msg_rx = self.local_msg_subscriber.subscribe();

        let (mut w_stream, mut r_stream) = (stream.clone(), stream);

        // Println remote peer's messages
        let peer_id_cloned = peer_id.clone();
        tokio::spawn(async move {
            loop {
                match r_stream.try_next().await {
                    Ok(Some(bytes)) => {
                        if let Ok(msg) = String::from_utf8(bytes.to_vec()) {
                            println!("{}", msg);
                        }
                    }
                    Ok(None) => {
                        debug!("remote {} stream closed", peer_id_cloned);
                        break;
                    }
                    Err(err) => {
                        error!("read inbound stream: {}", err);
                        break;
                    }
                }
            }
        });

        // Deliver our messages to peer
        loop {
            match local_msg_rx.recv().await {
                Ok(msg) => {
                    let msg = format!("{}: {}", self.local_nickname, msg);
                    debug!("got local msg");

                    if let Err(err) = w_stream.send(msg.into()).await {
                        error!("send local message {}", err);
                        break;
                    }
                }
                Err(err) => match err {
                    broadcast::RecvError::Closed => {
                        debug!("local broadcast closed");
                        break;
                    }
                    broadcast::RecvError::Lagged(n) => {
                        warn!("local broadcast to {} lagged {}", peer_id, n);
                    }
                }
            }
        }
    }
}

#[tokio::main]
pub async fn main() -> AnyResult<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )?;

    let opt = Opt::from_args();
    debug!("opt: {:?}", opt);

    // Broadcast channel to all connected peers
    let (local_msg_tx, _local_msg_rx) = broadcast::channel(10);

    let our = Persona::new(opt.nickname, opt.listen);
    let remote = if let Some(remote_addr) = opt.remote_addr {
        let remote_nickname = opt.remote_nickname.expect("remote nickname");
        Some(Persona::new(remote_nickname, remote_addr))
    } else {
        None
    };

    let local_msg_subscriber = local_msg_tx.clone();
    tokio::spawn(async move {
        if let Err(err) = run_client(our, remote, local_msg_subscriber).await {
            error!("run client {}", err);
        }
    });

    let mut stdin = BufReader::new(stdin());
    loop {
        let mut msg = String::new();
        stdin.read_line(&mut msg).await?;
        debug!("msg {}", msg);
        
        if let Err(err) = local_msg_tx.send(msg) {
            error!("impossible, we still hold a _local_msg_rx, {:?}", err);
            break;
        }
    }

    Ok(())
}

async fn run_client(our: Persona, remote: Option<Persona>, local_msg_subscriber: broadcast::Sender<String>) -> AnyResult<()> {
    debug!("our peer id {}", our.peer_id);

    let peer_store = PeerStore::default();
    peer_store.register(our.to_info(Connectedness::Connected)).await;
    if let Some(ref remote) = remote {
        peer_store.register(remote.to_info(Connectedness::CanConnect)).await;
    }

    let mut host = QuicHost::make(&Persona::make_privkey_from_nickname(&our.nickname), peer_store.clone())?;
    let chat_proto = ChatProtocol::new(our.nickname.clone(), local_msg_subscriber.clone());

    host.add_handler(Box::new(chat_proto.clone())).await?;
    host.listen(our.multiaddr.clone()).await?;

    if let Some(remote) = remote {
        match host.new_stream(Context::new(), &remote.peer_id, ChatProtocol::protocol()).await {
            Ok(chat_stream) => chat_proto.clone().handle(chat_stream).await,
            Err(err) => warn!("create new chat stream {}", err),
        }
    }

    Ok(())
}

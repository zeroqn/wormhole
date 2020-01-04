use anyhow::Result as AnyResult;
use async_trait::async_trait;
use creep::Context;
use futures::{channel::mpsc, SinkExt, StreamExt, TryStreamExt};
use structopt::{self, StructOpt};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    sync::broadcast,
};
use tracing::{debug, error, warn};
use wormhole::{
    bootstrap::{self, BootstrapProtocol},
    crypto::{PeerId, PrivateKey, PublicKey},
    host::{FramedStream, Host, ProtocolHandler, QuicHost},
    multiaddr::{Multiaddr, MultiaddrExt},
    network::{Connectedness, Protocol, ProtocolId},
    peer_store::{PeerInfo, PeerStore},
};

use std::net::SocketAddr;

const BOOTSTRAP_NICKNAME: &'static str = "bot";
const BOOTSTRAP_PROTO_ID: u64 = 1;
const GROUP_CHAT_PROTO_ID: u64 = 77;
const GROUP_CHAT_NAME: &str = "group-chat/1.0";

/// Basic Usage:
///
/// Bootstrap:
/// cargo run --example group_chat -- -l 127.0.0.1:2077
///
/// Ciri:
/// cargo run --example group_chat -- -b 127.0.0.1:2077 -l 127.0.0.1:2020 -n ciri
///
/// Triss:
/// cargo run --example group_chat -- -b 127.0.0.1:2077 -l 127.0.0.1:2021 -n triss
#[derive(StructOpt, Debug)]
#[structopt(name = "group-chat")]
struct Opt {
    /// Remote bootstrap address
    #[structopt(short = "b", long)]
    bootstrap_addr: Option<SocketAddr>,

    /// Our listen address
    #[structopt(short = "l", long)]
    listen: SocketAddr,

    /// Our nickname
    #[structopt(short = "n", long, default_value = "minion from despicable me")]
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
            multiaddr,
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
struct GroupChatProtocol {
    local_nickname: String,
    local_msg_subscriber: broadcast::Sender<String>,
}

impl GroupChatProtocol {
    fn new(local_nickname: String, local_msg_subscriber: broadcast::Sender<String>) -> Self {
        GroupChatProtocol {
            local_nickname,
            local_msg_subscriber,
        }
    }

    fn protocol() -> Protocol {
        Protocol::new(GROUP_CHAT_PROTO_ID, GROUP_CHAT_NAME)
    }
}

#[async_trait]
impl ProtocolHandler for GroupChatProtocol {
    fn proto_id(&self) -> ProtocolId {
        GROUP_CHAT_PROTO_ID.into()
    }

    fn proto_name(&self) -> &'static str {
        GROUP_CHAT_NAME
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
                },
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

    // Bootstrap node always use hardcode name, so that we don't need
    // to pass bootstrap peer id and it's multiaddr from command line.
    let nickname = if opt.bootstrap_addr.is_none() {
        BOOTSTRAP_NICKNAME.to_owned()
    } else {
        opt.nickname
    };

    let our = Persona::new(nickname, opt.listen);
    let bt = if let Some(bt_addr) = opt.bootstrap_addr {
        Some(Persona::new(BOOTSTRAP_NICKNAME.to_owned(), bt_addr))
    } else {
        None
    };

    let local_msg_subscriber = local_msg_tx.clone();
    tokio::spawn(async move {
        if let Err(err) = run_client(our, bt, local_msg_subscriber).await {
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

async fn run_client(
    our: Persona,
    bootstrap: Option<Persona>,
    local_msg_subscriber: broadcast::Sender<String>,
) -> AnyResult<()> {
    debug!("our peer id {}", our.peer_id);

    let peer_store = PeerStore::default();
    peer_store
        .register(our.to_info(Connectedness::Connected))
        .await;
    if let Some(ref bt) = bootstrap {
        peer_store
            .register(bt.to_info(Connectedness::CanConnect))
            .await;
    }

    let mut host = QuicHost::make(
        &Persona::make_privkey_from_nickname(&our.nickname),
        peer_store.clone(),
    )?;
    let group_chat_proto =
        GroupChatProtocol::new(our.nickname.clone(), local_msg_subscriber.clone());

    let (bt_evt_tx, mut bt_evt_rx) = mpsc::channel(10);
    let bt_mode = if bootstrap.is_none() {
        bootstrap::Mode::Publisher
    } else {
        bootstrap::Mode::Subscriber
    };
    let bt_proto = BootstrapProtocol::new(
        BOOTSTRAP_PROTO_ID.into(),
        our.peer_id.clone(),
        our.multiaddr.clone(),
        bt_mode,
        peer_store.clone(),
        bt_evt_tx,
    );

    host.add_handler(Box::new(group_chat_proto.clone())).await?;
    host.listen(our.multiaddr.clone()).await?;

    if bt_mode == bootstrap::Mode::Publisher {
        host.add_handler(Box::new(bt_proto)).await?;

        // Drive bootstrap arrival
        tokio::spawn(async move {
            loop {
                if let None = bt_evt_rx.next().await {
                    debug!("bootstrap: no more new peers");
                    break;
                }
            }
        });
        return Ok(());
    }

    let bt_peer_id = bootstrap
        .expect("subscriber should have bootstrap info")
        .peer_id;
    let bt_stream = host
        .new_stream(
            Context::new(),
            &bt_peer_id,
            BootstrapProtocol::protocol(BOOTSTRAP_PROTO_ID.into()),
        )
        .await?;
    tokio::spawn(async move { bt_proto.handle(bt_stream).await });

    if let Ok(bt_group_chat) = host
        .new_stream(Context::new(), &bt_peer_id, GroupChatProtocol::protocol())
        .await
    {
        let group_chat_proto = group_chat_proto.clone();
        tokio::spawn(async move { group_chat_proto.handle(bt_group_chat).await });
    }

    loop {
        if let None = bt_evt_rx.next().await {
            debug!("no more new peers");
            break;
        }

        let peers = peer_store.choose(40).await;
        for (peer_id, _multiaddr) in peers.into_iter() {
            if peer_id != our.peer_id
                && peer_id != bt_peer_id
                && peer_store.get_connectedness(&peer_id).await == Connectedness::CanConnect
            {
                match host
                    .new_stream(Context::new(), &peer_id, GroupChatProtocol::protocol())
                    .await
                {
                    Ok(group_chat) => {
                        let group_chat_proto = group_chat_proto.clone();
                        tokio::spawn(async move { group_chat_proto.handle(group_chat).await });
                    }
                    Err(err) => warn!("create new group chat to peer {} {}", peer_id, err),
                }
            }
        }
    }

    Ok(())
}
